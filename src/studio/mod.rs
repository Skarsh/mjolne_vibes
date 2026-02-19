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
use crate::graph::watch::{GraphRefreshUpdate, GraphWatchHandle, spawn_graph_watch_worker};

pub mod events;

use self::events::{CanvasOp, StudioCommand, StudioEvent, StudioTurnResult};

const APP_TITLE: &str = "mjolne_vibes studio";
const MAX_CANVAS_SUMMARIES: usize = 24;
const CANVAS_PREVIEW_CHAR_LIMIT: usize = 180;

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
                            let tool_call_count = result.trace.tool_calls;
                            let assistant_preview = summarize_for_canvas(&result.final_text);

                            if event_tx
                                .send(StudioEvent::TurnCompleted {
                                    message: message.clone(),
                                    result,
                                })
                                .is_err()
                            {
                                break;
                            }

                            let _ = event_tx.send(StudioEvent::CanvasUpdate {
                                op: CanvasOp::AppendTurnSummary {
                                    user_message: message,
                                    assistant_preview,
                                    tool_call_count,
                                },
                            });
                            let _ = event_tx.send(StudioEvent::CanvasUpdate {
                                op: CanvasOp::SetStatus {
                                    message: "Idle".to_owned(),
                                },
                            });
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
                            let _ = event_tx.send(StudioEvent::CanvasUpdate {
                                op: CanvasOp::SetStatus {
                                    message: format!("Turn failed: {details}"),
                                },
                            });
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct CanvasState {
    status: String,
    graph_revision: Option<u64>,
    graph_node_count: usize,
    graph_edge_count: usize,
    graph_last_trigger: Option<String>,
    turn_summaries: Vec<CanvasTurnSummary>,
}

impl Default for CanvasState {
    fn default() -> Self {
        Self {
            status: "Idle".to_owned(),
            graph_revision: None,
            graph_node_count: 0,
            graph_edge_count: 0,
            graph_last_trigger: None,
            turn_summaries: Vec::new(),
        }
    }
}

impl CanvasState {
    fn apply(&mut self, op: CanvasOp) {
        match op {
            CanvasOp::SetStatus { message } => {
                self.status = message;
            }
            CanvasOp::SetGraphStats {
                revision,
                node_count,
                edge_count,
                trigger,
            } => {
                self.graph_revision = Some(revision);
                self.graph_node_count = node_count;
                self.graph_edge_count = edge_count;
                self.graph_last_trigger = Some(trigger);
            }
            CanvasOp::AppendTurnSummary {
                user_message,
                assistant_preview,
                tool_call_count,
            } => {
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
        }
    }
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
        self.canvas.apply(CanvasOp::SetGraphStats {
            revision,
            node_count,
            edge_count,
            trigger: trigger.clone(),
        });
        self.canvas.apply(CanvasOp::SetStatus {
            message: format!(
                "Graph refreshed (rev {revision}, {node_count} nodes, {edge_count} edges, trigger: {trigger})"
            ),
        });
    }

    fn apply_event(&mut self, event: StudioEvent) {
        match event {
            StudioEvent::TurnStarted {
                message,
                started_at,
            } => {
                self.turn_in_flight = true;
                let _ = started_at;
                self.canvas.apply(CanvasOp::SetStatus {
                    message: format!("Running turn for: {}", summarize_for_canvas(&message)),
                });
            }
            StudioEvent::TurnCompleted { message, result } => {
                let _ = message;
                self.turn_in_flight = false;
                self.chat_history
                    .push(ChatEntry::assistant(result.final_text));
            }
            StudioEvent::TurnFailed { message, error } => {
                self.turn_in_flight = false;
                self.chat_history.push(ChatEntry::system(format!(
                    "Turn failed for `{}`: {error}",
                    summarize_for_canvas(&message)
                )));
            }
            StudioEvent::CanvasUpdate { op } => self.canvas.apply(op),
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
        self.canvas.apply(CanvasOp::SetStatus {
            message: "Queued turn...".to_owned(),
        });

        if let Err(error) = self
            .command_tx
            .send(StudioCommand::SubmitUserMessage { message })
        {
            self.turn_in_flight = false;
            self.runtime_disconnected = true;
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

    fn render_canvas_pane(&self, ui: &mut egui::Ui) {
        ui.heading("Canvas");
        ui.label(format!("Status: {}", self.canvas.status));
        if let Some(revision) = self.canvas.graph_revision {
            ui.label(format!("Graph revision: {revision}"));
            ui.label(format!("Graph nodes: {}", self.canvas.graph_node_count));
            ui.label(format!("Graph edges: {}", self.canvas.graph_edge_count));
            if let Some(trigger) = &self.canvas.graph_last_trigger {
                ui.label(format!("Last graph trigger: {trigger}"));
            }
        } else {
            ui.label("Graph revision: pending initial refresh...");
        }
        ui.separator();

        egui::ScrollArea::vertical().show(ui, |ui| {
            if self.canvas.turn_summaries.is_empty() {
                ui.label("No turn summaries yet.");
                return;
            }

            for summary in self.canvas.turn_summaries.iter().rev() {
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
    use super::{CanvasOp, CanvasState, summarize_for_canvas};

    #[test]
    fn summarize_for_canvas_truncates_long_text() {
        let long_text = "x".repeat(260);
        let summary = summarize_for_canvas(&long_text);
        assert_eq!(summary.chars().count(), 180);
        assert!(summary.ends_with('…'));
    }

    #[test]
    fn canvas_state_tracks_latest_status_and_turns() {
        let mut canvas = CanvasState::default();
        canvas.apply(CanvasOp::SetStatus {
            message: "Running".to_owned(),
        });
        canvas.apply(CanvasOp::AppendTurnSummary {
            user_message: "hello".to_owned(),
            assistant_preview: "world".to_owned(),
            tool_call_count: 1,
        });

        assert_eq!(canvas.status, "Running");
        assert_eq!(canvas.turn_summaries.len(), 1);
        assert_eq!(canvas.turn_summaries[0].tool_call_count, 1);
    }

    #[test]
    fn canvas_state_tracks_graph_refresh_stats() {
        let mut canvas = CanvasState::default();
        canvas.apply(CanvasOp::SetGraphStats {
            revision: 3,
            node_count: 12,
            edge_count: 17,
            trigger: "turn_completed".to_owned(),
        });

        assert_eq!(canvas.graph_revision, Some(3));
        assert_eq!(canvas.graph_node_count, 12);
        assert_eq!(canvas.graph_edge_count, 17);
        assert_eq!(canvas.graph_last_trigger.as_deref(), Some("turn_completed"));
    }
}
