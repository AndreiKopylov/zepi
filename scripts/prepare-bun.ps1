# Fetches the pinned bun (native/bun.json) into .local/bun so the agent build is
# reproducible and does not depend on the machine's installed bun.
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$pin = Get-Content (Join-Path $repo 'native\bun.json') -Raw | ConvertFrom-Json
$dir = Join-Path $repo '.local\bun'
$exe = Join-Path $dir 'bun.exe'
$zip = Join-Path $dir 'bun.zip'
New-Item -ItemType Directory $dir -Force | Out-Null

if ((Test-Path $exe) -and ((& $exe --version) -eq $pin.version)) {
    Write-Host ("bun " + $pin.version + " already prepared")
    return
}
if (-not (Test-Path $zip) -or ((Get-FileHash $zip -Algorithm SHA256).Hash.ToLower() -ne $pin.sha256)) {
    Write-Host ("downloading bun " + $pin.version)
    Invoke-WebRequest -Uri $pin.url -OutFile $zip -UseBasicParsing
}
$hash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
if ($hash -ne $pin.sha256) { throw ("bun zip sha256 mismatch: " + $hash) }
Expand-Archive -Path $zip -DestinationPath $dir -Force
$inner = Join-Path $dir 'bun-windows-x64\bun.exe'
if (Test-Path $inner) { Move-Item $inner $exe -Force; Remove-Item (Join-Path $dir 'bun-windows-x64') -Recurse -Force }
Write-Host ("prepared bun " + (& $exe --version))