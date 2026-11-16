use serde_json::Value;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;
use stock_optimizer_core as gp_core;

pub(crate) const MARKET_REFRESH_HISTORY_LOOKBACK_DAYS: u128 = 500;

pub(crate) fn cache_epoch_ms(value: Option<&Value>) -> Option<u128> {
    value.and_then(|value| {
        value.as_u64().map(u128::from).or_else(|| {
            value.as_str().and_then(|text| {
                let text = text.trim();
                text.parse::<u128>()
                    .ok()
                    .or_else(|| parse_cache_datetime_epoch_ms(text))
            })
        })
    })
}

pub(crate) fn parse_cache_datetime_epoch_ms(text: &str) -> Option<u128> {
    let date = text.get(0..10)?;
    let year = date.get(0..4)?.parse::<i32>().ok()?;
    let month = date.get(5..7)?.parse::<u32>().ok()?;
    let day = date.get(8..10)?.parse::<u32>().ok()?;
    if date.get(4..5)? != "-" || date.get(7..8)? != "-" {
        return None;
    }
    let mut hour = 0u32;
    let mut minute = 0u32;
    let mut second = 0u32;
    let mut millis = 0u32;
    let mut offset_seconds = 8 * 60 * 60;
    if text.len() >= 19 && matches!(text.as_bytes().get(10), Some(b'T' | b't' | b' ')) {
        hour = text.get(11..13)?.parse::<u32>().ok()?;
        minute = text.get(14..16)?.parse::<u32>().ok()?;
        second = text.get(17..19)?.parse::<u32>().ok()?;
        let mut rest = text.get(19..).unwrap_or("");
        if text.get(13..14)? != ":" || text.get(16..17)? != ":" {
            return None;
        }
        if let Some(fraction) = rest.strip_prefix('.') {
            let digits: String = fraction
                .chars()
                .take_while(|ch| ch.is_ascii_digit())
                .take(3)
                .collect();
            if !digits.is_empty() {
                let padded = format!("{digits:0<3}");
                millis = padded.parse::<u32>().ok()?;
            }
            rest = &fraction[digits.len()..];
        }
        if let Some(tz) = rest.strip_prefix('Z').or_else(|| rest.strip_prefix('z')) {
            let _ = tz;
            offset_seconds = 0;
        } else if rest.starts_with('+') || rest.starts_with('-') {
            offset_seconds = parse_timezone_offset_seconds(rest)?;
        }
    }
    if month == 0 || month > 12 || day == 0 || day > 31 || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let days = days_from_civil_epoch(year, month, day)?;
    let epoch_seconds =
        days * 86_400 + i128::from(hour * 3600 + minute * 60 + second) - i128::from(offset_seconds);
    if epoch_seconds < 0 {
        return None;
    }
    let epoch_ms = epoch_seconds
        .checked_mul(1000)?
        .checked_add(i128::from(millis))?;
    u128::try_from(epoch_ms).ok()
}

pub(crate) fn parse_timezone_offset_seconds(text: &str) -> Option<i32> {
    let sign = if text.starts_with('-') { -1 } else { 1 };
    let body = text.get(1..)?;
    let hour = body.get(0..2)?.parse::<i32>().ok()?;
    let minute = if body.get(2..3) == Some(":") {
        body.get(3..5).unwrap_or("00").parse::<i32>().ok()?
    } else if body.len() >= 4 {
        body.get(2..4)?.parse::<i32>().ok()?
    } else {
        0
    };
    if hour > 23 || minute > 59 {
        return None;
    }
    Some(sign * (hour * 3600 + minute * 60))
}

pub(crate) fn local_yyyymmdd_from_epoch_ms(epoch_ms: u128) -> Option<String> {
    let seconds = i128::try_from(epoch_ms / 1000).ok()? + 8 * 60 * 60;
    let days = seconds.div_euclid(86_400);
    let (year, month, day) = civil_from_days(days)?;
    Some(format!("{year:04}{month:02}{day:02}"))
}

pub(crate) fn market_quote_cache_stale(
    generated_at_epoch_ms: Option<u128>,
    now_epoch_ms: u128,
    quote_coverage_ratio: Option<f64>,
) -> bool {
    market_quote_cache_stale_with_calendar(
        generated_at_epoch_ms,
        now_epoch_ms,
        quote_coverage_ratio,
        None,
    )
}

pub(crate) fn market_quote_cache_stale_with_calendar(
    generated_at_epoch_ms: Option<u128>,
    now_epoch_ms: u128,
    quote_coverage_ratio: Option<f64>,
    calendar: Option<&Value>,
) -> bool {
    let Some(quote_date) = generated_at_epoch_ms
        .and_then(|epoch| expected_market_quote_date_with_calendar(epoch, calendar))
    else {
        return true;
    };
    let Some(expected_date) = expected_market_quote_date_with_calendar(now_epoch_ms, calendar)
    else {
        return true;
    };
    let coverage_is_complete = quote_coverage_ratio.is_some_and(|ratio| {
        ratio.is_finite() && ratio + f64::EPSILON >= gp_core::MIN_MARKET_BREADTH_COVERAGE
    });
    quote_date != expected_date || !coverage_is_complete
}

