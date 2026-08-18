$ErrorActionPreference = "Stop"

$root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$exporter = Join-Path $PSScriptRoot "export-mobile-industry-snapshot.ps1"
$directory = Join-Path ([System.IO.Path]::GetTempPath()) ("gp-industry-export-" + [guid]::NewGuid().ToString("N"))
$sourcePath = Join-Path $directory "stocks.csv"
$snapshotPath = Join-Path $directory "snapshot.json"
$optionsPath = Join-Path $directory "screenIndustryOptions.ts"

New-Item -ItemType Directory -Path $directory -Force | Out-Null
try {
    [System.IO.File]::WriteAllText(
        $sourcePath,
        "f12,f100`n600000,BetaIndustry`n000001,AlphaIndustry`n",
        [System.Text.UTF8Encoding]::new($false)
    )

    & $exporter -SourcePath $sourcePath -OutputPath $snapshotPath -OptionsOutputPath $optionsPath

    $optionsSource = [System.IO.File]::ReadAllText($optionsPath)
    $requiredFragments = @(
        'export const LEGACY_BROAD_INDUSTRY_OPTIONS = [',
        'export const INDUSTRY_OPTIONS = [',
        'export const ALL_INDUSTRY_OPTIONS = [',
        'export function isLegacyBroadIndustry(value: string): boolean',
        '"AlphaIndustry"',
        '"BetaIndustry"'
    )
    foreach ($fragment in $requiredFragments) {
        if (-not $optionsSource.Contains($fragment)) {
            throw "Generated options are missing required contract fragment: $fragment"
        }
    }

    $snapshot = Get-Content -LiteralPath $snapshotPath -Raw | ConvertFrom-Json
    $actualOptions = @($snapshot.options)
    $expectedOptions = @("AlphaIndustry", "BetaIndustry")
    if (($actualOptions -join "|") -ne ($expectedOptions -join "|")) {
        throw "Unexpected generated snapshot options: $($actualOptions -join ', ')"
    }

    Write-Host "Mobile industry exporter contract test passed."
} finally {
    if (Test-Path -LiteralPath $directory) {
        Remove-Item -LiteralPath $directory -Recurse -Force
    }
}
