use std::collections::BTreeSet;

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
    pub sequence: u64,
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

        let module_columns = 4_usize;
        let module_row_height = 140_i32;
        for (index, node) in module_nodes.iter().enumerate() {
            let x = 80 + ((index % 4) as i32 * 250);
            let y = 100 + ((index / module_columns) as i32 * module_row_height);
            let shape = build_node_shape(node, x, y, &changed, &impact);
            fit_ids.push(shape.id.clone());
            module_shape_ids.push(shape.id.clone());
            commands.push(CanvasDrawCommand::UpsertShape { shape });
        }
        let module_rows = module_nodes.len().max(1).div_ceil(module_columns) as i32;
        let file_start_y = 100 + (module_rows * module_row_height) + 180;
        for (index, node) in file_nodes.iter().enumerate() {
            let x = 80 + ((index % 4) as i32 * 250);
            let y = file_start_y + ((index / 4) as i32 * 125);
            let shape = build_node_shape(node, x, y, &changed, &impact);
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

        for (index, card) in input.tool_cards.iter().rev().take(5).enumerate() {
            commands.push(CanvasDrawCommand::UpsertShape {
                shape: CanvasShapeObject {
                    id: format!("tool-card:{index}:{}", card.id),
                    layer: 220,
                    kind: CanvasShapeKind::Text,
                    points: vec![CanvasPoint {
                        x: 940,
                        y: 70 + (index as i32 * 55),
                    }],
                    text: Some(format!("Tool: {} | {}", card.title, card.body)),
                    style: CanvasStyle {
                        fill_color: Some("#e9f5ff".to_owned()),
                        stroke_color: Some("#446fa6".to_owned()),
                        stroke_width_px: Some(1),
                        text_color: Some("#274262".to_owned()),
                    },
                },
            });
        }

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
                x: x + 190,
                y: y + 64,
            },
        ],
        text: Some(node.display_label.clone()),
        style: CanvasStyle {
            fill_color: Some(fill_color.to_owned()),
            stroke_color: Some(stroke_color.to_owned()),
            stroke_width_px: Some(2),
            text_color: Some("#ffffff".to_owned()),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::time::UNIX_EPOCH;

    use crate::graph::{
        ArchitectureEdge, ArchitectureEdgeKind, ArchitectureGraph, ArchitectureNode,
        ArchitectureNodeKind,
    };

    use super::{ArchitectureOverviewRenderInput, ArchitectureOverviewRenderer, CanvasToolCard};

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
            sequence: 10,
        });
        let two = ArchitectureOverviewRenderer::render(ArchitectureOverviewRenderInput {
            graph: &graph,
            changed_target_ids: &["module:crate::tools".to_owned()],
            impact_target_ids: &["file:src/tools.rs".to_owned()],
            show_impact_overlay: true,
            tool_cards: &cards,
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
