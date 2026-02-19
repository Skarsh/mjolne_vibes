use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use eframe::egui;
use tokio::runtime::Handle;
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tracing::{info, warn};

use crate::agent::run_chat_turn;
use crate::config::AgentSettings;
use crate::graph::ArchitectureGraph;
use crate::graph::watch::{GraphRefreshUpdate, GraphWatchHandle, spawn_graph_watch_worker};

pub mod canvas;
pub mod events;

use self::canvas::{CanvasState, render_graph_snapshot};
use self::events::{CanvasOp, StudioCommand, StudioEvent, StudioTurnResult};

const APP_TITLE: &str = "mjolne_vibes studio";
const MAX_CANVAS_SUMMARIES: usize = 24;
const CANVAS_PREVIEW_CHAR_LIMIT: usize = 180;
const MAX_IMPACT_NODE_ANNOTATIONS: usize = 12;

pub fn run_studio(settings: &AgentSettings) -> Result<()> {
    let runtime_handle = Handle::try_current().context("studio requires a tokio runtime")?;
    let workspace_root =
        std::env::current_dir().context("failed to resolve workspace root for studio")?;

    let (command_tx, command_rx) = unbounded_channel::<StudioCommand>();
    let (event_tx, event_rx) = unbounded_channel::<StudioEvent>();
    let (graph_watch_handle, graph_update_rx) =
        spawn_graph_watch_worker(&runtime_handle, workspace_root.clone());
    let app_settings = settings.clone();

    spawn_runtime_worker(
        &runtime_handle,
        settings.clone(),
        command_rx,
        event_tx,
        graph_watch_handle.clone(),
    );
    info!(
        provider = %settings.model_provider,
        model = %settings.model,
        workspace_root = %workspace_root.display(),
        "starting native studio shell"
    );

    eframe::run_native(
        APP_TITLE,
        eframe::NativeOptions::default(),
        Box::new(move |_cc| {
            Ok(Box::new(StudioApp::new(
                app_settings,
                command_tx,
                event_rx,
                graph_update_rx,
                graph_watch_handle,
                workspace_root,
            )))
        }),
    )
    .map_err(|error| anyhow::anyhow!("studio UI exited with error: {error}"))
}

