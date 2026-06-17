from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def test_sidecar_build_collects_akshare_runtime_data():
    script = (ROOT / "scripts" / "build-tauri-sidecar.ps1").read_text(encoding="utf-8")

    assert '"--collect-all",' in script
    assert '"akshare",' in script


def test_requirements_include_socks_proxy_support():
    requirements = (ROOT / "requirements.txt").read_text(encoding="utf-8").lower()

    assert "pysocks" in requirements or "requests[socks]" in requirements
