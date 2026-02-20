use std::collections::BTreeSet;
use std::collections::HashMap;

use crate::graph::{ArchitectureGraph, ArchitectureNode, ArchitectureNodeKind};

use super::canvas::CanvasToolCard;
use super::events::{
    CanvasDrawCommand, CanvasDrawCommandBatch, CanvasGroupObject, CanvasPoint, CanvasShapeKind,
    CanvasShapeObject, CanvasStyle, CanvasViewportHint,
};

pub struct ArchitectureOverviewRenderInput<'a> {
    pub graph: &'a ArchitectureGraph,
    pub changed_target_ids: &'a [String],
    pub impact_target_ids: &'a [String],
    pub show_impact_overlay: bool,
    pub tool_cards: &'a [CanvasToolCard],
    pub turn_in_flight: bool,
    pub canvas_status: &'a str,
    pub recent_activity: &'a [ArchitectureActivitySummary<'a>],
    pub sequence: u64,
}

pub struct ArchitectureActivitySummary<'a> {
    pub user_message: &'a str,
    pub assistant_preview: &'a str,
    pub tool_call_count: u32,
}

pub struct ArchitectureOverviewRenderer;

impl ArchitectureOverviewRenderer {
    pub fn render(input: ArchitectureOverviewRenderInput<'_>) -> CanvasDrawCommandBatch {
        let changed = input
            .changed_target_ids
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        let impact = if input.show_impact_overlay {
            input
                .impact_target_ids
                .iter()
                .map(String::as_str)
                .collect::<BTreeSet<_>>()
        } else {
            BTreeSet::new()
        };

        let node_labels = build_semantic_node_labels(&input.graph.nodes);

        let mut module_nodes = input
            .graph
            .nodes
            .iter()
            .filter(|node| node.kind == ArchitectureNodeKind::Module)
            .collect::<Vec<_>>();
        let mut file_nodes = input
            .graph
            .nodes
            .iter()
            .filter(|node| node.kind == ArchitectureNodeKind::File)
            .collect::<Vec<_>>();
        module_nodes.sort_by(|a, b| a.id.cmp(&b.id));
        file_nodes.sort_by(|a, b| a.id.cmp(&b.id));

        let mut commands = Vec::new();
        let mut module_shape_ids = Vec::new();
        let mut file_shape_ids = Vec::new();
        let mut fit_ids = Vec::new();

        commands.push(CanvasDrawCommand::UpsertShape {
            shape: CanvasShapeObject {
                id: "lane:modules-label".to_owned(),
                layer: 5,
                kind: CanvasShapeKind::Text,
                points: vec![CanvasPoint { x: 80, y: 72 }],
                text: Some("Modules lane".to_owned()),
                style: CanvasStyle {
                    fill_color: None,
                    stroke_color: None,
                    stroke_width_px: None,
                    text_color: Some("#355a7e".to_owned()),
                },
            },
        });

        let module_layout = layout_rows(&module_nodes, &node_labels, 100, 4, 80, 250, 28);
        for (node, x, y) in &module_layout {
            let shape = build_node_shape(
                node,
                node_labels
                    .get(node.id.as_str())
                    .map(String::as_str)
                    .unwrap_or(node.display_label.as_str()),
                *x,
                *y,
                &changed,
                &impact,
            );
            fit_ids.push(shape.id.clone());
            module_shape_ids.push(shape.id.clone());
            commands.push(CanvasDrawCommand::UpsertShape { shape });
        }
        let module_end_y = module_layout
            .iter()
            .map(|(node, _, y)| y + node_shape_height(label_for(node, &node_labels)))
            .max()
            .unwrap_or(180);
        let file_start_y = module_end_y + 96;
        commands.push(CanvasDrawCommand::UpsertShape {
            shape: CanvasShapeObject {
                id: "lane:files-label".to_owned(),
                layer: 5,
                kind: CanvasShapeKind::Text,
                points: vec![CanvasPoint {
                    x: 80,
                    y: file_start_y - 28,
                }],
                text: Some("Files lane".to_owned()),
                style: CanvasStyle {
                    fill_color: None,
                    stroke_color: None,
                    stroke_width_px: None,
                    text_color: Some("#3b6b4d".to_owned()),
                },
            },
        });
        let file_layout = layout_rows(&file_nodes, &node_labels, file_start_y, 4, 80, 250, 24);
        for (node, x, y) in &file_layout {
            let shape = build_node_shape(
                node,
                node_labels
                    .get(node.id.as_str())
                    .map(String::as_str)
                    .unwrap_or(node.display_label.as_str()),
                *x,
                *y,
                &changed,
                &impact,
            );
            fit_ids.push(shape.id.clone());
            file_shape_ids.push(shape.id.clone());
            commands.push(CanvasDrawCommand::UpsertShape { shape });
        }

        commands.push(CanvasDrawCommand::UpsertGroup {
            group: CanvasGroupObject {
                id: "group:modules".to_owned(),
                layer: 10,
                label: Some("Modules".to_owned()),
                object_ids: module_shape_ids,
            },
        });
        commands.push(CanvasDrawCommand::UpsertGroup {
            group: CanvasGroupObject {
                id: "group:files".to_owned(),
                layer: 20,
                label: Some("Files".to_owned()),
                object_ids: file_shape_ids,
            },
        });

        let mut edges = input.graph.edges.iter().collect::<Vec<_>>();
        edges.sort_by(|a, b| a.from.cmp(&b.from).then_with(|| a.to.cmp(&b.to)));
        for edge in edges {
            let style =
                if changed.contains(edge.from.as_str()) || changed.contains(edge.to.as_str()) {
                    CanvasStyle {
                        fill_color: None,
                        stroke_color: Some("#cc762f".to_owned()),
                        stroke_width_px: Some(2),
                        text_color: None,
                    }
                } else if impact.contains(edge.from.as_str()) || impact.contains(edge.to.as_str()) {
                    CanvasStyle {
                        fill_color: None,
                        stroke_color: Some("#4c8daf".to_owned()),
                        stroke_width_px: Some(2),
                        text_color: None,
                    }
                } else {
                    CanvasStyle {
                        fill_color: None,
                        stroke_color: Some("#8ea0b8".to_owned()),
                        stroke_width_px: Some(1),
                        text_color: None,
                    }
                };

            commands.push(CanvasDrawCommand::UpsertConnector {
                connector: super::events::CanvasConnectorObject {
                    id: format!("edge:{}->{}", edge.from, edge.to),
                    from_id: format!("node:{}", edge.from),
                    to_id: format!("node:{}", edge.to),
                    label: None,
                    style,
                },
            });
        }

        let _ = (
            input.tool_cards,
            input.turn_in_flight,
            input.canvas_status,
            input.recent_activity,
        );

        commands.push(CanvasDrawCommand::SetViewportHint {
            hint: CanvasViewportHint {
                center: None,
                zoom_percent: Some(100),
                fit_to_object_ids: fit_ids,
            },
        });

        CanvasDrawCommandBatch {
            sequence: input.sequence,
            commands,
        }
    }
}

