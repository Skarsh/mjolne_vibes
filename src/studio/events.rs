use std::time::SystemTime;

use serde::{Deserialize, Serialize};

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
pub enum CanvasSceneData {
    ArchitectureGraph { graph: ArchitectureGraph },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanvasPoint {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanvasStyle {
    pub fill_color: Option<String>,
    pub stroke_color: Option<String>,
    pub stroke_width_px: Option<u16>,
    pub text_color: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CanvasShapeKind {
    Rectangle,
    Ellipse,
    Line,
    Path,
    Text,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanvasShapeObject {
    pub id: String,
    pub layer: u16,
    pub kind: CanvasShapeKind,
    pub points: Vec<CanvasPoint>,
    pub text: Option<String>,
    pub style: CanvasStyle,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanvasConnectorObject {
    pub id: String,
    pub from_id: String,
    pub to_id: String,
    pub label: Option<String>,
    pub style: CanvasStyle,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanvasGroupObject {
    pub id: String,
    pub layer: u16,
    pub label: Option<String>,
    pub object_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanvasViewportHint {
    pub center: Option<CanvasPoint>,
    pub zoom_percent: Option<u16>,
    pub fit_to_object_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum CanvasDrawCommand {
    UpsertShape { shape: CanvasShapeObject },
    UpsertConnector { connector: CanvasConnectorObject },
    UpsertGroup { group: CanvasGroupObject },
    DeleteObject { id: String },
    ClearScene,
    SetViewportHint { hint: CanvasViewportHint },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanvasDrawCommandBatch {
    pub sequence: u64,
    pub commands: Vec<CanvasDrawCommand>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanvasOp {
    SetSceneData {
        scene: CanvasSceneData,
    },
    SetHighlightedTargets {
        target_ids: Vec<String>,
    },
    SetFocusedTarget {
        target_id: Option<String>,
    },
    UpsertAnnotation {
        id: String,
        text: String,
        target_id: Option<String>,
    },
    // Legacy graph-specific operation aliases kept for transition compatibility.
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

impl CanvasOp {
    pub fn set_scene_graph(graph: ArchitectureGraph) -> Self {
        Self::SetSceneData {
            scene: CanvasSceneData::ArchitectureGraph { graph },
        }
    }

    pub fn set_highlighted_targets(target_ids: Vec<String>) -> Self {
        Self::SetHighlightedTargets { target_ids }
    }

    pub fn set_focused_target(target_id: Option<String>) -> Self {
        Self::SetFocusedTarget { target_id }
    }

    pub fn upsert_annotation(
        id: impl Into<String>,
        text: impl Into<String>,
        target_id: Option<String>,
    ) -> Self {
        Self::UpsertAnnotation {
            id: id.into(),
            text: text.into(),
            target_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use crate::agent::{ChatTurnOutcome, TurnTraceSummary};
    use crate::graph::{ArchitectureGraph, ArchitectureNode, ArchitectureNodeKind};
    use serde_json::json;

    use super::{CanvasDrawCommandBatch, CanvasOp, CanvasSceneData, StudioTurnResult};

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
    fn canvas_set_scene_graph_wraps_graph_payload() {
        let graph = graph_with_nodes(7, &["module:crate", "module:crate::tools"]);
        let op = CanvasOp::set_scene_graph(graph.clone());

        match op {
            CanvasOp::SetSceneData {
                scene: CanvasSceneData::ArchitectureGraph { graph: actual },
            } => {
                assert_eq!(actual, graph)
            }
            CanvasOp::SetHighlightedTargets { .. }
            | CanvasOp::SetFocusedTarget { .. }
            | CanvasOp::UpsertAnnotation { .. }
            | CanvasOp::HighlightNodes { .. }
            | CanvasOp::FocusNode { .. }
            | CanvasOp::AddAnnotation { .. }
            | CanvasOp::SetGraph { .. }
            | CanvasOp::ClearAnnotations => panic!("unexpected canvas op"),
        }
    }

    #[test]
    fn canvas_upsert_annotation_preserves_fields() {
        let op = CanvasOp::upsert_annotation(
            "todo-1",
            "inspect parser module",
            Some("module:crate::parser".to_owned()),
        );

        match op {
            CanvasOp::UpsertAnnotation {
                id,
                text,
                target_id,
            } => {
                assert_eq!(id, "todo-1");
                assert_eq!(text, "inspect parser module");
                assert_eq!(target_id.as_deref(), Some("module:crate::parser"));
            }
            CanvasOp::SetSceneData { .. }
            | CanvasOp::SetHighlightedTargets { .. }
            | CanvasOp::SetFocusedTarget { .. }
            | CanvasOp::SetGraph { .. }
            | CanvasOp::HighlightNodes { .. }
            | CanvasOp::FocusNode { .. }
            | CanvasOp::ClearAnnotations => panic!("unexpected canvas op"),
            CanvasOp::AddAnnotation { .. } => panic!("unexpected canvas op"),
        }
    }

    #[test]
    fn canvas_highlight_and_focus_include_target_ids() {
        let highlight = CanvasOp::set_highlighted_targets(vec![
            "module:crate".to_owned(),
            "module:crate::tools".to_owned(),
        ]);
        let focus = CanvasOp::set_focused_target(Some("module:crate::tools".to_owned()));

        match highlight {
            CanvasOp::SetHighlightedTargets { target_ids } => {
                assert_eq!(target_ids, vec!["module:crate", "module:crate::tools"]);
            }
            CanvasOp::SetSceneData { .. }
            | CanvasOp::SetFocusedTarget { .. }
            | CanvasOp::UpsertAnnotation { .. }
            | CanvasOp::SetGraph { .. }
            | CanvasOp::FocusNode { .. }
            | CanvasOp::AddAnnotation { .. }
            | CanvasOp::ClearAnnotations => panic!("unexpected canvas op"),
            CanvasOp::HighlightNodes { .. } => panic!("unexpected canvas op"),
        }

        match focus {
            CanvasOp::SetFocusedTarget { target_id } => {
                assert_eq!(target_id.as_deref(), Some("module:crate::tools"));
            }
            CanvasOp::SetSceneData { .. }
            | CanvasOp::SetHighlightedTargets { .. }
            | CanvasOp::UpsertAnnotation { .. }
            | CanvasOp::SetGraph { .. }
            | CanvasOp::HighlightNodes { .. }
            | CanvasOp::AddAnnotation { .. }
            | CanvasOp::ClearAnnotations => panic!("unexpected canvas op"),
            CanvasOp::FocusNode { .. } => panic!("unexpected canvas op"),
        }
    }

    #[test]
    fn legacy_graph_specific_ops_remain_constructible() {
        let graph = graph_with_nodes(3, &["module:crate"]);
        let set_graph = CanvasOp::SetGraph {
            graph: graph.clone(),
        };
        let highlight_nodes = CanvasOp::HighlightNodes {
            node_ids: vec!["module:crate".to_owned()],
        };
        let focus_node = CanvasOp::FocusNode {
            node_id: Some("module:crate".to_owned()),
        };
        let add_annotation = CanvasOp::AddAnnotation {
            id: "legacy".to_owned(),
            text: "legacy path".to_owned(),
            node_id: Some("module:crate".to_owned()),
        };

        match set_graph {
            CanvasOp::SetGraph { graph: actual } => assert_eq!(actual, graph),
            _ => panic!("expected legacy set graph op"),
        }
        match highlight_nodes {
            CanvasOp::HighlightNodes { node_ids } => assert_eq!(node_ids, ["module:crate"]),
            _ => panic!("expected legacy highlight nodes op"),
        }
        match focus_node {
            CanvasOp::FocusNode { node_id } => assert_eq!(node_id.as_deref(), Some("module:crate")),
            _ => panic!("expected legacy focus node op"),
        }
        match add_annotation {
            CanvasOp::AddAnnotation { id, text, node_id } => {
                assert_eq!(id, "legacy");
                assert_eq!(text, "legacy path");
                assert_eq!(node_id.as_deref(), Some("module:crate"));
            }
            _ => panic!("expected legacy add annotation op"),
        }
    }

    #[test]
    fn draw_command_batch_deserializes_with_typed_payloads() {
        let payload = json!({
            "sequence": 12,
            "commands": [
                {
                    "op": "upsert_shape",
                    "shape": {
                        "id": "module-card",
                        "layer": 1,
                        "kind": "rectangle",
                        "points": [{ "x": 10, "y": 16 }, { "x": 180, "y": 84 }],
                        "text": "module:crate::tools",
                        "style": {
                            "fill_color": "#eef5ff",
                            "stroke_color": "#3f6eb3",
                            "stroke_width_px": 2,
                            "text_color": "#1a2d4f"
                        }
                    }
                },
                { "op": "clear_scene" }
            ]
        });

        let batch: CanvasDrawCommandBatch =
            serde_json::from_value(payload).expect("valid draw command batch");
        assert_eq!(batch.sequence, 12);
        assert_eq!(batch.commands.len(), 2);
    }

    #[test]
    fn draw_command_batch_rejects_unknown_fields() {
        let with_unknown_top_level = json!({
            "sequence": 1,
            "commands": [],
            "unexpected": true
        });
        assert!(serde_json::from_value::<CanvasDrawCommandBatch>(with_unknown_top_level).is_err());

        let with_unknown_nested_field = json!({
            "sequence": 2,
            "commands": [{
                "op": "upsert_shape",
                "shape": {
                    "id": "node-1",
                    "layer": 1,
                    "kind": "rectangle",
                    "points": [{ "x": 0, "y": 0 }],
                    "text": null,
                    "style": {
                        "fill_color": null,
                        "stroke_color": "#000000",
                        "stroke_width_px": 1,
                        "text_color": null,
                        "shadow": "invalid"
                    }
                }
            }]
        });
        assert!(
            serde_json::from_value::<CanvasDrawCommandBatch>(with_unknown_nested_field).is_err()
        );
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
