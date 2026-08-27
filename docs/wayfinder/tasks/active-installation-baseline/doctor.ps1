[CmdletBinding()]
param(
    [string]$SpecPath = (Join-Path $PSScriptRoot 'baseline.spec.json'),
    [string]$OutputPath
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function New-Check {
    param(
        [string]$Name,
        [bool]$Passed,
        $Expected,
        $Actual
    )

    [pscustomobject]@{
        name = $Name
        passed = $Passed
        expected = $Expected
        actual = $Actual
    }
}

function Get-Sha256 {
    param([string]$Path)

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        return $null
    }

    (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash
}

function Get-PeSubsystem {
    param([string]$Path)

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        return $null
    }

    $stream = [IO.File]::OpenRead($Path)
    try {
        $reader = [IO.BinaryReader]::new($stream)
        $stream.Position = 0x3c
        $peHeaderOffset = $reader.ReadInt32()
        $stream.Position = $peHeaderOffset + 24 + 68
        $reader.ReadUInt16()
    } finally {
        $stream.Dispose()
    }
}

function Get-TextSha256 {
    param([string]$Text)

    $algorithm = [System.Security.Cryptography.SHA256]::Create()
    try {
        ([BitConverter]::ToString($algorithm.ComputeHash([Text.Encoding]::UTF8.GetBytes($Text)))).Replace('-', '')
    } finally {
        $algorithm.Dispose()
    }
}

function Get-Shortcut {
    param([string]$Path)

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        return [pscustomobject]@{ path = $Path; exists = $false; target = $null; arguments = $null }
    }

    $shell = New-Object -ComObject WScript.Shell
    $shortcut = $shell.CreateShortcut($Path)
    [pscustomobject]@{
        path = $Path
        exists = $true
        target = $shortcut.TargetPath
        arguments = $shortcut.Arguments
    }
}

function Get-DirectoryManifest {
    param([string]$Path)

    if (-not (Test-Path -LiteralPath $Path -PathType Container)) {
        return [pscustomobject]@{ files = @(); digest = $null }
    }

    $files = @(Get-ChildItem -LiteralPath $Path -File | Sort-Object Name | ForEach-Object {
        [pscustomobject]@{
            name = $_.Name
            path = $_.FullName
            bytes = $_.Length
            sha256 = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash
        }
    })
    $manifest = [string]::Join("`n", @($files | ForEach-Object { "{0}`t{1}" -f $_.name, $_.sha256 }))

    [pscustomobject]@{
        files = $files
        digest = Get-TextSha256 $manifest
    }
}

function Invoke-CapturedProcess {
    param(
        [string]$Path,
        [string[]]$Arguments
    )

    $output = & $Path @Arguments 2>&1 | Out-String
    [pscustomobject]@{
        exit_code = $LASTEXITCODE
        output = $output.Trim()
    }
}

function Test-JsonText {
    param([string]$Text)

    try {
        $null = $Text | ConvertFrom-Json
        $true
    } catch {
        $false
    }
}

$spec = Get-Content -LiteralPath $SpecPath -Raw | ConvertFrom-Json
$checks = [Collections.Generic.List[object]]::new()
$checks.Add((New-Check 'spec-version' ($spec.version -eq 2) 2 $spec.version))

$barHash = Get-Sha256 $spec.active_bar.path
$checks.Add((New-Check 'active-bar-file' ($null -ne $barHash) $spec.active_bar.path $(if ($barHash) { $spec.active_bar.path } else { $null })))
$checks.Add((New-Check 'active-bar-hash' ($barHash -eq $spec.active_bar.sha256) $spec.active_bar.sha256 $barHash))
$checks.Add((New-Check 'active-bar-gui-subsystem' ((Get-PeSubsystem $spec.active_bar.path) -eq 2) 2 (Get-PeSubsystem $spec.active_bar.path)))

$barShortcut = Get-Shortcut $spec.active_bar.startup_shortcut
$checks.Add((New-Check 'appbar-startup-shortcut' ($barShortcut.exists -and $barShortcut.target.Equals($spec.active_bar.path, [StringComparison]::OrdinalIgnoreCase) -and [string]::IsNullOrWhiteSpace($barShortcut.arguments)) @{ target = $spec.active_bar.path; arguments = '' } $barShortcut))

