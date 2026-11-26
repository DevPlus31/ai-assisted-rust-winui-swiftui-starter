# Packages the Windows sample shell into an MSIX in dist\.
#
# Usage (after `cargo build -p windows-app --release --target <target>`):
#   pwsh scripts/package-msix.ps1 -Target x86_64-pc-windows-msvc -Arch x64 -Version 1.2.0
#
# Signing is optional: the Microsoft Store re-signs submitted packages. To
# sign for sideloading, set WINDOWS_CERTIFICATE (a base64 .pfx whose subject
# matches Identity/@Publisher in the manifest) and WINDOWS_CERTIFICATE_PASSWORD.
param(
    [Parameter(Mandatory)] [string] $Target,
    [Parameter(Mandatory)] [ValidateSet('x64', 'arm64')] [string] $Arch,
    [string] $Version = '1.0.0'
)
$ErrorActionPreference = 'Stop'

if ($Version -notmatch '^\d+\.\d+\.\d+$') { throw "Version must look like 1.2.3, got '$Version'" }
$root = Split-Path -Parent $PSScriptRoot
$packaging = Join-Path $root 'examples/windows-app/packaging'
$stage = Join-Path $root "target/msix-$Arch"
$dist = Join-Path $root 'dist'
$msix = Join-Path $dist "AppStarter-$Version-$Arch.msix"

# Stage the package layout: exe, manifest, logos, translations.
Remove-Item -Recurse -Force $stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $stage, $dist | Out-Null
Copy-Item (Join-Path $root "target/$Target/release/app-starter.exe") $stage
Copy-Item -Recurse (Join-Path $packaging 'Assets') $stage
$translations = Join-Path $root 'examples/windows-app/l10n'
if (Test-Path $translations) { Copy-Item -Recurse $translations (Join-Path $stage 'l10n') }

# MSIX versions have four parts; the last is reserved for the Store.
[xml] $manifest = Get-Content (Join-Path $packaging 'AppxManifest.xml')
$manifest.Package.Identity.Version = "$Version.0"
$manifest.Package.Identity.ProcessorArchitecture = $Arch
$manifest.Save((Join-Path $stage 'AppxManifest.xml'))

function Find-SdkTool([string] $name) {
    $kits = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits/10/bin'
    $tool = Get-ChildItem $kits -Recurse -Filter $name -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -match '\\x64\\' } |
        Sort-Object FullName -Descending | Select-Object -First 1
    if (-not $tool) { throw "$name not found; install the Windows SDK" }
    $tool.FullName
}

& (Find-SdkTool 'makeappx.exe') pack /o /d $stage /p $msix
if ($LASTEXITCODE -ne 0) { throw "makeappx failed ($LASTEXITCODE)" }

if ($env:WINDOWS_CERTIFICATE) {
    $pfx = Join-Path $env:RUNNER_TEMP 'signing.pfx'
    [IO.File]::WriteAllBytes($pfx, [Convert]::FromBase64String($env:WINDOWS_CERTIFICATE))
    try {
        & (Find-SdkTool 'signtool.exe') sign /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 `
            /f $pfx /p $env:WINDOWS_CERTIFICATE_PASSWORD $msix
        if ($LASTEXITCODE -ne 0) { throw "signtool failed ($LASTEXITCODE)" }
    } finally {
        Remove-Item -Force $pfx
    }
}

Write-Output "Built $msix"
