use super::{
    build_http_client_with_proxy, cached_market_data, epoch_millis, normalize_stock_code,
    powershell_http_get_bytes_with_headers, read_json_file,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    env, fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::Manager;

const NEWS_CACHE_FILE: &str = "news-cache.json";
const SOURCE_TIER_NEWS: &str = "news";
const SOURCE_TIER_COMMUNITY: &str = "community";
const SOURCE_SINA_FINANCE: &str = "\u{65b0}\u{6d6a}\u{8d22}\u{7ecf}";
const SOURCE_THS_F10: &str = "\u{540c}\u{82b1}\u{987a}F10\u{8d44}\u{8baf}";
const CHAIN_RELATION_TYPES: [&str; 3] =
    ["supply_chain", "manufacturing_chain", "upstream_material"];
const NEWS_TIMEOUT_SECS: u64 = 8;
const NEWS_ANDROID_TIMEOUT_SECS: u64 = 5;
const NEWS_MAX_CACHE_ITEMS: usize = 2_000;

pub(crate) async fn api_news_rag_impl(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    let data = cached_market_data(&app)?;
    let stock_by_code = stock_map(&data);
    let scope_codes = scope_codes(&payload, &stock_by_code);
    if scope_codes.is_empty() {
        return Ok(json!({
            "scope_codes": [],
            "relation_count": 0,
            "message_count": 0,
            "findings": [],
            "sentiment_groups": sentiment_groups(&[], "plain_news"),
            "notes": ["请输入目标股票代码后再分析上下游消息；不会再用当前筛选结果自动代替目标股票。"]
        }));
    }

    let stock_only = news_scope_mode(&payload) == "stock_only";
    let relations = if stock_only {
        Vec::new()
    } else {
        scope_chain_relations(&data, &scope_codes)
    };
    let related_codes = if stock_only {
        scope_codes.clone()
    } else {
        related_codes(&scope_codes, &relations)
    };
    let related_stocks = related_codes
        .iter()
        .filter_map(|code| stock_by_code.get(code).cloned())
        .collect::<Vec<_>>();
    let days = payload_u64(&payload, "days", 30, 1, 365) as i64;
    let max_items = payload_u64(&payload, "max_items", 24, 1, 100) as usize;
    let mobile_fast = payload
        .get("mobile_fast")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let include_us_market_brief = payload
        .get("include_us_market_brief")
        .and_then(Value::as_bool)
        .unwrap_or(!mobile_fast)
        && !mobile_fast
        && env_bool("GP_NEWS_ENABLE_US_MARKET_BRIEF", true);

    let news_result = if mobile_fast {
        fetch_android_short_news_items(&related_stocks, &relations, days, Some(&payload)).await
    } else {
        fetch_news_items(&related_stocks, &relations, days, None).await
    };
    let us_market_brief = if include_us_market_brief {
        Some(fetch_us_market_brief().await)
    } else {
        None
    };
    let (fetched, adapter_notes) = news_result;
    let cache_path = news_cache_path(&app)?;
    let cached = read_news_cache_items(&cache_path);
    let merged_cache = merge_news_items(cached, fetched);
    write_news_cache_items(&cache_path, &merged_cache)?;
    let evidence = query_evidence(&merged_cache, &related_codes, days, max_items);

    let base_findings = if stock_only {
        build_stock_news_findings(&scope_codes, &evidence, &stock_by_code)
    } else {
        build_findings(&scope_codes, &relations, &evidence, &stock_by_code)
    };
    let (findings, llm_notes, analysis_mode) = if stock_only {
        (
            base_findings,
            vec!["\u{5df2}\u{6309}\u{76ee}\u{6807}\u{4e2a}\u{80a1}\u{672c}\u{8eab}\u{6d88}\u{606f}\u{5206}\u{7ec4}\u{5c55}\u{793a}\u{5229}\u{597d}/\u{5229}\u{7a7a}\u{ff1b}\u{4e0a}\u{4e0b}\u{6e38} RAG \u{9700}\u{8981}\u{65f6}\u{8bf7}\u{5355}\u{72ec}\u{4f7f}\u{7528}\u{540c}\u{6b65}\u{5305}\u{6216}\u{5173}\u{7cfb}\u{56fe}\u{5206}\u{6790}\u{3002}".to_string()],
            "plain_news".to_string(),
        )
    } else {
        apply_llm_analysis(
            payload.get("llm"),
            &scope_codes,
            &relations,
            &evidence,
            &stock_by_code,
            &base_findings,
        )
        .await
    };
    let mut notes = adapter_notes;
    notes.extend(llm_notes);
    if stock_only {
        notes.push("\u{5f53}\u{524d}\u{4e3a}\u{4e2a}\u{80a1}\u{6d88}\u{606f}\u{6a21}\u{5f0f}\u{ff1a}\u{53ea}\u{62c9}\u{53d6}\u{76ee}\u{6807}\u{80a1}\u{672c}\u{8eab}\u{6d88}\u{606f}\u{5e76}\u{5206}\u{7ec4}\u{5229}\u{597d}/\u{5229}\u{7a7a}\u{ff0c}\u{672a}\u{8ba1}\u{7b97}\u{4e0a}\u{4e0b}\u{6e38}\u{5173}\u{8054}\u{5f71}\u{54cd}\u{3002}".to_string());
        notes.push("\u{4e0a}\u{4e0b}\u{6e38} RAG \u{8bf7}\u{4f7f}\u{7528}\u{540c}\u{6b65}\u{5305}\u{3001}\u{5173}\u{7cfb}\u{56fe}\u{6216}\u{79bb}\u{7ebf} RAG \u{529f}\u{80fd}\u{5355}\u{72ec}\u{5206}\u{6790}\u{3002}".to_string());
    } else {
        notes.push("RAG \u{53ea}\u{5728}\u{5df2}\u{6709}\u{80a1}\u{7968}\u{5173}\u{7cfb}\u{56fe}\u{8303}\u{56f4}\u{5185}\u{5206}\u{6790}\u{6d88}\u{606f}\u{ff0c}\u{4e0d}\u{4ece}\u{65b0}\u{95fb}\u{81ea}\u{52a8}\u{53d1}\u{660e}\u{4e0a}\u{4e0b}\u{6e38}\u{5173}\u{7cfb}\u{3002}".to_string());
        if relations.is_empty() {
            notes.push("\u{5f53}\u{524d}\u{8303}\u{56f4}\u{6ca1}\u{6709}\u{4f9b}\u{5e94}\u{94fe}\u{3001}\u{5236}\u{9020}\u{94fe}\u{6216}\u{4e0a}\u{6e38}\u{6750}\u{6599}\u{5173}\u{7cfb}\u{ff0c}\u{7ed3}\u{679c}\u{4ec5}\u{4fdd}\u{7559}\u{53ef}\u{89e3}\u{91ca}\u{63d0}\u{793a}\u{3002}".to_string());
        }
    }
    notes.push(format!(
        "\u{6d88}\u{606f}\u{7f13}\u{5b58}\u{ff1a}{}\u{3002}",
        cache_path.display()
    ));
    if evidence.is_empty() {
        notes.push("当前时间窗口没有命中可用消息。".to_string());
    }

    Ok(json!({
        "scope_codes": scope_codes,
        "relation_count": relations.len(),
        "message_count": evidence.len(),
        "findings": findings,
        "sentiment_groups": sentiment_groups(&evidence, &analysis_mode),
        "us_market_brief": us_market_brief,
        "notes": notes
    }))
}

fn stock_map(data: &Value) -> HashMap<String, Value> {
    let mut result = HashMap::new();
    if let Some(stocks) = data.get("stocks").and_then(Value::as_array) {
        for stock in stocks {
            if let Some(code) = stock
                .get("code")
                .and_then(Value::as_str)
                .and_then(normalize_stock_code)
            {
                result.insert(code, stock.clone());
            }
        }
    }
    result
}

fn news_scope_mode(payload: &Value) -> &'static str {
    match payload
        .get("scope_mode")
        .or_else(|| payload.get("mode"))
        .and_then(Value::as_str)
        .unwrap_or("")
    {
        "stock_only" | "single_stock" | "plain_news" => "stock_only",
        _ => "upstream",
    }
}

fn scope_codes(payload: &Value, stock_by_code: &HashMap<String, Value>) -> Vec<String> {
    let mut raw_codes = Vec::new();
    if let Some(code) = payload.get("code").and_then(Value::as_str) {
        raw_codes.push(code.to_string());
    }
    if let Some(seed_codes) = payload.get("seed_codes").and_then(Value::as_array) {
        for code in seed_codes.iter().filter_map(Value::as_str) {
            raw_codes.push(code.to_string());
        }
    }

    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for raw in raw_codes {
        let Some(code) = normalize_stock_code(&raw) else {
            continue;
        };
        if !stock_by_code.contains_key(&code) || !seen.insert(code.clone()) {
            continue;
        }
        result.push(code);
        if result.len() >= 10 {
            break;
        }
    }
    result
}

fn scope_chain_relations(data: &Value, scope_codes: &[String]) -> Vec<Value> {
    let scope = scope_codes.iter().cloned().collect::<HashSet<_>>();
    let mut relations = data
        .get("relations")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|relation| {
            let source = relation
                .get("source_code")
                .and_then(Value::as_str)
                .and_then(normalize_stock_code)?;
            let target = relation
                .get("target_code")
                .and_then(Value::as_str)
                .and_then(normalize_stock_code)?;
            let relation_type = relation
                .get("relation_type")
                .and_then(Value::as_str)
                .unwrap_or("");
            if !CHAIN_RELATION_TYPES.contains(&relation_type)
                || (!scope.contains(&source) && !scope.contains(&target))
            {
                return None;
            }
            let mut normalized = relation.as_object()?.clone();
            normalized.insert("source_code".to_string(), json!(source));
            normalized.insert("target_code".to_string(), json!(target));
            Some(Value::Object(normalized))
        })
        .collect::<Vec<_>>();
    relations.sort_by(|left, right| relation_weight(right).total_cmp(&relation_weight(left)));
    relations.truncate(30);
    relations
}

fn related_codes(scope_codes: &[String], relations: &[Value]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut codes = Vec::new();
    for code in scope_codes {
        if seen.insert(code.clone()) {
            codes.push(code.clone());
        }
    }
    for relation in relations {
        for key in ["source_code", "target_code"] {
            if let Some(code) = relation.get(key).and_then(Value::as_str) {
                if seen.insert(code.to_string()) {
                    codes.push(code.to_string());
                }
            }
        }
    }
    codes.sort();
    codes
}

