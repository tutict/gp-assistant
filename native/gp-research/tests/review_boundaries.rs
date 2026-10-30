use gp_research::*;
use rusqlite::Connection;
#[cfg(feature = "cli")]
use sha2::{Digest, Sha256};
#[cfg(feature = "cli")]
use std::process::Command;
use std::{
    fs,
    path::{Path, PathBuf},
};

fn temp() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "gp-review-boundary-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}
fn legacy(path: &Path, extra: &str) -> PathBuf {
    fs::create_dir_all(path.join("stock-research")).unwrap();
    let db = path.join("stock-research/research.sqlite");
    let conn = Connection::open(&db).unwrap();
    conn.execute_batch("PRAGMA user_version=1;CREATE TABLE datasets(dataset_id TEXT PRIMARY KEY,record TEXT NOT NULL);CREATE TABLE reports(run_id TEXT PRIMARY KEY,summary TEXT NOT NULL,report TEXT NOT NULL);").unwrap();
    conn.execute_batch(extra).unwrap();
    drop(conn);
    db
}
#[cfg(feature = "cli")]
fn cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_stock-research"))
        .args(args)
        .output()
        .unwrap()
}
#[cfg(feature = "cli")]
fn envelope(output: &std::process::Output) -> serde_json::Value {
    let stdout = std::str::from_utf8(&output.stdout).unwrap();
    assert_eq!(
        stdout.lines().count(),
        1,
        "stdout must contain one JSON envelope: {stdout}"
    );
    serde_json::from_str(stdout).unwrap()
}

