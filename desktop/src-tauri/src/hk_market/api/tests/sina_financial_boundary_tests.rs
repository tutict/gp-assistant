use super::*;

const FIXTURE: &str = include_str!("../../../../testdata/sina-hk-financial.html");

fn parse(text: &str) -> HkFinancialSnapshot {
    parse_sina_hk_financial_snapshot(text, "00700.HK", "boundary-fetched-at").unwrap()
}

#[test]
fn sina_financial_traditional_amount_units_keep_their_multiplier() {
    for (unit, revenue, profit) in [
        ("百萬元", 401_243_000_000.0, 117_369_000_000.0),
        ("億元", 40_124_300_000_000.0, 11_736_900_000_000.0),
        ("千港元", 401_243_000.0, 117_369_000.0),
    ] {
        let text = FIXTURE.replace("百万元", unit).replace("百萬元", unit);
        let snapshot = parse(&text);
        assert_eq!(snapshot.revenue.value, Some(revenue), "{unit}");
        assert_eq!(snapshot.net_profit.value, Some(profit), "{unit}");
        assert_eq!(snapshot.revenue.currency, "HKD");
        assert_eq!(snapshot.revenue.unit, "currency_total");
        assert_eq!(snapshot.candidate_source, "Sina Finance HK statements");
        assert_eq!(snapshot.candidate_fetched_at, "boundary-fetched-at");
    }
}

#[test]
fn sina_financial_column_unit_overrides_page_and_other_columns() {
    let text = FIXTURE.replace(
        "<th>單位</th><td>百萬元</td><td>百萬元</td>",
        "<th>單位</th><td>千港元</td><td>亿元</td>",
    );
    let snapshot = parse(&text);
    assert_eq!(snapshot.revenue.value, Some(401_243_000.0));
    assert_eq!(snapshot.net_profit.value, Some(117_369_000.0));
}

#[test]
fn sina_financial_row_units_override_table_without_affecting_other_rows() {
    let text = FIXTURE
        .replace("<th>營業額</th>", "<th>營業額(亿元)</th>")
        .replace(
            "<th>股東應佔盈利/(虧損)</th>",
            "<th>股東應佔盈利(千港元)</th>",
        );
    let snapshot = parse(&text);
    assert_eq!(snapshot.revenue.value, Some(40_124_300_000_000.0));
    assert_eq!(snapshot.net_profit.value, Some(117_369_000.0));
}

#[test]
fn sina_financial_missing_or_unsupported_currency_is_explicit() {
    for text in [
        FIXTURE.replace("<tr><th>幣種</th><td>港幣</td><td>港幣</td></tr>", ""),
        FIXTURE.replace("港幣", "未知币种"),
        FIXTURE.replace("港幣", "XYZ"),
        FIXTURE.replace("<td>港幣</td><td>港幣</td>", "<td>--</td><td>港幣</td>"),
    ] {
        let error = parse_sina_hk_financial_snapshot(&text, "00700", "test")
            .expect_err("must not fabricate a reporting currency");
        assert!(error.contains("currency"), "{error}");
    }
}

#[test]
fn sina_financial_missing_or_unknown_amount_unit_keeps_amounts_missing() {
    for text in [
        FIXTURE
            .replace("百万元", "未知金额单位")
            .replace("百萬元", "未知金额单位"),
        FIXTURE
            .replace("<p>单位：（百万元）</p>", "")
            .replace("<tr><th>單位</th><td>百萬元</td><td>百萬元</td></tr>", ""),
        FIXTURE.replace("<th>營業額</th>", "<th>營業額(未知金额单位)</th>"),
    ] {
        let snapshot = parse(&text);
        assert_eq!(snapshot.revenue.value, None);
        assert!(snapshot.revenue.reason.as_deref().unwrap().contains("unit"));
        assert!(snapshot.field_errors.contains_key("revenue"));
        assert!((snapshot.eps.value.unwrap() - 12.639).abs() < 1e-9);
    }
}

#[test]
fn sina_financial_page_unit_applies_only_without_table_declaration() {
    let text = FIXTURE
        .replace("<tr><th>單位</th><td>百萬元</td><td>百萬元</td></tr>", "")
        .replace("百万元", "千港元")
        .replace("</body>", "<p>其他报表介绍涉及亿元</p></body>");
    assert_eq!(parse(&text).revenue.value, Some(401_243_000.0));
}

