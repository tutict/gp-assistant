[CmdletBinding()]
param(
    [ValidateSet('status', 'prune')]
    [string]$Action = 'status',
    [int]$MaxMb = 200,
    [string]$AppDataDir
)

$ErrorActionPreference = 'Stop'

function Resolve-AppDataDir {
    param([string]$Override)
    if (-not [string]::IsNullOrWhiteSpace($Override)) {
        return [System.IO.Path]::GetFullPath($Override)
    }
    if ([string]::IsNullOrWhiteSpace($env:APPDATA)) {
        throw 'APPDATA is not set; pass -AppDataDir explicitly.'
    }
    return (Join-Path $env:APPDATA 'com.tutict.stockoptimizer')
}

function Get-FileInfoJson {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        return @{ exists = $false; path = $Path; bytes = 0 }
    }
    $item = Get-Item -LiteralPath $Path
    return @{
        exists = $true
        path = $item.FullName
        bytes = [int64]$item.Length
        updated_at = $item.LastWriteTime.ToString('o')
    }
}

function Measure-TreeBytes {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path)) { return 0 }
    return [int64]((Get-ChildItem -LiteralPath $Path -Recurse -File -ErrorAction SilentlyContinue | Measure-Object -Property Length -Sum).Sum)
}

$root = Resolve-AppDataDir $AppDataDir
$marketPath = Join-Path $root 'market\mobile-market-data.json'
$newsPath = Join-Path $root 'news\news-cache.json'
$externalDataDirectory = Join-Path $root 'external-data'
$externalDataPath = Join-Path $externalDataDirectory 'external-data.sqlite'
$ragPath = Join-Path $root 'rag\rag-pack.json'
$upstreamDesktopPath = Join-Path $root 'upstream_rag_desktop'
$upstreamMobilePath = Join-Path $root 'upstream_rag_mobile'

if ($Action -eq 'prune') {
    $removed = @()
}

$market = Get-FileInfoJson $marketPath
$news = Get-FileInfoJson $newsPath
$externalData = Get-FileInfoJson $externalDataPath
$externalDataWal = Get-FileInfoJson ($externalDataPath + '-wal')
$externalDataShm = Get-FileInfoJson ($externalDataPath + '-shm')
$externalDataBytes = [int64]$externalData.bytes + [int64]$externalDataWal.bytes + [int64]$externalDataShm.bytes
$rag = Get-FileInfoJson $ragPath
$result = @{
    runtime = 'tauri-appdata'
    action = $Action
    app_data_dir = $root
    policy = @{ mode = 'light'; max_bytes = [int64]$MaxMb * 1024 * 1024 }
    caches = @{
        market = $market
        news = $news
        external_data = @{
            database = $externalData
            wal = $externalDataWal
            shared_memory = $externalDataShm
            bytes = $externalDataBytes
            warning_threshold_bytes = [int64]200 * 1024 * 1024
            capacity_warning = $externalDataBytes -ge ([int64]200 * 1024 * 1024)
            cleanup = 'managed by the app after preview and explicit confirmation; excluded from this script prune action'
        }
        rag_pack = $rag
        upstream_rag_desktop = @{ exists = (Test-Path -LiteralPath $upstreamDesktopPath); path = $upstreamDesktopPath; bytes = Measure-TreeBytes $upstreamDesktopPath }
        upstream_rag_mobile = @{ exists = (Test-Path -LiteralPath $upstreamMobilePath); path = $upstreamMobilePath; bytes = Measure-TreeBytes $upstreamMobilePath }
    }
    cache_bytes = [int64]$market.bytes + [int64]$news.bytes + $externalDataBytes + [int64]$rag.bytes + (Measure-TreeBytes $upstreamDesktopPath) + (Measure-TreeBytes $upstreamMobilePath)
    notes = @(
        'This script inspects the Tauri app-data caches without the retired service stack.',
        'Prune is inspection-only so legacy recovery files are preserved; use the app preview and confirmation flow to clean selected external data domains.',
        'The external-data database is rebuildable provider cache and is excluded from encrypted user backups.'
    )
}
if ($Action -eq 'prune') { $result.removed = $removed }

$result | ConvertTo-Json -Depth 8