#[test]
fn review_v1_foreign_permitted_tables_preserve_exact_bytes() {
    let cases=["CREATE TABLE parameter_sets(unrelated_secret TEXT);INSERT INTO parameter_sets VALUES('keep me');","CREATE TABLE active_parameters(singleton INTEGER PRIMARY KEY,parameter_id TEXT NOT NULL);","CREATE TABLE reports_copy(secret TEXT);","PRAGMA application_id=1196446258;"];
    let mut failures = Vec::new();
    for extra in cases {
        let path = temp();
        let db = legacy(&path, extra);
        let before = fs::read(&db).unwrap();
        if ResearchStore::open_readonly(&path).is_ok() {
            failures.push(format!("readonly adopted {extra}"));
        }
        #[cfg(feature = "cli")]
        {
            let output = cli(&[
                "rollback",
                "--app-data-dir",
                path.to_str().unwrap(),
                "--parameters",
                "fixed-v1",
            ]);
            let value = envelope(&output);
            if output.status.code() != Some(2) || value["ok"] != false {
                failures.push(format!("CLI adopted {extra}: {value}"));
            }
        }
        if ResearchStore::open(&path).is_ok() {
            failures.push(format!("writer adopted {extra}"));
        }
        if fs::read(&db).unwrap() != before {
            failures.push(format!("database bytes changed for {extra}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
#[test]
fn review_v2_wrong_keys_reject_open_and_existing_mutations() {
    let cases=[("parameter_sets","CREATE TABLE parameter_sets(id TEXT,payload TEXT NOT NULL,dataset_id TEXT NOT NULL,core TEXT NOT NULL,research TEXT NOT NULL,fingerprint TEXT NOT NULL);"),("calibration_receipts","CREATE TABLE calibration_receipts(run_id TEXT,payload TEXT NOT NULL);"),("test_locks","CREATE TABLE test_locks(dataset_id TEXT NOT NULL,test_start TEXT NOT NULL,test_end TEXT NOT NULL,signature TEXT NOT NULL,parameters TEXT NOT NULL,split TEXT NOT NULL,PRIMARY KEY(dataset_id));"),("qualifications","CREATE TABLE qualifications(parameter_id TEXT,run_id TEXT NOT NULL);"),("active_parameters","CREATE TABLE active_parameters(singleton INTEGER PRIMARY KEY,parameter_id TEXT NOT NULL);")];
    let mut failures = Vec::new();
    for (table, ddl) in cases {
        let path = temp();
        let writer = ResearchStore::open(&path).unwrap();
        let db = writer.root().join("research.sqlite");
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(&format!("DROP TABLE {table};{ddl}"))
            .unwrap();
        let before = fs::read(&db).unwrap();
        let wal = PathBuf::from(format!("{}-wal", db.display()));
        let before_wal = fs::read(&wal).unwrap();
        if writer.rollback("fixed-v1").is_ok() {
            failures.push(format!("existing writer adopted wrong {table} constraints"));
        }
        if ResearchStore::open_readonly(&path).is_ok() {
            failures.push(format!("readonly adopted wrong {table} constraints"));
        }
        if ResearchStore::open(&path).is_ok() {
            failures.push(format!("fresh writer adopted wrong {table} constraints"));
        }
        if fs::read(&db).unwrap() != before || fs::read(&wal).unwrap() != before_wal {
            failures.push(format!("database/WAL bytes changed for {table}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
#[test]
fn review_valid_v1_readonly_and_migration_keep_old_report() {
    let path = temp();
    let db = legacy(&path, "");
    let conn = Connection::open(&db).unwrap();
    let summary = serde_json::json!({"run_id":"old","dataset_id":"","kind":"evaluation","status":"completed","parameter_set_id":null});
    conn.execute(
        "INSERT INTO reports VALUES('old',?,?)",
        rusqlite::params![summary.to_string(), "{\"run_id\":\"old\",\"legacy\":true}"],
    )
    .unwrap();
    drop(conn);
    let before = fs::read(&db).unwrap();
    let readonly = ResearchStore::open_readonly(&path).unwrap();
    assert_eq!(readonly.report("old").unwrap()["legacy"], true);
    assert_eq!(
        readonly
            .resolve_parameters(None)
            .unwrap()
            .parameters
            .parameter_set_id,
        "fixed-v1"
    );
    drop(readonly);
    assert_eq!(fs::read(&db).unwrap(), before);
    let writer = ResearchStore::open(&path).unwrap();
    assert_eq!(writer.report("old").unwrap()["legacy"], true);
    writer.rollback("fixed-v1").unwrap();
    drop(writer);
    let conn = Connection::open(&db).unwrap();
    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        2
    );
    assert_eq!(
        conn.query_row("PRAGMA application_id", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1196446258
    );
}

#[test]
fn review_v1_joined_column_tokens_preserve_exact_database_bytes() {
    let cases=["DROP TABLE datasets;CREATE TABLE datasets(dataset_id TEXT PRIMARY KEY,recordTEXT NOT NULL);","DROP TABLE reports;CREATE TABLE reports(run_id TEXT PRIMARY KEY,summaryTEXT NOT NULL,report TEXT NOT NULL);"];
    let mut failures = Vec::new();
    for ddl in cases {
        let path = temp();
        let db = legacy(&path, ddl);
        let before = fs::read(&db).unwrap();
        if ResearchStore::open_readonly(&path).is_ok() {
            failures.push(format!("readonly accepted joined tokens: {ddl}"));
        }
        #[cfg(feature = "cli")]
        {
            let output = cli(&[
                "rollback",
                "--app-data-dir",
                path.to_str().unwrap(),
                "--parameters",
                "fixed-v1",
            ]);
            let value = envelope(&output);
            if output.status.code() != Some(2) || value["ok"] != false {
                failures.push(format!("CLI accepted joined tokens: {value}"));
            }
        }
        if ResearchStore::open(&path).is_ok() {
            failures.push(format!("writer accepted joined tokens: {ddl}"));
        }
        if fs::read(&db).unwrap() != before {
            failures.push(format!("changed bytes for joined tokens: {ddl}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
#[test]
fn review_v2_joined_column_tokens_preserve_database_and_wal_bytes() {
    let cases=[("parameter_sets","CREATE TABLE parameter_sets(idTEXT PRIMARY KEY,payload TEXT NOT NULL,dataset_id TEXT NOT NULL,core TEXT NOT NULL,research TEXT NOT NULL,fingerprint TEXT NOT NULL);"),("calibration_receipts","CREATE TABLE calibration_receipts(run_id TEXT PRIMARY KEY,payloadTEXT NOT NULL);"),("test_locks","CREATE TABLE test_locks(dataset_id TEXT NOT NULL,test_start TEXT NOT NULL,test_end TEXT NOT NULL,signatureTEXT NOT NULL,parameters TEXT NOT NULL,split TEXT NOT NULL,PRIMARY KEY(dataset_id,test_start,test_end));"),("qualifications","CREATE TABLE qualifications(parameter_id TEXT PRIMARY KEY,run_idTEXT NOT NULL);"),("active_parameters","CREATE TABLE active_parameters(singleton INTEGER PRIMARY KEY CHECK(singleton=1),parameter_idTEXT NOT NULL);")];
    let mut failures = Vec::new();
    for (table, ddl) in cases {
        let path = temp();
        let writer = ResearchStore::open(&path).unwrap();
        let db = writer.root().join("research.sqlite");
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(&format!("DROP TABLE {table};{ddl}"))
            .unwrap();
        let before = fs::read(&db).unwrap();
        let wal = PathBuf::from(format!("{}-wal", db.display()));
        let before_wal = fs::read(&wal).unwrap();
        if writer.rollback("fixed-v1").is_ok() {
            failures.push(format!("existing writer accepted joined {table}"));
        }
        if ResearchStore::open_readonly(&path).is_ok() {
            failures.push(format!("readonly accepted joined {table}"));
        }
        #[cfg(feature = "cli")]
        {
            let output = cli(&[
                "rollback",
                "--app-data-dir",
                path.to_str().unwrap(),
                "--parameters",
                "fixed-v1",
            ]);
            if output.status.code() != Some(2) || envelope(&output)["ok"] != false {
                failures.push(format!("CLI accepted joined {table}"));
            }
        }
        if ResearchStore::open(&path).is_ok() {
            failures.push(format!("fresh writer accepted joined {table}"));
        }
        if fs::read(&db).unwrap() != before || fs::read(&wal).unwrap() != before_wal {
            failures.push(format!("changed database/WAL bytes for joined {table}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
#[test]
fn review_schema_spacing_case_and_if_not_exists_are_harmless() {
    let path = temp();
    let db=legacy(&path,"DROP TABLE datasets; DROP TABLE reports; create table if not exists datasets ( dataset_id text primary key , record text not null ); create table reports ( run_id text primary key , summary text not null , report text not null );");
    let before = fs::read(&db).unwrap();
    let reader = ResearchStore::open_readonly(&path).unwrap();
    assert_eq!(
        reader
            .resolve_parameters(None)
            .unwrap()
            .parameters
            .parameter_set_id,
        "fixed-v1"
    );
    drop(reader);
    assert_eq!(fs::read(&db).unwrap(), before);
    let writer = ResearchStore::open(&path).unwrap();
    writer.rollback("fixed-v1").unwrap();
}

#[cfg(feature = "cli")]
fn fixture_copy() -> PathBuf {
    let dst = temp();
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tiny");
    for item in fs::read_dir(src).unwrap() {
        let item = item.unwrap();
        fs::copy(item.path(), dst.join(item.file_name())).unwrap();
    }
    dst
}
#[cfg(feature = "cli")]
fn rehash(bundle: &Path, name: &str) {
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(bundle.join("manifest.json")).unwrap()).unwrap();
    let hash = Sha256::digest(fs::read(bundle.join(name)).unwrap())
        .iter()
        .map(|v| format!("{v:02x}"))
        .collect::<String>();
    for file in manifest["files"].as_array_mut().unwrap() {
        if file["path"] == name {
            file["sha256"] = serde_json::json!(hash);
        }
    }
    fs::write(
        bundle.join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
}
#[cfg(feature = "cli")]
#[test]
fn review_cli_invalid_inputs_exit_two_but_io_exits_one() {
    let mut failures = Vec::new();
    for case in [
        "hash",
        "identity",
        "calendar",
        "manifest",
        "csv",
        "units",
        "bar",
        "numeric",
        "required",
        "duplicate",
        "timestamp",
        "foreign-key",
    ] {
        let bundle = fixture_copy();
        let app = temp();
        match case {
            "hash" => {
                fs::write(bundle.join("securities.csv"), b"altered").unwrap();
            }
            "identity" => {
                let s = fs::read_to_string(bundle.join("securities.csv"))
                    .unwrap()
                    .replacen("000001.SZ", "BAD", 1);
                fs::write(bundle.join("securities.csv"), s).unwrap();
                rehash(&bundle, "securities.csv");
            }
            "calendar" => {
                let mut s = fs::read_to_string(bundle.join("trading_calendar.csv")).unwrap();
                s.push_str("2025-11-03\n");
                fs::write(bundle.join("trading_calendar.csv"), s).unwrap();
                rehash(&bundle, "trading_calendar.csv");
            }
            "manifest" => {
                fs::write(bundle.join("manifest.json"), b"{ invalid json").unwrap();
            }
            "csv" => {
                let mut s = fs::read_to_string(bundle.join("securities.csv")).unwrap();
                s.push_str("000999.SZ,Too,Many,Fields,2015-01-01,EXTRA\n");
                fs::write(bundle.join("securities.csv"), s).unwrap();
                rehash(&bundle, "securities.csv");
            }
            "units" => {
                let mut m: serde_json::Value =
                    serde_json::from_slice(&fs::read(bundle.join("manifest.json")).unwrap())
                        .unwrap();
                m["rate_units"]["roe"] = serde_json::json!("unknown");
                fs::write(bundle.join("manifest.json"), m.to_string()).unwrap();
            }
            "bar" => {
                let mut rows: Vec<String> = fs::read_to_string(bundle.join("daily_bars.csv"))
                    .unwrap()
                    .lines()
                    .map(str::to_owned)
                    .collect();
                let mut cells: Vec<String> = rows[1].split(',').map(str::to_owned).collect();
                cells[4] = "-1".into();
                rows[1] = cells.join(",");
                fs::write(bundle.join("daily_bars.csv"), rows.join("\n") + "\n").unwrap();
                rehash(&bundle, "daily_bars.csv");
            }
            "numeric" => {
                let mut rows: Vec<String> = fs::read_to_string(bundle.join("daily_bars.csv"))
                    .unwrap()
                    .lines()
                    .map(str::to_owned)
                    .collect();
                let mut cells: Vec<String> = rows[1].split(',').map(str::to_owned).collect();
                cells[2] = "NaN".into();
                rows[1] = cells.join(",");
                fs::write(bundle.join("daily_bars.csv"), rows.join("\n") + "\n").unwrap();
                rehash(&bundle, "daily_bars.csv");
            }
            "required" => {
                let mut rows: Vec<String> = fs::read_to_string(bundle.join("securities.csv"))
                    .unwrap()
                    .lines()
                    .map(str::to_owned)
                    .collect();
                let mut cells: Vec<String> = rows[1].split(',').map(str::to_owned).collect();
                cells[1] = "".into();
                rows[1] = cells.join(",");
                fs::write(bundle.join("securities.csv"), rows.join("\n") + "\n").unwrap();
                rehash(&bundle, "securities.csv");
            }
            "duplicate" => {
                let mut s = fs::read_to_string(bundle.join("securities.csv")).unwrap();
                let mut cells: Vec<String> = s
                    .lines()
                    .nth(1)
                    .unwrap()
                    .split(',')
                    .map(str::to_owned)
                    .collect();
                cells[1] = "Conflict".into();
                s.push_str(&cells.join(","));
                s.push('\n');
                fs::write(bundle.join("securities.csv"), s).unwrap();
                rehash(&bundle, "securities.csv");
            }
            "timestamp" => {
                let mut rows: Vec<String> = fs::read_to_string(bundle.join("financial_events.csv"))
                    .unwrap()
                    .lines()
                    .map(str::to_owned)
                    .collect();
                let mut cells: Vec<String> = rows[1].split(',').map(str::to_owned).collect();
                cells[2] = "not-a-timestamp".into();
                rows[1] = cells.join(",");
                fs::write(bundle.join("financial_events.csv"), rows.join("\n") + "\n").unwrap();
                rehash(&bundle, "financial_events.csv");
            }
            "foreign-key" => {
                let s = fs::read_to_string(bundle.join("daily_bars.csv"))
                    .unwrap()
                    .replacen("000001.SZ", "000999.SZ", 1);
                fs::write(bundle.join("daily_bars.csv"), s).unwrap();
                rehash(&bundle, "daily_bars.csv");
            }
            _ => unreachable!(),
        }
        let output = cli(&[
            "import",
            "--app-data-dir",
            app.to_str().unwrap(),
            "--bundle",
            bundle.to_str().unwrap(),
        ]);
        let value = envelope(&output);
        assert!(!output.stderr.is_empty());
        if output.status.code() != Some(2) || value["ok"] != false {
            failures.push(format!("{case}: exit {:?} {value}", output.status.code()));
        }
    }
    let bundle = temp();
    let app = temp();
    let output = cli(&[
        "import",
        "--app-data-dir",
        app.to_str().unwrap(),
        "--bundle",
        bundle.to_str().unwrap(),
    ]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(envelope(&output)["error"]["code"], "io");
    assert!(!output.stderr.is_empty());
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    let app = temp();
    let store = ResearchStore::open(&app).unwrap();
    let data = store
        .import_bundle(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tiny"))
        .unwrap();
    let conn = Connection::open(store.root().join("research.sqlite")).unwrap();
    conn.execute(
        "UPDATE datasets SET record='{' WHERE dataset_id=?",
        [&data.dataset_id],
    )
    .unwrap();
    drop(conn);
    drop(store);
    let output = cli(&[
        "evaluate",
        "--app-data-dir",
        app.to_str().unwrap(),
        "--dataset",
        &data.dataset_id,
    ]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        envelope(&output)["error"]["code"],
        "json",
        "stored internal evidence JSON must not be classified as supplied bundle JSON"
    );
}
