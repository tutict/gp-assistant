from __future__ import annotations

import os
from datetime import datetime, timezone
from pathlib import Path
from typing import Iterable

from app.providers.base import get_provider
from app.schemas import CachePolicy, CachePruneResult, DataCacheStatus, DataRefreshResult

CACHE_DIR = Path(os.getenv("GP_CACHE_DIR", "data/cache"))
UNIVERSE_CACHE_FILES = {
    "akshare": Path(os.getenv("AKSHARE_CACHE", str(CACHE_DIR / "stocks.csv"))),
    "eastmoney": Path(os.getenv("EASTMONEY_CACHE", str(CACHE_DIR / "eastmoney_stocks.csv"))),
}


def data_source_status(source: str, policy: CachePolicy | None = None) -> DataCacheStatus:
    policy = policy or CachePolicy()
    normalized_source = _normalize_source(source)
    cache_path = UNIVERSE_CACHE_FILES.get(normalized_source)
    cache_bytes = _directory_size(CACHE_DIR)
    universe_count = _universe_count(normalized_source, cache_path)
    updated_at, age_hours = _file_age(cache_path)
    stale = normalized_source != "mock" and (cache_path is None or not cache_path.exists() or (age_hours or 0) > 24)
    notes = _status_notes(normalized_source, cache_path, universe_count, stale)

    return DataCacheStatus(
        source=normalized_source,
        cache_dir=str(CACHE_DIR),
        cache_bytes=cache_bytes,
        cache_limit_bytes=policy.max_bytes,
        cache_usage=round(cache_bytes / policy.max_bytes, 6) if policy.max_bytes else 0,
        universe_count=universe_count,
        universe_cache_path=str(cache_path) if cache_path else None,
        universe_updated_at=updated_at,
        universe_age_hours=age_hours,
        stale=stale,
        policy=policy,
        notes=notes,
    )


def refresh_universe(source: str, policy: CachePolicy | None = None) -> DataRefreshResult:
    policy = policy or CachePolicy()
    normalized_source = _normalize_source(source)
    notes: list[str] = []
    refreshed_source = normalized_source

    try:
        provider = get_provider(normalized_source, refresh=True)
        count = len(provider.list_stocks())
        notes.append(f"Refreshed {count} stocks from {normalized_source}.")
    except Exception as exc:
        if normalized_source == "akshare":
            notes.append(f"AkShare 刷新失败，已回退到东方财富：{exc}")
            refreshed_source = "eastmoney"
            try:
                provider = get_provider("eastmoney", refresh=True)
                count = len(provider.list_stocks())
                notes.append(f"Refreshed {count} stocks from eastmoney.")
            except Exception as fallback_exc:
                status = data_source_status(refreshed_source, policy)
                notes.append(f"东方财富回退也失败：{fallback_exc}")
                return DataRefreshResult(source=refreshed_source, refreshed=False, status=status, notes=notes)
        else:
            status = data_source_status(normalized_source, policy)
            notes.append(f"Refresh failed: {exc}")
            return DataRefreshResult(source=normalized_source, refreshed=False, status=status, notes=notes)

    status = data_source_status(refreshed_source, policy)
    if policy.auto_prune:
        prune_result = prune_cache(refreshed_source, policy)
        if prune_result.removed_files:
            notes.append(f"Pruned {prune_result.removed_files} cache files.")
        status = prune_result.status
    return DataRefreshResult(source=refreshed_source, refreshed=True, status=status, notes=notes)


def prune_cache(source: str, policy: CachePolicy | None = None) -> CachePruneResult:
    policy = policy or CachePolicy()
    CACHE_DIR.mkdir(parents=True, exist_ok=True)
    protected = {path.resolve() for path in UNIVERSE_CACHE_FILES.values()}
    candidates = [
        path
        for path in _cache_files(CACHE_DIR)
        if path.resolve() not in protected and path.is_file()
    ]
    candidates.sort(key=lambda path: path.stat().st_mtime)

    removed_files = 0
    removed_bytes = 0
    current_bytes = _directory_size(CACHE_DIR)
    target_bytes = int(policy.max_bytes * 0.85)

    for path in candidates:
        if current_bytes <= target_bytes:
            break
        size = path.stat().st_size
        path.unlink(missing_ok=True)
        current_bytes -= size
        removed_files += 1
        removed_bytes += size

    status = data_source_status(source, policy)
    notes = ["股票池缓存会被保留，只清理可丢弃的历史行情和分钟线缓存。"]
    if not removed_files:
        notes.append("No disposable cache files needed pruning.")
    return CachePruneResult(
        removed_files=removed_files,
        removed_bytes=removed_bytes,
        status=status,
        notes=notes,
    )


def _normalize_source(source: str | None) -> str:
    value = (source or os.getenv("STOCK_PROVIDER", "mock")).strip().lower()
    return value if value in {"mock", "akshare", "eastmoney"} else "mock"


def _universe_count(source: str, cache_path: Path | None) -> int:
    if source == "mock":
        return len(get_provider("mock").list_stocks())
    if cache_path is None or not cache_path.exists():
        return 0
    try:
        with cache_path.open("r", encoding="utf-8", errors="ignore") as handle:
            return max(sum(1 for _ in handle) - 1, 0)
    except OSError:
        return 0


def _file_age(path: Path | None) -> tuple[str | None, float | None]:
    if path is None or not path.exists():
        return None, None
    modified = datetime.fromtimestamp(path.stat().st_mtime, tz=timezone.utc)
    age_hours = (datetime.now(timezone.utc) - modified).total_seconds() / 3600
    return modified.astimezone().isoformat(timespec="seconds"), round(age_hours, 2)


def _status_notes(source: str, cache_path: Path | None, universe_count: int, stale: bool) -> list[str]:
    if source == "mock":
        return ["本地演示数据为确定性样本，不使用磁盘缓存。"]
    if cache_path is None or not cache_path.exists():
        return ["No local stock universe cache yet. Refresh the universe before full-market screening."]
    notes = [f"Local stock universe has {universe_count} rows."]
    if stale:
        notes.append("股票池缓存已超过 24 小时，建议刷新。")
    return notes


def _cache_files(root: Path) -> Iterable[Path]:
    if not root.exists():
        return []
    return (path for path in root.rglob("*") if path.is_file())


def _directory_size(root: Path) -> int:
    if not root.exists():
        return 0
    total = 0
    for path in root.rglob("*"):
        if path.is_file():
            try:
                total += path.stat().st_size
            except OSError:
                continue
    return total
