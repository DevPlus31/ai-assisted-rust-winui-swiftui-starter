# Runs the Windows App Certification Kit (WACK) on the packaged MSIX, the
# test Microsoft runs during Store certification. Called by
# store-check-windows.ps1 -RunWack. Needs admin (WACK installs the package);
# meant for CI runners or a test VM, because it trusts a throwaway signing
# certificate on the machine and installs the Windows App SDK runtime.
param([ValidateSet('x64', 'arm64')] [string] $Arch = 'x64')
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$temp = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { $env:TEMP }

$msix = Get-ChildItem (Join-Path $root 'dist') -Filter "*-$Arch.msix" |
    Sort-Object LastWriteTime -Descending | Select-Object -First 1
if (-not $msix) { throw "no dist/*-$Arch.msix; run scripts/package-msix.ps1 first" }
[xml] $manifest = Get-Content (Join-Path $root "target/msix-$Arch/AppxManifest.xml")
$publisher = $manifest.Package.Identity.Publisher

function Find-SdkTool([string] $name) {
    $kits = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits/10'
    $tool = Get-ChildItem $kits -Recurse -Filter $name -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -notmatch '\\(x86|arm|arm64)\\' } |
        Sort-Object FullName -Descending | Select-Object -First 1
    if (-not $tool) { throw "$name not found; install the Windows SDK" }
    $tool.FullName
}

# 1. A trusted test signature whose subject matches Identity/@Publisher, so
#    Windows will install the package. Removed again at the end.
$password = [guid]::NewGuid().ToString()
$secure = ConvertTo-SecureString $password -AsPlainText -Force
$cert = New-SelfSignedCertificate -Type Custom -Subject $publisher -KeyUsage DigitalSignature `
    -FriendlyName 'App Starter WACK test' -CertStoreLocation Cert:\CurrentUser\My `
    -TextExtension @('2.5.29.37={text}1.3.6.1.5.5.7.3.3', '2.5.29.19={text}')
$pfx = Join-Path $temp 'wack-test.pfx'
$signed = Join-Path $temp "wack-$Arch.msix"
try {
    Export-PfxCertificate -Cert $cert -FilePath $pfx -Password $secure | Out-Null
    Import-PfxCertificate -FilePath $pfx -CertStoreLocation Cert:\LocalMachine\TrustedPeople -Password $secure | Out-Null
    Copy-Item $msix.FullName $signed -Force
    & (Find-SdkTool 'signtool.exe') sign /fd SHA256 /f $pfx /p $password $signed | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "signtool failed ($LASTEXITCODE)" }

    # 2. The Windows App SDK runtime the package depends on.
    $dependency = ($manifest.Package.Dependencies.PackageDependency |
        Where-Object { $_.Name -like 'Microsoft.WindowsAppRuntime.*' }).Name
    if (-not (Get-AppxPackage -Name "$dependency*")) {
        $id = winget search --id $dependency --source winget --accept-source-agreements |
            Select-String -Pattern "($([regex]::Escape($dependency))[\w.]*)" |
            ForEach-Object { $_.Matches[0].Groups[1].Value } |
            Sort-Object -Descending | Select-Object -First 1
        if (-not $id) { throw "can't find $dependency in winget; install the Windows App SDK runtime" }
        Write-Output "    installing $id"
        winget install --id $id --exact --silent --accept-package-agreements --accept-source-agreements | Out-Null
    }

    # 3. Run the kit and summarise its XML report.
    $appcert = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits/10/App Certification Kit/appcert.exe'
    if (-not (Test-Path $appcert)) { throw 'appcert.exe not found; install the Windows SDK with the App Certification Kit' }
    $report = Join-Path $root "target/wack-$Arch.xml"
    & $appcert reset | Out-Null
    & $appcert test -appxpackagepath $signed -reportoutputpath $report | Out-Null
    if (-not (Test-Path $report)) { throw "WACK produced no report ($LASTEXITCODE)" }
    [xml] $result = Get-Content $report
    $overall = $result.REPORT.OVERALL_RESULT
    foreach ($test in $result.SelectNodes('//TEST')) {
        $outcome = $test.RESULT.InnerText.Trim()
        if ($outcome -ne 'PASS') { Write-Output "    $outcome  $($test.NAME)" }
    }
    Write-Output "    overall: $overall (full report: $report)"
    exit $(if ($overall -eq 'FAIL') { 1 } else { 0 })
} finally {
    Get-ChildItem Cert:\LocalMachine\TrustedPeople, Cert:\CurrentUser\My |
        Where-Object Thumbprint -eq $cert.Thumbprint | Remove-Item -ErrorAction SilentlyContinue
    Remove-Item $pfx, $signed -Force -ErrorAction SilentlyContinue
}
