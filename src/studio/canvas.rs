use std::collections::{BTreeMap, BTreeSet};

use eframe::egui;

use crate::graph::{ArchitectureGraph, ArchitectureNode, ArchitectureNodeKind};

use super::events::CanvasOp;

const MIN_CANVAS_SURFACE_WIDTH: f32 = 320.0;
const MIN_CANVAS_SURFACE_HEIGHT: f32 = 240.0;
const CANVAS_FRAME_INSET: f32 = 8.0;
const CANVAS_CONTENT_INSET_X: f32 = 24.0;
const CANVAS_CONTENT_INSET_Y: f32 = 24.0;

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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CanvasViewport {
    zoom: f32,
    pan: egui::Vec2,
}

impl Default for CanvasViewport {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            pan: egui::Vec2::ZERO,
        }
    }
}

impl CanvasViewport {
    const MIN_ZOOM: f32 = 0.45;
    const MAX_ZOOM: f32 = 2.75;

    pub fn zoom_percent(&self) -> u32 {
        (self.zoom * 100.0).round() as u32
    }

    pub fn zoom_in(&mut self) {
        self.zoom = (self.zoom * 1.12).clamp(Self::MIN_ZOOM, Self::MAX_ZOOM);
    }

    pub fn zoom_out(&mut self) {
        self.zoom = (self.zoom / 1.12).clamp(Self::MIN_ZOOM, Self::MAX_ZOOM);
    }

    pub fn reset(&mut self) {
        self.zoom = 1.0;
        self.pan = egui::Vec2::ZERO;
    }

    pub fn fit_to_view(&mut self) {
        // Current graph layout already fills the stage; fit maps to reset.
        self.reset();
    }

    fn apply_pointer_input(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        frame_center: egui::Pos2,
    ) {
        if response.dragged() {
            self.pan += ui.input(|input| input.pointer.delta());
        }

        if response.hovered() {
            let scroll_delta = ui.input(|input| input.raw_scroll_delta.y);
            if scroll_delta.abs() > f32::EPSILON {
                let factor = (scroll_delta / 280.0).exp();
                let anchor = response.hover_pos().unwrap_or(frame_center);
                self.zoom_with_anchor(anchor, frame_center, factor);
            }
        }
    }

    fn transformed_position(&self, position: egui::Pos2, canvas_center: egui::Pos2) -> egui::Pos2 {
        canvas_center + ((position - canvas_center) * self.zoom) + self.pan
    }

    fn zoom_clamped(&self, min: f32, max: f32) -> f32 {
        self.zoom.clamp(min, max)
    }

    fn zoom_with_anchor(&mut self, anchor: egui::Pos2, canvas_center: egui::Pos2, factor: f32) {
        let previous_zoom = self.zoom;
        let next_zoom = (previous_zoom * factor).clamp(Self::MIN_ZOOM, Self::MAX_ZOOM);
        if (next_zoom - previous_zoom).abs() <= f32::EPSILON {
            return;
        }

        let anchor_delta = anchor - canvas_center;
        let world_before = (anchor_delta - self.pan) / previous_zoom;
        self.zoom = next_zoom;
        self.pan = anchor_delta - (world_before * next_zoom);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanvasToolCard {
    pub id: String,
    pub title: String,
    pub body: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanvasSurfaceAdapterKind {
    ArchitectureGraph,
}

impl CanvasSurfaceAdapterKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::ArchitectureGraph => "Architecture graph",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct GraphSurfaceAdapterOptions<'a> {
    pub changed_node_ids: &'a [String],
    pub impact_node_ids: &'a [String],
    pub show_impact_overlay: bool,
    pub show_graph_legend: bool,
    pub tool_cards: &'a [CanvasToolCard],
}

#[derive(Debug, Clone, Copy)]
pub enum CanvasSurfaceAdapter<'a> {
    ArchitectureGraph {
        options: GraphSurfaceAdapterOptions<'a>,
    },
}

impl<'a> CanvasSurfaceAdapter<'a> {
    pub fn architecture_graph(options: GraphSurfaceAdapterOptions<'a>) -> Self {
        Self::ArchitectureGraph { options }
    }

