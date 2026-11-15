use crate::market::cache::cached_external_market_data_entry;
use crate::market::cache::forget_mobile_market_data_cache;
use crate::market::cache::remember_external_market_data_cache;
use crate::market::cache::versions::external_market_data_cache_version;
use crate::market::store::files::MOBILE_MARKET_DATA_FILE;
use crate::market_domain::datasets::parse_mobile_market_data_snapshot;
use crate::market_domain::dates::epoch_millis;
use crate::network::http::build_http_client_with_proxy;
use crate::network::proxy::proxy_from_payload;
use crate::network::proxy::proxy_mode_from_payload;
use crate::wire_values::sanitize_path_part;
use serde_json::json;
use std::fs;
use std::time::Duration;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

#[test]
fn configured_network_proxy_takes_precedence_without_replacing_provider_proxy_settings() {
    let payload = json!({
        "proxy_mode": "manual",
        "proxy_url": "http://llm-proxy.local:9000",
        "network_proxy_mode": "manual",
        "network_proxy_url": "http://market-proxy.local:7890"
    });

    assert_eq!(proxy_mode_from_payload(Some(&payload)), "manual");
    assert_eq!(
        proxy_from_payload(Some(&payload)).as_deref(),
        Some("http://market-proxy.local:7890")
    );
}

#[test]
fn configured_network_proxy_rejects_embedded_authentication() {
    let payload = json!({
        "network_proxy_mode": "manual",
        "network_proxy_url": "http://user:secret@127.0.0.1:7890"
    });
    let result = build_http_client_with_proxy(
        "proxy validation test",
        Duration::from_secs(1),
        Some(&payload),
    );

    assert!(
        result.is_err(),
        "manual proxy URLs must not contain credentials"
    );
}

#[test]
fn configured_network_proxy_routes_requests_through_the_selected_proxy() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind local proxy fixture");
    let address = listener.local_addr().expect("read local proxy address");
    let proxy = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept proxied request");
        let mut buffer = [0_u8; 2048];
        let read = stream.read(&mut buffer).expect("read proxy request");
        let request = String::from_utf8_lossy(&buffer[..read]).to_string();
        let body = b"ok";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream
            .write_all(response.as_bytes())
            .expect("write proxy headers");
        stream.write_all(body).expect("write proxy body");
        request
    });
    let payload = json!({
        "network_proxy_mode":"manual",
        "network_proxy_url":format!("http://{address}"),
    });
    let (status, body) = tauri::async_runtime::block_on(async {
        let client = build_http_client_with_proxy(
            "local HK proxy integration test",
            Duration::from_secs(2),
            Some(&payload),
        )
        .expect("build proxied client");
        let response = client
            .get("http://stocks.example.test/probe")
            .send()
            .await
            .expect("request should reach local proxy");
        let status = response.status();
        let body = response.text().await.expect("read proxy response");
        (status, body)
    });
    assert!(status.is_success());
    assert_eq!(body, "ok");
    assert!(proxy
        .join()
        .expect("proxy fixture should finish")
        .starts_with("GET http://stocks.example.test/probe HTTP/1.1"));
}

#[test]
fn sanitize_path_part_keeps_dot_only_components_inside_cache_root() {
    let root = std::env::temp_dir().join(format!(
        "gp-assistant-safe-rag-path-{}-{}",
        std::process::id(),
        epoch_millis()
    ));
    let cache_root = root.join("upstream-rag");
    fs::create_dir_all(&cache_root).unwrap();
    let canonical_cache_root = fs::canonicalize(&cache_root).unwrap();

    for input in [".", "..", "...", "....", "../outside", "..\\outside"] {
        let component = sanitize_path_part(input);
        assert!(
            !matches!(component.as_str(), "." | ".."),
            "{input:?} must not remain a special path component"
        );
        let candidate = cache_root.join(component);
        fs::create_dir_all(&candidate).unwrap();
        let canonical_candidate = fs::canonicalize(&candidate).unwrap();
        assert!(
            canonical_candidate.starts_with(&canonical_cache_root),
            "{input:?} escaped the cache root to {}",
            canonical_candidate.display()
        );
    }

    assert_eq!(sanitize_path_part("600000.SH"), "600000.SH");
    assert_eq!(sanitize_path_part("v1.2.3"), "v1.2.3");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn cache_version_tracks_sqlite_and_legacy_source_changes() {
    let root = std::env::temp_dir().join(format!(
        "gp-assistant-market-cache-version-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    let legacy_path = root.join(MOBILE_MARKET_DATA_FILE);
    let database_path = root.join("external-data.sqlite");
    fs::write(&legacy_path, b"legacy-cache-v1").unwrap();
    {
        let connection = rusqlite::Connection::open(&database_path).unwrap();
        connection
            .execute_batch("CREATE TABLE version_fixture (value TEXT NOT NULL);")
            .unwrap();
        connection
            .execute(
                "INSERT INTO version_fixture(value) VALUES (?1)",
                ["sqlite-v1"],
            )
            .unwrap();
    }
    let data = json!({"stocks": [], "relations": []});
    let (typed, summary) = parse_mobile_market_data_snapshot(&data, "test market data")
        .expect("empty market snapshot should parse");

    let initial = external_market_data_cache_version(&database_path, &legacy_path, None)
        .expect("external store should have a cache version");
    remember_external_market_data_cache(
        &legacy_path,
        initial.clone(),
        &data,
        typed.clone(),
        &summary,
    );
    assert!(cached_external_market_data_entry(&legacy_path, &initial).is_some());

    {
        let connection = rusqlite::Connection::open(&database_path).unwrap();
        connection
            .execute(
                "UPDATE version_fixture SET value=?1",
                rusqlite::params!["sqlite-v2-".repeat(1_000)],
            )
            .unwrap();
    }
    let changed_database = external_market_data_cache_version(&database_path, &legacy_path, None)
        .expect("updated external store should have a cache version");
    assert_ne!(changed_database, initial);
    assert!(cached_external_market_data_entry(&legacy_path, &changed_database).is_none());
    remember_external_market_data_cache(
        &legacy_path,
        changed_database.clone(),
        &data,
        typed,
        &summary,
    );

    let original_modified = fs::metadata(&legacy_path).unwrap().modified().unwrap();
    fs::write(&legacy_path, b"legacy-cache-v2").unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(&legacy_path)
        .unwrap()
        .set_modified(original_modified)
        .unwrap();
    let changed_legacy = external_market_data_cache_version(&database_path, &legacy_path, None)
        .expect("updated legacy cache should have a cache version");
    assert_ne!(changed_legacy, changed_database);
    assert!(cached_external_market_data_entry(&legacy_path, &changed_legacy).is_none());

    forget_mobile_market_data_cache(&legacy_path);
    fs::remove_dir_all(root).unwrap();
}
