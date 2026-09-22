use reqwest::Url;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};

const DAY: i64 = 86_400_000;
const CST: i64 = 28_800_000;
pub(crate) const RULE_VERSION: &str = "sentiment-v1.0";

fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

fn digest(s: &str) -> String {
    format!("{:x}", Sha256::digest(s.as_bytes()))
}

pub(crate) fn day_string(day: i64) -> String {
    crate::civil_date_from_days(day)
}

pub(crate) fn date_day(s: &str) -> Option<i64> {
    let date = if s.len() == 8 && s.bytes().all(|b| b.is_ascii_digit()) {
        format!("{}-{}-{}", &s[..4], &s[4..6], &s[6..8])
    } else {
        s.get(..10)?.to_owned()
    };
    let compact = date.replace('-', "");
    if compact.len() != 8 || !compact.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let day = crate::days_from_civil_key(&compact)?;
    (day_string(day) == date).then_some(day)
}

pub(crate) fn timestamp(v: &Value) -> Option<i64> {
    if let Some(n) = v.as_i64() {
        return (n > 0).then_some(n);
    }
    let s = v.as_str()?;
    date_day(s)?;
    crate::parse_cache_datetime_epoch_ms(s).and_then(|n| i64::try_from(n).ok())
}

fn normalized(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

// Host ownership is independent of self-reported names and tiers.
fn source(url: &str) -> (&'static str, bool) {
    let Ok(parsed) = Url::parse(url) else {
        return ("unverified", false);
    };
    if !matches!(parsed.scheme(), "http" | "https")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return ("unverified", false);
    }
    let Some(host) = parsed.host_str() else {
        return ("unverified", false);
    };
    let host = host.trim_end_matches('.');
    let belongs = |root: &str| host == root || host.ends_with(&format!(".{root}"));
    if belongs("gov.cn") {
        ("policy_official", true)
    } else if ["sse.com.cn", "szse.cn", "bse.cn", "cninfo.com.cn"]
        .iter()
        .any(|root| belongs(root))
    {
        ("filing", true)
    } else if [
        "yicai.com",
        "caixin.com",
        "stcn.com",
        "cnstock.com",
        "cs.com.cn",
    ]
    .iter()
    .any(|root| belongs(root))
    {
        ("news_media", true)
    } else if ["guba.eastmoney.com", "xueqiu.com"]
        .iter()
        .any(|root| belongs(root))
    {
        ("community", true)
    } else {
        ("unverified", false)
    }
}

fn source_priority(tier: &str) -> u8 {
    match tier {
        "policy_official" | "filing" => 0,
        "news_media" => 1,
        "community" => 2,
        _ => 3,
    }
}

fn canonical_url(raw: &str) -> Option<String> {
    let mut url = Url::parse(raw).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return None;
    }
    url.set_fragment(None);
    let mut pairs: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| {
            !k.to_ascii_lowercase().starts_with("utm_")
                && !matches!(k.as_ref(), "spm" | "from" | "ref" | "source" | "fbclid")
        })
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    pairs.sort();
    url.set_query(None);
    if !pairs.is_empty() {
        url.query_pairs_mut().extend_pairs(pairs);
    }
    Some(url.into())
}

fn associated(doc: &Value, code: &str, industry: Option<&str>) -> bool {
    let metadata = &doc["metadata"];
    let code_match = |v: &Value| v.as_str().is_some_and(|s| s.eq_ignore_ascii_case(code));
    [doc, metadata].iter().any(|v| {
        code_match(&v["stock_code"])
            || ["stock_codes", "mapped_stock_codes"]
                .iter()
                .any(|k| v[*k].as_array().is_some_and(|a| a.iter().any(code_match)))
            || industry.is_some_and(|name| {
                v["industries"]
                    .as_array()
                    .is_some_and(|a| a.iter().any(|s| s.as_str() == Some(name)))
                    || (text(v, "scope_type") == "industry"
                        && v["scope_tags"]
                            .as_array()
                            .is_some_and(|a| a.iter().any(|s| s.as_str() == Some(name))))
            })
    })
}

#[derive(Clone)]
struct Document {
    raw: Value,
    day: i64,
    first_seen: i64,
    published: i64,
    canonical: Option<String>,
    normalized: String,
    tier: &'static str,
    verified: bool,
    discussion: bool,
    sentiment: &'static str,
    duplicates: Vec<Value>,
}

#[derive(Clone, Copy)]
struct Bar {
    close: f64,
    volume: Option<f64>,
}

fn bars(data: &Value, code: &str, cutoff: i64) -> BTreeMap<i64, Bar> {
    data["histories"][code]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| {
            let day = date_day(text(v, "date"))?;
            // Daily OHLCV becomes usable after the 15:00 Shanghai close.
            if day * DAY - CST + 15 * 3_600_000 > cutoff {
                return None;
            }
            if v.get("first_seen_at")
                .and_then(timestamp)
                .is_some_and(|t| t > cutoff)
            {
                return None;
            }
            let close = v["close"].as_f64().filter(|n| n.is_finite() && *n > 0.0)?;
            let volume = v["volume"].as_f64().filter(|n| n.is_finite() && *n >= 0.0);
            Some((day, Bar { close, volume }))
        })
        .collect()
}

