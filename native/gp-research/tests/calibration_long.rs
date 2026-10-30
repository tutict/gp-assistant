use chrono::{Datelike, Days, NaiveDate, Weekday};
use gp_research::*;
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf};

#[test]
fn full_length_synthetic_replay_freezes_without_qualifying() {
    let root = std::env::temp_dir().join(format!(
        "gp-cal-long-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let bundle = root.join("bundle");
    fs::create_dir_all(&bundle).unwrap();
    let start = NaiveDate::from_ymd_opt(2016, 1, 1).unwrap();
    let end = NaiveDate::from_ymd_opt(2026, 9, 30).unwrap();
    let calendar: Vec<_> = (0..4000)
        .map(|i| start.checked_add_days(Days::new(i)).unwrap())
        .take_while(|d| *d <= end)
        .filter(|d| !matches!(d.weekday(), Weekday::Sat | Weekday::Sun))
        .collect();
    let mut securities = "code,name,security_type,listed_date,delisted_date\n".to_owned();
    let mut bars="code,date,open,high,low,close,volume,amount,total_shares,float_shares,turnover_rate,volume_ratio\n".to_owned();
    let mut statuses = "code,date,name,industry,is_listed,is_st,can_buy,can_sell\n".to_owned();
    let mut financial="code,report_date,available_at,revision,source,eps_ttm,bps,roe,deducted_net_profit_billion,deducted_net_profit_growth_rate,deducted_net_profit_margin,dividend_yield\n".to_owned();
    for i in 0..12 {
        let code = if i < 10 {
            format!("{:06}.SZ", i + 1)
        } else if i == 10 {
            "000001.SH".into()
        } else {
            "399001.SZ".into()
        };
        let kind = if i < 10 { "equity" } else { "index" };
        securities.push_str(&format!("{code},Name{i},{kind},2015-01-01,\n"));
        if i < 10 {
            financial.push_str(&format!(
                "{code},2015-12-31,2015-12-31,1,synthetic,1,3,18,10,8,20,2\n"
            ));
        }
        for (n, date) in calendar.iter().enumerate() {
            let price = 10. + i as f64 + n as f64 * 0.0005;
            bars.push_str(&format!("{code},{date},{price},{price},{price},{price},10000000,100000000,1000000000,800000000,1,1\n"));
            if i < 10 {
                statuses.push_str(&format!(
                    "{code},{date},Name{i},Industry{},true,false,true,true\n",
                    i % 4
                ));
            }
        }
    }
    let files = [
        ("securities", "securities.csv", securities),
        ("daily_bars", "daily_bars.csv", bars),
        ("daily_status", "daily_status.csv", statuses),
        ("financial_events", "financial_events.csv", financial),
        (
            "corporate_actions",
            "corporate_actions.csv",
            "code,event_id,kind,known_at,ex_date,pay_date,cash_per_share,share_multiplier\n".into(),
        ),
        (
            "trading_calendar",
            "trading_calendar.csv",
            format!(
                "date\n{}",
                calendar
                    .iter()
                    .map(|d| format!("{d}\n"))
                    .collect::<String>()
            ),
        ),
    ];
    let mut entries = Vec::new();
    for (role, name, content) in files {
        let hash = Sha256::digest(content.as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        fs::write(bundle.join(name), content).unwrap();
        entries.push(serde_json::json!({"role":role,"path":name,"sha256":hash}));
    }
    let mut manifest: serde_json::Value = serde_json::from_slice(
        &fs::read(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tiny/manifest.json"),
        )
        .unwrap(),
    )
    .unwrap();
    manifest["name"] = serde_json::json!("Long explicit synthetic replay");
    manifest["source"] = serde_json::json!("Synthetic deterministic weekdays, not exchange data");
    manifest["coverage"]["start"] = serde_json::json!(calendar[0]);
    manifest["coverage"]["end"] = serde_json::json!(calendar.last().unwrap());
    manifest["files"] = serde_json::json!(entries);
    fs::write(
        bundle.join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    let store = ResearchStore::open(&root.join("app")).unwrap();
    let data = store.import_bundle(&bundle).unwrap();
    let report = calibrate(&store, &data.dataset_id, &CalibrationOptions::default()).unwrap();
    assert_eq!(report.status, "insufficient_data");
    assert!(
        report.data_quality.passed,
        "{:?}",
        report.data_quality.issues
    );
    assert_eq!(report.parameters.parameter_set_id, "fixed-v1");
    assert!(report
        .mode_calibration
        .values()
        .all(|m| !m.calibrated && m.reason.is_some()));
    assert_eq!(
        report
            .test
            .as_ref()
            .unwrap()
            .monthly_folds
            .iter()
            .filter(|f| f.complete)
            .count(),
        60
    );
    assert_eq!(report.stress_tests.len(), 2);
    assert_eq!(report.latency.label, "local PIT replay");
    assert!(report.latency.cold_ms.is_some());
    assert_eq!(
        report.diagnostics.factors["test/quality/h20"].unavailable_labels,
        210
    );
    assert!(!report.gate.passed);
    assert!(!store.qualification_verified(&report.run_id).unwrap());
    assert!(store.promote(&report.run_id).is_err());
    assert_eq!(
        store
            .resolve_parameters(None)
            .unwrap()
            .parameters
            .parameter_set_id,
        "fixed-v1"
    );
    let repeat = calibrate(&store, &data.dataset_id, &CalibrationOptions::default()).unwrap();
    assert_eq!(
        serde_json::to_value(report).unwrap(),
        serde_json::to_value(repeat).unwrap()
    );
}
