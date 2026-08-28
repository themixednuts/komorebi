$ErrorActionPreference = 'Stop'

$nativeRoot = Join-Path $PSScriptRoot 'native'
$manifest = Join-Path $nativeRoot 'Cargo.toml'
$stamp = Get-Date -Format 'yyyyMMdd-HHmmssfffffff'
$report = Join-Path (Join-Path $nativeRoot 'results') "appbar-lifecycle-$stamp.json"

& cargo test --manifest-path $manifest
if ($LASTEXITCODE -ne 0) { throw 'AppBar lifecycle tests failed' }

& cargo clippy --manifest-path $manifest --all-targets -- -D warnings
if ($LASTEXITCODE -ne 0) { throw 'AppBar lifecycle Clippy audit failed' }

& cargo build --release --manifest-path $manifest --bins
if ($LASTEXITCODE -ne 0) { throw 'AppBar lifecycle release build failed' }

$probe = Join-Path (Join-Path (Join-Path $nativeRoot 'target') 'release') 'appbar-probe.exe'
& $probe --output $report
if ($LASTEXITCODE -ne 0) { throw 'AppBar lifecycle native matrix failed' }

Write-Output $report
