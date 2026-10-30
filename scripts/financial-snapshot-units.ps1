function Get-MobileFinancialRateUnits {
    [ordered]@{
        operating_revenue_yoy = 'percentage_points'
        parent_net_profit_yoy = 'percentage_points'
        gross_margin = 'percentage_points'
        net_margin = 'percentage_points'
        roe = 'percentage_points'
        asset_liability_ratio = 'percentage_points'
        goodwill_to_net_assets = 'percentage_points'
        pledged_share_ratio = 'percentage_points'
        dividend_yield = 'percentage_points'
        dividend_payout_ratio = 'percentage_points'
        deducted_net_profit_margin = 'percentage_points'
        deducted_net_profit_growth_rate = 'percentage_points'
    }
}

function Copy-MobileFinancialProperties {
    param([Parameter(Mandatory = $true)] $Value)
    $copy = [ordered]@{}
    if ($Value -is [System.Collections.IDictionary]) {
        foreach ($key in $Value.Keys) { $copy[[string]$key] = $Value[$key] }
    } else {
        foreach ($property in $Value.PSObject.Properties) { $copy[$property.Name] = $property.Value }
    }
    return $copy
}

function ConvertTo-MobileFinancialUnitContract {
    param([Parameter(Mandatory = $true)] $Snapshot)
    $copy = Copy-MobileFinancialProperties $Snapshot
    $canonicalUnits = Get-MobileFinancialRateUnits
    if ($copy.Contains('metric_units')) {
        if ($null -eq $copy['metric_units'] -or $copy['metric_units'] -is [string] -or $copy['metric_units'] -is [array]) {
            throw 'Invalid financial metric unit declaration'
        }
        $inputUnits = Copy-MobileFinancialProperties $copy['metric_units']
        foreach ($field in $inputUnits.Keys) {
            if (-not $canonicalUnits.Contains($field) -or $inputUnits[$field] -notin @('ratio', 'percentage_points')) {
                throw "Invalid financial metric unit: $field"
            }
        }
    } elseif ($copy['schema_version'] -eq 'mobile-financial-snapshot/v3') {
        $inputUnits = $canonicalUnits
    } else {
        throw 'Financial snapshot requires explicit metric_units or the known v3 contract'
    }
    if (-not $copy.Contains('financials') -or $null -eq $copy['financials']) {
        throw 'Financial snapshot requires a financials object'
    }
    $financials = [ordered]@{}
    $sourceFinancials = Copy-MobileFinancialProperties $copy['financials']
    foreach ($code in $sourceFinancials.Keys) {
        $row = Copy-MobileFinancialProperties $sourceFinancials[$code]
        foreach ($field in $canonicalUnits.Keys) {
            if (-not $row.Contains($field) -or $null -eq $row[$field] -or $row[$field] -eq '') { continue }
            if (-not $inputUnits.Contains($field)) { throw "Financial metric $field is missing its unit" }
            $number = 0.0
            if (-not [double]::TryParse([string]$row[$field], [Globalization.NumberStyles]::Float, [Globalization.CultureInfo]::InvariantCulture, [ref]$number) -or [double]::IsNaN($number) -or [double]::IsInfinity($number)) {
                throw "Financial metric $field is not finite: $code"
            }
            if ($inputUnits[$field] -eq 'ratio') { $number *= 100.0 }
            if ([double]::IsInfinity($number)) { throw "Financial metric $field conversion overflowed: $code" }
            $row[$field] = $number
        }
        $financials[$code] = [pscustomobject]$row
    }
    $copy['financials'] = [pscustomobject]$financials
    $copy['schema_version'] = 'mobile-financial-snapshot/v4'
    $copy['metric_contract_version'] = 2
    $copy['metric_units'] = [pscustomobject]$canonicalUnits
    return [pscustomobject]$copy
}
