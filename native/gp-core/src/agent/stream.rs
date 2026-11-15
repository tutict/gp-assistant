use crate::agent::service::run_agent_with_source_and_context;
use crate::agent::types::{AgentContext, AgentStreamEvent};
use crate::data_source::{CoreDataSet, MarketDataSource, StaticDataSource};
use serde_json::{json, Value};

pub fn run_agent_stream_with_data_events(
    data: &CoreDataSet,
    message: &str,
    run_id: Option<&str>,
    mode: Option<&str>,
) -> Vec<AgentStreamEvent> {
    run_agent_stream_with_data_events_with_context(
        data,
        message,
        run_id,
        mode,
        &AgentContext::default(),
    )
}

pub fn run_agent_stream_with_data_events_with_context(
    data: &CoreDataSet,
    message: &str,
    run_id: Option<&str>,
    mode: Option<&str>,
    context: &AgentContext,
) -> Vec<AgentStreamEvent> {
    let source = StaticDataSource::new(data);
    run_agent_stream_with_source_events_with_context(&source, message, run_id, mode, context)
}

pub fn run_agent_stream_with_source_events(
    source: &impl MarketDataSource,
    message: &str,
    run_id: Option<&str>,
    mode: Option<&str>,
) -> Vec<AgentStreamEvent> {
    run_agent_stream_with_source_events_with_context(
        source,
        message,
        run_id,
        mode,
        &AgentContext::default(),
    )
}

pub fn run_agent_stream_with_source_events_with_context(
    source: &impl MarketDataSource,
    message: &str,
    run_id: Option<&str>,
    mode: Option<&str>,
    context: &AgentContext,
) -> Vec<AgentStreamEvent> {
    let run_id = run_id.unwrap_or("gp-agent-run").to_string();
    let mut events = vec![
        agent_status_event(&run_id, "understand", "理解任务", 8, None),
        agent_status_event(&run_id, "intent", "选择工具", 24, None),
        agent_status_event(&run_id, "execute", "执行本地工具", 64, None),
    ];

    match run_agent_with_source_and_context(source, message, mode, context) {
        Ok(response) => {
            let action = response.action.clone();
            for call in response.tool_calls.clone() {
                events.push(agent_payload_event(
                    &run_id,
                    "tool_start",
                    Some(action.as_str()),
                    json!({
                        "id": call.id,
                        "tool": call.tool,
                        "label": call.label,
                        "input": call.input,
                    }),
                ));
                events.push(agent_payload_event(
                    &run_id,
                    "tool_result",
                    Some(action.as_str()),
                    json!({
                        "id": call.id,
                        "tool": call.tool,
                        "status": call.status,
                        "output_summary": call.output_summary,
                        "warnings": call.warnings,
                    }),
                ));
            }
            if !response.evidence_summary.is_empty() {
                events.push(agent_payload_event(
                    &run_id,
                    "evidence",
                    Some(action.as_str()),
                    json!({ "items": response.evidence_summary.clone() }),
                ));
            }
            events.push(agent_status_event(
                &run_id,
                "format",
                "整理结果",
                88,
                Some(action.as_str()),
            ));
            events.push(agent_status_event(
                &run_id,
                "complete",
                "完成",
                100,
                Some(action.as_str()),
            ));
            events.push(agent_payload_event(
                &run_id,
                "final",
                Some(action.as_str()),
                json!({
                    "intent": response.intent.clone(),
                    "warnings": response.warnings.clone(),
                    "next_actions": response.next_actions.clone(),
                }),
            ));
            events.push(AgentStreamEvent {
                run_id,
                event_type: "result".to_string(),
                stage: None,
                label: None,
                percent: None,
                action: Some(action),
                response: Some(response),
                payload: None,
                message: None,
            });
        }
        Err(error) => {
            events.push(AgentStreamEvent {
                run_id,
                event_type: "error".to_string(),
                stage: None,
                label: None,
                percent: None,
                action: None,
                response: None,
                payload: None,
                message: Some(error.to_string()),
            });
        }
    }

    events
}

fn agent_status_event(
    run_id: &str,
    stage: &str,
    label: &str,
    percent: u8,
    action: Option<&str>,
) -> AgentStreamEvent {
    AgentStreamEvent {
        run_id: run_id.to_string(),
        event_type: "status".to_string(),
        stage: Some(stage.to_string()),
        label: Some(label.to_string()),
        percent: Some(percent),
        action: action.map(ToOwned::to_owned),
        response: None,
        payload: None,
        message: None,
    }
}

fn agent_payload_event(
    run_id: &str,
    event_type: &str,
    action: Option<&str>,
    payload: Value,
) -> AgentStreamEvent {
    AgentStreamEvent {
        run_id: run_id.to_string(),
        event_type: event_type.to_string(),
        stage: None,
        label: None,
        percent: None,
        action: action.map(ToOwned::to_owned),
        response: None,
        payload: Some(payload),
        message: None,
    }
}
