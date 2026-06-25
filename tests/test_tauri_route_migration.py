from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def _read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def test_rag_and_upstream_routes_are_bridged_to_tauri_commands():
    app_js = _read("app/static/app.js")
    lib_rs = _read("desktop/src-tauri/src/lib.rs")

    expected_routes = {
        "\"/api/rag-pack/status\"": "api_rag_pack_status",
        "\"/api/rag-pack/build\"": "api_rag_pack_build",
        "\"/api/rag-pack/build-from-news-cache\"": "api_rag_pack_build_from_news_cache",
        "\"/api/rag-pack/query\"": "api_rag_pack_query",
        "\"/api/upstream-rag/status\"": "api_upstream_rag_status",
        "\"/api/upstream-rag/build\"": "api_upstream_rag_build",
        "\"/api/upstream-rag/transfer/start\"": "api_upstream_rag_transfer_start",
    }
    for route, command in expected_routes.items():
        assert route in app_js
        assert command in app_js
        assert command in lib_rs

    assert "RAG 同步包构建尚未迁移" not in app_js
    assert "hasInlinePayload" in app_js
    assert "descriptor_json" in app_js


def test_tauri_data_maintenance_script_replaces_default_python_script_path():
    script = _read("scripts/maintain-data-tauri.ps1")
    readme = _read("README.md")
    audit = _read("docs/fastapi-to-tauri-audit.md")

    assert "app.services" not in script
    assert "maintain_data.py" not in script
    assert "com.tutict.stockoptimizer" in script
    assert "scripts\\maintain-data-tauri.ps1" in readme
    assert "scripts/maintain-data-tauri.ps1" in audit
    assert "Local `/api/rag-pack/*` vector pack | Replaced" in audit
    assert "Desktop upstream RAG build and inline transfer | Replaced" in audit