$managerShortcut = Get-Shortcut $spec.komorebi.startup_shortcut
$checks.Add((New-Check 'manager-startup-shortcut' ($managerShortcut.exists -and $managerShortcut.target.Equals($spec.komorebi.startup_target, [StringComparison]::OrdinalIgnoreCase) -and ($managerShortcut.arguments -eq $spec.komorebi.startup_arguments)) @{ target = $spec.komorebi.startup_target; arguments = $spec.komorebi.startup_arguments } $managerShortcut))
$checks.Add((New-Check 'manager-launcher-gui-subsystem' ((Get-PeSubsystem $spec.komorebi.startup_target) -eq 2) 2 (Get-PeSubsystem $spec.komorebi.startup_target)))

$scheduledTaskStates = @($spec.disabled_scheduled_tasks | ForEach-Object {
    $task = Get-ScheduledTask -TaskName $_ -ErrorAction SilentlyContinue
    [pscustomobject]@{ name = $_; state = if ($task) { [string]$task.State } else { $null } }
})
$enabledScheduledTasks = @($scheduledTaskStates | Where-Object { $_.state -ne 'Disabled' })
$checks.Add((New-Check 'legacy-scheduled-tasks-disabled' ($enabledScheduledTasks.Count -eq 0) 'all disabled' $scheduledTaskStates))

$barProcesses = @(Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'komorebi-bar.exe' } | Select-Object ProcessId, ExecutablePath, CommandLine)
$barProcessPaths = @($barProcesses | ForEach-Object { $_.ExecutablePath })
$checks.Add((New-Check 'active-bar-process' (($barProcessPaths.Count -eq 1) -and ($barProcessPaths[0] -eq $spec.active_bar.path)) @($spec.active_bar.path) $barProcessPaths))

$managerProcesses = @(Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'komorebi.exe' } | Select-Object ProcessId, ExecutablePath, CommandLine)
$managerProcessPaths = @($managerProcesses | ForEach-Object { $_.ExecutablePath })
$checks.Add((New-Check 'active-manager-process' (($managerProcessPaths.Count -eq 1) -and ($managerProcessPaths[0] -eq $spec.komorebi.executable_path)) @($spec.komorebi.executable_path) $managerProcessPaths))

$wallpaperManifest = Get-DirectoryManifest $spec.wallpapers.path
$checks.Add((New-Check 'wallpaper-count' ($wallpaperManifest.files.Count -eq $spec.wallpapers.file_count) $spec.wallpapers.file_count $wallpaperManifest.files.Count))
$checks.Add((New-Check 'wallpaper-manifest' ($wallpaperManifest.digest -eq $spec.wallpapers.manifest_sha256) $spec.wallpapers.manifest_sha256 $wallpaperManifest.digest))

