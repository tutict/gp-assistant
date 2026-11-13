//! Source-bearing, stock-scoped evidence shared by HK answers and follow-ups.

use serde_json::{json, Value};
use std::collections::{BTreeSet, HashSet};

pub(crate) const MAX_EVIDENCE_ITEMS: usize = 16;
const MAX_EVIDENCE_TEXT_CHARS: usize = 1_600;

pub(crate) const ANALYSIS_PROMPT: &str = "港股个股研究使用证据目录中的固定 E# 编号，reply 和关键事实须邻近引用，不能用工具完成状态代替事实来源。研报模式覆盖盈利质量、资本配置、估值假设、风险/失效条件。引用金额或 EPS 时说明原始币种、单位和报告期；披露日期不同于抓取日期。单季、累计、半年与全年/TTM 不可混算，不将半年 EPS 自动年化，不直接用 HKD 股价除以 CNY EPS。估值只引用有明确口径的来源值，或列出仍待核验的同币种价格、TTM 盈利和汇率，不生成缺失的比率。候选财务、未经审计的公告和媒体正文摘要保持各自来源身份。经营现金流、自由现金流、资本开支、分红和回购金额没有证据时明确标为缺项；新闻中的回购或扩张线索不等于已核验的资金用途。";

const CASHFLOW_KEYS: &[&str] = &[
    "operating_cash_flow",
    "net_cash_from_operations",
    "cash_flow_operating",
];
const FREE_CASHFLOW_KEYS: &[&str] = &["free_cash_flow", "free_cashflow"];
const CAPEX_KEYS: &[&str] = &["capital_expenditure", "capex"];
const DIVIDEND_KEYS: &[&str] = &["cash_dividends", "dividend_amount", "dividends"];
const BUYBACK_KEYS: &[&str] = &["buyback_amount", "share_buybacks", "repurchases"];

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn bounded_text(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

fn metric<'a>(item: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    let metrics = item.get("metadata")?.get("metrics")?;
    keys.iter().find_map(|key| {
        metrics.get(*key).filter(|metric| {
            metric
                .get("value")
                .and_then(Value::as_f64)
                .is_some_and(f64::is_finite)
        })
    })
}

fn metric_fact(item: &Value, label: &str, keys: &[&str]) -> Option<String> {
    let metric = metric(item, keys)?;
    let unit = text(metric, "unit");
    let currency = text(metric, "currency");
    let ratio = matches!(
        unit.as_str(),
        "percent" | "%" | "ratio" | "multiple" | "times"
    );
    let currency = if ratio {
        ""
    } else if currency.is_empty() {
        "币种未提供"
    } else {
        &currency
    };
    let unit = if unit.is_empty() {
        "单位未提供"
    } else {
        &unit
    };
    Some(format!(
        "{}；{label}：{currency} {} {unit} [{}]",
        text(item, "report_period"),
        metric["value"],
        text(item, "id")
    ))
}

