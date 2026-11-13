[CmdletBinding()]
param([Parameter(Mandatory = $true)][string] $ParametersPath)

$ErrorActionPreference = 'Stop'
$values = Get-Content -LiteralPath $ParametersPath -Raw -Encoding UTF8 | ConvertFrom-Json
$gateParameters = @{}
foreach ($property in $values.PSObject.Properties) {
    $gateParameters[$property.Name] = $property.Value
}

# Keep expected failures out of the legacy console's redirected-error formatter.
try {
    & (Join-Path $PSScriptRoot 'release-check.ps1') @gateParameters 2>&1
} catch {
    Write-Output $_.Exception.Message
    exit 1
}
if ($LASTEXITCODE) { exit $LASTEXITCODE }
