use std::collections::{BTreeMap, BTreeSet};

use eframe::egui;

use crate::graph::{ArchitectureGraph, ArchitectureNode, ArchitectureNodeKind};

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

pub fn render_graph_snapshot(
    ui: &mut egui::Ui,
    state: &CanvasState,
    changed_node_ids: &[String],
    impact_node_ids: &[String],
    show_impact_overlay: bool,
) {
    const GRAPH_VIEW_HEIGHT: f32 = 360.0;
    const MODULE_NODE_RADIUS: f32 = 7.0;
    const FILE_NODE_SIZE: egui::Vec2 = egui::vec2(15.0, 9.0);
    const LABEL_MAX_CHARS: usize = 22;

    let desired_size = egui::vec2(ui.available_width().max(320.0), GRAPH_VIEW_HEIGHT);
    let (response, painter) = ui.allocate_painter(desired_size, egui::Sense::hover());
    let frame = response.rect.shrink(8.0);
    painter.rect_filled(
        frame,
        10.0,
        ui.visuals().extreme_bg_color.gamma_multiply(0.8),
    );
    painter.rect_stroke(
        frame,
        10.0,
        egui::Stroke::new(1.0, ui.visuals().widgets.inactive.bg_stroke.color),
        egui::StrokeKind::Outside,
    );

    let Some(graph) = state.graph() else {
        painter.text(
            frame.center(),
            egui::Align2::CENTER_CENTER,
            "Graph preview pending initial refresh",
            egui::FontId::proportional(13.0),
            ui.visuals().weak_text_color(),
        );
        return;
    };

    let graph_rect = frame.shrink2(egui::vec2(24.0, 24.0));
    let positions = compute_node_positions(graph, graph_rect);
    let changed = changed_node_ids
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let impact = if show_impact_overlay {
        impact_node_ids
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>()
    } else {
        BTreeSet::new()
    };
    let highlighted = state
        .highlighted_node_ids()
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let hovered_node_id = response.hover_pos().and_then(|pointer_pos| {
        positions
            .iter()
            .find(|(_, pos)| pointer_pos.distance(**pos) <= MODULE_NODE_RADIUS + 4.0)
            .map(|(id, _)| id.clone())
    });

    for edge in &graph.edges {
        let Some(from) = positions.get(edge.from.as_str()) else {
            continue;
        };
        let Some(to) = positions.get(edge.to.as_str()) else {
            continue;
        };
        let edge_touches_changed =
            changed.contains(edge.from.as_str()) || changed.contains(edge.to.as_str());
        let edge_touches_impact =
            impact.contains(edge.from.as_str()) || impact.contains(edge.to.as_str());
        let stroke = if edge_touches_changed {
            egui::Stroke::new(1.8, egui::Color32::from_rgb(235, 149, 58))
        } else if edge_touches_impact {
            egui::Stroke::new(1.5, egui::Color32::from_rgb(107, 160, 224))
        } else {
            egui::Stroke::new(
                0.9,
                egui::Color32::from_rgba_unmultiplied(125, 138, 158, 135),
            )
        };
        painter.line_segment([*from, *to], stroke);
    }

    let draw_all_labels = graph.nodes.len() <= 24;
    for node in &graph.nodes {
        let Some(position) = positions.get(node.id.as_str()) else {
            continue;
        };

        let is_changed = changed.contains(node.id.as_str());
        let is_impact = impact.contains(node.id.as_str()) && !is_changed;
        let is_focused = state
            .focused_node_id()
            .is_some_and(|focused| focused == node.id);
        let is_highlighted = highlighted.contains(node.id.as_str());
        let is_hovered = hovered_node_id
            .as_deref()
            .is_some_and(|hovered| hovered == node.id);

        let fill = if is_changed {
            egui::Color32::from_rgb(235, 149, 58)
        } else if is_impact {
            egui::Color32::from_rgb(107, 160, 224)
        } else if is_highlighted {
            egui::Color32::from_rgb(191, 171, 73)
        } else {
            match node.kind {
                ArchitectureNodeKind::Module => egui::Color32::from_rgb(84, 112, 147),
                ArchitectureNodeKind::File => egui::Color32::from_rgb(71, 129, 97),
            }
        };
        let stroke = if is_focused || is_hovered {
            egui::Stroke::new(2.2, egui::Color32::from_rgb(248, 222, 84))
        } else {
            egui::Stroke::new(1.0, egui::Color32::from_rgb(24, 29, 35))
        };
        match node.kind {
            ArchitectureNodeKind::Module => {
                painter.circle_filled(*position, MODULE_NODE_RADIUS, fill);
                painter.circle_stroke(*position, MODULE_NODE_RADIUS, stroke);
            }
            ArchitectureNodeKind::File => {
                let rect = egui::Rect::from_center_size(*position, FILE_NODE_SIZE);
                painter.rect_filled(rect, 4.0, fill);
                painter.rect_stroke(rect, 4.0, stroke, egui::StrokeKind::Outside);
            }
        }

        if draw_all_labels || is_changed || is_impact || is_focused || is_hovered {
            painter.text(
                *position + egui::vec2(0.0, MODULE_NODE_RADIUS + 5.0),
                egui::Align2::CENTER_TOP,
                clipped_label(&node.display_label, LABEL_MAX_CHARS),
                egui::FontId::proportional(11.0),
                ui.visuals().strong_text_color(),
            );
        }
    }

    render_legend(ui, &painter, frame);

    if let Some(hovered_node_id) = hovered_node_id
        && let Some(node) = graph.nodes.iter().find(|node| node.id == hovered_node_id)
    {
        let kind = match node.kind {
            ArchitectureNodeKind::Module => "module",
            ArchitectureNodeKind::File => "file",
        };
        let hint = format!("{kind}: {}", node.display_label);
        painter.text(
            frame.left_top() + egui::vec2(16.0, 16.0),
            egui::Align2::LEFT_TOP,
            clipped_label(&hint, 48),
            egui::FontId::proportional(11.0),
            egui::Color32::from_rgb(220, 228, 241),
        );
    }
}

