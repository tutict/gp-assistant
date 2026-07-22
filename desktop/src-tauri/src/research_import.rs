use crate::research;
use base64::{engine::general_purpose, Engine as _};
use futures::StreamExt;
use lopdf::{Document, LoadOptions};
use reqwest::{
    header::{CONTENT_LENGTH, CONTENT_TYPE, LOCATION},
    redirect::Policy,
    Url,
};
use scraper::{Html, Selector};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    str::FromStr,
    time::Duration,
};

const MAX_URL_BYTES: usize = 5 * 1024 * 1024;
const MAX_PDF_BYTES: usize = 25 * 1024 * 1024;
const MAX_PDF_PAGES: usize = 500;
const MAX_PDF_OBJECT_BYTES: usize = 8 * 1024 * 1024;
const MAX_PDF_PAGE_BYTES: usize = 2 * 1024 * 1024;
const MAX_REDIRECTS: usize = 5;

pub(crate) async fn import_url(app: &tauri::AppHandle, payload: &Value) -> Result<Value, String> {
    let raw_url = payload
        .get("url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "url is required".to_string())?;
    let mut url = Url::parse(raw_url).map_err(|error| format!("invalid URL: {error}"))?;

    for redirect_index in 0..=MAX_REDIRECTS {
        let addresses = resolve_public_https_url(&url).await?;
        let host = url
            .host_str()
            .ok_or_else(|| "URL host is required".to_string())?;
        let client = reqwest::Client::builder()
            .redirect(Policy::none())
            .no_proxy()
            .resolve_to_addrs(host, &addresses)
            .timeout(Duration::from_secs(20))
            .user_agent("GuXuanYou/0.4 research-import")
            .build()
            .map_err(|error| format!("failed to build secure URL client: {error}"))?;
        let response = client
            .get(url.clone())
            .send()
            .await
            .map_err(|error| format!("URL import request failed: {error}"))?;

        if response.status().is_redirection() {
            if redirect_index == MAX_REDIRECTS {
                return Err("URL import exceeded the redirect limit".to_string());
            }
            let location = response
                .headers()
                .get(LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or_else(|| "URL redirect is missing a valid Location header".to_string())?;
            url = url
                .join(location)
                .map_err(|error| format!("invalid redirect URL: {error}"))?;
            continue;
        }
        if !response.status().is_success() {
            return Err(format!("URL import returned HTTP {}", response.status()));
        }
        if response
            .headers()
            .get(CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<usize>().ok())
            .is_some_and(|length| length > MAX_PDF_BYTES)
        {
            return Err("URL import exceeds the 25 MB safety limit".to_string());
        }
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase();
        let byte_limit = if content_type.contains("pdf") {
            MAX_PDF_BYTES
        } else {
            MAX_URL_BYTES
        };
        let bytes = read_limited_body(response, byte_limit).await?;
        if content_type.contains("pdf") || bytes.starts_with(b"%PDF-") {
            return import_pdf_bytes(app, payload, &bytes, Some(url.as_str()));
        }
        let html = String::from_utf8(bytes)
            .map_err(|_| "URL import only supports UTF-8 HTML/text or PDF".to_string())?;
        let (page_title, content) = extract_html_text(&html)?;
        let title = payload
            .get("title")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(&page_title);
        let document_id = format!("url-{}", sha256_hex(url.as_str().as_bytes()));
        let store = research::open_app_store(app)?;
        let result = store.ingest_documents(&[json!({
            "document_id": document_id,
            "title": title,
            "content": content,
            "source_tier": "research_report",
            "source_name": payload.get("source_name").and_then(Value::as_str).unwrap_or("用户导入 URL"),
            "url": url.as_str(),
            "stock_codes": payload.get("stock_codes").cloned().unwrap_or_else(|| json!([])),
            "user_imported": true,
            "pinned": true,
            "metadata": {"import_kind": "url"}
        })])?;
        return Ok(json!({
            "kind": "url",
            "final_url": url.as_str(),
            "title": title,
            "imported": result
        }));
    }
    Err("URL import failed before receiving a response".to_string())
}

pub(crate) fn import_pdf(app: &tauri::AppHandle, payload: &Value) -> Result<Value, String> {
    let encoded = payload
        .get("bytes_base64")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "bytes_base64 is required".to_string())?;
    let bytes = general_purpose::STANDARD
        .decode(encoded)
        .map_err(|error| format!("invalid PDF base64 payload: {error}"))?;
    import_pdf_bytes(app, payload, &bytes, None)
}

fn import_pdf_bytes(
    app: &tauri::AppHandle,
    payload: &Value,
    bytes: &[u8],
    source_url: Option<&str>,
) -> Result<Value, String> {
    let documents = extract_pdf_documents(payload, bytes, source_url)?;
    let page_count = documents.len();
    let store = research::open_app_store(app)?;
    let imported = store.ingest_documents(&documents)?;
    Ok(json!({
        "kind": "pdf",
        "page_count": page_count,
        "imported": imported,
        "ocr_required": false
    }))
}

fn extract_pdf_documents(
    payload: &Value,
    bytes: &[u8],
    source_url: Option<&str>,
) -> Result<Vec<Value>, String> {
    if bytes.len() > MAX_PDF_BYTES {
        return Err("PDF exceeds the 25 MB safety limit".to_string());
    }
    let options = LoadOptions::with_max_decompressed_size(MAX_PDF_OBJECT_BYTES);
    let document = Document::load_mem_with_options(bytes, options)
        .map_err(|error| format!("failed to open PDF safely: {error}"))?;
    let pages = document.get_pages();
    if pages.len() > MAX_PDF_PAGES {
        return Err(format!(
            "PDF has {} pages; the limit is {MAX_PDF_PAGES}",
            pages.len()
        ));
    }
    let title = payload
        .get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("用户导入 PDF");
    let pdf_hash = sha256_hex(bytes);
    let mut extracted = Vec::new();
    for page_number in pages.keys().copied() {
        let text = document
            .extract_text_with_limit(&[page_number], MAX_PDF_PAGE_BYTES)
            .map_err(|error| format!("failed to extract PDF page {page_number}: {error}"))?;
        if text.trim().is_empty() {
            continue;
        }
        extracted.push(json!({
            "document_id": format!("pdf-{pdf_hash}-page-{page_number}"),
            "title": format!("{title} · 第 {page_number} 页"),
            "content": text,
            "page_number": page_number,
            "source_tier": "research_report",
            "source_name": payload.get("source_name").and_then(Value::as_str).unwrap_or("用户导入 PDF"),
            "url": source_url,
            "stock_codes": payload.get("stock_codes").cloned().unwrap_or_else(|| json!([])),
            "user_imported": true,
            "pinned": true,
            "metadata": {"import_kind": "pdf", "pdf_hash": pdf_hash}
        }));
    }
    let extracted_chars = extracted
        .iter()
        .filter_map(|item| item.get("content").and_then(Value::as_str))
        .map(str::chars)
        .map(Iterator::count)
        .sum::<usize>();
    if extracted_chars < 20 {
        return Err(
            "PDF appears to be scanned or contains no extractable text; OCR is required."
                .to_string(),
        );
    }
    Ok(extracted)
}

async fn resolve_public_https_url(url: &Url) -> Result<Vec<SocketAddr>, String> {
    validate_url_shape(url)?;
    let host = url
        .host_str()
        .ok_or_else(|| "URL host is required".to_string())?;
    let port = url.port_or_known_default().unwrap_or(443);
    let ips = if let Ok(ip) = IpAddr::from_str(host) {
        vec![ip]
    } else {
        tokio::net::lookup_host((host, port))
            .await
            .map_err(|error| format!("failed to resolve URL host: {error}"))?
            .map(|address| address.ip())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
    };
    validate_public_ips(&ips)?;
    Ok(ips
        .into_iter()
        .map(|ip| SocketAddr::new(ip, port))
        .collect())
}

fn validate_url_shape(url: &Url) -> Result<(), String> {
    if url.scheme() != "https" {
        return Err("only public HTTPS URLs can be imported".to_string());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("URL credentials are not allowed".to_string());
    }
    if url.host_str().is_none() {
        return Err("URL host is required".to_string());
    }
    Ok(())
}

fn validate_public_ips(ips: &[IpAddr]) -> Result<(), String> {
    if ips.is_empty() {
        return Err("URL host did not resolve to an address".to_string());
    }
    if ips.iter().any(|ip| !is_public_ip(*ip)) {
        return Err(
            "URL resolves to a loopback, private, link-local, or reserved address".to_string(),
        );
    }
    Ok(())
}

fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_public_ipv4(ip),
        IpAddr::V6(ip) => ip
            .to_ipv4_mapped()
            .map(is_public_ipv4)
            .unwrap_or_else(|| is_public_ipv6(ip)),
    }
}

