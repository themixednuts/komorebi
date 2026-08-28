[CmdletBinding()]
param(
    [ValidateSet('Register', 'Status', 'RequestAccess', 'Measure', 'Unregister')]
    [string] $Action = 'Status'
)

$ErrorActionPreference = 'Stop'

$prototypeRoot = $PSScriptRoot
$nativeRoot = Join-Path $prototypeRoot 'native'
$manifest = Join-Path $nativeRoot 'Cargo.toml'
$packageRoot = Join-Path $nativeRoot 'package'
$packageExecutable = Join-Path $packageRoot 'notification-probe.exe'
$assetRoot = Join-Path $packageRoot 'Assets'
$sourceAsset = Join-Path (Split-Path (Split-Path (Split-Path $prototypeRoot -Parent) -Parent) -Parent) 'assets\layout-ratios_before.png'
$packageName = 'themixednuts.Komorebi.NotificationProbe'
$certificateFriendlyName = 'Komorebi Notification Probe Temporary Signing'
$distRoot = Join-Path $nativeRoot 'dist'
$msix = Join-Path $distRoot 'notification-probe.msix'
$certificateFile = Join-Path $distRoot 'notification-probe-signing.cer'
$thumbprintFile = Join-Path $distRoot 'certificate-thumbprint.txt'
$elevatedScript = Join-Path $nativeRoot 'elevated-package.ps1'

function Build-Probe {
    & cargo test --manifest-path $manifest
    if ($LASTEXITCODE -ne 0) { throw 'Notification probe tests failed' }

    & cargo clippy --manifest-path $manifest --all-targets -- -D warnings
    if ($LASTEXITCODE -ne 0) { throw 'Notification probe Clippy audit failed' }

    & cargo build --release --manifest-path $manifest
    if ($LASTEXITCODE -ne 0) { throw 'Notification probe release build failed' }

    $builtExecutable = Join-Path $nativeRoot 'target\release\notification-probe.exe'
    New-Item -ItemType Directory -Path $assetRoot -Force | Out-Null
    Copy-Item -LiteralPath $builtExecutable -Destination $packageExecutable -Force
    Copy-Item -LiteralPath $sourceAsset -Destination (Join-Path $assetRoot 'StoreLogo.png') -Force
    Copy-Item -LiteralPath $sourceAsset -Destination (Join-Path $assetRoot 'Square150x150Logo.png') -Force
    Copy-Item -LiteralPath $sourceAsset -Destination (Join-Path $assetRoot 'Square44x44Logo.png') -Force
}

function Invoke-Probe([string[]] $Arguments) {
    $package = Get-AppxPackage -Name $packageName
    if (-not $package) { throw 'The packaged notification probe is not registered' }
    $installedExecutable = Join-Path $package.InstallLocation 'notification-probe.exe'
    & $installedExecutable @Arguments
    if ($LASTEXITCODE -ne 0) { throw "Notification probe failed with exit code $LASTEXITCODE" }
}

function Get-SdkTool([string] $Name) {
    $kitsRoot = (Get-ItemProperty -LiteralPath 'HKLM:\SOFTWARE\Microsoft\Windows Kits\Installed Roots').KitsRoot10
    if (-not $kitsRoot) { throw 'Windows SDK installation root was not registered' }
    $sdkBin = Join-Path $kitsRoot 'bin'
    $matches = @(Get-ChildItem -LiteralPath $sdkBin -Recurse -File -Filter $Name | Where-Object { $_.Directory.Name -eq 'x64' } | Sort-Object { [version]$_.Directory.Parent.Name } -Descending)
    if ($matches.Count -eq 0) { throw "Windows SDK tool was not found: $Name" }
    return $matches[0].FullName
}