async fn fetch_android_short_news_items(
    stocks: &[Value],
    relations: &[Value],
    days: i64,
    network_payload: Option<&Value>,
) -> (Vec<Value>, Vec<String>) {
    let client = match build_http_client_with_proxy(
        "Mozilla/5.0 GuXuanYou/0.3 android-news-short",
        Duration::from_secs(NEWS_ANDROID_TIMEOUT_SECS),
        network_payload,
    ) {
        Ok(client) => client,
        Err(error) => {
            return (
                Vec::new(),
                vec![format!("Android news short HTTP client failed: {error}")],
            )
        }
    };
    let short_stocks = stocks.iter().take(1).cloned().collect::<Vec<_>>();
    let mut notes = Vec::new();
    let mut items = Vec::new();
    notes.push("Android short news source enabled: Eastmoney Guba, Eastmoney search, Sina stock news, THS F10.".to_string());

    match fetch_guba_community(&client, &short_stocks, relations, days).await {
        Ok(guba_items) => {
            notes.push(format!("Android short source Eastmoney Guba returned {} items.", guba_items.len()));
            items.extend(guba_items);
        }
        Err(error) => notes.push(format!(
            "Android short source Eastmoney Guba failed: {}",
            limit_chars(&error, 160)
        )),
    }
    match fetch_eastmoney_stock_news(&client, &short_stocks, relations, days).await {
        Ok(news_items) => {
            notes.push(format!("Android short source Eastmoney stock news returned {} items.", news_items.len()));
            items.extend(news_items);
        }
        Err(error) => notes.push(format!(
            "Android short source Eastmoney stock news failed: {}",
            limit_chars(&error, 160)
        )),
    }
    match fetch_sina_stock_news(&client, &short_stocks, relations, days).await {
        Ok(news_items) => {
            notes.push(format!("Android short source Sina stock news returned {} items.", news_items.len()));
            items.extend(news_items);
        }
        Err(error) => notes.push(format!(
            "Android short source Sina stock news failed: {}",
            limit_chars(&error, 160)
        )),
    }
    match fetch_ths_stock_news(&client, &short_stocks, relations, days).await {
        Ok(news_items) => {
            notes.push(format!("Android short source THS F10 returned {} items.", news_items.len()));
            items.extend(news_items);
        }
        Err(error) => notes.push(format!(
            "Android short source THS F10 failed: {}",
            limit_chars(&error, 160)
        )),
    }

    (dedupe_news_items(items), notes)
}

async fn fetch_news_items(
    stocks: &[Value],
    relations: &[Value],
    days: i64,
    network_payload: Option<&Value>,
) -> (Vec<Value>, Vec<String>) {
    let client = match build_http_client_with_proxy(
        "Mozilla/5.0 GuXuanYou/0.3 news-rag",
        Duration::from_secs(NEWS_TIMEOUT_SECS),
        network_payload,
    )
    {
        Ok(client) => client,
        Err(error) => {
            return (
                Vec::new(),
                vec![format!("创建新闻 HTTP 客户端失败：{error}")],
            )
        }
    };
    let mut notes = Vec::new();
    let mut items = Vec::new();

    if env_bool("GP_NEWS_ENABLE_GUBA", true) {
        match fetch_guba_community(&client, stocks, relations, days).await {
            Ok(guba_items) => {
                if guba_items.is_empty() {
                    notes.push("已尝试东方财富股吧社区抓取，当前窗口未命中讨论。".to_string());
                } else {
                    notes.push(format!(
                        "已通过东方财富股吧抓取并缓存 {} 条社区讨论；社区内容仅作情绪/传闻信号。",
                        guba_items.len()
                    ));
                    items.extend(guba_items);
                }
            }
            Err(error) => notes.push(format!(
                "东方财富股吧社区抓取不可用，已继续使用其他消息源：{}",
                limit_chars(&error, 160)
            )),
        }
    } else {
        notes.push(
            "东方财富股吧社区抓取未启用；可设置 GP_NEWS_ENABLE_GUBA=true 后接入社区讨论。"
                .to_string(),
        );
    }

    if env_bool("GP_NEWS_ENABLE_TRADITIONAL_MEDIA", true) {
        if env_bool("GP_NEWS_ENABLE_SINA", true) {
            match fetch_sina_stock_news(&client, stocks, relations, days).await {
                Ok(news_items) => {
                    if news_items.is_empty() {
                        notes.push("\u{5df2}\u{5c1d}\u{8bd5}\u{65b0}\u{6d6a}\u{8d22}\u{7ecf}\u{4e2a}\u{80a1}\u{8d44}\u{8baf}\u{6293}\u{53d6}\u{ff0c}\u{5f53}\u{524d}\u{7a97}\u{53e3}\u{672a}\u{547d}\u{4e2d}\u{6d88}\u{606f}\u{3002}".to_string());
                    } else {
                        notes.push(format!(
                            "\u{5df2}\u{901a}\u{8fc7}\u{65b0}\u{6d6a}\u{8d22}\u{7ecf}\u{4e2a}\u{80a1}\u{8d44}\u{8baf}\u{6293}\u{53d6}\u{5e76}\u{7f13}\u{5b58} {} \u{6761}\u{4f20}\u{7edf}\u{5a92}\u{4f53}\u{6d88}\u{606f}\u{3002}",
                            news_items.len()
                        ));
                        items.extend(news_items);
                    }
                }
                Err(error) => notes.push(format!(
                    "\u{65b0}\u{6d6a}\u{8d22}\u{7ecf}\u{4e2a}\u{80a1}\u{8d44}\u{8baf}\u{6293}\u{53d6}\u{4e0d}\u{53ef}\u{7528}\u{ff0c}\u{5df2}\u{7ee7}\u{7eed}\u{4f7f}\u{7528}\u{5176}\u{5b83}\u{6d88}\u{606f}\u{6e90}\u{ff1a}{}",
                    limit_chars(&error, 160)
                )),
            }
        }
        if env_bool("GP_NEWS_ENABLE_THS", true) {
            match fetch_ths_stock_news(&client, stocks, relations, days).await {
                Ok(news_items) => {
                    if news_items.is_empty() {
                        notes.push("\u{5df2}\u{5c1d}\u{8bd5}\u{540c}\u{82b1}\u{987a} F10 \u{8d44}\u{8baf}\u{6293}\u{53d6}\u{ff0c}\u{5f53}\u{524d}\u{7a97}\u{53e3}\u{672a}\u{547d}\u{4e2d}\u{6d88}\u{606f}\u{3002}".to_string());
                    } else {
                        notes.push(format!(
                            "\u{5df2}\u{901a}\u{8fc7}\u{540c}\u{82b1}\u{987a} F10 \u{8d44}\u{8baf}\u{6293}\u{53d6}\u{5e76}\u{7f13}\u{5b58} {} \u{6761}\u{4f20}\u{7edf}\u{5a92}\u{4f53}\u{6d88}\u{606f}\u{3002}",
                            news_items.len()
                        ));
                        items.extend(news_items);
                    }
                }
                Err(error) => notes.push(format!(
                    "\u{540c}\u{82b1}\u{987a} F10 \u{8d44}\u{8baf}\u{6293}\u{53d6}\u{4e0d}\u{53ef}\u{7528}\u{ff0c}\u{5df2}\u{7ee7}\u{7eed}\u{4f7f}\u{7528}\u{5176}\u{5b83}\u{6d88}\u{606f}\u{6e90}\u{ff1a}{}",
                    limit_chars(&error, 160)
                )),
            }
        }
    } else {
        notes.push("\u{4f20}\u{7edf}\u{8d22}\u{7ecf}\u{5a92}\u{4f53}\u{6293}\u{53d6}\u{5df2}\u{5173}\u{95ed}\u{ff1b}\u{53ef}\u{8bbe}\u{7f6e} GP_NEWS_ENABLE_TRADITIONAL_MEDIA=true \u{540e}\u{91cd}\u{65b0}\u{542f}\u{7528}\u{3002}".to_string());
    }

    if env_bool("GP_NEWS_ENABLE_EASTMONEY", true) || env_bool("GP_NEWS_ENABLE_AKSHARE", true) {
        match fetch_eastmoney_stock_news(&client, stocks, relations, days).await {
            Ok(news_items) => {
                if news_items.is_empty() {
                    notes.push("已尝试东方财富个股新闻接口抓取，当前窗口未命中消息。".to_string());
                } else {
                    notes.push(format!(
                        "已通过东方财富个股新闻接口抓取并缓存 {} 条消息。",
                        news_items.len()
                    ));
                    items.extend(news_items);
                }
            }
            Err(error) => notes.push(format!(
                "东方财富个股新闻抓取不可用，已继续使用缓存：{}",
                limit_chars(&error, 160)
            )),
        }
    } else {
        notes.push(
            "东方财富个股新闻抓取已关闭；可设置 GP_NEWS_ENABLE_EASTMONEY=true 后重新启用。"
                .to_string(),
        );
    }

    (dedupe_news_items(items), notes)
}

async fn fetch_us_market_brief() -> Value {
    let client = match build_http_client_with_proxy(
        "Mozilla/5.0 GuXuanYou/0.3 us-market-brief",
        Duration::from_secs(env_usize("GP_NEWS_US_MARKET_TIMEOUT_SECS", 5, 2, 15) as u64),
        None,
    )
    {
        Ok(client) => client,
        Err(error) => {
            return unavailable_us_market_brief(format!("创建美股行情 HTTP 客户端失败：{error}"))
        }
    };
    match fetch_sina_us_market_brief(&client).await {
        Ok(brief) => brief,
        Err(error) => unavailable_us_market_brief(format!(
            "新浪财经全球指数暂不可用：{}",
            limit_chars(&error, 160)
        )),
    }
}

async fn fetch_sina_us_market_brief(client: &reqwest::Client) -> Result<Value, String> {
    let url = "https://hq.sinajs.cn/list=gb_dji,gb_ixic,gb_inx,gb_vix";
    let text = http_get_text_with_headers_timeout(
        client,
        url,
        "https://finance.sina.com.cn/",
        env_usize("GP_NEWS_US_MARKET_TIMEOUT_SECS", 5, 2, 15) as u64,
    )
    .await
    .map_err(|error| format!("Sina US market request failed: {error}"))?;
    parse_sina_us_market_brief(&text)
}

fn parse_sina_us_market_brief(text: &str) -> Result<Value, String> {
    let specs = [
        ("gb_dji", "DJI", "道琼斯"),
        ("gb_ixic", "IXIC", "纳斯达克"),
        ("gb_inx", "SPX", "标普500"),
        ("gb_vix", "VIX", "VIX"),
    ];
    let mut items = Vec::new();
    for (hq_key, symbol, fallback_name) in specs {
        let Some(raw) = extract_sina_hq_string(text, hq_key) else {
            continue;
        };
        if let Some(item) = parse_sina_us_index_item(&raw, symbol, fallback_name) {
            items.push(item);
        }
    }
    if items.is_empty() {
        return Err("Sina US market payload did not include usable indexes".to_string());
    }
    Ok(build_us_market_brief(items, "新浪财经全球指数", Vec::new()))
}

fn parse_sina_us_index_item(raw: &str, symbol: &str, fallback_name: &str) -> Option<Value> {
    let fields = raw.split(',').map(str::trim).collect::<Vec<_>>();
    if fields.len() < 4 || fields.first().copied().unwrap_or_default().is_empty() {
        return None;
    }
    let price = parse_f64_field(fields.get(1).copied())?;
    let change_percent = parse_f64_field(fields.get(2).copied()).unwrap_or(0.0) / 100.0;
    let change = parse_f64_field(fields.get(4).copied()).unwrap_or(price * change_percent);
    let as_of = fields
        .get(3)
        .copied()
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(current_iso_like);
    Some(json!({
        "symbol": symbol,
        "name": fields.first().copied().unwrap_or(fallback_name),
        "price": price,
        "change": change,
        "change_percent": change_percent,
        "as_of": as_of
    }))
}

