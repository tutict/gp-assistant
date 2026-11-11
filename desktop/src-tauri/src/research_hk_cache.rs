use crate::hk_market::{HkFinancialMetric, HkMarketCache};
use serde_json::{json, Value};
use std::collections::HashSet;

fn metric(label: &str, value: &HkFinancialMetric) -> String {
    let amount = |number| if value.currency.is_empty() { format!("{number}") } else { format!("{} {number}", value.currency) };
    match value.value.filter(|value| value.is_finite()) {
        Some(number) => format!("{label}：{}；单位 {}；来源 {}", amount(number), value.unit, value.source),
        None => format!("{label}：暂无；{}；币种 {}；单位 {}；来源 {}", value.reason.as_deref().unwrap_or("数据源未提供"), value.currency, value.unit, value.source),
    }
}

fn financial_source_url(source: &str, code: &str, official_url: Option<&str>) -> Option<String> {
    let source = source.to_ascii_lowercase();
    if source.contains("sina") {
        Some(format!("https://stock.finance.sina.com.cn/hkstock/finance/{code}.html"))
    } else if source.contains("eastmoney") {
        Some(format!("https://emweb.securities.eastmoney.com/PC_HKF10/pages/home.html?code={code}&type=web&color=b"))
    } else if source.contains("hkex") {
        official_url.map(ToOwned::to_owned)
    } else { None }
}

fn metric_metadata(value: &HkFinancialMetric, code: &str, official_url: Option<&str>) -> Value {
    let number = value.value.filter(|value| value.is_finite());
    json!({"value":number,"currency":value.currency,"unit":value.unit,"source":value.source,
        "reason":value.reason.as_deref().or_else(|| number.is_none().then_some("数据源未提供有效数值")),
        "source_url":financial_source_url(&value.source, code, official_url)})
}

