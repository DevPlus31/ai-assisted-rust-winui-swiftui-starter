# Optional Microsoft Store pre-flight checks on a built package.
#
# Usage (after building and running scripts/package-msix.ps1):
#   pwsh scripts/store-check-windows.ps1 -Arch x64 [-RunWack]
#
# Checks what a machine can check before Store certification: the exe's
# imports (no networking DLLs, static CRT) and exploit mitigations (ASLR,
# DEP, Control Flow Guard, CET on x64), and the package manifest's
# capabilities and dependencies. -RunWack also runs the Windows App
# Certification Kit, which installs the package (needs admin, the Windows
# App SDK runtime and a trusted signature, all set up here for CI).
# Exit code 1 when a check fails; warnings don't fail.
param(
    [ValidateSet('x64', 'arm64')] [string] $Arch = 'x64',
    [string] $Target = $(if ($Arch -eq 'x64') { 'x86_64-pc-windows-msvc' } else { 'aarch64-pc-windows-msvc' }),
    [switch] $RunWack
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$exe = Join-Path $root "target/$Target/release/app-starter.exe"
$manifestPath = Join-Path $root "target/msix-$Arch/AppxManifest.xml"
$script:failures = 0

function Ok([string] $m) { Write-Output "  ✓ $m" }
function Warn([string] $m) { Write-Output "  ! $m" }
function Fail([string] $m) { Write-Output "  ✗ $m"; $script:failures++ }

function Find-Dumpbin {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
    $found = & $vswhere -latest -products * -find 'VC/Tools/MSVC/**/bin/Hostx64/x64/dumpbin.exe' | Select-Object -First 1
    if (-not $found) { throw 'dumpbin.exe not found; install the Visual Studio C++ build tools' }
    $found
}

foreach ($path in $exe, $manifestPath) {
    if (-not (Test-Path $path)) { throw "$path not found; build and package first (see the header)" }
}
$dumpbin = Find-Dumpbin
Write-Output "Microsoft Store pre-flight: app-starter.exe ($Arch)`n"

# --- Imports -------------------------------------------------------------
Write-Output 'Imports'
$dlls = & $dumpbin /nologo /imports $exe |
    Select-String -Pattern '^\s{4}(\S+\.dll)\s*$' |
    ForEach-Object { $_.Matches[0].Groups[1].Value.ToLowerInvariant() } |
    Sort-Object -Unique
$network = @('ws2_32.dll', 'wsock32.dll', 'mswsock.dll', 'winhttp.dll', 'wininet.dll', 'urlmon.dll',
    'websocket.dll', 'webio.dll', 'httpapi.dll', 'dnsapi.dll')
$networkHits = $dlls | Where-Object { $network -contains $_ }
if ($networkHits) { Fail "imports networking DLLs: $($networkHits -join ', ')" }
else { Ok 'no networking DLLs' }
$crt = $dlls | Where-Object { $_ -match '^(vcruntime|msvcp)\d+' -or $_ -like 'api-ms-win-crt-*' }
if ($crt) { Fail "depends on the shared C runtime ($($crt -join ', ')); keep +crt-static from .cargo/config.toml" }
else { Ok 'C runtime linked statically (no VCLibs dependency)' }
Write-Output "    imports: $($dlls -join ', ')"

# --- Exploit mitigations ---------------------------------------------------
Write-Output 'Exploit mitigations'
$headers = (& $dumpbin /nologo /headers $exe) -join "`n"
$mitigations = [ordered]@{
    'Dynamic base' = 'ASLR (dynamic base)'
    'High Entropy Virtual Addresses' = 'high-entropy ASLR'
    'NX compatible' = 'DEP (NX compatible)'
    'Guard' = 'Control Flow Guard'
}
foreach ($flag in $mitigations.Keys) {
    if ($headers -match [regex]::Escape($flag)) { Ok $mitigations[$flag] }
    else { Fail "$($mitigations[$flag]) is off" }
}
if ($Arch -eq 'x64') {
    if ($headers -match 'CET compatible') { Ok 'CET shadow stack compatible' }
    else { Fail 'not CET compatible; keep -C link-arg=/CETCOMPAT in .cargo/config.toml' }
}

# --- Package manifest --------------------------------------------------------
Write-Output 'Package manifest'
[xml] $manifest = Get-Content $manifestPath
$identity = $manifest.Package.Identity
if ($identity.ProcessorArchitecture -eq $Arch) { Ok "architecture $Arch" }
else { Fail "manifest architecture is $($identity.ProcessorArchitecture), expected $Arch" }
if ($identity.Version -match '^\d+\.\d+\.\d+\.0$') { Ok "version $($identity.Version)" }
else { Fail "version $($identity.Version) must be like 1.2.3.0 (the last part is reserved for the Store)" }
$capabilities = @($manifest.Package.Capabilities.ChildNodes | ForEach-Object { $_.Name })
$extra = $capabilities | Where-Object { $_ -ne 'runFullTrust' }
if ($extra) { Fail "capabilities beyond runFullTrust: $($extra -join ', ')" }
else { Ok 'only the runFullTrust capability (no internet or device access)' }
$runtime = $manifest.Package.Dependencies.PackageDependency | Where-Object { $_.Name -like 'Microsoft.WindowsAppRuntime.*' }
if ($runtime) { Ok "depends on $($runtime.Name) $($runtime.MinVersion)" }
else { Fail 'no PackageDependency on Microsoft.WindowsAppRuntime (WinUI 3 needs it)' }
if ((Get-Content $manifestPath -Raw) -match 'REPLACE_') {
    Warn 'placeholder identity (REPLACE_*): set the values from Partner Center before submitting'
}

# --- Windows App Certification Kit ------------------------------------------
if ($RunWack) {
    Write-Output 'Windows App Certification Kit'
    & (Join-Path $PSScriptRoot 'run-wack.ps1') -Arch $Arch
    if ($LASTEXITCODE -eq 0) { Ok 'WACK passed' } else { Fail 'WACK reported failures (see the report above)' }
}

Write-Output ''
if ($script:failures -gt 0) {
    Write-Output "$script:failures check(s) failed."
    exit 1
}
Write-Output 'All checks passed. Store certification can still fail for reasons no tool can check.'