fn is_public_ipv4(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    !(ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_multicast()
        || octets[0] == 0
        || octets[0] >= 224
        || (octets[0] == 100 && (64..=127).contains(&octets[1]))
        || (octets[0] == 192 && octets[1] == 0 && octets[2] == 0)
        || (octets[0] == 198 && (octets[1] == 18 || octets[1] == 19)))
}

fn is_public_ipv6(ip: Ipv6Addr) -> bool {
    let segments = ip.segments();
    let global_unicast = (segments[0] & 0xe000) == 0x2000;
    let documentation = segments[0] == 0x2001 && segments[1] == 0x0db8;
    let benchmarking = segments[0] == 0x2001 && segments[1] == 0x0002 && segments[2] == 0;
    let orchid = segments[0] == 0x2001 && matches!(segments[1] & 0xfff0, 0x0010 | 0x0020);
    let transition = (segments[0] == 0x2001 && segments[1] == 0) || segments[0] == 0x2002;
    global_unicast && !documentation && !benchmarking && !orchid && !transition
}

async fn read_limited_body(response: reqwest::Response, limit: usize) -> Result<Vec<u8>, String> {
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| format!("failed to read URL response: {error}"))?;
        if bytes.len().saturating_add(chunk.len()) > limit {
            return Err(format!("URL response exceeds the {} byte limit", limit));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn extract_html_text(html: &str) -> Result<(String, String), String> {
    let document = Html::parse_document(html);
    let title_selector =
        Selector::parse("title").map_err(|error| format!("invalid title selector: {error}"))?;
    let content_selector = Selector::parse("article, main, h1, h2, h3, p, li, td, blockquote")
        .map_err(|error| format!("invalid content selector: {error}"))?;
    let title = document
        .select(&title_selector)
        .next()
        .map(|element| element.text().collect::<Vec<_>>().join(" "))
        .map(|value| collapse_whitespace(&value))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "用户导入网页".to_string());
    let mut seen = HashSet::new();
    let content = document
        .select(&content_selector)
        .map(|element| collapse_whitespace(&element.text().collect::<Vec<_>>().join(" ")))
        .filter(|value| value.chars().count() >= 8)
        .filter(|value| seen.insert(value.clone()))
        .collect::<Vec<_>>()
        .join("\n");
    if content.chars().count() < 20 {
        return Err("URL page contains too little extractable article text".to_string());
    }
    Ok((title, content))
}

