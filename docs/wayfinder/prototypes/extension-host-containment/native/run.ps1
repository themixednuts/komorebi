$ErrorActionPreference = 'Stop'
$previousRustFlags = $env:RUSTFLAGS

try {
    $env:RUSTFLAGS = (($previousRustFlags, '-C target-feature=+crt-static') -join ' ').Trim()
    Push-Location -LiteralPath $PSScriptRoot
    cargo build --release --bins
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed with exit code $LASTEXITCODE" }
    & .\target\release\containment-host.exe
    if ($LASTEXITCODE -ne 0) { throw "containment harness failed with exit code $LASTEXITCODE" }
}
finally {
    Pop-Location
    $env:RUSTFLAGS = $previousRustFlags
}