fn analysis_sections(catalog: &[Value]) -> (Vec<Value>, Vec<String>) {
    let mut gaps = Vec::new();
    for (tier, label) in [
        ("filing", "官方公告"),
        ("financial_snapshot", "财务快照"),
        ("news", "新闻"),
    ] {
        if !catalog.iter().any(|item| text(item, "source_tier") == tier) {
            gaps.push(format!("缺少{label}证据"));
        }
    }
    for (keys, label) in [
        (CASHFLOW_KEYS, "经营现金流"),
        (FREE_CASHFLOW_KEYS, "自由现金流"),
        (CAPEX_KEYS, "资本开支"),
    ] {
        if !catalog.iter().any(|item| metric(item, keys).is_some()) {
            gaps.push(format!("{label}缺失，尚不能核对盈利兑现与资本强度"));
        }
    }
    if !catalog
        .iter()
        .any(|item| metric(item, DIVIDEND_KEYS).is_some() || metric(item, BUYBACK_KEYS).is_some())
    {
        gaps.push("分红与回购金额缺失，资本配置效果仍待公告核验".to_string());
    }
    gaps.push(
        "估值所需的同币种价格、全年/TTM EPS与汇率口径仍需核对；不自行计算跨币种或半年EPS市盈率"
            .to_string(),
    );
    if catalog
        .iter()
        .any(|item| text(item, "published_at").is_empty())
    {
        gaps.push("部分证据缺少披露日期，缓存时间不能代替披露时间".to_string());
    }
    let periods = catalog
        .iter()
        .map(|item| text(item, "report_period"))
        .filter(|period| !period.is_empty())
        .collect::<BTreeSet<_>>();
    if periods.len() > 1 {
        gaps.push(format!(
            "报告期存在不同口径：{}；单季、累计和半年数据不能直接混算",
            periods.into_iter().collect::<Vec<_>>().join("、")
        ));
    }

    let mut quality = Vec::new();
    for item in catalog {
        for (label, keys) in [
            ("每股收益 EPS", &["eps"][..]),
            ("净资产收益率 ROE", &["roe"][..]),
            ("营业收入", &["revenue"][..]),
            ("归母净利润", &["net_profit"][..]),
            ("单季营业收入", &["quarterly_revenue"][..]),
            ("九个月累计营业收入", &["nine_month_revenue"][..]),
            ("单季净利润", &["quarterly_net_profit"][..]),
            ("九个月累计净利润", &["nine_month_net_profit"][..]),
            ("经营现金流", CASHFLOW_KEYS),
            ("自由现金流", FREE_CASHFLOW_KEYS),
            ("资本开支", CAPEX_KEYS),
        ] {
            if let Some(fact) = metric_fact(item, label, keys) {
                quality.push(fact);
            }
        }
    }
    if quality.is_empty() {
        quality.push("缺少可核对的财务指标，当前不能评定盈利质量。".to_string());
    }
    quality
        .push("净利润与现金兑现须分别核对；现金流或资本开支缺项时不推算自由现金流。".to_string());

    let mut capital = catalog
        .iter()
        .filter(|item| {
            let body = text(item, "summary");
            ["回购", "分红", "资本", "融资", "并购", "债务"]
                .iter()
                .any(|term| body.contains(term))
        })
        .take(3)
        .map(|item| {
            format!(
                "{} [{}]；资金用途及金额按原始公告核验。",
                bounded_text(&text(item, "summary"), 300),
                text(item, "id")
            )
        })
        .collect::<Vec<_>>();
    for item in catalog {
        for (label, keys) in [("分红金额", DIVIDEND_KEYS), ("回购金额", BUYBACK_KEYS)] {
            if let Some(fact) = metric_fact(item, label, keys) {
                capital.push(fact);
            }
        }
    }
    if capital.is_empty() {
        capital.push(
            "当前没有可核验的分红、回购、再投资或融资金额；资本配置判断保持待核验。".to_string(),
        );
    }
    let mut valuation = Vec::new();
    for item in catalog {
        for (label, keys) in [
            ("来源市盈率 PE", &["pe", "pe_ttm"][..]),
            ("来源市净率 PB", &["pb"][..]),
            ("原始每股收益 EPS", &["eps"][..]),
        ] {
            if let Some(fact) = metric_fact(item, label, keys) {
                valuation.push(fact);
            }
        }
    }
    valuation.push("估值需价格、EPS和汇率的同币种与同报告期证据；半年EPS不等于全年或TTM。来源PE可引用，但不据此给出目标价。".to_string());
    let mut risks = catalog
        .iter()
        .filter_map(|item| {
            let tier = text(item, "source_tier");
            if tier == "financial_snapshot" && text(item, "verification_status") != "verified" {
                Some(format!(
                    "财务快照为候选数值，报告期 {}，仍需原始公告核验。[{}]",
                    text(item, "report_period"),
                    text(item, "id")
                ))
            } else if text(item, "audit_status").contains("unaudited") {
                Some(format!(
                    "{} 为未经审计公告口径。[{}]",
                    text(item, "report_period"),
                    text(item, "id")
                ))
            } else if tier == "news" || tier == "community" {
                Some(format!(
                    "{}为{}，不能单独确认财务影响。[{}]",
                    text(item, "source"),
                    if text(item, "summary_kind") == "article_summary" {
                        "媒体正文摘要"
                    } else {
                        "资讯线索"
                    },
                    text(item, "id")
                ))
            } else {
                None
            }
        })
        .take(4)
        .collect::<Vec<_>>();
    risks.extend(gaps.iter().cloned());
    let sections = vec![
        json!({"title":"盈利质量","bullets":quality}),
        json!({"title":"资本配置","bullets":capital}),
        json!({"title":"估值假设","bullets":valuation}),
        json!({"title":"风险与待核验项","bullets":risks}),
    ];
    (sections, gaps)
}