fn render_legend(ui: &egui::Ui, painter: &egui::Painter, frame: egui::Rect) {
    let origin = frame.right_top() + egui::vec2(-180.0, 12.0);
    let bg = egui::Rect::from_min_size(origin, egui::vec2(168.0, 62.0));
    painter.rect_filled(bg, 8.0, ui.visuals().faint_bg_color.gamma_multiply(0.9));
    painter.rect_stroke(
        bg,
        8.0,
        egui::Stroke::new(1.0, ui.visuals().widgets.inactive.bg_stroke.color),
        egui::StrokeKind::Outside,
    );

    let items = [
        ("Module", egui::Color32::from_rgb(84, 112, 147)),
        ("File", egui::Color32::from_rgb(71, 129, 97)),
        ("Changed", egui::Color32::from_rgb(235, 149, 58)),
        ("Impact", egui::Color32::from_rgb(107, 160, 224)),
    ];
    for (index, (label, color)) in items.iter().enumerate() {
        let y = bg.top() + 12.0 + (index as f32 * 12.5);
        painter.circle_filled(egui::pos2(bg.left() + 10.0, y), 3.8, *color);
        painter.text(
            egui::pos2(bg.left() + 19.0, y),
            egui::Align2::LEFT_CENTER,
            *label,
            egui::FontId::proportional(10.0),
            ui.visuals().text_color(),
        );
    }
}

fn compute_node_positions(
    graph: &ArchitectureGraph,
    bounds: egui::Rect,
) -> BTreeMap<String, egui::Pos2> {
    let mut module_nodes = Vec::new();
    let mut file_nodes = Vec::new();
    for node in &graph.nodes {
        match node.kind {
            ArchitectureNodeKind::Module => module_nodes.push(node),
            ArchitectureNodeKind::File => file_nodes.push(node),
        }
    }

    let mut positions = BTreeMap::new();
    if module_nodes.is_empty() || file_nodes.is_empty() {
        let all_nodes = graph.nodes.iter().collect::<Vec<_>>();
        place_nodes_in_rect(&all_nodes, bounds, &mut positions);
        return positions;
    }

    let split_y = bounds.top() + bounds.height() * 0.58;
    let module_rect = egui::Rect::from_min_max(
        bounds.left_top(),
        egui::pos2(bounds.right(), (split_y - 10.0).max(bounds.top())),
    );
    let file_rect = egui::Rect::from_min_max(
        egui::pos2(bounds.left(), (split_y + 10.0).min(bounds.bottom())),
        bounds.right_bottom(),
    );

    place_nodes_in_rect(&module_nodes, module_rect, &mut positions);
    place_nodes_in_rect(&file_nodes, file_rect, &mut positions);
    positions
}

