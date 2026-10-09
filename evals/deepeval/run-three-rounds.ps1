param(
    [switch]$Validate,
    [switch]$Run
)

$ErrorActionPreference = 'Stop'
if (-not $Validate -and -not $Run) { throw 'Use -Validate or -Run.' }

$EvalRoot = $PSScriptRoot
$Runner = Join-Path $EvalRoot 'runner.py'
$EnvFile = Join-Path $EvalRoot '.env'
$PolicyFile = Join-Path $EvalRoot 'judge_policy.json'
$ManifestFile = Join-Path $EvalRoot 'frozen_manifest.json'
$ResultsFile = Join-Path $EvalRoot 'frozen_public_results.jsonl'
$PythonExe = Join-Path $EvalRoot '.venv/Scripts/python.exe'

function Read-DotEnv([string]$Path) {
    $values = @{}
    foreach ($line in Get-Content -LiteralPath $Path -Encoding UTF8) {
        $trimmed = $line.Trim()
        if ([string]::IsNullOrWhiteSpace($trimmed) -or $trimmed.StartsWith('#')) { continue }
        if ($trimmed.StartsWith('export ')) { $trimmed = $trimmed.Substring(7).TrimStart() }
        $parts = $trimmed.Split('=', 2)
        if ($parts.Count -ne 2) { continue }
        $key = $parts[0].Trim()
        $value = $parts[1].Trim().Trim('"').Trim("'")
        $values[$key] = $value
    }
    return $values
}

function Assert-Config {
    foreach ($path in @($EnvFile, $Runner, $PolicyFile, $ManifestFile, $ResultsFile)) {
        if (-not (Test-Path -LiteralPath $path)) { throw "Required evaluation file is missing: $path" }
    }
    if (-not (Test-Path -LiteralPath $PythonExe)) { throw "DeepEval virtual environment is missing: $PythonExe. Install it with python -m pip install -e evals/deepeval." }
    $proxyValues = @($env:ALL_PROXY, $env:all_proxy, $env:HTTP_PROXY, $env:http_proxy, $env:HTTPS_PROXY, $env:https_proxy)
    $socksProxyConfigured = $proxyValues | Where-Object { -not [string]::IsNullOrWhiteSpace($_) -and $_ -match "^(?i)socks" }
    if ($socksProxyConfigured) {
        & $PythonExe -c "import importlib.util,sys; sys.exit(0 if importlib.util.find_spec('socksio') else 1)" | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "SOCKS proxy is configured but the DeepEval environment lacks socksio. Install the declared evaluation dependencies first." }
    }
    $envValues = Read-DotEnv $EnvFile
    $base = [string]$envValues['DEEPEVAL_JUDGE_BASE_URL']
    $key = [string]$envValues['DEEPEVAL_JUDGE_API_KEY']
    $uri = $null
    if (-not [uri]::TryCreate($base, [UriKind]::Absolute, [ref]$uri)) { throw 'DEEPEVAL_JUDGE_BASE_URL is not an absolute URL.' }
    if ($uri.Scheme -notin @('http', 'https') -or -not [string]::IsNullOrEmpty($uri.UserInfo) -or -not [string]::IsNullOrEmpty($uri.Query) -or -not [string]::IsNullOrEmpty($uri.Fragment)) { throw 'DEEPEVAL_JUDGE_BASE_URL must be a clean http(s) URL without credentials/query/fragment.' }
    if ([string]::IsNullOrWhiteSpace($key) -or $key -match '^(PASTE_|YOUR_|REPLACE_)') { throw 'DEEPEVAL_JUDGE_API_KEY is missing or still a placeholder.' }
    $policy = Get-Content -LiteralPath $PolicyFile -Raw -Encoding UTF8 | ConvertFrom-Json
    $runnerText = Get-Content -LiteralPath $Runner -Raw -Encoding UTF8
    $runnerModel = [regex]::Match($runnerText, 'FIXED_JUDGE_MODEL\s*=\s*"([^"]+)"').Groups[1].Value
    if ($runnerModel -ne [string]$policy.judge_model) { throw "Runner model and policy model differ." }
    if ($runnerText -notmatch 'DeepSeekModel') { throw 'DeepSeekModel adapter is not present.' }
    [pscustomobject]@{ config = 'ok'; endpoint_host = $uri.Host; endpoint_path = $uri.AbsolutePath; api_key = 'present'; judge_model = [string]$policy.judge_model; temperature = [int]$policy.judge_temperature; frozen_results = 'present'; network_called = $false } | ConvertTo-Json -Compress
}

Assert-Config | Write-Output
if ($Validate) { exit 0 }

$report = Join-Path ([System.IO.Path]::GetTempPath()) ('gp-assistant-deepeval-' + [DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss') + '.json')
$stdout = "$report.stdout.log"
$stderr = "$report.stderr.log"
$env:PYTHONIOENCODING = 'utf-8'
$env:PYTHONUTF8 = '1'
$env:DEEPEVAL_TELEMETRY_OPT_OUT = 'YES'
$env:DEEPEVAL_PER_TASK_TIMEOUT_SECONDS_OVERRIDE = '600'
$env:DEEPEVAL_PER_ATTEMPT_TIMEOUT_SECONDS_OVERRIDE = '180'
$env:DEEPEVAL_TASK_GATHER_BUFFER_SECONDS_OVERRIDE = '60'
$process = Start-Process -FilePath $PythonExe -ArgumentList @($Runner, '--mode', 'report', '--repetitions', '3', '--output', $report) -WorkingDirectory $EvalRoot -WindowStyle Hidden -Wait -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
if (-not (Test-Path -LiteralPath $report)) { [pscustomobject]@{ run = 'failed'; exit_code = $process.ExitCode; report = 'not-created'; logs = 'captured-in-temp' } | ConvertTo-Json -Compress; exit 1 }
$reportSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $report).Hash.ToLowerInvariant()
$result = Get-Content -LiteralPath $report -Raw -Encoding UTF8 | ConvertFrom-Json
$metricRows = @()
foreach ($runItem in $result.deepeval.runs) { foreach ($caseItem in $runItem.cases) { foreach ($metricItem in $caseItem.metrics) { if ($null -ne $metricItem.score) { $metricRows += [pscustomobject]@{ name = $metricItem.name; score = [double]$metricItem.score } } } } }
$summary = @()
foreach ($group in ($metricRows | Group-Object name)) { $scores = @($group.Group | ForEach-Object { $_.score }); $summary += [pscustomobject]@{ name = $group.Name; samples = $scores.Count; mean = [math]::Round((($scores | Measure-Object -Average).Average), 4); min = [math]::Round((($scores | Measure-Object -Minimum).Minimum), 4); max = [math]::Round((($scores | Measure-Object -Maximum).Maximum), 4) } }
[pscustomobject]@{ run = 'completed'; exit_code = $process.ExitCode; judge_model = $result.judge_model; temperature = $result.judge_temperature; repetitions = $result.deepeval.runs.Count; cases_per_run = @($result.deepeval.runs | ForEach-Object { $_.cases.Count }); metrics = $summary; report_path = $report; report_sha256 = $reportSha256; results_sha256 = [string]$result.results_sha256 } | ConvertTo-Json -Depth 6
exit $process.ExitCode