fn build_us_market_brief(mut items: Vec<Value>, source: &str, notes: Vec<String>) -> Value {
    items.sort_by(|left, right| {
        us_market_sort_rank(left)
            .cmp(&us_market_sort_rank(right))
            .then_with(|| {
                left.get("symbol")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .cmp(right.get("symbol").and_then(Value::as_str).unwrap_or(""))
            })
    });
    let directional = items
        .iter()
        .filter(|item| item.get("symbol").and_then(Value::as_str) != Some("VIX"))
        .filter_map(|item| item.get("change_percent").and_then(Value::as_f64))
        .collect::<Vec<_>>();
    let average = if directional.is_empty() {
        0.0
    } else {
        directional.iter().sum::<f64>() / directional.len() as f64
    };
    let rising = directional.iter().filter(|value| **value > 0.001).count();
    let falling = directional.iter().filter(|value| **value < -0.001).count();
    let stance = us_market_stance(average, rising, falling);
    let as_of = items
        .iter()
        .filter_map(|item| item.get("as_of").and_then(Value::as_str))
        .max()
        .unwrap_or("")
        .to_string();
    json!({
        "status": "ok",
        "source": source,
        "stance": stance,
        "headline": us_market_headline(stance, average, rising, falling),
        "average_change_percent": average,
        "as_of": if as_of.is_empty() { current_iso_like() } else { as_of },
        "items": items,
        "notes": notes
    })
}

fn unavailable_us_market_brief(note: String) -> Value {
    json!({
        "status": "unavailable",
        "source": "新浪财经全球指数",
        "stance": "unavailable",
        "headline": "美股风向暂不可用",
        "average_change_percent": null,
        "as_of": current_iso_like(),
        "items": [],
        "notes": [note, "美股风向简报不使用 Yahoo 作为默认源，避免代理依赖；行情源失败不影响个股消息归类。"]
    })
}

fn us_market_sort_rank(item: &Value) -> usize {
    match item.get("symbol").and_then(Value::as_str).unwrap_or("") {
        "SPX" => 0,
        "IXIC" => 1,
        "DJI" => 2,
        "VIX" => 3,
        _ => 9,
    }
}
fn us_market_stance(average: f64, rising: usize, falling: usize) -> &'static str {
    if average >= 0.003 && rising >= 2 {
        "positive"
    } else if average <= -0.003 && falling >= 2 {
        "negative"
    } else if rising > 0 && falling > 0 {
        "mixed"
    } else {
        "neutral"
    }
}
fn us_market_headline(stance: &str, average: f64, rising: usize, falling: usize) -> String {
    let average_text = signed_percent_text(average);
    match stance {
        "positive" => format!("美股偏强：三大指数多数上涨，平均变动 {average_text}。"),
        "negative" => format!("美股偏弱：三大指数多数回落，平均变动 {average_text}。"),
        "mixed" => {
            format!("美股分化：上涨 {rising} 个、下跌 {falling} 个，平均变动 {average_text}。")
        }
        _ => format!("美股震荡：三大指数方向不强，平均变动 {average_text}。"),
    }
}
fn signed_percent_text(value: f64) -> String {
    let sign = if value > 0.0 { "+" } else { "" };
    format!("{sign}{:.2}%", value * 100.0)
}
fn extract_sina_hq_string(text: &str, hq_key: &str) -> Option<String> {
    let marker = format!("hq_str_{hq_key}=\"");
    let start = text.find(&marker)? + marker.len();
    let end = text[start..].find('"').map(|offset| start + offset)?;
    Some(text[start..end].to_string())
}
fn parse_f64_field(value: Option<&str>) -> Option<f64> {
    let cleaned = value?.trim().replace(',', "");
    if cleaned.is_empty() || cleaned == "--" {
        None
    } else {
        cleaned
            .parse::<f64>()
            .ok()
            .filter(|number| number.is_finite())
    }
}

async fn fetch_guba_community(
    client: &reqwest::Client,
    stocks: &[Value],
    relations: &[Value],
    days: i64,
) -> Result<Vec<Value>, String> {
    let mut items = Vec::new();
    let cutoff = cutoff_epoch_millis(days);
    let relation_map = relation_map(relations);
    let max_stocks = env_usize("GP_NEWS_GUBA_MAX_STOCKS", 6, 0, 50);
    let max_posts = env_usize("GP_NEWS_GUBA_MAX_POSTS", 5, 0, 50);
    for stock in stocks.iter().take(max_stocks) {
        let Some(code) = stock
            .get("code")
            .and_then(Value::as_str)
            .and_then(normalize_stock_code)
        else {
            continue;
        };
        let Some(digits) = code_digits(&code) else {
            continue;
        };
        let url = format!("https://guba.eastmoney.com/list,{digits}.html");
        let html = http_get_text(client, &url).await?;
        items.extend(parse_guba_article_list(
            &html,
            stock,
            &relation_map,
            cutoff,
            max_posts,
        ));
    }
    for url in configured_urls("GP_NEWS_GUBA_URLS") {
        let html = http_get_text(client, &url).await?;
        if let Some(item) = parse_guba_post_article(&html, &url, stocks, &relation_map, cutoff) {
            items.push(item);
        }
    }
    Ok(items)
}