fn build_node_shape(
    node: &ArchitectureNode,
    label: &str,
    x: i32,
    y: i32,
    changed: &BTreeSet<&str>,
    impact: &BTreeSet<&str>,
) -> CanvasShapeObject {
    let (fill_color, stroke_color) = if changed.contains(node.id.as_str()) {
        ("#d17a34", "#7f451e")
    } else if impact.contains(node.id.as_str()) {
        ("#5295b7", "#2c607f")
    } else {
        match node.kind {
            ArchitectureNodeKind::Module => ("#4d7d9e", "#2b4964"),
            ArchitectureNodeKind::File => ("#548f6a", "#2c5b3f"),
        }
    };

    let width = node_shape_width();
    let height = node_shape_height(label);

    CanvasShapeObject {
        id: format!("node:{}", node.id),
        layer: match node.kind {
            ArchitectureNodeKind::Module => 40,
            ArchitectureNodeKind::File => 60,
        },
        kind: CanvasShapeKind::Rectangle,
        points: vec![
            CanvasPoint { x, y },
            CanvasPoint {
                x: x + width,
                y: y + height,
            },
        ],
        text: Some(label.to_owned()),
        style: CanvasStyle {
            fill_color: Some(fill_color.to_owned()),
            stroke_color: Some(stroke_color.to_owned()),
            stroke_width_px: Some(2),
            text_color: Some("#ffffff".to_owned()),
        },
    }
}