#[test]
fn sina_financial_diluted_eps_converts_only_explicit_cents() {
    for (label, expected) in [
        ("摊薄每股收益(港元/股)", 12.34),
        ("攤薄每股收益(仙)", 0.1234),
        ("摊薄每股盈利(分)", 0.1234),
        ("摊薄每股收益(cents)", 0.1234),
    ] {
        let text = FIXTURE.replace(
            "</tbody>",
            &format!("<tr><th>{label}</th><td>12.34</td><td>10</td></tr></tbody>"),
        );
        let snapshot = parse(&text);
        let metric = &snapshot.additional_metrics["diluted_eps"];
        assert!((metric.value.unwrap() - expected).abs() < 1e-9, "{label}");
        assert_eq!(metric.unit, "currency/share");
        assert_eq!(metric.currency, "HKD");
    }
}

#[test]
fn sina_financial_basic_eps_and_bps_share_explicit_cent_rules() {
    for (label, expected) in [
        ("基本每股收益(港元/股)", 1263.9),
        ("基本每股盈利(分)", 12.639),
        ("基本每股收益(cents)", 12.639),
    ] {
        let snapshot = parse(&FIXTURE.replace("基本每股盈利(仙)", label));
        assert!(
            (snapshot.eps.value.unwrap() - expected).abs() < 1e-9,
            "{label}"
        );
    }
    for (label, expected) in [
        ("每股净资产(分)", 1.24925),
        ("每股資產淨值(cents)", 1.24925),
        ("每股淨資產(港元/股)", 124.925),
    ] {
        let snapshot = parse(&FIXTURE.replace("每股資產淨值(港元/股)", label));
        assert!(
            (snapshot.additional_metrics["bps"].value.unwrap() - expected).abs() < 1e-9,
            "{label}"
        );
    }
}

#[test]
fn sina_financial_supported_currencies_and_explicit_base_units_keep_values() {
    for (raw_currency, expected_currency, unit) in [
        ("港幣", "HKD", "港元"),
        ("人民幣", "CNY", "元"),
        ("美元", "USD", "美元"),
    ] {
        let text = FIXTURE
            .replace("港幣", raw_currency)
            .replace("百万元", unit)
            .replace("百萬元", unit)
            .replace("基本每股盈利(仙)", "基本每股盈利(元/股)")
            .replace("每股資產淨值(港元/股)", "每股資產淨值(元/股)");
        let snapshot = parse(&text);
        assert_eq!(snapshot.currency, expected_currency);
        assert_eq!(snapshot.revenue.value, Some(401_243.0));
        assert_eq!(snapshot.revenue.currency, expected_currency);
        assert_eq!(snapshot.eps.value, Some(1263.9));
        assert_eq!(snapshot.eps.currency, expected_currency);
    }
}

#[test]
fn sina_financial_zero_and_missing_values_stay_distinct() {
    let text = FIXTURE
        .replace("<td>401243</td>", "<td>0</td>")
        .replace("<td>117369</td>", "<td>--</td>");
    let snapshot = parse(&text);
    assert_eq!(snapshot.revenue.value, Some(0.0));
    assert_eq!(snapshot.revenue.reason, None);
    assert_eq!(snapshot.net_profit.value, None);
    assert!(snapshot.net_profit.reason.is_some());
}

#[test]
fn sina_financial_explicit_row_currency_is_preserved_without_conversion() {
    let text = FIXTURE
        .replace("港幣", "人民幣")
        .replace("<th>營業額</th>", "<th>營業額(百万美元)</th>")
        .replace("基本每股盈利(仙)", "基本每股盈利(港元/股)");
    let snapshot = parse(&text);
    assert_eq!(snapshot.currency, "CNY");
    assert_eq!(snapshot.revenue.currency, "USD");
    assert_eq!(snapshot.revenue.value, Some(401_243_000_000.0));
    assert_eq!(snapshot.eps.currency, "HKD");
    assert_eq!(snapshot.eps.value, Some(1263.9));
    assert_eq!(snapshot.net_profit.currency, "CNY");
}

#[test]
fn sina_financial_table_caption_unit_overrides_page_only_for_its_table() {
    let text = FIXTURE
        .replace("<tr><th>單位</th><td>百萬元</td><td>百萬元</td></tr>", "")
        .replace("<tbody>", "<caption>单位：千港元</caption><tbody>");
    assert_eq!(parse(&text).revenue.value, Some(401_243_000.0));
}