pub(crate) fn expected_market_quote_date_from_epoch_ms(epoch_ms: u128) -> Option<String> {
    let seconds = i128::try_from(epoch_ms / 1000).ok()? + 8 * 60 * 60;
    let mut days = seconds.div_euclid(86_400);
    let seconds_in_day = seconds.rem_euclid(86_400);
    let minutes = seconds_in_day / 60;
    let weekday = weekday_from_days_since_epoch(days);
    if weekday == 6 {
        days -= 1;
    } else if weekday == 0 {
        days -= 2;
    } else if minutes < 9 * 60 + 30 {
        days -= 1;
    }
    while matches!(weekday_from_days_since_epoch(days), 0 | 6) {
        days -= 1;
    }
    let (year, month, day) = civil_from_days(days)?;
    Some(format!("{year:04}{month:02}{day:02}"))
}

pub(crate) fn expected_market_quote_date_with_calendar(
    epoch_ms: u128,
    calendar: Option<&Value>,
) -> Option<String> {
    let date = local_yyyymmdd_from_epoch_ms(epoch_ms)?;
    if let Some(calendar) =
        calendar.filter(|calendar| calendar.get("checked").and_then(Value::as_bool) == Some(true))
    {
        let start = calendar.get("coverage_start").and_then(Value::as_str);
        let end = calendar.get("coverage_end").and_then(Value::as_str);
        if start
            .zip(end)
            .is_some_and(|(start, end)| start <= date.as_str() && date.as_str() <= end)
        {
            let minutes = ((epoch_ms / 1000 + 8 * 60 * 60) % 86400) / 60;
            let include_today = minutes >= 9 * 60 + 30;
            if let Some(expected) = calendar
                .get("trading_dates")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .filter(|candidate| {
                    if include_today {
                        *candidate <= date.as_str()
                    } else {
                        *candidate < date.as_str()
                    }
                })
                .max()
            {
                return Some(expected.to_string());
            }
        }
    }
    expected_market_quote_date_from_epoch_ms(epoch_ms)
}

pub(crate) fn weekday_from_days_since_epoch(days_since_epoch: i128) -> i128 {
    (days_since_epoch + 4).rem_euclid(7)
}

pub(crate) fn days_from_civil_epoch(year: i32, month: u32, day: u32) -> Option<i128> {
    let month_i = i128::from(month);
    let day_i = i128::from(day);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let y = i128::from(year) - i128::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = month_i + if month > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2).div_euclid(5) + day_i - 1;
    let doe = yoe * 365 + yoe.div_euclid(4) - yoe.div_euclid(100) + doy;
    Some(era * 146_097 + doe - 719_468)
}

pub(crate) fn civil_from_days(days_since_epoch: i128) -> Option<(i32, u32, u32)> {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 }.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096).div_euclid(365);
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2).div_euclid(153);
    let day = doy - (153 * mp + 2).div_euclid(5) + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if month <= 2 { 1 } else { 0 };
    Some((
        i32::try_from(year).ok()?,
        u32::try_from(month).ok()?,
        u32::try_from(day).ok()?,
    ))
}

pub(crate) fn market_refresh_history_start_date() -> String {
    local_yyyymmdd_from_epoch_ms(
        epoch_millis().saturating_sub(
            MARKET_REFRESH_HISTORY_LOOKBACK_DAYS.saturating_mul(24 * 60 * 60 * 1_000),
        ),
    )
    .unwrap_or_else(|| "20200101".to_string())
}

pub(crate) fn window_days(start_date: &str, end_date: &str) -> i64 {
    let start = compact_date_key(start_date)
        .and_then(|key| days_from_civil_key(&key))
        .unwrap_or(0);
    let end = compact_date_key(end_date)
        .and_then(|key| days_from_civil_key(&key))
        .unwrap_or(start);
    (end - start + 1).max(1)
}

pub(crate) fn fallback_today_date() -> String {
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| (duration.as_secs() / 86_400) as i64)
        .unwrap_or(0);
    civil_date_from_days(days)
}

pub(crate) fn days_from_civil_key(key: &str) -> Option<i64> {
    if key.len() != 8 {
        return None;
    }
    let year = key.get(0..4)?.parse::<i32>().ok()?;
    let month = key.get(4..6)?.parse::<u32>().ok()?;
    let day = key.get(6..8)?.parse::<u32>().ok()?;
    Some(days_from_civil(year, month, day))
}

pub(crate) fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let y = year as i64 - if month <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = month as i64 + if month > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub(crate) fn civil_date_from_days(days_since_epoch: i64) -> String {
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

pub(crate) fn current_calendar_year_utc() -> i32 {
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| (duration.as_secs() / 86_400) as i64)
        .unwrap_or(0);
    civil_year_from_days(days)
}

pub(crate) fn civil_year_from_days(days_since_epoch: i64) -> i32 {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let month = mp + if mp < 10 { 3 } else { -9 };
    (y + if month <= 2 { 1 } else { 0 }) as i32
}

pub(crate) fn normalize_history_date(raw: &str) -> Option<String> {
    let key = compact_date_key(raw)?;
    Some(format!("{}-{}-{}", &key[..4], &key[4..6], &key[6..8]))
}

pub(crate) fn compact_date_key(raw: &str) -> Option<String> {
    let digits = raw
        .chars()
        .filter(|ch| ch.is_ascii_digit())
        .take(8)
        .collect::<String>();
    if digits.len() == 8 {
        Some(digits)
    } else {
        None
    }
}

pub(crate) fn epoch_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}