fn label_for<'a>(node: &'a ArchitectureNode, labels: &'a HashMap<&str, String>) -> &'a str {
    labels
        .get(node.id.as_str())
        .map(String::as_str)
        .unwrap_or(node.display_label.as_str())
}

fn node_shape_width() -> i32 {
    210
}

fn node_shape_height(label: &str) -> i32 {
    let lines = label.lines().count().max(1) as i32;
    28 + (lines * 13)
}

fn layout_rows<'a>(
    nodes: &'a [&ArchitectureNode],
    labels: &HashMap<&str, String>,
    start_y: i32,
    columns: usize,
    start_x: i32,
    x_step: i32,
    row_gap: i32,
) -> Vec<(&'a ArchitectureNode, i32, i32)> {
    if nodes.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::with_capacity(nodes.len());
    let mut y_cursor = start_y;
    for row in nodes.chunks(columns.max(1)) {
        let mut row_height = 0_i32;
        for (col, node) in row.iter().enumerate() {
            let x = start_x + (col as i32 * x_step);
            out.push((*node, x, y_cursor));
            row_height = row_height.max(node_shape_height(label_for(node, labels)));
        }
        y_cursor += row_height + row_gap;
    }
    out
}

fn build_semantic_node_labels(nodes: &[ArchitectureNode]) -> HashMap<&str, String> {
    let parts = nodes
        .iter()
        .map(|node| (node.id.as_str(), split_node_parts(&node.id)))
        .collect::<Vec<_>>();
    let mut labels = HashMap::new();

    for (id, node_parts) in &parts {
        let unique_suffix_len = (1..=node_parts.len())
            .find(|suffix_len| {
                let suffix = node_parts[node_parts.len() - suffix_len..].join("::");
                parts
                    .iter()
                    .filter(|(other_id, _)| *other_id != *id)
                    .all(|(_, other_parts)| {
                        let other_suffix = if *suffix_len > other_parts.len() {
                            other_parts.join("::")
                        } else {
                            other_parts[other_parts.len() - suffix_len..].join("::")
                        };
                        other_suffix != suffix
                    })
            })
            .unwrap_or(node_parts.len());

        let suffix_start = node_parts.len().saturating_sub(unique_suffix_len);
        let short = node_parts[suffix_start..].join("::");
        let context = if suffix_start == 0 {
            None
        } else {
            Some(node_parts[..suffix_start].join("::"))
        };

        let mut lines = wrap_identifier_lines(&short, 20);
        if let Some(context) = context {
            lines.extend(wrap_identifier_lines(&context, 20));
        }
        let label = lines.join("\n");
        labels.insert(*id, label);
    }

    labels
}

fn split_node_parts(id: &str) -> Vec<String> {
    let parts = id
        .split("::")
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if parts.is_empty() {
        vec![id.to_owned()]
    } else {
        parts
    }
}

fn wrap_identifier_lines(text: &str, max_chars_per_line: usize) -> Vec<String> {
    if text.is_empty() || max_chars_per_line == 0 {
        return Vec::new();
    }

    let parts = text.split("::").collect::<Vec<_>>();
    let mut lines = Vec::new();
    let mut current = String::new();

    for part in parts {
        let segment = if current.is_empty() {
            part.to_owned()
        } else {
            format!("::{part}")
        };

        if current.chars().count() + segment.chars().count() <= max_chars_per_line {
            current.push_str(&segment);
            continue;
        }

        if !current.is_empty() {
            lines.push(current);
            current = String::new();
        }

        if part.chars().count() <= max_chars_per_line {
            current.push_str(part);
        } else {
            let chunks = part.chars().collect::<Vec<_>>();
            for chunk in chunks.chunks(max_chars_per_line) {
                lines.push(chunk.iter().collect::<String>());
            }
        }
    }

    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(text.to_owned());
    }
    lines
}

#[cfg(test)]
mod tests {
    use std::time::UNIX_EPOCH;

    use crate::graph::{
        ArchitectureEdge, ArchitectureEdgeKind, ArchitectureGraph, ArchitectureNode,
        ArchitectureNodeKind,
    };

    use super::{
        ArchitectureActivitySummary, ArchitectureOverviewRenderInput, ArchitectureOverviewRenderer,
        CanvasToolCard, build_semantic_node_labels, split_node_parts, wrap_identifier_lines,
    };