pub(crate) fn validate_model_analysis(model: &Value, mode: &str) -> Result<(), String> {
    if matches!(mode, "research" | "value_compounder_v1") {
        let titles = model
            .get("answer_sections")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|section| text(section, "title"))
            .collect::<Vec<_>>();
        for alternatives in [
            ["盈利", "现金流"],
            ["资本", "配置"],
            ["估值", "安全边际"],
            ["风险", "失效"],
        ] {
            if !titles
                .iter()
                .any(|title| alternatives.iter().any(|label| title.contains(label)))
            {
                return Err(format!("港股研报缺少分析维度：{}", alternatives[0]));
            }
        }
    }
    let mut claims = vec![text(model, "reply")];
    claims.extend(
        model
            .get("answer_sections")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .flat_map(|section| {
                section
                    .get("bullets")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
            }),
    );
    for claim in claims {
        for clause in claim.split(['。', '，', '；', '\n', ';']) {
            for label in [
                "EPS",
                "eps",
                "每股收益",
                "营业收入",
                "净利润",
                "现金流",
                "回购金额",
                "分红金额",
                "revenue",
                "net_profit",
                "net profit",
            ] {
                for (label_index, _) in clause.match_indices(label) {
                    let suffix = &clause[label_index + label.len()..];
                    let value_part = suffix.split('[').next().unwrap_or("");
                    let Some(number_start) = value_part.find(|ch: char| ch.is_ascii_digit()) else {
                        continue;
                    };
                    let number_end = value_part[number_start..]
                        .find(|ch: char| !ch.is_ascii_digit() && ch != '.' && ch != ',')
                        .map(|length| number_start + length)
                        .unwrap_or(value_part.len());
                    let after = value_part[number_end..].trim_start();
                    let before = &value_part[..number_start];
                    if after.starts_with('年') || after.starts_with('月') {
                        continue;
                    }
                    let growth = [
                        "同比", "环比", "增长", "增幅", "下降", "变化", "变动", "率", "growth",
                        "margin",
                    ]
                    .iter()
                    .any(|term| before.contains(term));
                    if growth && (after.starts_with('%') || after.starts_with('％')) {
                        continue;
                    }
                    let before_amount = &clause[..label_index + label.len() + number_start];
                    let prefix = before_amount.rsplit(',').next().unwrap_or(before_amount);
                    let unit_context =
                        format!("{prefix} {}", after.split([',', '[']).next().unwrap_or(""));
                    if !["CNY", "HKD", "USD", "RMB", "人民币", "港元", "美元", "港币"]
                        .iter()
                        .any(|currency| unit_context.contains(currency))
                    {
                        return Err("港股金额或EPS结论缺少原始币种".to_string());
                    }
                    if ![
                        "元",
                        "每股",
                        "/share",
                        "currency",
                        "million",
                        "billion",
                        "百万元",
                        "亿元",
                        "万港币",
                    ]
                    .iter()
                    .any(|unit| unit_context.contains(unit))
                    {
                        return Err("港股金额或EPS结论缺少单位".to_string());
                    }
                }
            }
        }
    }
    Ok(())
}

fn explicitly_refreshes(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    [
        "最新",
        "刷新",
        "重新查询",
        "重新取证",
        "更新数据",
        "实时",
        "refresh",
        "latest",
    ]
    .iter()
    .any(|word| lower.contains(word))
}

