use crate::*;
use chrono::NaiveDate;
use serde_json::{json, Value};
use std::{collections::BTreeMap, path::Path};

const USAGE:&str="stock-research <command> --app-data-dir <explicit path> [options]\nimport --bundle <gp-pit/v1 directory>\nvalidate --dataset <id> [--start YYYY-MM-DD --end YYYY-MM-DD --mode auto|range|trend|defensive]\nevaluate --dataset <id> [--parameters fixed-v1|registered-id --start YYYY-MM-DD --end YYYY-MM-DD --cost <bps> --top <1..50> --mode auto|range|trend|defensive|transition]\ncalibrate --dataset <id> [--end-date YYYY-MM-DD]\npromote --run <calibration-run-id>\nrollback --parameters fixed-v1|previously-qualified-id\n--help emits this complete usage in a JSON envelope. No network/provider downloads.\nExit:0 success,1 I/O/internal,2 invalid/incomplete configuration/data,3 failed eligibility/performance. Latencies are measured local PIT replay.";
fn invalid(message: impl Into<String>) -> ResearchError {
    ResearchError::new("arguments", message)
}
fn parse(args: &[String]) -> ResearchResult<(String, BTreeMap<String, String>)> {
    if args.is_empty() {
        return Err(invalid(
            "a command and explicit --app-data-dir are required; use --help",
        ));
    }
    if args == ["--help"] || args == ["-h"] {
        return Ok(("help".into(), BTreeMap::new()));
    }
    let command = args[0].clone();
    let allowed: &[&str] = match command.as_str() {
        "import" => &["bundle"],
        "validate" => &["dataset", "start", "end", "mode"],
        "evaluate" => &[
            "dataset",
            "parameters",
            "start",
            "end",
            "cost",
            "top",
            "mode",
        ],
        "calibrate" => &["dataset", "end-date"],
        "promote" => &["run"],
        "rollback" => &["parameters"],
        _ => return Err(invalid(format!("unknown command {command}"))),
    };
    if args.len() == 2 && (args[1] == "--help" || args[1] == "-h") {
        return Ok(("help".into(), BTreeMap::new()));
    }
    let mut values = BTreeMap::new();
    let mut i = 1;
    while i < args.len() {
        let key = args[i]
            .strip_prefix("--")
            .ok_or_else(|| invalid("options require --name value"))?;
        if key != "app-data-dir" && !allowed.contains(&key) {
            return Err(invalid(format!(
                "unknown/conflicting option --{key} for {command}"
            )));
        }
        let value = args
            .get(i + 1)
            .filter(|v| !v.trim().is_empty() && !v.starts_with("--"))
            .ok_or_else(|| invalid(format!("missing value for --{key}")))?;
        if values.insert(key.to_owned(), value.clone()).is_some() {
            return Err(invalid(format!("duplicate --{key}")));
        }
        i += 2;
    }
    if !values.contains_key("app-data-dir") {
        return Err(invalid(
            "--app-data-dir is required; no default path is guessed",
        ));
    }
    let required = match command.as_str() {
        "import" => "bundle",
        "promote" => "run",
        "rollback" => "parameters",
        _ => "dataset",
    };
    if !values.contains_key(required) {
        return Err(invalid(format!("--{required} is required")));
    }
    Ok((command, values))
}
fn date(v: &str) -> ResearchResult<NaiveDate> {
    let parsed = NaiveDate::parse_from_str(v, "%Y-%m-%d")
        .map_err(|_| invalid("dates must use YYYY-MM-DD"))?;
    if parsed.to_string() != v {
        return Err(invalid("dates must use YYYY-MM-DD"));
    }
    Ok(parsed)
}
fn config(
    values: &BTreeMap<String, String>,
    d: &DatasetReader,
) -> ResearchResult<EvaluationConfig> {
    // Ordinary research defaults to the supported dataset interval, with a
    // minimum60-bar prior signal. This never changes calibration's split.
    let mut c = EvaluationConfig {
        start_date: *d
            .calendar()
            .get(60)
            .or_else(|| d.calendar().get(1))
            .or_else(|| d.calendar().first())
            .ok_or_else(|| {
                ResearchError::new(
                    "insufficient_data",
                    "evaluation requires60 prior bars and a following execution session",
                )
            })?,
        end_date: d.record().end_date,
        ..Default::default()
    };
    if let Some(v) = values.get("start") {
        c.start_date = date(v)?;
    }
    if let Some(v) = values.get("end") {
        c.end_date = date(v)?;
    }
    if let Some(v) = values.get("mode") {
        c.mode = v.clone();
    }
    if let Some(v) = values.get("cost") {
        c.transaction_cost_bps = v
            .parse()
            .map_err(|_| invalid("cost must be finite nonnegative basis points"))?;
    }
    if let Some(v) = values.get("top") {
        c.top_n = v
            .parse()
            .map_err(|_| invalid("top must be an integer in1..50"))?;
    }
    crate::dataset::validate_config(&c)?;
    Ok(c)
}
fn execute(command: &str, values: &BTreeMap<String, String>) -> ResearchResult<(Value, i32)> {
    if command == "help" {
        return Ok((json!({"usage":USAGE}), 0));
    }
    let path = Path::new(&values["app-data-dir"]);
    if path.as_os_str().is_empty() || path.is_file() {
        return Err(invalid("app-data-dir must be an explicit directory path"));
    }
    // Import, evaluation persistence, calibration and explicit pointer writes
    // are CLI-authorized; app library read entrypoints never migrate/write.
    let store = ResearchStore::open(path)?;
    match command {
        "import" => {
            let bundle = Path::new(&values["bundle"]);
            if !bundle.is_dir() {
                return Err(invalid("bundle must be an existing directory"));
            }
            eprintln!("importing local immutable PIT bundle");
            Ok((serde_json::to_value(store.import_bundle(bundle)?)?, 0))
        }
        "validate" => {
            let d = store.dataset(&values["dataset"])?;
            let q = d.validate(&config(values, &d)?)?;
            let code = if q.passed { 0 } else { 2 };
            Ok((serde_json::to_value(q)?, code))
        }
        "evaluate" => {
            let d = store.dataset(&values["dataset"])?;
            let p = store.research_parameters(
                values
                    .get("parameters")
                    .map(String::as_str)
                    .unwrap_or("fixed-v1"),
            )?;
            if p.dataset_id
                .as_deref()
                .is_some_and(|id| id != d.record().dataset_id)
            {
                return Err(ResearchError::new(
                    "parameters",
                    "candidate origin differs from evaluation dataset",
                ));
            }
            eprintln!("evaluating strict local PIT portfolio");
            let report = evaluate(&d, &p.parameters, &config(values, &d)?)?;
            store.save_report("evaluation", &serde_json::to_value(&report)?)?;
            Ok((serde_json::to_value(report)?, 0))
        }
        "calibrate" => {
            eprintln!("calibration: fixed36+/12/60-month split, deterministic search, frozen test");
            let options = CalibrationOptions {
                end_date: values.get("end-date").map(|v| date(v)).transpose()?,
            };
            let report = calibrate(&store, &values["dataset"], &options)?;
            let code = match report.status.as_str() {
                "complete" => 0,
                "insufficient_data" => 2,
                _ => 3,
            };
            Ok((serde_json::to_value(report)?, code))
        }
        "promote" => Ok((serde_json::to_value(store.promote(&values["run"])?)?, 0)),
        "rollback" => Ok((
            serde_json::to_value(store.rollback(&values["parameters"])?)?,
            0,
        )),
        _ => Err(invalid("unknown command")),
    }
}
pub fn run(args: Vec<String>) -> i32 {
    let mut command = args.first().cloned().unwrap_or_default();
    let result = parse(&args).and_then(|(parsed, values)| {
        command = parsed.clone();
        execute(&parsed, &values)
    });
    match result {
        Ok((data, code)) => {
            println!("{}", json!({"ok":true,"command":command,"data":data}));
            code
        }
        Err(error) => {
            let code = match error.code.as_str() {
                "eligibility" | "test_locked" | "stale_parameters" => 3,
                "arguments" | "config" | "data_quality" | "insufficient_data" | "dataset_id"
                | "manifest" | "parameters" | "unsafe_path" | "csv_headers" | "date"
                | "horizon" | "newer_schema" | "foreign_schema" | "schema" | "core"
                | "not_found" | "input_json" | "input_csv" | "input_numeric" | "input_record"
                | "hash_mismatch" | "identity" | "calendar" | "rate_unit" | "required_field"
                | "duplicate_conflict" | "bar" | "security" | "listing_interval" | "timestamp"
                | "financial" | "status" | "action" | "action_coverage" => 2,
                _ => 1,
            };
            println!("{}", json!({"ok":false,"command":command,"error":error}));
            code
        }
    }
}
