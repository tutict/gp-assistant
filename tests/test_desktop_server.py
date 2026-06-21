import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from app import desktop_server


class DesktopServerRuntimePathTests(unittest.TestCase):
    def test_configure_runtime_paths_uses_absolute_cache_paths(self):
        managed_keys = [
            "GP_CACHE_DIR",
            "TDX_CACHE",
            "TDX_FUNDAMENTAL_CACHE",
            "EASTMONEY_CACHE",
            "AKSHARE_CACHE",
            "GP_NEWS_CACHE",
            "GP_CAPITAL_CACHE",
            "GP_RAG_PACK_PATH",
            "GP_UPSTREAM_RAG_ROOT",
        ]
        with tempfile.TemporaryDirectory() as temp_dir:
            data_root = str(Path(temp_dir) / "data")
            with patch.dict(os.environ, {"GP_ASSISTANT_DATA_ROOT": data_root}, clear=False):
                for key in managed_keys:
                    os.environ.pop(key, None)

                desktop_server._configure_runtime_paths()

                self.assertEqual(Path(os.environ["GP_ASSISTANT_DATA_ROOT"]), Path(data_root).resolve())
                for key in managed_keys:
                    self.assertTrue(Path(os.environ[key]).is_absolute(), f"{key} should be absolute")
                self.assertEqual(Path(os.environ["TDX_CACHE"]).name, "tdx_stocks.csv")
                self.assertEqual(Path(os.environ["GP_CAPITAL_CACHE"]).name, "capital_evidence.sqlite")


if __name__ == "__main__":
    unittest.main()