    pub fn kind(&self) -> CanvasSurfaceAdapterKind {
        match self {
            Self::ArchitectureGraph { .. } => CanvasSurfaceAdapterKind::ArchitectureGraph,
        }
    }

    pub fn render(
        self,
        ui: &mut egui::Ui,
        state: &CanvasState,
        viewport: &mut CanvasViewport,
        surface_height: f32,
    ) {
        match self {
            Self::ArchitectureGraph { options } => {
                render_graph_snapshot(
                    ui,
                    state,
                    viewport,
                    GraphRenderOptions {
                        changed_node_ids: options.changed_node_ids,
                        impact_node_ids: options.impact_node_ids,
                        show_impact_overlay: options.show_impact_overlay,
                        show_graph_legend: options.show_graph_legend,
                        surface_height,
                        tool_cards: options.tool_cards,
                    },
                );
            }
        }
    }
}

struct GraphRenderOptions<'a> {
    changed_node_ids: &'a [String],
    impact_node_ids: &'a [String],
    show_impact_overlay: bool,
    show_graph_legend: bool,
    surface_height: f32,
    tool_cards: &'a [CanvasToolCard],
}

struct CanvasSurfaceFrame {
    response: egui::Response,
    painter: egui::Painter,
    frame: egui::Rect,
    content_rect: egui::Rect,
}

fn canvas_desired_size(available_width: f32, surface_height: f32) -> egui::Vec2 {
    egui::vec2(
        available_width.max(MIN_CANVAS_SURFACE_WIDTH),
        surface_height.max(MIN_CANVAS_SURFACE_HEIGHT),
    )
}

fn canvas_content_rect(frame: egui::Rect) -> egui::Rect {
    frame.shrink2(egui::vec2(CANVAS_CONTENT_INSET_X, CANVAS_CONTENT_INSET_Y))
}

fn render_canvas_surface_frame(
    ui: &mut egui::Ui,
    viewport: &mut CanvasViewport,
    surface_height: f32,
) -> CanvasSurfaceFrame {
    let desired_size = canvas_desired_size(ui.available_width(), surface_height);
    let (response, painter) = ui.allocate_painter(desired_size, egui::Sense::drag());
    let frame = response.rect.shrink(CANVAS_FRAME_INSET);
    painter.rect_filled(frame, 10.0, egui::Color32::from_rgb(249, 252, 255));
    painter.rect_stroke(
        frame,
        10.0,
        egui::Stroke::new(1.0, egui::Color32::from_rgb(202, 217, 236)),
        egui::StrokeKind::Outside,
    );
    viewport.apply_pointer_input(ui, &response, frame.center());

    CanvasSurfaceFrame {
        response,
        painter,
        frame,
        content_rect: canvas_content_rect(frame),
    }
}

