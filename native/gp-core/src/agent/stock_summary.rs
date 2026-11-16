use crate::agent::types::{AgentAnswerSection, AgentResponse};
use crate::observation::financial::{format_number, format_percent};
use crate::rates::ratio_percentage_points;
use serde_json::Value;

pub(super) fn agent_beginner_stock_sections(
    response: &AgentResponse,
) -> Option<Vec<AgentAnswerSection>> {
    let data = response.data.as_ref()?;
    let stock = data.get("stock")?;
    let name = json_string(stock, "name").unwrap_or_else(|| "这只股票".to_string());
    let code = json_string(stock, "code").unwrap_or_default();
    let price = json_f64(stock, "price").map(format_price_text);
    let change = json_f64(stock, "change_pct").map(format_signed_percent_text);
    let pe = json_f64(stock, "pe").map(|value| format_number(value));
    let pb = json_f64(stock, "pb").map(|value| format_number(value));
    let roe = json_f64(stock, "roe").map(format_percent);
    let market_cap =
        json_f64(stock, "market_cap_billion").map(|value| format!("{}亿", format_number(value)));

    let trend_signal = data.get("trend").and_then(|trend| trend.get("signal"));
    let status = trend_signal.and_then(|signal| json_string(signal, "status"));
    let signal_type = trend_signal.and_then(|signal| json_string(signal, "signal_type"));
    let swl = trend_signal.and_then(|signal| json_f64(signal, "swl"));
    let sws = trend_signal.and_then(|signal| json_f64(signal, "sws"));
    let support = trend_signal.and_then(|signal| json_f64(signal, "support"));
    let resistance = trend_signal.and_then(|signal| json_f64(signal, "resistance"));
    let breakout = trend_signal.and_then(|signal| json_f64(signal, "breakout"));
    let reversal = trend_signal.and_then(|signal| json_f64(signal, "reversal"));
    let k = trend_signal.and_then(|signal| json_f64(signal, "k"));
    let d = trend_signal.and_then(|signal| json_f64(signal, "d"));
    let j = trend_signal.and_then(|signal| json_f64(signal, "j"));
    let golden_cross = trend_signal
        .and_then(|signal| signal.get("golden_cross"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let dead_cross = trend_signal
        .and_then(|signal| signal.get("dead_cross"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let quant_score = trend_signal.and_then(|signal| json_i64(signal, "quant_score"));
    let quant_score_max = trend_signal
        .and_then(|signal| json_i64(signal, "quant_score_max"))
        .unwrap_or(90);
    let risk_flags = trend_signal
        .and_then(|signal| signal.get("risk_flags"))
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).collect::<Vec<_>>())
        .unwrap_or_default();

    let capital = data.get("capital_evidence");
    let capital_score = capital.and_then(|value| json_f64(value, "composite_score"));
    let capital_confidence = capital.and_then(|value| json_string(value, "confidence"));
    let institution_summary = agent_capital_item_summary(capital, "institution_seat");
    let fund_summary = agent_capital_item_summary(capital, "fund_flow");
    let technical_summary = agent_capital_item_summary(capital, "technical_inference");

    let mut conclusion = Vec::new();
    let quote = [
        price.map(|value| format!("最新价 {value}")),
        change.map(|value| format!("涨跌幅 {value}")),
        pe.map(|value| format!("市盈率 {value}")),
        pb.map(|value| format!("市净率 {value}")),
        roe.map(|value| format!("ROE {value}")),
        market_cap.map(|value| format!("总市值 {value}")),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join("，");
    if quote.is_empty() {
        conclusion.push(format!(
            "{}{}：已找到本地个股数据，但行情字段不完整。",
            name,
            code_suffix(&code)
        ));
    } else {
        conclusion.push(format!(
            "{}{}：{}。这些是先看公司价格、估值和盈利质量的基础信息。",
            name,
            code_suffix(&code),
            quote
        ));
    }
    conclusion.push(agent_beginner_trend_sentence(
        status.as_deref(),
        signal_type.as_deref(),
        swl,
        sws,
        quant_score,
        quant_score_max,
    ));
    conclusion.push("简单理解：现在更像是“可继续观察”的状态，不等于马上买入；新手应先看趋势有没有延续，再看跌破哪个位置说明判断失效。".to_string());

    let mut evidence = Vec::new();
    if let Some(score) = capital_score {
        evidence.push(format!("资金证据综合分 {}，置信度{}。分数越高，说明本地资金、机构、技术证据越一致；50 分附近代表偏中性，不能单独当买入理由。", format_number(score), capital_confidence.unwrap_or_else(|| "未知".to_string())));
    } else {
        evidence.push("资金证据暂不完整，所以不要只根据这一轮 Agent 结果判断强弱。".to_string());
    }
    if let Some(text) = institution_summary {
        evidence.push(format!("机构席位：{text}。新手可以把它理解为“大资金是否留下过明显痕迹”的线索，但它不是实时买卖信号。"));
    }
    if let Some(text) = fund_summary {
        evidence.push(format!(
            "量价资金：{text}。这是用本地成交量和价格估算的资金线索，可靠性低于真实资金流数据。"
        ));
    }
    if let Some(text) = technical_summary {
        evidence.push(format!(
            "技术承接：{text}。意思是价格趋势和指标是否互相支持。"
        ));
    }
    if evidence.len() == 1 {
        evidence.push(agent_data_summary(response));
    }

    let mut levels = Vec::new();
    if let (Some(support), Some(resistance)) = (support, resistance) {
        levels.push(format!("支撑位约 {}、压力位约 {}。支撑位可以理解为“跌到这里附近要小心是否撑不住”，压力位可以理解为“涨到这里附近可能遇到卖压”。", format_price_text(support), format_price_text(resistance)));
    }
    if let Some(breakout) = breakout {
        levels.push(format!(
            "突破观察位约 {}：如果放量站上，才说明上攻更有说服力。",
            format_price_text(breakout)
        ));
    }
    if let Some(reversal) = reversal {
        levels.push(format!(
            "风险观察位约 {}：如果跌破，说明原来的上升判断可能失效。",
            format_price_text(reversal)
        ));
    }
    if let (Some(k), Some(d), Some(j)) = (k, d, j) {
        let cross = if golden_cross {
            "当前出现 KDJ 金叉，短线动能转强的信号更明显。"
        } else if dead_cross {
            "当前出现 KDJ 死叉，短线动能转弱，需要更谨慎。"
        } else {
            "当前没有明确 KDJ 金叉或死叉，说明短线拐点信号还不够明确。"
        };
        levels.push(format!(
            "KDJ：K={}、D={}、J={}。{cross}",
            format_number(k),
            format_number(d),
            format_number(j)
        ));
    }
    if levels.is_empty() {
        levels.push("关键价位和指标暂不完整，建议先补足历史行情后再判断趋势。".to_string());
    }

    let mut risks = Vec::new();
    if risk_flags.is_empty() {
        risks.push("本地指标没有给出明确风险旗标，但这不代表没有风险；它只代表当前规则没有识别到特别异常。".to_string());
    } else {
        risks.push(format!(
            "本地风险旗标：{}。出现风险旗标时，新手应降低仓促决策的倾向。",
            risk_flags.join("、")
        ));
    }
    risks.push("不要把“趋势较好”理解成“必涨”。股价会受大盘、行业、公告、业绩和情绪影响，任何单一指标都可能失效。".to_string());
    risks.push("这不是投资建议，不输出买入、卖出、目标价、仓位或收益承诺。".to_string());

    Some(vec![
        AgentAnswerSection {
            title: "一句话结论".to_string(),
            bullets: conclusion,
        },
        AgentAnswerSection {
            title: "为什么这么判断".to_string(),
            bullets: evidence,
        },
        AgentAnswerSection {
            title: "新手要盯的价位".to_string(),
            bullets: levels,
        },
        AgentAnswerSection {
            title: "风险和下一步".to_string(),
            bullets: risks,
        },
    ])
}

fn agent_beginner_trend_sentence(
    status: Option<&str>,
    signal_type: Option<&str>,
    swl: Option<f64>,
    sws: Option<f64>,
    quant_score: Option<i64>,
    quant_score_max: i64,
) -> String {
    let status_text = match status.unwrap_or_default() {
        "uptrend" => "趋势偏上升",
        "downtrend" => "趋势偏走弱",
        "sideways" => "趋势偏横盘",
        _ => "趋势状态不明确",
    };
    let signal_text = match signal_type.unwrap_or_default() {
        "trend_continuation" => "更像是原有趋势还在延续",
        "breakout" => "更像是尝试突破",
        "reversal" => "更像是可能反转",
        "pullback" => "更像是上涨后的回落观察",
        _ => "暂时没有特别清晰的信号类型",
    };
    let line_text = match (swl, sws) {
        (Some(swl), Some(sws)) if swl > sws => format!(
            "SWL 高于 SWS（{} > {}），可以粗略理解为短中期趋势线偏强。",
            format_price_text(swl),
            format_price_text(sws)
        ),
        (Some(swl), Some(sws)) if swl < sws => format!(
            "SWL 低于 SWS（{} < {}），可以粗略理解为趋势线偏弱。",
            format_price_text(swl),
            format_price_text(sws)
        ),
        (Some(swl), Some(sws)) => format!(
            "SWL 和 SWS 接近（{} / {}），说明趋势强弱还不明显。",
            format_price_text(swl),
            format_price_text(sws)
        ),
        _ => "SWL/SWS 趋势线数据不完整。".to_string(),
    };
    let score_text = quant_score
        .map(|score| format!("本地量化分 {score}/{quant_score_max}。"))
        .unwrap_or_else(|| "本地量化分暂缺。".to_string());
    format!("趋势：{status_text}，{signal_text}。{line_text}{score_text}")
}

fn agent_capital_item_summary(capital: Option<&Value>, category: &str) -> Option<String> {
    let item = capital?
        .get("items")?
        .as_array()?
        .iter()
        .find(|item| json_string(item, "category").as_deref() == Some(category))?;
    if let Some(note) = json_string(item, "note").filter(|text| !text.trim().is_empty()) {
        return Some(note);
    }
    if let Some(score) = json_f64(item, "score") {
        return Some(format!(
            "得分 {}，方向 {}",
            format_number(score),
            json_string(item, "sentiment").unwrap_or_else(|| "未知".to_string())
        ));
    }
    json_string(item, "title")
}

fn json_string(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_string)
}

fn json_f64(value: &Value, key: &str) -> Option<f64> {
    value
        .get(key)
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
}

fn json_i64(value: &Value, key: &str) -> Option<i64> {
    value.get(key).and_then(Value::as_i64)
}

fn code_suffix(code: &str) -> String {
    if code.is_empty() {
        String::new()
    } else {
        format!("（{code}）")
    }
}

fn format_price_text(value: f64) -> String {
    if value.abs() >= 100.0 {
        format!("{value:.2}")
    } else {
        format!("{value:.3}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}

pub(crate) fn format_signed_percent_text(value: f64) -> String {
    match ratio_percentage_points(value) {
        Some(percent) => format!("{percent:+.2}%"),
        None => "—".to_string(),
    }
}

pub(super) fn agent_data_summary(response: &AgentResponse) -> String {
    let Some(data) = response.data.as_ref() else {
        return "未返回结构化数据。".to_string();
    };
    let returned = data
        .get("returned")
        .and_then(Value::as_u64)
        .or_else(|| {
            data.get("items")
                .and_then(Value::as_array)
                .map(|items| items.len() as u64)
        })
        .or_else(|| {
            data.get("equity_curve")
                .and_then(Value::as_array)
                .map(|items| items.len() as u64)
        });
    match returned {
        Some(count) => format!("返回 {count} 条本地记录。"),
        None => "已返回本地结构化数据。".to_string(),
    }
}
