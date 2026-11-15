mod context;
mod intent;
mod registry;
mod response;
mod service;
mod stock_summary;
mod stream;
mod types;

pub use registry::agent_skill_registry;
pub use service::{
    run_agent_with_data, run_agent_with_data_and_context, run_agent_with_mock,
    run_agent_with_source, run_agent_with_source_and_context,
};
pub use stream::{
    run_agent_stream_with_data_events, run_agent_stream_with_data_events_with_context,
    run_agent_stream_with_source_events, run_agent_stream_with_source_events_with_context,
};
pub use types::{
    AgentAnswerSection, AgentContext, AgentEvidenceItem, AgentIntent, AgentRequest, AgentResponse,
    AgentSkill, AgentStreamEvent, AgentToolCall, AgentWatchlistItem,
};

#[cfg(test)]
pub(crate) use intent::heuristic_criteria;
#[cfg(test)]
pub(crate) use stock_summary::format_signed_percent_text;
