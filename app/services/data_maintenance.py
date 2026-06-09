from __future__ import annotations

import os
from datetime import date, datetime, time, timedelta, timezone
from functools import lru_cache
from pathlib import Path
from typing import Iterable

from app.providers.base import get_provider
from app.schemas import AutoRefreshResult, CachePolicy, CachePruneResult, DataCacheStatus, DataRefreshResult

CACHE_DIR = Path(os.getenv("GP_CACHE_DIR", "data/cache"))
UNIVERSE_CACHE_FILES = {
    "tdx": Path(os.getenv("TDX_CACHE", str(CACHE_DIR / "tdx_stocks.csv"))),
}
CHINA_TZ = timezone(timedelta(hours=8))
AUTO_REFRESH_CLOSE_TIME = time(15, 30)


def data_source_status(source: str, policy: CachePolicy | None = None) -> DataCacheStatus:
    policy = policy or CachePolicy()
    normalized_source = _normalize_source(source)
    cache_path = UNIVERSE_CACHE_FILES.get(normalized_source)
    cache_bytes = _directory_size(CACHE_DIR)
    universe_count = _universe_count(normalized_source, cache_path)
    updated_at, age_hours = _file_age(cache_path)
    stale = cache_path is None or not cache_path.exists() or (age_hours or 0) > 24
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

    try:
        provider = get_provider(normalized_source, refresh=True)
        count = len(provider.list_stocks())
        notes.append(f"Refreshed {count} stocks from {normalized_source}.")
    except Exception as exc:
        status = data_source_status(normalized_source, policy)
        notes.append(f"Refresh failed: {exc}")
        return DataRefreshResult(source=normalized_source, refreshed=False, status=status, notes=notes)

    status = data_source_status(normalized_source, policy)
    if policy.auto_prune:
        prune_result = prune_cache(normalized_source, policy)
        if prune_result.removed_files:
            notes.append(f"Pruned {prune_result.removed_files} cache files.")
        status = prune_result.status
    return DataRefreshResult(source=normalized_source, refreshed=True, status=status, notes=notes)


def auto_refresh_universe_after_close(
    source: str,
    policy: CachePolicy | None = None,
    now: datetime | None = None,
) -> AutoRefreshResult:
    policy = policy or CachePolicy()
    normalized_source = _normalize_source(source)
    checked_at = _china_now(now)
    status = data_source_status(normalized_source, policy)
    trading_day, calendar_note = _is_a_share_trading_day(checked_at.date())
    after_close = checked_at.time() >= AUTO_REFRESH_CLOSE_TIME
    notes: list[str] = []
    if calendar_note:
        notes.append(calendar_note)

    if not trading_day:
        notes.append("今天不是 A 股交易日，已跳过交易日自动刷新。")
        return _auto_refresh_result(
            normalized_source,
            checked_at,
            trading_day=False,
            after_close=after_close,
            due=False,
            refreshed=False,
            status=status,
            notes=notes,
        )

    if not after_close:
        notes.append("尚未到北京时间 15:30，盘中不自动刷新基础股票池。")
        return _auto_refresh_result(
            normalized_source,
            checked_at,
            trading_day=True,
            after_close=False,
            due=False,
            refreshed=False,
            status=status,
            notes=notes,
        )

    if _universe_cache_refreshed_after_close(normalized_source, checked_at):
        notes.append("今天收盘后已经刷新过基础股票池。")
        return _auto_refresh_result(
            normalized_source,
            checked_at,
            trading_day=True,
            after_close=True,
            due=False,
            refreshed=False,
            status=status,
            notes=notes,
        )

    refresh_result = refresh_universe(normalized_source, policy)
    notes.extend(refresh_result.notes)
    if refresh_result.refreshed:
        notes.insert(0, "交易日收盘后自动刷新基础股票池已完成。")
    else:
        notes.insert(0, "交易日收盘后自动刷新基础股票池失败，可稍后重试或手动刷新。")
    return _auto_refresh_result(
        normalized_source,
        checked_at,
        trading_day=True,
        after_close=True,
        due=True,
        refreshed=refresh_result.refreshed,
        status=refresh_result.status,
        notes=notes,
    )


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
    value = (source or os.getenv("STOCK_PROVIDER", "tdx")).strip().lower()
    if value in {"tdx", "astock", "akshare", "eastmoney"}:
        return "tdx"
    return "tdx"


def _universe_count(source: str, cache_path: Path | None) -> int:
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
    if source == "tdx":
        notes = ["通达信数据源用于股票池、昨收价、日线、分钟线和盘口；本地股票池缓存用于减少全市场枚举耗时。"]
        if cache_path is None or not cache_path.exists():
            notes.append("No local stock universe cache yet. Refresh the universe before full-market screening.")
            return notes
        notes.append(f"Local stock universe has {universe_count} rows.")
        if stale:
            notes.append("股票池缓存已超过 24 小时，建议刷新。")
        return notes
    if cache_path is None or not cache_path.exists():
        return ["No local stock universe cache yet. Refresh the universe before full-market screening."]
    notes = [f"Local stock universe has {universe_count} rows."]
    if stale:
        notes.append("股票池缓存已超过 24 小时，建议刷新。")
    return notes


def _auto_refresh_result(
    source: str,
    checked_at: datetime,
    *,
    trading_day: bool,
    after_close: bool,
    due: bool,
    refreshed: bool,
    status: DataCacheStatus,
    notes: list[str],
) -> AutoRefreshResult:
    return AutoRefreshResult(
        source=source,
        checked_at=checked_at.isoformat(timespec="seconds"),
        trading_day=trading_day,
        after_close=after_close,
        due=due,
        refreshed=refreshed,
        status=status,
        notes=notes,
    )


def _china_now(now: datetime | None = None) -> datetime:
    value = now or datetime.now(CHINA_TZ)
    if value.tzinfo is None:
        return value.replace(tzinfo=CHINA_TZ)
    return value.astimezone(CHINA_TZ)


def _universe_cache_refreshed_after_close(source: str, checked_at: datetime) -> bool:
    cache_path = UNIVERSE_CACHE_FILES.get(source)
    if cache_path is None or not cache_path.exists():
        return False
    cutoff = datetime.combine(checked_at.date(), AUTO_REFRESH_CLOSE_TIME, tzinfo=CHINA_TZ)
    modified = datetime.fromtimestamp(cache_path.stat().st_mtime, tz=timezone.utc).astimezone(CHINA_TZ)
    return modified >= cutoff


def _is_a_share_trading_day(day: date) -> tuple[bool, str | None]:
    try:
        return day in _a_share_trading_days(), None
    except Exception as exc:
        return False, f"A 股交易日历不可用，已跳过自动刷新：{exc}"


@lru_cache(maxsize=1)
def _a_share_trading_days() -> frozenset[date]:
    import akshare as ak

    frame = ak.tool_trade_date_hist_sina()
    if "trade_date" not in frame.columns:
        raise RuntimeError("AkShare 交易日历缺少 trade_date 字段")
    dates = {_parse_trade_date(value) for value in frame["trade_date"].dropna()}
    return frozenset(value for value in dates if value is not None)


def _parse_trade_date(value) -> date | None:
    if isinstance(value, datetime):
        return value.date()
    if isinstance(value, date):
        return value
    text = str(value).strip()
    if not text:
        return None
    text = text.split(" ")[0]
    for fmt in ("%Y-%m-%d", "%Y%m%d"):
        try:
            return datetime.strptime(text, fmt).date()
        except ValueError:
            continue
    return None


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
