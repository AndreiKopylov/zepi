# Mirrors packages/{acp,runtime} node_modules into the bundle. Junction points are never followed: /XJ.
param(
    [Parameter(Mandatory = $true)][string]$Bundle,
    [string]$Source = ''
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent

function MirrorModules([string]$Package, [string]$PackageSource) {
    if (-not (Test-Path $PackageSource)) { throw ('missing source: ' + $PackageSource) }
    $destination = Join-Path $Bundle ('packages\' + $Package + '\node_modules')
    Get-ChildItem $PackageSource -Force | Where-Object { $_.PSIsContainer -and -not $_.LinkType } | ForEach-Object {
        robocopy $_.FullName (Join-Path $destination $_.Name) /MIR /XJ /NFL /NDL /NJH /NJS /R:1 /W:1 | Out-Null
        if ($LASTEXITCODE -ge 8) { throw ('robocopy failed for ' + $_.Name + ' (' + $LASTEXITCODE + ')') }
    }
}

foreach ($package in 'acp', 'runtime') {
    $packageSource = if ($Source -and $package -eq 'acp') { $Source } else { Join-Path $repo ('packages\' + $package + '\node_modules') }
    MirrorModules $package $packageSource
}

# Versions drift silently and only show up as a broken agent later
$destination = Join-Path $Bundle 'packages\acp\node_modules'
foreach ($package in '@earendil-works\pi-coding-agent', 'typebox', 'minimatch') {
    $from = Join-Path $repo ('packages\acp\node_modules\' + $package + '\package.json')
    $to = Join-Path $destination "$package\package.json"
    if ((Test-Path $from) -and (Test-Path $to)) {
        $a = (Get-Content $from -Raw | ConvertFrom-Json).version
        $b = (Get-Content $to -Raw | ConvertFrom-Json).version
        if ($a -ne $b) { Write-Host ('WARNING: ' + $package + ' bundle=' + $b + ' repo=' + $a) }
    }
}
Write-Host 'acp modules synced'