fn period_is_covered(message: &str, catalog: &[Value]) -> bool {
    let upper = message.to_ascii_uppercase();
    let mut scopes = Vec::new();
    for (scope, labels) in [
        ("FY", &["全年", "年度", "年报", "FY"][..]),
        ("TTM", &["TTM", "滚动十二个月", "过去十二个月"][..]),
        ("H1", &["H1", "上半年", "半年报", "中报"][..]),
        ("H2", &["H2", "下半年"][..]),
        ("Q1", &["Q1", "一季度", "一季报", "第一季"][..]),
        ("Q2", &["Q2", "二季度", "二季报", "第二季"][..]),
        ("Q3", &["Q3", "三季度", "三季报", "第三季"][..]),
        ("Q4", &["Q4", "四季度", "四季报", "第四季"][..]),
    ] {
        if labels.iter().any(|label| upper.contains(label)) {
            scopes.push(scope);
        }
    }
    let mut years = BTreeSet::new();
    let bytes = message.as_bytes();
    for index in 0..bytes.len().saturating_sub(3) {
        let year = &bytes[index..index + 4];
        if !year.starts_with(b"20")
            || !year.iter().all(u8::is_ascii_digit)
            || (index > 0 && bytes[index - 1].is_ascii_digit())
            || bytes.get(index + 4).is_some_and(u8::is_ascii_digit)
            || upper[..index].ends_with("HK")
            || upper[index + 4..].starts_with(".HK")
        {
            continue;
        }
        years.insert(std::str::from_utf8(year).unwrap_or_default().to_string());
    }
    let available = catalog
        .iter()
        .map(|item| text(item, "report_period").to_ascii_uppercase())
        .filter(|period| !period.is_empty())
        .collect::<Vec<_>>();
    let scope_matches = |period: &str, scope: &str| {
        if scope == "FY" {
            period.ends_with("FY")
                || period.ends_with('Y')
                || period.contains("ANNUAL")
                || period.contains("年度")
        } else {
            period.ends_with(scope)
        }
    };
    if years.is_empty() {
        for scope in scopes {
            let matched = available
                .iter()
                .filter(|period| scope_matches(period, scope))
                .map(|period| period.chars().take(4).collect::<String>())
                .collect::<BTreeSet<_>>();
            if matched.len() != 1 {
                return false;
            }
        }
    } else {
        for year in years {
            if scopes.is_empty() {
                if !available.iter().any(|period| period.starts_with(&year)) {
                    return false;
                }
            } else {
                for scope in &scopes {
                    if !available
                        .iter()
                        .any(|period| period.starts_with(&year) && scope_matches(period, scope))
                    {
                        return false;
                    }
                }
            }
        }
    }
    true
}

pub(crate) fn note_requested_period(response: &mut Value, message: &str) {
    let catalog = response
        .get("evidence_summary")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if period_is_covered(message, &catalog) {
        return;
    }
    let gap = "当前证据未覆盖问题指定的报告期；已列半年、单季或累计数据不能充当所请求的全年/TTM或其他报告期";
    if let Some(gaps) = response
        .get_mut("research_context")
        .and_then(|context| context.get_mut("data_gaps"))
        .and_then(Value::as_array_mut)
    {
        gaps.push(json!(gap));
    }
    if let Some(sections) = response
        .get_mut("answer_sections")
        .and_then(Value::as_array_mut)
    {
        sections.push(json!({"title":"所请求报告期缺项","bullets":[gap]}));
    }
}

pub(crate) fn reusable_context(
    value: &Value,
    stock_code: &str,
    message: &str,
    remote_safe_only: bool,
) -> Option<Value> {
    if explicitly_refreshes(message)
        || value.get("market").and_then(Value::as_str) != Some("hk")
        || value.get("stock_code").and_then(Value::as_str) != Some(stock_code)
    {
        return None;
    }
    let timestamp = value.get("snapshot_at_epoch_ms")?.as_u64()?;
    if timestamp == 0
        || u128::from(timestamp) > crate::market::epoch_millis().saturating_add(5 * 60 * 1000)
    {
        return None;
    }
    let catalog = value.get("evidence")?.as_array()?;
    if catalog.is_empty()
        || catalog.len() > MAX_EVIDENCE_ITEMS
        || !period_is_covered(message, catalog)
        || serde_json::to_vec(value).ok()?.len() > 128 * 1024
    {
        return None;
    }
    for (index, item) in catalog.iter().enumerate() {
        if text(item, "id") != format!("E{}", index + 1)
            || text(item, "document_id").is_empty()
            || text(item, "summary").chars().count() > MAX_EVIDENCE_TEXT_CHARS
            || item.get("unavailable").and_then(Value::as_bool) == Some(true)
            || (remote_safe_only
                && item.get("remote_export_allowed").and_then(Value::as_bool) != Some(true))
        {
            return None;
        }
        let codes = item.get("stock_codes")?.as_array()?;
        if !codes.iter().any(|code| code.as_str() == Some(stock_code))
            || codes
                .iter()
                .any(|code| code.as_str().is_none_or(|code| !code.ends_with(".HK")))
        {
            return None;
        }
    }
    let mut result = value.clone();
    result["reused"] = json!(true);
    Some(result)
}

