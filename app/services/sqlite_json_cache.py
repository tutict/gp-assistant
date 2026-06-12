from __future__ import annotations

import json
import re
import sqlite3
from pathlib import Path
from typing import Any, Mapping, Sequence, TypeVar


T = TypeVar("T")
_IDENTIFIER_PATTERN = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")


class SQLiteJsonCache:
    def __init__(
        self,
        path: Path,
        table: str,
        key_columns: Sequence[str],
        *,
        order_columns: Sequence[str] = (),
    ) -> None:
        if not key_columns:
            raise ValueError("key_columns must not be empty")
        self.path = path
        self.table = _safe_identifier(table)
        self.key_columns = tuple(_safe_identifier(column) for column in key_columns)
        self.order_columns = tuple(_safe_identifier(column) for column in order_columns)

    def load(self, model_type: type[T], keys: Mapping[str, Any]) -> T | None:
        row = self._fetch_payload(keys)
        return _parse_payload(model_type, row["payload"]) if row else None

    def load_latest(
        self,
        model_type: type[T],
        match: Mapping[str, Any],
        *,
        exclude: Mapping[str, Any] | None = None,
    ) -> T | None:
        row = self._fetch_payload(match, exclude=exclude or {}, latest=True)
        return _parse_payload(model_type, row["payload"]) if row else None

    def store(self, keys: Mapping[str, Any], generated_at: str, payload: Any) -> None:
        self._validate_keys(keys)
        self.path.parent.mkdir(parents=True, exist_ok=True)
        values = [str(keys[column]) for column in self.key_columns]
        serialized = _serialize_payload(payload)
        columns = [*self.key_columns, "generated_at", "payload"]
        placeholders = ", ".join("?" for _ in columns)
        conn = self._connect()
        try:
            self._init_db(conn)
            conn.execute(
                f"""
                INSERT OR REPLACE INTO {self.table}
                ({", ".join(columns)})
                VALUES ({placeholders})
                """,
                [*values, generated_at, serialized],
            )
            conn.commit()
        finally:
            conn.close()

    def _fetch_payload(
        self,
        keys: Mapping[str, Any],
        *,
        exclude: Mapping[str, Any] | None = None,
        latest: bool = False,
    ) -> sqlite3.Row | None:
        if not self.path.exists():
            return None
        self._validate_keys(keys, partial=latest)
        clauses = [f"{_safe_identifier(column)} = ?" for column in keys]
        params = [str(value) for value in keys.values()]
        for column, value in (exclude or {}).items():
            clauses.append(f"{_safe_identifier(column)} <> ?")
            params.append(str(value))
        order = ""
        if latest:
            order_columns = self.order_columns or (*self.key_columns, "generated_at")
            order = "ORDER BY " + ", ".join(f"{column} DESC" for column in order_columns)
        conn = self._connect()
        try:
            self._init_db(conn)
            return conn.execute(
                f"""
                SELECT payload
                FROM {self.table}
                WHERE {" AND ".join(clauses)}
                {order}
                LIMIT 1
                """,
                params,
            ).fetchone()
        finally:
            conn.close()

    def _connect(self) -> sqlite3.Connection:
        conn = sqlite3.connect(self.path)
        conn.row_factory = sqlite3.Row
        return conn

    def _init_db(self, conn: sqlite3.Connection) -> None:
        key_definitions = ",\n            ".join(f"{column} TEXT NOT NULL" for column in self.key_columns)
        primary_key = ", ".join(self.key_columns)
        conn.execute(
            f"""
            CREATE TABLE IF NOT EXISTS {self.table} (
                {key_definitions},
                generated_at TEXT NOT NULL,
                payload TEXT NOT NULL,
                PRIMARY KEY ({primary_key})
            )
            """
        )
        conn.commit()

    def _validate_keys(self, keys: Mapping[str, Any], *, partial: bool = False) -> None:
        unknown = set(keys) - set(self.key_columns)
        if unknown:
            raise ValueError(f"unknown cache key columns: {sorted(unknown)}")
        if partial:
            if not keys:
                raise ValueError("at least one cache key is required")
            return
        missing = set(self.key_columns) - set(keys)
        if missing:
            raise ValueError(f"missing cache key columns: {sorted(missing)}")


def _serialize_payload(payload: Any) -> str:
    if hasattr(payload, "model_dump_json"):
        return payload.model_dump_json()
    return json.dumps(payload, ensure_ascii=False)


def _parse_payload(model_type: type[T], payload: str) -> T | None:
    try:
        if hasattr(model_type, "model_validate_json"):
            return model_type.model_validate_json(payload)  # type: ignore[return-value]
        return model_type(**json.loads(payload))  # type: ignore[misc, return-value]
    except Exception:
        return None


def _safe_identifier(value: str) -> str:
    if not _IDENTIFIER_PATTERN.match(value):
        raise ValueError(f"unsafe sqlite identifier: {value!r}")
    return value