async fn fetch_eastmoney_stock_news(
    client: &reqwest::Client,
    stocks: &[Value],
    relations: &[Value],
    days: i64,
) -> Result<Vec<Value>, String> {
    let mut items = Vec::new();
    let cutoff = cutoff_epoch_millis(days);
    let relation_map = relation_map(relations);
    for stock in stocks.iter().take(8) {
        let Some(code) = stock
            .get("code")
            .and_then(Value::as_str)
            .and_then(normalize_stock_code)
        else {
            continue;
        };
        let Some(digits) = code_digits(&code) else {
            continue;
        };
        let callback = format!("jQuery{}_{}", 3510179294063109245u64, epoch_millis());
        let param = json!({
            "uid": "", "keyword": digits, "type": ["cmsArticleWebOld"], "client": "web", "clientType": "web", "clientVersion": "curr",
            "param": {"cmsArticleWebOld": {"searchScope": "default", "sort": "default", "pageIndex": 1, "pageSize": 10, "preTag": "<em>", "postTag": "</em>"}}
        }).to_string();
        let now = epoch_millis().to_string();
        let mut url = reqwest::Url::parse("https://search-api-web.eastmoney.com/search/jsonp")
            .map_err(|error| format!("Eastmoney news URL parse failed: {error}"))?;
        url.query_pairs_mut()
            .append_pair("cb", &callback)
            .append_pair("param", &param)
            .append_pair("_", &now);
        let referer = format!("https://so.eastmoney.com/news/s?keyword={digits}");
        let text =
            http_get_text_with_headers_timeout(client, url.as_str(), &referer, NEWS_TIMEOUT_SECS)
                .await
                .map_err(|error| format!("Eastmoney news request failed: {error}"))?;
        let data =
            parse_jsonp(&text).ok_or_else(|| "Eastmoney news JSONP parse failed".to_string())?;
        let rows = data
            .get("result")
            .and_then(|value| value.get("cmsArticleWebOld"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for row in rows.into_iter().take(10) {
            let title = clean_html(row.get("title").and_then(Value::as_str).unwrap_or(""));
            if title.is_empty() {
                continue;
            }
            let summary = clean_html(row.get("content").and_then(Value::as_str).unwrap_or(""));
            let published_at =
                parse_news_time(row.get("date").and_then(Value::as_str).unwrap_or(""));
            let published_epoch =
                parse_time_epoch_millis(&published_at).unwrap_or_else(epoch_millis);
            if published_epoch < cutoff {
                continue;
            }
            let article_code = row.get("code").and_then(Value::as_str).unwrap_or("");
            let url = if article_code.trim().is_empty() {
                format!("https://so.eastmoney.com/news/s?keyword={digits}")
            } else {
                format!("http://finance.eastmoney.com/a/{article_code}.html")
            };
            items.push(news_item(json!({
                "title": title,
                "summary": if summary.is_empty() { title.clone() } else { summary.clone() },
                "source": row.get("mediaName").and_then(Value::as_str).filter(|value| !value.trim().is_empty()).unwrap_or("东方财富个股新闻"),
                "source_tier": SOURCE_TIER_NEWS,
                "published_at": published_at,
                "published_at_epoch_ms": published_epoch,
                "url": url,
                "stock_codes": [code.clone()],
                "industries": stock.get("industry").and_then(Value::as_str).map(|value| vec![value.to_string()]).unwrap_or_default(),
                "relation_types": relation_map.get(&code).cloned().unwrap_or_default(),
                "sentiment": infer_sentiment(&format!("{} {}", title, row.get("content").and_then(Value::as_str).unwrap_or(""))),
                "fetched_at_epoch_ms": epoch_millis()
            })));
        }
    }
    Ok(items)
}

async fn fetch_sina_stock_news(
    client: &reqwest::Client,
    stocks: &[Value],
    relations: &[Value],
    days: i64,
) -> Result<Vec<Value>, String> {
    let mut items = Vec::new();
    let cutoff = cutoff_epoch_millis(days);
    let relation_map = relation_map(relations);
    let max_stocks = env_usize("GP_NEWS_SINA_MAX_STOCKS", 8, 0, 50);
    let max_items = env_usize("GP_NEWS_SINA_MAX_ITEMS", 24, 0, 100);
    for stock in stocks.iter().take(max_stocks) {
        let Some(code) = stock
            .get("code")
            .and_then(Value::as_str)
            .and_then(normalize_stock_code)
        else {
            continue;
        };
        let Some(symbol) = sina_symbol(&code) else {
            continue;
        };
        let url = format!(
            "https://vip.stock.finance.sina.com.cn/corp/go.php/vCB_AllNewsStock/symbol/{symbol}.phtml"
        );
        let html = http_get_text(client, &url).await?;
        items.extend(parse_sina_stock_news(
            &html,
            stock,
            &relation_map,
            cutoff,
            max_items,
        ));
    }
    Ok(items)
}

async fn fetch_ths_stock_news(
    client: &reqwest::Client,
    stocks: &[Value],
    relations: &[Value],
    days: i64,
) -> Result<Vec<Value>, String> {
    let mut items = Vec::new();
    let cutoff = cutoff_epoch_millis(days);
    let relation_map = relation_map(relations);
    let max_stocks = env_usize("GP_NEWS_THS_MAX_STOCKS", 8, 0, 50);
    let max_items = env_usize("GP_NEWS_THS_MAX_ITEMS", 40, 0, 160);
    for stock in stocks.iter().take(max_stocks) {
        let Some(code) = stock
            .get("code")
            .and_then(Value::as_str)
            .and_then(normalize_stock_code)
        else {
            continue;
        };
        let Some(digits) = code_digits(&code) else {
            continue;
        };
        let url = format!("https://basic.10jqka.com.cn/{digits}/news.html");
        let html = http_get_text(client, &url).await?;
        items.extend(parse_ths_linkage_news(
            &html,
            stock,
            &relation_map,
            cutoff,
            max_items,
        ));
    }
    Ok(items)
}

fn parse_sina_stock_news(
    html: &str,
    stock: &Value,
    relation_map: &HashMap<String, Vec<String>>,
    cutoff: u128,
    max_items: usize,
) -> Vec<Value> {
    let Some(code) = stock
        .get("code")
        .and_then(Value::as_str)
        .and_then(normalize_stock_code)
    else {
        return Vec::new();
    };
    let industry = stock_industries(stock);
    let Some(start) = html
        .find("class=\"datelist\"")
        .or_else(|| html.find("class='datelist'"))
    else {
        return Vec::new();
    };
    let list_start = html[..start].rfind('<').unwrap_or(start);
    let list_end = html[list_start..]
        .find("</ul>")
        .map(|offset| list_start + offset)
        .unwrap_or(html.len());
    let list = &html[list_start..list_end];
    let mut items = Vec::new();
    for chunk in list.split("<br") {
        if items.len() >= max_items {
            break;
        }
        let Some(published_at) = extract_sina_news_time(chunk) else {
            continue;
        };
        let Some(published_epoch) = parse_time_epoch_millis(&published_at) else {
            continue;
        };
        if published_epoch < cutoff {
            continue;
        }
        let Some(url) = extract_first_attr_value(chunk, "href") else {
            continue;
        };
        let title = extract_link_text(chunk);
        if title.is_empty() {
            continue;
        }
        items.push(news_item(json!({
            "title": title,
            "summary": title.clone(),
            "source": SOURCE_SINA_FINANCE,
            "source_tier": SOURCE_TIER_NEWS,
            "published_at": published_at,
            "published_at_epoch_ms": published_epoch,
            "url": url,
            "stock_codes": [code.clone()],
            "industries": industry.clone(),
            "relation_types": relation_map.get(&code).cloned().unwrap_or_default(),
            "sentiment": infer_sentiment(&title),
            "fetched_at_epoch_ms": epoch_millis()
        })));
    }
    items
}

fn parse_ths_linkage_news(
    html: &str,
    stock: &Value,
    relation_map: &HashMap<String, Vec<String>>,
    cutoff: u128,
    max_items: usize,
) -> Vec<Value> {
    let Some(code) = stock
        .get("code")
        .and_then(Value::as_str)
        .and_then(normalize_stock_code)
    else {
        return Vec::new();
    };
    let Some(digits) = code_digits(&code) else {
        return Vec::new();
    };
    let Some(raw) = extract_hidden_element_text(html, "linkagedata") else {
        return Vec::new();
    };
    let Ok(rows) = serde_json::from_str::<Vec<Value>>(&raw) else {
        return Vec::new();
    };
    let industry = stock_industries(stock);
    let mut items = Vec::new();
    for row in rows.into_iter().take(max_items) {
        let title = clean_html(row.get("title").and_then(Value::as_str).unwrap_or(""));
        if title.is_empty() {
            continue;
        }
        let stocks_field = row.get("stocks").and_then(Value::as_str).unwrap_or("");
        if !stocks_field.trim().is_empty() && !stocks_field.contains(&digits) {
            continue;
        }
        let published_epoch = row
            .get("ctime")
            .and_then(Value::as_u64)
            .map(|seconds| u128::from(seconds) * 1_000)
            .unwrap_or_else(epoch_millis);
        if published_epoch < cutoff {
            continue;
        }
        let published_at = row
            .get("ctime")
            .and_then(Value::as_u64)
            .map(epoch_seconds_to_news_time)
            .unwrap_or_else(current_iso_like);
        let source = row
            .get("source")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(SOURCE_THS_F10);
        let url = row
            .get("curl")
            .or_else(|| row.get("url"))
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| format!("https://basic.10jqka.com.cn/{digits}/news.html"));
        let author = row
            .get("author")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("");
        let summary = if author.is_empty() {
            title.clone()
        } else {
            format!("{title} - {author}")
        };
        items.push(news_item(json!({
            "title": title,
            "summary": summary,
            "source": source,
            "source_tier": SOURCE_TIER_NEWS,
            "published_at": published_at,
            "published_at_epoch_ms": published_epoch,
            "url": url,
            "stock_codes": [code.clone()],
            "industries": industry.clone(),
            "relation_types": relation_map.get(&code).cloned().unwrap_or_default(),
            "sentiment": infer_sentiment(&summary),
            "fetched_at_epoch_ms": epoch_millis()
        })));
    }
    items
}

fn parse_guba_article_list(
    html: &str,
    stock: &Value,
    relation_map: &HashMap<String, Vec<String>>,
    cutoff: u128,
    max_posts: usize,
) -> Vec<Value> {
    let Some(raw) = extract_embedded_object(html, "article_list") else {
        return Vec::new();
    };
    let Ok(data) = serde_json::from_str::<Value>(&raw) else {
        return Vec::new();
    };
    let rows = data
        .get("re")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let fallback_code = stock
        .get("code")
        .and_then(Value::as_str)
        .and_then(normalize_stock_code);
    let industry = stock
        .get("industry")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let mut items = Vec::new();
    for row in rows.into_iter().take(max_posts) {
        let title = clean_html(row.get("post_title").and_then(Value::as_str).unwrap_or(""));
        if title.is_empty() {
            continue;
        }
        let published_at = parse_news_time(
            row.get("post_publish_time")
                .or_else(|| row.get("post_display_time"))
                .and_then(Value::as_str)
                .unwrap_or(""),
        );
        let published_epoch = parse_time_epoch_millis(&published_at).unwrap_or_else(epoch_millis);
        if published_epoch < cutoff {
            continue;
        }
        let Some(code) = row
            .get("stockbar_code")
            .and_then(Value::as_str)
            .and_then(normalize_stock_code)
            .or_else(|| fallback_code.clone())
        else {
            continue;
        };
        let post_id = row
            .get("post_id")
            .and_then(Value::as_i64)
            .map(|value| value.to_string())
            .or_else(|| {
                row.get("post_id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .unwrap_or_default();
        let url = if post_id.trim().is_empty() {
            format!(
                "https://guba.eastmoney.com/list,{}.html",
                code_digits(&code).unwrap_or_default()
            )
        } else {
            format!(
                "https://guba.eastmoney.com/news,{},{post_id}.html",
                code_digits(&code).unwrap_or_default()
            )
        };
        items.push(news_item(json!({
            "title": title,
            "summary": guba_list_summary(&row, &title),
            "source": "东方财富股吧",
            "source_tier": SOURCE_TIER_COMMUNITY,
            "published_at": published_at,
            "published_at_epoch_ms": published_epoch,
            "url": url,
            "stock_codes": [code.clone()],
            "industries": if industry.is_empty() { Vec::<String>::new() } else { vec![industry.clone()] },
            "relation_types": relation_map.get(&code).cloned().unwrap_or_default(),
            "sentiment": infer_sentiment(&title),
            "fetched_at_epoch_ms": epoch_millis()
        })));
    }
    items
}

fn parse_guba_post_article(
    html: &str,
    url: &str,
    stocks: &[Value],
    relation_map: &HashMap<String, Vec<String>>,
    cutoff: u128,
) -> Option<Value> {
    let stock_by_code = stock_map(&json!({"stocks": stocks}));
    let data = extract_embedded_object(html, "post_article")
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .unwrap_or_else(|| json!({}));
    let code = data
        .get("post_guba")
        .and_then(|value| value.get("stockbar_code"))
        .and_then(Value::as_str)
        .and_then(normalize_stock_code)
        .or_else(|| code_from_guba_url(url).and_then(|value| normalize_stock_code(&value)))?;
    let title_fallback = first_class_text(html, "newstitle");
    let title = clean_html(
        data.get("post_title")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(&title_fallback),
    );
    if title.is_empty() {
        return None;
    }
    let summary = clean_html(
        data.get("post_abstract")
            .or_else(|| data.get("post_content"))
            .and_then(Value::as_str)
            .unwrap_or(&title),
    );
    let time_fallback = first_class_text(html, "time");
    let published_at = parse_news_time(
        data.get("post_publish_time")
            .or_else(|| data.get("post_display_time"))
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(&time_fallback),
    );
    let published_epoch = parse_time_epoch_millis(&published_at).unwrap_or_else(epoch_millis);
    if published_epoch < cutoff {
        return None;
    }
    let industry = stock_by_code
        .get(&code)
        .and_then(|stock| stock.get("industry"))
        .and_then(Value::as_str)
        .map(|value| vec![value.to_string()])
        .unwrap_or_default();
    Some(news_item(json!({
        "title": title,
        "summary": if summary.is_empty() { title.clone() } else { summary.clone() },
        "source": "东方财富股吧",
        "source_tier": SOURCE_TIER_COMMUNITY,
        "published_at": published_at,
        "published_at_epoch_ms": published_epoch,
        "url": url,
        "stock_codes": [code.clone()],
        "industries": industry,
        "relation_types": relation_map.get(&code).cloned().unwrap_or_default(),
        "sentiment": infer_sentiment(&format!("{} {}", title, summary)),
        "fetched_at_epoch_ms": epoch_millis()
    })))
}

fn news_item(mut item: Value) -> Value {
    let id = item
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| news_key(&item));
    if let Some(object) = item.as_object_mut() {
        object.insert("id".to_string(), json!(id));
    }
    item
}

fn read_news_cache_items(path: &Path) -> Vec<Value> {
    if !path.exists() {
        return Vec::new();
    }
    read_json_file(path)
        .ok()
        .and_then(|value| value.get("items").and_then(Value::as_array).cloned())
        .unwrap_or_default()
}

fn write_news_cache_items(path: &Path, items: &[Value]) -> Result<(), String> {
    let root = path
        .parent()
        .ok_or_else(|| "news cache path has no parent".to_string())?;
    fs::create_dir_all(root).map_err(|error| format!("创建消息缓存目录失败：{error}"))?;
    let payload =
        json!({"schema_version": 1, "updated_at_epoch_ms": epoch_millis(), "items": items});
    let bytes =
        serde_json::to_vec(&payload).map_err(|error| format!("序列化消息缓存失败：{error}"))?;
    let tmp_path = root.join(format!("{NEWS_CACHE_FILE}.tmp-{}", epoch_millis()));
    fs::write(&tmp_path, &bytes).map_err(|error| format!("写入消息缓存临时文件失败：{error}"))?;
    if path.exists() {
        fs::remove_file(path).map_err(|error| format!("替换旧消息缓存失败：{error}"))?;
    }
    fs::rename(&tmp_path, path).map_err(|error| format!("提交消息缓存失败：{error}"))
}

fn news_cache_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let mut root = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("获取应用数据目录失败：{error}"))?;
    root.push("news");
    root.push(NEWS_CACHE_FILE);
    Ok(root)
}

fn merge_news_items(mut cached: Vec<Value>, fetched: Vec<Value>) -> Vec<Value> {
    cached.extend(fetched);
    let mut seen = HashSet::new();
    let mut unique = Vec::new();
    for item in cached {
        let key = item
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| news_key(&item));
        if seen.insert(key) {
            unique.push(news_item(item));
        }
    }
    unique.sort_by(|left, right| item_epoch(right).cmp(&item_epoch(left)));
    unique.truncate(NEWS_MAX_CACHE_ITEMS);
    unique
}