pub(crate) fn frozen_evidence(context: &Value) -> Value {
    let citations = context
        .get("evidence")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|item| {
            let mut citation = item.clone();
            citation["source_name"] = item.get("source").cloned().unwrap_or(Value::Null);
            if text(item, "summary_kind") == "article_summary" {
                citation["article_summary"] =
                    json!({"text":text(item,"summary"),"provenance":"extracted_article_summary"});
            }
            citation
        })
        .collect::<Vec<_>>();
    json!({"market":"hk","stock_code":context["stock_code"],"snapshot_at_epoch_ms":context["snapshot_at_epoch_ms"],
        "citations":citations,"frozen_snapshot":true,"research_store":true})
}

pub(crate) fn context_from_frozen_evidence(evidence: &Value) -> Value {
    let originals = evidence
        .get("citations")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|citation| {
            let mut original = citation.clone();
            if let Some(fields) = original.as_object_mut() {
                fields.remove("source_name");
                fields.remove("article_summary");
            }
            original
        })
        .collect::<Vec<_>>();
    json!({"market":evidence["market"],"stock_code":evidence["stock_code"],"evidence":originals})
}

pub(crate) fn answer_from_context(context: &Value, message: &str) -> Value {
    let catalog = context
        .get("evidence")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let (sections, gaps) = analysis_sections(&catalog);
    let code = text(context, "stock_code");
    let mut context = context.clone();
    context["data_gaps"] = json!(gaps);
    json!({"market":"hk","action":"stock_research","reply":format!("继续研究 {code}，本轮沿用原证据和披露日期。[E1]"),
        "evidence_summary":catalog,"answer_sections":sections,"research_context":context,
        "tool_calls":[{"id":"tool_hk_research_snapshot","tool":"news_evidence","label":"沿用研究证据",
            "status":"cached","input":{"stock_code":code,"query":bounded_text(message,256)},"output_summary":"已沿用上轮固定证据，无需重复取证"}],
        "warnings":["仅供选股研究，不构成投资建议。"],"next_actions":["核对最新公告","追问盈利质量或资本配置"]})
}

pub(crate) fn snapshot_for_run(run: &Value, conversation_id: &str) -> Option<Value> {
    if conversation_id.is_empty()
        || run.get("conversation_id").and_then(Value::as_str) != Some(conversation_id)
        || run.get("status").and_then(Value::as_str) != Some("completed")
    {
        return None;
    }
    let result = run.get("result")?;
    if result.get("market").and_then(Value::as_str) != Some("hk") {
        return None;
    }
    let context = result.get("research_context")?;
    (context.get("market").and_then(Value::as_str) == Some("hk")).then(|| context.clone())
}

pub(crate) fn evidence_catalog(result: &Value, stock_code: &str) -> Vec<Value> {
    evidence_catalog_for_scope(result, Some(stock_code))
}

pub(crate) fn unscoped_evidence_catalog(result: &Value) -> Vec<Value> {
    evidence_catalog_for_scope(result, None)
}

