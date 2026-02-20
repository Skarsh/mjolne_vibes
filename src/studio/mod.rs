use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use eframe::egui;
use tokio::runtime::Handle;
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tracing::{info, warn};

use crate::agent::{ExecutedToolCall, run_chat_turn};
use crate::config::AgentSettings;
use crate::graph::ArchitectureGraph;
use crate::graph::watch::{GraphRefreshUpdate, GraphWatchHandle, spawn_graph_watch_worker};

pub mod canvas;
pub mod events;
pub mod renderer;

use self::canvas::{
    CanvasState, CanvasSurfaceAdapter, CanvasSurfaceAdapterKind, CanvasToolCard, CanvasViewport,
    GraphSurfaceAdapterOptions,
};
use self::events::{CanvasOp, StudioCommand, StudioEvent, StudioTurnResult};
use self::renderer::{ArchitectureOverviewRenderInput, ArchitectureOverviewRenderer};

const APP_TITLE: &str = "mjolne_vibes studio";
const MAX_CANVAS_SUMMARIES: usize = 24;
const MAX_CANVAS_TOOL_CARDS: usize = 16;
const CANVAS_PREVIEW_CHAR_LIMIT: usize = 180;
const MAX_IMPACT_NODE_ANNOTATIONS: usize = 12;
const MAX_GRAPH_UPDATES_PER_FRAME: usize = 4;

fn studio_text() -> egui::Color32 {
    egui::Color32::from_rgb(24, 36, 51)
}

fn studio_muted_text() -> egui::Color32 {
    egui::Color32::from_rgb(94, 109, 126)
}

fn studio_surface() -> egui::Color32 {
    egui::Color32::from_rgb(241, 247, 253)
}

fn studio_surface_alt() -> egui::Color32 {
    egui::Color32::from_rgb(250, 253, 255)
}

fn studio_panel_tint() -> egui::Color32 {
    egui::Color32::from_rgb(231, 241, 250)
}

fn studio_border() -> egui::Color32 {
    egui::Color32::from_rgb(183, 201, 223)
}

fn studio_border_strong() -> egui::Color32 {
    egui::Color32::from_rgb(151, 175, 203)
}

fn studio_accent() -> egui::Color32 {
    egui::Color32::from_rgb(22, 103, 186)
}

fn studio_accent_soft() -> egui::Color32 {
    egui::Color32::from_rgb(217, 233, 247)
}

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

type CanvasSurfaceKind = CanvasSurfaceAdapterKind;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct GraphSurfaceState {
    changed_target_ids: Vec<String>,
    impact_target_ids: Vec<String>,
    impact_overlay_enabled: bool,
    legend_enabled: bool,
    inspector_enabled: bool,
    last_refresh_trigger: Option<String>,
}

impl GraphSurfaceState {
    fn apply_refresh(
        &mut self,
        previous_graph: Option<&ArchitectureGraph>,
        current_graph: &ArchitectureGraph,
        trigger_label: &str,
    ) {
        let delta = graph_change_delta(previous_graph, current_graph);
        self.changed_target_ids = delta.changed_node_ids;
        self.impact_target_ids = delta.impact_node_ids;
        self.last_refresh_trigger = Some(trigger_label.to_owned());
    }

    fn apply_visualization(&self, canvas: &mut CanvasState) {
        canvas.apply(CanvasOp::set_highlighted_targets(
            self.highlight_target_ids(),
        ));
        canvas.apply(CanvasOp::ClearAnnotations);
        if self.changed_target_ids.is_empty() {
            return;
        }

        canvas.apply(CanvasOp::upsert_annotation(
            "changed-summary",
            format!("Changed nodes: {}", self.changed_target_ids.len()),
            None,
        ));

        if !self.impact_overlay_enabled {
            return;
        }

        canvas.apply(CanvasOp::upsert_annotation(
            "impact-summary",
            format!("1-hop impact nodes: {}", self.impact_target_ids.len()),
            None,
        ));

        for target_id in self
            .impact_target_ids
            .iter()
            .take(MAX_IMPACT_NODE_ANNOTATIONS)
            .cloned()
        {
            canvas.apply(CanvasOp::upsert_annotation(
                format!("impact:{target_id}"),
                "1-hop impact",
                Some(target_id),
            ));
        }
    }

    fn highlight_target_ids(&self) -> Vec<String> {
        build_highlight_node_ids(
            &self.changed_target_ids,
            &self.impact_target_ids,
            self.impact_overlay_enabled,
        )
    }