fn query_evidence(items: &[Value], codes: &[String], days: i64, limit: usize) -> Vec<Value> {
    let code_set = codes.iter().cloned().collect::<HashSet<_>>();
    let cutoff = cutoff_epoch_millis(days);
    let mut evidence = items
        .iter()
        .filter(|item| item_epoch(item) >= cutoff)
        .filter(|item| {
            item.get("stock_codes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .any(|code| {
                    normalize_stock_code(code)
                        .map(|normalized| code_set.contains(&normalized))
                        .unwrap_or(false)
                })
        })
        .map(news_evidence)
        .collect::<Vec<_>>();
    evidence.sort_by(|left, right| item_epoch(right).cmp(&item_epoch(left)));
    dedupe_news_items(evidence)
        .into_iter()
        .take(limit)
        .collect()
}

fn news_evidence(item: &Value) -> Value {
    json!({
        "title": item.get("title").and_then(Value::as_str).unwrap_or(""),
        "summary": item.get("summary").and_then(Value::as_str),
        "source": item.get("source").and_then(Value::as_str).unwrap_or("未知来源"),
        "source_tier": item.get("source_tier").and_then(Value::as_str).unwrap_or(SOURCE_TIER_NEWS),
        "published_at": item.get("published_at").and_then(Value::as_str),
        "url": item.get("url").and_then(Value::as_str),
        "stock_codes": string_array(item.get("stock_codes")),
        "relation_types": string_array(item.get("relation_types")),
        "sentiment": normalize_sentiment(item.get("sentiment").and_then(Value::as_str).unwrap_or("uncertain")),
    })
}

fn build_findings(
    scope_codes: &[String],
    relations: &[Value],
    evidence: &[Value],
    stock_by_code: &HashMap<String, Value>,
) -> Vec<Value> {
    let mut findings = Vec::new();
    for code in scope_codes.iter().take(8) {
        let stock_name = stock_by_code
            .get(code)
            .and_then(|stock| stock.get("name"))
            .and_then(Value::as_str)
            .unwrap_or(code);
        let related = relations
            .iter()
            .filter(|relation| {
                relation_code(relation, "source_code") == Some(code.as_str())
                    || relation_code(relation, "target_code") == Some(code.as_str())
            })
            .cloned()
            .collect::<Vec<_>>();
        let direct = evidence
            .iter()
            .filter(|item| evidence_has_code(item, code))
            .cloned()
            .collect::<Vec<_>>();
        let neighbor_codes = related
            .iter()
            .filter_map(|relation| {
                let source = relation_code(relation, "source_code")?;
                let target = relation_code(relation, "target_code")?;
                Some(if source == code {
                    target.to_string()
                } else {
                    source.to_string()
                })
            })
            .collect::<HashSet<_>>();
        let neighbor = evidence
            .iter()
            .filter(|item| {
                neighbor_codes
                    .iter()
                    .any(|neighbor| evidence_has_code(item, neighbor))
            })
            .cloned()
            .collect::<Vec<_>>();
        let mut selected = direct;
        selected.extend(neighbor);
        selected = dedupe_news_items(selected).into_iter().take(5).collect();
        let direction = direction(&selected);
        let has_community = evidence_has_community(&selected);
        findings.push(json!({"target": format!("{stock_name}（{code}）"), "direction": direction, "confidence": confidence(&selected, &related), "impact_chain": impact_chain(code, stock_name, &related, stock_by_code, &selected), "evidence": selected, "pending_checks": pending_checks(&direction, has_community)}));
    }
    findings
}

fn build_stock_news_findings(
    scope_codes: &[String],
    evidence: &[Value],
    stock_by_code: &HashMap<String, Value>,
) -> Vec<Value> {
    let mut findings = Vec::new();
    for code in scope_codes.iter().take(8) {
        let stock_name = stock_by_code
            .get(code)
            .and_then(|stock| stock.get("name"))
            .and_then(Value::as_str)
            .unwrap_or(code);
        let selected = dedupe_news_items(
            evidence
                .iter()
                .filter(|item| evidence_has_code(item, code))
                .cloned()
                .collect(),
        )
        .into_iter()
        .take(8)
        .collect::<Vec<_>>();
        let direction = direction(&selected);
        let has_community = evidence_has_community(&selected);
        findings.push(json!({
            "target": format!("{stock_name}\u{ff08}{code}\u{ff09}"),
            "direction": direction,
            "confidence": confidence(&selected, &[]),
            "impact_chain": format!("{}\u{4e2a}\u{80a1}\u{6d88}\u{606f}\u{672c}\u{5730}\u{5f52}\u{7c7b}\u{4e3a}{}\u{ff1b}\u{672c}\u{6b21}\u{4e0d}\u{8ba1}\u{7b97}\u{4e0a}\u{4e0b}\u{6e38}\u{4f20}\u{5bfc}\u{5f71}\u{54cd}\u{3002}", stock_name, direction),
            "evidence": selected,
            "pending_checks": pending_checks(&direction, has_community),
        }));
    }
    findings
}

async fn apply_llm_analysis(
    llm_value: Option<&Value>,
    scope_codes: &[String],
    relations: &[Value],
    evidence: &[Value],
    stock_by_code: &HashMap<String, Value>,
    base_findings: &[Value],
) -> (Vec<Value>, Vec<String>, String) {
    if base_findings.is_empty() {
        return (
            base_findings.to_vec(),
            vec!["没有可分析目标，未调用模型。".to_string()],
            "plain_news".to_string(),
        );
    }
    let Some(config) = resolve_llm_config(llm_value) else {
        return (
            base_findings.to_vec(),
            vec!["未接入模型，已按本地规则展示个股及上下游利好/利空消息。".to_string()],
            "plain_news".to_string(),
        );
    };
    match call_news_llm(
        &config,
        scope_codes,
        relations,
        evidence,
        stock_by_code,
        base_findings,
    )
    .await
    {
        Ok(llm_result) => {
            let mut notes = vec![format!(
                "已调用模型 {} 基于检索证据参与上下游影响判断。",
                config.model
            )];
            notes.extend(string_array(llm_result.get("notes")).into_iter().take(4));
            (
                merge_llm_findings(base_findings, &llm_result),
                notes,
                "llm_analysis".to_string(),
            )
        }
        Err(error) => (
            base_findings.to_vec(),
            vec![format!(
                "模型分析失败，已回退为个股及上下游利好/利空消息：{}",
                redact_secret(&error)
            )],
            "plain_news".to_string(),
        ),
    }
}

struct LlmConfig {
    api_key: String,
    base_url: String,
    model: String,
    temperature: f64,
    timeout_seconds: u64,
    json_mode: bool,
    organization: Option<String>,
    project: Option<String>,
}

