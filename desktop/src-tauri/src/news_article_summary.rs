use futures::{stream, StreamExt};
use scraper::{Html, Selector};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    future::Future,
    sync::OnceLock,
    time::Duration,
};

pub(super) const FIELDS: [&str; 4] = [
    "article_summary",
    "article_summary_status",
    "article_summary_url",
    "article_summary_fetched_at_epoch_ms",
];

pub(super) fn allowed_url(raw: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(raw) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };
    matches!(url.scheme(), "http" | "https")
        && url.username().is_empty()
        && url.password().is_none()
        && (host.ends_with(".sina.cn") || host.ends_with(".sina.com.cn"))
        && (url.path().starts_with("/articles/view/")
            || url.path().starts_with("/pc/7x24/")
            || url.path().ends_with(".shtml"))
        && url.port().is_none()
}

pub(super) fn extract(html: &str, title: &str) -> Option<String> {
    static WORDS: OnceLock<jieba_rs::Jieba> = OnceLock::new();
    let document = Html::parse_document(html);
    let paragraphs = Selector::parse("p").ok()?;
    let mut candidates = Vec::new();
    for selector in [
        "#artibody",
        "[itemprop=articleBody]",
        ".article-body",
        ".article-content",
    ] {
        let selector = Selector::parse(selector).ok()?;
        if let Some(body) = document.select(&selector).next() {
            candidates.extend(
                body.select(&paragraphs)
                    .map(|p| p.text().collect::<String>()),
            );
            if !candidates.is_empty() {
                break;
            }
        }
    }
    if candidates.is_empty() {
        let metadata =
            Selector::parse("meta[name=description],meta[property='og:description']").ok()?;
        candidates.extend(
            document
                .select(&metadata)
                .filter_map(|node| node.value().attr("content").map(str::to_owned)),
        );
    }
    let mut seen = HashSet::new();
    let keywords = WORDS
        .get_or_init(jieba_rs::Jieba::new)
        .cut(title, false)
        .into_iter()
        .map(|token| token.word)
        .filter(|word| word.chars().count() >= 2)
        .collect::<HashSet<_>>();
    let mut best: Option<(usize, String)> = None;
    for text in candidates {
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if text.chars().count() < 15
            || text == title.trim()
            || !seen.insert(text.clone())
            || [
                "来源：",
                "来源:",
                "特别声明",
                "海量资讯",
                "责任编辑",
                "新浪财经为您提供",
            ]
            .iter()
            .any(|prefix| text.starts_with(prefix))
        {
            continue;
        }
        let score = keywords.iter().filter(|word| text.contains(**word)).count();
        if best.as_ref().is_none_or(|(old, _)| score > *old) {
            best = Some((score, text));
        }
    }
    let (_, text) = best?;
    let mut result = String::new();
    let mut sentences = 0;
    for (index, character) in text.chars().enumerate() {
        if index >= 240 {
            result.push('…');
            break;
        }
        result.push(character);
        if matches!(character, '。' | '！' | '？' | '!' | '?') {
            sentences += 1;
        }
        if sentences >= 2 {
            break;
        }
    }
    Some(result)
}

fn has_summary(item: &Value) -> bool {
    item.get("article_summary")
        .and_then(Value::as_str)
        .is_some_and(|text| {
            text.chars().count() >= 15 && Some(text) != item.get("title").and_then(Value::as_str)
        })
}

pub(super) fn merge_fields(target: &mut Value, fresh: &Value) {
    if has_summary(target) && !has_summary(fresh) {
        return;
    }
    for key in FIELDS {
        if let Some(value) = fresh.get(key) {
            target[key] = value.clone();
        }
    }
}

