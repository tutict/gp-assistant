"""ctypes bridge to the native ``gp-core`` cdylib.

This lets the Python test-suite call the exact same Rust core that the mobile
(Tauri/Android) build ships, so we can pin Python<->Rust parity. No PyO3 or
build-time codegen is required: the crate already exposes a stable C ABI
(``gp_core_*_json`` + ``gp_core_free_string``).

If the cdylib has not been built, :func:`core_available` returns ``False`` and
parity tests skip with a build hint instead of failing.
"""

from __future__ import annotations

import ctypes
import json
from pathlib import Path
from typing import Any

_REPO_ROOT = Path(__file__).resolve().parent.parent
_TARGET_DIR = _REPO_ROOT / "native" / "gp-core" / "target"

# Build with: cargo build --release --manifest-path native/gp-core/Cargo.toml
BUILD_HINT = "cargo build --release --manifest-path native/gp-core/Cargo.toml"

_LIB_NAMES = ("gp_core.dll", "libgp_core.so", "libgp_core.dylib")

_CORE_FUNCTIONS = (
    "gp_core_screen_with_data_json",
    "gp_core_graph_screen_with_data_json",
    "gp_core_backtest_with_data_json",
    "gp_core_trend_with_data_json",
    "gp_core_trend_screen_with_data_json",
)


def _find_library() -> Path | None:
    for profile in ("release", "debug"):
        for name in _LIB_NAMES:
            candidate = _TARGET_DIR / profile / name
            if candidate.exists():
                return candidate
    return None


_LIB_PATH = _find_library()
_lib: ctypes.CDLL | None = None


def core_available() -> bool:
    return _LIB_PATH is not None


def _load() -> ctypes.CDLL:
    global _lib
    if _lib is not None:
        return _lib
    if _LIB_PATH is None:
        raise RuntimeError(f"gp-core cdylib not built. Build it with: {BUILD_HINT}")
    lib = ctypes.CDLL(str(_LIB_PATH))
    lib.gp_core_free_string.argtypes = [ctypes.c_void_p]
    lib.gp_core_free_string.restype = None
    for fn_name in _CORE_FUNCTIONS:
        fn = getattr(lib, fn_name)
        fn.argtypes = [ctypes.c_char_p]
        fn.restype = ctypes.c_void_p
    _lib = lib
    return lib


def call(fn_name: str, payload: Any) -> Any:
    """Call a ``gp_core_*_json`` function and return the unwrapped ``data``.

    Raises ``RuntimeError`` if the Rust core reports ``ok: false``.
    """

    lib = _load()
    fn = getattr(lib, fn_name)
    encoded = json.dumps(payload, ensure_ascii=False).encode("utf-8")
    ptr = fn(encoded)
    if not ptr:
        raise RuntimeError(f"{fn_name} returned a null pointer")
    try:
        raw = ctypes.string_at(ptr).decode("utf-8")
    finally:
        lib.gp_core_free_string(ptr)
    envelope = json.loads(raw)
    if not envelope.get("ok"):
        raise RuntimeError(f"{fn_name} failed: {envelope.get('error')}")
    return envelope.get("data")


def core_dataset_from_provider(provider, *, histories: dict[str, Any] | None = None) -> dict:
    """Serialize a Python ``StockProvider`` into a Rust ``CoreDataSet`` dict.

    Field names already line up between the Python ``StockItem``/``StockRelation``
    pydantic models and the Rust structs, so this is a direct dump. ``histories``
    maps stock code -> list of bar dicts (``date/open/high/low/close/volume/capital``).
    """

    return {
        "stocks": [stock.model_dump() for stock in provider.list_stocks()],
        "relations": [relation.model_dump() for relation in provider.list_relations()],
        "histories": histories or {},
    }