fn evidence_catalog_for_scope(result: &Value, stock_code: Option<&str>) -> Vec<Value> {
    let mut seen = HashSet::new();
    result.get("citations").and_then(Value::as_array).into_iter().flatten()
        .filter(|citation| citation.get("unavailable").and_then(Value::as_bool) != Some(true))
        .filter(|citation| stock_code.is_none_or(|stock_code| citation.get("stock_codes").and_then(Value::as_array).is_some_and(|codes| {
            codes.iter().filter_map(Value::as_str).any(|code| code == stock_code)
        })))
        .filter_map(|citation| {
            let document_id = text(citation, "document_id");
            let chunk_id = text(citation, "chunk_id");
            if document_id.is_empty() || !seen.insert((document_id.clone(), chunk_id.clone())) { return None; }
            let metadata = citation.get("metadata").filter(|value| value.is_object()).cloned().unwrap_or_else(|| json!({}));
            let summary = citation.get("article_summary");
            let summary_text = summary.and_then(|value| value.as_str().or_else(|| value.get("text").and_then(Value::as_str)))
                .filter(|value| !value.trim().is_empty());
            let excerpt = bounded_text(&text(citation, "excerpt"), MAX_EVIDENCE_TEXT_CHARS);
            let source_tier = text(citation, "source_tier");
            Some(json!({
                "citation_id":text(citation,"citation_id"),"document_id":document_id,"chunk_id":chunk_id,
                "document_content_hash":citation.get("document_content_hash"),"chunk_content_hash":citation.get("chunk_content_hash"),
                "title":bounded_text(&text(citation,"title"),160),"source":bounded_text(&text(citation,"source_name"),200),
                "level":source_tier,"source_tier":source_tier,"excerpt":excerpt,
                "summary":bounded_text(summary_text.unwrap_or(&excerpt),MAX_EVIDENCE_TEXT_CHARS),
                "summary_kind":if summary_text.is_some() {"article_summary"} else {"excerpt"},
                "published_at":citation.get("published_at"),"url":citation.get("url"),
                "report_period":metadata.get("report_period"),
                "period_end":metadata.get("period_end").or_else(||metadata.get("report_date")),
                "currency":metadata.get("currency"),"unit":metadata.get("unit"),
                "verification_status":metadata.get("verification_status"),"audit_status":metadata.get("audit_status"),
                "stock_codes":stock_code.map(|code|json!([code])).unwrap_or_else(||citation.get("stock_codes").cloned().unwrap_or_else(||json!([]))),"metadata":metadata,
                "remote_export_allowed":citation.get("remote_export_allowed").and_then(Value::as_bool).unwrap_or(false)
            }))
        }).take(MAX_EVIDENCE_ITEMS).enumerate().map(|(index,mut item)| {
            item["id"] = json!(format!("E{}",index+1)); item
        }).collect()
}

