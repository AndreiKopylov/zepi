# Mirrors packages/acp/node_modules into the bundle. Junction points are never followed: /XJ.
param(
    [Parameter(Mandatory = $true)][string]$Bundle,
    [string]$Source = ''
)
if (-not $Source) { $Source = Join-Path (Split-Path $PSScriptRoot -Parent) 'packages\acp\node_modules' }
$ErrorActionPreference = 'Stop'
$destination = Join-Path $Bundle 'packages\acp\node_modules'
if (-not (Test-Path $Source)) { throw ('missing source: ' + $Source) }

Get-ChildItem $Source -Force | Where-Object { $_.PSIsContainer -and -not $_.LinkType } | ForEach-Object {
    robocopy $_.FullName (Join-Path $destination $_.Name) /MIR /XJ /NFL /NDL /NJH /NJS /R:1 /W:1 | Out-Null
    if ($LASTEXITCODE -ge 8) { throw ('robocopy failed for ' + $_.Name + ' (' + $LASTEXITCODE + ')') }
}

# Versions drift silently and only show up as a broken agent later
foreach ($package in '@earendil-works\pi-coding-agent', 'typebox', 'minimatch') {
    $from = Join-Path $Source "$package\package.json"
    $to = Join-Path $destination "$package\package.json"
    if ((Test-Path $from) -and (Test-Path $to)) {
        $a = (Get-Content $from -Raw | ConvertFrom-Json).version
        $b = (Get-Content $to -Raw | ConvertFrom-Json).version
        if ($a -ne $b) { Write-Host ('WARNING: ' + $package + ' bundle=' + $b + ' repo=' + $a) }
    }
}
Write-Host 'acp modules synced'
