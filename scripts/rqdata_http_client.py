import io
import os
from typing import Any, Dict, Optional

import pandas as pd
import requests


class RQDataHttpClient:
    def __init__(
        self,
        username: Optional[str] = None,
        password: Optional[str] = None,
        token: Optional[str] = None,
        base_url: str = "https://rqdata.ricequant.com",
        timeout: int = 30,
    ):
        self.username = username or os.getenv("RQDATA_USERNAME")
        self.password = password or os.getenv("RQDATA_PASSWORD")
        self._token = token or os.getenv("RQDATA_TOKEN")
        self.base_url = base_url.rstrip("/")
        self.timeout = timeout

    def auth(self) -> str:
        if self._token:
            return self._token
        if not self.username or not self.password:
            raise RuntimeError("Missing RQDATA_USERNAME / RQDATA_PASSWORD")
        url = f"{self.base_url}/auth"
        payload = {"user_name": self.username, "password": self.password}
        resp = requests.post(url, json=payload, timeout=self.timeout)
        resp.raise_for_status()
        token = resp.text.strip().strip('"')
        if not token:
            raise RuntimeError("Failed to obtain token")
        self._token = token
        return token

    def call(self, method: str, **params: Any) -> pd.DataFrame:
        token = self.auth()
        url = f"{self.base_url}/api"
        payload: Dict[str, Any] = {"method": method}
        payload.update({k: v for k, v in params.items() if v is not None})
        headers = {"token": token}
        resp = requests.post(url, json=payload, headers=headers, timeout=self.timeout)
        resp.raise_for_status()
        text = resp.text.strip()
        if not text:
            return pd.DataFrame()
        try:
            return pd.read_csv(io.StringIO(text))
        except Exception as exc:
            raise RuntimeError(f"Failed to parse CSV response: {text[:200]}") from exc