    fn refresh_status_label(&self) -> String {
        if self.changed_target_ids.is_empty() {
            "Canvas refreshed".to_owned()
        } else {
            format!(
                "Canvas refreshed ({} changed node{})",
                self.changed_target_ids.len(),
                if self.changed_target_ids.len() == 1 {
                    ""
                } else {
                    "s"
                }
            )
        }
    }

    fn last_trigger_label(&self) -> &str {
        self.last_refresh_trigger
            .as_deref()
            .unwrap_or("not yet refreshed")
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
    canvas_status: String,
    graph_surface: GraphSurfaceState,
    active_canvas_surface: CanvasSurfaceKind,
    chat_panel_expanded: bool,
    canvas_viewport: CanvasViewport,
    canvas_tool_cards: Vec<CanvasToolCard>,
    next_draw_command_sequence: u64,
    next_tool_card_id: u64,
    turn_summaries: Vec<CanvasTurnSummary>,
    theme_applied: bool,
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
            graph_surface: GraphSurfaceState::default(),
            active_canvas_surface: CanvasSurfaceKind::ArchitectureGraph,
            chat_panel_expanded: true,
            canvas_viewport: CanvasViewport::default(),
            canvas_tool_cards: Vec::new(),
            next_draw_command_sequence: 0,
            next_tool_card_id: 0,
            turn_summaries: Vec::new(),
            theme_applied: false,
            turn_in_flight: false,
            runtime_disconnected: false,
            graph_watch_disconnected: false,
        }
    }

    fn ensure_theme(&mut self, ctx: &egui::Context) {
        if self.theme_applied {
            return;
        }

        let mut style = (*ctx.style()).clone();
        style.spacing.item_spacing = egui::vec2(9.0, 9.0);
        style.spacing.button_padding = egui::vec2(12.0, 7.0);
        style.spacing.interact_size = egui::vec2(46.0, 32.0);
        style.spacing.indent = 13.0;
        style.spacing.window_margin = egui::Margin::symmetric(13, 11);
        style.spacing.menu_margin = egui::Margin::symmetric(9, 7);
        style.spacing.scroll = egui::style::ScrollStyle::floating();

        style.text_styles.insert(
            egui::TextStyle::Heading,
            egui::FontId::new(22.0, egui::FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Button,
            egui::FontId::new(13.8, egui::FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Body,
            egui::FontId::new(13.8, egui::FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Small,
            egui::FontId::new(11.8, egui::FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Monospace,
            egui::FontId::new(11.5, egui::FontFamily::Monospace),
        );

        let mut visuals = egui::Visuals::light();
        visuals.override_text_color = Some(studio_text());
        visuals.panel_fill = studio_surface();
        visuals.extreme_bg_color = egui::Color32::from_rgb(255, 255, 255);
        visuals.faint_bg_color = studio_panel_tint();
        visuals.code_bg_color = egui::Color32::from_rgb(236, 243, 251);
        visuals.window_fill = studio_surface_alt();
        visuals.window_stroke = egui::Stroke::new(1.0, studio_border());
        visuals.menu_corner_radius = 12.into();
        visuals.window_corner_radius = 12.into();
        visuals.selection.bg_fill = studio_accent().gamma_multiply(0.85);
        visuals.selection.stroke = egui::Stroke::new(1.0, studio_accent());
        visuals.widgets.noninteractive.corner_radius = 10.into();
        visuals.widgets.inactive.corner_radius = 10.into();
        visuals.widgets.hovered.corner_radius = 10.into();
        visuals.widgets.active.corner_radius = 10.into();
        visuals.widgets.open.corner_radius = 10.into();
        visuals.widgets.noninteractive.bg_fill = studio_surface_alt();
        visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, studio_text());
        visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(239, 247, 254);
        visuals.widgets.inactive.weak_bg_fill = egui::Color32::from_rgb(233, 242, 251);
        visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(225, 238, 250);
        visuals.widgets.active.bg_fill = egui::Color32::from_rgb(214, 231, 246);
        visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, studio_border());
        visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, studio_border_strong());
        visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0, studio_accent());
        visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, studio_text());
        visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.2, studio_text());
        visuals.widgets.active.fg_stroke = egui::Stroke::new(1.2, studio_text());
        visuals.hyperlink_color = egui::Color32::from_rgb(39, 110, 214);
        visuals.warn_fg_color = egui::Color32::from_rgb(184, 121, 34);
        visuals.error_fg_color = egui::Color32::from_rgb(193, 67, 67);
        style.visuals = visuals;

        ctx.set_style(style);
        self.theme_applied = true;
    }

    fn card_frame(_ui: &egui::Ui) -> egui::Frame {
        egui::Frame::new()
            .fill(studio_surface_alt())
            .stroke(egui::Stroke::new(1.0, studio_border()))
            .corner_radius(12)
            .inner_margin(egui::Margin::symmetric(12, 10))
    }

    fn chip(
        ui: &mut egui::Ui,
        text: impl Into<String>,
        fill: egui::Color32,
        stroke: egui::Color32,
        text_color: egui::Color32,
    ) {
        egui::Frame::new()
            .fill(fill)
            .stroke(egui::Stroke::new(1.0, stroke))
            .corner_radius(15)
            .inner_margin(egui::Margin::symmetric(9, 4))
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(text.into())
                        .small()
                        .strong()
                        .color(text_color),
                );
            });
    }

    fn session_status(&self) -> (&'static str, egui::Color32, egui::Color32, egui::Color32) {
        if self.turn_in_flight {
            (
                "Running",
                egui::Color32::from_rgb(255, 241, 220),
                egui::Color32::from_rgb(224, 175, 117),
                egui::Color32::from_rgb(150, 96, 27),
            )
        } else if self.runtime_disconnected {
            (
                "Disconnected",
                egui::Color32::from_rgb(253, 232, 232),
                egui::Color32::from_rgb(226, 160, 160),
                egui::Color32::from_rgb(163, 61, 61),
            )
        } else {
            (
                "Ready",
                egui::Color32::from_rgb(223, 244, 234),
                egui::Color32::from_rgb(153, 205, 182),
                egui::Color32::from_rgb(34, 118, 88),
            )
        }
    }

    fn render_top_bar(&mut self, ui: &mut egui::Ui) {
        let (status_label, status_fill, status_stroke, status_text_color) = self.session_status();
        ui.horizontal(|ui| {
            let toggle_button = egui::Button::new(
                egui::RichText::new(if self.chat_panel_expanded {
                    "Hide chat"
                } else {
                    "Show chat"
                })
                .small()
                .strong()
                .color(studio_text()),
            )
            .fill(studio_accent_soft())
            .stroke(egui::Stroke::new(1.0, studio_border()))
            .min_size(egui::vec2(94.0, 28.0));
            if ui.add(toggle_button).clicked() {
                self.chat_panel_expanded = !self.chat_panel_expanded;
            }
            ui.separator();
            ui.label(egui::RichText::new("Studio").strong().color(studio_text()));
            ui.label(
                egui::RichText::new(format!(
                    "{} / {}",
                    self.settings.model_provider, self.settings.model
                ))
                .small()
                .monospace()
                .color(studio_muted_text()),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.turn_in_flight {
                    ui.add(egui::Spinner::new());
                }
                Self::chip(
                    ui,
                    status_label,
                    status_fill,
                    status_stroke,
                    status_text_color,
                );
            });
        });
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
        for _ in 0..MAX_GRAPH_UPDATES_PER_FRAME {
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
        let trigger = update.trigger.label().to_owned();
        self.graph_surface
            .apply_refresh(self.canvas.graph(), &update.graph, &trigger);
        self.canvas.apply(CanvasOp::set_scene_graph(update.graph));
        self.apply_graph_visualization();
        self.render_architecture_overview_scene();
        self.canvas_status = self.graph_surface.refresh_status_label();
    }

    fn apply_graph_visualization(&mut self) {
        self.graph_surface.apply_visualization(&mut self.canvas);
    }

    fn render_architecture_overview_scene(&mut self) {
        let Some(graph) = self.canvas.graph().cloned() else {
            return;
        };

        self.next_draw_command_sequence = self.next_draw_command_sequence.saturating_add(1);
        let batch = ArchitectureOverviewRenderer::render(ArchitectureOverviewRenderInput {
            graph: &graph,
            changed_target_ids: &self.graph_surface.changed_target_ids,
            impact_target_ids: &self.graph_surface.impact_target_ids,
            show_impact_overlay: self.graph_surface.impact_overlay_enabled,
            tool_cards: &self.canvas_tool_cards,
            sequence: self.next_draw_command_sequence,
        });
        self.canvas.apply(CanvasOp::apply_draw_command_batch(batch));
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
                self.record_tool_cards(&result.tool_calls);
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

    fn record_tool_cards(&mut self, tool_calls: &[ExecutedToolCall]) {
        for call in tool_calls {
            let preview = summarize_for_canvas(&call.output);
            self.canvas_tool_cards.push(CanvasToolCard {
                id: format!("tool-card-{}", self.next_tool_card_id),
                title: call.tool_name.clone(),
                body: preview,
            });
            self.next_tool_card_id = self.next_tool_card_id.saturating_add(1);
        }

        if self.canvas_tool_cards.len() > MAX_CANVAS_TOOL_CARDS {
            let extra = self.canvas_tool_cards.len() - MAX_CANVAS_TOOL_CARDS;
            self.canvas_tool_cards.drain(0..extra);
        }

        self.render_architecture_overview_scene();
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
        const COMPOSER_SECTION_HEIGHT: f32 = 188.0;

        ui.horizontal_wrapped(|ui| {
            ui.label(
                egui::RichText::new("Chat")
                    .heading()
                    .strong()
                    .color(studio_text()),
            );
            Self::chip(
                ui,
                format!("{} messages", self.chat_history.len()),
                studio_accent_soft(),
                studio_border(),
                studio_muted_text(),
            );
        });
        ui.horizontal_wrapped(|ui| {
            Self::chip(
                ui,
                format!("{} / {}", self.settings.model_provider, self.settings.model),
                egui::Color32::from_rgb(236, 245, 253),
                studio_border(),
                studio_muted_text(),
            );
            let workspace_label = truncate_ui_text(&self.workspace_root.display().to_string(), 38);
            Self::chip(
                ui,
                format!("root {workspace_label}"),
                egui::Color32::from_rgb(239, 246, 252),
                studio_border(),
                studio_muted_text(),
            );
        });

        Self::card_frame(ui).show(ui, |ui| {
            egui::ScrollArea::vertical()
                .stick_to_bottom(true)
                .max_height((ui.available_height() - COMPOSER_SECTION_HEIGHT).max(140.0))
                .show(ui, |ui| {
                    for entry in &self.chat_history {
                        self.render_chat_entry(ui, entry);
                    }
                });
        });

        Self::card_frame(ui).show(ui, |ui| {
            ui.label(
                egui::RichText::new("Prompt")
                    .small()
                    .strong()
                    .color(studio_muted_text()),
            );
            ui.add(
                egui::TextEdit::multiline(&mut self.input_buffer)
                    .hint_text("Ask the agent...")
                    .desired_rows(4),
            );

            let can_send = !self.turn_in_flight
                && !self.runtime_disconnected
                && !self.input_buffer.trim().is_empty();
            ui.horizontal(|ui| {
                let send_button = egui::Button::new(
                    egui::RichText::new("Send")
                        .strong()
                        .color(egui::Color32::from_rgb(250, 253, 255)),
                )
                .fill(studio_accent())
                .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(31, 84, 151)))
                .min_size(egui::vec2(112.0, 31.0));
                if ui.add_enabled(can_send, send_button).clicked() {
                    self.submit_prompt();
                }

                if self.turn_in_flight {
                    Self::chip(
                        ui,
                        "Running...",
                        egui::Color32::from_rgb(255, 241, 220),
                        egui::Color32::from_rgb(224, 175, 117),
                        egui::Color32::from_rgb(150, 96, 27),
                    );
                } else if self.runtime_disconnected {
                    Self::chip(
                        ui,
                        "Runtime disconnected",
                        egui::Color32::from_rgb(253, 232, 232),
                        egui::Color32::from_rgb(226, 160, 160),
                        egui::Color32::from_rgb(163, 61, 61),
                    );
                }
            });
        });
    }

    fn render_chat_rail(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            let open_button = egui::Button::new(
                egui::RichText::new("›")
                    .heading()
                    .strong()
                    .color(studio_text()),
            )
            .fill(studio_accent_soft())
            .stroke(egui::Stroke::new(1.0, studio_border()))
            .min_size(egui::vec2(30.0, 30.0));
            if ui.add(open_button).clicked() {
                self.chat_panel_expanded = true;
            }
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("chat")
                    .small()
                    .strong()
                    .color(studio_muted_text()),
            );
            ui.label(
                egui::RichText::new(self.chat_history.len().to_string())
                    .small()
                    .strong()
                    .color(studio_text()),
            );
            if self.turn_in_flight {
                ui.add(egui::Spinner::new());
            }
        });
    }

    fn render_canvas_surface(&mut self, ui: &mut egui::Ui, surface_height: f32) {
        // Canvas surface dispatch point for future renderers (timeline, diffs, notes).
        let surface_adapter = Self::build_canvas_surface_adapter(
            self.active_canvas_surface,
            &self.graph_surface.changed_target_ids,
            &self.graph_surface.impact_target_ids,
            self.graph_surface.impact_overlay_enabled,
            self.graph_surface.legend_enabled,
            &self.canvas_tool_cards,
        );
        surface_adapter.render(ui, &self.canvas, &mut self.canvas_viewport, surface_height);
    }

    fn build_canvas_surface_adapter<'a>(
        active_surface: CanvasSurfaceKind,
        changed_node_ids: &'a [String],
        impact_node_ids: &'a [String],
        show_impact_overlay: bool,
        show_graph_legend: bool,
        tool_cards: &'a [CanvasToolCard],
    ) -> CanvasSurfaceAdapter<'a> {
        match active_surface {
            CanvasSurfaceKind::ArchitectureGraph => {
                CanvasSurfaceAdapter::architecture_graph(GraphSurfaceAdapterOptions {
                    changed_node_ids,
                    impact_node_ids,
                    show_impact_overlay,
                    show_graph_legend,
                    tool_cards,
                })
            }
        }
    }

    fn render_canvas_pane(&mut self, ui: &mut egui::Ui) {
        let revision = self.canvas.graph().map(|graph| graph.revision);
        let node_count = self.canvas.graph().map_or(0, |graph| graph.nodes.len());
        let edge_count = self.canvas.graph().map_or(0, |graph| graph.edges.len());
        let changed_count = self.graph_surface.changed_target_ids.len();
        let impact_count = self.graph_surface.impact_target_ids.len();

        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Canvas")
                    .heading()
                    .strong()
                    .color(studio_text()),
            );
            Self::chip(
                ui,
                self.active_canvas_surface.label(),
                studio_accent_soft(),
                studio_border(),
                studio_muted_text(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                egui::Frame::new()
                    .fill(studio_accent_soft())
                    .stroke(egui::Stroke::new(1.0, studio_border()))
                    .corner_radius(10)
                    .inner_margin(egui::Margin::symmetric(6, 4))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if ui.button("Fit").clicked() {
                                self.canvas_viewport.fit_to_view();
                            }
                            if ui.button("+").clicked() {
                                self.canvas_viewport.zoom_in();
                            }
                            if ui
                                .button(format!("{}%", self.canvas_viewport.zoom_percent()))
                                .clicked()
                            {
                                self.canvas_viewport.reset();
                            }
                            if ui.button("−").clicked() {
                                self.canvas_viewport.zoom_out();
                            }
                        });
                    });
            });
        });

        ui.horizontal_wrapped(|ui| {
            let revision_text = revision.map_or_else(|| "n/a".to_owned(), |rev| rev.to_string());
            Self::chip(
                ui,
                self.canvas_status.clone(),
                egui::Color32::from_rgb(237, 245, 252),
                studio_border(),
                studio_muted_text(),
            );
            Self::chip(
                ui,
                format!("revision {revision_text}"),
                egui::Color32::from_rgb(237, 245, 252),
                studio_border(),
                studio_muted_text(),
            );
            Self::chip(
                ui,
                format!("{node_count} nodes / {edge_count} edges"),
                egui::Color32::from_rgb(237, 245, 252),
                studio_border(),
                studio_muted_text(),
            );
            if changed_count > 0 {
                Self::chip(
                    ui,
                    format!("changed {changed_count}"),
                    egui::Color32::from_rgb(255, 237, 218),
                    egui::Color32::from_rgb(223, 167, 108),
                    egui::Color32::from_rgb(153, 100, 34),
                );
            }
            if impact_count > 0 {
                Self::chip(
                    ui,
                    format!("impact {impact_count}"),
                    egui::Color32::from_rgb(224, 240, 251),
                    egui::Color32::from_rgb(146, 184, 214),
                    egui::Color32::from_rgb(43, 97, 140),
                );
            }
        });

        let annotation_height = if self.canvas.annotations().is_empty() {
            0.0
        } else {
            46.0
        };
        let stage_height = (ui.available_height() - annotation_height).max(260.0);

        Self::card_frame(ui).show(ui, |ui| {
            let surface_height = (stage_height - 14.0).max(240.0);
            self.render_canvas_surface(ui, surface_height);
        });

        if !self.canvas.annotations().is_empty() {
            Self::card_frame(ui).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for annotation in self.canvas.annotations().iter().take(10) {
                        let text = if let Some(node_id) = &annotation.node_id {
                            format!("{} ({node_id})", annotation.text)
                        } else {
                            annotation.text.clone()
                        };
                        Self::chip(
                            ui,
                            text,
                            egui::Color32::from_rgb(234, 244, 252),
                            studio_border(),
                            studio_muted_text(),
                        );
                    }
                });
            });
        }

        let mut overlay_changed = false;
        Self::card_frame(ui).show(ui, |ui| {
            ui.collapsing("Surface options", |ui| {
                overlay_changed = ui
                    .checkbox(
                        &mut self.graph_surface.impact_overlay_enabled,
                        "Show impact overlay",
                    )
                    .changed();
                ui.checkbox(&mut self.graph_surface.legend_enabled, "Show graph legend");
                ui.checkbox(
                    &mut self.graph_surface.inspector_enabled,
                    "Show graph inspector",
                );
                ui.label(
                    egui::RichText::new(self.active_canvas_surface.label())
                        .small()
                        .color(studio_muted_text()),
                );
            });
        });
        if overlay_changed {
            self.apply_graph_visualization();
            self.render_architecture_overview_scene();
        }

        if self.graph_surface.inspector_enabled {
            Self::card_frame(ui).show(ui, |ui| {
                ui.label(
                    egui::RichText::new("Graph inspector")
                        .small()
                        .strong()
                        .color(studio_text()),
                );
                ui.horizontal_wrapped(|ui| {
                    Self::chip(
                        ui,
                        format!(
                            "revision {}",
                            revision.map_or_else(|| "n/a".to_owned(), |rev| rev.to_string())
                        ),
                        egui::Color32::from_rgb(237, 245, 252),
                        studio_border(),
                        studio_muted_text(),
                    );
                    Self::chip(
                        ui,
                        format!("{node_count} nodes / {edge_count} edges"),
                        egui::Color32::from_rgb(237, 245, 252),
                        studio_border(),
                        studio_muted_text(),
                    );
                    Self::chip(
                        ui,
                        format!("changed {changed_count}"),
                        egui::Color32::from_rgb(255, 237, 218),
                        egui::Color32::from_rgb(223, 167, 108),
                        egui::Color32::from_rgb(153, 100, 34),
                    );
                    Self::chip(
                        ui,
                        format!("impact {impact_count}"),
                        egui::Color32::from_rgb(224, 240, 251),
                        egui::Color32::from_rgb(146, 184, 214),
                        egui::Color32::from_rgb(43, 97, 140),
                    );
                });
                let trigger = self.graph_surface.last_trigger_label();
                ui.label(
                    egui::RichText::new(format!("Last refresh trigger: {trigger}"))
                        .small()
                        .color(studio_muted_text()),
                );
            });
        }
    }

    fn render_chat_entry(&self, ui: &mut egui::Ui, entry: &ChatEntry) {
        let (fill, stroke, label_color, text_color) = match entry.speaker {
            ChatSpeaker::User => (
                egui::Color32::from_rgb(233, 243, 253),
                egui::Color32::from_rgb(143, 177, 213),
                egui::Color32::from_rgb(47, 95, 146),
                egui::Color32::from_rgb(33, 67, 108),
            ),
            ChatSpeaker::Assistant => (
                egui::Color32::from_rgb(232, 245, 237),
                egui::Color32::from_rgb(139, 188, 160),
                egui::Color32::from_rgb(32, 113, 84),
                egui::Color32::from_rgb(28, 88, 68),
            ),
            ChatSpeaker::System => (
                egui::Color32::from_rgb(243, 246, 250),
                egui::Color32::from_rgb(188, 198, 213),
                egui::Color32::from_rgb(94, 109, 126),
                egui::Color32::from_rgb(73, 84, 102),
            ),
        };

        egui::Frame::new()
            .fill(fill)
            .stroke(egui::Stroke::new(1.0, stroke))
            .corner_radius(10)
            .inner_margin(egui::Margin::symmetric(10, 8))
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(entry.speaker.label())
                        .small()
                        .strong()
                        .color(label_color),
                );
                ui.add_space(1.0);
                ui.label(egui::RichText::new(&entry.text).color(text_color));
            });
        ui.add_space(5.0);
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
        self.ensure_theme(ctx);
        self.drain_events();
        self.drain_graph_updates();

        egui::TopBottomPanel::top("studio_header")
            .exact_height(42.0)
            .frame(
                egui::Frame::new()
                    .fill(studio_panel_tint())
                    .stroke(egui::Stroke::new(1.0, studio_border()))
                    .inner_margin(egui::Margin::symmetric(10, 6)),
            )
            .show(ctx, |ui| self.render_top_bar(ui));

        if self.chat_panel_expanded {
            egui::SidePanel::left("studio_chat_pane")
                .resizable(true)
                .default_width(305.0)
                .min_width(250.0)
                .max_width(420.0)
                .frame(
                    egui::Frame::new()
                        .fill(studio_surface())
                        .inner_margin(egui::Margin::symmetric(11, 10)),
                )
                .show(ctx, |ui| self.render_chat_pane(ui));
        } else {
            egui::SidePanel::left("studio_chat_rail")
                .resizable(false)
                .default_width(52.0)
                .min_width(52.0)
                .max_width(52.0)
                .frame(
                    egui::Frame::new()
                        .fill(studio_surface())
                        .inner_margin(egui::Margin::symmetric(6, 8)),
                )
                .show(ctx, |ui| self.render_chat_rail(ui));
        }

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(studio_surface())
                    .inner_margin(egui::Margin::symmetric(12, 10)),
            )
            .show(ctx, |ui| self.render_canvas_pane(ui));

        ctx.request_repaint_after(Duration::from_millis(120));
    }
}