fn spawn_runtime_worker(
    handle: &Handle,
    settings: AgentSettings,
    mut command_rx: UnboundedReceiver<StudioCommand>,
    event_tx: UnboundedSender<StudioEvent>,
    graph_watch_handle: GraphWatchHandle,
) {
    let _task = handle.spawn(async move {
        while let Some(command) = command_rx.recv().await {
            match command {
                StudioCommand::SubmitUserMessage { message } => {
                    if event_tx
                        .send(StudioEvent::TurnStarted {
                            message: message.clone(),
                            started_at: SystemTime::now(),
                        })
                        .is_err()
                    {
                        break;
                    }

                    match run_chat_turn(&settings, &message).await {
                        Ok(outcome) => {
                            let result = StudioTurnResult::from(outcome);

                            if event_tx
                                .send(StudioEvent::TurnCompleted {
                                    message: message.clone(),
                                    result,
                                })
                                .is_err()
                            {
                                break;
                            }
                        }
                        Err(error) => {
                            let details = error.details();
                            if event_tx
                                .send(StudioEvent::TurnFailed {
                                    message: message.clone(),
                                    error: details.clone(),
                                })
                                .is_err()
                            {
                                break;
                            }
                        }
                    }

                    // Graph refreshes are decoupled from turn success/failure.
                    graph_watch_handle.notify_turn_completed();
                }
                StudioCommand::Shutdown => break,
            }
        }
    });
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChatSpeaker {
    User,
    Assistant,
    System,
}

impl ChatSpeaker {
    fn label(self) -> &'static str {
        match self {
            Self::User => "You",
            Self::Assistant => "Agent",
            Self::System => "Studio",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ChatEntry {
    speaker: ChatSpeaker,
    text: String,
}

impl ChatEntry {
    fn user(text: impl Into<String>) -> Self {
        Self {
            speaker: ChatSpeaker::User,
            text: text.into(),
        }
    }

    fn assistant(text: impl Into<String>) -> Self {
        Self {
            speaker: ChatSpeaker::Assistant,
            text: text.into(),
        }
    }

    fn system(text: impl Into<String>) -> Self {
        Self {
            speaker: ChatSpeaker::System,
            text: text.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CanvasTurnSummary {
    user_message: String,
    assistant_preview: String,
    tool_call_count: u32,
}

struct StudioApp {
    settings: AgentSettings,
    workspace_root: PathBuf,
    command_tx: UnboundedSender<StudioCommand>,
    event_rx: UnboundedReceiver<StudioEvent>,
    graph_update_rx: UnboundedReceiver<GraphRefreshUpdate>,
    graph_watch_handle: GraphWatchHandle,
    input_buffer: String,
    chat_history: Vec<ChatEntry>,
    canvas: CanvasState,
    canvas_status: String,
    graph_last_trigger: Option<String>,
    changed_node_ids: Vec<String>,
    impact_node_ids: Vec<String>,
    impact_overlay_enabled: bool,
    turn_summaries: Vec<CanvasTurnSummary>,
    turn_in_flight: bool,
    runtime_disconnected: bool,
    graph_watch_disconnected: bool,
}

impl StudioApp {
    fn new(
        settings: AgentSettings,
        command_tx: UnboundedSender<StudioCommand>,
        event_rx: UnboundedReceiver<StudioEvent>,
        graph_update_rx: UnboundedReceiver<GraphRefreshUpdate>,
        graph_watch_handle: GraphWatchHandle,
        workspace_root: PathBuf,
    ) -> Self {
        Self {
            settings,
            workspace_root,
            command_tx,
            event_rx,
            graph_update_rx,
            graph_watch_handle,
            input_buffer: String::new(),
            chat_history: vec![ChatEntry::system(
                "Studio ready. Send a prompt to run a chat turn.",
            )],
            canvas: CanvasState::default(),
            canvas_status: "Idle".to_owned(),
            graph_last_trigger: None,
            changed_node_ids: Vec::new(),
            impact_node_ids: Vec::new(),
            impact_overlay_enabled: false,
            turn_summaries: Vec::new(),
            turn_in_flight: false,
            runtime_disconnected: false,
            graph_watch_disconnected: false,
        }
    }

    fn drain_events(&mut self) {
        loop {
            match self.event_rx.try_recv() {
                Ok(event) => self.apply_event(event),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if !self.runtime_disconnected {
                        warn!("studio runtime worker disconnected");
                        self.chat_history.push(ChatEntry::system(
                            "Runtime worker disconnected. Restart studio to continue.",
                        ));
                    }
                    self.runtime_disconnected = true;
                    self.turn_in_flight = false;
                    break;
                }
            }
        }
    }

    fn drain_graph_updates(&mut self) {
        loop {
            match self.graph_update_rx.try_recv() {
                Ok(update) => self.apply_graph_update(update),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if !self.graph_watch_disconnected {
                        warn!("graph watch worker disconnected");
                        self.chat_history.push(ChatEntry::system(
                            "Graph watch worker disconnected; graph updates stopped.",
                        ));
                    }
                    self.graph_watch_disconnected = true;
                    break;
                }
            }
        }
    }

    fn apply_graph_update(&mut self, update: GraphRefreshUpdate) {
        let revision = update.graph.revision;
        let node_count = update.graph.nodes.len();
        let edge_count = update.graph.edges.len();
        let trigger = update.trigger.label().to_owned();
        let delta = graph_change_delta(self.canvas.graph(), &update.graph);
        self.changed_node_ids = delta.changed_node_ids;
        self.impact_node_ids = delta.impact_node_ids;
        self.canvas.apply(CanvasOp::SetGraph {
            graph: update.graph,
        });
        self.apply_graph_visualization();
        self.graph_last_trigger = Some(trigger.clone());
        self.canvas_status = if self.changed_node_ids.is_empty() {
            format!(
                "Graph refreshed (rev {revision}, {node_count} nodes, {edge_count} edges, trigger: {trigger})"
            )
        } else {
            format!(
                "Graph refreshed (rev {revision}, {node_count} nodes, {edge_count} edges, trigger: {trigger}, changed: {}, impact: {})",
                self.changed_node_ids.len(),
                self.impact_node_ids.len()
            )
        };
    }

    fn apply_graph_visualization(&mut self) {
        self.canvas.apply(CanvasOp::HighlightNodes {
            node_ids: build_highlight_node_ids(
                &self.changed_node_ids,
                &self.impact_node_ids,
                self.impact_overlay_enabled,
            ),
        });

        self.canvas.apply(CanvasOp::ClearAnnotations);
        if self.changed_node_ids.is_empty() {
            return;
        }

        self.canvas.apply(CanvasOp::AddAnnotation {
            id: "changed-summary".to_owned(),
            text: format!("Changed nodes: {}", self.changed_node_ids.len()),
            node_id: None,
        });

        if !self.impact_overlay_enabled {
            return;
        }

        self.canvas.apply(CanvasOp::AddAnnotation {
            id: "impact-summary".to_owned(),
            text: format!("1-hop impact nodes: {}", self.impact_node_ids.len()),
            node_id: None,
        });

        for node_id in self
            .impact_node_ids
            .iter()
            .take(MAX_IMPACT_NODE_ANNOTATIONS)
            .cloned()
        {
            self.canvas.apply(CanvasOp::AddAnnotation {
                id: format!("impact:{node_id}"),
                text: "1-hop impact".to_owned(),
                node_id: Some(node_id),
            });
        }
    }

    fn apply_event(&mut self, event: StudioEvent) {
        match event {
            StudioEvent::TurnStarted {
                message,
                started_at,
            } => {
                self.turn_in_flight = true;
                let _ = started_at;
                self.canvas_status =
                    format!("Running turn for: {}", summarize_for_canvas(&message));
            }
            StudioEvent::TurnCompleted { message, result } => {
                self.turn_in_flight = false;
                let assistant_preview = summarize_for_canvas(&result.final_text);
                self.record_turn_summary(message, assistant_preview, result.trace.tool_calls);
                self.chat_history
                    .push(ChatEntry::assistant(result.final_text));
                self.canvas_status = "Idle".to_owned();
            }
            StudioEvent::TurnFailed { message, error } => {
                self.turn_in_flight = false;
                self.chat_history.push(ChatEntry::system(format!(
                    "Turn failed for `{}`: {error}",
                    summarize_for_canvas(&message)
                )));
                self.canvas_status = format!("Turn failed: {error}");
            }
            StudioEvent::CanvasUpdate { op } => self.canvas.apply(op),
        }
    }

    fn record_turn_summary(
        &mut self,
        user_message: String,
        assistant_preview: String,
        tool_call_count: u32,
    ) {
        self.turn_summaries.push(CanvasTurnSummary {
            user_message,
            assistant_preview,
            tool_call_count,
        });
        if self.turn_summaries.len() > MAX_CANVAS_SUMMARIES {
            let extra = self.turn_summaries.len() - MAX_CANVAS_SUMMARIES;
            self.turn_summaries.drain(0..extra);
        }
    }

    fn submit_prompt(&mut self) {
        let message = self.input_buffer.trim().to_owned();
        if message.is_empty() {
            return;
        }

        self.input_buffer.clear();
        self.chat_history.push(ChatEntry::user(message.clone()));
        self.turn_in_flight = true;
        self.canvas_status = "Queued turn...".to_owned();

        if let Err(error) = self
            .command_tx
            .send(StudioCommand::SubmitUserMessage { message })
        {
            self.turn_in_flight = false;
            self.runtime_disconnected = true;
            self.canvas_status = "Runtime disconnected".to_owned();
            self.chat_history.push(ChatEntry::system(format!(
                "Failed to submit turn to runtime worker: {error}"
            )));
        }
    }

    fn render_chat_pane(&mut self, ui: &mut egui::Ui) {
        ui.heading("Chat");
        ui.label(format!(
            "Provider: {} | Model: {}",
            self.settings.model_provider, self.settings.model
        ));
        ui.label(format!("Workspace: {}", self.workspace_root.display()));
        ui.separator();

        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .max_height((ui.available_height() - 220.0).max(160.0))
            .show(ui, |ui| {
                for entry in &self.chat_history {
                    let speaker_style = match entry.speaker {
                        ChatSpeaker::User => egui::RichText::new(entry.speaker.label()).strong(),
                        ChatSpeaker::Assistant => egui::RichText::new(entry.speaker.label())
                            .color(egui::Color32::from_rgb(26, 103, 64))
                            .strong(),
                        ChatSpeaker::System => egui::RichText::new(entry.speaker.label())
                            .color(egui::Color32::from_rgb(140, 84, 0))
                            .strong(),
                    };
                    ui.label(speaker_style);
                    ui.label(&entry.text);
                    ui.add_space(8.0);
                }
            });

        ui.separator();
        ui.label("Prompt");
        ui.add(
            egui::TextEdit::multiline(&mut self.input_buffer)
                .hint_text("Ask the agent...")
                .desired_rows(4),
        );

        let can_send = !self.turn_in_flight
            && !self.runtime_disconnected
            && !self.input_buffer.trim().is_empty();
        if ui
            .add_enabled(can_send, egui::Button::new("Send"))
            .clicked()
        {
            self.submit_prompt();
        }

        if self.turn_in_flight {
            ui.label("Turn running...");
        }
        if self.runtime_disconnected {
            ui.colored_label(
                egui::Color32::from_rgb(173, 33, 33),
                "Runtime worker is disconnected.",
            );
        }
    }

    fn render_canvas_pane(&mut self, ui: &mut egui::Ui) {
        ui.heading("Canvas");
        ui.label(format!("Status: {}", self.canvas_status));
        if let Some(graph) = self.canvas.graph() {
            let revision = graph.revision;
            let node_count = graph.nodes.len();
            let edge_count = graph.edges.len();
            ui.label(format!("Graph revision: {revision}"));
            ui.label(format!("Graph nodes: {node_count}"));
            ui.label(format!("Graph edges: {edge_count}"));
            if let Some(trigger) = &self.graph_last_trigger {
                ui.label(format!("Last graph trigger: {trigger}"));
            }
        } else {
            ui.label("Graph revision: pending initial refresh...");
        }
        if ui
            .checkbox(
                &mut self.impact_overlay_enabled,
                "Show 1-hop impact overlay",
            )
            .changed()
        {
            self.apply_graph_visualization();
        }
        ui.label(format!("Changed nodes: {}", self.changed_node_ids.len()));
        if self.impact_overlay_enabled {
            ui.label(format!(
                "1-hop impact nodes: {}",
                self.impact_node_ids.len()
            ));
        } else {
            ui.label("1-hop impact nodes: hidden");
        }
        ui.label(format!(
            "Highlighted nodes: {}",
            self.canvas.highlighted_node_ids().len()
        ));
        if let Some(focused_node) = self.canvas.focused_node_id() {
            ui.label(format!("Focused node: {focused_node}"));
        } else {
            ui.label("Focused node: none");
        }
        ui.label(format!("Annotations: {}", self.canvas.annotations().len()));
        for annotation in self.canvas.annotations() {
            if let Some(node_id) = &annotation.node_id {
                ui.label(format!("- {} ({node_id})", annotation.text));
            } else {
                ui.label(format!("- {}", annotation.text));
            }
        }
        ui.separator();
        ui.label(egui::RichText::new("Graph View").strong());
        render_graph_snapshot(
            ui,
            &self.canvas,
            &self.changed_node_ids,
            &self.impact_node_ids,
            self.impact_overlay_enabled,
        );
        ui.separator();

        egui::ScrollArea::vertical().show(ui, |ui| {
            if self.turn_summaries.is_empty() {
                ui.label("No turn summaries yet.");
                return;
            }

            for summary in self.turn_summaries.iter().rev() {
                ui.group(|ui| {
                    ui.label(egui::RichText::new("Turn").strong());
                    ui.label(format!("User: {}", summary.user_message));
                    ui.label(format!("Assistant: {}", summary.assistant_preview));
                    ui.label(format!("Tool calls: {}", summary.tool_call_count));
                });
                ui.add_space(6.0);
            }
        });
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct GraphChangeDelta {
    changed_node_ids: Vec<String>,
    impact_node_ids: Vec<String>,
}

fn graph_change_delta(
    previous: Option<&ArchitectureGraph>,
    current: &ArchitectureGraph,
) -> GraphChangeDelta {
    let Some(previous_graph) = previous else {
        return GraphChangeDelta::default();
    };

    let previous_nodes_by_id = previous_graph
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect::<BTreeMap<_, _>>();
    let current_nodes_by_id = current
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect::<BTreeMap<_, _>>();

    let mut changed_node_ids = BTreeSet::new();

    for node in &current.nodes {
        match previous_nodes_by_id.get(node.id.as_str()) {
            None => {
                changed_node_ids.insert(node.id.clone());
            }
            Some(previous_node) if *previous_node != node => {
                changed_node_ids.insert(node.id.clone());
            }
            Some(_) => {}
        }
    }

    let previous_edges = previous_graph
        .edges
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let current_edges = current.edges.iter().cloned().collect::<BTreeSet<_>>();

    for edge in current_edges.difference(&previous_edges) {
        if current_nodes_by_id.contains_key(edge.from.as_str()) {
            changed_node_ids.insert(edge.from.clone());
        }
        if current_nodes_by_id.contains_key(edge.to.as_str()) {
            changed_node_ids.insert(edge.to.clone());
        }
    }

    for edge in previous_edges.difference(&current_edges) {
        if current_nodes_by_id.contains_key(edge.from.as_str()) {
            changed_node_ids.insert(edge.from.clone());
        }
        if current_nodes_by_id.contains_key(edge.to.as_str()) {
            changed_node_ids.insert(edge.to.clone());
        }
    }

    let mut impact_node_ids = BTreeSet::new();
    if !changed_node_ids.is_empty() {
        for edge in &current.edges {
            let from_changed = changed_node_ids.contains(edge.from.as_str());
            let to_changed = changed_node_ids.contains(edge.to.as_str());
            if from_changed && !to_changed {
                impact_node_ids.insert(edge.to.clone());
            } else if to_changed && !from_changed {
                impact_node_ids.insert(edge.from.clone());
            }
        }
    }

    GraphChangeDelta {
        changed_node_ids: changed_node_ids.into_iter().collect(),
        impact_node_ids: impact_node_ids.into_iter().collect(),
    }
}

fn build_highlight_node_ids(
    changed_node_ids: &[String],
    impact_node_ids: &[String],
    include_impact_overlay: bool,
) -> Vec<String> {
    let mut highlighted = changed_node_ids.iter().cloned().collect::<BTreeSet<_>>();
    if include_impact_overlay {
        highlighted.extend(impact_node_ids.iter().cloned());
    }
    highlighted.into_iter().collect()
}

impl Drop for StudioApp {
    fn drop(&mut self) {
        let _ = self.command_tx.send(StudioCommand::Shutdown);
        self.graph_watch_handle.shutdown();
    }
}

impl eframe::App for StudioApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_events();
        self.drain_graph_updates();

        egui::SidePanel::left("studio_chat_pane")
            .resizable(true)
            .default_width(520.0)
            .show(ctx, |ui| self.render_chat_pane(ui));

        egui::CentralPanel::default().show(ctx, |ui| self.render_canvas_pane(ui));

        ctx.request_repaint_after(Duration::from_millis(120));
    }
}

fn summarize_for_canvas(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= CANVAS_PREVIEW_CHAR_LIMIT {
        return trimmed.to_owned();
    }

    let mut preview = trimmed
        .chars()
        .take(CANVAS_PREVIEW_CHAR_LIMIT.saturating_sub(1))
        .collect::<String>();
    preview.push('…');
    preview
}

#[cfg(test)]
mod tests {
    use std::time::UNIX_EPOCH;

    use crate::graph::{
        ArchitectureEdge, ArchitectureEdgeKind, ArchitectureGraph, ArchitectureNode,
        ArchitectureNodeKind,
    };

    use super::{build_highlight_node_ids, graph_change_delta, summarize_for_canvas};

    #[test]
    fn summarize_for_canvas_truncates_long_text() {
        let long_text = "x".repeat(260);
        let summary = summarize_for_canvas(&long_text);
        assert_eq!(summary.chars().count(), 180);
        assert!(summary.ends_with('…'));
    }

    #[test]
    fn graph_change_delta_is_empty_without_previous_graph() {
        let current = graph_for_test(2, &["module:crate"], &[("module:crate", "module:crate")]);
        let delta = graph_change_delta(None, &current);
        assert!(delta.changed_node_ids.is_empty());
        assert!(delta.impact_node_ids.is_empty());
    }

    #[test]
    fn graph_change_delta_detects_added_nodes_and_one_hop_impact() {
        let previous = graph_for_test(
            1,
            &["module:crate", "module:crate::tools"],
            &[("module:crate", "module:crate::tools")],
        );
        let current = graph_for_test(
            2,
            &[
                "module:crate",
                "module:crate::tools",
                "module:crate::tools::parser",
            ],
            &[
                ("module:crate", "module:crate::tools"),
                ("module:crate::tools", "module:crate::tools::parser"),
            ],
        );

        let delta = graph_change_delta(Some(&previous), &current);
        assert_eq!(
            delta.changed_node_ids,
            vec![
                "module:crate::tools".to_owned(),
                "module:crate::tools::parser".to_owned()
            ]
        );
        assert_eq!(delta.impact_node_ids, vec!["module:crate".to_owned()]);
    }

    #[test]
    fn build_highlight_node_ids_optionally_includes_impact_nodes() {
        let changed = vec!["module:crate::tools".to_owned()];
        let impact = vec!["module:crate".to_owned(), "module:crate::tools".to_owned()];

        let without_overlay = build_highlight_node_ids(&changed, &impact, false);
        assert_eq!(without_overlay, vec!["module:crate::tools".to_owned()]);

        let with_overlay = build_highlight_node_ids(&changed, &impact, true);
        assert_eq!(
            with_overlay,
            vec!["module:crate".to_owned(), "module:crate::tools".to_owned()]
        );
    }

    fn graph_for_test(
        revision: u64,
        node_ids: &[&str],
        edges: &[(&str, &str)],
    ) -> ArchitectureGraph {
        ArchitectureGraph {
            nodes: node_ids.iter().copied().map(graph_node).collect(),
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
        ArchitectureNode {
            id: node_id.to_owned(),
            display_label: node_id.to_owned(),
            kind: ArchitectureNodeKind::Module,
            path: None,
        }
    }
}
