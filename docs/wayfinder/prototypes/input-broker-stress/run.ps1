$ErrorActionPreference = 'Stop'

$prototypeDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$outputDir = Join-Path $prototypeDir 'bin'
$sourcePath = Join-Path $prototypeDir 'InputBrokerStressPrototype.cs'
$outputPath = Join-Path $outputDir 'InputBrokerStressPrototype.exe'
$compilerPath = 'C:\Windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe'

if (-not (Test-Path -LiteralPath $compilerPath)) {
    throw "The .NET Framework C# compiler was not found at $compilerPath"
}

New-Item -ItemType Directory -Force -Path $outputDir | Out-Null

& $compilerPath /nologo /target:winexe /platform:x64 /optimize+ `
    /reference:System.dll `
    /reference:System.Core.dll `
    /reference:System.Drawing.dll `
    /reference:System.Windows.Forms.dll `
    /out:$outputPath `
    $sourcePath

if ($LASTEXITCODE -ne 0) {
    throw "Prototype compilation failed with exit code $LASTEXITCODE"
}

Start-Process -FilePath $outputPath
