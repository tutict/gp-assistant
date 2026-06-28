from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def _read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8").lower()


def test_default_dev_launcher_keeps_tauri_default_and_explicit_web_mode():
    batch = _read("start-dev.bat")
    script = _read("scripts/start-dev.ps1")

    assert "scripts\\start-dev.ps1" in batch
    assert "default mode: tauri desktop" in batch
    assert "start-tauri-dev.ps1" in script
    assert "return 'tauri'" in script
    assert "return 'web'" in script
    assert "8010" in script
    assert "uvicorn" in script
    assert "stop-process" in script
    assert "$prefix-dev.latest.log" in script
    assert "pip install" not in batch
    assert "pip install" not in script


def test_start_test_batch_points_to_scripts_directory():
    batch = _read("start-test.bat")

    assert "scripts\\start-test.ps1" in batch
    assert "scriptsstart-test.ps1" not in batch


def test_release_check_is_tauri_rust_only_by_default():
    script = _read("scripts/release-check.ps1")

    assert "native/gp-core/cargo.toml" in script
    assert "desktop/src-tauri/cargo.toml" in script
    assert "prepare-tauri-android-assets.ps1" in script
    assert "compileall" not in script
    assert "pytest" not in script
    assert "requirements.txt" not in script
