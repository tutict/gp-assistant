param(
    [string] $SourcePath = "",
    [string] $OutputPath = "",
    [string] $OptionsOutputPath = ""
)

$ErrorActionPreference = "Stop"

$root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
if ([string]::IsNullOrWhiteSpace($SourcePath)) {
    $SourcePath = Join-Path $root "data\cache\astock_stocks.csv"
}
if ([string]::IsNullOrWhiteSpace($OutputPath)) {
    $OutputPath = Join-Path $root "desktop\frontend\public\mobile-industry-snapshot.json"
}
if ([string]::IsNullOrWhiteSpace($OptionsOutputPath)) {
    $OptionsOutputPath = Join-Path $root "desktop\frontend\src\lib\screenIndustryOptions.ts"
}

if (-not (Test-Path -LiteralPath $SourcePath)) {
    if ((Test-Path -LiteralPath $OutputPath) -and (Test-Path -LiteralPath $OptionsOutputPath)) {
        Write-Host "Industry source is unavailable; using committed generated assets."
        return
    }
    throw "Industry source is missing and generated assets are unavailable: $SourcePath"
}

$industries = [ordered]@{}
$optionSet = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::Ordinal)
$sourceStockCount = 0
foreach ($row in Import-Csv -LiteralPath $SourcePath -Encoding UTF8) {
    $digits = ([string] $row.f12).Trim()
    $industry = ([string] $row.f100).Trim()
    if ($digits -notmatch '^\d{6}$') {
        continue
    }
    $sourceStockCount += 1
    if ([string]::IsNullOrWhiteSpace($industry) -or $industry -eq '-') { continue }

    $market = if ($digits.StartsWith('6')) {
        'SH'
    } elseif ($digits.StartsWith('4') -or $digits.StartsWith('8') -or $digits.StartsWith('9')) {
        'BJ'
    } else {
        'SZ'
    }
    $industries["$digits.$market"] = $industry
    $null = $optionSet.Add($industry)
}

$options = [string[]] @($optionSet)
[Array]::Sort($options, [System.StringComparer]::Ordinal)
$snapshot = [ordered]@{
    schema_version = "mobile-industry-snapshot/v1"
    source = "data/cache/astock_stocks.csv"
    source_stock_count = $sourceStockCount
    stock_count = $industries.Count
    option_count = $options.Count
    options = $options
    industries = $industries
}

$parent = Split-Path -Parent $OutputPath
New-Item -ItemType Directory -Path $parent -Force | Out-Null
$json = $snapshot | ConvertTo-Json -Depth 5 -Compress
[System.IO.File]::WriteAllText($OutputPath, $json, [System.Text.UTF8Encoding]::new($false))

$optionLines = New-Object System.Collections.Generic.List[string]
$optionLines.Add('export const INDUSTRY_OPTIONS = [') | Out-Null
$optionLines.Add('  "",') | Out-Null
foreach ($option in $options) {
    $escaped = $option.Replace('\', '\\').Replace('"', '\"')
    $optionLines.Add("  `"$escaped`",") | Out-Null
}
$optionLines.Add('] as const;') | Out-Null
$optionsParent = Split-Path -Parent $OptionsOutputPath
New-Item -ItemType Directory -Path $optionsParent -Force | Out-Null
[System.IO.File]::WriteAllLines($OptionsOutputPath, $optionLines, [System.Text.UTF8Encoding]::new($false))

Write-Host "Prepared mobile industry data: $($industries.Count) stocks, $($options.Count) industries"