fn place_nodes_in_rect(
    nodes: &[&ArchitectureNode],
    rect: egui::Rect,
    positions: &mut BTreeMap<String, egui::Pos2>,
) {
    if nodes.is_empty() {
        return;
    }

    let columns = if nodes.len() <= 4 {
        nodes.len()
    } else {
        (nodes.len() as f32).sqrt().ceil() as usize
    }
    .max(1);
    let rows = nodes.len().div_ceil(columns);
    let x_step = if columns == 1 {
        0.0
    } else {
        rect.width() / (columns.saturating_sub(1) as f32)
    };
    let y_step = if rows == 1 {
        0.0
    } else {
        rect.height() / (rows.saturating_sub(1) as f32)
    };

    for (index, node) in nodes.iter().enumerate() {
        let row = index / columns;
        let col = index % columns;
        let x = if columns == 1 {
            rect.center().x
        } else {
            rect.left() + x_step * col as f32
        };
        let y = if rows == 1 {
            rect.center().y
        } else {
            rect.top() + y_step * row as f32
        };
        positions.insert(node.id.clone(), egui::pos2(x, y));
    }
}

fn clipped_label(label: &str, max_chars: usize) -> String {
    if label.chars().count() <= max_chars {
        return label.to_owned();
    }

    let mut clipped = label
        .chars()
        .take(max_chars.saturating_sub(3))
        .collect::<String>();
    clipped.push_str("...");
    clipped
}

#[cfg(test)]
mod tests {
    use std::time::UNIX_EPOCH;

    use eframe::egui;

    use crate::graph::{
        ArchitectureEdge, ArchitectureEdgeKind, ArchitectureGraph, ArchitectureNode,
        ArchitectureNodeKind,
    };

    use super::{CanvasOp, CanvasState, clipped_label, compute_node_positions};

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

    #[test]
    fn compute_node_positions_is_deterministic_and_complete() {
        let graph = graph_with_nodes_and_edges(
            1,
            &[
                ("module:crate", ArchitectureNodeKind::Module),
                ("module:crate::tools", ArchitectureNodeKind::Module),
                ("file:src/lib.rs", ArchitectureNodeKind::File),
                ("file:src/tools.rs", ArchitectureNodeKind::File),
            ],
            &[("module:crate", "module:crate::tools")],
        );
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(640.0, 320.0));

        let one = compute_node_positions(&graph, rect);
        let two = compute_node_positions(&graph, rect);

        assert_eq!(one, two);
        assert_eq!(one.len(), 4);
    }

    #[test]
    fn compute_node_positions_splits_module_and_file_lanes() {
        let graph = graph_with_nodes_and_edges(
            1,
            &[
                ("module:crate", ArchitectureNodeKind::Module),
                ("file:src/lib.rs", ArchitectureNodeKind::File),
            ],
            &[("module:crate", "file:src/lib.rs")],
        );
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(320.0, 220.0));
        let positions = compute_node_positions(&graph, rect);

        let module_y = positions
            .get("module:crate")
            .expect("module position should be present")
            .y;
        let file_y = positions
            .get("file:src/lib.rs")
            .expect("file position should be present")
            .y;
        assert!(module_y < file_y);
    }

    #[test]
    fn clipped_label_truncates_long_labels() {
        let label = clipped_label("crate::very::long::module::name", 12);
        assert_eq!(label, "crate::ve...");
    }

    fn graph_with_nodes(revision: u64, node_ids: &[&str]) -> ArchitectureGraph {
        ArchitectureGraph {
            nodes: node_ids.iter().copied().map(graph_node).collect(),
            edges: Vec::new(),
            revision,
            generated_at: UNIX_EPOCH,
        }
    }

    fn graph_with_nodes_and_edges(
        revision: u64,
        nodes: &[(&str, ArchitectureNodeKind)],
        edges: &[(&str, &str)],
    ) -> ArchitectureGraph {
        ArchitectureGraph {
            nodes: nodes
                .iter()
                .map(|(id, kind)| graph_node_with_kind(id, *kind))
                .collect(),
            edges: edges
                .iter()
                .map(|(from, to)| ArchitectureEdge {
                    from: (*from).to_owned(),
                    to: (*to).to_owned(),
                    relation: ArchitectureEdgeKind::DeclaresModule,
                })
                .collect(),
            revision,
            generated_at: UNIX_EPOCH,
        }
    }

    fn graph_node(node_id: &str) -> ArchitectureNode {
        graph_node_with_kind(node_id, ArchitectureNodeKind::Module)
    }

    fn graph_node_with_kind(node_id: &str, kind: ArchitectureNodeKind) -> ArchitectureNode {
        ArchitectureNode {
            id: node_id.to_owned(),
            display_label: node_id.to_owned(),
            kind,
            path: None,
        }
    }
}