fn number(metrics: &mut Map<String, Value>, key: &str, value: Option<f64>) {
    metrics.insert(key.to_owned(), json!(value.filter(|v| v.is_finite())));
}

fn balance(docs: &[&Document]) -> Option<f64> {
    let positive = docs.iter().filter(|d| d.sentiment == "positive").count() as f64;
    let negative = docs.iter().filter(|d| d.sentiment == "negative").count() as f64;
    (positive + negative > 0.0).then(|| (positive - negative) / (positive + negative))
}

fn tier_balance(docs: &[&Document], tiers: &[&str]) -> Option<f64> {
    tiers.iter().find_map(|tier| {
        let selected: Vec<&Document> = docs
            .iter()
            .copied()
            .filter(|d| d.tier == *tier && matches!(d.sentiment, "positive" | "negative"))
            .collect();
        balance(&selected)
    })
}

fn hierarchy_balance(docs: &[&Document]) -> Option<f64> {
    tier_balance(
        docs,
        &[
            "policy_official",
            "filing",
            "news_media",
            "financial_snapshot",
            "research_report",
            "news",
        ],
    )
}

fn pct_change(current: f64, previous: f64) -> Option<f64> {
    (previous > 0.0).then(|| (current / previous - 1.0) * 100.0)
}

pub(crate) fn build_snapshot(
    stock_code: &str,
    window_days: u32,
    cutoff: i64,
    generation: &str,
    documents: Vec<Value>,
    data: Value,
    requested_industry: Option<&str>,
) -> Result<Value, String> {
    if window_days != 30 {
        return Err("sentiment snapshots require 30 natural days".into());
    }
    if cutoff <= 0 {
        return Err("invalid snapshot cutoff".into());
    }
    let stocks: Vec<&Value> = data["stocks"].as_array().into_iter().flatten().collect();
    let stock = stocks
        .iter()
        .find(|v| text(v, "code") == stock_code)
        .copied();
    let industry = stock.map(|v| text(v, "industry")).filter(|s| {
        !s.is_empty() && !matches!(*s, "沪市A股" | "深市A股" | "北交所" | "创业板" | "科创板")
    });
    if requested_industry.is_some_and(|s| Some(s) != industry) {
        return Err("industry must match the cached primary industry".into());
    }
    let end = (cutoff + CST) / DAY;
    let start = end - 29;
    let baseline_start = start - 90;
    let mut gaps: BTreeSet<String> = BTreeSet::new();
    let mut excluded_time = 0;
    let mut valid = Vec::new();
    for raw in documents {
        if !associated(&raw, stock_code, industry) {
            continue;
        }
        let Some(first_seen) = raw.get("first_seen_at").and_then(timestamp) else {
            excluded_time += 1;
            continue;
        };
        let Some(published) = raw.get("published_at").and_then(timestamp) else {
            excluded_time += 1;
            continue;
        };
        if first_seen > cutoff || published > cutoff {
            excluded_time += 1;
            continue;
        }
        let day = (published + CST) / DAY;
        if day < baseline_start || day > end {
            continue;
        }
        let url = text(&raw, "url");
        let (tier, verified) = source(url);
        let declared_discussion = matches!(
            text(&raw, "source_tier"),
            "community" | "discussion" | "social"
        );
        let discussion = if verified {
            tier == "community"
        } else {
            declared_discussion
        };
        let sentiment_value = raw["metadata"]
            .get("sentiment")
            .or_else(|| raw.get("sentiment"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let sentiment = match sentiment_value {
            "positive" | "正面" | "积极" => "positive",
            "negative" | "负面" | "消极" => "negative",
            "neutral" | "中性" => "neutral",
            _ => "uncertain",
        };
        let canonical = canonical_url(url);
        let normalized = normalized(&format!(
            "{} {}",
            text(&raw, "title"),
            text(&raw, "content")
        ));
        valid.push(Document {
            raw,
            day,
            first_seen,
            published,
            canonical,
            normalized,
            tier,
            verified,
            discussion,
            sentiment,
            duplicates: vec![],
        });
    }
    // Stable ordering keeps evidence IDs reproducible for the same frozen inputs.
    valid.sort_by(|a, b| {
        source_priority(a.tier)
            .cmp(&source_priority(b.tier))
            .then(a.published.cmp(&b.published))
            .then(a.first_seen.cmp(&b.first_seen))
            .then(text(&a.raw, "document_id").cmp(text(&b.raw, "document_id")))
    });
    let mut deduped: Vec<Document> = vec![];
    let mut url_index = HashMap::new();
    let mut text_index = HashMap::new();
    for doc in valid {
        let duplicate = doc
            .canonical
            .as_ref()
            .and_then(|s| url_index.get(&(doc.discussion, s.clone())))
            .copied()
            .or_else(|| {
                (!doc.normalized.is_empty())
                    .then(|| {
                        text_index
                            .get(&(doc.discussion, doc.normalized.clone()))
                            .copied()
                    })
                    .flatten()
            });
        if let Some(index) = duplicate {
            let existing: &mut Document = &mut deduped[index];
            let prior = existing.raw.clone();
            let prior_first_seen = existing.first_seen;
            let prior_duplicate = existing.duplicates.clone();
            if doc.raw["metadata"]["correction_of"].as_str().is_some()
                || doc.raw["metadata"]["retracted"].as_bool() == Some(true)
            {
                *existing = doc.clone();
                existing.first_seen = prior_first_seen;
                existing.duplicates = prior_duplicate;
            }
            existing.duplicates.push(json!({"document_id": prior["document_id"], "title": prior["title"], "content": prior["content"], "url": prior["url"], "published_at": prior["published_at"], "first_seen_at": prior_first_seen, "source_verified": existing.verified, "source_tier": existing.tier}));
            if let Some(url) = doc.canonical {
                url_index.insert((doc.discussion, url), index);
            }
            if !doc.normalized.is_empty() {
                text_index.insert((doc.discussion, doc.normalized), index);
            }
        } else {
            let index = deduped.len();
            if let Some(url) = &doc.canonical {
                url_index.insert((doc.discussion, url.clone()), index);
            }
            if !doc.normalized.is_empty() {
                text_index.insert((doc.discussion, doc.normalized.clone()), index);
            }
            deduped.push(doc);
        }
    }
    let retracted_ids: BTreeSet<String> = deduped
        .iter()
        .filter(|d| d.raw["metadata"]["retracted"].as_bool() == Some(true))
        .filter_map(|d| {
            d.raw["metadata"]["correction_of"]
                .as_str()
                .map(str::to_owned)
        })
        .collect();
    let current: Vec<&Document> = deduped
        .iter()
        .filter(|d| {
            d.day >= start
                && (!retracted_ids.contains(text(&d.raw, "document_id"))
                    || d.raw["metadata"]["retracted"].as_bool() == Some(true))
        })
        .collect();
    let facts: Vec<&Document> = current
        .iter()
        .copied()
        .filter(|d| {
            !d.discussion && d.verified && d.raw["metadata"]["retracted"].as_bool() != Some(true)
        })
        .collect();
    let discussions: Vec<&Document> = current
        .iter()
        .copied()
        .filter(|d| d.discussion && d.verified)
        .collect();
    let history: Vec<&Document> = deduped
        .iter()
        .filter(|d| d.day < start && d.verified)
        .collect();
    let history_days = history.iter().map(|d| d.day).collect::<BTreeSet<_>>().len();
    if facts.is_empty() {
        gaps.insert("No verified official or traditional-media facts in this window".into());
    }
    if discussions.is_empty() {
        gaps.insert(
            "No verified community discussion observations; missing counts are not zero".into(),
        );
    }
    if current.iter().any(|d| !d.verified) {
        gaps.insert(
            "Unverified sources are retained as evidence but excluded from scored metrics".into(),
        );
    }
    if excluded_time > 0 {
        gaps.insert(format!("{excluded_time} relevant documents excluded for unknown or future availability/publication time"));
    }
    gaps.insert(
        "Document counts describe the observed corpus, not complete market-wide feed coverage"
            .into(),
    );
    gaps.insert(
        "Sentiment labels are imported metadata, not verified factual direction or event impact"
            .into(),
    );
    let target_bars = bars(&data, stock_code, cutoff);
    let prices: Vec<(i64, Bar)> = target_bars
        .range(start..=end)
        .map(|(d, b)| (*d, *b))
        .collect();
    if prices.len() < 15 {
        gaps.insert("Fewer than 15 completed price days in the 30-day window".into());
    }
    if prices.iter().any(|(_, b)| b.volume.is_none()) || prices.is_empty() {
        gaps.insert("Daily volume coverage is incomplete".into());
    }
    gaps.insert(
        "Cached daily bars lack point-in-time ingestion provenance; no historical backtest claim"
            .into(),
    );
    let members: BTreeSet<String> = stocks
        .iter()
        .filter(|s| industry.is_some_and(|i| text(s, "industry") == i))
        .map(|s| text(s, "code").to_owned())
        .collect();
    let member_bars: Vec<BTreeMap<i64, Bar>> = members
        .iter()
        .map(|code| bars(&data, code, cutoff))
        .collect();
    let trade_days: BTreeSet<i64> = member_bars.iter().flat_map(|b| b.keys().copied()).collect();
    let mut industry_daily = BTreeMap::new();
    let mut latest_industry_covered = 0;
    for day in start..=end {
        let previous = trade_days.range(..day).next_back().copied();
        let returns: Vec<f64> = member_bars
            .iter()
            .filter_map(|b| Some((b.get(&day)?.close / b.get(&previous?)?.close - 1.0) * 100.0))
            .collect();
        if !returns.is_empty() {
            latest_industry_covered = returns.len();
        }
        let coverage = if members.is_empty() {
            0.0
        } else {
            returns.len() as f64 / members.len() as f64
        };
        let enough = members.len() >= 2 && coverage >= 0.8;
        industry_daily.insert(
            day,
            (
                enough.then(|| returns.iter().sum::<f64>() / returns.len() as f64),
                enough.then(|| {
                    returns.iter().filter(|r| **r > 0.0).count() as f64 / returns.len() as f64
                        * 100.0
                }),
                coverage,
            ),
        );
    }
    if industry.is_none() {
        gaps.insert("Primary industry is unavailable".into());
    }
    if members.len() < 2 || latest_industry_covered as f64 / (members.len().max(1) as f64) < 0.8 {
        gaps.insert(
            "Industry requires at least two fixed members and 80% paired-date coverage".into(),
        );
    }
    gaps.insert("Industry membership is frozen from the current primary-industry cache, not historical constituents".into());
    let mut metrics = Map::new();
    metrics.insert(
        "M1".into(),
        json!("messages: observed verified facts and discussions; sentiment from imported labels"),
    );
    metrics.insert(
        "M2".into(),
        json!("price: completed cached daily close and volume; returns in percent"),
    );
    metrics.insert("M3".into(), json!("industry: fixed primary-industry members, equal-weight paired-date returns, breadth in percent, coverage >=80%"));
    number(
        &mut metrics,
        "positive_count",
        (!facts.is_empty())
            .then(|| facts.iter().filter(|d| d.sentiment == "positive").count() as f64),
    );
    number(
        &mut metrics,
        "negative_count",
        (!facts.is_empty())
            .then(|| facts.iter().filter(|d| d.sentiment == "negative").count() as f64),
    );
    number(&mut metrics, "sentiment_balance", hierarchy_balance(&facts));
    number(
        &mut metrics,
        "official_sentiment_balance",
        tier_balance(&facts, &["policy_official", "filing"]),
    );
    number(
        &mut metrics,
        "media_sentiment_balance",
        tier_balance(&facts, &["news_media"]),
    );
    metrics.insert(
        "sentiment_direction_source".into(),
        json!(if facts
            .iter()
            .any(|d| matches!(d.tier, "policy_official" | "filing")
                && matches!(d.sentiment, "positive" | "negative"))
        {
            "official"
        } else if facts
            .iter()
            .any(|d| d.tier == "news_media" && matches!(d.sentiment, "positive" | "negative"))
        {
            "media"
        } else {
            "unknown"
        }),
    );
    number(
        &mut metrics,
        "discussion_count",
        (!discussions.is_empty()).then_some(discussions.len() as f64),
    );
    number(&mut metrics, "discussion_balance", balance(&discussions));
    number(
        &mut metrics,
        "verified_fact_count",
        (!facts.is_empty()).then_some(facts.len() as f64),
    );
    let recent: Vec<&Document> = facts.iter().copied().filter(|d| d.day >= end - 6).collect();
    let preceding: Vec<&Document> = facts
        .iter()
        .copied()
        .filter(|d| (end - 13..=end - 7).contains(&d.day))
        .collect();
    number(
        &mut metrics,
        "sentiment_change_7d",
        balance(&recent)
            .zip(balance(&preceding))
            .map(|(a, b)| a - b),
    );
    let discussion_recent = discussions.iter().filter(|d| d.day >= end - 6).count();
    let discussion_prior = discussions
        .iter()
        .filter(|d| (end - 13..=end - 7).contains(&d.day))
        .count();
    let observed_discussion_days: BTreeSet<i64> = discussions.iter().map(|d| d.day).collect();
    let discussion_complete = (end - 13..=end).all(|d| observed_discussion_days.contains(&d));
    number(
        &mut metrics,
        "discussion_change_7d_pct",
        discussion_complete
            .then(|| pct_change(discussion_recent as f64, discussion_prior as f64))
            .flatten(),
    );
    let history_discussions: Vec<&Document> =
        history.iter().copied().filter(|d| d.discussion).collect();
    let history_fact_days = history
        .iter()
        .filter(|d| !d.discussion)
        .map(|d| d.day)
        .collect::<BTreeSet<_>>();
    let historical_discussion_days: BTreeSet<i64> =
        history_discussions.iter().map(|d| d.day).collect();
    let historical_heats: Vec<f64> = (baseline_start + 6..start)
        .filter(|last| (*last - 6..=*last).all(|d| historical_discussion_days.contains(&d)))
        .map(|last| {
            history_discussions
                .iter()
                .filter(|d| (last - 6..=last).contains(&d.day))
                .count() as f64
        })
        .collect();
    let heat = (history_days >= 60
        && historical_heats.len() >= 30
        && (end - 6..=end).all(|d| observed_discussion_days.contains(&d)))
    .then(|| {
        historical_heats
            .iter()
            .filter(|v| **v <= discussion_recent as f64)
            .count() as f64
            / historical_heats.len() as f64
            * 100.0
    });
    number(&mut metrics, "heat_percentile", heat);
    if heat.is_none() {
        gaps.insert("Heat percentile unavailable: requires 60 historical observed days, 30 fully observed historical seven-day windows and seven current observed days".into());
    }
    number(
        &mut metrics,
        "baseline_sentiment_balance",
        (history_fact_days.len() >= 60)
            .then(|| {
                hierarchy_balance(
                    &history
                        .iter()
                        .copied()
                        .filter(|d| !d.discussion)
                        .collect::<Vec<_>>(),
                )
            })
            .flatten(),
    );
    number(
        &mut metrics,
        "baseline_history_days",
        Some(history_days as f64),
    );
    let return_between = |start_day: i64| -> Option<f64> {
        let (last_day, last) = target_bars.range(start..=end).next_back()?;
        if end - last_day > 2 {
            return None;
        }
        let (_, first) = target_bars.range(..=start_day).next_back()?;
        let (first_day, _) = target_bars.range(..=start_day).next_back()?;
        if start_day - first_day > 7 {
            return None;
        }
        pct_change(last.close, first.close)
    };
    number(
        &mut metrics,
        "price_return_30d_pct",
        return_between(start - 1),
    );
    number(&mut metrics, "price_return_7d_pct", return_between(end - 7));
    number(
        &mut metrics,
        "latest_close",
        prices.last().map(|(_, b)| b.close),
    );
    let recent_vol: Vec<f64> = prices
        .iter()
        .filter(|(d, _)| *d >= end - 6)
        .filter_map(|(_, b)| b.volume)
        .collect();
    let prior_prices: Vec<&(i64, Bar)> = prices
        .iter()
        .filter(|(d, _)| (end - 13..=end - 7).contains(d))
        .collect();
    let prior_vol: Vec<f64> = prior_prices.iter().filter_map(|(_, b)| b.volume).collect();
    let recent_price_count = prices.iter().filter(|(d, _)| *d >= end - 6).count();
    number(
        &mut metrics,
        "volume_change_7d_pct",
        if !recent_vol.is_empty()
            && !prior_vol.is_empty()
            && recent_vol.len() == recent_price_count
            && prior_vol.len() == prior_prices.len()
        {
            pct_change(
                recent_vol.iter().sum::<f64>() / recent_vol.len() as f64,
                prior_vol.iter().sum::<f64>() / prior_vol.len() as f64,
            )
        } else {
            None
        },
    );
    let latest_industry = industry_daily
        .iter()
        .rev()
        .find(|(_, v)| v.0.is_some())
        .map(|(_, v)| *v);
    number(
        &mut metrics,
        "industry_return_pct",
        latest_industry.and_then(|v| v.0),
    );
    number(
        &mut metrics,
        "industry_breadth_pct",
        latest_industry.and_then(|v| v.1),
    );
    number(
        &mut metrics,
        "industry_coverage_pct",
        (!members.is_empty())
            .then_some(latest_industry_covered as f64 / members.len().max(1) as f64 * 100.0),
    );
    let industry_period = |since: i64| -> Option<f64> {
        let finish_day = *trade_days.range(start..=end).next_back()?;
        if end - finish_day > 2 {
            return None;
        }
        let beginning = trade_days.range(..=since).next_back()?;
        if since - beginning > 7 {
            return None;
        }
        let returns: Vec<f64> = member_bars
            .iter()
            .filter_map(|b| pct_change(b.get(&finish_day)?.close, b.get(beginning)?.close))
            .collect();
        (members.len() >= 2 && returns.len() as f64 / members.len() as f64 >= 0.8)
            .then(|| returns.iter().sum::<f64>() / returns.len() as f64)
    };
    number(
        &mut metrics,
        "industry_return_30d_pct",
        industry_period(start - 1),
    );
    number(
        &mut metrics,
        "industry_return_7d_pct",
        industry_period(end - 7),
    );
    let timeline: Vec<Value> = (start..=end).map(|day| {
        let day_facts: Vec<&Document> = facts.iter().copied().filter(|d| d.day == day).collect();
        let day_discussions = discussions.iter().filter(|d| d.day == day).count();
        let bar = target_bars.get(&day);
        let (industry_return, industry_breadth, coverage) = industry_daily[&day];
        json!({"date": day_string(day), "positive": (!day_facts.is_empty()).then(|| day_facts.iter().filter(|d| d.sentiment == "positive").count()), "negative": (!day_facts.is_empty()).then(|| day_facts.iter().filter(|d| d.sentiment == "negative").count()), "discussion_count": (day_discussions>0).then_some(day_discussions), "sentiment_balance": hierarchy_balance(&day_facts), "close": bar.map(|b| b.close), "volume": bar.and_then(|b| b.volume), "industry_return": industry_return, "industry_breadth": industry_breadth, "industry_coverage_pct": (!members.is_empty()).then_some(coverage*100.0)})
    }).collect();
    let evidence: Vec<Value> = current.iter().enumerate().map(|(i,d)| {
        let content = text(&d.raw, "content");
        let coverage = if text(&d.raw["metadata"], "coverage") == "full_text" || d.raw["metadata"]["full_text"].as_bool() == Some(true) { "full_text" } else { "excerpt" };
        json!({"id": format!("E{}",i+1), "document_id": d.raw["document_id"], "event_id": format!("event-{}", &digest(d.canonical.as_deref().unwrap_or(&d.normalized))[..16]), "title": d.raw["title"], "excerpt": content.chars().take(1200).collect::<String>(), "source_name": d.raw["source_name"], "source_tier": d.tier, "source_verified": d.verified, "published_at": d.raw["published_at"], "first_seen_at": d.first_seen, "url": d.canonical, "sentiment": d.sentiment, "pool": if d.discussion {"discussion"} else {"fact"}, "coverage": coverage, "duplicate_count": d.duplicates.len(), "provenance": {"original_url": d.raw["url"], "original_source_tier": d.raw["source_tier"], "raw_content": content, "raw_content_sha256": digest(content), "duplicate_versions": d.duplicates, "correction_of": d.raw["metadata"]["correction_of"], "retracted": d.raw["metadata"]["retracted"], "source_verification": "host ownership only; article claims are not validated"}})
    }).collect();
    let mut snapshot = json!({"snapshot_id": "", "stock_code": stock_code, "stock_name": stock.map(|v| text(v,"name")).unwrap_or(stock_code), "industry": industry, "window_days": 30, "cutoff": cutoff, "generation": generation, "rule_version": RULE_VERSION, "evidence": evidence, "timeline": timeline, "metrics": metrics, "coverage": {"facts": facts.len(), "discussions": discussions.len(), "price_days": prices.len(), "history_days": history_days, "industry_members": members.len(), "industry_covered": latest_industry_covered, "gaps": gaps.into_iter().collect::<Vec<_>>()}, "industry_member_codes": members.into_iter().collect::<Vec<_>>(), "baseline": {"start": day_string(baseline_start), "end": day_string(start-1), "natural_days":90, "verified_documents":history.len(), "heat_comparison_windows": historical_heats.len()}, "quality_gates": {"messages": facts.len() >= 10 && facts.iter().filter(|d| matches!(d.sentiment,"positive"|"negative")).count() >= 5, "price": prices.len()>=15, "industry":latest_industry.is_some(), "historical_heat":heat.is_some()}, "metric_references": {"M1": ["positive_count","negative_count","sentiment_balance","discussion_count","sentiment_change_7d","heat_percentile"], "M2": ["price_return_30d_pct","price_return_7d_pct","volume_change_7d_pct"], "M3": ["industry_return_30d_pct","industry_return_7d_pct","industry_breadth_pct","industry_coverage_pct"]}});
    snapshot["snapshot_id"] = json!(format!("snapshot-{}", &digest(&snapshot.to_string())[..24]));
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn cutoff() -> i64 {
        timestamp(&json!("2026-09-15T16:00:00+08:00")).unwrap()
    }
    fn doc(id: &str, url: &str, body: &str) -> Value {
        json!({"document_id":id,"title":"A release","content":body,"source_tier":"official","source_name":"claimed regulator","url":url,"published_at":"2026-09-14T10:00:00+08:00","first_seen_at":cutoff()-DAY,"metadata":{"stock_codes":["000001.SZ"],"sentiment":"positive"}})
    }
    fn data() -> Value {
        json!({"stocks":[{"code":"000001.SZ","name":"A","industry":"Bank"},{"code":"000002.SZ","name":"B","industry":"Bank"}],"histories":{"000001.SZ":[{"date":"2026-09-14","close":10,"volume":100},{"date":"2026-09-15","close":12,"volume":200},{"date":"2026-09-16","close":900}],"000002.SZ":[{"date":"2026-09-14","close":10},{"date":"2026-09-15","close":9}]}})
    }
    #[test]
    fn host_boundary_does_not_trust_claimed_tier() {
        for url in [
            "https://gov.cn.attacker.test/a",
            "https://evilgov.cn/a",
            "https://www.yicai.com.attacker.test/a",
            "https://gov.cn@attacker.test/a",
            "file://www.gov.cn/a",
        ] {
            assert!(!source(url).1, "{url}");
        }
        assert_eq!(
            source("https://www.csrc.gov.cn/a"),
            ("policy_official", true)
        );
        assert_eq!(source("https://www.cninfo.com.cn/a"), ("filing", true));
        assert_eq!(source("https://www.yicai.com/a"), ("news_media", true));
        let s = build_snapshot(
            "000001.SZ",
            30,
            cutoff(),
            "g",
            vec![doc("1", "https://evil.test/a", "body")],
            data(),
            None,
        )
        .unwrap();
        assert_eq!(s["coverage"]["facts"], 0);
        assert!(s["metrics"]["positive_count"].is_null());
        assert_eq!(s["evidence"][0]["source_verified"], false);
    }
    #[test]
    fn excludes_future_unknown_and_unrelated_documents() {
        let valid = doc("valid", "https://www.gov.cn/a", "a");
        let mut future = doc("future", "https://www.gov.cn/b", "b");
        future["published_at"] = json!("2026-09-16");
        let mut seen = doc("seen", "https://www.gov.cn/c", "c");
        seen["first_seen_at"] = json!(cutoff() + 1);
        let mut missing = doc("missing", "https://www.gov.cn/d", "d");
        missing["first_seen_at"] = Value::Null;
        let mut unrelated = doc("unrelated", "https://www.gov.cn/e", "000001.SZ text alone");
        unrelated["metadata"] = json!({"stock_codes":["999999.SZ"]});
        let s = build_snapshot(
            "000001.SZ",
            30,
            cutoff(),
            "g",
            vec![valid, future, seen, missing, unrelated],
            data(),
            None,
        )
        .unwrap();
        assert_eq!(s["evidence"].as_array().unwrap().len(), 1);
        assert_eq!(s["timeline"].as_array().unwrap().len(), 30);
        assert_eq!(s["metrics"]["latest_close"], 12.0);
    }
    #[test]
    fn normalized_text_and_canonical_urls_deduplicate_without_invented_events() {
        let docs = vec![
            doc("a", "https://www.gov.cn/a?utm_source=feed", "Same body!"),
            doc("b", "https://www.gov.cn/a#top", "corrected body"),
            doc("c", "https://www.yicai.com/b", "same BODY"),
        ];
        let s = build_snapshot("000001.SZ", 30, cutoff(), "g", docs, data(), None).unwrap();
        assert_eq!(s["evidence"].as_array().unwrap().len(), 1);
        assert_eq!(s["evidence"][0]["duplicate_count"], 2);
        assert_eq!(
            s["evidence"][0]["provenance"]["duplicate_versions"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }
    #[test]
    fn industry_equal_weight_breadth_and_missing_coverage_are_explicit() {
        let s = build_snapshot("000001.SZ", 30, cutoff(), "g", vec![], data(), None).unwrap();
        assert!((s["metrics"]["industry_return_pct"].as_f64().unwrap() - 5.0).abs() < 1e-8);
        assert_eq!(s["metrics"]["industry_breadth_pct"], 50.0);
        let mut missing = data();
        missing["histories"]["000002.SZ"] = json!([]);
        let s = build_snapshot("000001.SZ", 30, cutoff(), "g", vec![], missing, None).unwrap();
        assert!(s["metrics"]["industry_return_pct"].is_null());
        assert!(s["metrics"]["heat_percentile"].is_null());
        assert!(s["timeline"][0]["discussion_count"].is_null());
    }
    #[test]
    fn unfinished_daily_bar_is_not_visible_and_snapshot_is_deterministic() {
        let early = timestamp(&json!("2026-09-15T14:00:00+08:00")).unwrap();
        let s = build_snapshot("000001.SZ", 30, early, "g", vec![], data(), None).unwrap();
        assert_eq!(s["metrics"]["latest_close"], 10.0);
        assert_eq!(
            s,
            build_snapshot("000001.SZ", 30, early, "g", vec![], data(), None).unwrap()
        );
        assert!(build_snapshot(
            "000001.SZ",
            30,
            early,
            "g",
            vec![],
            data(),
            Some("opportunistic concept")
        )
        .is_err());
        assert!(date_day("2026-02-30").is_none());
    }
    #[test]
    fn community_copy_cannot_displace_official_evidence() {
        let mut post = doc("a", "https://xueqiu.com/a", "same announcement");
        post["published_at"] = json!("2026-09-13T10:00:00+08:00");
        let official = doc("b", "https://www.cninfo.com.cn/a", "same announcement");
        let s = build_snapshot(
            "000001.SZ",
            30,
            cutoff(),
            "g",
            vec![post, official],
            data(),
            None,
        )
        .unwrap();
        assert_eq!(s["coverage"]["facts"], 1);
        assert_eq!(s["coverage"]["discussions"], 1);
        assert_eq!(s["evidence"][0]["source_tier"], "filing");
    }

    #[test]
    fn official_original_outranks_earlier_media_copy() {
        let mut media = doc("a", "https://www.yicai.com/a", "same announcement");
        media["published_at"] = json!("2026-09-13T10:00:00+08:00");
        let official = doc("b", "https://www.cninfo.com.cn/a", "same announcement");
        let s = build_snapshot(
            "000001.SZ",
            30,
            cutoff(),
            "g",
            vec![media, official],
            data(),
            None,
        )
        .unwrap();
        assert_eq!(s["coverage"]["facts"], 1);
        assert_eq!(s["evidence"][0]["source_tier"], "filing");
        assert_eq!(s["evidence"][0]["duplicate_count"], 1);
    }
    #[test]
    fn hierarchy_does_not_let_media_volume_reverse_official_direction() {
        let mut official = doc(
            "official",
            "https://www.cninfo.com.cn/official",
            "official loss",
        );
        official["metadata"]["sentiment"] = json!("negative");
        let mut docs = vec![official];
        for n in 0..20 {
            docs.push(doc(
                &format!("m{n}"),
                &format!("https://www.yicai.com/{n}"),
                &format!("media opinion {n}"),
            ));
        }
        let s = build_snapshot("000001.SZ", 30, cutoff(), "g", docs, data(), None).unwrap();
        assert_eq!(s["metrics"]["sentiment_balance"], -1.0);
        assert_eq!(s["metrics"]["official_sentiment_balance"], -1.0);
        assert_eq!(s["metrics"]["media_sentiment_balance"], 1.0);
        assert_eq!(s["metrics"]["sentiment_direction_source"], "official");
        assert_eq!(s["timeline"][28]["sentiment_balance"], -1.0);
    }

    #[test]
    fn current_correction_supersedes_old_version_and_preserves_audit() {
        let mut original = doc(
            "original",
            "https://www.cninfo.com.cn/event",
            "old positive claim",
        );
        original["published_at"] = json!("2026-07-20T10:00:00+08:00");
        original["first_seen_at"] = json!(timestamp(&original["published_at"]).unwrap());
        let mut correction = doc(
            "correction",
            "https://www.cninfo.com.cn/event",
            "corrected negative claim",
        );
        correction["metadata"]["sentiment"] = json!("negative");
        correction["metadata"]["correction_of"] = json!("original");
        let mut future = correction.clone();
        future["document_id"] = json!("future");
        future["first_seen_at"] = json!(cutoff() + 1);
        let s = build_snapshot(
            "000001.SZ",
            30,
            cutoff(),
            "g",
            vec![original, correction, future],
            data(),
            None,
        )
        .unwrap();
        assert_eq!(s["coverage"]["facts"], 1);
        assert_eq!(s["metrics"]["sentiment_balance"], -1.0);
        assert_eq!(s["evidence"][0]["document_id"], "correction");
        assert_eq!(
            s["evidence"][0]["provenance"]["duplicate_versions"][0]["document_id"],
            "original"
        );
    }

    #[test]
    fn explicit_retraction_at_new_url_removes_original_from_scored_facts() {
        let original = doc(
            "original",
            "https://www.cninfo.com.cn/original",
            "old claim",
        );
        let mut retraction = doc(
            "retraction",
            "https://www.cninfo.com.cn/retraction",
            "withdrawn claim",
        );
        retraction["published_at"] = json!("2026-09-15T10:00:00+08:00");
        retraction["metadata"]["correction_of"] = json!("original");
        retraction["metadata"]["retracted"] = json!(true);
        let s = build_snapshot(
            "000001.SZ",
            30,
            cutoff(),
            "g",
            vec![original, retraction],
            data(),
            None,
        )
        .unwrap();
        assert_eq!(s["coverage"]["facts"], 0);
        assert!(s["metrics"]["sentiment_balance"].is_null());
        assert_eq!(s["evidence"].as_array().unwrap().len(), 1);
        assert_eq!(s["evidence"][0]["document_id"], "retraction");
    }

    #[test]
    fn missing_last_week_is_not_zero_return_or_sufficient_price_coverage() {
        let mut d = data();
        let end = (cutoff() + CST) / DAY;
        let observations: Vec<Value> = (end - 29..=end - 8)
            .map(|day| json!({"date":day_string(day),"close":10.0,"volume":100.0}))
            .collect();
        d["histories"]["000001.SZ"] = json!(observations);
        d["histories"]["000002.SZ"] = d["histories"]["000001.SZ"].clone();
        let s = build_snapshot("000001.SZ", 30, cutoff(), "g", vec![], d, None).unwrap();
        assert!(s["metrics"]["price_return_7d_pct"].is_null());
        assert!(s["metrics"]["industry_return_7d_pct"].is_null());
        assert!(s["metrics"]["industry_return_pct"].is_null());
        assert_eq!(s["quality_gates"]["price"], false);
        assert_eq!(s["quality_gates"]["industry"], false);
    }
}