fn collapse_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_ssrf_targets_and_url_credentials() {
        assert!(!is_public_ip(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))));
        assert!(!is_public_ip(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2))));
        assert!(!is_public_ip(IpAddr::V4(Ipv4Addr::new(169, 254, 1, 1))));
        assert!(!is_public_ip(IpAddr::V6(Ipv6Addr::LOCALHOST)));
        assert!(!is_public_ip(IpAddr::V6(
            "fec0::1".parse().expect("site-local IPv6 should parse")
        )));
        assert!(!is_public_ip(IpAddr::V6(
            "2001:db8::1"
                .parse()
                .expect("documentation IPv6 should parse")
        )));
        assert!(is_public_ip(IpAddr::V6(
            "2606:4700:4700::1111"
                .parse()
                .expect("public IPv6 should parse")
        )));
        assert!(!is_public_ip(IpAddr::V6(
            Ipv4Addr::new(127, 0, 0, 1).to_ipv6_mapped()
        )));
        assert!(is_public_ip(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))));
        assert!(validate_url_shape(&Url::parse("https://example.com/a").unwrap()).is_ok());
        assert!(validate_url_shape(&Url::parse("http://example.com/a").unwrap()).is_err());
        assert!(
            validate_url_shape(&Url::parse("https://user:pass@example.com/a").unwrap()).is_err()
        );
    }

    #[test]
    fn extracts_structured_html_without_scripts() {
        let html = "<html><head><title>订单 公告</title><script>ignore me</script></head><body><main><h1>储能订单</h1><p>公司公告订单交付按计划稳定推进。</p></main></body></html>";
        let (title, content) = extract_html_text(html).unwrap();
        assert_eq!(title, "订单 公告");
        assert!(content.contains("订单交付"));
        assert!(!content.contains("ignore me"));
    }

    #[test]
    fn enforces_pdf_size_before_parsing() {
        let payload = json!({"title": "oversized"});
        let bytes = vec![0u8; MAX_PDF_BYTES + 1];
        assert!(extract_pdf_documents(&payload, &bytes, None)
            .unwrap_err()
            .contains("25 MB"));
    }
}
