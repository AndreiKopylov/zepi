# Builds the bundled python layer in deps/python: embeddable CPython + pinned wheels.
param(
    [string]$Deps = '',
    [string]$Version = '3.13.7'
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
if (-not $Deps) { $Deps = Join-Path (Split-Path $PSScriptRoot -Parent) '.local\payload' }

$target = Join-Path $Deps 'python'
$wheels = Join-Path $env:TEMP 'zepi-pywheels'
$zip = Join-Path $env:TEMP ("python-" + $Version + "-embed-amd64.zip")
$requirements = Join-Path $PSScriptRoot 'python-requirements.txt'

$python = Get-Command python -ErrorAction SilentlyContinue
if (-not $python) { throw 'system python with pip is required to download wheels' }

if (Test-Path $target) { Write-Host ('missing: replacing existing ' + $target) }
Remove-Item $target -Recurse -Force -ErrorAction SilentlyContinue
Remove-Item $wheels -Recurse -Force -ErrorAction SilentlyContinue

Invoke-WebRequest -Uri ("https://www.python.org/ftp/python/$Version/python-$Version-embed-amd64.zip") -OutFile $zip -UseBasicParsing
Expand-Archive -Path $zip -DestinationPath $target
Write-Host ('python ' + $Version + ' unpacked')

& python -m pip download -q --only-binary=:all: --python-version 313 --platform win_amd64 -d $wheels -r $requirements
$site = Join-Path $target 'Lib\site-packages'
New-Item -ItemType Directory -Force -Path $site | Out-Null
Add-Type -AssemblyName System.IO.Compression.FileSystem
Get-ChildItem (Join-Path $wheels '*.whl') | ForEach-Object {
    $tmp = Join-Path $env:TEMP ('whl_' + [guid]::NewGuid().ToString('N'))
    [IO.Compression.ZipFile]::ExtractToDirectory($_.FullName, $tmp)
    Get-ChildItem $tmp | Where-Object { $_.Name -ne 'WHEEL' } | Move-Item -Destination $site -Force
    Remove-Item $tmp -Recurse -Force
}
# embeddable python ignores site-packages unless the path is listed in the _pth file
$pth = Get-ChildItem $target -Filter 'python*._pth' | Select-Object -First 1
if (-not (Select-String -Path $pth.FullName -Pattern 'Lib\\site-packages' -Quiet)) {
    Add-Content -Path $pth.FullName -Value 'Lib\\site-packages'
}

# the agent may type python3; without this copy the Windows Store alias wins
Copy-Item (Join-Path $target 'python.exe') (Join-Path $target 'python3.exe') -Force
& (Join-Path $target 'python.exe') -c "import pptx, docx, openpyxl, PIL, pypdf; print('bundled python ok:', pptx.__version__, openpyxl.__version__)"
if ($LASTEXITCODE -ne 0) { throw 'bundled python import check failed' }

$size = [int](((Get-ChildItem $target -Recurse | Measure-Object Length -Sum).Sum) / 1MB)
Write-Host ('ready: ' + $target + ' (' + $size + ' MB)')
Remove-Item $zip -Force
Remove-Item $wheels -Recurse -Force
