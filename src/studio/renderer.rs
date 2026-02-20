use std::collections::{BTreeMap, BTreeSet, HashMap};

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
    pub before_graph: Option<&'a ArchitectureGraph>,
    pub show_before_after_overlay: bool,
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

#[derive(Default)]
struct SubsystemBucket<'a> {
    modules: Vec<&'a ArchitectureNode>,
    files: Vec<&'a ArchitectureNode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NodeDeltaKind {
    Added,
    Changed,
    Impact,
    Unchanged,
}

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
        let before_node_ids = input
            .before_graph
            .map(|graph| {
                graph
                    .nodes
                    .iter()
                    .map(|node| node.id.as_str())
                    .collect::<BTreeSet<_>>()
            })
            .unwrap_or_default();
        let current_node_ids = input
            .graph
            .nodes
            .iter()
            .map(|node| node.id.as_str())
            .collect::<BTreeSet<_>>();
        let added_count = current_node_ids.difference(&before_node_ids).count();
        let removed_count = before_node_ids.difference(&current_node_ids).count();
        let changed_count = changed
            .iter()
            .filter(|id| before_node_ids.contains(**id))
            .count();

        let node_labels = build_semantic_node_labels(&input.graph.nodes);

        let mut commands = Vec::new();
        let mut fit_ids = Vec::new();
        let mut subsystem_buckets: BTreeMap<String, SubsystemBucket<'_>> = BTreeMap::new();
        let mut node_subsystems: HashMap<&str, String> = HashMap::new();
        for node in &input.graph.nodes {
            let subsystem = subsystem_key(node);
            node_subsystems.insert(node.id.as_str(), subsystem.clone());
            let bucket = subsystem_buckets.entry(subsystem).or_default();
            match node.kind {
                ArchitectureNodeKind::Module => bucket.modules.push(node),
                ArchitectureNodeKind::File => bucket.files.push(node),
            }
        }
        for bucket in subsystem_buckets.values_mut() {
            bucket.modules.sort_by(|a, b| a.id.cmp(&b.id));
            bucket.files.sort_by(|a, b| a.id.cmp(&b.id));
        }

        commands.push(CanvasDrawCommand::UpsertShape {
            shape: CanvasShapeObject {
                id: "systems:title".to_owned(),
                layer: 5,
                kind: CanvasShapeKind::Text,
                points: vec![CanvasPoint { x: 84, y: 34 }],
                text: Some("Systems".to_owned()),
                style: CanvasStyle {
                    fill_color: None,
                    stroke_color: None,
                    stroke_width_px: None,
                    text_color: Some("#255882".to_owned()),
                },
            },
        });
        if input.show_before_after_overlay && input.before_graph.is_some() {
            commands.push(CanvasDrawCommand::UpsertShape {
                shape: CanvasShapeObject {
                    id: "overlay:before-after-summary".to_owned(),
                    layer: 7,
                    kind: CanvasShapeKind::Text,
                    points: vec![CanvasPoint { x: 220, y: 36 }],
                    text: Some(format!(
                        "Δ +{added_count}  -{removed_count}  ~{changed_count}"
                    )),
                    style: CanvasStyle {
                        fill_color: None,
                        stroke_color: None,
                        stroke_width_px: None,
                        text_color: Some("#4d647b".to_owned()),
                    },
                },
            });
        }

        let mut subsystem_group_ids = Vec::new();
        let mut x_cursor = 92;
        for (subsystem, bucket) in &subsystem_buckets {
            commands.push(CanvasDrawCommand::UpsertShape {
                shape: CanvasShapeObject {
                    id: format!("system-label:{subsystem}"),
                    layer: 6,
                    kind: CanvasShapeKind::Text,
                    points: vec![CanvasPoint { x: x_cursor, y: 62 }],
                    text: Some(format!("{} system", clipped_system_label(subsystem))),
                    style: CanvasStyle {
                        fill_color: None,
                        stroke_color: None,
                        stroke_width_px: None,
                        text_color: Some("#315f81".to_owned()),
                    },
                },
            });

            let module_layout = layout_column(&bucket.modules, &node_labels, 104, x_cursor, 28);
            let mut module_shape_ids = Vec::new();
            for (node, x, y) in &module_layout {
                let shape = build_node_shape(
                    node,
                    node_labels
                        .get(node.id.as_str())
                        .map(String::as_str)
                        .unwrap_or(node.display_label.as_str()),
                    *x,
                    *y,
                    node_delta_kind(node.id.as_str(), &before_node_ids, &changed, &impact),
                );
                fit_ids.push(shape.id.clone());
                module_shape_ids.push(shape.id.clone());
                commands.push(CanvasDrawCommand::UpsertShape { shape });
            }

            let module_end_y = module_layout
                .iter()
                .map(|(node, _, y)| y + node_shape_height(label_for(node, &node_labels)))
                .max()
                .unwrap_or(126);
            let file_start_y = module_end_y + 74;
            let file_layout =
                layout_column(&bucket.files, &node_labels, file_start_y, x_cursor, 22);
            let mut file_shape_ids = Vec::new();
            for (node, x, y) in &file_layout {
                let shape = build_node_shape(
                    node,
                    node_labels
                        .get(node.id.as_str())
                        .map(String::as_str)
                        .unwrap_or(node.display_label.as_str()),
                    *x,
                    *y,
                    node_delta_kind(node.id.as_str(), &before_node_ids, &changed, &impact),
                );
                fit_ids.push(shape.id.clone());
                file_shape_ids.push(shape.id.clone());
                commands.push(CanvasDrawCommand::UpsertShape { shape });
            }

            let mut object_ids = module_shape_ids;
            object_ids.extend(file_shape_ids);
            subsystem_group_ids.push(format!("group:system:{subsystem}"));
            commands.push(CanvasDrawCommand::UpsertGroup {
                group: CanvasGroupObject {
                    id: format!("group:system:{subsystem}"),
                    layer: 24,
                    label: Some(format!("system:{subsystem}")),
                    object_ids,
                },
            });

            x_cursor += node_shape_width() + 86;
        }
        commands.push(CanvasDrawCommand::UpsertGroup {
            group: CanvasGroupObject {
                id: "group:systems".to_owned(),
                layer: 30,
                label: Some("Systems".to_owned()),
                object_ids: subsystem_group_ids,
            },
        });

        let mut edges = input.graph.edges.iter().collect::<Vec<_>>();
        edges.sort_by(|a, b| a.from.cmp(&b.from).then_with(|| a.to.cmp(&b.to)));
        if input.show_before_after_overlay
            && let Some(before_graph) = input.before_graph
        {
            let mut before_edges = before_graph.edges.iter().collect::<Vec<_>>();
            before_edges.sort_by(|a, b| a.from.cmp(&b.from).then_with(|| a.to.cmp(&b.to)));
            for edge in before_edges {
                commands.push(CanvasDrawCommand::UpsertConnector {
                    connector: super::events::CanvasConnectorObject {
                        id: format!("before-edge:{}->{}", edge.from, edge.to),
                        from_id: format!("node:{}", edge.from),
                        to_id: format!("node:{}", edge.to),
                        label: None,
                        style: CanvasStyle {
                            fill_color: None,
                            stroke_color: Some("#d6dee8".to_owned()),
                            stroke_width_px: Some(1),
                            text_color: None,
                        },
                    },
                });
            }
        }
        for edge in edges {
            let same_subsystem = node_subsystems
                .get(edge.from.as_str())
                .zip(node_subsystems.get(edge.to.as_str()))
                .is_some_and(|(from, to)| from == to);
            let style =
                if changed.contains(edge.from.as_str()) || changed.contains(edge.to.as_str()) {
                    CanvasStyle {
                        fill_color: None,
                        stroke_color: Some("#cf6c2a".to_owned()),
                        stroke_width_px: Some(2),
                        text_color: None,
                    }
                } else if impact.contains(edge.from.as_str()) || impact.contains(edge.to.as_str()) {
                    CanvasStyle {
                        fill_color: None,
                        stroke_color: Some("#3f89b2".to_owned()),
                        stroke_width_px: Some(2),
                        text_color: None,
                    }
                } else if same_subsystem {
                    CanvasStyle {
                        fill_color: None,
                        stroke_color: Some("#6f8da9".to_owned()),
                        stroke_width_px: Some(1),
                        text_color: None,
                    }
                } else {
                    CanvasStyle {
                        fill_color: None,
                        stroke_color: Some("#b5c5d6".to_owned()),
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
    delta_kind: NodeDeltaKind,
) -> CanvasShapeObject {
    let (fill_color, stroke_color) = match delta_kind {
        NodeDeltaKind::Added => ("#3aa66a", "#1f6642"),
        NodeDeltaKind::Changed => ("#dc7e35", "#88451b"),
        NodeDeltaKind::Impact => ("#4f98bf", "#2d6687"),
        NodeDeltaKind::Unchanged => match node.kind {
            ArchitectureNodeKind::Module => ("#3e7faa", "#22577a"),
            ArchitectureNodeKind::File => ("#4e9164", "#2f6543"),
        },
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

fn node_delta_kind<'a>(
    node_id: &'a str,
    before_node_ids: &BTreeSet<&'a str>,
    changed: &BTreeSet<&'a str>,
    impact: &BTreeSet<&'a str>,
) -> NodeDeltaKind {
    if !before_node_ids.is_empty() && !before_node_ids.contains(node_id) {
        NodeDeltaKind::Added
    } else if changed.contains(node_id) {
        NodeDeltaKind::Changed
    } else if impact.contains(node_id) {
        NodeDeltaKind::Impact
    } else {
        NodeDeltaKind::Unchanged
    }
}

fn label_for<'a>(node: &'a ArchitectureNode, labels: &'a HashMap<&str, String>) -> &'a str {
    labels
        .get(node.id.as_str())
        .map(String::as_str)
        .unwrap_or(node.display_label.as_str())
}

fn node_shape_width() -> i32 {
    222
}

fn node_shape_height(label: &str) -> i32 {
    let lines = label.lines().count().max(1) as i32;
    28 + (lines * 13)
}

fn layout_column<'a>(
    nodes: &'a [&ArchitectureNode],
    labels: &HashMap<&str, String>,
    start_y: i32,
    x: i32,
    gap: i32,
) -> Vec<(&'a ArchitectureNode, i32, i32)> {
    if nodes.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::with_capacity(nodes.len());
    let mut y_cursor = start_y;
    for node in nodes {
        let row_height = node_shape_height(label_for(node, labels));
        out.push((*node, x, y_cursor));
        y_cursor += row_height + gap;
    }
    out
}

fn subsystem_key(node: &ArchitectureNode) -> String {
    match node.kind {
        ArchitectureNodeKind::Module => {
            let raw = node.id.strip_prefix("module:").unwrap_or(node.id.as_str());
            let parts = raw.split("::").collect::<Vec<_>>();
            if parts.first() == Some(&"crate") && parts.len() >= 2 {
                return parts[1].to_owned();
            }
            parts.first().copied().unwrap_or("root").to_owned()
        }
        ArchitectureNodeKind::File => {
            if let Some(path) = &node.path {
                let normalized = path
                    .strip_prefix("src/")
                    .or_else(|| path.strip_prefix("./src/"))
                    .unwrap_or(path.as_str());
                return normalized
                    .split('/')
                    .next()
                    .filter(|segment| !segment.is_empty() && !segment.ends_with(".rs"))
                    .unwrap_or("root")
                    .to_owned();
            }
            let raw = node.id.strip_prefix("file:").unwrap_or(node.id.as_str());
            let normalized = raw.strip_prefix("src/").unwrap_or(raw);
            normalized
                .split('/')
                .next()
                .filter(|segment| !segment.is_empty() && !segment.ends_with(".rs"))
                .unwrap_or("root")
                .to_owned()
        }
    }
}

fn clipped_system_label(label: &str) -> String {
    let trimmed = label.trim();
    if trimmed.is_empty() {
        return "root".to_owned();
    }
    if trimmed.chars().count() <= 16 {
        trimmed.to_owned()
    } else {
        let mut short = trimmed.chars().take(13).collect::<String>();
        short.push_str("...");
        short
    }
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
            before_graph: None,
            show_before_after_overlay: false,
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
            before_graph: None,
            show_before_after_overlay: false,
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
            before_graph: None,
            show_before_after_overlay: false,
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
        assert_eq!(changed_shape.style.fill_color.as_deref(), Some("#dc7e35"));
    }

    #[test]
    fn architecture_renderer_places_files_below_module_block() {
        let graph = ArchitectureGraph {
            nodes: vec![
                ArchitectureNode {
                    id: "module:crate::core::a".to_owned(),
                    display_label: "a".to_owned(),
                    kind: ArchitectureNodeKind::Module,
                    path: None,
                },
                ArchitectureNode {
                    id: "module:crate::core::b".to_owned(),
                    display_label: "b".to_owned(),
                    kind: ArchitectureNodeKind::Module,
                    path: None,
                },
                ArchitectureNode {
                    id: "module:crate::core::c".to_owned(),
                    display_label: "c".to_owned(),
                    kind: ArchitectureNodeKind::Module,
                    path: None,
                },
                ArchitectureNode {
                    id: "module:crate::core::d".to_owned(),
                    display_label: "d".to_owned(),
                    kind: ArchitectureNodeKind::Module,
                    path: None,
                },
                ArchitectureNode {
                    id: "module:crate::core::e".to_owned(),
                    display_label: "e".to_owned(),
                    kind: ArchitectureNodeKind::Module,
                    path: None,
                },
                ArchitectureNode {
                    id: "file:src/core/f1.rs".to_owned(),
                    display_label: "f1".to_owned(),
                    kind: ArchitectureNodeKind::File,
                    path: Some("src/core/f1.rs".to_owned()),
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
            before_graph: None,
            show_before_after_overlay: false,
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
            before_graph: None,
            show_before_after_overlay: false,
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
    fn architecture_renderer_before_after_marks_added_nodes_and_emits_overlay_summary() {
        let before = ArchitectureGraph {
            nodes: vec![ArchitectureNode {
                id: "module:crate".to_owned(),
                display_label: "crate".to_owned(),
                kind: ArchitectureNodeKind::Module,
                path: None,
            }],
            edges: Vec::new(),
            revision: 1,
            generated_at: UNIX_EPOCH,
        };
        let after = ArchitectureGraph {
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
            ],
            edges: Vec::new(),
            revision: 2,
            generated_at: UNIX_EPOCH,
        };

        let batch = ArchitectureOverviewRenderer::render(ArchitectureOverviewRenderInput {
            graph: &after,
            changed_target_ids: &["module:crate::tools".to_owned()],
            impact_target_ids: &[],
            show_impact_overlay: false,
            before_graph: Some(&before),
            show_before_after_overlay: true,
            tool_cards: &[],
            turn_in_flight: false,
            canvas_status: "Idle",
            recent_activity: &[],
            sequence: 2,
        });

        let has_overlay_summary = batch.commands.iter().any(|command| match command {
            super::CanvasDrawCommand::UpsertShape { shape } => {
                shape.id == "overlay:before-after-summary"
            }
            _ => false,
        });
        assert!(has_overlay_summary);

        let added_shape_fill = batch.commands.iter().find_map(|command| match command {
            super::CanvasDrawCommand::UpsertShape { shape }
                if shape.id == "node:module:crate::tools" =>
            {
                shape.style.fill_color.as_deref()
            }
            _ => None,
        });
        assert_eq!(added_shape_fill, Some("#3aa66a"));
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
