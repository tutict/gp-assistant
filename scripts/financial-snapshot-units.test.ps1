$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'financial-snapshot-units.ps1')

function Assert-RateClose($Actual, $Expected, $Context) {
    if ([Math]::Abs([double]$Actual - [double]$Expected) -gt 0.000000000001) {
        throw "$Context expected $Expected, got $Actual"
    }
}

$legacy = '{"schema_version":"mobile-financial-snapshot/v3","generated_at":"2026-07-16","financials":{"000001.SZ":{"roe":0.07,"deducted_net_profit_growth_rate":0.804352,"latest_eps":0.67}}}' | ConvertFrom-Json
$normalized = ConvertTo-MobileFinancialUnitContract $legacy
Assert-RateClose $normalized.financials.'000001.SZ'.roe 0.07 'legacy ROE points'
Assert-RateClose $normalized.financials.'000001.SZ'.deducted_net_profit_growth_rate 0.804352 'legacy growth points'
if ($normalized.schema_version -ne 'mobile-financial-snapshot/v4' -or $normalized.metric_contract_version -ne 2) { throw 'Missing unit contract version' }
if ($normalized.generated_at -ne '2026-07-16') { throw 'Migration incorrectly claims refreshed data' }

$ratio = '{"schema_version":"mobile-financial-snapshot/v4","metric_units":{"roe":"ratio","deducted_net_profit_growth_rate":"ratio"},"financials":{"000001.SZ":{"roe":1.2,"deducted_net_profit_growth_rate":0.00804352}}}' | ConvertFrom-Json
$converted = ConvertTo-MobileFinancialUnitContract $ratio
Assert-RateClose $converted.financials.'000001.SZ'.roe 120 'large ratio'
Assert-RateClose $converted.financials.'000001.SZ'.deducted_net_profit_growth_rate 0.804352 'declared growth ratio'
$again = ConvertTo-MobileFinancialUnitContract $converted
Assert-RateClose $again.financials.'000001.SZ'.roe 120 'idempotent migration'
Assert-RateClose $again.financials.'000001.SZ'.deducted_net_profit_growth_rate 0.804352 'idempotent small growth'

foreach ($invalid in @(
    '{"financials":{"000001.SZ":{"roe":0.07}}}',
    '{"schema_version":"mobile-financial-snapshot/v4","metric_units":{},"financials":{"000001.SZ":{"roe":0.07}}}',
    '{"schema_version":"mobile-financial-snapshot/v4","metric_units":{"roe":"unknown"},"financials":{"000001.SZ":{"roe":0.07}}}'
)) {
    $rejected = $false
    try { ConvertTo-MobileFinancialUnitContract ($invalid | ConvertFrom-Json) | Out-Null } catch { $rejected = $true }
    if (-not $rejected) { throw "Unknown or missing units accepted: $invalid" }
}
Write-Output 'Financial snapshot units: conversion, versions, idempotence, provenance and missing-unit rejection passed'