pub(crate) fn attach_evidence(mut response: Value, evidence: &Value, stock_code: &str) -> Value {
    let catalog = evidence_catalog(evidence, stock_code);
    let (mut sections, gaps) = analysis_sections(&catalog);
    let bullets = catalog
        .iter()
        .map(|item| {
            let date = item
                .get("published_at")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .unwrap_or("披露日期未提供");
            let period = item
                .get("report_period")
                .and_then(Value::as_str)
                .unwrap_or("");
            json!(format!(
                "{}；{} {}：{} [{}]",
                text(item, "source"),
                date,
                period,
                bounded_text(&text(item, "summary"), 600),
                text(item, "id")
            ))
        })
        .collect::<Vec<_>>();
    response["evidence_summary"] = json!(catalog);
    response["market"] = json!("hk");
    response["research_context"] = json!({"market":"hk","stock_code":stock_code,
        "snapshot_at_epoch_ms":crate::market::epoch_millis(),"evidence":catalog,"data_gaps":gaps,"reused":false});
    if !bullets.is_empty() {
        response["reply"] = json!(format!(
            "已整理 {stock_code} 的本地研究证据，结论与来源可逐项核对。[E1]"
        ));
        sections.insert(0, json!({"title":"证据底稿","bullets":bullets}));
        response["answer_sections"] = json!(sections);
    } else {
        response["reply"] = json!(format!(
            "{stock_code} 当前缺少可引用的公告、财务或新闻证据。"
        ));
        sections.insert(0,json!({"title":"证据缺项","bullets":["当前股票未命中可引用资料，尚不能形成研究判断。"]}));
        response["answer_sections"] = json!(sections);
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    fn financial_evidence() -> Value {
        json!({"citations":[{"citation_id":"C1","document_id":"financial","chunk_id":"financial-1",
            "title":"腾讯财务快照","excerpt":"报告期2026H1；每股收益 CNY 12.639 currency/share。",
            "source_tier":"financial_snapshot","source_name":"Eastmoney HK financial indicators",
            "published_at":"2026-08-13","url":"https://emweb.securities.eastmoney.com/PC_HKF10/pages/home.html?code=00700",
            "stock_codes":["00700.HK"],"remote_export_allowed":true,
            "metadata":{"report_period":"2026H1","report_date":"2026-06-30","currency":"CNY","verification_status":"candidate",
                "metrics":{"eps":{"value":12.639,"currency":"CNY","unit":"currency/share"}}}}]})
    }

    #[test]
    fn snapshot_for_run_rejects_other_conversations_and_incomplete_runs() {
        let context = attach_evidence(json!({}), &financial_evidence(), "00700.HK")
            ["research_context"]
            .clone();
        let mut run = json!({"run_id":"source-run","conversation_id":"chat-a","status":"completed","result":{"market":"hk","research_context":context}});
        assert!(snapshot_for_run(&run, "chat-b").is_none());
        assert!(snapshot_for_run(&run, "chat-a").is_some());
        run["status"] = json!("running");
        assert!(snapshot_for_run(&run, "chat-a").is_none());
    }

    #[test]
    fn snapshot_for_run_keeps_authoritative_source_permissions_and_market_scope() {
        let mut context = attach_evidence(json!({}), &financial_evidence(), "00700.HK")
            ["research_context"]
            .clone();
        context["evidence"][0]["remote_export_allowed"] = json!(false);
        let mut run = json!({"run_id":"source-run","conversation_id":"chat-a","status":"completed","result":{"market":"hk","research_context":context}});
        let retained = snapshot_for_run(&run, "chat-a").unwrap();
        assert_eq!(retained["evidence"][0]["remote_export_allowed"], false);
        assert!(reusable_context(&retained, "00700.HK", "它的风险", true).is_none());
        run["result"]["market"] = json!("cn");
        assert!(snapshot_for_run(&run, "chat-a").is_none());
    }

    #[test]
    fn hk_research_brief_exposes_four_analysis_dimensions_and_missing_cashflow() {
        let response = attach_evidence(json!({}), &financial_evidence(), "00700.HK");
        let sections = response["answer_sections"].as_array().unwrap();
        for dimension in ["盈利质量", "资本配置", "估值假设", "风险"] {
            assert!(
                sections
                    .iter()
                    .any(|section| text(section, "title").contains(dimension)),
                "missing {dimension}"
            );
        }
        let gaps = response["research_context"]["data_gaps"].to_string();
        assert!(gaps.contains("经营现金流"));
        assert!(gaps.contains("自由现金流"));
        assert!(gaps.contains("同币种"));
        assert!(response["answer_sections"].to_string().contains("2026H1"));
        assert!(response["answer_sections"]
            .to_string()
            .contains("currency/share"));
        assert!(response["answer_sections"].to_string().contains("候选"));
    }

    #[test]
    fn hk_research_zero_cashflow_is_known_data_with_its_own_currency_and_unit() {
        let mut evidence = financial_evidence();
        evidence["citations"][0]["metadata"]["metrics"]["operating_cash_flow"] =
            json!({"value":0,"currency":"CNY","unit":"million","source":"fixture"});
        let response = attach_evidence(json!({}), &evidence, "00700.HK");
        let gaps = response["research_context"]["data_gaps"]
            .as_array()
            .unwrap();
        assert!(!gaps.iter().any(|gap| gap
            .as_str()
            .is_some_and(|gap| gap.starts_with("经营现金流缺失"))));
        assert!(response["answer_sections"]
            .to_string()
            .contains("经营现金流：CNY 0 million"));
    }

    #[test]
    fn hk_research_preserves_single_quarter_and_cumulative_metric_labels() {
        let mut evidence = financial_evidence();
        evidence["citations"][0]["metadata"]["report_period"] = json!("2026Q3");
        evidence["citations"][0]["metadata"]["metrics"] = json!({
            "quarterly_revenue":{"value":192869,"currency":"CNY","unit":"million","period_scope":"single_quarter"},
            "nine_month_revenue":{"value":568000,"currency":"CNY","unit":"million","period_scope":"nine_month_cumulative"}
        });
        let response = attach_evidence(json!({}), &evidence, "00700.HK");
        let quality = response["answer_sections"]
            .as_array()
            .unwrap()
            .iter()
            .find(|section| text(section, "title") == "盈利质量")
            .unwrap()
            .to_string();
        assert!(quality.contains("单季营业收入：CNY 192869 million"));
        assert!(quality.contains("九个月累计营业收入：CNY 568000 million"));
    }

    #[test]
    fn hk_followup_does_not_reuse_half_year_for_annual_ttm_or_yearless_quarters() {
        let context = attach_evidence(json!({}), &financial_evidence(), "00700.HK")
            ["research_context"]
            .clone();
        for question in [
            "改看2026年全年业绩",
            "改看2026年度业绩",
            "改看三季度公告",
            "请核对TTM盈利",
        ] {
            assert!(
                reusable_context(&context, "00700.HK", question, false).is_none(),
                "incompatible {question}"
            );
        }
        assert!(reusable_context(&context, "00700.HK", "继续看2026年上半年盈利", false).is_some());
        let mut quarter = context;
        quarter["evidence"][0]["report_period"] = json!("2026Q3");
        assert!(reusable_context(&quarter, "00700.HK", "改看2026年上半年", false).is_none());
    }

    #[test]
    fn hk_money_claim_does_not_lose_its_units_when_another_metric_is_a_percent() {
        for claim in [
            "2026H1 净利润100，同比增长10%。[E1]",
            "EPS 12.639，ROE 20%。[E1]",
        ] {
            let model = json!({"reply":claim,"answer_sections":[]});
            assert!(
                validate_model_analysis(&model, "quick").is_err(),
                "unqualified amount {claim}"
            );
        }
        assert!(validate_model_analysis(
            &json!({"reply":"2026H1 净利润 CNY 100 百万元，同比增长10%。[E1]"}),
            "quick"
        )
        .is_ok());
        assert!(validate_model_analysis(
            &json!({"reply":"2026H1 净利润同比增长10%。[E1]"}),
            "quick"
        )
        .is_ok());
    }

    #[test]
    fn hk_model_money_claims_need_currency_and_unit_before_they_are_published() {
        let response = attach_evidence(json!({}), &financial_evidence(), "00700.HK");
        let mut model = json!({"reply":"盈利质量仍需核对现金流。[E1]","answer_sections":[
            {"title":"盈利质量","bullets":["2026H1 每股收益 12.639。[E1]"]},
            {"title":"资本配置","bullets":["分红与回购金额尚待披露。[E1]"]},
            {"title":"估值假设","bullets":["尚缺同币种TTM口径，无法自行计算市盈率。[E1]"]},
            {"title":"风险","bullets":["候选财务数值仍需公告核验。[E1]"]}
        ]});
        assert!(crate::agent_harness::merge_model_response(
            response.clone(),
            &model,
            "research",
            None
        )
        .is_err());
        model["answer_sections"][0]["bullets"][0] =
            json!("2026H1 每股收益 CNY 12.639 currency/share。[E1]");
        assert!(
            crate::agent_harness::merge_model_response(response, &model, "research", None).is_ok()
        );
    }

    #[test]
    fn hk_research_model_requires_capital_allocation_valuation_and_risks() {
        let response = attach_evidence(json!({}), &financial_evidence(), "00700.HK");
        let model = json!({"reply":"仅能核对EPS。[E1]","answer_sections":[
            {"title":"盈利质量","bullets":["2026H1 每股收益 CNY 12.639 currency/share。[E1]"]}
        ]});
        assert!(
            crate::agent_harness::merge_model_response(response, &model, "research", None).is_err()
        );
    }
}
