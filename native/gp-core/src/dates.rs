use chrono::{Local, NaiveDate};

use crate::error::{CoreError, CoreResult};

pub(crate) fn current_system_date_yyyymmdd() -> String {
    Local::now().date_naive().format("%Y%m%d").to_string()
}

pub(crate) fn parse_date(input: &str) -> CoreResult<NaiveDate> {
    NaiveDate::parse_from_str(input, "%Y%m%d")
        .or_else(|_| NaiveDate::parse_from_str(input, "%Y-%m-%d"))
        .map_err(|_| CoreError::new(format!("Invalid date: {input}")))
}

pub(crate) fn default_start_date() -> String {
    "20200101".to_string()
}

pub(crate) fn default_end_date() -> String {
    current_system_date_yyyymmdd()
}

pub(crate) fn capital_trade_date(value: &str) -> Option<String> {
    let raw = value.trim();
    if raw.len() >= 10
        && raw.as_bytes().get(4) == Some(&b'-')
        && raw.as_bytes().get(7) == Some(&b'-')
    {
        return Some(raw[..10].to_string());
    }
    let digits = raw
        .chars()
        .filter(|ch| ch.is_ascii_digit())
        .collect::<String>();
    if digits.len() < 8 {
        return None;
    }
    NaiveDate::parse_from_str(&digits[..8], "%Y%m%d")
        .ok()
        .map(|date| date.format("%Y-%m-%d").to_string())
}
