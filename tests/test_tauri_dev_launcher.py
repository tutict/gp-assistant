import json
import re
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def _read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def _default_batch_args() -> list[str]:
    batch = _read("start-tauri-dev.bat")
    match = re.search(r'^set\s+"DEFAULT_ARGS=([^"]*)"', batch, flags=re.IGNORECASE | re.MULTILINE)
    assert match, "start-tauri-dev.bat should define DEFAULT_ARGS"
    return match.group(1).lower().split()


def test_default_tauri_launcher_does_not_prepare_android_twice():
    config = json.loads(_read("desktop/src-tauri/tauri.conf.json"))
    before_dev = config["build"].get("beforeDevCommand", "").lower()
    args = _default_batch_args()

    assert "prepare:android" in before_dev
    assert "-runpreflight" in args
    assert "-skipprepare" in args, (
        "The default launcher should skip its own prepare step because "
        "tauri dev already runs beforeDevCommand=prepare:android."
    )


def test_one_click_dev_launcher_forwards_skip_prepare_for_tauri_dev():
    script = re.sub(r"\s+", " ", _read("scripts/start-dev.ps1").lower())

    assert "if ($skipprepare -or -not $preflightonly) { $tauriargs += '-skipprepare' }" in script
