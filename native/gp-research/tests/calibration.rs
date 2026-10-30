use gp_research::*;
use std::{path::PathBuf, process::Command};

#[test]
fn immutable_report_json_preserves_exact_f64_evidence() {
    for step in 0..10000u64 {
        let value = f64::from_bits(0x412e848000000000 + step * 7919);
        let encoded = serde_json::to_string(&serde_json::json!({"equity":value})).unwrap();
        let decoded: serde_json::Value = serde_json::from_str(&encoded).unwrap();
        assert_eq!(
            decoded["equity"].as_f64().unwrap().to_bits(),
            value.to_bits(),
            "immutable numeric evidence drift: {encoded}"
        );
    }
}

fn temp() -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "gp-calibration-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&p).unwrap();
    p
}
#[test]
fn six_command_help_is_available() {
    let output = Command::new("cargo")
        .args([
            "run",
            "--quiet",
            "--manifest-path",
            "native/gp-research/Cargo.toml",
            "--features",
            "cli",
            "--bin",
            "stock-research",
            "--",
            "--help",
        ])
        .current_dir(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["ok"], true);
    for c in [
        "import",
        "validate",
        "evaluate",
        "calibrate",
        "promote",
        "rollback",
    ] {
        assert!(value["data"]["usage"].as_str().unwrap().contains(c));
    }
}
#[test]
fn existing_generic_passed_report_has_no_authority() {
    let p = temp();
    let store = ResearchStore::open(&p).unwrap();
    store
        .save_report(
            "calibration",
            &serde_json::json!({"run_id":"forged","dataset_id":"fixture","gate":{"passed":true}}),
        )
        .unwrap();
    assert_eq!(store.report("forged").unwrap()["gate"]["passed"], true);
    assert!(!store.qualification_verified("forged").unwrap());
    assert_eq!(store.promote("forged").unwrap_err().code, "eligibility");
    assert_eq!(
        store
            .resolve_parameters(None)
            .unwrap()
            .parameters
            .parameter_set_id,
        "fixed-v1"
    );
}
#[test]
fn registry_candidate_readonly_idempotence_and_corrupt_pointer() {
    let p = temp();
    let writer = ResearchStore::open(&p).unwrap();
    let r = writer
        .import_bundle(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tiny"))
        .unwrap();
    let reader = ResearchStore::open_readonly(&p).unwrap();
    let mut params = gp_core::fixed_scoring_parameters();
    params.algorithm_version = "adaptive_swing_v2".into();
    params.parameter_set_id = "test-candidate".into();
    writer.register_parameters(&params, &r.dataset_id).unwrap();
    writer.register_parameters(&params, &r.dataset_id).unwrap();
    assert_eq!(
        reader
            .research_parameters("test-candidate")
            .unwrap()
            .dataset_id,
        Some(r.dataset_id.clone())
    );
    assert!(reader.resolve_parameters(Some("test-candidate")).is_err());
    assert_eq!(
        writer.rollback("test-candidate").unwrap_err().code,
        "eligibility"
    );
    let mut changed = params.clone();
    changed.modes.get_mut("range").unwrap().overheat_penalty += 0.01;
    assert_eq!(
        writer
            .register_parameters(&changed, &r.dataset_id)
            .unwrap_err()
            .code,
        "immutable_conflict"
    );
    writer.rollback("fixed-v1").unwrap();
    assert_eq!(
        reader
            .resolve_parameters(None)
            .unwrap()
            .parameters
            .parameter_set_id,
        "fixed-v1"
    );
    let conn = rusqlite::Connection::open(writer.root().join("research.sqlite")).unwrap();
    conn.execute("UPDATE active_parameters SET parameter_id='unknown'", [])
        .unwrap();
    assert!(reader.resolve_parameters(None).is_err());
    assert_eq!(
        reader
            .resolve_parameters(Some("fixed-v1"))
            .unwrap()
            .parameters
            .parameter_set_id,
        "fixed-v1"
    );
    assert_eq!(reader.rollback("fixed-v1").unwrap_err().code, "readonly");
    conn.execute("UPDATE parameter_sets SET research='old'", [])
        .unwrap();
    assert_eq!(
        reader
            .research_parameters("test-candidate")
            .unwrap_err()
            .code,
        "stale_parameters"
    );
}
#[test]
fn readonly_v1_builtin_and_foreign_v2_are_preserved() {
    let p = temp();
    std::fs::create_dir_all(p.join("stock-research")).unwrap();
    let path = p.join("stock-research/research.sqlite");
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("PRAGMA user_version=1;CREATE TABLE datasets(dataset_id TEXT PRIMARY KEY,record TEXT NOT NULL);CREATE TABLE reports(run_id TEXT PRIMARY KEY,summary TEXT NOT NULL,report TEXT NOT NULL);").unwrap();
    drop(conn);
    let before = std::fs::read(&path).unwrap();
    let reader = ResearchStore::open_readonly(&p).unwrap();
    assert_eq!(
        reader
            .resolve_parameters(None)
            .unwrap()
            .parameters
            .parameter_set_id,
        "fixed-v1"
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
    drop(reader);
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("PRAGMA user_version=2;").unwrap();
    drop(conn);
    let before = std::fs::read(&path).unwrap();
    assert!(ResearchStore::open(&p).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
}
#[test]
fn bound_neighbors_omit_illegal_moves_instead_of_aborting_search() {
    let mut p = gp_core::fixed_scoring_parameters();
    p.algorithm_version = "adaptive_swing_v2".into();
    p.parameter_set_id = "bounded".into();
    let m = p.modes.get_mut("range").unwrap();
    *m.weights.get_mut("quality").unwrap() += 0.05;
    *m.weights.get_mut("valuation").unwrap() -= 0.05;
    let neighbors = feasible_neighbors(&p, &["range".into()]).unwrap();
    assert!(!neighbors.is_empty());
    for n in neighbors {
        gp_core::validate_scoring_parameters(&n).unwrap();
        assert_eq!(n.modes["range"].weights["trend"], 0.);
    }
}
#[test]
fn short_fixture_calibration_persists_no_qualification() {
    let p = temp();
    let store = ResearchStore::open(&p).unwrap();
    let dataset = store
        .import_bundle(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tiny"))
        .unwrap();
    let report = calibrate(&store, &dataset.dataset_id, &CalibrationOptions::default()).unwrap();
    assert_eq!(report.status, "insufficient_data");
    assert!(!report.gate.passed);
    assert!(report.test.is_none());
    assert!(!report.issues.is_empty());
    assert_eq!(
        store.report(&report.run_id).unwrap()["status"],
        "insufficient_data"
    );
    assert_eq!(
        calibrate(&store, &dataset.dataset_id, &CalibrationOptions::default())
            .unwrap()
            .run_id,
        report.run_id
    );
    assert!(!store.qualification_verified(&report.run_id).unwrap());
    assert!(store.promote(&report.run_id).is_err());
    let conn = rusqlite::Connection::open(store.root().join("research.sqlite")).unwrap();
    let mut changed = store.report(&report.run_id).unwrap();
    changed["issues"] = serde_json::json!(["tampered but still typed"]);
    conn.execute(
        "UPDATE reports SET report=? WHERE run_id=?",
        rusqlite::params![serde_json::to_string(&changed).unwrap(), report.run_id],
    )
    .unwrap();
    assert_eq!(
        calibrate(&store, &dataset.dataset_id, &CalibrationOptions::default())
            .unwrap_err()
            .code,
        "immutable_conflict"
    );
    assert!(store.promote(&report.run_id).is_err());
    assert_eq!(
        store
            .resolve_parameters(None)
            .unwrap()
            .parameters
            .parameter_set_id,
        "fixed-v1"
    );
}
#[test]
fn orphan_shared_memory_is_explicitly_rejected_by_readers() {
    let p = temp();
    let writer = ResearchStore::open(&p).unwrap();
    let db = writer.root().join("research.sqlite");
    writer.rollback("fixed-v1").unwrap();
    drop(writer);
    let shm = PathBuf::from(format!("{}-shm", db.display()));
    std::fs::write(&shm, b"orphan shared memory evidence").unwrap();
    assert!(matches!(ResearchStore::open_readonly(&p),Err(e) if e.code=="readonly_wal"));
}
#[test]
fn existing_writer_cannot_mutate_a_newer_registry() {
    let p = temp();
    let store = ResearchStore::open(&p).unwrap();
    let conn = rusqlite::Connection::open(store.root().join("research.sqlite")).unwrap();
    conn.execute_batch("PRAGMA user_version=3;").unwrap();
    assert!(matches!(store.rollback("fixed-v1"),Err(e) if e.code=="newer_schema"));
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM active_parameters", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        3
    );
}
#[cfg(feature = "cli")]
#[test]
fn cli_fixture_stdout_stderr_and_exit_contracts() {
    fn cli(args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_stock-research"))
            .args(args)
            .output()
            .unwrap()
    }
    fn parsed(o: &std::process::Output) -> serde_json::Value {
        serde_json::from_slice(&o.stdout).unwrap()
    }
    let p = temp();
    let path = p.to_str().unwrap();
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tiny");
    let o = cli(&[
        "import",
        "--app-data-dir",
        path,
        "--bundle",
        fixture.to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(0));
    assert!(!o.stderr.is_empty());
    let id = parsed(&o)["data"]["dataset_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let o = cli(&["validate", "--app-data-dir", path, "--dataset", &id]);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(parsed(&o)["data"]["passed"], true);
    let o = cli(&["evaluate", "--app-data-dir", path, "--dataset", &id]);
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stdout)
    );
    assert_eq!(parsed(&o)["data"]["data_quality"]["real_data"], false);
    let o = cli(&["calibrate", "--app-data-dir", path, "--dataset", &id]);
    assert_eq!(o.status.code(), Some(2));
    let run = parsed(&o)["data"]["run_id"].as_str().unwrap().to_owned();
    assert_eq!(parsed(&o)["data"]["status"], "insufficient_data");
    let o = cli(&["promote", "--app-data-dir", path, "--run", &run]);
    assert_eq!(o.status.code(), Some(3));
    assert_eq!(parsed(&o)["ok"], false);
    let o = cli(&[
        "rollback",
        "--app-data-dir",
        path,
        "--parameters",
        "fixed-v1",
    ]);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(
        parsed(&o)["data"]["parameters"]["parameter_set_id"],
        "fixed-v1"
    );
    for args in [
        vec!["import", "--bundle", "missing"],
        vec![
            "evaluate",
            "--app-data-dir",
            path,
            "--dataset",
            &id,
            "--typo",
            "1",
        ],
        vec![
            "calibrate",
            "--app-data-dir",
            path,
            "--dataset",
            &id,
            "--cost",
            "10",
        ],
        vec![
            "evaluate",
            "--app-data-dir",
            path,
            "--dataset",
            &id,
            "--cost",
            "NaN",
        ],
    ] {
        let o = cli(&args);
        assert_eq!(o.status.code(), Some(2));
        assert_eq!(parsed(&o)["ok"], false);
    }
}
