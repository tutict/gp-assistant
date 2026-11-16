use crate::mobile_skill::rules::{
    mobile_overall_label, mobile_overview_summary, mobile_source_to_finding, normalize_source_tier,
    sort_findings,
};
use crate::mobile_skill::types::{
    MobileStockSkillOverview, MobileStockSkillRequest, MobileStockSkillResult,
};

pub fn run_mobile_stock_skill(request: &MobileStockSkillRequest) -> MobileStockSkillResult {
    let mut positive_factors = Vec::new();
    let mut negative_factors = Vec::new();
    let mut neutral_information = Vec::new();
    let mut unverified_leads = Vec::new();
    let mut notes = vec![
        "手机端股票分析 Skill 已按结构化信源生成结论。".to_string(),
        "仅供选股研究，不构成投资建议。".to_string(),
    ];

    for source in request.sources.iter().take(80) {
        let finding = mobile_source_to_finding(source);
        match finding.label.as_str() {
            "positive" => positive_factors.push(finding),
            "negative" => negative_factors.push(finding),
            "neutral" => neutral_information.push(finding),
            _ => unverified_leads.push(finding),
        }
    }

    sort_findings(&mut positive_factors);
    sort_findings(&mut negative_factors);
    sort_findings(&mut neutral_information);
    sort_findings(&mut unverified_leads);

    if request.sources.is_empty() {
        notes.push("未找到可靠信源；请先接入 CNINFO、通达信 F10 或公开新闻 URL。".to_string());
    }
    if request
        .sources
        .iter()
        .any(|item| normalize_source_tier(&item.source_tier) == "community")
    {
        notes.push("社区来源只作为待验证线索，不能单独形成利好或利空事实。".to_string());
    }

    let overview = MobileStockSkillOverview {
        stock_code: request.stock_code.trim().to_uppercase(),
        stock_name: request.stock_name.trim().to_string(),
        overall_label: mobile_overall_label(
            positive_factors.len(),
            negative_factors.len(),
            unverified_leads.len(),
        ),
        summary: mobile_overview_summary(
            request,
            positive_factors.len(),
            negative_factors.len(),
            neutral_information.len(),
            unverified_leads.len(),
        ),
        positive_count: positive_factors.len(),
        negative_count: negative_factors.len(),
        neutral_count: neutral_information.len(),
        unverified_count: unverified_leads.len(),
    };

    MobileStockSkillResult {
        overview,
        positive_factors,
        negative_factors,
        neutral_information,
        unverified_leads,
        notes,
    }
}
