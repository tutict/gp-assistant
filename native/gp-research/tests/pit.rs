use chrono::{Datelike, Days, NaiveDate, Weekday};
use gp_research::*;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

#[test]
fn calibration_signal_purge_retains_full_validation_portfolio_without_late_orders() {
    let (_, d) = load(&fixture());
    let ds = dates();
    let mut value = serde_json::to_value(config()).unwrap();
    value["start_date"] = json!(ds[60]);
    value["end_date"] = json!(ds[120]);
    value["signal_cutoff"] = json!(ds[75]);
    let c: EvaluationConfig = serde_json::from_value(value).unwrap();
    let report = evaluate(&d, &gp_core::fixed_scoring_parameters(), &c).unwrap();
    assert_eq!(report.equity_curve.last().unwrap().date, ds[120]);
    assert!(
        report.trades.iter().all(|trade| trade.date <= ds[76]),
        "purged signals must not place later validation orders"
    );
    assert!(report
        .monthly_folds
        .iter()
        .any(|fold| fold.selection_date > ds[76]));
    assert!(
        report
            .monthly_folds
            .iter()
            .filter(|fold| fold.signal_date > ds[75])
            .all(|fold| fold.precision_at_n.is_none()),
        "purged folds cannot query late forward labels"
    );
}
#[test]
fn supported_weekend_month_end_has_complete_final_fold() {
    let (_, d) = load(&fixture());
    let mut c = config();
    c.start_date = NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
    c.end_date = NaiveDate::from_ymd_opt(2026, 5, 31).unwrap();
    assert_eq!(
        d.record().end_date,
        NaiveDate::from_ymd_opt(2026, 5, 29).unwrap()
    );
    let q = d.validate(&c).unwrap();
    assert!(q.passed, "{:?}", q.issues);
    let report = evaluate(&d, &gp_core::fixed_scoring_parameters(), &c).unwrap();
    assert!(report.monthly_folds.last().unwrap().complete);
}

static NEXT: AtomicUsize = AtomicUsize::new(0);
fn dates() -> Vec<NaiveDate> {
    (0..220)
        .map(|i| {
            NaiveDate::from_ymd_opt(2025, 11, 3)
                .unwrap()
                .checked_add_days(Days::new(i))
                .unwrap()
        })
        .filter(|d| !matches!(d.weekday(), Weekday::Sat | Weekday::Sun))
        .take(150)
        .collect()
}
fn temp() -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "gp-pit-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&p).unwrap();
    p
}
fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|v| format!("{v:02x}"))
        .collect()
}
fn fixture() -> PathBuf {
    let p = temp();
    let ds = dates();
    let mut securities = "code,name,security_type,listed_date,delisted_date\n".to_string();
    let mut bars="code,date,open,high,low,close,volume,amount,total_shares,float_shares,turnover_rate,volume_ratio\n".to_string();
    let mut status = "code,date,name,industry,is_listed,is_st,can_buy,can_sell\n".to_string();
    let mut financial="code,report_date,available_at,revision,source,eps_ttm,bps,roe,deducted_net_profit_billion,deducted_net_profit_growth_rate,deducted_net_profit_margin,dividend_yield\n".to_string();
    for i in 0..12 {
        let code = if i < 10 {
            format!("{:06}.SZ", i + 1)
        } else if i == 10 {
            "000001.SH".into()
        } else {
            "399001.SZ".into()
        };
        securities += &format!(
            "{code},Stock{i},{},{},\n",
            if i < 10 { "equity" } else { "index" },
            ds[0]
        );
        if i < 10 {
            financial += &format!(
                "{code},2025-09-30,{}T10:00:00+08:00,0,synthetic,1,5,{},2,8,10,2\n",
                ds[0],
                if i == 0 { 20 } else { 10 }
            );
        }
        for date in &ds {
            bars +=
                &format!("{code},{date},10,10.1,9.9,10,1000000,10000000,100000000,80000000,1,1\n");
            if i < 10 {
                status += &format!(
                    "{code},{date},Stock{i},Industry{},true,false,true,true\n",
                    i / 2
                );
            }
        }
    }
    for (role, text) in [
        ("securities", securities),
        ("daily_bars", bars),
        ("financial_events", financial),
        ("daily_status", status),
        (
            "corporate_actions",
            "code,event_id,kind,known_at,ex_date,pay_date,cash_per_share,share_multiplier\n".into(),
        ),
        (
            "trading_calendar",
            format!(
                "date\n{}",
                ds.iter().map(|d| format!("{d}\n")).collect::<String>()
            ),
        ),
    ] {
        fs::write(p.join(format!("{role}.csv")), text).unwrap();
    }
    let roles = [
        "securities",
        "daily_bars",
        "financial_events",
        "daily_status",
        "corporate_actions",
        "trading_calendar",
    ];
    let manifest = json!({"format":"gp-pit/v1","name":"Synthetic PIT test","market":"cn","timezone":"Asia/Shanghai","source":"synthetic hand-checked fixture","real_data":false,"full_universe":true,"coverage":{"start":ds[0],"end":ds[149],"corporate_actions_complete":true},"rate_units":{"roe":"percentage_points","dividend_yield":"percentage_points","deducted_net_profit_growth_rate":"percentage_points","deducted_net_profit_margin":"percentage_points","turnover_rate":"percentage_points"},"money_unit":"CNY_100m","files":roles.iter().map(|r|json!({"role":r,"path":format!("{r}.csv"),"sha256":hash(&fs::read(p.join(format!("{r}.csv"))).unwrap())})).collect::<Vec<_>>()});
    fs::write(
        p.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    p
}
fn change(p: &Path, role: &str, f: impl FnOnce(String) -> String) {
    let path = p.join(format!("{role}.csv"));
    fs::write(&path, f(fs::read_to_string(&path).unwrap())).unwrap();
    let mut m: Value = serde_json::from_slice(&fs::read(p.join("manifest.json")).unwrap()).unwrap();
    for file in m["files"].as_array_mut().unwrap() {
        if file["role"] == role {
            file["sha256"] = json!(hash(&fs::read(&path).unwrap()));
        }
    }
    fs::write(p.join("manifest.json"), serde_json::to_vec(&m).unwrap()).unwrap();
}
fn config() -> EvaluationConfig {
    let ds = dates();
    EvaluationConfig {
        start_date: ds[60],
        end_date: ds[89],
        initial_cash: 1000.0,
        transaction_cost_bps: 0.0,
        top_n: 1,
        mode: "defensive".into(),
        ..Default::default()
    }
}
fn load(p: &Path) -> (ResearchStore, DatasetReader) {
    let s = ResearchStore::open(&temp()).unwrap();
    let r = s.import_bundle(p).unwrap();
    let d = s.dataset(&r.dataset_id).unwrap();
    (s, d)
}
fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-8, "{a} != {b}");
}