$themeExists = Test-Path -LiteralPath $spec.windows.theme_path -PathType Leaf
$themeText = if ($themeExists) { Get-Content -LiteralPath $spec.windows.theme_path -Raw } else { '' }
$currentTheme = [string](Get-ItemPropertyValue 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes' 'CurrentTheme' -ErrorAction SilentlyContinue)
$checks.Add((New-Check 'current-theme' ($currentTheme -eq $spec.windows.theme_path) $spec.windows.theme_path $currentTheme))
$checks.Add((New-Check 'theme-has-no-source-path' (-not $themeText.Contains($spec.forbidden_active_root, [StringComparison]::OrdinalIgnoreCase)) 'no source checkout path' $(if ($themeText.Contains($spec.forbidden_active_root, [StringComparison]::OrdinalIgnoreCase)) { $spec.forbidden_active_root } else { $null })))

$rootMatch = [regex]::Match($themeText, '(?m)^ImagesRootPath=(.+)$')
$themeRoot = if ($rootMatch.Success) { $rootMatch.Groups[1].Value.Trim() } else { $null }
$themeItems = @([regex]::Matches($themeText, '(?m)^Item\d+Path=(.+)$') | ForEach-Object { $_.Groups[1].Value.Trim() })
$themeItemsUnderRoot = @($themeItems | Where-Object { -not $_.StartsWith($spec.wallpapers.path + '\', [StringComparison]::OrdinalIgnoreCase) })
$checks.Add((New-Check 'theme-wallpaper-root' ($themeRoot -eq $spec.wallpapers.path) $spec.wallpapers.path $themeRoot))
$checks.Add((New-Check 'theme-wallpaper-items' (($themeItems.Count -eq $spec.wallpapers.file_count) -and ($themeItemsUnderRoot.Count -eq 0)) @{ count = $spec.wallpapers.file_count; root = $spec.wallpapers.path } @{ count = $themeItems.Count; outside_root = $themeItemsUnderRoot }))

$slideshowTool = Join-Path $PSScriptRoot 'bin\DesktopSlideshow.exe'
$slideshowRead = if (Test-Path -LiteralPath $slideshowTool -PathType Leaf) { Invoke-CapturedProcess $slideshowTool @('get') } else { [pscustomobject]@{ exit_code = 127; output = 'tool not built' } }
$liveSlideshowItems = @($slideshowRead.output -split "`r?`n" | Where-Object { $_ })
$checks.Add((New-Check 'live-slideshow-root' (($slideshowRead.exit_code -eq 0) -and ($liveSlideshowItems.Count -eq 1) -and ($liveSlideshowItems[0] -eq $spec.wallpapers.path)) @($spec.wallpapers.path) $liveSlideshowItems))

$wallpaperCacheHash = Get-Sha256 $spec.windows.wallpaper_cache_path
$checks.Add((New-Check 'wallpaper-cache' ($null -ne $wallpaperCacheHash) $spec.windows.wallpaper_cache_path $(if ($wallpaperCacheHash) { $spec.windows.wallpaper_cache_path } else { $null })))

$config = Get-Content -LiteralPath $spec.komorebi.config_path -Raw | ConvertFrom-Json
$barConfig = Get-Content -LiteralPath $spec.komorebi.bar_config_path -Raw | ConvertFrom-Json
$applications = Get-Content -LiteralPath $spec.komorebi.applications_path -Raw | ConvertFrom-Json
$workspaceCount = @($config.monitors[0].workspaces).Count
$checks.Add((New-Check 'configured-workspaces' ($workspaceCount -eq $spec.komorebi.workspace_count) $spec.komorebi.workspace_count $workspaceCount))
$checks.Add((New-Check 'applications-schema' ($applications.'$schema' -eq $spec.komorebi.applications_schema) $spec.komorebi.applications_schema $applications.'$schema'))
$checks.Add((New-Check 'komorebi-schema-pinned' ($config.'$schema' -match '/v\d+\.\d+\.\d+/') 'versioned schema URL' $config.'$schema'))
$checks.Add((New-Check 'bar-schema-pinned' ($barConfig.'$schema' -match '/v\d+\.\d+\.\d+/') 'versioned schema URL' $barConfig.'$schema'))

$bindingIndices = @(Get-Content -LiteralPath $spec.komorebi.whkd_config_path | Where-Object { -not $_.TrimStart().StartsWith('#') } | ForEach-Object {
    $match = [regex]::Match($_, 'komorebic\s+(?:focus-workspace|move-to-workspace)\s+(\d+)')
    if ($match.Success) { [int]$match.Groups[1].Value }
})
$invalidBindings = @($bindingIndices | Where-Object { $_ -ge $workspaceCount })
$checks.Add((New-Check 'workspace-bindings' ($invalidBindings.Count -eq 0) "indices below $workspaceCount" $bindingIndices))

$configurationCheck = Invoke-CapturedProcess $spec.komorebi.client_path @('check')
$checks.Add((New-Check 'komorebic-check' ($configurationCheck.exit_code -eq 0) 0 $configurationCheck.exit_code))
$stateQuery = Invoke-CapturedProcess $spec.komorebi.client_path @('state')
$stateIsJson = ($stateQuery.exit_code -eq 0) -and (Test-JsonText $stateQuery.output)
$stateQueryActual = [ordered]@{
    exit_code = $stateQuery.exit_code
    json = $stateIsJson
    bytes = [Text.Encoding]::UTF8.GetByteCount($stateQuery.output)
    error = if ($stateIsJson) { $null } else { $stateQuery.output }
}
$checks.Add((New-Check 'komorebi-ipc-state' $stateIsJson 'exit 0 and JSON response' $stateQueryActual))

$managerSocket = Join-Path $spec.komorebi.data_path 'komorebi.sock'
$subscriberSockets = @(Get-ChildItem -LiteralPath $spec.komorebi.data_path -Filter 'komorebi-bar-*' -ErrorAction SilentlyContinue | Select-Object Name, FullName, CreationTime, LastWriteTime)
$runtimeLink = Get-Item -LiteralPath $spec.komorebi.data_path -Force -ErrorAction SilentlyContinue
$runtimeLinkTargets = @($(if ($runtimeLink) { $runtimeLink.Target }))
$checks.Add((New-Check 'runtime-directory-target' (($runtimeLink.LinkType -eq 'Junction') -and ($runtimeLinkTargets.Count -eq 1) -and ($runtimeLinkTargets[0] -eq $spec.komorebi.runtime_target_path)) $spec.komorebi.runtime_target_path $runtimeLinkTargets))
$checks.Add((New-Check 'manager-socket-marker' (Test-Path -LiteralPath $managerSocket) $managerSocket $(Test-Path -LiteralPath $managerSocket)))
$checks.Add((New-Check 'bar-socket-marker' ($subscriberSockets.Count -eq 1) 'exactly one live marker' @($subscriberSockets | ForEach-Object { $_.Name })))

$originPush = (& git -C $spec.repository.path remote get-url --push origin 2>&1 | Out-String).Trim()
$checks.Add((New-Check 'fork-push-remote' ($originPush -eq $spec.repository.origin_push_url) $spec.repository.origin_push_url $originPush))

$failedChecks = @($checks | Where-Object { -not $_.passed })
$report = [ordered]@{
    schema_version = 1
    generated_at = (Get-Date).ToString('o')
    passed = ($failedChecks.Count -eq 0)
    summary = [ordered]@{
        check_count = $checks.Count
        failed_count = $failedChecks.Count
        failed_checks = @($failedChecks | ForEach-Object { $_.name })
    }
    paths = [ordered]@{
        spec = (Resolve-Path -LiteralPath $SpecPath).Path
        active_bar = $spec.active_bar.path
        appbar_startup = $spec.active_bar.startup_shortcut
        manager_startup = $spec.komorebi.startup_shortcut
        wallpapers = $spec.wallpapers.path
        theme = $spec.windows.theme_path
        komorebi_config = $spec.komorebi.config_path
        applications = $spec.komorebi.applications_path
        whkd = $spec.komorebi.whkd_config_path
        runtime_target = $spec.komorebi.runtime_target_path
        repository = $spec.repository.path
        live_slideshow = $liveSlideshowItems
    }
    hashes = [ordered]@{
        active_bar_sha256 = $barHash
        wallpaper_manifest_sha256 = $wallpaperManifest.digest
        theme_sha256 = Get-Sha256 $spec.windows.theme_path
        wallpaper_cache_sha256 = $wallpaperCacheHash
        komorebi_config_sha256 = Get-Sha256 $spec.komorebi.config_path
        bar_config_sha256 = Get-Sha256 $spec.komorebi.bar_config_path
        applications_sha256 = Get-Sha256 $spec.komorebi.applications_path
        whkd_sha256 = Get-Sha256 $spec.komorebi.whkd_config_path
    }
    wallpaper_files = $wallpaperManifest.files
    processes = [ordered]@{
        manager = $managerProcesses
        bar = $barProcesses
        whkd = @(Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'whkd.exe' } | Select-Object ProcessId, ExecutablePath, CommandLine)
    }
    checks = $checks
}

$json = $report | ConvertTo-Json -Depth 12
if ($OutputPath) {
    $parent = Split-Path -Parent $OutputPath
    if ($parent) {
        New-Item -ItemType Directory -Force -Path $parent | Out-Null
    }
    [IO.File]::WriteAllText($OutputPath, $json, [Text.UTF8Encoding]::new($false))
}
$json

if (-not $report.passed) {
    exit 1
}
