from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def _read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def test_rag_and_upstream_routes_are_bridged_to_tauri_commands():
    app_js = _read("app/static/app.js")
    lib_rs = _read("desktop/src-tauri/src/lib.rs")

    expected_routes = {
        "\"/health\"": "api_health",
        "\"/api/strategies\"": "api_strategies",
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


def test_fastapi_audit_has_no_unreplaced_routes():
    audit = _read("docs/fastapi-to-tauri-audit.md")

    forbidden = [
        "| None |",
        "Not migrated",
        "Legacy Python-only |",
        "Partially replaced",
        "Retired; no current frontend caller",
        "not needed by desktop app",
    ]
    for marker in forbidden:
        assert marker not in audit

    assert "api_health" in audit
    assert "api_strategies" in audit


def test_audit_lists_every_legacy_fastapi_route():
    routes_py = _read("app/api/routes.py")
    main_py = _read("app/main.py")
    audit = _read("docs/fastapi-to-tauri-audit.md")

    legacy_routes = ["GET /health"]
    for line in main_py.splitlines():
        if '@app.get("/")' in line:
            legacy_routes.append("GET /")
    for line in routes_py.splitlines():
        marker = line.strip()
        if not marker.startswith("@router."):
            continue
        method = marker.split(".", 1)[1].split("(", 1)[0].upper()
        path = marker.split("\"", 2)[1]
        legacy_routes.append(f"{method} /api{path}")

    for route in legacy_routes:
        assert f"`{route}`" in audit