pub(super) async fn enrich(
    network_payload: Option<&Value>,
    items: &mut [Value],
    cached: &[Value],
) -> Result<usize, String> {
    let default_redirects = reqwest::redirect::Policy::default();
    let client = crate::market::build_http_client_with_redirect_policy(
        "Mozilla/5.0 GuXuanYou/0.4 article-summary",
        Duration::from_secs(6),
        network_payload,
        reqwest::redirect::Policy::custom(move |attempt| {
            if !allowed_url(attempt.url().as_str()) {
                attempt.error("article redirect is outside the allowed sources")
            } else {
                default_redirects.redirect(attempt)
            }
        }),
    )?;
    let client = &client;
    let priority_urls = network_payload
        .and_then(|payload| payload.get("summary_priority_urls"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .take(120)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    Ok(
        enrich_with(items, cached, &priority_urls, |url| async move {
            let response = client
                .get(&url)
                .header("User-Agent", "Mozilla/5.0 GuXuanYou/0.4 article-summary")
                .send()
                .await
                .map_err(|error| error.to_string())?;
            if !response.status().is_success() || !allowed_url(response.url().as_str()) {
                return Err("article is unavailable".into());
            }
            let mut bytes = Vec::new();
            let mut body = response.bytes_stream();
            while let Some(chunk) = body.next().await {
                let chunk = chunk.map_err(|error| error.to_string())?;
                if bytes.len() + chunk.len() > 2 * 1024 * 1024 {
                    return Err("article exceeds size limit".into());
                }
                bytes.extend_from_slice(&chunk);
            }
            Ok(super::decode_http_text(&bytes))
        })
        .await,
    )
}

async fn enrich_with<F, Fut>(
    items: &mut [Value],
    cached: &[Value],
    priority_urls: &[String],
    fetch: F,
) -> usize
where
    F: Fn(String) -> Fut,
    Fut: Future<Output = Result<String, String>>,
{
    let cache = cached
        .iter()
        .filter_map(|item| Some((item.get("id")?.as_str()?, item)))
        .collect::<HashMap<_, _>>();
    let mut pending = Vec::new();
    for (index, item) in items.iter_mut().enumerate() {
        if let Some(old) = item
            .get("id")
            .and_then(Value::as_str)
            .and_then(|id| cache.get(id))
        {
            merge_fields(item, old);
        }
        let title = item
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let raw_summary = item
            .get("summary")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let url = item.get("url").and_then(Value::as_str).unwrap_or_default();
        if has_summary(item) || raw_summary.trim() != title.trim() || !allowed_url(url) {
            continue;
        }
        pending.push((index, url.to_owned(), title.to_owned()));
    }
    let priority = priority_urls
        .iter()
        .enumerate()
        .map(|(rank, url)| (url.as_str(), rank))
        .collect::<HashMap<_, _>>();
    pending.sort_by_key(|(_, url, _)| priority.get(url.as_str()).copied().unwrap_or(usize::MAX));
    pending.truncate(80);
    let work = stream::iter(pending.into_iter().map(|(index, url, title)| {
        let task = fetch(url.clone());
        async move {
            let summary = tokio::time::timeout(Duration::from_secs(6), task)
                .await
                .ok()
                .and_then(Result::ok)
                .and_then(|html| extract(&html, &title));
            (index, url, summary)
        }
    }))
    .buffer_unordered(4);
    futures::pin_mut!(work);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(25);
    let mut enriched = 0;
    while let Ok(Some((index, url, summary))) = tokio::time::timeout_at(deadline, work.next()).await
    {
        items[index]["article_summary_status"] = json!(if summary.is_some() {
            "available"
        } else {
            "unavailable"
        });
        items[index]["article_summary_url"] = json!(url);
        items[index]["article_summary_fetched_at_epoch_ms"] = json!(super::epoch_millis());
        if let Some(summary) = summary {
            items[index]["article_summary"] = json!(summary);
            enriched += 1;
        }
    }
    enriched
}

#[cfg(test)]
mod tests {
    use super::*;

    fn article_proxy_fixture(
        redirects: Vec<&'static str>,
    ) -> (
        Value,
        std::sync::mpsc::Sender<()>,
        std::thread::JoinHandle<Vec<String>>,
    ) {
        use std::io::{BufRead, BufReader, Write};
        use std::net::TcpListener;
        use std::sync::mpsc;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let (stop, stopped) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let mut requests = Vec::new();
            loop {
                if stopped.try_recv().is_ok() {
                    break;
                }
                let (mut connection, _) = match listener.accept() {
                    Ok(connection) => connection,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        if stopped.recv_timeout(Duration::from_millis(5)).is_ok() {
                            break;
                        }
                        continue;
                    }
                    Err(error) => panic!("article proxy accept failed: {error}"),
                };
                connection.set_nonblocking(false).unwrap();
                connection
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut reader = BufReader::new(&mut connection);
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                requests.push(line.split_whitespace().nth(1).unwrap().to_string());
                loop {
                    line.clear();
                    if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                        break;
                    }
                }
                let response = if let Some(location) = redirects.get(requests.len() - 1) {
                    format!("HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                } else {
                    let body = "<div id=\"artibody\"><p>腾讯混元上线招聘，社招有60余个岗位。青云课题涵盖逾160个岗位。</p></div>";
                    format!("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
                };
                connection.write_all(response.as_bytes()).unwrap();
            }
            requests
        });
        (
            json!({"network_proxy_mode":"manual","network_proxy_url":format!("http://{address}")}),
            stop,
            worker,
        )
    }

    #[tokio::test]
    async fn article_summary_blocks_redirect_before_requesting_outside_source() {
        let (payload, stop, worker) = article_proxy_fixture(vec![
            "http://finance.sina.com.cn/stock/article.shtml",
            "http://outside.example.test/articles/view/1",
        ]);
        let mut items = vec![json!({"id":"one","title":"腾讯招聘","summary":"腾讯招聘",
            "url":"http://t.cj.sina.cn/articles/view/1"})];
        let result = enrich(Some(&payload), &mut items, &[]).await;
        stop.send(()).unwrap();
        let requests = worker.join().unwrap();
        assert_eq!(result.unwrap(), 0);
        assert_eq!(items[0]["article_summary_status"], "unavailable");
        assert_eq!(
            requests,
            vec![
                "http://t.cj.sina.cn/articles/view/1",
                "http://finance.sina.com.cn/stock/article.shtml",
            ],
            "redirects must be checked before the next request is sent"
        );
    }

    #[tokio::test]
    async fn article_summary_follows_allowed_redirect_with_configured_proxy() {
        let (payload, stop, worker) =
            article_proxy_fixture(vec!["http://finance.sina.com.cn/stock/article.shtml"]);
        let mut items = vec![json!({"id":"one","title":"腾讯招聘","summary":"腾讯招聘",
            "url":"http://t.cj.sina.cn/articles/view/1"})];
        let result = enrich(Some(&payload), &mut items, &[]).await;
        stop.send(()).unwrap();
        let requests = worker.join().unwrap();
        assert_eq!(result.unwrap(), 1);
        assert_eq!(
            items[0]["article_summary"],
            "腾讯混元上线招聘，社招有60余个岗位。青云课题涵盖逾160个岗位。"
        );
        assert_eq!(
            requests,
            vec![
                "http://t.cj.sina.cn/articles/view/1",
                "http://finance.sina.com.cn/stock/article.shtml",
            ]
        );
    }

    #[tokio::test]
    async fn article_summary_prioritizes_visible_older_news_before_the_batch_limit() {
        let (mut payload, stop, worker) = article_proxy_fixture(Vec::new());
        let mut items: Vec<Value> = (0..81).map(|index| json!({"id":format!("article-{index}"),
            "title":"腾讯招聘","summary":"腾讯招聘","url":format!("http://t.cj.sina.cn/articles/view/{index}")})).collect();
        payload["summary_priority_urls"] = json!([items[80]["url"]]);
        let result = enrich(Some(&payload), &mut items, &[]).await;
        stop.send(()).unwrap();
        let requested = worker.join().unwrap();
        assert_eq!(result.unwrap(), 80);
        assert!(has_summary(&items[80]), "a visible old news item must receive a summary before newly fetched unrelated news consumes the batch");
        assert_eq!(requested.len(), 80);
        assert!(requested.contains(&"http://t.cj.sina.cn/articles/view/80".to_string()));
    }

    #[test]
    fn article_summary_selects_relevant_body_instead_of_headline_or_navigation() {
        let html = r#"<p>导航：登录新浪财经</p><div id="artibody">
          <p>来源：投资界</p><p>这是关于人工智能模型发展的报道。</p>
          <p>姚顺雨执掌的腾讯混元上线招聘。青云课题涵盖逾160个岗位，混元社招有60余个岗位。</p>
          <p>特别声明：以上内容来自作者。</p></div><p>海量资讯尽在新浪财经。</p>"#;
        let summary = extract(html, "姚顺雨摇人，腾讯正在大举招聘").unwrap();
        assert_eq!(
            summary,
            "姚顺雨执掌的腾讯混元上线招聘。青云课题涵盖逾160个岗位，混元社招有60余个岗位。"
        );
    }

    #[test]
    fn article_summary_rejects_title_only_metadata() {
        assert!(extract(
            r#"<meta name="description" content="腾讯发布业务进展">"#,
            "腾讯发布业务进展"
        )
        .is_none());
        assert_eq!(
            extract(
                r#"<meta name="description" content="腾讯混元上线招聘，社招有60余个岗位。青云课题涵盖逾160个岗位。">"#,
                "腾讯招聘"
            ),
            Some("腾讯混元上线招聘，社招有60余个岗位。青云课题涵盖逾160个岗位。".into())
        );
    }

    #[test]
    fn article_summary_urls_are_limited_to_public_sina_articles() {
        assert!(allowed_url(
            "https://t.cj.sina.cn/articles/view/1651428902/626ece2602001lifc"
        ));
        assert!(allowed_url(
            "https://t.cj.sina.com.cn/articles/view/1651428902/626ece2602001lifc"
        ));
        assert!(allowed_url(
            "https://finance.sina.com.cn/stock/t/2026-11-09/doc-iniyazea3082001.shtml"
        ));
        for url in [
            "https://127.0.0.1/articles/view/1",
            "https://sina.com.cn.evil.test/articles/view/1",
            "https://user:secret@t.cj.sina.cn/articles/view/1",
            "file:///articles/view/1",
        ] {
            assert!(!allowed_url(url));
        }
    }

    #[tokio::test]
    async fn article_summary_reuses_cache_and_keeps_headline_evidence() {
        let raw = serde_json::json!({"id":"one","title":"腾讯招聘","summary":"腾讯招聘","url":"https://t.cj.sina.cn/articles/view/1"});
        let mut cached = raw.clone();
        cached["article_summary"] =
            serde_json::json!("腾讯混元上线招聘，社招有60余个岗位。青云课题涵盖逾160个岗位。");
        let mut items = vec![raw];
        let updated = enrich_with(&mut items, &[cached], &[], |_| async {
            panic!("a cached summary must not fetch the article again")
        })
        .await;
        assert_eq!(updated, 0);
        assert_eq!(items[0]["summary"], "腾讯招聘");
        assert!(items[0]["article_summary"]
            .as_str()
            .unwrap()
            .contains("60余"));
    }

    #[tokio::test]
    async fn article_summary_failure_preserves_message_and_records_unavailable_state() {
        let mut items = vec![
            serde_json::json!({"id":"one","title":"腾讯招聘","summary":"腾讯招聘","url":"https://t.cj.sina.cn/articles/view/1"}),
        ];
        assert_eq!(
            enrich_with(&mut items, &[], &[], |_| async { Err("offline".into()) }).await,
            0
        );
        assert_eq!(items[0]["title"], "腾讯招聘");
        assert_eq!(items[0]["article_summary_status"], "unavailable");
    }

    #[tokio::test]
    #[ignore = "explicit live public article validation"]
    async fn article_summary_live_probe() {
        let input = std::env::var("GP_ARTICLE_SUMMARY_PROBE_INPUT").unwrap();
        let output = std::env::var("GP_ARTICLE_SUMMARY_PROBE_OUTPUT").unwrap();
        let mut items: Vec<Value> = serde_json::from_slice(&std::fs::read(input).unwrap()).unwrap();
        let count = enrich(None, &mut items, &[]).await.unwrap();
        println!("enriched {count} real cached public articles");
        for item in items.iter().filter(|item| {
            item["title"]
                .as_str()
                .is_some_and(|title| title.contains("姚顺雨摇人") || title.contains("官宣再投10亿"))
        }) {
            println!("{}: {}", item["title"], item["article_summary"]);
            assert!(has_summary(item));
        }
        assert!(count >= 2);
        std::fs::write(output, serde_json::to_vec_pretty(&items).unwrap()).unwrap();
    }

    #[tokio::test]
    #[ignore = "explicit live HK news refresh and isolated cache validation"]
    async fn hk_news_refresh_summary_cache_live_probe() {
        let input = std::env::var("GP_ARTICLE_SUMMARY_PROBE_INPUT").unwrap();
        let output = std::env::var("GP_HK_NEWS_REFRESH_PROBE_OUTPUT").unwrap();
        let cached: Vec<Value> = serde_json::from_slice(&std::fs::read(input).unwrap()).unwrap();
        let stocks: Vec<Value> = std::env::var("GP_HK_NEWS_REFRESH_PROBE_STOCKS")
            .ok()
            .map(|raw| serde_json::from_str(&raw).unwrap())
            .unwrap_or_else(|| {
                vec![json!({"code":"00700.HK","name":"腾讯控股","industry":"互联网"})]
            });
        let priority = cached
            .iter()
            .filter(|item| {
                item["stock_codes"]
                    .as_array()
                    .is_some_and(|codes| codes.iter().any(|code| code == "00700.HK"))
            })
            .filter_map(|item| item["url"].as_str())
            .collect::<Vec<_>>();
        let payload = json!({"market":"hk","code":if stocks.len()==1 {stocks[0]["code"].clone()} else {json!("")},
            "seed_codes":stocks.iter().map(|stock| stock["code"].clone()).collect::<Vec<_>>(),
            "summary_priority_urls":priority,"network_proxy_mode":"system","include_us_market_brief":false,"scope_mode":"stock_only"});
        let started = std::time::Instant::now();
        let (items, notes) =
            super::super::fetch_hk_news_items(&stocks, 30, Some(&payload), &cached).await;
        assert!(
            items.iter().all(
                |item| item["stock_codes"].as_array().is_some_and(|codes| codes
                    .iter()
                    .any(|code| stocks.iter().any(|stock| stock["code"] == *code)))
            ),
            "a single-stock news refresh must not fetch unrelated market news"
        );
        let root = std::env::temp_dir().join(format!(
            "gp-hk-news-summary-refresh-{}-{}",
            std::process::id(),
            super::super::epoch_millis()
        ));
        let cache_path = root.join("news/news-cache-hk.json");
        super::super::merge_into_news_cache_for_market(
            &cache_path,
            cached.clone(),
            crate::external_data::Market::HongKong,
        )
        .unwrap();
        let merged = super::super::merge_into_news_cache_for_market(
            &cache_path,
            items,
            crate::external_data::Market::HongKong,
        )
        .unwrap();
        let reloaded = super::super::read_news_cache_items_for_market(
            &cache_path,
            crate::external_data::Market::HongKong,
        )
        .unwrap();
        assert_eq!(reloaded.len(), merged.len());
        for title in [
            "姚顺雨摇人，腾讯正在大举招聘",
            "模型追上第一梯队，腾讯AI还差什么？",
        ] {
            let item = reloaded.iter().find(|item| item["title"] == title).unwrap();
            assert!(
                has_summary(item),
                "live refresh must persist a real summary for {title}"
            );
            assert_eq!(item["article_summary_status"], "available");
            assert_eq!(item["summary"], item["title"]);
        }
        println!(
            "live HK refresh persisted {} summaries from {} articles in {:.2} seconds; scope: {:?}; notes: {:?}",
            reloaded.iter().filter(|item| has_summary(item)).count(),
            reloaded.len(),
            started.elapsed().as_secs_f64(),
            stocks.iter().map(|stock| stock["code"].clone()).collect::<Vec<_>>(),
            notes
        );
        std::fs::write(output, serde_json::to_vec_pretty(&reloaded).unwrap()).unwrap();
    }
}