function Invoke-ElevatedPackage([string] $PackageAction, [string] $Thumbprint) {
    $arguments = @(
        '-NoProfile',
        '-ExecutionPolicy', 'Bypass',
        '-File', "`"$elevatedScript`"",
        '-Action', $PackageAction,
        '-Thumbprint', $Thumbprint
    )
    if ($PackageAction -eq 'Install') {
        $arguments += @('-PackagePath', "`"$msix`"", '-CertificatePath', "`"$certificateFile`"")
    }
    $process = Start-Process -FilePath 'powershell.exe' -Verb RunAs -ArgumentList $arguments -WindowStyle Hidden -Wait -PassThru
    if ($process.ExitCode -ne 0) { throw "Elevated package operation failed with exit code $($process.ExitCode)" }
}

switch ($Action) {
    'Register' {
        Build-Probe
        New-Item -ItemType Directory -Path $distRoot -Force | Out-Null
        $existing = @(Get-ChildItem -LiteralPath Cert:\CurrentUser\My | Where-Object FriendlyName -eq $certificateFriendlyName)
        if ($existing.Count -ne 0) { throw 'A prior probe private signing certificate still exists; run Unregister first' }
        $certificate = New-SelfSignedCertificate -Type Custom -Subject 'CN=themixednuts' -FriendlyName $certificateFriendlyName -CertStoreLocation 'Cert:\CurrentUser\My' -KeyAlgorithm RSA -KeyLength 2048 -HashAlgorithm SHA256 -KeyUsage DigitalSignature -TextExtension @('2.5.29.37={text}1.3.6.1.5.5.7.3.3')
        Set-Content -LiteralPath $thumbprintFile -Value $certificate.Thumbprint -Encoding ascii -NoNewline
        try {
            Export-Certificate -Cert $certificate -FilePath $certificateFile -Force | Out-Null

            & (Get-SdkTool 'makeappx.exe') pack /d $packageRoot /p $msix /o
            if ($LASTEXITCODE -ne 0) { throw 'MSIX packaging failed' }
            & (Get-SdkTool 'signtool.exe') sign /fd SHA256 /sha1 $certificate.Thumbprint /s My $msix
            if ($LASTEXITCODE -ne 0) { throw 'MSIX signing failed' }

            Invoke-ElevatedPackage 'Install' $certificate.Thumbprint
        }
        catch {
            Remove-Item -LiteralPath $certificateFile, $thumbprintFile, $msix -ErrorAction SilentlyContinue
            throw
        }
        finally {
            $privateCertificate = Join-Path 'Cert:\CurrentUser\My' $certificate.Thumbprint
            if (Test-Path -LiteralPath $privateCertificate) {
                Remove-Item -LiteralPath $privateCertificate
            }
        }
        Remove-Item -LiteralPath $certificateFile, $msix -ErrorAction SilentlyContinue
        Get-AppxPackage -Name $packageName | Select-Object Name, PackageFullName, InstallLocation
    }
    'Status' {
        Invoke-Probe @('status')
    }
    'RequestAccess' {
        Invoke-Probe @('request-access')
    }
    'Measure' {
        $results = Join-Path $nativeRoot 'results'
        New-Item -ItemType Directory -Path $results -Force | Out-Null
        $stamp = Get-Date -Format 'yyyyMMdd-HHmmssfffffff'
        $report = Join-Path $results "notification-presentation-$stamp.json"
        Invoke-Probe @('measure', '--output', $report)
        Write-Output $report
    }
    'Unregister' {
        if (-not (Test-Path -LiteralPath $thumbprintFile)) { throw 'The probe certificate thumbprint record is missing' }
        $thumbprint = (Get-Content -LiteralPath $thumbprintFile -Raw).Trim()
        Invoke-ElevatedPackage 'Remove' $thumbprint
        foreach ($storeName in @('My', 'TrustedPeople', 'Root')) {
            $certificate = Join-Path "Cert:\CurrentUser\$storeName" $thumbprint
            if (Test-Path -LiteralPath $certificate) {
                & certutil.exe -user -delstore $storeName $thumbprint | Out-Null
                if ($LASTEXITCODE -ne 0) { throw "Could not remove probe certificate from CurrentUser $storeName" }
            }
        }
        Remove-Item -LiteralPath $certificateFile, $thumbprintFile, $msix -ErrorAction SilentlyContinue
    }
}
