use std::collections::HashSet;

pub(crate) fn adaptive_benchmark_codes() -> [&'static str; 3] {
    ["000001.SH", "399001.SZ", "399006.SZ"]
}

pub(crate) fn build_candidate_codes(
    seed_codes: &[String],
    scan_candidates: bool,
    max_candidates: usize,
) -> Vec<String> {
    // The candidate list defines pagination windows. It MUST be stable across
    // the repeated invocations of a single full rebuild, otherwise `batch_start`
    // from the frontend drifts as the in-memory seed grows and codes get skipped
    // or re-fetched. When scanning, always emit the deterministic scan order
    // first and append only the seed-only codes (outside the scan ranges) at the
    // end. Seed DATA is merged/preserved separately, so ordering here only
    // decides which codes each batch fetches.
    let mut candidate_codes = Vec::new();
    if scan_candidates {
        append_tencent_candidate_codes(&mut candidate_codes);
    }
    candidate_codes.extend(seed_codes.iter().cloned());
    dedupe_stock_codes(&mut candidate_codes);
    if candidate_codes.len() > max_candidates {
        candidate_codes.truncate(max_candidates);
    }
    candidate_codes
}

pub(crate) fn append_tencent_candidate_codes(codes: &mut Vec<String>) {
    append_interleaved_ranges(
        codes,
        &[
            ("SZ", 1, 3999),
            ("SH", 600000, 605999),
            ("SZ", 300000, 301999),
            ("SH", 688000, 689999),
            ("BJ", 920000, 920999),
        ],
    );
}

pub(crate) fn append_interleaved_ranges(codes: &mut Vec<String>, ranges: &[(&str, u32, u32)]) {
    let max_len = ranges
        .iter()
        .map(|(_, start, end)| end.saturating_sub(*start))
        .max()
        .unwrap_or(0);
    for offset in 0..=max_len {
        for (market, start, end) in ranges {
            let value = start.saturating_add(offset);
            if value <= *end {
                codes.push(format!("{value:06}.{market}"));
            }
        }
    }
}

pub(crate) fn dedupe_stock_codes(codes: &mut Vec<String>) {
    let mut seen = HashSet::new();
    codes.retain(|code| seen.insert(code.clone()));
}

pub(crate) fn tencent_symbol(code: &str) -> Option<String> {
    let normalized = normalize_stock_code(code)?;
    let (digits, market) = normalized.split_once('.')?;
    match market {
        "HK" => Some(format!("hk{digits}")),
        "SH" => Some(format!("sh{digits}")),
        "BJ" => Some(format!("bj{digits}")),
        "SZ" => Some(format!("sz{digits}")),
        _ => None,
    }
}

pub(crate) fn normalize_stock_code(value: &str) -> Option<String> {
    let raw = value.trim().to_ascii_uppercase();
    if raw.is_empty() {
        return None;
    }
    if let Some(digits) = raw.strip_prefix("SH").filter(|digits| valid_digits(digits)) {
        return Some(format!("{digits}.SH"));
    }
    if let Some(digits) = raw.strip_prefix("SZ").filter(|digits| valid_digits(digits)) {
        return Some(format!("{digits}.SZ"));
    }
    if let Some(digits) = raw.strip_prefix("BJ").filter(|digits| valid_digits(digits)) {
        return Some(format!("{digits}.BJ"));
    }
    if let Some(digits) = raw
        .strip_prefix("HK")
        .filter(|digits| valid_hk_digits(digits))
    {
        return Some(format!("{digits}.HK"));
    }
    if let Some((digits, market)) = raw.split_once('.') {
        if (market == "HK" && valid_hk_digits(digits))
            || (valid_digits(digits) && matches!(market, "SH" | "SZ" | "BJ"))
        {
            return Some(format!("{digits}.{market}"));
        }
    }
    if raw.ends_with(".HK") || raw.starts_with("HK") {
        return None;
    }
    let digits: String = raw
        .chars()
        .filter(|ch| ch.is_ascii_digit())
        .take(6)
        .collect();
    if !valid_digits(&digits) {
        return None;
    }
    Some(format!("{}.{}", digits, infer_market(&digits)))
}

pub(crate) fn valid_digits(value: &str) -> bool {
    value.len() == 6 && value.chars().all(|ch| ch.is_ascii_digit())
}

pub(crate) fn valid_hk_digits(value: &str) -> bool {
    value.len() == 5 && value.chars().all(|ch| ch.is_ascii_digit())
}

pub(crate) fn infer_market(digits: &str) -> &'static str {
    if digits.starts_with('6') || digits.starts_with('9') || digits.starts_with('5') {
        "SH"
    } else if digits.starts_with('4') || digits.starts_with('8') {
        "BJ"
    } else {
        "SZ"
    }
}

pub(crate) fn board_label(code: &str) -> &'static str {
    let digits = &code[..6];
    if code.ends_with(".BJ") {
        "\u{5317}\u{4ea4}\u{6240}"
    } else if digits.starts_with("688") {
        "\u{79d1}\u{521b}\u{677f}"
    } else if digits.starts_with("300") || digits.starts_with("301") {
        "\u{521b}\u{4e1a}\u{677f}"
    } else if code.ends_with(".SH") {
        "\u{6caa}\u{5e02}A\u{80a1}"
    } else {
        "\u{6df1}\u{5e02}A\u{80a1}"
    }
}

pub(crate) fn ths_market_code(code: &str) -> Option<u16> {
    let normalized = normalize_stock_code(code)?;
    let digits = normalized.get(..6)?;
    if digits.starts_with("000")
        || digits.starts_with("001")
        || digits.starts_with("002")
        || digits.starts_with("003")
        || digits.starts_with("300")
        || digits.starts_with("301")
    {
        Some(33)
    } else if digits.starts_with("600")
        || digits.starts_with("601")
        || digits.starts_with("603")
        || digits.starts_with("605")
        || digits.starts_with("688")
    {
        Some(17)
    } else if digits.starts_with("920") || digits.starts_with("8") || digits.starts_with("4") {
        Some(151)
    } else {
        None
    }
}

pub(crate) fn eastmoney_market_code(code: &str) -> Option<u8> {
    if code.ends_with(".SH") {
        Some(1)
    } else if code.ends_with(".SZ") || code.ends_with(".BJ") {
        Some(0)
    } else {
        None
    }
}