    #[test]
    fn architecture_renderer_outputs_deterministic_commands() {
        let graph = graph_fixture();
        let cards = vec![CanvasToolCard {
            id: "1".to_owned(),
            title: "search_notes".to_owned(),
            body: "found 3".to_owned(),
        }];

        let one = ArchitectureOverviewRenderer::render(ArchitectureOverviewRenderInput {
            graph: &graph,
            changed_target_ids: &["module:crate::tools".to_owned()],
            impact_target_ids: &["file:src/tools.rs".to_owned()],
            show_impact_overlay: true,
            tool_cards: &cards,
            turn_in_flight: false,
            canvas_status: "Idle",
            recent_activity: &[],
            sequence: 10,
        });
        let two = ArchitectureOverviewRenderer::render(ArchitectureOverviewRenderInput {
            graph: &graph,
            changed_target_ids: &["module:crate::tools".to_owned()],
            impact_target_ids: &["file:src/tools.rs".to_owned()],
            show_impact_overlay: true,
            tool_cards: &cards,
            turn_in_flight: false,
            canvas_status: "Idle",
            recent_activity: &[],
            sequence: 10,
        });

        assert_eq!(one, two);
    }

    #[test]
    fn architecture_renderer_marks_changed_nodes_with_changed_style() {
        let graph = graph_fixture();
        let batch = ArchitectureOverviewRenderer::render(ArchitectureOverviewRenderInput {
            graph: &graph,
            changed_target_ids: &["module:crate::tools".to_owned()],
            impact_target_ids: &[],
            show_impact_overlay: false,
            tool_cards: &[],
            turn_in_flight: false,
            canvas_status: "Idle",
            recent_activity: &[],
            sequence: 1,
        });

        let changed_shape = batch
            .commands
            .iter()
            .find_map(|command| match command {
                super::CanvasDrawCommand::UpsertShape { shape }
                    if shape.id == "node:module:crate::tools" =>
                {
                    Some(shape)
                }
                _ => None,
            })
            .expect("changed node shape should be present");
        assert_eq!(changed_shape.style.fill_color.as_deref(), Some("#d17a34"));
    }

    #[test]
    fn architecture_renderer_places_files_below_module_block() {
        let graph = ArchitectureGraph {
            nodes: vec![
                ArchitectureNode {
                    id: "module:a".to_owned(),
                    display_label: "a".to_owned(),
                    kind: ArchitectureNodeKind::Module,
                    path: None,
                },
                ArchitectureNode {
                    id: "module:b".to_owned(),
                    display_label: "b".to_owned(),
                    kind: ArchitectureNodeKind::Module,
                    path: None,
                },
                ArchitectureNode {
                    id: "module:c".to_owned(),
                    display_label: "c".to_owned(),
                    kind: ArchitectureNodeKind::Module,
                    path: None,
                },
                ArchitectureNode {
                    id: "module:d".to_owned(),
                    display_label: "d".to_owned(),
                    kind: ArchitectureNodeKind::Module,
                    path: None,
                },
                ArchitectureNode {
                    id: "module:e".to_owned(),
                    display_label: "e".to_owned(),
                    kind: ArchitectureNodeKind::Module,
                    path: None,
                },
                ArchitectureNode {
                    id: "file:f1".to_owned(),
                    display_label: "f1".to_owned(),
                    kind: ArchitectureNodeKind::File,
                    path: Some("src/f1.rs".to_owned()),
                },
            ],
            edges: Vec::new(),
            revision: 1,
            generated_at: UNIX_EPOCH,
        };

        let batch = ArchitectureOverviewRenderer::render(ArchitectureOverviewRenderInput {
            graph: &graph,
            changed_target_ids: &[],
            impact_target_ids: &[],
            show_impact_overlay: false,
            tool_cards: &[],
            turn_in_flight: false,
            canvas_status: "Idle",
            recent_activity: &[],
            sequence: 1,
        });

        let mut module_max_y = i32::MIN;
        let mut file_min_y = i32::MAX;
        for command in &batch.commands {
            let super::CanvasDrawCommand::UpsertShape { shape } = command else {
                continue;
            };
            let y = shape
                .points
                .first()
                .map(|point| point.y)
                .unwrap_or_default();
            if shape.id.starts_with("node:module:") {
                module_max_y = module_max_y.max(y);
            }
            if shape.id.starts_with("node:file:") {
                file_min_y = file_min_y.min(y);
            }
        }

        assert!(module_max_y > i32::MIN);
        assert!(file_min_y < i32::MAX);
        assert!(file_min_y > module_max_y);
    }

