import os
import tempfile
import unittest
from datetime import datetime, timezone
from pathlib import Path
from unittest.mock import patch

from app.schemas import CachePolicy, DataRefreshResult
from app.services import data_maintenance as maintenance


class DataMaintenanceAutoRefreshTests(unittest.TestCase):
    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory()
        self.cache_dir = Path(self.temp_dir.name)
        self.cache_path = self.cache_dir / "tdx_stocks.csv"
        self.policy = CachePolicy(auto_prune=False)

    def tearDown(self):
        self.temp_dir.cleanup()

    def test_auto_refresh_skips_non_trading_day(self):
        with self._patched_cache(), patch.object(
            maintenance, "_is_a_share_trading_day", return_value=(False, None)
        ), patch.object(maintenance, "refresh_universe") as refresh:
            result = maintenance.auto_refresh_universe_after_close(
                "tdx",
                self.policy,
                now=datetime(2026, 6, 7, 16, 0, tzinfo=maintenance.CHINA_TZ),
            )

        self.assertFalse(result.due)
        self.assertFalse(result.refreshed)
        self.assertFalse(result.trading_day)
        refresh.assert_not_called()

    def test_auto_refresh_skips_before_close_on_trading_day(self):
        with self._patched_cache(), patch.object(
            maintenance, "_is_a_share_trading_day", return_value=(True, None)
        ), patch.object(maintenance, "refresh_universe") as refresh:
            result = maintenance.auto_refresh_universe_after_close(
                "tdx",
                self.policy,
                now=datetime(2026, 6, 8, 14, 59, tzinfo=maintenance.CHINA_TZ),
            )

        self.assertFalse(result.due)
        self.assertFalse(result.refreshed)
        self.assertTrue(result.trading_day)
        self.assertFalse(result.after_close)
        refresh.assert_not_called()

    def test_auto_refresh_skips_when_cache_refreshed_after_close(self):
        self._write_cache(datetime(2026, 6, 8, 15, 45, tzinfo=maintenance.CHINA_TZ))

        with self._patched_cache(), patch.object(
            maintenance, "_is_a_share_trading_day", return_value=(True, None)
        ), patch.object(maintenance, "refresh_universe") as refresh:
            result = maintenance.auto_refresh_universe_after_close(
                "tdx",
                self.policy,
                now=datetime(2026, 6, 8, 16, 0, tzinfo=maintenance.CHINA_TZ),
            )

        self.assertFalse(result.due)
        self.assertFalse(result.refreshed)
        self.assertTrue(result.after_close)
        refresh.assert_not_called()

    def test_auto_refresh_runs_after_close_when_cache_is_old(self):
        self._write_cache(datetime(2026, 6, 8, 14, 0, tzinfo=maintenance.CHINA_TZ))

        with self._patched_cache(), patch.object(
            maintenance, "_is_a_share_trading_day", return_value=(True, None)
        ):
            status = maintenance.data_source_status("tdx", self.policy)
            refresh_result = DataRefreshResult(
                source="tdx",
                refreshed=True,
                status=status,
                notes=["refreshed"],
            )
            with patch.object(maintenance, "refresh_universe", return_value=refresh_result) as refresh:
                result = maintenance.auto_refresh_universe_after_close(
                    "tdx",
                    self.policy,
                    now=datetime(2026, 6, 8, 16, 0, tzinfo=maintenance.CHINA_TZ),
                )

        self.assertTrue(result.due)
        self.assertTrue(result.refreshed)
        self.assertTrue(result.after_close)
        refresh.assert_called_once()

    def test_auto_refresh_falls_back_to_weekday_when_calendar_is_unavailable(self):
        self._write_cache(datetime(2026, 6, 8, 14, 0, tzinfo=maintenance.CHINA_TZ))

        with self._patched_cache(), patch.object(
            maintenance, "_a_share_trading_days", side_effect=RuntimeError("calendar offline")
        ):
            status = maintenance.data_source_status("tdx", self.policy)
            refresh_result = DataRefreshResult(
                source="tdx",
                refreshed=True,
                status=status,
                notes=["refreshed"],
            )
            with patch.object(maintenance, "refresh_universe", return_value=refresh_result) as refresh:
                result = maintenance.auto_refresh_universe_after_close(
                    "tdx",
                    self.policy,
                    now=datetime(2026, 6, 8, 16, 0, tzinfo=maintenance.CHINA_TZ),
                )

        self.assertTrue(result.trading_day)
        self.assertTrue(result.due)
        self.assertTrue(result.refreshed)
        self.assertTrue(any("交易日历不可用" in note for note in result.notes))
        refresh.assert_called_once()

    def _patched_cache(self):
        return patch.multiple(
            maintenance,
            CACHE_DIR=self.cache_dir,
            UNIVERSE_CACHE_FILES={"tdx": self.cache_path},
        )

    def _write_cache(self, modified_at: datetime):
        self.cache_path.write_text("code,name\n000001.SZ,Sample\n", encoding="utf-8")
        timestamp = modified_at.astimezone(timezone.utc).timestamp()
        os.utime(self.cache_path, (timestamp, timestamp))