#[test]
fn import_is_immutable_idempotent_and_reports_round_trip_readonly() {
    let p = fixture();
    let (s, d) = load(&p);
    assert_eq!(d.record().security_count, 12);
    assert_eq!(d.record().bar_count, 1800);
    assert!(!d.record().real_data);
    assert_eq!(
        s.import_bundle(&p).unwrap().dataset_id,
        d.record().dataset_id
    );
    assert_eq!(s.datasets().unwrap().len(), 1);
    let report = evaluate(&d, &gp_core::fixed_scoring_parameters(), &config()).unwrap();
    let v = serde_json::to_value(&report).unwrap();
    let id = s.save_report("evaluation", &v).unwrap();
    assert_eq!(s.report(&id).unwrap(), v);
    let app = s.root().parent().unwrap().to_path_buf();
    let ro = ResearchStore::open_readonly(&app).unwrap();
    assert_eq!(ro.reports().unwrap().len(), 1);
    assert_eq!(
        ro.save_report("evaluation", &v).unwrap_err().code,
        "readonly"
    );
    let missing = temp().join("missing");
    assert!(ResearchStore::open_readonly(&missing).is_err());
    assert!(!missing.exists());
}
#[test]
fn disclosures_use_next_session_and_revisions_do_not_rewrite_history() {
    let p = fixture();
    let ds = dates();
    change(&p, "financial_events", |mut t| {
        t+=&format!("000001.SZ,2025-12-31,{},1,synthetic,2,5,30,2,8,10,2\n000001.SZ,2025-12-31,{}T15:01:00+08:00,2,synthetic,4,5,40,2,8,10,2\n",ds[60],ds[62]);
        t
    });
    let (_, d) = load(&p);
    for (i, pe, roe) in [
        (60, 10.0, 0.2),
        (61, 5.0, 0.3),
        (62, 5.0, 0.3),
        (63, 2.5, 0.4),
    ] {
        let f = d.features_on(ds[i], &config()).unwrap();
        let row = f
            .candidates
            .iter()
            .find(|r| r.stock.code == "000001.SZ")
            .unwrap();
        near(row.stock.pe.unwrap(), pe);
        near(row.stock.roe.unwrap(), roe);
        near(row.stock.deducted_net_profit_growth_rate.unwrap(), 8.0);
        near(row.stock.turnover_rate.unwrap(), 0.01);
    }
}
#[test]
fn future_mutation_cannot_change_earlier_factors() {
    let p = fixture();
    let (_, a) = load(&p);
    let before = a.features_on(dates()[60], &config()).unwrap();
    change(&p, "daily_bars", |t| {
        t.lines()
            .map(|l| {
                if l.starts_with(&format!("000001.SZ,{},", dates()[100])) {
                    l.replace(",10,10.1,9.9,10,", ",20,20.1,19.9,20,")
                } else {
                    l.into()
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n"
    });
    let (_, b) = load(&p);
    let after = b.features_on(dates()[60], &config()).unwrap();
    assert_eq!(
        serde_json::to_value(before).unwrap(),
        serde_json::to_value(after).unwrap()
    );
}
#[test]
fn validation_fails_closed_on_missing_status_bar_and_slow_factor() {
    for role in ["daily_status", "daily_bars", "financial_events"] {
        let p = fixture();
        change(&p, role, |t| {
            t.lines()
                .filter(|l| {
                    !(l.starts_with("000001.SZ,")
                        && (role == "financial_events" || l.contains(&dates()[60].to_string())))
                })
                .collect::<Vec<_>>()
                .join("\n")
                + "\n"
        });
        let (_, d) = load(&p);
        let q = d.validate(&config()).unwrap();
        assert!(!q.passed, "{role}");
        assert!(!q.missing_details.is_empty());
        assert_eq!(
            evaluate(&d, &gp_core::fixed_scoring_parameters(), &config())
                .unwrap_err()
                .code,
            "data_quality"
        );
    }
}
#[test]
fn import_rejects_hash_path_conflicts_and_missing_units() {
    let p = fixture();
    fs::write(p.join("securities.csv"), "altered").unwrap();
    let s = ResearchStore::open(&temp()).unwrap();
    assert_eq!(s.import_bundle(&p).unwrap_err().code, "hash_mismatch");
    assert!(s.datasets().unwrap().is_empty());
    let p = fixture();
    let mut m: Value = serde_json::from_slice(&fs::read(p.join("manifest.json")).unwrap()).unwrap();
    m["files"][0]["path"] = json!("../escape.csv");
    fs::write(p.join("manifest.json"), serde_json::to_vec(&m).unwrap()).unwrap();
    assert_eq!(s.import_bundle(&p).unwrap_err().code, "unsafe_path");
    let p = fixture();
    change(&p, "financial_events", |t| {
        let row = t.lines().nth(1).unwrap().replace(",1,5,20,", ",2,5,20,");
        t + &row + "\n"
    });
    assert_eq!(s.import_bundle(&p).unwrap_err().code, "duplicate_conflict");
    let p = fixture();
    let mut m: Value = serde_json::from_slice(&fs::read(p.join("manifest.json")).unwrap()).unwrap();
    m["rate_units"].as_object_mut().unwrap().remove("roe");
    fs::write(p.join("manifest.json"), serde_json::to_vec(&m).unwrap()).unwrap();
    assert_eq!(s.import_bundle(&p).unwrap_err().code, "rate_unit");
}
#[test]
fn first_trade_is_next_close_and_fees_and_cash_are_real() {
    let p = fixture();
    let (_, d) = load(&p);
    let mut c = config();
    c.transaction_cost_bps = 100.0;
    let r = evaluate(&d, &gp_core::fixed_scoring_parameters(), &c).unwrap();
    let buy = r
        .trades
        .iter()
        .find(|t| t.executed && !t.benchmark)
        .unwrap();
    assert_eq!(buy.date, dates()[60]);
    assert_eq!(r.monthly_folds[0].signal_date, dates()[59]);
    assert_eq!(buy.code, "000001.SZ");
    near(buy.shares, 99.00990099009901);
    near(r.equity_curve[0].equity, 990.0990099009901);
    near(r.metrics.total_transaction_cost, 9.900990099009901);
    assert_eq!(
        serde_json::to_value(evaluate(&d, &gp_core::fixed_scoring_parameters(), &c).unwrap())
            .unwrap(),
        serde_json::to_value(r).unwrap()
    );
}
#[test]
fn realized_actions_pay_later_split_shares_and_forward_labels_mature() {
    let p = fixture();
    let ds = dates();
    change(&p, "corporate_actions", |t| {
        t+&format!("000001.SZ,div,cash_dividend,{}T10:00:00+08:00,{},{},1,\n000001.SZ,split,share_split,{}T10:00:00+08:00,{},{},,2\n",ds[61],ds[62],ds[65],ds[62],ds[64],ds[64])
    });
    change(&p, "daily_bars", |t| {
        t.lines()
            .map(|l| {
                if l.starts_with("000001.SZ,")
                    && l.split(',').nth(1).unwrap() >= ds[64].to_string().as_str()
                {
                    l.replace(",10,10.1,9.9,10,", ",5,5.1,4.9,5,")
                } else {
                    l.into()
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n"
    });
    let (_, d) = load(&p);
    let r = evaluate(&d, &gp_core::fixed_scoring_parameters(), &config()).unwrap();
    near(r.equity_curve[2].equity, 1100.0);
    near(r.equity_curve[4].equity, 1100.0);
    near(r.metrics.total_return, 0.1);
    near(
        d.forward_return("000001.SZ", ds[59], 10).unwrap().unwrap(),
        0.1,
    );
    assert_eq!(d.forward_return("000001.SZ", ds[145], 10).unwrap(), None);
}

fn map_role(p: &Path, role: &str, f: impl FnMut(&str) -> String) {
    change(p, role, |t| {
        t.lines().map(f).collect::<Vec<_>>().join("\n") + "\n"
    });
}
fn month_start_index(ds: &[NaiveDate], after: usize) -> usize {
    (after + 1..ds.len())
        .find(|i| ds[*i].month() != ds[*i - 1].month())
        .unwrap()
}
#[test]
fn auto_requires_two_actual_index_histories_and_ten_equities() {
    let p = fixture();
    let (_, d) = load(&p);
    let mut c = config();
    c.mode = "auto".into();
    c.top_n = 10;
    assert!(d.validate(&c).unwrap().passed);
    let r = evaluate(&d, &gp_core::fixed_scoring_parameters(), &c).unwrap();
    assert_eq!(r.monthly_folds[0].selected_symbols.len(), 10);
    assert!(!r.data_quality.real_data);
    let p = fixture();
    change(&p, "daily_bars", |t| {
        t.lines()
            .filter(|l| !l.starts_with(&format!("399001.SZ,{},", dates()[58])))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n"
    });
    let (_, d) = load(&p);
    let q = d.validate(&c).unwrap();
    assert!(!q.passed);
    assert!(q.missing_details.iter().any(|s| s.contains("399001.SZ")));
}
#[test]
fn historical_names_st_and_industry_and_dynamic_benchmark_are_preserved() {
    let p = fixture();
    let ds = dates();
    let next = month_start_index(&ds, 60);
    let change_day = ds[next - 1].to_string();
    map_role(&p, "daily_status", |l| {
        if l.starts_with("000001.SZ,") && l.split(',').nth(1).unwrap() >= change_day.as_str() {
            l.replace(
                ",Stock0,Industry0,true,false,",
                ",Renamed,NewIndustry,true,true,",
            )
        } else {
            l.into()
        }
    });
    let (_, d) = load(&p);
    let before = d.features_on(ds[60], &config()).unwrap();
    let old = before
        .candidates
        .iter()
        .find(|r| r.stock.code == "000001.SZ")
        .unwrap();
    assert_eq!(old.stock.name, "Stock0");
    assert_eq!(old.stock.industry, "Industry0");
    let after = d.features_on(ds[next - 1], &config()).unwrap();
    assert!(!after.candidates.iter().any(|r| r.stock.code == "000001.SZ"));
    let r = evaluate(&d, &gp_core::fixed_scoring_parameters(), &config()).unwrap();
    assert_eq!(r.monthly_folds[0].eligible_symbol_count, 10);
    assert_eq!(r.monthly_folds[1].eligible_symbol_count, 9);
    assert!(r.trades.iter().any(|t| t.benchmark
        && t.date == ds[next]
        && t.code == "000001.SZ"
        && t.side == "sell"
        && t.executed));
}
#[test]
fn locked_holdings_keep_nav_and_unbuyable_targets_leave_cash() {
    let p = fixture();
    let ds = dates();
    let next = month_start_index(&ds, 60);
    let change_day = ds[next - 1].to_string();
    map_role(&p, "daily_status", |l| {
        if l.starts_with("000001.SZ,") && l.split(',').nth(1).unwrap() >= change_day.as_str() {
            l.replace(",true,false,true,true", ",true,true,false,false")
        } else {
            l.into()
        }
    });
    change(&p, "daily_bars", |t| {
        t.lines()
            .filter(|l| !l.starts_with(&format!("000001.SZ,{},", ds[next])))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n"
    });
    let (_, d) = load(&p);
    let r = evaluate(&d, &gp_core::fixed_scoring_parameters(), &config()).unwrap();
    assert!(r
        .trades
        .iter()
        .any(|t| !t.benchmark && t.code == "000001.SZ" && t.side == "sell" && !t.executed));
    assert!(r
        .exposures
        .iter()
        .any(|e| !e.benchmark && e.date == ds[next] && e.code == "000001.SZ" && e.locked));
    near(r.equity_curve[next - 60].equity, 1000.0);
    let p = fixture();
    map_role(&p, "daily_status", |l| {
        if l.starts_with(&format!("000001.SZ,{},", ds[60])) {
            l.replace(",true,false,true,true", ",true,false,false,true")
        } else {
            l.into()
        }
    });
    let (_, d) = load(&p);
    let r = evaluate(&d, &gp_core::fixed_scoring_parameters(), &config()).unwrap();
    near(r.equity_curve[0].equity, 1000.0);
    assert!(r
        .trades
        .iter()
        .any(|t| !t.benchmark && t.date == ds[60] && !t.executed && t.side == "buy"));
    assert!(!r.exposures.iter().any(|e| !e.benchmark && e.date == ds[60]));
}
#[test]
fn split_cash_entitlement_is_not_spendable_before_payment() {
    let p = fixture();
    let ds = dates();
    let next = month_start_index(&ds, 60);
    change(&p, "corporate_actions", |t| {
        t + &format!(
            "000001.SZ,div,cash_dividend,{}T10:00:00+08:00,{},{},1,\n",
            ds[next - 2],
            ds[next - 1],
            ds[next + 2]
        )
    });
    map_role(&p, "daily_status", |l| {
        if l.starts_with("000001.SZ,")
            && l.split(',').nth(1).unwrap() >= ds[next - 1].to_string().as_str()
        {
            l.replace(",true,false,true,true", ",true,true,true,true")
        } else {
            l.into()
        }
    });
    let (_, d) = load(&p);
    let r = evaluate(&d, &gp_core::fixed_scoring_parameters(), &config()).unwrap();
    let buy = r
        .trades
        .iter()
        .find(|t| !t.benchmark && t.date == ds[next] && t.side == "buy" && t.executed)
        .unwrap();
    near(buy.notional, 1000.0);
    near(r.equity_curve[next - 60].equity, 1100.0);
    near(r.equity_curve[next + 2 - 60].equity, 1100.0);
}
#[test]
fn explicit_zero_delisting_settlement_is_loss_and_missing_evidence_fails() {
    let p = fixture();
    let ds = dates();
    let delist = ds[63].to_string();
    map_role(&p, "securities", |l| {
        if l.starts_with("000001.SZ,") {
            l.to_string() + &delist
        } else {
            l.into()
        }
    });
    for role in ["daily_status", "daily_bars"] {
        change(&p, role, |t| {
            t.lines()
                .filter(|l| {
                    !l.starts_with("000001.SZ,") || l.split(',').nth(1).unwrap() < delist.as_str()
                })
                .collect::<Vec<_>>()
                .join("\n")
                + "\n"
        });
    }
    let (_, d) = load(&p);
    assert!(!d.validate(&config()).unwrap().passed);
    change(&p, "corporate_actions", |t| {
        t + &format!(
            "000001.SZ,delist,delisting_settlement,{}T10:00:00+08:00,{},{},0,\n",
            ds[62], ds[63], ds[65]
        )
    });
    let (_, d) = load(&p);
    let r = evaluate(&d, &gp_core::fixed_scoring_parameters(), &config()).unwrap();
    near(r.equity_curve[3].equity, 0.0);
    near(r.metrics.total_return, -1.0);
    near(r.metrics.max_drawdown.unwrap(), 1.0);
    near(
        d.forward_return("000001.SZ", ds[59], 10).unwrap().unwrap(),
        -1.0,
    );
}
#[test]
fn suspended_endpoint_with_actual_bar_counts_and_without_endpoint_is_unavailable() {
    let p = fixture();
    let ds = dates();
    map_role(&p, "daily_status", |l| {
        if l.starts_with(&format!("000001.SZ,{},", ds[70])) {
            l.replace(",true,false,true,true", ",true,false,false,false")
        } else {
            l.into()
        }
    });
    let (_, d) = load(&p);
    near(
        d.forward_return("000001.SZ", ds[59], 10).unwrap().unwrap(),
        0.0,
    );
    change(&p, "daily_bars", |t| {
        t.lines()
            .filter(|l| !l.starts_with(&format!("000001.SZ,{},", ds[70])))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n"
    });
    let (_, d) = load(&p);
    assert_eq!(d.forward_return("000001.SZ", ds[59], 10).unwrap(), None);
    let r = evaluate(&d, &gp_core::fixed_scoring_parameters(), &config()).unwrap();
    near(r.equity_curve[10].equity, 1000.0);
}
#[test]
fn ipo_never_enters_earlier_frames_and_missing_first_disclosure_stays_unavailable() {
    let p = fixture();
    let ds = dates();
    change(&p, "securities", |t| {
        t + &format!("000011.SZ,NewIPO,equity,{},\n", ds[70])
    });
    change(&p, "daily_bars", |mut t| {
        for date in &ds[70..] {
            t += &format!(
                "000011.SZ,{date},10,10.1,9.9,10,1000000,10000000,100000000,80000000,1,1\n"
            );
        }
        t
    });
    change(&p, "daily_status", |mut t| {
        for date in &ds[70..] {
            t += &format!("000011.SZ,{date},IPO,NewIndustry,true,false,true,true\n");
        }
        t
    });
    let (_, d) = load(&p);
    let f = d.features_on(ds[69], &config()).unwrap();
    assert_eq!(f.candidates.len(), 10);
    assert!(!f.candidates.iter().any(|r| r.stock.code == "000011.SZ"));
    let q = d.validate(&config()).unwrap();
    assert!(!q.passed);
    assert!(q.missing_details.iter().any(|s| s.contains("000011.SZ")));
}
#[test]
fn exact_ninety_five_percent_slow_coverage_is_allowed_but_unavailable_is_reported() {
    let p = fixture();
    let ds = dates();
    change(&p, "securities", |mut t| {
        for i in 11..=20 {
            t += &format!("{i:06}.SZ,Extra{i},equity,{},\n", ds[0]);
        }
        t
    });
    change(&p, "daily_bars", |mut t| {
        for i in 11..=20 {
            for date in &ds {
                t += &format!(
                    "{i:06}.SZ,{date},10,10.1,9.9,10,1000000,10000000,100000000,80000000,1,1\n"
                );
            }
        }
        t
    });
    change(&p, "daily_status", |mut t| {
        for i in 11..=20 {
            for date in &ds {
                t += &format!("{i:06}.SZ,{date},Extra{i},ExtraIndustry{i},true,false,true,true\n");
            }
        }
        t
    });
    change(&p, "financial_events", |mut t| {
        for i in 11..20 {
            t += &format!(
                "{i:06}.SZ,2025-09-30,{}T10:00:00+08:00,0,synthetic,1,5,10,2,8,10,2\n",
                ds[0]
            );
        }
        t
    });
    let (_, d) = load(&p);
    let q = d.validate(&config()).unwrap();
    near(q.slow_factor_coverage, 0.95);
    assert!(q.passed, "{:?}", q.issues);
    assert!(q.missing_details.iter().any(|s| s.contains("000020.SZ")));
}
#[test]
fn newer_registry_schema_is_preserved_on_both_open_modes() {
    let app = temp();
    let s = ResearchStore::open(&app).unwrap();
    let db = s.root().join("research.sqlite");
    drop(s);
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute_batch("PRAGMA user_version=42; CREATE TABLE future(value TEXT);")
        .unwrap();
    drop(conn);
    let before = fs::read(&db).unwrap();
    assert_eq!(
        ResearchStore::open(&app).err().unwrap().code,
        "newer_schema"
    );
    assert_eq!(
        ResearchStore::open_readonly(&app).err().unwrap().code,
        "newer_schema"
    );
    assert_eq!(fs::read(&db).unwrap(), before);
}
#[test]
fn unsupported_and_incomplete_actions_fail_strict_validation() {
    for (kind, cash, shares) in [
        ("unsupported", "", ""),
        ("cash_dividend", "", ""),
        ("share_split", "", ""),
    ] {
        let p = fixture();
        let ds = dates();
        change(&p, "corporate_actions", |t| {
            t + &format!(
                "000001.SZ,bad,{kind},{}T10:00:00+08:00,{},{},{cash},{shares}\n",
                ds[61], ds[62], ds[62]
            )
        });
        let (_, d) = load(&p);
        assert!(!d.validate(&config()).unwrap().passed);
    }
}
#[test]
fn truncated_histories_have_precise_quality_errors() {
    let p = fixture();
    change(&p, "daily_bars", |t| {
        t.lines()
            .filter(|l| {
                l.starts_with("code,")
                    || l.split(',').nth(1).unwrap() >= dates()[5].to_string().as_str()
            })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n"
    });
    let (_, d) = load(&p);
    let q = d.validate(&config()).unwrap();
    assert!(!q.passed);
    assert!(q
        .missing_details
        .iter()
        .any(|s| s.contains("missing tradable bar")));
}

#[test]
fn split_adjusts_signal_day_change_without_using_future_known_actions() {
    let p = fixture();
    let ds = dates();
    change(&p, "corporate_actions", |t| {
        t + &format!(
            "000001.SZ,split,share_split,{}T10:00:00+08:00,{},{},,2\n",
            ds[62], ds[64], ds[64]
        )
    });
    map_role(&p, "daily_bars", |l| {
        if l.starts_with("000001.SZ,")
            && l.split(',').nth(1).unwrap() >= ds[64].to_string().as_str()
        {
            l.replace(",10,10.1,9.9,10,", ",5,5.1,4.9,5,")
        } else {
            l.into()
        }
    });
    let (_, d) = load(&p);
    let f = d.features_on(ds[64], &config()).unwrap();
    let r = f
        .candidates
        .iter()
        .find(|r| r.stock.code == "000001.SZ")
        .unwrap();
    near(r.stock.change_pct.unwrap(), 0.0);
    near(r.raw_return_20, 0.0);
    let p = fixture();
    let (_, a) = load(&p);
    let expected = serde_json::to_value(a.features_on(ds[70], &config()).unwrap()).unwrap();
    change(&p, "corporate_actions", |t| {
        t + &format!(
            "000001.SZ,late,share_split,{}T10:00:00+08:00,{},{},,2\n",
            ds[100], ds[64], ds[64]
        )
    });
    change(&p, "financial_events", |t| {
        t + &format!(
            "000001.SZ,2025-12-31,{}T10:00:00+08:00,1,synthetic,99,99,99,99,99,99,99\n",
            ds[100]
        )
    });
    let (_, b) = load(&p);
    assert_eq!(
        serde_json::to_value(b.features_on(ds[70], &config()).unwrap()).unwrap(),
        expected
    );
}
#[test]
fn actual_execution_close_is_used_while_selection_uses_prior_session() {
    let p = fixture();
    let ds = dates();
    map_role(&p, "daily_bars", |l| {
        if l.starts_with(&format!("000001.SZ,{},", ds[60])) {
            l.replace(",10,10.1,9.9,10,", ",20,20.1,19.9,20,")
        } else {
            l.into()
        }
    });
    let (_, d) = load(&p);
    let r = evaluate(&d, &gp_core::fixed_scoring_parameters(), &config()).unwrap();
    let trade = r
        .trades
        .iter()
        .find(|t| !t.benchmark && t.executed)
        .unwrap();
    near(trade.price.unwrap(), 20.0);
    near(trade.shares, 50.0);
    near(r.equity_curve[0].equity, 1000.0);
    near(r.equity_curve[1].equity, 500.0);
}
#[test]
fn money_and_declared_rate_units_normalize_to_identical_core_inputs() {
    let p = fixture();
    let (_, a) = load(&p);
    let expected = serde_json::to_value(a.features_on(dates()[60], &config()).unwrap()).unwrap();
    change(&p, "financial_events", |t| {
        t.lines()
            .map(|l| {
                if l.starts_with("code,") {
                    l.into()
                } else {
                    let mut cells = l.split(',').map(str::to_owned).collect::<Vec<_>>();
                    for i in [7, 9, 10, 11] {
                        cells[i] = (cells[i].parse::<f64>().unwrap() / 100.0).to_string();
                    }
                    cells[8] = "200000000".into();
                    cells.join(",")
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n"
    });
    map_role(&p, "daily_bars", |l| {
        if l.starts_with("code,") {
            l.into()
        } else {
            let mut cells = l.split(',').map(str::to_owned).collect::<Vec<_>>();
            cells[10] = "0.01".into();
            cells.join(",")
        }
    });
    let mut m: Value = serde_json::from_slice(&fs::read(p.join("manifest.json")).unwrap()).unwrap();
    m["money_unit"] = json!("CNY");
    for value in m["rate_units"].as_object_mut().unwrap().values_mut() {
        *value = json!("ratio");
    }
    fs::write(p.join("manifest.json"), serde_json::to_vec(&m).unwrap()).unwrap();
    let (_, b) = load(&p);
    assert_eq!(
        serde_json::to_value(b.features_on(dates()[60], &config()).unwrap()).unwrap(),
        expected
    );
}
#[test]
fn all_role_headers_are_required_even_for_empty_actions() {
    let p = fixture();
    change(&p, "corporate_actions", |_| "\n".into());
    let s = ResearchStore::open(&temp()).unwrap();
    assert_eq!(s.import_bundle(&p).unwrap_err().code, "csv_headers");
    assert!(s.datasets().unwrap().is_empty());
}
#[test]
fn bars_outside_listing_interval_cannot_supply_fake_ipo_history() {
    let p = fixture();
    map_role(&p, "securities", |l| {
        if l.starts_with("000001.SZ,") {
            l.replace(&dates()[0].to_string(), &dates()[70].to_string())
        } else {
            l.into()
        }
    });
    let s = ResearchStore::open(&temp()).unwrap();
    assert_eq!(s.import_bundle(&p).unwrap_err().code, "listing_interval");
}
#[test]
fn auto_rejects_explicitly_partial_universe_even_with_complete_supplied_rows() {
    let p = fixture();
    let mut m: Value = serde_json::from_slice(&fs::read(p.join("manifest.json")).unwrap()).unwrap();
    m["full_universe"] = json!(false);
    fs::write(p.join("manifest.json"), serde_json::to_vec(&m).unwrap()).unwrap();
    let (_, d) = load(&p);
    let mut c = config();
    c.mode = "auto".into();
    let q = d.validate(&c).unwrap();
    assert!(!q.passed);
    assert!(q.issues.iter().any(|s| s.contains("full_universe")));
}
#[test]
fn legacy_balanced_uses_historical_pool_and_same_simulator() {
    let p = fixture();
    let (_, d) = load(&p);
    let mut c = config();
    c.strategy = "legacy_balanced".into();
    c.transaction_cost_bps = 100.0;
    let r = evaluate(&d, &gp_core::fixed_scoring_parameters(), &c).unwrap();
    near(r.equity_curve[0].equity, 990.0990099009901);
    assert_eq!(r.monthly_folds[0].eligible_symbol_count, 10);
    assert_eq!(r.monthly_folds[0].selected_symbols.len(), 1);
}

#[test]
fn equal_weight_orders_include_fees_for_every_target_and_benchmark_member() {
    let p = fixture();
    let (_, d) = load(&p);
    let mut c = config();
    c.top_n = 2;
    c.transaction_cost_bps = 100.0;
    let r = evaluate(&d, &gp_core::fixed_scoring_parameters(), &c).unwrap();
    let strategy = r
        .trades
        .iter()
        .filter(|t| !t.benchmark && t.date == dates()[60] && t.executed)
        .collect::<Vec<_>>();
    assert_eq!(strategy.len(), 2);
    for t in strategy {
        near(t.shares, 49.504950495049506);
    }
    let benchmark = r
        .trades
        .iter()
        .filter(|t| t.benchmark && t.date == dates()[60] && t.executed)
        .collect::<Vec<_>>();
    assert_eq!(benchmark.len(), 10);
    for t in benchmark {
        near(t.shares, 9.900990099009901);
    }
    near(
        r.benchmark_metrics.total_transaction_cost,
        9.900990099009901,
    );
}
#[test]
fn monthly_folds_carry_nav_and_zero_nav_reports_remain_finite() {
    let p = fixture();
    let (_, d) = load(&p);
    let r = evaluate(&d, &gp_core::fixed_scoring_parameters(), &config()).unwrap();
    let compounded = r
        .monthly_folds
        .iter()
        .map(|f| 1.0 + f.net_return)
        .product::<f64>();
    near(compounded, 1.0 + r.metrics.total_return);
    let p = fixture();
    let ds = dates();
    let delist = ds[63].to_string();
    map_role(&p, "securities", |l| {
        if l.starts_with("000001.SZ,") {
            l.to_string() + &delist
        } else {
            l.into()
        }
    });
    for role in ["daily_status", "daily_bars"] {
        change(&p, role, |t| {
            t.lines()
                .filter(|l| {
                    !l.starts_with("000001.SZ,") || l.split(',').nth(1).unwrap() < delist.as_str()
                })
                .collect::<Vec<_>>()
                .join("\n")
                + "\n"
        });
    }
    change(&p, "corporate_actions", |t| {
        t + &format!(
            "000001.SZ,delist,delisting_settlement,{}T10:00:00+08:00,{},{},0,\n",
            ds[62], ds[63], ds[65]
        )
    });
    let (_, d) = load(&p);
    let r = evaluate(&d, &gp_core::fixed_scoring_parameters(), &config()).unwrap();
    assert!(r
        .monthly_folds
        .iter()
        .all(|f| f.net_return.is_finite() && f.benchmark_return.is_finite()));
    let value = serde_json::to_value(&r).unwrap();
    assert!(serde_json::from_value::<EvaluationReport>(value).is_ok());
}

#[test]
fn published_synthetic_bundle_imports_and_evaluates_without_promotion_evidence() {
    let bundle = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tiny");
    let (_, d) = load(&bundle);
    assert_eq!(d.record().security_count, 12);
    assert_eq!(d.record().bar_count, 1080);
    let mut c = config();
    c.end_date = dates()[74];
    c.mode = "auto".into();
    c.top_n = 10;
    let r = evaluate(&d, &gp_core::fixed_scoring_parameters(), &c).unwrap();
    assert!(r.data_quality.passed);
    assert!(!r.data_quality.real_data);
    assert_eq!(r.monthly_folds.len(), 2);
}
#[test]
fn empty_actions_require_explicit_complete_source_coverage() {
    let p = fixture();
    let mut m: Value = serde_json::from_slice(&fs::read(p.join("manifest.json")).unwrap()).unwrap();
    m["coverage"]["corporate_actions_complete"] = json!(false);
    fs::write(p.join("manifest.json"), serde_json::to_vec(&m).unwrap()).unwrap();
    let s = ResearchStore::open(&temp()).unwrap();
    assert_eq!(s.import_bundle(&p).unwrap_err().code, "action_coverage");
}

#[test]
fn screen_on_honors_pit_filters_as_of_and_parameter_metadata() {
    let p = fixture();
    let (_, d) = load(&p);
    let mut parameters = gp_core::fixed_scoring_parameters();
    parameters.algorithm_version = "adaptive_swing_v2".into();
    parameters.parameter_set_id = "test-calibrated".into();
    let request = gp_core::AdaptiveScreenRequest {
        mode: "defensive".into(),
        primary_limit: 2,
        exploration_limit: 0,
        criteria: gp_core::ScreenCriteria {
            industry: Some("Industry1".into()),
            ..Default::default()
        },
        ..Default::default()
    };
    let result = d.screen_on(dates()[60], &request, &parameters).unwrap();
    assert_eq!(result.items.len(), 2);
    assert!(result.items.iter().all(|i| i.stock.industry == "Industry1"
        && i.stock.quote_time.as_deref() == Some("2026-01-26 15:00:00")));
    assert_eq!(result.parameter_set_id, "test-calibrated");
    assert_eq!(result.algorithm_version, "adaptive_swing_v2");
    assert_eq!(
        result.market_regime.as_of_date.as_deref(),
        Some("2026-01-26")
    );
    assert_eq!(
        result
            .groups
            .iter()
            .find(|g| g.key == "exploration")
            .unwrap()
            .items
            .len(),
        0
    );
    let mut wrong = request.clone();
    wrong.as_of_date = Some("2026-01-27".into());
    assert_eq!(
        d.screen_on(dates()[60], &wrong, &parameters)
            .unwrap_err()
            .code,
        "signal_date"
    );
}

#[test]
fn repair_screen_rejects_prior_tradable_bar_status_and_required_index_gaps() {
    let ds = dates();
    for (role, code, mode) in [
        ("daily_bars", "000001.SZ", "defensive"),
        ("daily_status", "000001.SZ", "defensive"),
        ("daily_bars", "000001.SH", "auto"),
    ] {
        let p = fixture();
        change(&p, role, |text| {
            text.lines()
                .filter(|line| !line.starts_with(&format!("{code},{},", ds[50])))
                .collect::<Vec<_>>()
                .join("\n")
                + "\n"
        });
        let (_, d) = load(&p);
        let request = gp_core::AdaptiveScreenRequest {
            mode: mode.into(),
            primary_limit: 2,
            criteria: gp_core::ScreenCriteria {
                industry: (mode != "auto").then(|| "Industry1".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        let err = d
            .screen_on(ds[60], &request, &gp_core::fixed_scoring_parameters())
            .expect_err("strict screen cannot skip prior required sessions even outside criteria");
        assert_eq!(err.code, "data_quality");
        assert!(err
            .details
            .iter()
            .any(|detail| detail.contains(code) && detail.contains(&ds[50].to_string())));
    }
}

#[test]
fn repair_screen_preserves_suspension_ipo_warmup_and_future_invariance() {
    let p = fixture();
    let ds = dates();
    map_role(&p, "daily_status", |line| {
        if line.starts_with(&format!("000001.SZ,{},", ds[50])) {
            line.replace(",true,false,true,true", ",true,false,false,false")
        } else {
            line.into()
        }
    });
    change(&p, "daily_bars", |text| {
        text.lines()
            .filter(|line| !line.starts_with(&format!("000001.SZ,{},", ds[50])))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n"
    });
    map_role(&p, "securities", |line| {
        if line.starts_with("000002.SZ,") {
            line.replace(&ds[0].to_string(), &ds[58].to_string())
        } else {
            line.into()
        }
    });
    for role in ["daily_bars", "daily_status"] {
        change(&p, role, |text| {
            text.lines()
                .filter(|line| {
                    !line.starts_with("000002.SZ,")
                        || line.split(',').nth(1).unwrap() >= ds[58].to_string().as_str()
                })
                .collect::<Vec<_>>()
                .join("\n")
                + "\n"
        });
    }
    let request = gp_core::AdaptiveScreenRequest {
        mode: "defensive".into(),
        primary_limit: 2,
        ..Default::default()
    };
    let (_, before) = load(&p);
    let expected = serde_json::to_value(
        before
            .screen_on(ds[60], &request, &gp_core::fixed_scoring_parameters())
            .unwrap(),
    )
    .unwrap();
    for role in ["daily_bars", "daily_status"] {
        change(&p, role, |text| {
            text.lines()
                .filter(|line| !line.starts_with(&format!("000001.SZ,{},", ds[61])))
                .collect::<Vec<_>>()
                .join("\n")
                + "\n"
        });
    }
    let (_, after) = load(&p);
    assert_eq!(
        serde_json::to_value(
            after
                .screen_on(ds[60], &request, &gp_core::fixed_scoring_parameters())
                .unwrap()
        )
        .unwrap(),
        expected
    );
}
#[test]
fn readonly_open_and_report_do_not_create_wal_sidecars_or_change_files() {
    let app = temp();
    let s = ResearchStore::open(&app).unwrap();
    let root = s.root().to_path_buf();
    s.save_report(
        "test",
        &json!({"run_id":"read-only-test","dataset_id":"synthetic"}),
    )
    .unwrap();
    drop(s);
    let inventory = || {
        let mut names = fs::read_dir(&root)
            .unwrap()
            .map(|r| r.unwrap().file_name())
            .collect::<Vec<_>>();
        names.sort();
        names
    };
    let before = inventory();
    let db_before = fs::read(root.join("research.sqlite")).unwrap();
    let ro = ResearchStore::open_readonly(&app).unwrap();
    assert_eq!(
        ro.report("read-only-test").unwrap()["dataset_id"],
        "synthetic"
    );
    assert_eq!(inventory(), before);
    assert_eq!(fs::read(root.join("research.sqlite")).unwrap(), db_before);
}

#[test]
fn readonly_registry_sees_committed_publication_with_writer_alive_and_later_writes() {
    let app = temp();
    let s = ResearchStore::open(&app).unwrap();
    s.save_report("test", &json!({"run_id":"before","dataset_id":"synthetic"}))
        .unwrap();
    drop(s);
    let ro = ResearchStore::open_readonly(&app).unwrap();
    assert_eq!(ro.reports().unwrap().len(), 1);
    let writer = ResearchStore::open(&app).unwrap();
    let p = fixture();
    let record = writer.import_bundle(&p).unwrap();
    writer
        .save_report(
            "test",
            &json!({"run_id":"after","dataset_id":record.dataset_id}),
        )
        .unwrap();
    let inventory = || {
        let mut names = fs::read_dir(writer.root())
            .unwrap()
            .map(|r| r.unwrap().file_name())
            .collect::<Vec<_>>();
        names.sort();
        names
    };
    let before = inventory();
    assert_eq!(ro.report("after").unwrap()["dataset_id"], record.dataset_id);
    assert_eq!(ro.datasets().unwrap().len(), 1);
    let current = ResearchStore::open_readonly(&app).unwrap();
    assert_eq!(current.datasets().unwrap().len(), 1);
    assert_eq!(current.reports().unwrap().len(), 2);
    assert_eq!(inventory(), before);
}

#[test]
fn readonly_nonempty_wal_without_shm_requires_explicit_writable_recovery() {
    let source = temp();
    let writer = ResearchStore::open(&source).unwrap();
    writer
        .save_report(
            "test",
            &json!({"run_id":"wal-only","dataset_id":"synthetic"}),
        )
        .unwrap();
    let app = temp();
    let root = app.join("stock-research");
    fs::create_dir(&root).unwrap();
    let source_wal = writer.root().join("research.sqlite-wal");
    assert!(fs::metadata(&source_wal).unwrap().len() > 0);
    fs::copy(
        writer.root().join("research.sqlite"),
        root.join("research.sqlite"),
    )
    .unwrap();
    fs::copy(source_wal, root.join("research.sqlite-wal")).unwrap();
    let before = fs::read(root.join("research.sqlite-wal")).unwrap();
    assert_eq!(
        ResearchStore::open_readonly(&app).err().unwrap().code,
        "readonly_wal"
    );
    assert!(!root.join("research.sqlite-shm").exists());
    assert_eq!(fs::read(root.join("research.sqlite-wal")).unwrap(), before);
    let recovered = ResearchStore::open(&app).unwrap();
    assert_eq!(
        recovered.report("wal-only").unwrap()["dataset_id"],
        "synthetic"
    );
}
#[test]
fn same_day_dividend_uses_previous_shares_even_when_split_sorts_first() {
    let p = fixture();
    let ds = dates();
    change(&p, "corporate_actions", |t| {
        t+&format!("000001.SZ,a-split,share_split,{}T10:00:00+08:00,{},{},,2\n000001.SZ,z-dividend,cash_dividend,{}T10:00:00+08:00,{},{},1,\n",ds[61],ds[62],ds[62],ds[61],ds[62],ds[65])
    });
    map_role(&p, "daily_bars", |l| {
        if l.starts_with("000001.SZ,")
            && l.split(',').nth(1).unwrap() >= ds[62].to_string().as_str()
        {
            l.replace(",10,10.1,9.9,10,", ",4.5,4.6,4.4,4.5,")
        } else {
            l.into()
        }
    });
    let (_, d) = load(&p);
    let f = d.features_on(ds[62], &config()).unwrap();
    let stock = f
        .candidates
        .iter()
        .find(|r| r.stock.code == "000001.SZ")
        .unwrap();
    near(stock.stock.change_pct.unwrap(), 0.0);
    near(stock.raw_return_20, 0.0);
    let r = evaluate(&d, &gp_core::fixed_scoring_parameters(), &config()).unwrap();
    near(r.equity_curve[2].equity, 1000.0);
    near(r.metrics.total_return, 0.0);
    near(
        d.forward_return("000001.SZ", ds[59], 10).unwrap().unwrap(),
        0.0,
    );
}

#[test]
fn review_suspended_dividend_adjusts_retained_mark_without_fabricating_quote() {
    let p = fixture();
    let ds = dates();
    change(&p, "corporate_actions", |t| {
        t + &format!(
            "000001.SZ,div,cash_dividend,{}T10:00:00+08:00,{},{},1,\n",
            ds[61], ds[62], ds[65]
        )
    });
    map_role(&p, "daily_status", |l| {
        if l.starts_with(&format!("000001.SZ,{},", ds[62])) {
            l.replace(",true,false,true,true", ",true,false,false,false")
        } else {
            l.into()
        }
    });
    change(&p, "daily_bars", |t| {
        t.lines()
            .filter(|l| !l.starts_with(&format!("000001.SZ,{},", ds[62])))
            .map(|l| {
                if l.starts_with("000001.SZ,")
                    && l.split(',').nth(1).unwrap() >= ds[62].to_string().as_str()
                {
                    l.replace(",10,10.1,9.9,10,", ",9,9.1,8.9,9,")
                } else {
                    l.into()
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n"
    });
    let (_, d) = load(&p);
    let mut c = config();
    c.end_date = ds[62];
    let r = evaluate(&d, &gp_core::fixed_scoring_parameters(), &c).unwrap();
    assert!(r.data_quality.passed);
    near(r.equity_curve[2].equity, 1000.0);
    near(r.metrics.total_return, 0.0);
    let held = r
        .exposures
        .iter()
        .find(|e| !e.benchmark && e.date == ds[62] && e.code == "000001.SZ")
        .unwrap();
    near(held.last_price, 9.0);
    assert!(held.locked);
    near(
        d.forward_return("000001.SZ", ds[59], 10).unwrap().unwrap(),
        0.0,
    );
    c.end_date = ds[63];
    let resumed = evaluate(&d, &gp_core::fixed_scoring_parameters(), &c).unwrap();
    near(resumed.equity_curve[3].equity, 1000.0);
    near(resumed.metrics.max_drawdown.unwrap(), 0.0);
}
#[test]
fn review_locked_overweight_benchmark_reserves_exposure_and_equal_peer_funding() {
    let p = fixture();
    let ds = dates();
    let next = month_start_index(&ds, 60);
    change(&p, "corporate_actions", |t| {
        t + &format!(
            "000001.SZ,div,cash_dividend,{}T10:00:00+08:00,{},{},1,\n",
            ds[next - 2],
            ds[next - 1],
            ds[next]
        )
    });
    map_role(&p, "daily_status", |l| {
        if l.starts_with(&format!("000001.SZ,{},", ds[next])) {
            l.replace(",true,false,true,true", ",true,false,true,false")
        } else {
            l.into()
        }
    });
    map_role(&p, "daily_bars", |l| {
        if l.starts_with(&format!("000001.SZ,{},", ds[next])) {
            l.replace(",10,10.1,9.9,10,", ",100,100.1,99.9,100,")
        } else {
            l.into()
        }
    });
    let (_, d) = load(&p);
    let mut c = config();
    c.end_date = ds[next];
    let r = evaluate(&d, &gp_core::fixed_scoring_parameters(), &c).unwrap();
    let buys = r
        .trades
        .iter()
        .filter(|t| t.benchmark && t.date == ds[next] && t.side == "buy" && t.executed)
        .collect::<Vec<_>>();
    assert_eq!(buys.len(), 9);
    for t in buys {
        near(t.notional, 1.1111111111111112);
    }
    let locked = r
        .exposures
        .iter()
        .find(|e| e.benchmark && e.date == ds[next] && e.code == "000001.SZ")
        .unwrap();
    near(locked.shares, 10.0);
    near(locked.last_price, 100.0);
    assert!(locked.locked);
    near(r.benchmark_curve.last().unwrap().equity, 1910.0);
}
#[test]
fn review_screen_filter_cannot_bypass_effective_unsupported_action_evidence() {
    let p = fixture();
    let ds = dates();
    change(&p, "corporate_actions", |t| {
        t + &format!(
            "000001.SZ,bad,unsupported,{}T10:00:00+08:00,{},{},,\n",
            ds[60], ds[61], ds[61]
        )
    });
    let (_, d) = load(&p);
    let mut c = config();
    c.start_date = ds[62];
    c.end_date = ds[62];
    assert!(!d.validate(&c).unwrap().passed);
    let request = gp_core::AdaptiveScreenRequest {
        mode: "defensive".into(),
        primary_limit: 2,
        exploration_limit: 0,
        criteria: gp_core::ScreenCriteria {
            industry: Some("Industry1".into()),
            ..Default::default()
        },
        ..Default::default()
    };
    let err = d
        .screen_on(ds[62], &request, &gp_core::fixed_scoring_parameters())
        .unwrap_err();
    assert_eq!(err.code, "data_quality");
    assert!(err
        .details
        .iter()
        .any(|s| s.contains("unsupported/incomplete action 000001.SZ bad")));
}
#[test]
fn review_legacy_concentrated_leaders_are_replaced_from_complete_ranked_pool() {
    let p = fixture();
    let ds = dates();
    map_role(&p, "daily_status", |l| {
        if l.starts_with("000003.SZ,") || l.starts_with("000004.SZ,") {
            l.replace(",Industry1,", ",Industry0,")
        } else {
            l.into()
        }
    });
    map_role(&p, "financial_events", |l| {
        if ["000001.SZ,", "000002.SZ,", "000003.SZ,", "000004.SZ,"]
            .iter()
            .any(|s| l.starts_with(s))
        {
            let mut cells = l.split(',').map(str::to_owned).collect::<Vec<_>>();
            cells[7] = "90".into();
            cells.join(",")
        } else {
            l.into()
        }
    });
    change(&p, "securities", |t| {
        t + &format!("000011.SZ,Extra,equity,{},\n", ds[0])
    });
    change(&p, "financial_events", |t| {
        t + &format!(
            "000011.SZ,2025-09-30,{}T10:00:00+08:00,0,synthetic,1,5,10,2,8,10,2\n",
            ds[0]
        )
    });
    change(&p, "daily_bars", |t| {
        t + &ds
            .iter()
            .map(|d| {
                format!("000011.SZ,{d},10,10.1,9.9,10,1000000,10000000,100000000,80000000,1,1\n")
            })
            .collect::<String>()
    });
    change(&p, "daily_status", |t| {
        t + &ds
            .iter()
            .map(|d| format!("000011.SZ,{d},Extra,Industry4,true,false,true,true\n"))
            .collect::<String>()
    });
    let (_, d) = load(&p);
    let mut c = config();
    c.top_n = 10;
    c.strategy = "legacy_balanced".into();
    c.end_date = ds[60];
    assert!(d.validate(&c).unwrap().passed);
    assert_eq!(d.features_on(ds[59], &c).unwrap().candidates.len(), 11);
    let r = evaluate(&d, &gp_core::fixed_scoring_parameters(), &c).unwrap();
    assert_eq!(r.monthly_folds[0].selected_symbols.len(), 10);
    assert!(r.monthly_folds[0]
        .selected_symbols
        .iter()
        .any(|s| s == "000011.SZ"));
    assert_eq!(
        r.monthly_folds[0]
            .selected_symbols
            .iter()
            .filter(|s| ["000001.SZ", "000002.SZ", "000003.SZ", "000004.SZ"].contains(&s.as_str()))
            .count(),
        3
    );
}

#[test]
fn review_locked_target_funding_includes_fees_and_is_target_order_invariant() {
    let ds = dates();
    let next = month_start_index(&ds, 60);
    for (fee, want_notional, want_cost) in [
        (0.0, 1.1111111111111112, 0.0),
        (25.0, 1.105576319660809, 0.0027639407991520225),
    ] {
        let mut sequences = Vec::new();
        let mut allocations = Vec::new();
        for reverse in [false, true] {
            let p = fixture();
            change(&p, "corporate_actions", |t| {
                t + &format!(
                    "000001.SZ,div,cash_dividend,{}T10:00:00+08:00,{},{},1,\n",
                    ds[next - 2],
                    ds[next - 1],
                    ds[next]
                )
            });
            map_role(&p, "daily_status", |l| {
                if l.starts_with(&format!("000001.SZ,{},", ds[next])) {
                    l.replace(",true,false,true,true", ",true,false,true,false")
                } else {
                    l.into()
                }
            });
            map_role(&p, "daily_bars", |l| {
                if l.starts_with(&format!("000001.SZ,{},", ds[next])) {
                    l.replace(",10,10.1,9.9,10,", ",100,100.1,99.9,100,")
                } else {
                    l.into()
                }
            });
            if reverse {
                map_role(&p, "financial_events", |l| {
                    if l.starts_with("code,") || l.starts_with("000001.SZ,") {
                        l.into()
                    } else {
                        let mut cells = l.split(',').map(str::to_owned).collect::<Vec<_>>();
                        let number = cells[0][..6].parse::<usize>().unwrap();
                        cells[7] = (10 + number).to_string();
                        cells.join(",")
                    }
                });
            }
            let (_, d) = load(&p);
            let mut c = config();
            c.end_date = ds[next];
            c.transaction_cost_bps = fee;
            let r = evaluate(&d, &gp_core::fixed_scoring_parameters(), &c).unwrap();
            let buys = r
                .trades
                .iter()
                .filter(|t| t.benchmark && t.date == ds[next] && t.side == "buy" && t.executed)
                .collect::<Vec<_>>();
            assert_eq!(buys.len(), 9);
            for trade in &buys {
                near(trade.notional, want_notional);
                near(trade.cost, want_cost);
            }
            assert!(!r
                .trades
                .iter()
                .any(|t| t.benchmark && t.date == ds[next] && t.side == "buy" && !t.executed));
            sequences.push(buys.iter().map(|t| t.code.clone()).collect::<Vec<_>>());
            allocations.push(
                buys.iter()
                    .map(|t| (t.code.clone(), t.notional))
                    .collect::<std::collections::BTreeMap<_, _>>(),
            );
        }
        assert_ne!(sequences[0], sequences[1]);
        for (code, amount) in &allocations[0] {
            near(*amount, allocations[1][code]);
        }
    }
}
#[test]
fn review_retained_same_day_cash_split_mark_uses_pre_action_share_units() {
    let p = fixture();
    let ds = dates();
    change(&p, "corporate_actions", |t| {
        t+&format!("000001.SZ,a-split,share_split,{}T10:00:00+08:00,{},{},,2\n000001.SZ,z-dividend,cash_dividend,{}T10:00:00+08:00,{},{},1,\n",ds[61],ds[62],ds[62],ds[61],ds[62],ds[65])
    });
    map_role(&p, "daily_status", |l| {
        if l.starts_with(&format!("000001.SZ,{},", ds[62])) {
            l.replace(",true,false,true,true", ",true,false,false,false")
        } else {
            l.into()
        }
    });
    change(&p, "daily_bars", |t| {
        t.lines()
            .filter(|l| !l.starts_with(&format!("000001.SZ,{},", ds[62])))
            .map(|l| {
                if l.starts_with("000001.SZ,")
                    && l.split(',').nth(1).unwrap() >= ds[62].to_string().as_str()
                {
                    l.replace(",10,10.1,9.9,10,", ",4.5,4.6,4.4,4.5,")
                } else {
                    l.into()
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n"
    });
    let (_, d) = load(&p);
    let mut c = config();
    c.end_date = ds[63];
    let r = evaluate(&d, &gp_core::fixed_scoring_parameters(), &c).unwrap();
    near(r.equity_curve[2].equity, 1000.0);
    near(r.equity_curve[3].equity, 1000.0);
    let held = r
        .exposures
        .iter()
        .find(|e| !e.benchmark && e.date == ds[62] && e.code == "000001.SZ")
        .unwrap();
    near(held.shares, 200.0);
    near(held.last_price, 4.5);
    assert!(held.locked);
    near(
        d.forward_return("000001.SZ", ds[59], 10).unwrap().unwrap(),
        0.0,
    );
}
#[test]
fn review_asof_action_quality_ignores_future_disclosure_but_requires_visible_settlement() {
    let p = fixture();
    let ds = dates();
    let request = gp_core::AdaptiveScreenRequest {
        mode: "defensive".into(),
        primary_limit: 2,
        exploration_limit: 0,
        criteria: gp_core::ScreenCriteria {
            industry: Some("Industry1".into()),
            ..Default::default()
        },
        ..Default::default()
    };
    let (_, before) = load(&p);
    let expected = serde_json::to_value(
        before
            .screen_on(ds[62], &request, &gp_core::fixed_scoring_parameters())
            .unwrap(),
    )
    .unwrap();
    change(&p, "corporate_actions", |t| {
        t + &format!(
            "000001.SZ,late,unsupported,{}T10:00:00+08:00,{},{},,\n",
            ds[100], ds[61], ds[61]
        )
    });
    let (_, after) = load(&p);
    assert_eq!(
        serde_json::to_value(
            after
                .screen_on(ds[62], &request, &gp_core::fixed_scoring_parameters())
                .unwrap()
        )
        .unwrap(),
        expected
    );
    let p = fixture();
    change(&p, "corporate_actions", |t| {
        t + &format!(
            "000001.SZ,incomplete,cash_dividend,{}T10:00:00+08:00,{},{},,\n",
            ds[60], ds[61], ds[61]
        )
    });
    let (_, d) = load(&p);
    let err = d
        .screen_on(ds[62], &request, &gp_core::fixed_scoring_parameters())
        .unwrap_err();
    assert_eq!(err.code, "data_quality");
    assert!(err.details.iter().any(|s| s.contains("incomplete")));
    let p = fixture();
    let delist = ds[62].to_string();
    map_role(&p, "securities", |l| {
        if l.starts_with("000001.SZ,") {
            l.to_string() + &delist
        } else {
            l.into()
        }
    });
    for role in ["daily_bars", "daily_status"] {
        change(&p, role, |t| {
            t.lines()
                .filter(|l| {
                    !l.starts_with("000001.SZ,") || l.split(',').nth(1).unwrap() < delist.as_str()
                })
                .collect::<Vec<_>>()
                .join("\n")
                + "\n"
        });
    }
    change(&p, "corporate_actions", |t| {
        t + &format!(
            "000001.SZ,settlement,delisting_settlement,{}T10:00:00+08:00,{},{},4,\n",
            ds[70], ds[62], ds[65]
        )
    });
    let (_, d) = load(&p);
    let err = d
        .screen_on(ds[62], &request, &gp_core::fixed_scoring_parameters())
        .unwrap_err();
    assert_eq!(err.code, "data_quality");
    assert!(err
        .details
        .iter()
        .any(|s| s.contains("missing delisting settlement evidence 000001.SZ")));
    assert!(d
        .screen_on(ds[70], &request, &gp_core::fixed_scoring_parameters())
        .is_ok());
}

fn assert_report_numbers_finite(report: &EvaluationReport) {
    for trade in &report.trades {
        for number in [trade.shares, trade.notional, trade.cost]
            .into_iter()
            .chain(trade.price)
        {
            assert!(number.is_finite(), "nonfinite trade: {trade:?}");
        }
    }
    for exposure in &report.exposures {
        assert!(exposure.shares.is_finite() && exposure.last_price.is_finite());
    }
    for point in report.equity_curve.iter().chain(&report.benchmark_curve) {
        assert!(point.equity.is_finite());
    }
    for metrics in [&report.metrics, &report.benchmark_metrics] {
        for number in [
            metrics.total_return,
            metrics.annualized_turnover,
            metrics.total_transaction_cost,
        ]
        .into_iter()
        .chain(metrics.annualized_return)
        .chain(metrics.max_drawdown)
        {
            assert!(number.is_finite());
        }
    }
    for fold in &report.monthly_folds {
        assert!(fold.net_return.is_finite() && fold.benchmark_return.is_finite());
        if let Some(value) = fold.precision_at_n {
            assert!(value.is_finite());
        }
    }
}
#[test]
fn rereview_zero_retained_mark_preserves_finite_unpriced_buy_intent_and_cash() {
    let p = fixture();
    let ds = dates();
    let next = month_start_index(&ds, 60);
    change(&p, "corporate_actions", |t| {
        t + &format!(
            "000001.SZ,full-div,cash_dividend,{}T10:00:00+08:00,{},{},10,\n",
            ds[next - 1],
            ds[next],
            ds[next + 2]
        )
    });
    map_role(&p, "daily_status", |l| {
        if l.starts_with(&format!("000001.SZ,{},", ds[next])) {
            l.replace(",true,false,true,true", ",true,false,false,false")
        } else {
            l.into()
        }
    });
    change(&p, "daily_bars", |t| {
        t.lines()
            .filter(|l| !l.starts_with(&format!("000001.SZ,{},", ds[next])))
            .map(|l| {
                if l.starts_with("000001.SZ,")
                    && l.split(',').nth(1).unwrap() > ds[next].to_string().as_str()
                {
                    l.replace(",10,10.1,9.9,10,", ",1,1.1,0.9,1,")
                } else {
                    l.into()
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n"
    });
    let (_, d) = load(&p);
    let mut c = config();
    c.top_n = 2;
    c.end_date = ds[next];
    assert!(d.validate(&c).unwrap().passed);
    let r = evaluate(&d, &gp_core::fixed_scoring_parameters(), &c).unwrap();
    assert_report_numbers_finite(&r);
    for (benchmark, want_cash, want_shares) in [(false, 250.0, 50.0), (true, 90.0, 10.0)] {
        let intent = r
            .trades
            .iter()
            .find(|t| {
                t.benchmark == benchmark
                    && t.date == ds[next]
                    && t.code == "000001.SZ"
                    && t.side == "buy"
                    && !t.executed
            })
            .unwrap();
        near(intent.shares, 0.0);
        assert_eq!(intent.price, None);
        near(intent.notional, want_cash);
        near(intent.cost, 0.0);
        assert!(intent
            .reason
            .as_deref()
            .unwrap()
            .contains("quantity unavailable"));
        let realized_sales = r
            .trades
            .iter()
            .filter(|t| {
                t.benchmark == benchmark && t.date == ds[next] && t.side == "sell" && t.executed
            })
            .map(|t| t.notional)
            .sum::<f64>();
        near(realized_sales, want_cash);
        assert!(!r.trades.iter().any(|t| t.benchmark == benchmark
            && t.date == ds[next]
            && t.side == "buy"
            && t.executed));
        let held = r
            .exposures
            .iter()
            .find(|e| e.benchmark == benchmark && e.date == ds[next] && e.code == "000001.SZ")
            .unwrap();
        near(held.shares, want_shares);
        near(held.last_price, 0.0);
        assert!(held.locked);
    }
    near(r.equity_curve.last().unwrap().equity, 1000.0);
    near(r.benchmark_curve.last().unwrap().equity, 1000.0);
    let value = serde_json::to_value(&r).unwrap();
    let decoded: EvaluationReport = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), value);
}
#[test]
fn rereview_nonfinite_computed_metric_cannot_be_published_as_success() {
    let p = fixture();
    let ds = dates();
    map_role(&p, "daily_bars", |l| {
        if l.starts_with(&format!("000001.SZ,{},", ds[61])) {
            l.replace(",10,10.1,9.9,10,", ",1000000,1000000.1,999999.9,1000000,")
        } else {
            l.into()
        }
    });
    let (_, d) = load(&p);
    let mut c = config();
    c.end_date = ds[61];
    assert!(d.validate(&c).unwrap().passed);
    let outcome = evaluate(&d, &gp_core::fixed_scoring_parameters(), &c);
    assert!(
        outcome.is_err(),
        "successful reports must not contain overflowed metrics"
    );
    let err = outcome.unwrap_err();
    assert_eq!(err.code, "numeric");
    assert!(err.details.iter().any(|s| s.contains("annualized_return")));
}
