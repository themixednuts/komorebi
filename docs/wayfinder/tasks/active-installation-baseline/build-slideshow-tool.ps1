$ErrorActionPreference = 'Stop'

$compiler = 'C:\Windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe'
$source = Join-Path $PSScriptRoot 'DesktopSlideshow.cs'
$outputDirectory = Join-Path $PSScriptRoot 'bin'
$output = Join-Path $outputDirectory 'DesktopSlideshow.exe'

if (-not (Test-Path -LiteralPath $compiler -PathType Leaf)) {
    throw "C# compiler not found at $compiler"
}

New-Item -ItemType Directory -Force -Path $outputDirectory | Out-Null
& $compiler /nologo /target:exe /platform:x64 /optimize+ "/out:$output" $source
if ($LASTEXITCODE -ne 0) {
    throw "DesktopSlideshow compilation failed with exit code $LASTEXITCODE"
}

& $output get
if ($LASTEXITCODE -ne 0) {
    throw "DesktopSlideshow read-back failed with exit code $LASTEXITCODE"
}
