use std::collections::BTreeSet;

use crate::graph::ArchitectureGraph;

use super::events::CanvasOp;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanvasAnnotation {
    pub id: String,
    pub text: String,
    pub node_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CanvasState {
    graph: Option<ArchitectureGraph>,
    highlighted_node_ids: Vec<String>,
    focused_node_id: Option<String>,
    annotations: Vec<CanvasAnnotation>,
}

impl CanvasState {
    pub fn graph(&self) -> Option<&ArchitectureGraph> {
        self.graph.as_ref()
    }

    pub fn highlighted_node_ids(&self) -> &[String] {
        &self.highlighted_node_ids
    }

    pub fn focused_node_id(&self) -> Option<&str> {
        self.focused_node_id.as_deref()
    }

    pub fn annotations(&self) -> &[CanvasAnnotation] {
        &self.annotations
    }

    pub fn apply(&mut self, op: CanvasOp) {
        match op {
            CanvasOp::SetGraph { graph } => {
                self.graph = Some(graph);
                self.prune_unknown_node_references();
            }
            CanvasOp::HighlightNodes { node_ids } => {
                let mut seen = BTreeSet::new();
                let mut filtered = Vec::with_capacity(node_ids.len());
                for node_id in node_ids {
                    if !self.contains_node(&node_id) || !seen.insert(node_id.clone()) {
                        continue;
                    }
                    filtered.push(node_id);
                }
                self.highlighted_node_ids = filtered;
            }
            CanvasOp::FocusNode { node_id } => {
                self.focused_node_id = node_id.filter(|candidate| self.contains_node(candidate));
            }
            CanvasOp::AddAnnotation { id, text, node_id } => {
                if node_id
                    .as_deref()
                    .is_some_and(|candidate| !self.contains_node(candidate))
                {
                    return;
                }

                if let Some(existing) = self.annotations.iter_mut().find(|entry| entry.id == id) {
                    existing.text = text;
                    existing.node_id = node_id;
                } else {
                    self.annotations
                        .push(CanvasAnnotation { id, text, node_id });
                }
            }
            CanvasOp::ClearAnnotations => self.annotations.clear(),
        }
    }

    fn contains_node(&self, node_id: &str) -> bool {
        self.graph
            .as_ref()
            .is_some_and(|graph| graph.nodes.iter().any(|node| node.id == node_id))
    }

    fn prune_unknown_node_references(&mut self) {
        let known_node_ids = self
            .graph
            .as_ref()
            .map(|graph| {
                graph
                    .nodes
                    .iter()
                    .map(|node| node.id.as_str())
                    .collect::<BTreeSet<_>>()
            })
            .unwrap_or_default();

        self.highlighted_node_ids
            .retain(|node_id| known_node_ids.contains(node_id.as_str()));

        if self
            .focused_node_id
            .as_ref()
            .is_some_and(|node_id| !known_node_ids.contains(node_id.as_str()))
        {
            self.focused_node_id = None;
        }

        self.annotations.retain(|annotation| {
            annotation
                .node_id
                .as_deref()
                .is_none_or(|node_id| known_node_ids.contains(node_id))
        });
    }
}

#[cfg(test)]
mod tests {
    use std::time::UNIX_EPOCH;

    use crate::graph::{ArchitectureGraph, ArchitectureNode, ArchitectureNodeKind};

    use super::{CanvasOp, CanvasState};

    #[test]
    fn set_graph_replaces_snapshot_and_prunes_missing_node_references() {
        let mut state = CanvasState::default();
        state.apply(CanvasOp::SetGraph {
            graph: graph_with_nodes(1, &["module:crate", "module:crate::tools"]),
        });
        state.apply(CanvasOp::HighlightNodes {
            node_ids: vec!["module:crate".to_owned()],
        });
        state.apply(CanvasOp::FocusNode {
            node_id: Some("module:crate".to_owned()),
        });
        state.apply(CanvasOp::AddAnnotation {
            id: "one".to_owned(),
            text: "note".to_owned(),
            node_id: Some("module:crate".to_owned()),
        });
        state.apply(CanvasOp::AddAnnotation {
            id: "global".to_owned(),
            text: "global note".to_owned(),
            node_id: None,
        });

        state.apply(CanvasOp::SetGraph {
            graph: graph_with_nodes(2, &["module:crate::tools"]),
        });

        assert_eq!(state.graph().map(|graph| graph.revision), Some(2));
        assert!(state.highlighted_node_ids().is_empty());
        assert_eq!(state.focused_node_id(), None);
        assert_eq!(state.annotations().len(), 1);
        assert_eq!(state.annotations()[0].id, "global");
    }

    #[test]
    fn highlight_nodes_ignores_unknown_ids_and_deduplicates() {
        let mut state = CanvasState::default();
        state.apply(CanvasOp::SetGraph {
            graph: graph_with_nodes(1, &["module:crate", "module:crate::tools"]),
        });
        state.apply(CanvasOp::HighlightNodes {
            node_ids: vec![
                "module:crate".to_owned(),
                "module:missing".to_owned(),
                "module:crate".to_owned(),
                "module:crate::tools".to_owned(),
            ],
        });

        assert_eq!(
            state.highlighted_node_ids(),
            ["module:crate", "module:crate::tools"]
        );
    }

    #[test]
    fn focus_node_requires_valid_id_and_supports_clear() {
        let mut state = CanvasState::default();
        state.apply(CanvasOp::SetGraph {
            graph: graph_with_nodes(1, &["module:crate"]),
        });
        state.apply(CanvasOp::FocusNode {
            node_id: Some("module:crate".to_owned()),
        });
        assert_eq!(state.focused_node_id(), Some("module:crate"));

        state.apply(CanvasOp::FocusNode {
            node_id: Some("module:missing".to_owned()),
        });
        assert_eq!(state.focused_node_id(), None);

        state.apply(CanvasOp::FocusNode { node_id: None });
        assert_eq!(state.focused_node_id(), None);
    }

    #[test]
    fn add_annotation_updates_existing_id_and_rejects_unknown_nodes() {
        let mut state = CanvasState::default();
        state.apply(CanvasOp::SetGraph {
            graph: graph_with_nodes(1, &["module:crate"]),
        });
        state.apply(CanvasOp::AddAnnotation {
            id: "one".to_owned(),
            text: "first".to_owned(),
            node_id: Some("module:crate".to_owned()),
        });
        state.apply(CanvasOp::AddAnnotation {
            id: "one".to_owned(),
            text: "updated".to_owned(),
            node_id: None,
        });
        state.apply(CanvasOp::AddAnnotation {
            id: "invalid".to_owned(),
            text: "skip".to_owned(),
            node_id: Some("module:missing".to_owned()),
        });

        assert_eq!(state.annotations().len(), 1);
        assert_eq!(state.annotations()[0].id, "one");
        assert_eq!(state.annotations()[0].text, "updated");
        assert_eq!(state.annotations()[0].node_id, None);
    }

    #[test]
    fn clear_annotations_removes_all_entries() {
        let mut state = CanvasState::default();
        state.apply(CanvasOp::SetGraph {
            graph: graph_with_nodes(1, &["module:crate"]),
        });
        state.apply(CanvasOp::AddAnnotation {
            id: "one".to_owned(),
            text: "note".to_owned(),
            node_id: Some("module:crate".to_owned()),
        });
        state.apply(CanvasOp::AddAnnotation {
            id: "two".to_owned(),
            text: "global".to_owned(),
            node_id: None,
        });
        assert_eq!(state.annotations().len(), 2);

        state.apply(CanvasOp::ClearAnnotations);
        assert!(state.annotations().is_empty());
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
