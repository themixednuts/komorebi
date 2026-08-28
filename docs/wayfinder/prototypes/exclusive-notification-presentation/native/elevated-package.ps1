[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [ValidateSet('Install', 'Remove')]
    [string] $Action,

    [string] $PackagePath,

    [string] $CertificatePath,

    [Parameter(Mandatory)]
    [ValidatePattern('^[0-9A-Fa-f]{40}$')]
    [string] $Thumbprint
)

$ErrorActionPreference = 'Stop'
$packageName = 'themixednuts.Komorebi.NotificationProbe'
$stores = @('Cert:\LocalMachine\TrustedPeople', 'Cert:\LocalMachine\Root')

if ($Action -eq 'Install') {
    if (-not $PackagePath -or -not $CertificatePath) {
        throw 'Install requires PackagePath and CertificatePath'
    }
    try {
        foreach ($store in $stores) {
            Import-Certificate -FilePath $CertificatePath -CertStoreLocation $store | Out-Null
        }
        Add-AppxPackage -Path $PackagePath
        exit 0
    }
    catch {
        Get-AppxPackage -Name $packageName | Remove-AppxPackage -ErrorAction SilentlyContinue
        foreach ($store in $stores) {
            $certificate = Join-Path $store $Thumbprint
            if (Test-Path -LiteralPath $certificate) {
                Remove-Item -LiteralPath $certificate
            }
        }
        throw
    }
}

Get-AppxPackage -Name $packageName | Remove-AppxPackage
foreach ($store in $stores) {
    $certificate = Join-Path $store $Thumbprint
    if (Test-Path -LiteralPath $certificate) {
        Remove-Item -LiteralPath $certificate
    }
}
