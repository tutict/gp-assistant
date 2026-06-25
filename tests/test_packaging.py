from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def _read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8").lower()


def test_default_dev_launcher_uses_tauri_without_python_runtime():
    batch = _read("start-dev.bat")
    script = _read("scripts/start-dev.ps1")

    assert "start-tauri-dev.bat" in batch
    assert "start-tauri-dev.ps1" in script
    assert "uvicorn" not in batch
    assert "uvicorn" not in script
    assert "pip install" not in batch
    assert "pip install" not in script


def test_release_check_is_tauri_rust_only_by_default():
    script = _read("scripts/release-check.ps1")

    assert "native/gp-core/cargo.toml" in script
    assert "desktop/src-tauri/cargo.toml" in script
    assert "prepare-tauri-android-assets.ps1" in script
    assert "compileall" not in script
    assert "pytest" not in script
    assert "requirements.txt" not in script
