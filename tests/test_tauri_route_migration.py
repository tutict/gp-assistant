from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def _read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")

def test_python_web_entry_uses_react_build_output():
    main_py = _read("app/main.py")

    assert "FRONTEND_DIST_DIR = ROOT_DIR / \"desktop\" / \"mobile-dist\"" in main_py
    assert "FRONTEND_INDEX = FRONTEND_DIST_DIR / \"index.html\"" in main_py
    assert "app.mount(\"/assets\"" in main_py
    assert "app.mount(\"/vendor\"" in main_py
    assert "BASE_DIR / \"static\"" not in main_py
    assert "app.mount(\"/static\"" not in main_py


def test_react_entry_loads_qr_decoder_from_public_vendor():
    index_html = _read("desktop/frontend/index.html")
    news_panel = _read("desktop/frontend/src/components/panels/NewsRagPanel.tsx")

    assert "<script src=\"/vendor/jsQR.js\"></script>" in index_html
    assert "jsQR" in news_panel
    assert "scanQrCode" in news_panel

def test_rag_and_upstream_routes_are_bridged_to_tauri_commands():
    bridge_ts = _read("desktop/frontend/src/lib/tauri.ts")
    news_panel = _read("desktop/frontend/src/components/panels/NewsRagPanel.tsx")
    lib_rs = _read("desktop/src-tauri/src/lib.rs")

    expected_routes = {
        '"/health"': "api_health",
        '"/api/strategies"': "api_strategies",
        '"/api/rag-pack/status"': "api_rag_pack_status",
        '"/api/rag-pack/build"': "api_rag_pack_build",
        '"/api/rag-pack/build-from-news-cache"': "api_rag_pack_build_from_news_cache",
        '"/api/rag-pack/query"': "api_rag_pack_query",
        '"/api/upstream-rag/status"': "api_upstream_rag_status",
        '"/api/upstream-rag/build"': "api_upstream_rag_build",
        '"/api/upstream-rag/transfer/start"': "api_upstream_rag_transfer_start",
        '"/api/upstream-rag/mobile/list"': "core_upstream_rag_list",
        '"/api/upstream-rag/mobile/detail"': "core_upstream_rag_detail",
        '"/api/upstream-rag/mobile/import"': "core_upstream_rag_import",
        '"/api/upstream-rag/mobile/rollback"': "core_upstream_rag_rollback",
    }
    for route, command in expected_routes.items():
        assert route in bridge_ts
        assert command in bridge_ts
        assert command in lib_rs

    assert "RAG 同步包构建尚未迁移" not in bridge_ts
    assert "MOBILE_API_HANDLERS" not in bridge_ts
    assert "descriptor_json" in news_panel


def test_react_tauri_bridge_has_mobile_observe_and_cache_maintenance_paths():
    bridge_ts = _read("desktop/frontend/src/lib/tauri.ts")
    lib_rs = _read("desktop/src-tauri/src/lib.rs")

    bridge_tokens = [
        "api_observe",
        "mobile_fast_observe",
        "MOBILE_OBSERVE_INVOKE_TIMEOUT_MS",
        "loadMobileFinancialSnapshotForCode",
        "fetchObserveDailyHistoryForTauri",
        "api_market_refresh",
        "refreshTauriMarketData",
        "validateTauriMarketCache",
        "core_mobile_market_data_read",
        "core_validate_data_source",
        "core_mobile_stock_skill",
    ]
    for token in bridge_tokens:
        assert token in bridge_ts

    rust_commands = [
        "api_observe",
        "api_market_refresh",
        "core_mobile_market_data_read",
        "core_validate_data_source",
        "core_mobile_stock_skill",
    ]
    for command in rust_commands:
        assert command in lib_rs


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
        path = marker.split('"', 2)[1]
        legacy_routes.append(f"{method} /api{path}")

    for route in legacy_routes:
        assert f"`{route}`" in audit
