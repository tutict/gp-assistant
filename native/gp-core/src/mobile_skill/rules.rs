use crate::mobile_skill::types::{
    MobileStockSkillFinding, MobileStockSkillRequest, MobileStockSourceItem,
};
use crate::numeric::round4;
use crate::text::contains_any;

pub(super) fn mobile_source_to_finding(source: &MobileStockSourceItem) -> MobileStockSkillFinding {
    let source_tier = normalize_source_tier(&source.source_tier);
    let evidence = first_non_empty(&[
        source.evidence.as_str(),
        source.summary.as_str(),
        source.title.as_str(),
    ]);
    let label = classify_mobile_source(&source_tier, &source.title, &source.summary, &evidence);
    let confidence = mobile_confidence(&source_tier, &label, !evidence.is_empty());

    MobileStockSkillFinding {
        label: label.clone(),
        title: truncate_chars(
            &first_non_empty(&[source.title.as_str(), source.summary.as_str(), "未命名信源"]),
            80,
        ),
        summary: mobile_finding_summary(&label, source),
        source_tier,
        source_name: first_non_empty(&[source.source_name.as_str(), "未标注来源"]),
        published_at: source.published_at.clone(),
        evidence: truncate_chars(&evidence, 220),
        confidence,
        risk_note: mobile_risk_note(&label),
    }
}

fn classify_mobile_source(source_tier: &str, title: &str, summary: &str, evidence: &str) -> String {
    if source_tier == "community" || evidence.trim().is_empty() {
        return "unverified".to_string();
    }
    let text = format!("{title} {summary} {evidence}").to_lowercase();
    let positive = contains_any(
        &text,
        &[
            "增长", "预增", "扭亏", "订单", "中标", "签订", "扩产", "回购", "增持", "分红", "盈利",
            "改善", "突破", "获批", "投产", "景气", "涨价", "positive", "beat", "upgrade",
        ],
    );
    let negative = contains_any(
        &text,
        &[
            "下滑",
            "下降",
            "亏损",
            "预亏",
            "减持",
            "处罚",
            "调查",
            "诉讼",
            "仲裁",
            "违约",
            "终止",
            "取消",
            "风险",
            "计提",
            "减值",
            "停产",
            "限产",
            "退市",
            "negative",
            "miss",
            "downgrade",
        ],
    );

    match (positive, negative) {
        (true, false) => "positive".to_string(),
        (false, true) => "negative".to_string(),
        (true, true) => "neutral".to_string(),
        (false, false) => "neutral".to_string(),
    }
}

fn mobile_confidence(source_tier: &str, label: &str, has_evidence: bool) -> f64 {
    if !has_evidence || label == "unverified" {
        return 0.2;
    }
    let base = match source_tier {
        "filing" => 0.9,
        "financial_snapshot" => 0.75,
        "news" => 0.62,
        "research" => 0.55,
        "manual_url" => 0.5,
        "community" => 0.2,
        _ => 0.4,
    };
    round4(base)
}

fn mobile_finding_summary(label: &str, source: &MobileStockSourceItem) -> String {
    let body = first_non_empty(&[
        source.summary.as_str(),
        source.evidence.as_str(),
        source.title.as_str(),
    ]);
    let prefix = match label {
        "positive" => "选股研究视角下偏利好",
        "negative" => "选股研究视角下偏利空",
        "neutral" => "中性信息",
        _ => "待验证线索",
    };
    format!("{prefix}: {}", truncate_chars(&body, 120))
}

fn mobile_risk_note(label: &str) -> String {
    let note = match label {
        "positive" => "需要用后续公告、财报和成交数据验证，不能据此作交易决定。",
        "negative" => "需要确认事项影响范围和持续性，不能单独作为交易依据。",
        "neutral" => "属于信息披露或背景材料，需结合财务和行情继续验证。",
        _ => "来源不足或未被官方披露确认，只能作为待验证线索。",
    };
    format!("{note} 仅供选股研究，不构成投资建议。")
}

pub(super) fn mobile_overall_label(positive: usize, negative: usize, unverified: usize) -> String {
    if positive == 0 && negative == 0 && unverified > 0 {
        return "unverified".to_string();
    }
    if positive > negative {
        "positive".to_string()
    } else if negative > positive {
        "negative".to_string()
    } else {
        "neutral".to_string()
    }
}

pub(super) fn mobile_overview_summary(
    request: &MobileStockSkillRequest,
    positive: usize,
    negative: usize,
    neutral: usize,
    unverified: usize,
) -> String {
    let target = if request.stock_name.trim().is_empty() {
        request.stock_code.trim()
    } else {
        request.stock_name.trim()
    };
    if positive + negative + neutral + unverified == 0 {
        return format!(
            "{target} 未找到可靠信源，暂不能形成利好利空判断。仅供选股研究，不构成投资建议。"
        );
    }
    format!(
        "{target} 当前命中利好 {positive} 条、利空 {negative} 条、中性 {neutral} 条、待验证 {unverified} 条。仅供选股研究，不构成投资建议。"
    )
}

pub(super) fn sort_findings(findings: &mut [MobileStockSkillFinding]) {
    findings.sort_by(|left, right| {
        right
            .confidence
            .total_cmp(&left.confidence)
            .then_with(|| right.published_at.cmp(&left.published_at))
    });
}

pub(super) fn normalize_source_tier(value: &str) -> String {
    let normalized = value.trim().to_lowercase();
    match normalized.as_str() {
        "filing" | "financial_snapshot" | "news" | "research" | "community" | "manual_url" => {
            normalized
        }
        "tdx" | "f10" => "financial_snapshot".to_string(),
        "cninfo" | "notice" | "announcement" => "filing".to_string(),
        "" => "manual_url".to_string(),
        _ => normalized,
    }
}

fn first_non_empty(values: &[&str]) -> String {
    values
        .iter()
        .find_map(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
        .unwrap_or_default()
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    let mut result = String::new();
    for (index, ch) in value.chars().enumerate() {
        if index >= max_chars {
            result.push_str("...");
            break;
        }
        result.push(ch);
    }
    result
}
