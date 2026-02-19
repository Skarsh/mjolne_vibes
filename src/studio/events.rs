use std::time::SystemTime;

use crate::agent::{ChatTurnOutcome, ExecutedToolCall, TurnTraceSummary};
use crate::graph::ArchitectureGraph;

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
    SetGraph {
        graph: ArchitectureGraph,
    },
    HighlightNodes {
        node_ids: Vec<String>,
    },
    FocusNode {
        node_id: Option<String>,
    },
    AddAnnotation {
        id: String,
        text: String,
        node_id: Option<String>,
    },
    ClearAnnotations,
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use crate::agent::{ChatTurnOutcome, TurnTraceSummary};
    use crate::graph::{ArchitectureGraph, ArchitectureNode, ArchitectureNodeKind};

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
    fn canvas_set_graph_wraps_graph_payload() {
        let graph = graph_with_nodes(7, &["module:crate", "module:crate::tools"]);
        let op = CanvasOp::SetGraph {
            graph: graph.clone(),
        };

        match op {
            CanvasOp::SetGraph { graph: actual } => {
                assert_eq!(actual, graph);
            }
            CanvasOp::HighlightNodes { .. }
            | CanvasOp::FocusNode { .. }
            | CanvasOp::AddAnnotation { .. }
            | CanvasOp::ClearAnnotations => panic!("unexpected canvas op"),
        }
    }

    #[test]
    fn canvas_add_annotation_preserves_fields() {
        let op = CanvasOp::AddAnnotation {
            id: "todo-1".to_owned(),
            text: "inspect parser module".to_owned(),
            node_id: Some("module:crate::parser".to_owned()),
        };

        match op {
            CanvasOp::AddAnnotation { id, text, node_id } => {
                assert_eq!(id, "todo-1");
                assert_eq!(text, "inspect parser module");
                assert_eq!(node_id.as_deref(), Some("module:crate::parser"));
            }
            CanvasOp::SetGraph { .. }
            | CanvasOp::HighlightNodes { .. }
            | CanvasOp::FocusNode { .. }
            | CanvasOp::ClearAnnotations => panic!("unexpected canvas op"),
        }
    }

    #[test]
    fn canvas_highlight_and_focus_include_node_ids() {
        let highlight = CanvasOp::HighlightNodes {
            node_ids: vec!["module:crate".to_owned(), "module:crate::tools".to_owned()],
        };
        let focus = CanvasOp::FocusNode {
            node_id: Some("module:crate::tools".to_owned()),
        };

        match highlight {
            CanvasOp::HighlightNodes { node_ids } => {
                assert_eq!(node_ids, vec!["module:crate", "module:crate::tools"]);
            }
            CanvasOp::SetGraph { .. }
            | CanvasOp::FocusNode { .. }
            | CanvasOp::AddAnnotation { .. }
            | CanvasOp::ClearAnnotations => panic!("unexpected canvas op"),
        }

        match focus {
            CanvasOp::FocusNode { node_id } => {
                assert_eq!(node_id.as_deref(), Some("module:crate::tools"));
            }
            CanvasOp::SetGraph { .. }
            | CanvasOp::HighlightNodes { .. }
            | CanvasOp::AddAnnotation { .. }
            | CanvasOp::ClearAnnotations => panic!("unexpected canvas op"),
        }
    }

    fn graph_with_nodes(revision: u64, node_ids: &[&str]) -> ArchitectureGraph {
        ArchitectureGraph {
            nodes: node_ids.iter().copied().map(graph_node).collect(),
            edges: Vec::new(),
            revision,
            generated_at: UNIX_EPOCH,
        }
    }

    fn graph_node(node_id: &str) -> ArchitectureNode {
        ArchitectureNode {
            id: node_id.to_owned(),
            display_label: node_id.to_owned(),
            kind: ArchitectureNodeKind::Module,
            path: None,
        }
    }
}
