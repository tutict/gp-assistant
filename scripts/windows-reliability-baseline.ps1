[CmdletBinding()]
param(
  [string]$ExePath = "",
  [int]$Runs = 3,
  [int]$ObserveSeconds = 30,
  [switch]$SkipPowerReport
)
$ErrorActionPreference = 'Stop'
$base = 'C:\tmp'
$root = Join-Path $base ('guxuanyou-reliability-' + [guid]::NewGuid().ToString('N'))
$temp = Join-Path $root 'Temp'
$roaming = Join-Path $root 'Roaming'
$local = Join-Path $root 'Local'
New-Item -ItemType Directory -Force -Path $temp,$roaming,$local | Out-Null
if (-not $ExePath) { $ExePath = @(Get-ChildItem -LiteralPath $env:LOCALAPPDATA -Directory -ErrorAction SilentlyContinue | ForEach-Object { $candidate = Join-Path $_.FullName 'stock-optimizer-desktop.exe'; if (Test-Path -LiteralPath $candidate) { $candidate } } | Select-Object -First 1) }; if (-not $ExePath -or -not (Test-Path -LiteralPath $ExePath)) { throw "Installed executable not found" }
function Assert-IsolatedPath([string]$Path) {
  $full = [IO.Path]::GetFullPath($Path)
  $prefix = [IO.Path]::GetFullPath($base).TrimEnd('\') + '\'
  if (-not $full.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) { throw "Refusing operation outside C:\tmp: $full" }
}
function New-ProcessInfo([string]$ProfileRoot) {
  $psi = [Diagnostics.ProcessStartInfo]::new()
  $psi.FileName = $ExePath
  $psi.WorkingDirectory = Split-Path -Parent $ExePath
  $psi.UseShellExecute = $false
  $psi.Environment['APPDATA'] = Join-Path $ProfileRoot 'Roaming'
  $psi.Environment['LOCALAPPDATA'] = Join-Path $ProfileRoot 'Local'
  $psi.Environment['TEMP'] = Join-Path $ProfileRoot 'Temp'
  $psi.Environment['TMP'] = Join-Path $ProfileRoot 'Temp'
  $psi.Environment['GP_ASSISTANT_SAFE_START'] = '1'
  return $psi
}
function Start-Isolated([string]$ProfileRoot, [int]$seconds) {
  $timer = [Diagnostics.Stopwatch]::StartNew()
  $p = [Diagnostics.Process]::Start((New-ProcessInfo $ProfileRoot))
  $samples = [Collections.Generic.List[object]]::new()
  $firstWindow = $null
  $peakWorking = 0L; $peakPrivate = 0L; $peakThreads = 0
  $lastCpu = 0.0
  while ($timer.Elapsed.TotalSeconds -lt $seconds) {
    Start-Sleep -Milliseconds 250
    try { $p.Refresh() } catch { break }
    if ($p.HasExited) { break }
    $working = [int64]$p.WorkingSet64; $private = [int64]$p.PrivateMemorySize64
    if ($working -gt $peakWorking) { $peakWorking = $working }
    if ($private -gt $peakPrivate) { $peakPrivate = $private }
    $threads = $p.Threads.Count; if ($threads -gt $peakThreads) { $peakThreads = $threads }
    if ($null -eq $firstWindow -and $p.MainWindowHandle -ne [IntPtr]::Zero) { $firstWindow = $timer.ElapsedMilliseconds }
    try { $lastCpu = $p.TotalProcessorTime.TotalSeconds } catch { }
    if ($samples.Count -lt 240) {
      $samples.Add([pscustomobject]@{ms=$timer.ElapsedMilliseconds; working_set=$working; private_bytes=$private; cpu_seconds=[math]::Round($lastCpu,3); threads=$threads})
    }
  }
  try { $p.Refresh() } catch { }
  $exited = $p.HasExited
  if (-not $exited) { $p.CloseMainWindow() | Out-Null; if (-not $p.WaitForExit(5000)) { $p.Kill(); $p.WaitForExit() } }
  [pscustomobject]@{
    pid=$p.Id; exited=$exited; first_window_ms=$firstWindow; peak_working_set_bytes=$peakWorking; peak_private_bytes=$peakPrivate; peak_threads=$peakThreads; final_cpu_seconds=[math]::Round($lastCpu,3); samples=$samples
  }
}
function Start-Killed([string]$ProfileRoot, [int]$afterMs) {
  $p = [Diagnostics.Process]::Start((New-ProcessInfo $ProfileRoot))
  Start-Sleep -Milliseconds $afterMs
  $p.Refresh(); $processId = $p.Id; if (-not $p.HasExited) { $p.Kill(); $p.WaitForExit() }
  [pscustomobject]@{pid=$processId; killed_after_ms=$afterMs; exit_code=$p.ExitCode}
}
function Find-DatabaseFiles([string]$ProfileRoot) {
  @(Get-ChildItem -LiteralPath $ProfileRoot -Recurse -File -Include '*.sqlite','*.json' -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName })
}
$results = [ordered]@{
  generated_at=(Get-Date).ToString('o'); exe=$ExePath; exe_hash=(Get-FileHash -Algorithm SHA256 -LiteralPath $ExePath).Hash; profile_root=$root
  battery_before=@(Get-CimInstance Win32_Battery -ErrorAction SilentlyContinue | Select-Object Name,EstimatedChargeRemaining,BatteryStatus)
  thermal_before=@(Get-CimInstance MSAcpi_ThermalZoneTemperature -ErrorAction SilentlyContinue | Select-Object CurrentTemperature,InstanceName)
  cold_starts=@(); crash_restart=@(); databases_before_fault=@(); write_denied=$null; power_report=$null
}
for ($i=1; $i -le $Runs; $i++) {
  $profile = Join-Path $root ('cold-' + $i)
  New-Item -ItemType Directory -Force -Path (Join-Path $profile 'Temp'),(Join-Path $profile 'Roaming'),(Join-Path $profile 'Local') | Out-Null
  $results.cold_starts += Start-Isolated $profile $ObserveSeconds
}
$crashProfile = Join-Path $root 'crash-restart'
New-Item -ItemType Directory -Force -Path (Join-Path $crashProfile 'Temp'),(Join-Path $crashProfile 'Roaming'),(Join-Path $crashProfile 'Local') | Out-Null
foreach ($ms in @(500,1500,3000,6000)) {
  $results.crash_restart += Start-Killed $crashProfile $ms
  $restart = Start-Isolated $crashProfile 8
  $results.crash_restart += [pscustomobject]@{restart_after_kill_ms=$ms; restart_result=$restart}
}
$results.databases_before_fault = Find-DatabaseFiles $crashProfile
$faultProfile = Join-Path $root 'write-denied'
New-Item -ItemType Directory -Force -Path (Join-Path $faultProfile 'Temp'),(Join-Path $faultProfile 'Roaming'),(Join-Path $faultProfile 'Local') | Out-Null
$initial = Start-Isolated $faultProfile 8
$results.write_denied = [ordered]@{initial=$initial; profile=$faultProfile}
Assert-IsolatedPath $faultProfile
& icacls.exe $faultProfile /inheritance:r /grant:r "$env:USERNAME:(OI)(CI)(RX)" | Out-Null
try { $results.write_denied.read_only_launch = Start-Isolated $faultProfile 10 } finally { & icacls.exe $faultProfile /reset /T /C | Out-Null }
if (-not $SkipPowerReport) {
  $power = Join-Path $root 'powercfg'
  New-Item -ItemType Directory -Force -Path $power | Out-Null
  $energy = Join-Path $power 'energy.html'
  & powercfg.exe /energy /duration 60 /output $energy /xml | Out-Null
  $results.power_report = [ordered]@{path=$energy; exists=(Test-Path -LiteralPath $energy)}
}
$results.battery_after=@(Get-CimInstance Win32_Battery -ErrorAction SilentlyContinue | Select-Object Name,EstimatedChargeRemaining,BatteryStatus)
$results.thermal_after=@(Get-CimInstance MSAcpi_ThermalZoneTemperature -ErrorAction SilentlyContinue | Select-Object CurrentTemperature,InstanceName)
$output = Join-Path $root 'windows-reliability.json'
$results | ConvertTo-Json -Depth 12 | Set-Content -Encoding utf8 -LiteralPath $output
Write-Output "RESULT=$output"
Write-Output ($results | ConvertTo-Json -Depth 4)