fn truncate_ui_text(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }

    let mut clipped = text
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    clipped.push('…');
    clipped
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
    use std::fs;
    use std::path::PathBuf;
    use std::time::UNIX_EPOCH;

    use tokio::runtime::Handle;
    use tokio::sync::mpsc::unbounded_channel;
    use tokio::time::{Duration, timeout};

    use crate::config::{AgentSettings, ModelProvider};
    use crate::graph::watch::{GraphRefreshTrigger, GraphRefreshUpdate, spawn_graph_watch_worker};
    use crate::graph::{
        ArchitectureEdge, ArchitectureEdgeKind, ArchitectureGraph, ArchitectureNode,
        ArchitectureNodeKind,
    };
    use crate::test_support::{remove_dir_if_exists, temp_path};

    use super::{
        CanvasOp, CanvasState, GraphSurfaceState, MAX_GRAPH_UPDATES_PER_FRAME, StudioApp,
        StudioCommand, StudioEvent, build_highlight_node_ids, graph_change_delta,
        spawn_runtime_worker, summarize_for_canvas,
    };

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

    #[test]
    fn graph_surface_state_refresh_and_visualization_stay_isolated_from_shell() {
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
        let mut surface = GraphSurfaceState {
            impact_overlay_enabled: true,
            ..GraphSurfaceState::default()
        };

        surface.apply_refresh(Some(&previous), &current, "turn_completed");
        assert_eq!(
            surface.changed_target_ids,
            vec![
                "module:crate::tools".to_owned(),
                "module:crate::tools::parser".to_owned()
            ]
        );
        assert_eq!(surface.impact_target_ids, vec!["module:crate".to_owned()]);
        assert_eq!(
            surface.last_refresh_trigger.as_deref(),
            Some("turn_completed")
        );

        let mut canvas = CanvasState::default();
        canvas.apply(CanvasOp::set_scene_graph(current));
        surface.apply_visualization(&mut canvas);
        assert_eq!(
            canvas.highlighted_target_ids(),
            [
                "module:crate",
                "module:crate::tools",
                "module:crate::tools::parser"
            ]
        );
        assert_eq!(canvas.annotations().len(), 3);
        assert_eq!(
            surface.refresh_status_label(),
            "Canvas refreshed (2 changed nodes)"
        );
        assert_eq!(surface.last_trigger_label(), "turn_completed");
    }

    #[test]
    fn graph_surface_state_visualization_excludes_impact_without_overlay_toggle() {
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
        let mut surface = GraphSurfaceState::default();
        surface.apply_refresh(Some(&previous), &current, "files_changed");

        let mut canvas = CanvasState::default();
        canvas.apply(CanvasOp::set_scene_graph(current));
        surface.apply_visualization(&mut canvas);

        assert_eq!(
            canvas.highlighted_target_ids(),
            ["module:crate::tools", "module:crate::tools::parser"]
        );
        assert_eq!(canvas.annotations().len(), 1);
        assert_eq!(canvas.annotations()[0].id, "changed-summary");
    }

    #[test]
    fn graph_surface_state_last_trigger_label_defaults_before_first_refresh() {
        let mut surface = GraphSurfaceState::default();
        assert_eq!(surface.last_trigger_label(), "not yet refreshed");

        surface.last_refresh_trigger = Some("turn_completed".to_owned());
        assert_eq!(surface.last_trigger_label(), "turn_completed");
    }

    #[tokio::test]
    async fn runtime_worker_emits_failed_turn_and_turn_completion_graph_refresh() {
        let workspace_root = create_workspace_root("studio-runtime-flow");
        let settings = studio_test_settings(1);
        let (command_tx, command_rx) = unbounded_channel();
        let (event_tx, mut event_rx) = unbounded_channel();
        let runtime_handle = Handle::current();
        let (graph_watch_handle, mut graph_update_rx) =
            spawn_graph_watch_worker(&runtime_handle, workspace_root.clone());

        spawn_runtime_worker(
            &runtime_handle,
            settings,
            command_rx,
            event_tx,
            graph_watch_handle.clone(),
        );

        command_tx
            .send(StudioCommand::SubmitUserMessage {
                message: "hello".to_owned(),
            })
            .expect("command send should succeed");

        let started = timeout(Duration::from_secs(2), event_rx.recv())
            .await
            .expect("turn started should arrive within timeout")
            .expect("event channel should remain open");
        match started {
            StudioEvent::TurnStarted { message, .. } => assert_eq!(message, "hello"),
            other => panic!("expected TurnStarted event, got {other:?}"),
        }

        let failed = timeout(Duration::from_secs(2), event_rx.recv())
            .await
            .expect("turn failed should arrive within timeout")
            .expect("event channel should remain open");
        match failed {
            StudioEvent::TurnFailed { message, error } => {
                assert_eq!(message, "hello");
                assert!(error.contains("AGENT_MAX_INPUT_CHARS"));
            }
            other => panic!("expected TurnFailed event, got {other:?}"),
        }

        let turn_refresh = timeout(Duration::from_secs(3), async {
            loop {
                let update = graph_update_rx
                    .recv()
                    .await
                    .expect("graph update channel should remain open");
                if matches!(
                    update.trigger,
                    GraphRefreshTrigger::TurnCompleted
                        | GraphRefreshTrigger::TurnCompletedAndFilesChanged
                ) {
                    break update;
                }
            }
        })
        .await
        .expect("turn-completion graph refresh should arrive within timeout");
        assert!(matches!(
            turn_refresh.trigger,
            GraphRefreshTrigger::TurnCompleted | GraphRefreshTrigger::TurnCompletedAndFilesChanged
        ));

        graph_watch_handle.shutdown();
        remove_dir_if_exists(&workspace_root);
    }

    #[tokio::test]
    async fn drain_graph_updates_processes_bounded_batch_per_frame() {
        let workspace_root = create_workspace_root("studio-bounded-drain");
        let settings = studio_test_settings(8);
        let (command_tx, _command_rx) = unbounded_channel();
        let (_event_tx, event_rx) = unbounded_channel();
        let (graph_update_tx, graph_update_rx) = unbounded_channel();
        let runtime_handle = Handle::current();
        let (graph_watch_handle, _graph_watch_rx) =
            spawn_graph_watch_worker(&runtime_handle, workspace_root.clone());
        let mut app = StudioApp::new(
            settings,
            command_tx,
            event_rx,
            graph_update_rx,
            graph_watch_handle.clone(),
            workspace_root.clone(),
        );

        let total_updates = MAX_GRAPH_UPDATES_PER_FRAME + 2;
        for revision in 1..=(total_updates as u64) {
            graph_update_tx
                .send(GraphRefreshUpdate {
                    graph: graph_for_test(revision, &["module:crate"], &[]),
                    trigger: GraphRefreshTrigger::TurnCompleted,
                })
                .expect("graph update send should succeed");
        }

        app.drain_graph_updates();
        assert_eq!(
            app.canvas.graph().map(|graph| graph.revision),
            Some(MAX_GRAPH_UPDATES_PER_FRAME as u64)
        );

        app.drain_graph_updates();
        assert_eq!(
            app.canvas.graph().map(|graph| graph.revision),
            Some(total_updates as u64)
        );

        graph_watch_handle.shutdown();
        remove_dir_if_exists(&workspace_root);
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

    fn studio_test_settings(max_input_chars: u32) -> AgentSettings {
        AgentSettings {
            model_provider: ModelProvider::Ollama,
            model: "qwen2.5:3b".to_owned(),
            ollama_base_url: "http://127.0.0.1:9".to_owned(),
            openai_api_key: None,
            max_steps: 4,
            max_tool_calls: 4,
            max_tool_calls_per_step: 2,
            max_consecutive_tool_steps: 2,
            max_input_chars,
            max_output_chars: 2000,
            tool_timeout_ms: 100,
            fetch_url_max_bytes: 4096,
            fetch_url_follow_redirects: false,
            fetch_url_allowed_domains: vec!["example.com".to_owned()],
            notes_dir: "notes".to_owned(),
            save_note_allow_overwrite: false,
            model_timeout_ms: 100,
            model_max_retries: 0,
        }
    }

    fn create_workspace_root(prefix: &str) -> PathBuf {
        let root = temp_path(prefix);
        fs::create_dir_all(root.join("src")).expect("workspace src directory should be creatable");
        fs::write(root.join("src/lib.rs"), "pub fn seed() {}\n")
            .expect("workspace seed file should be writable");
        root
    }
}