    #[test]
    fn architecture_renderer_does_not_emit_activity_or_tool_overlay_text() {
        let graph = graph_fixture();
        let activity = vec![
            ArchitectureActivitySummary {
                user_message: "inspect parser",
                assistant_preview: "Updated parser flow",
                tool_call_count: 2,
            },
            ArchitectureActivitySummary {
                user_message: "trace cfg",
                assistant_preview: "No config drift",
                tool_call_count: 1,
            },
        ];

        let batch = ArchitectureOverviewRenderer::render(ArchitectureOverviewRenderInput {
            graph: &graph,
            changed_target_ids: &[],
            impact_target_ids: &[],
            show_impact_overlay: false,
            tool_cards: &[],
            turn_in_flight: true,
            canvas_status: "Running turn for: inspect parser",
            recent_activity: &activity,
            sequence: 3,
        });

        let has_overlay_shape = batch.commands.iter().any(|command| match command {
            super::CanvasDrawCommand::UpsertShape { shape } => {
                shape.id == "status:activity"
                    || shape.id.starts_with("summary:")
                    || shape.id.starts_with("tool-card:")
            }
            _ => false,
        });
        assert!(!has_overlay_shape);
    }

    #[test]
    fn semantic_labels_use_unique_suffix_with_context_line() {
        let graph = ArchitectureGraph {
            nodes: vec![
                ArchitectureNode {
                    id: "crate::studio::renderer".to_owned(),
                    display_label: "crate::studio::renderer".to_owned(),
                    kind: ArchitectureNodeKind::Module,
                    path: None,
                },
                ArchitectureNode {
                    id: "crate::graph::renderer".to_owned(),
                    display_label: "crate::graph::renderer".to_owned(),
                    kind: ArchitectureNodeKind::Module,
                    path: None,
                },
            ],
            edges: Vec::new(),
            revision: 1,
            generated_at: UNIX_EPOCH,
        };
        let labels = build_semantic_node_labels(&graph.nodes);
        let studio = labels
            .get("crate::studio::renderer")
            .expect("studio label exists");
        let graph_label = labels
            .get("crate::graph::renderer")
            .expect("graph label exists");
        assert!(studio.contains('\n'));
        assert!(graph_label.contains('\n'));
        assert_ne!(studio, graph_label);
    }

    #[test]
    fn split_node_parts_uses_double_colon_boundaries() {
        let parts = split_node_parts("crate:studio::renderer::tests");
        assert_eq!(parts, ["crate:studio", "renderer", "tests"]);
    }

    #[test]
    fn wrap_identifier_lines_preserves_full_text_without_ellipsis() {
        let wrapped = wrap_identifier_lines("crate::very::long::module::path", 10);
        assert!(wrapped.len() >= 2);
        assert!(wrapped.iter().all(|line| line.chars().count() <= 10));
        assert!(!wrapped.join("").contains('…'));
        assert_eq!(
            wrapped.join("").replace("::", ""),
            "crateverylongmodulepath"
        );
    }

    fn graph_fixture() -> ArchitectureGraph {
        ArchitectureGraph {
            nodes: vec![
                ArchitectureNode {
                    id: "module:crate".to_owned(),
                    display_label: "crate".to_owned(),
                    kind: ArchitectureNodeKind::Module,
                    path: None,
                },
                ArchitectureNode {
                    id: "module:crate::tools".to_owned(),
                    display_label: "tools".to_owned(),
                    kind: ArchitectureNodeKind::Module,
                    path: None,
                },
                ArchitectureNode {
                    id: "file:src/tools.rs".to_owned(),
                    display_label: "tools.rs".to_owned(),
                    kind: ArchitectureNodeKind::File,
                    path: Some("src/tools.rs".to_owned()),
                },
            ],
            edges: vec![ArchitectureEdge {
                from: "module:crate".to_owned(),
                to: "module:crate::tools".to_owned(),
                relation: ArchitectureEdgeKind::DeclaresModule,
            }],
            revision: 1,
            generated_at: UNIX_EPOCH,
        }
    }
}