fn resolve_llm_config(value: Option<&Value>) -> Option<LlmConfig> {
    let api_key = value
        .and_then(|item| item.get("api_key"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .or_else(|| {
            env::var("OPENAI_API_KEY")
                .ok()
                .filter(|item| !item.trim().is_empty())
        })?;
    let base_url = value
        .and_then(|item| item.get("base_url"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(|item| item.trim_end_matches('/').to_string())
        .or_else(|| {
            env::var("OPENAI_BASE_URL")
                .ok()
                .map(|item| item.trim_end_matches('/').to_string())
                .filter(|item| !item.is_empty())
        })
        .unwrap_or_else(|| "https://api.openai.com/v1".to_string());
    let model = value
        .and_then(|item| item.get("model"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .or_else(|| {
            env::var("OPENAI_MODEL")
                .ok()
                .filter(|item| !item.trim().is_empty())
        })
        .unwrap_or_else(|| "gpt-4o-mini".to_string());
    let temperature = value
        .and_then(|item| item.get("temperature"))
        .and_then(Value::as_f64)
        .or_else(|| {
            env::var("OPENAI_TEMPERATURE")
                .ok()
                .and_then(|item| item.parse::<f64>().ok())
        })
        .unwrap_or(0.2)
        .clamp(0.0, 2.0);
    let timeout_seconds = value
        .and_then(|item| item.get("timeout_seconds"))
        .and_then(Value::as_u64)
        .or_else(|| {
            env::var("OPENAI_TIMEOUT_SECONDS")
                .ok()
                .and_then(|item| item.parse::<u64>().ok())
        })
        .unwrap_or(30)
        .clamp(1, 180);
    let json_mode = value
        .and_then(|item| item.get("json_mode"))
        .and_then(Value::as_bool)
        .or_else(|| {
            env::var("OPENAI_JSON_MODE").ok().map(|item| {
                !matches!(
                    item.to_ascii_lowercase().as_str(),
                    "0" | "false" | "no" | "off"
                )
            })
        })
        .unwrap_or(true);
    Some(LlmConfig {
        api_key,
        base_url,
        model,
        temperature,
        timeout_seconds,
        json_mode,
        organization: value
            .and_then(|item| item.get("organization"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(str::to_string)
            .or_else(|| env::var("OPENAI_ORG_ID").ok()),
        project: value
            .and_then(|item| item.get("project"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(str::to_string)
            .or_else(|| env::var("OPENAI_PROJECT_ID").ok()),
    })
}

async fn call_news_llm(
    config: &LlmConfig,
    scope_codes: &[String],
    relations: &[Value],
    evidence: &[Value],
    stock_by_code: &HashMap<String, Value>,
    findings: &[Value],
) -> Result<Value, String> {
    let client = build_http_client_with_proxy(
        "Mozilla/5.0 GuXuanYou/0.3 news-llm",
        Duration::from_secs(config.timeout_seconds),
        None,
    )
    .map_err(|error| format!("create LLM HTTP client failed: {error}"))?;
    let payload = news_llm_payload(scope_codes, relations, evidence, stock_by_code, findings);
    let mut request = json!({"model": config.model, "messages": [{"role": "system", "content": news_llm_system_prompt()}, {"role": "user", "content": serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string())}], "temperature": config.temperature});
    if config.json_mode {
        request["response_format"] = json!({"type": "json_object"});
    }
    match post_llm_request(&client, config, &request).await {
        Ok(value) => Ok(value),
        Err(first_error) if config.json_mode => {
            let mut fallback = request;
            if let Some(object) = fallback.as_object_mut() {
                object.remove("response_format");
            }
            post_llm_request(&client, config, &fallback)
                .await
                .map_err(|second_error| {
                    format!("{first_error}; retry without JSON mode failed: {second_error}")
                })
        }
        Err(error) => Err(error),
    }
}

async fn post_llm_request(
    client: &reqwest::Client,
    config: &LlmConfig,
    request: &Value,
) -> Result<Value, String> {
    let url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));
    let body = serde_json::to_vec(request)
        .map_err(|error| format!("serialize LLM request failed: {error}"))?;
    let mut builder = client
        .post(url)
        .header("Authorization", format!("Bearer {}", config.api_key))
        .header("Content-Type", "application/json")
        .body(body);
    if let Some(org) = &config.organization {
        builder = builder.header("OpenAI-Organization", org);
    }
    if let Some(project) = &config.project {
        builder = builder.header("OpenAI-Project", project);
    }
    let response = builder
        .send()
        .await
        .map_err(|error| format!("LLM request failed: {error}"))?;
    let status = response.status();
    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("read LLM response failed: {error}"))?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    if !status.is_success() {
        return Err(format!("LLM HTTP {status}: {}", limit_chars(&text, 300)));
    }
    let response_json: Value = serde_json::from_str(&text).map_err(|error| {
        format!(
            "parse LLM response failed: {error}: {}",
            limit_chars(&text, 160)
        )
    })?;
    let content = response_json
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .unwrap_or("{}");
    parse_json_response(content)
}

fn news_llm_payload(
    scope_codes: &[String],
    relations: &[Value],
    evidence: &[Value],
    stock_by_code: &HashMap<String, Value>,
    findings: &[Value],
) -> Value {
    json!({"scope_codes": scope_codes, "stocks": scope_codes.iter().filter_map(|code| { let stock = stock_by_code.get(code)?; Some(json!({"code": code, "name": stock.get("name").and_then(Value::as_str).unwrap_or(code), "industry": stock.get("industry").and_then(Value::as_str).unwrap_or("")})) }).collect::<Vec<_>>(), "relations": relations.iter().take(30).map(|relation| json!({"source_code": relation.get("source_code").and_then(Value::as_str).unwrap_or(""), "target_code": relation.get("target_code").and_then(Value::as_str).unwrap_or(""), "relation_type": relation.get("relation_type").and_then(Value::as_str).unwrap_or(""), "weight": relation_weight(relation), "description": relation.get("description").and_then(Value::as_str)})).collect::<Vec<_>>(), "evidence": evidence.iter().take(30).enumerate().map(|(index, item)| json!({"id": format!("E{}", index + 1), "title": item.get("title").and_then(Value::as_str).unwrap_or(""), "summary": item.get("summary").and_then(Value::as_str).unwrap_or(""), "source": item.get("source").and_then(Value::as_str).unwrap_or(""), "source_tier": item.get("source_tier").and_then(Value::as_str).unwrap_or(SOURCE_TIER_NEWS), "published_at": item.get("published_at").and_then(Value::as_str), "stock_codes": string_array(item.get("stock_codes")), "relation_types": string_array(item.get("relation_types")), "sentiment": item.get("sentiment").and_then(Value::as_str).unwrap_or("uncertain")})).collect::<Vec<_>>(), "rule_findings": findings.iter().map(|finding| json!({"target": finding.get("target").and_then(Value::as_str).unwrap_or(""), "direction": finding.get("direction").and_then(Value::as_str).unwrap_or(""), "confidence": finding.get("confidence").and_then(Value::as_str).unwrap_or(""), "impact_chain": finding.get("impact_chain").and_then(Value::as_str).unwrap_or(""), "evidence_titles": finding.get("evidence").and_then(Value::as_array).into_iter().flatten().filter_map(|item| item.get("title").and_then(Value::as_str)).collect::<Vec<_>>(), "pending_checks": string_array(finding.get("pending_checks"))})).collect::<Vec<_>>()})
}

fn news_llm_system_prompt() -> &'static str {
    "你是A股上下游消息RAG分析器，只能基于用户提供的关系边和证据判断。不要编造公告、客户、供应商、订单或新闻；不要给交易建议。community证据只能作为情绪、传闻或待核查线索，不能单独支撑确定性利好/利空。输出严格JSON：{\"findings\":[{\"target\":\"与输入target一致\",\"direction\":\"利好|利空|中性|不确定\",\"confidence\":\"低|中|高\",\"impact_chain\":\"一句中文解释\",\"pending_checks\":[\"待核查项\"]}],\"notes\":[\"简短说明\"]}。"
}

fn merge_llm_findings(base_findings: &[Value], llm_result: &Value) -> Vec<Value> {
    let rows = llm_result
        .get("findings")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    base_findings
        .iter()
        .map(|finding| {
            let Some(row) = matching_llm_finding(finding, &rows) else {
                return finding.clone();
            };
            let mut merged = finding.as_object().cloned().unwrap_or_default();
            if let Some(direction) =
                normalize_direction(row.get("direction").and_then(Value::as_str).unwrap_or(""))
            {
                merged.insert("direction".to_string(), json!(direction));
            }
            if let Some(confidence) =
                normalize_confidence(row.get("confidence").and_then(Value::as_str).unwrap_or(""))
            {
                merged.insert("confidence".to_string(), json!(confidence));
            }
            if let Some(impact_chain) = row
                .get("impact_chain")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                merged.insert(
                    "impact_chain".to_string(),
                    json!(limit_chars(impact_chain, 420)),
                );
            }
            let pending = string_array(row.get("pending_checks"));
            if !pending.is_empty() {
                merged.insert(
                    "pending_checks".to_string(),
                    json!(pending.into_iter().take(5).collect::<Vec<_>>()),
                );
            }
            Value::Object(merged)
        })
        .collect()
}

fn matching_llm_finding<'a>(finding: &Value, rows: &'a [Value]) -> Option<&'a Value> {
    let target = finding.get("target").and_then(Value::as_str).unwrap_or("");
    let code = extract_stock_code(target).unwrap_or_default();
    rows.iter().find(|row| {
        let row_target = row.get("target").and_then(Value::as_str).unwrap_or("");
        (!row_target.is_empty()
            && (row_target == target || row_target.contains(target) || target.contains(row_target)))
            || (!code.is_empty() && row_target.contains(&code))
    })
}

fn sentiment_groups(evidence: &[Value], mode: &str) -> Value {
    let mut positive = Vec::new();
    let mut negative = Vec::new();
    let mut mixed = Vec::new();
    let mut uncertain = Vec::new();
    for item in dedupe_news_items(evidence.to_vec()) {
        match item
            .get("sentiment")
            .and_then(Value::as_str)
            .unwrap_or("uncertain")
        {
            "positive" => positive.push(item),
            "negative" => negative.push(item),
            "mixed" => mixed.push(item),
            _ => uncertain.push(item),
        }
    }
    json!({"mode": if mode == "llm_analysis" { "llm_analysis" } else { "plain_news" }, "positive": positive, "negative": negative, "mixed": mixed, "uncertain": uncertain})
}
fn relation_map(relations: &[Value]) -> HashMap<String, Vec<String>> {
    let mut result: HashMap<String, HashSet<String>> = HashMap::new();
    for relation in relations {
        let relation_type = relation
            .get("relation_type")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if relation_type.is_empty() {
            continue;
        }
        for key in ["source_code", "target_code"] {
            if let Some(code) = relation.get(key).and_then(Value::as_str) {
                result
                    .entry(code.to_string())
                    .or_default()
                    .insert(relation_type.clone());
            }
        }
    }
    result
        .into_iter()
        .map(|(code, values)| {
            let mut values = values.into_iter().collect::<Vec<_>>();
            values.sort();
            (code, values)
        })
        .collect()
}
fn dedupe_news_items(items: Vec<Value>) -> Vec<Value> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for item in items {
        let key = item
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| news_key(&item));
        if seen.insert(key) {
            result.push(news_item(item));
        }
    }
    result
}
fn news_key(item: &Value) -> String {
    let raw = format!(
        "{}|{}|{}|{}",
        item.get("source").and_then(Value::as_str).unwrap_or(""),
        item.get("title").and_then(Value::as_str).unwrap_or(""),
        item.get("url").and_then(Value::as_str).unwrap_or(""),
        item.get("published_at")
            .and_then(Value::as_str)
            .unwrap_or("")
    );
    let digest = Sha256::digest(raw.as_bytes());
    digest
        .iter()
        .take(12)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn direction(evidence: &[Value]) -> String {
    let positives = evidence
        .iter()
        .filter(|item| item.get("sentiment").and_then(Value::as_str) == Some("positive"))
        .count();
    let negatives = evidence
        .iter()
        .filter(|item| item.get("sentiment").and_then(Value::as_str) == Some("negative"))
        .count();
    let mixed = evidence
        .iter()
        .filter(|item| item.get("sentiment").and_then(Value::as_str) == Some("mixed"))
        .count();
    if positives > negatives && positives >= mixed {
        "利好".to_string()
    } else if negatives > positives {
        "利空".to_string()
    } else if mixed > 0 {
        "中性".to_string()
    } else {
        "不确定".to_string()
    }
}
fn confidence(evidence: &[Value], relations: &[Value]) -> &'static str {
    if evidence.is_empty() || relations.is_empty() {
        return "低";
    }
    let verified = evidence
        .iter()
        .filter(|item| {
            item.get("source_tier").and_then(Value::as_str) != Some(SOURCE_TIER_COMMUNITY)
        })
        .count();
    let max_weight = relations.iter().map(relation_weight).fold(0.0, f64::max);
    if verified >= 3 && max_weight >= 0.6 {
        "中"
    } else {
        "低"
    }
}
fn impact_chain(
    code: &str,
    stock_name: &str,
    relations: &[Value],
    stock_by_code: &HashMap<String, Value>,
    evidence: &[Value],
) -> String {
    if relations.is_empty() {
        return format!("{stock_name} 暂无已建模上下游关系，不能从消息推出产业链影响。");
    }
    let strongest = relations
        .iter()
        .max_by(|left, right| relation_weight(left).total_cmp(&relation_weight(right)))
        .unwrap();
    let source = strongest
        .get("source_code")
        .and_then(Value::as_str)
        .unwrap_or("");
    let target = strongest
        .get("target_code")
        .and_then(Value::as_str)
        .unwrap_or("");
    let other_code = if source == code { target } else { source };
    let other_name = stock_by_code
        .get(other_code)
        .and_then(|stock| stock.get("name"))
        .and_then(Value::as_str)
        .unwrap_or(other_code);
    format!(
        "{other_name} -> {} -> {stock_name}；当前消息影响判断为{}。",
        relation_type_label(
            strongest
                .get("relation_type")
                .and_then(Value::as_str)
                .unwrap_or("")
        ),
        direction(evidence)
    )
}
fn pending_checks(direction: &str, has_community: bool) -> Vec<String> {
    let mut checks = vec![
        "核验公告和财报是否支持该消息影响。".to_string(),
        "结合日线趋势、成交量和估值变化复查。".to_string(),
    ];
    if has_community {
        checks.push(
            "社区讨论仅作市场情绪、风险传闻或待核查线索，需用官方披露或交易数据二次验证。"
                .to_string(),
        );
    }
    if matches!(direction, "利好" | "中性") {
        checks.push("关注订单兑现、价格传导和毛利率变化。".to_string());
    }
    if matches!(direction, "利空" | "中性") {
        checks.push("关注成本压力、库存和需求下滑风险。".to_string());
    }
    checks
}
fn infer_sentiment(text: &str) -> &'static str {
    let lowered = text.to_lowercase();
    if contains_any(
        &lowered,
        &[
            "下滑",
            "亏损",
            "风险",
            "承压",
            "下降",
            "减产",
            "调查",
            "破发",
            "放弃申购",
            "看空",
            "下跌",
            "违约",
            "处罚",
        ],
    ) {
        "negative"
    } else if contains_any(
        &lowered,
        &[
            "增长", "改善", "突破", "订单", "中标", "扩产", "景气", "利好", "看多", "上涨", "企稳",
            "增持", "合作",
        ],
    ) {
        "positive"
    } else if contains_any(&lowered, &["成本", "涨价", "波动", "分歧"]) {
        "mixed"
    } else {
        "uncertain"
    }
}
fn normalize_sentiment(value: &str) -> &'static str {
    match value {
        "positive" => "positive",
        "negative" => "negative",
        "mixed" => "mixed",
        _ => "uncertain",
    }
}
fn normalize_direction(value: &str) -> Option<&'static str> {
    match value.trim().to_lowercase().as_str() {
        "positive" | "bullish" | "利好" => Some("利好"),
        "negative" | "bearish" | "利空" => Some("利空"),
        "neutral" | "中性" => Some("中性"),
        "uncertain" | "unknown" | "不确定" => Some("不确定"),
        _ => None,
    }
}
fn normalize_confidence(value: &str) -> Option<&'static str> {
    match value.trim().to_lowercase().as_str() {
        "high" | "高" => Some("高"),
        "medium" | "mid" | "中" => Some("中"),
        "low" | "低" => Some("低"),
        _ => None,
    }
}
fn relation_type_label(value: &str) -> &str {
    match value {
        "supply_chain" => "供应链",
        "manufacturing_chain" => "制造链",
        "upstream_material" => "上游材料",
        _ => value,
    }
}
fn relation_weight(relation: &Value) -> f64 {
    relation
        .get("weight")
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
        .unwrap_or(0.0)
}
fn relation_code<'a>(relation: &'a Value, key: &str) -> Option<&'a str> {
    relation.get(key).and_then(Value::as_str)
}
fn evidence_has_code(item: &Value, code: &str) -> bool {
    item.get("stock_codes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .any(|item_code| normalize_stock_code(item_code).as_deref() == Some(code))
}
fn evidence_has_community(items: &[Value]) -> bool {
    items
        .iter()
        .any(|item| item.get("source_tier").and_then(Value::as_str) == Some(SOURCE_TIER_COMMUNITY))
}
fn item_epoch(item: &Value) -> u128 {
    item.get("published_at_epoch_ms")
        .and_then(Value::as_u64)
        .map(u128::from)
        .or_else(|| {
            item.get("fetched_at_epoch_ms")
                .and_then(Value::as_u64)
                .map(u128::from)
        })
        .or_else(|| {
            item.get("published_at")
                .and_then(Value::as_str)
                .and_then(parse_time_epoch_millis)
        })
        .unwrap_or_default()
}
fn cutoff_epoch_millis(days: i64) -> u128 {
    epoch_millis().saturating_sub(days.max(1) as u128 * 24 * 60 * 60 * 1_000)
}
fn parse_time_epoch_millis(value: &str) -> Option<u128> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let date_part = trimmed.get(0..10)?;
    let year = date_part.get(0..4)?.parse::<i32>().ok()?;
    let month = date_part.get(5..7)?.parse::<u32>().ok()?;
    let day = date_part.get(8..10)?.parse::<u32>().ok()?;
    let time = trimmed.get(11..19).unwrap_or("00:00:00");
    let hour = time
        .get(0..2)
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(0);
    let minute = time
        .get(3..5)
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(0);
    let second = time
        .get(6..8)
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(0);
    let days = days_from_civil(year, month, day)?;
    let seconds =
        days as i128 * 86_400 + hour as i128 * 3_600 + minute as i128 * 60 + second as i128;
    if seconds < 0 {
        None
    } else {
        Some(seconds as u128 * 1_000)
    }
}
fn days_from_civil(year: i32, month: u32, day: u32) -> Option<i64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let y = year - if month <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = month as i32 + if month > 2 { -3 } else { 9 };
    let doy = (153 * m + 2) / 5 + day as i32 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some((era * 146097 + doe - 719468) as i64)
}
fn parse_news_time(value: &str) -> String {
    let raw = value.trim();
    if raw.is_empty() {
        return current_iso_like();
    }
    if raw.contains('T') {
        return raw.to_string();
    }
    if raw.len() >= 19 && raw.as_bytes().get(4) == Some(&b'-') {
        return raw[..19].to_string();
    }
    if raw.len() >= 10 && raw.as_bytes().get(4) == Some(&b'-') {
        return format!("{} 00:00:00", &raw[..10]);
    }
    if raw.len() >= 10 && raw.as_bytes().get(4) == Some(&b'/') {
        return raw.replace('/', "-");
    }
    raw.to_string()
}
fn current_iso_like() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    format!("{seconds}")
}
async fn http_get_text(client: &reqwest::Client, url: &str) -> Result<String, String> {
    http_get_text_with_headers_timeout(client, url, "", NEWS_TIMEOUT_SECS).await
}
async fn http_get_text_with_headers_timeout(
    client: &reqwest::Client,
    url: &str,
    referer: &str,
    timeout_secs: u64,
) -> Result<String, String> {
    let user_agent = "Mozilla/5.0 GuXuanYou/0.3 news-rag";
    match powershell_http_get_bytes_with_headers(url, timeout_secs, user_agent, referer) {
        Ok(bytes) => return Ok(decode_http_text(&bytes)),
        Err(powershell_error) => {
            let mut request = client.get(url).header("User-Agent", user_agent);
            if !referer.trim().is_empty() {
                request = request.header("Referer", referer);
            }
            let response = request.send().await.map_err(|error| {
                format!(
                    "PowerShell HTTP fallback failed: {}; reqwest GET {url} failed: {error}",
                    limit_chars(&powershell_error, 180)
                )
            })?;
            if !response.status().is_success() {
                return Err(format!(
                    "PowerShell HTTP fallback failed: {}; reqwest GET {url} HTTP {}",
                    limit_chars(&powershell_error, 180),
                    response.status()
                ));
            }
            let bytes = response
                .bytes()
                .await
                .map_err(|error| format!("GET {url} body read failed: {error}"))?;
            Ok(decode_http_text(&bytes))
        }
    }
}
fn decode_http_text(bytes: &[u8]) -> String {
    let (utf8, _, had_errors) = encoding_rs::UTF_8.decode(bytes);
    if had_errors {
        let (gbk, _, _) = encoding_rs::GBK.decode(bytes);
        gbk.into_owned()
    } else {
        utf8.into_owned()
    }
}
fn parse_jsonp(text: &str) -> Option<Value> {
    let start = text.find('(')? + 1;
    let end = text.rfind(')')?;
    serde_json::from_str(text.get(start..end)?.trim()).ok()
}
fn parse_json_response(content: &str) -> Result<Value, String> {
    serde_json::from_str(content).or_else(|_| {
        let start = content
            .find('{')
            .ok_or_else(|| "LLM response does not contain JSON object".to_string())?;
        let end = content
            .rfind('}')
            .ok_or_else(|| "LLM response does not contain complete JSON object".to_string())?;
        serde_json::from_str(&content[start..=end])
            .map_err(|error| format!("parse embedded LLM JSON failed: {error}"))
    })
}
fn extract_embedded_object(html: &str, var_name: &str) -> Option<String> {
    let marker = format!("var {var_name}");
    let marker_pos = html.find(&marker)?;
    let start = html[marker_pos..].find('{')? + marker_pos;
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for (offset, ch) in html[start..].char_indices() {
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == active_quote {
                quote = None;
            }
            continue;
        }
        match ch {
            '\'' | '"' => quote = Some(ch),
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(html[start..start + offset + ch.len_utf8()].to_string());
                }
            }
            _ => {}
        }
    }
    None
}
fn first_class_text(html: &str, class_name: &str) -> String {
    let marker = format!("class=\"{class_name}\"");
    let Some(pos) = html.find(&marker) else {
        return String::new();
    };
    let Some(start) = html[pos..].find('>').map(|offset| pos + offset + 1) else {
        return String::new();
    };
    let end = html[start..]
        .find('<')
        .map(|offset| start + offset)
        .unwrap_or(html.len());
    clean_html(&html[start..end])
}
fn clean_html(value: &str) -> String {
    let mut output = String::new();
    let mut in_tag = false;
    for ch in value.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => output.push(ch),
            _ => {}
        }
    }
    output
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace('\u{3000}', "")
        .replace("\r\n", " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
fn guba_list_summary(row: &Value, title: &str) -> String {
    let mut parts = vec![title.to_string()];
    if let Some(author) = row
        .get("user_nickname")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
    {
        parts.push(format!("作者：{author}"));
    }
    if let Some(comments) = row.get("post_comment_count").and_then(Value::as_i64) {
        parts.push(format!("评论：{comments}"));
    }
    if let Some(clicks) = row.get("post_click_count").and_then(Value::as_i64) {
        parts.push(format!("阅读：{clicks}"));
    }
    parts.join("；")
}

fn stock_industries(stock: &Value) -> Vec<String> {
    stock
        .get("industry")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(|value| vec![value.to_string()])
        .unwrap_or_default()
}
fn sina_symbol(code: &str) -> Option<String> {
    let normalized = normalize_stock_code(code)?;
    let digits = code_digits(&normalized)?;
    let prefix =
        if normalized.ends_with(".SH") || digits.starts_with('6') || digits.starts_with('9') {
            "sh"
        } else {
            "sz"
        };
    Some(format!("{prefix}{digits}"))
}
fn extract_first_attr_value(markup: &str, attr: &str) -> Option<String> {
    for quote in ['\'', '"'] {
        let marker = format!("{attr}={quote}");
        let Some(start) = markup.find(&marker).map(|offset| offset + marker.len()) else {
            continue;
        };
        let end = markup[start..]
            .find(quote)
            .map(|offset| start + offset)
            .unwrap_or(markup.len());
        let value = markup[start..end].trim();
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    None
}
fn extract_link_text(markup: &str) -> String {
    let Some(link_start) = markup.find("<a") else {
        return String::new();
    };
    let Some(text_start) = markup[link_start..]
        .find('>')
        .map(|offset| link_start + offset + 1)
    else {
        return String::new();
    };
    let text_end = markup[text_start..]
        .find("</a>")
        .map(|offset| text_start + offset)
        .unwrap_or(markup.len());
    clean_html(&markup[text_start..text_end])
}
fn extract_sina_news_time(markup: &str) -> Option<String> {
    let bytes = markup.as_bytes();
    for index in 0..bytes.len().saturating_sub(9) {
        if !is_ascii_date_at(bytes, index) {
            continue;
        }
        let date = markup.get(index..index + 10)?;
        let after = markup.get(index + 10..).unwrap_or("");
        if let Some(time_index) = find_ascii_time_index(after) {
            let time = after.get(time_index..time_index + 5)?;
            return Some(format!("{date} {time}:00"));
        }
        return Some(format!("{date} 00:00:00"));
    }
    None
}
fn is_ascii_date_at(bytes: &[u8], index: usize) -> bool {
    index + 10 <= bytes.len()
        && bytes[index..index + 4].iter().all(u8::is_ascii_digit)
        && bytes[index + 4] == b'-'
        && bytes[index + 5..index + 7].iter().all(u8::is_ascii_digit)
        && bytes[index + 7] == b'-'
        && bytes[index + 8..index + 10].iter().all(u8::is_ascii_digit)
}
fn find_ascii_time_index(value: &str) -> Option<usize> {
    let bytes = value.as_bytes();
    for index in 0..bytes.len().saturating_sub(4) {
        if bytes[index].is_ascii_digit()
            && bytes[index + 1].is_ascii_digit()
            && bytes[index + 2] == b':'
            && bytes[index + 3].is_ascii_digit()
            && bytes[index + 4].is_ascii_digit()
        {
            return Some(index);
        }
    }
    None
}
fn extract_hidden_element_text(html: &str, id: &str) -> Option<String> {
    let marker_double = format!("id=\"{id}\"");
    let marker_single = format!("id='{id}'");
    let id_pos = html
        .find(&marker_double)
        .or_else(|| html.find(&marker_single))?;
    let start = html[id_pos..].find('>').map(|offset| id_pos + offset + 1)?;
    let end = html[start..]
        .find("</div>")
        .map(|offset| start + offset)
        .unwrap_or(html.len());
    let value = html[start..end].trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}
fn epoch_seconds_to_news_time(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64;
    let rem = seconds % 86_400;
    let hour = rem / 3_600;
    let minute = (rem % 3_600) / 60;
    let second = rem % 60;
    format!(
        "{} {hour:02}:{minute:02}:{second:02}",
        civil_date_from_days(days)
    )
}
fn civil_date_from_days(days_since_epoch: i64) -> String {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if month <= 2 { 1 } else { 0 };
    format!("{year:04}-{month:02}-{day:02}")
}
fn configured_urls(env_name: &str) -> Vec<String> {
    env::var(env_name)
        .unwrap_or_default()
        .split(|ch: char| ch.is_whitespace() || ch == ';')
        .map(|item| item.trim().trim_matches(','))
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect()
}
fn env_bool(name: &str, default: bool) -> bool {
    env::var(name)
        .ok()
        .map(|value| {
            !matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "0" | "false" | "no" | "off"
            )
        })
        .unwrap_or(default)
}
fn env_usize(name: &str, default: usize, min: usize, max: usize) -> usize {
    env::var(name)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(default)
        .clamp(min, max)
}
fn payload_u64(payload: &Value, key: &str, default: u64, min: u64, max: u64) -> u64 {
    payload
        .get(key)
        .and_then(Value::as_u64)
        .unwrap_or(default)
        .clamp(min, max)
}
fn string_array(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}
fn contains_any(text: &str, words: &[&str]) -> bool {
    words.iter().any(|word| text.contains(word))
}
fn code_digits(code: &str) -> Option<String> {
    let digits = code
        .chars()
        .filter(|ch| ch.is_ascii_digit())
        .take(6)
        .collect::<String>();
    if digits.len() == 6 {
        Some(digits)
    } else {
        None
    }
}
fn code_from_guba_url(url: &str) -> Option<String> {
    for marker in ["news,", "list,"] {
        let Some(pos) = url.find(marker) else {
            continue;
        };
        let digits = url[pos + marker.len()..]
            .chars()
            .take_while(|ch| ch.is_ascii_digit())
            .take(6)
            .collect::<String>();
        if digits.len() == 6 {
            return Some(digits);
        }
    }
    None
}
fn extract_stock_code(text: &str) -> Option<String> {
    for token in text.split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '.')) {
        if let Some(code) = normalize_stock_code(token) {
            return Some(code);
        }
    }
    None
}
fn limit_chars(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}
fn redact_secret(value: &str) -> String {
    let mut output = value.to_string();
    if let Ok(key) = env::var("OPENAI_API_KEY") {
        if key.len() > 8 {
            output = output.replace(&key, "***");
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sina_us_market_brief_parser_extracts_indices() {
        let payload = r#"
            var hq_str_gb_dji="道琼斯,51848.8984,0.35,2026-06-25 04:41:44,182.0600,51660.7500";
            var hq_str_gb_ixic="纳斯达克,25476.6356,-0.43,2026-06-25 09:48:40,-110.4035,25578.6239";
            var hq_str_gb_inx="标普500指数,7358.2202,-0.10,2026-06-25 04:41:44,-7.2400,7370.8799";
            var hq_str_gb_vix="";
        "#;
        let brief = parse_sina_us_market_brief(payload).unwrap();
        assert_eq!(brief["status"], "ok");
        assert_eq!(brief["source"], "新浪财经全球指数");
        assert_eq!(brief["items"].as_array().unwrap().len(), 3);
        assert_eq!(brief["items"][0]["symbol"], "SPX");
        assert_eq!(brief["items"][1]["symbol"], "IXIC");
        assert_eq!(brief["items"][2]["symbol"], "DJI");
        let dji_change = brief["items"][2]["change_percent"].as_f64().unwrap();
        assert!((dji_change - 0.0035).abs() < 0.000001);
        assert!(brief["headline"].as_str().unwrap().contains("美股"));
    }

    #[test]
    fn sina_stock_news_parser_extracts_traditional_media_items() {
        let stock = json!({"code":"000100.SZ", "name":"TCL科技", "industry":"面板"});
        let relations =
            relation_map(&[json!({"source_code":"000100.SZ", "relation_type":"supply_chain"})]);
        let html = r#"
            <div class="datelist"><ul>
            &nbsp;&nbsp;2026-06-25&nbsp;17:37&nbsp;&nbsp;<a target='_blank' href='https://cj.sina.cn/articles/view/1'>TCL科技获机构调研，面板需求改善</a><br>
            &nbsp;&nbsp;2026-06-24&nbsp;09:10&nbsp;&nbsp;<a target="_blank" href="https://finance.sina.com.cn/test">TCL科技融资余额上升</a><br>
            </ul></div>
        "#;
        let cutoff = parse_time_epoch_millis("2026-06-01 00:00:00").unwrap();
        let items = parse_sina_stock_news(&html, &stock, &relations, cutoff, 10);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["source"], SOURCE_SINA_FINANCE);
        assert_eq!(items[0]["source_tier"], SOURCE_TIER_NEWS);
        assert_eq!(items[0]["stock_codes"][0], "000100.SZ");
        assert_eq!(items[0]["published_at"], "2026-06-25 17:37:00");
        assert!(items[0]["url"].as_str().unwrap().contains("cj.sina.cn"));
    }

    #[test]
    fn ths_linkage_news_parser_extracts_media_items() {
        let stock = json!({"code":"000100.SZ", "name":"TCL科技", "industry":"面板"});
        let relations = relation_map(&[
            json!({"target_code":"000100.SZ", "relation_type":"manufacturing_chain"}),
        ]);
        let html = r#"
            <div style="display:none" id="linkagedata">[
              {"seq":1,"ctime":1782176731,"curl":"http:\/\/news.10jqka.com.cn\/field\/20260623\/677633298.shtml","title":"TCL科技：6月22日获融资买入10.31亿元","source":"同花顺iNews","author":"两融研究","stocks":"000100","type":"yidong"},
              {"seq":2,"ctime":1782263131,"curl":"http:\/\/finance.eastmoney.com\/a.html","title":"TCL科技接受每日经济新闻采访","source":"每日经济新闻","stocks":"000100"}
            ]</div>
        "#;
        let cutoff = parse_time_epoch_millis("2026-06-01 00:00:00").unwrap();
        let items = parse_ths_linkage_news(&html, &stock, &relations, cutoff, 10);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["source"], "同花顺iNews");
        assert_eq!(items[1]["source"], "每日经济新闻");
        assert_eq!(items[0]["source_tier"], SOURCE_TIER_NEWS);
        assert_eq!(items[0]["stock_codes"][0], "000100.SZ");
        assert!(items[0]["published_at"]
            .as_str()
            .unwrap()
            .starts_with("2026-06-23"));
        assert!(items[0]["url"]
            .as_str()
            .unwrap()
            .starts_with("http://news.10jqka.com.cn"));
    }
    #[test]
    fn embedded_object_parser_handles_nested_json() {
        let html =
            r#"<script>var article_list={"re":[{"post_title":"订单增长","post_id":1}]};</script>"#;
        let raw = extract_embedded_object(html, "article_list").expect("object should parse");
        let value: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(value["re"][0]["post_title"], "订单增长");
    }
    #[test]
    fn news_scope_mode_supports_stock_only() {
        assert_eq!(
            news_scope_mode(&json!({"scope_mode":"stock_only"})),
            "stock_only"
        );
        assert_eq!(news_scope_mode(&json!({"mode":"plain_news"})), "stock_only");
        assert_eq!(news_scope_mode(&json!({})), "upstream");
    }

    #[test]
    fn stock_news_findings_ignore_neighbor_evidence() {
        let stocks = stock_map(&json!({"stocks":[
            {"code":"300750.SZ", "name":"CATL", "industry":"battery"},
            {"code":"002594.SZ", "name":"BYD", "industry":"auto"}
        ]}));
        let evidence = vec![
            news_evidence(
                &json!({"title":"target order", "source":"news", "sentiment":"positive", "stock_codes":["300750.SZ"], "relation_types":[]}),
            ),
            news_evidence(
                &json!({"title":"neighbor pressure", "source":"news", "sentiment":"negative", "stock_codes":["002594.SZ"], "relation_types":["supply_chain"]}),
            ),
        ];
        let findings = build_stock_news_findings(&["300750.SZ".to_string()], &evidence, &stocks);
        let rows = findings[0]["evidence"].as_array().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["stock_codes"][0], "300750.SZ");
    }

    #[test]
    fn sentiment_groups_separate_positive_and_negative_news() {
        let items = vec![
            news_evidence(
                &json!({"title":"订单增长", "source":"news", "sentiment":"positive", "stock_codes":["300750.SZ"], "relation_types":[]}),
            ),
            news_evidence(
                &json!({"title":"成本承压", "source":"news", "sentiment":"negative", "stock_codes":["002594.SZ"], "relation_types":[]}),
            ),
        ];
        let groups = sentiment_groups(&items, "plain_news");
        assert_eq!(groups["positive"].as_array().unwrap().len(), 1);
        assert_eq!(groups["negative"].as_array().unwrap().len(), 1);
    }
    #[test]
    fn findings_include_neighbor_evidence_from_chain_relations() {
        let stocks = stock_map(
            &json!({"stocks":[{"code":"300750.SZ", "name":"宁德时代", "industry":"电池"},{"code":"002594.SZ", "name":"比亚迪", "industry":"汽车"}]}),
        );
        let relations = vec![
            json!({"source_code":"300750.SZ", "target_code":"002594.SZ", "relation_type":"supply_chain", "weight":0.7}),
        ];
        let evidence = vec![news_evidence(
            &json!({"title":"比亚迪订单改善", "summary":"供应链需求改善", "source":"东方财富个股新闻", "source_tier":"news", "stock_codes":["002594.SZ"], "relation_types":["supply_chain"], "sentiment":"positive"}),
        )];
        let findings = build_findings(&["300750.SZ".to_string()], &relations, &evidence, &stocks);
        assert_eq!(findings[0]["direction"], "利好");
        assert_eq!(findings[0]["evidence"].as_array().unwrap().len(), 1);
    }
    #[test]
    fn llm_merge_normalizes_english_direction() {
        let base = vec![
            json!({"target":"宁德时代（300750.SZ）", "direction":"不确定", "confidence":"低", "impact_chain":"原始判断", "evidence":[], "pending_checks":[]}),
        ];
        let llm = json!({"findings":[{"target":"300750.SZ", "direction":"positive", "confidence":"medium", "impact_chain":"模型判断", "pending_checks":["核查公告"]}]});
        let merged = merge_llm_findings(&base, &llm);
        assert_eq!(merged[0]["direction"], "利好");
        assert_eq!(merged[0]["confidence"], "中");
        assert_eq!(merged[0]["impact_chain"], "模型判断");
    }
}
