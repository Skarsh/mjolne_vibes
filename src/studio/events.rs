use std::time::SystemTime;

use crate::agent::{ChatTurnOutcome, ExecutedToolCall, TurnTraceSummary};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StudioCommand {
    SubmitUserMessage { message: String },
    Shutdown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StudioEvent {
    TurnStarted {
        message: String,
        started_at: SystemTime,
    },
    TurnCompleted {
        message: String,
        result: StudioTurnResult,
    },
    TurnFailed {
        message: String,
        error: String,
    },
    CanvasUpdate {
        op: CanvasOp,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StudioTurnResult {
    pub final_text: String,
    pub trace: TurnTraceSummary,
    pub tool_calls: Vec<ExecutedToolCall>,
}

impl From<ChatTurnOutcome> for StudioTurnResult {
    fn from(outcome: ChatTurnOutcome) -> Self {
        Self {
            final_text: outcome.final_text,
            trace: outcome.trace,
            tool_calls: outcome.tool_calls,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanvasOp {
    SetStatus {
        message: String,
    },
    AppendTurnSummary {
        user_message: String,
        assistant_preview: String,
        tool_call_count: u32,
    },
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::agent::{ChatTurnOutcome, TurnTraceSummary};

    use super::{CanvasOp, StudioTurnResult};

    #[test]
    fn studio_turn_result_preserves_chat_turn_payload() {
        let outcome = ChatTurnOutcome {
            final_text: "final response".to_owned(),
            trace: TurnTraceSummary {
                input_chars: 5,
                output_chars: Some(14),
                steps_executed: 1,
                model_calls: 1,
                tool_calls: 0,
                total_model_latency: Duration::from_millis(5),
                total_tool_latency: Duration::from_millis(0),
                tool_names: Vec::new(),
            },
            tool_calls: Vec::new(),
        };

        let studio_result = StudioTurnResult::from(outcome.clone());
        assert_eq!(studio_result.final_text, outcome.final_text);
        assert_eq!(studio_result.trace, outcome.trace);
        assert_eq!(studio_result.tool_calls, outcome.tool_calls);
    }

    #[test]
    fn canvas_append_turn_summary_tracks_preview_and_tool_count() {
        let op = CanvasOp::AppendTurnSummary {
            user_message: "summarize this".to_owned(),
            assistant_preview: "summary text".to_owned(),
            tool_call_count: 2,
        };

        match op {
            CanvasOp::AppendTurnSummary {
                user_message,
                assistant_preview,
                tool_call_count,
            } => {
                assert_eq!(user_message, "summarize this");
                assert_eq!(assistant_preview, "summary text");
                assert_eq!(tool_call_count, 2);
            }
            CanvasOp::SetStatus { .. } => panic!("unexpected canvas op"),
        }
    }
}