fn render_graph_snapshot(
    ui: &mut egui::Ui,
    state: &CanvasState,
    viewport: &mut CanvasViewport,
    options: GraphRenderOptions<'_>,
) {
    const MODULE_NODE_RADIUS: f32 = 7.0;
    const FILE_NODE_SIZE: egui::Vec2 = egui::vec2(15.0, 9.0);
    const LABEL_MAX_CHARS: usize = 22;

    let surface = render_canvas_surface_frame(ui, viewport, options.surface_height);

    let Some(graph) = state.graph() else {
        surface.painter.text(
            surface.frame.center(),
            egui::Align2::CENTER_CENTER,
            "Graph preview pending initial refresh",
            egui::FontId::proportional(13.0),
            ui.visuals().weak_text_color(),
        );
        return;
    };

    let positions = compute_node_positions(graph, surface.content_rect)
        .into_iter()
        .map(|(id, pos)| {
            (
                id,
                viewport.transformed_position(pos, surface.frame.center()),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let changed = options
        .changed_node_ids
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let impact = if options.show_impact_overlay {
        options
            .impact_node_ids
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
    let hovered_node_id = surface.response.hover_pos().and_then(|pointer_pos| {
        positions
            .iter()
            .find(|(_, pos)| {
                pointer_pos.distance(**pos)
                    <= (MODULE_NODE_RADIUS + 4.0) * viewport.zoom_clamped(0.8, 1.7)
            })
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
            egui::Stroke::new(1.8, egui::Color32::from_rgb(205, 118, 47))
        } else if edge_touches_impact {
            egui::Stroke::new(1.5, egui::Color32::from_rgb(76, 141, 175))
        } else {
            egui::Stroke::new(
                0.9,
                egui::Color32::from_rgba_unmultiplied(137, 152, 171, 125),
            )
        };
        surface.painter.line_segment([*from, *to], stroke);
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
            egui::Color32::from_rgb(209, 122, 52)
        } else if is_impact {
            egui::Color32::from_rgb(82, 149, 183)
        } else if is_highlighted {
            egui::Color32::from_rgb(186, 154, 66)
        } else {
            match node.kind {
                ArchitectureNodeKind::Module => egui::Color32::from_rgb(89, 134, 152),
                ArchitectureNodeKind::File => egui::Color32::from_rgb(95, 148, 109),
            }
        };
        let stroke = if is_focused || is_hovered {
            egui::Stroke::new(2.2, egui::Color32::from_rgb(157, 80, 37))
        } else {
            egui::Stroke::new(1.0, egui::Color32::from_rgb(52, 46, 37))
        };
        let scaled_node_radius = MODULE_NODE_RADIUS * viewport.zoom_clamped(0.72, 1.8);
        let scaled_file_node_size = FILE_NODE_SIZE * viewport.zoom_clamped(0.72, 1.8);
        match node.kind {
            ArchitectureNodeKind::Module => {
                surface
                    .painter
                    .circle_filled(*position, scaled_node_radius, fill);
                surface
                    .painter
                    .circle_stroke(*position, scaled_node_radius, stroke);
            }
            ArchitectureNodeKind::File => {
                let rect = egui::Rect::from_center_size(*position, scaled_file_node_size);
                surface.painter.rect_filled(rect, 4.0, fill);
                surface
                    .painter
                    .rect_stroke(rect, 4.0, stroke, egui::StrokeKind::Outside);
            }
        }

        if draw_all_labels || is_changed || is_impact || is_focused || is_hovered {
            surface.painter.text(
                *position + egui::vec2(0.0, scaled_node_radius + 5.0),
                egui::Align2::CENTER_TOP,
                clipped_label(&node.display_label, LABEL_MAX_CHARS),
                egui::FontId::proportional(11.0 * viewport.zoom_clamped(0.85, 1.35)),
                ui.visuals().strong_text_color(),
            );
        }
    }

    if options.show_graph_legend {
        render_legend(ui, &surface.painter, surface.frame, viewport.zoom_percent());
    }
    render_tool_cards(&surface.painter, surface.frame, options.tool_cards);

    if options.show_graph_legend
        && let Some(hovered_node_id) = hovered_node_id
        && let Some(node) = graph.nodes.iter().find(|node| node.id == hovered_node_id)
    {
        let kind = match node.kind {
            ArchitectureNodeKind::Module => "module",
            ArchitectureNodeKind::File => "file",
        };
        let hint = format!("{kind}: {}", node.display_label);
        surface.painter.text(
            surface.frame.left_top() + egui::vec2(16.0, 16.0),
            egui::Align2::LEFT_TOP,
            clipped_label(&hint, 48),
            egui::FontId::proportional(11.0),
            egui::Color32::from_rgb(43, 57, 76),
        );
    }
}

fn render_legend(ui: &egui::Ui, painter: &egui::Painter, frame: egui::Rect, zoom_percent: u32) {
    let origin = frame.right_top() + egui::vec2(-180.0, 12.0);
    let bg = egui::Rect::from_min_size(origin, egui::vec2(168.0, 76.0));
    painter.rect_filled(bg, 8.0, egui::Color32::from_rgb(243, 248, 253));
    painter.rect_stroke(
        bg,
        8.0,
        egui::Stroke::new(1.0, egui::Color32::from_rgb(188, 205, 226)),
        egui::StrokeKind::Outside,
    );

    let items = [
        ("Module", egui::Color32::from_rgb(89, 134, 152)),
        ("File", egui::Color32::from_rgb(95, 148, 109)),
        ("Changed", egui::Color32::from_rgb(209, 122, 52)),
        ("Impact", egui::Color32::from_rgb(82, 149, 183)),
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
    painter.text(
        egui::pos2(bg.left() + 8.0, bg.bottom() - 8.0),
        egui::Align2::LEFT_BOTTOM,
        format!("{zoom_percent}%  drag + scroll"),
        egui::FontId::proportional(9.5),
        ui.visuals().weak_text_color(),
    );
}

fn render_tool_cards(painter: &egui::Painter, frame: egui::Rect, tool_cards: &[CanvasToolCard]) {
    const CARD_WIDTH: f32 = 240.0;
    const CARD_HEIGHT: f32 = 52.0;
    const CARD_SPACING: f32 = 8.0;
    const MAX_VISIBLE: usize = 5;

    for (index, card) in tool_cards.iter().rev().take(MAX_VISIBLE).enumerate() {
        let y_offset = index as f32 * (CARD_HEIGHT + CARD_SPACING);
        let origin = frame.left_bottom() + egui::vec2(12.0, -12.0 - CARD_HEIGHT - y_offset);
        let rect = egui::Rect::from_min_size(origin, egui::vec2(CARD_WIDTH, CARD_HEIGHT));
        painter.rect_filled(
            rect,
            8.0,
            egui::Color32::from_rgba_unmultiplied(236, 245, 255, 232),
        );
        painter.rect_stroke(
            rect,
            8.0,
            egui::Stroke::new(1.0, egui::Color32::from_rgb(166, 196, 232)),
            egui::StrokeKind::Outside,
        );
        painter.text(
            rect.left_top() + egui::vec2(10.0, 9.0),
            egui::Align2::LEFT_TOP,
            clipped_label(&card.title, 22),
            egui::FontId::proportional(10.5),
            egui::Color32::from_rgb(45, 89, 149),
        );
        painter.text(
            rect.left_top() + egui::vec2(10.0, 24.0),
            egui::Align2::LEFT_TOP,
            clipped_label(&card.body, 40),
            egui::FontId::proportional(10.0),
            egui::Color32::from_rgb(53, 68, 92),
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

    use super::{
        CanvasOp, CanvasState, CanvasSurfaceAdapter, CanvasSurfaceAdapterKind, CanvasToolCard,
        GraphSurfaceAdapterOptions, canvas_content_rect, canvas_desired_size, clipped_label,
        compute_node_positions,
    };

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

    #[test]
    fn canvas_desired_size_enforces_minimums() {
        let clamped = canvas_desired_size(120.0, 180.0);
        assert_eq!(clamped, egui::vec2(320.0, 240.0));

        let passthrough = canvas_desired_size(640.0, 420.0);
        assert_eq!(passthrough, egui::vec2(640.0, 420.0));
    }

    #[test]
    fn canvas_content_rect_applies_standard_padding() {
        let frame = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(200.0, 120.0));
        let content = canvas_content_rect(frame);

        assert_eq!(content.min, egui::pos2(34.0, 44.0));
        assert_eq!(content.max, egui::pos2(186.0, 116.0));
    }

    #[test]
    fn canvas_surface_adapter_reports_graph_kind() {
        let changed = vec!["module:crate".to_owned()];
        let impact = vec!["module:crate::tools".to_owned()];
        let cards = vec![CanvasToolCard {
            id: "card-1".to_owned(),
            title: "Tool".to_owned(),
            body: "details".to_owned(),
        }];
        let adapter = CanvasSurfaceAdapter::architecture_graph(GraphSurfaceAdapterOptions {
            changed_node_ids: &changed,
            impact_node_ids: &impact,
            show_impact_overlay: true,
            show_graph_legend: false,
            tool_cards: &cards,
        });

        assert_eq!(adapter.kind(), CanvasSurfaceAdapterKind::ArchitectureGraph);
        assert_eq!(adapter.kind().label(), "Architecture graph");
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
