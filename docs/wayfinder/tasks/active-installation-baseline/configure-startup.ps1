[CmdletBinding()]
param(
    [string]$SpecPath = (Join-Path $PSScriptRoot 'baseline.spec.json')
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Set-StartupShortcut {
    param(
        [object]$Shell,
        [string]$Path,
        [string]$Target,
        [string]$Arguments,
        [string]$Description
    )

    if (-not (Test-Path -LiteralPath $Target -PathType Leaf)) {
        throw "Startup target is missing: $Target"
    }

    $shortcut = $Shell.CreateShortcut($Path)
    $shortcut.TargetPath = $Target
    $shortcut.Arguments = $Arguments
    $shortcut.WorkingDirectory = Split-Path -Parent $Target
    $shortcut.WindowStyle = 7
    $shortcut.Description = $Description
    $shortcut.Save()
}

$spec = Get-Content -LiteralPath $SpecPath -Raw | ConvertFrom-Json
$shell = New-Object -ComObject WScript.Shell

Set-StartupShortcut `
    -Shell $shell `
    -Path $spec.komorebi.startup_shortcut `
    -Target $spec.komorebi.startup_target `
    -Arguments $spec.komorebi.startup_arguments `
    -Description 'Start komorebi and whkd without a console'

Set-StartupShortcut `
    -Shell $shell `
    -Path $spec.active_bar.startup_shortcut `
    -Target $spec.active_bar.path `
    -Arguments '' `
    -Description 'Start the custom komorebi AppBar without a console'

$taskStates = @($spec.disabled_scheduled_tasks | ForEach-Object {
    $task = Get-ScheduledTask -TaskName $_ -ErrorAction SilentlyContinue
    if ($task) {
        Disable-ScheduledTask -TaskName $_ | Out-Null
    }
    [pscustomobject]@{
        name = $_
        state = if ($task) { [string](Get-ScheduledTask -TaskName $_).State } else { 'Absent' }
    }
})

[pscustomobject]@{
    manager_shortcut = $spec.komorebi.startup_shortcut
    appbar_shortcut = $spec.active_bar.startup_shortcut
    scheduled_tasks = $taskStates
} | ConvertTo-Json -Depth 4