pub(super) fn documents(cache: &HkMarketCache, codes: &[String]) -> Vec<Value> {
    let requested = codes.iter().filter(|code| code.to_ascii_uppercase().ends_with(".HK"))
        .filter_map(|code| crate::hk_market::canonical_hk_code(code)).collect::<HashSet<_>>();
    let mut documents = Vec::new();
    for (code, snapshot) in &cache.financials {
        if snapshot.code != *code || crate::hk_market::canonical_hk_code(code).as_deref() != Some(code.as_str())
            || (!codes.is_empty() && !requested.contains(code)) { continue; }
        let name = cache.securities.iter().find(|security| security.code == *code).map(|security| security.name.as_str()).unwrap_or(code);
        let title = format!("{name} {code}.HK 财务快照 {}", snapshot.normalized_period);
        let mut lines = vec![title.clone(),
            format!("报告期：{}；报表截止：{}；披露日期：{}；报表币种：{}", snapshot.normalized_period, snapshot.report_date.as_deref().unwrap_or("未提供"), snapshot.disclosed_at.as_deref().unwrap_or("未提供"), snapshot.currency),
            format!("来源：{}；抓取时间：{}；核验状态：{}；候选值需结合原始公告核验。", snapshot.candidate_source, snapshot.candidate_fetched_at, snapshot.verification_status),
            metric("每股收益 EPS", &snapshot.eps), metric("净资产收益率 ROE", &snapshot.roe),
            metric("营业总收入", &snapshot.revenue), metric("归母净利润", &snapshot.net_profit),
            metric("市盈率 PE", &snapshot.pe), metric("市净率 PB", &snapshot.pb), metric("股息率", &snapshot.dividend_yield),
            metric("总市值", &snapshot.market_cap), metric("总股本", &snapshot.total_shares), metric("流通股本", &snapshot.circulating_shares)];
        let source_url = financial_source_url(&snapshot.candidate_source, code, snapshot.official_source_url.as_deref());
        let mut metrics = serde_json::Map::new();
        for (key, value) in [("eps", &snapshot.eps), ("roe", &snapshot.roe), ("revenue", &snapshot.revenue),
            ("net_profit", &snapshot.net_profit), ("pe", &snapshot.pe), ("pb", &snapshot.pb),
            ("dividend_yield", &snapshot.dividend_yield), ("market_cap", &snapshot.market_cap),
            ("total_shares", &snapshot.total_shares), ("circulating_shares", &snapshot.circulating_shares)] {
            metrics.insert(key.to_string(), metric_metadata(value, code, snapshot.official_source_url.as_deref()));
        }
        let mut additional = snapshot.additional_metrics.iter().collect::<Vec<_>>();
        additional.sort_by_key(|(key, _)| *key);
        for (key, value) in additional {
            lines.push(metric(key, value));
            metrics.entry(key.clone()).or_insert_with(|| metric_metadata(value, code, snapshot.official_source_url.as_deref()));
        }
        let official_values = snapshot.official_values.iter().map(|(key, value)|
            (key.clone(), metric_metadata(value, code, snapshot.official_source_url.as_deref()))).collect::<serde_json::Map<_, _>>();
        if let Some(url) = source_url.as_deref() { lines.push(format!("财务原文：{url}")); }
        if let Some(url) = snapshot.official_source_url.as_deref() {
            lines.push(format!("官方披露线索：{url}；{}；核验状态 {}", snapshot.official_source_name.as_deref().unwrap_or("未提供来源名"), snapshot.verification_status));
        }
        if let Some(profile) = snapshot.company_profile.as_ref().filter(|profile| profile.code == *code) {
            lines.push(format!("公司：{}；行业：{}；主营业务：{}；公司资料来源：{}", profile.name, profile.industry, profile.business_summary, profile.source));
        }
        let mut errors = snapshot.field_errors.iter().collect::<Vec<_>>();
        errors.sort_by_key(|(field, _)| *field);
        for (field, error) in errors { lines.push(format!("数据提示 {field}：{error}")); }
        documents.push(json!({"document_id":format!("hk-financial-{code}-{}",snapshot.normalized_period),
            "title":title,"content":lines.join("\n"),"stock_codes":[format!("{code}.HK")],
            "source_tier":"financial_snapshot","source_name":snapshot.candidate_source,"sentiment":"uncertain",
            "published_at":snapshot.disclosed_at,"url":source_url,
            "metadata":{"market":"hk","report_period":snapshot.normalized_period,"report_date":snapshot.report_date,
                "raw_report_period":snapshot.raw_report_period,"period_end":snapshot.report_date,"disclosed_at":snapshot.disclosed_at,
                "currency":snapshot.currency,"fetched_at":snapshot.candidate_fetched_at,"verification_status":snapshot.verification_status,
                "audit_status":"not_provided","source_kind":"cached_hk_financial","metrics":metrics,
                "candidate_source":snapshot.candidate_source,"candidate_source_url":source_url,
                "supplemental_source":snapshot.supplemental_source,"supplemental_fetched_at":snapshot.supplemental_fetched_at,
                "official_source_url":snapshot.official_source_url,"official_source_name":snapshot.official_source_name,
                "official_values":official_values,"verification_conflicts":snapshot.verification_conflicts,"field_errors":snapshot.field_errors}}));
    }
    for (code, record) in &cache.quarterly_updates {
        if crate::hk_market::canonical_hk_code(code).as_deref() != Some(code.as_str()) || (!codes.is_empty() && !requested.contains(code)) { continue; }
        let Some(update) = record.update.as_ref().filter(|update| update.code == *code && !update.source_url.is_empty()) else { continue; };
        let mut lines = vec![format!("{} {code}.HK", update.title),
            format!("报告期：{}；截止：{}；披露：{}；币种：{}；单位：{}；审计状态：{}", update.period,update.period_end,update.disclosed_at,update.currency,update.unit,update.audit_status)];
        let mut metrics = serde_json::Map::new();
        for (key, label, value, currency, unit, scope) in [
            ("quarterly_revenue","单季营业收入",update.quarterly_revenue,update.currency.as_str(),update.unit.as_str(),"single_quarter"),
            ("nine_month_revenue","九个月累计营业收入",update.nine_month_revenue,update.currency.as_str(),update.unit.as_str(),"nine_month_cumulative"),
            ("quarterly_net_profit","单季净利润",update.quarterly_net_profit,update.currency.as_str(),update.unit.as_str(),"single_quarter"),
            ("nine_month_net_profit","九个月累计净利润",update.nine_month_net_profit,update.currency.as_str(),update.unit.as_str(),"nine_month_cumulative"),
            ("quarterly_total_comprehensive_income","单季综合收益总额",update.quarterly_total_comprehensive_income,update.currency.as_str(),update.unit.as_str(),"single_quarter"),
            ("nine_month_total_comprehensive_income","九个月累计综合收益总额",update.nine_month_total_comprehensive_income,update.currency.as_str(),update.unit.as_str(),"nine_month_cumulative"),
            ("quarterly_revenue_yoy_percent","单季营业收入同比",update.quarterly_revenue_yoy_percent,"","percent","single_quarter"),
            ("quarterly_revenue_qoq_percent","单季营业收入环比",update.quarterly_revenue_qoq_percent,"","percent","single_quarter"),
            ("nine_month_revenue_yoy_percent","九个月累计营业收入同比",update.nine_month_revenue_yoy_percent,"","percent","nine_month_cumulative"),
            ("quarterly_net_profit_yoy_percent","单季净利润同比",update.quarterly_net_profit_yoy_percent,"","percent","single_quarter"),
            ("quarterly_net_profit_qoq_percent","单季净利润环比",update.quarterly_net_profit_qoq_percent,"","percent","single_quarter"),
            ("nine_month_net_profit_yoy_percent","九个月累计净利润同比",update.nine_month_net_profit_yoy_percent,"","percent","nine_month_cumulative"),
            ("quarterly_total_comprehensive_income_yoy_percent","单季综合收益总额同比",update.quarterly_total_comprehensive_income_yoy_percent,"","percent","single_quarter"),
            ("quarterly_total_comprehensive_income_qoq_percent","单季综合收益总额环比",update.quarterly_total_comprehensive_income_qoq_percent,"","percent","single_quarter"),
            ("nine_month_total_comprehensive_income_yoy_percent","九个月累计综合收益总额同比",update.nine_month_total_comprehensive_income_yoy_percent,"","percent","nine_month_cumulative"),
        ] {
            let metric_value = HkFinancialMetric {value:value.filter(|value|value.is_finite()),currency:currency.to_string(),unit:unit.to_string(),source:update.source_name.clone(),reason:value.filter(|value|value.is_finite()).is_none().then(||"公告未提供或未能可靠提取此字段".to_string())};
            lines.push(metric(label, &metric_value));
            let mut metadata = metric_metadata(&metric_value, code, Some(&update.source_url));
            metadata["source_url"] = json!(update.source_url);
            metadata["period_scope"] = json!(scope);
            metrics.insert(key.to_string(), metadata);
        }
        lines.push(format!("来源：{}；原文：{}；未审计披露不代表已核验财务事实。", update.source_name, update.source_url));
        let verification_status = if update.audit_status == "unaudited" { "official_disclosure_unaudited" } else { "official_disclosure" };
        documents.push(json!({"document_id":format!("hk-quarterly-{code}-{}",update.period),"title":update.title,"content":lines.join("\n"),
            "stock_codes":[format!("{code}.HK")],"source_tier":"filing","source_name":update.source_name,"url":update.source_url,
            "published_at":update.disclosed_at,"sentiment":"uncertain","metadata":{"market":"hk","report_period":update.period,
                "report_date":update.period_end,"period_end":update.period_end,"disclosed_at":update.disclosed_at,
                "currency":update.currency,"unit":update.unit,"audit_status":update.audit_status,"verification_status":verification_status,
                "fetched_at":update.fetched_at,"source_url":update.source_url,"source_kind":"hkex_quarterly_update","metrics":metrics}}));
    }
    documents.sort_by(|left,right| left["document_id"].as_str().cmp(&right["document_id"].as_str()));
    documents
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cache() -> HkMarketCache {
        let mut cache = HkMarketCache::default();
        let snapshot = crate::hk_market::parse_hk_financial_snapshot(include_str!("../testdata/eastmoney-hk-financial.json"), "00700.HK", "1794116685279").unwrap();
        cache.financials.insert("00700".to_string(), snapshot);
        cache
    }

    #[test]
    fn hk_research_cache_financial_metadata_keeps_metric_currencies_zero_and_missing_reasons() {
        let mut cache = cache();
        let snapshot = cache.financials.get_mut("00700").unwrap();
        snapshot.net_profit.value = Some(0.0);
        snapshot.pe = HkFinancialMetric {value:Some(21.0),currency:String::new(),unit:"ratio".to_string(),source:"Tencent Finance".to_string(),reason:None};
        snapshot.market_cap = HkFinancialMetric {value:Some(5_000.0),currency:"HKD".to_string(),unit:"currency_total".to_string(),source:"Tencent Finance".to_string(),reason:None};
        snapshot.additional_metrics.insert("cash_flow".to_string(), HkFinancialMetric {value:None,currency:"CNY".to_string(),unit:"currency_total".to_string(),source:"Sina Finance HK statements".to_string(),reason:Some("现金流表未返回".to_string())});
        snapshot.official_source_url = Some("https://www1.hkexnews.hk/report.pdf".to_string());
        snapshot.official_source_name = Some("HKEXnews".to_string());
        snapshot.verification_status = "metadata_only".to_string();
        let docs = documents(&cache, &["00700.HK".to_string()]);
        let metadata = &docs[0]["metadata"];
        assert_eq!(metadata["metrics"]["net_profit"]["value"], 0.0);
        assert_eq!(metadata["metrics"]["net_profit"]["currency"], "CNY");
        assert_eq!(metadata["metrics"]["market_cap"]["currency"], "HKD");
        assert_eq!(metadata["metrics"]["market_cap"]["source"], "Tencent Finance");
        assert_eq!(metadata["metrics"]["eps"]["unit"], "currency/share");
        assert_eq!(metadata["metrics"]["pe"]["value"], 21.0);
        assert!(metadata["metrics"]["pb"]["value"].is_null());
        assert!(metadata["metrics"]["pb"]["reason"].as_str().unwrap().contains("Awaiting"));
        assert!(metadata["metrics"]["dividend_yield"]["value"].is_null());
        assert_eq!(metadata["metrics"]["cash_flow"]["reason"], "现金流表未返回");
        assert_eq!(metadata["report_period"], "2026H1");
        assert_eq!(metadata["report_date"], "2026-06-30");
        assert_eq!(metadata["period_end"], "2026-06-30");
        assert_eq!(metadata["disclosed_at"], docs[0]["published_at"]);
        assert_eq!(metadata["verification_status"], "metadata_only");
        assert_eq!(metadata["official_source_url"], "https://www1.hkexnews.hk/report.pdf");
        let content = docs[0]["content"].as_str().unwrap();
        assert!(content.contains("市盈率 PE：21"));
        assert!(content.contains("市净率 PB：暂无"));
        assert!(content.contains("HKD 5000"));
    }

    #[test]
    fn hk_research_cache_uses_the_candidate_financial_source_url_and_keeps_official_provenance_separate() {
        let mut cache = cache();
        let snapshot = crate::hk_market::parse_sina_hk_financial_snapshot(include_str!("../testdata/sina-hk-financial.html"), "00700.HK", "1794116685279").unwrap();
        cache.financials.insert("00700".to_string(), snapshot);
        cache.financials.get_mut("00700").unwrap().official_source_url = Some("https://www1.hkexnews.hk/disclosure.pdf".to_string());
        let docs = documents(&cache, &["00700.HK".to_string()]);
        assert_eq!(docs[0]["url"], "https://stock.finance.sina.com.cn/hkstock/finance/00700.html");
        assert_eq!(docs[0]["metadata"]["official_source_url"], "https://www1.hkexnews.hk/disclosure.pdf");
        assert_eq!(docs[0]["metadata"]["metrics"]["revenue"]["source"], "Sina Finance HK statements");
    }

    #[test]
    fn hk_research_cache_quarterly_separates_single_quarter_cumulative_zero_and_missing() {
        let mut cache = HkMarketCache::default();
        let mut update = crate::hk_market::parse_hkex_quarterly_update_text(include_str!("../testdata/smoore-hkex-q3-update.txt"),
            "06969.HK", "Financial Update for the Period Ended 30 September 2026", "2026-10-12", "https://www1.hkexnews.hk/update.pdf", "1794116685279").unwrap();
        update.quarterly_net_profit = None;
        update.nine_month_net_profit = Some(0.0);
        cache.quarterly_updates.insert("06969".to_string(), crate::hk_market::HkQuarterlyUpdateRecord {update:Some(update),checked_at:"checked".to_string(),error:None});
        let docs = documents(&cache, &["06969.HK".to_string()]);
        let content = docs[0]["content"].as_str().unwrap();
        assert!(content.contains("单季营业收入：CNY 3946.2"));
        assert!(content.contains("九个月累计营业收入：CNY 11155.1"));
        assert!(content.contains("单季净利润：暂无"));
        assert!(content.contains("九个月累计净利润：CNY 0"));
        assert!(!content.contains("Some(") && !content.contains("None"));
        assert!(content.contains("million") && content.contains("unaudited"));
        assert_eq!(docs[0]["url"], "https://www1.hkexnews.hk/update.pdf");
        assert_eq!(docs[0]["metadata"]["period_end"], "2026-09-30");
        assert_eq!(docs[0]["metadata"]["audit_status"], "unaudited");
        assert_eq!(docs[0]["metadata"]["verification_status"], "official_disclosure_unaudited");
        assert_eq!(docs[0]["metadata"]["metrics"]["quarterly_revenue"]["period_scope"], "single_quarter");
        assert_eq!(docs[0]["metadata"]["metrics"]["nine_month_revenue"]["period_scope"], "nine_month_cumulative");
        assert!(docs[0]["metadata"]["metrics"]["quarterly_net_profit"]["value"].is_null());
        assert!(docs[0]["metadata"]["metrics"]["quarterly_net_profit"]["reason"].as_str().is_some());
        assert_eq!(docs[0]["metadata"]["metrics"]["nine_month_net_profit"]["value"], 0.0);
    }

    #[test]
    fn hk_research_cache_documents_keep_report_currency_source_and_stock_identity() {
        let docs = documents(&cache(), &["00700.HK".to_string()]);
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0]["stock_codes"], json!(["00700.HK"]));
        assert_eq!(docs[0]["source_tier"], "financial_snapshot");
        let content = docs[0]["content"].as_str().unwrap();
        assert!(content.contains("2026H1"));
        assert!(content.contains("CNY"));
        assert!(content.contains("12.639"));
        assert!(content.contains("Eastmoney HK financial indicators"));
        assert!(content.contains("候选值"));
        assert_ne!(docs[0]["published_at"], docs[0]["metadata"]["fetched_at"]);
    }

    #[test]
    fn hk_research_cache_documents_are_scoped_and_do_not_relabel_other_securities() {
        let mut cache = cache();
        let docs = documents(&cache, &["00005.HK".to_string()]);
        assert!(docs.is_empty());
        cache.financials.get_mut("00700").unwrap().code = "00005".to_string();
        assert!(documents(&cache, &["00700.HK".to_string()]).is_empty());
        assert!(documents(&cache, &["000700.SH".to_string()]).is_empty());
    }

    #[test]
    fn hk_cached_financial_document_text_is_stable_across_map_iteration_orders() {
        let mut original = cache();
        let snapshot = original.financials.get_mut("00700").unwrap();
        for index in 0..30 { snapshot.field_errors.insert(format!("field-{index}"),format!("数据提示-{index}")); }
        let baseline = documents(&original,&[])[0]["content"].clone();
        for _ in 0..10 {
            let restored: HkMarketCache = serde_json::from_value(serde_json::to_value(&original).unwrap()).unwrap();
            assert_eq!(documents(&restored,&[])[0]["content"],baseline);
        }
    }

    #[test]
    fn hk_cached_financials_are_retrievable_as_citations_in_the_hong_kong_store() {
        let root = std::env::temp_dir().join(format!("gp-hk-research-financial-{}-{}", std::process::id(), crate::market::epoch_millis()));
        let store = crate::research::ResearchStore::open(root.join("research-hk.sqlite")).unwrap();
        store.ingest_documents(&documents(&cache(), &["00700.HK".to_string()])).unwrap();
        let result = store.query(&json!({"query":"00700.HK 每股收益 财务","stock_code":"00700.HK","top_k":8})).unwrap();
        assert!(!result["citations"].as_array().unwrap().is_empty());
        assert_eq!(result["citations"][0]["source_tier"], "financial_snapshot");
        assert!(result["answer"].as_str().unwrap().contains("12.639"));
        let agent = store.agent_evidence(&json!({"query":"竞争格局","stock_code":"00700.HK"})).unwrap();
        assert_eq!(agent["citations"][0]["metadata"]["metrics"]["eps"]["value"], 12.639);
        assert_eq!(agent["citations"][0]["metadata"]["metrics"]["bps"]["currency"], "CNY");
        assert!(agent["citations"][0]["metadata"]["metrics"]["total_shares"]["reason"].as_str().is_some());
        assert_eq!(agent["citations"][0]["metadata"]["verification_status"], "unverified");
        let messages = store.messages(&json!({"stock_code":"00700.HK"})).unwrap();
        assert_eq!(messages["items"][0]["url"], "https://emweb.securities.eastmoney.com/PC_HKF10/pages/home.html?code=00700&type=web&color=b");
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
}
