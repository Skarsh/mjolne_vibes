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
use crate::graph::watch::{
    GraphRefreshTrigger, GraphRefreshUpdate, GraphWatchHandle, spawn_graph_watch_worker,
};

pub mod canvas;
pub mod events;
pub mod renderer;

use self::canvas::{
    CanvasState, CanvasSurfaceAdapter, CanvasSurfaceAdapterKind, CanvasToolCard, CanvasViewport,
    GraphSurfaceAdapterOptions,
};
use self::events::{CanvasOp, StudioCommand, StudioEvent, StudioTurnResult};
use self::renderer::{
    ArchitectureActivitySummary, ArchitectureOverviewRenderInput, ArchitectureOverviewRenderer,
    SubsystemMapper,
};

const APP_TITLE: &str = "mjolne_vibes studio";
const MAX_CANVAS_SUMMARIES: usize = 24;
const MAX_CANVAS_TOOL_CARDS: usize = 16;
const MAX_TURN_SNAPSHOTS: usize = 24;
const CANVAS_PREVIEW_CHAR_LIMIT: usize = 180;
const MAX_IMPACT_NODE_ANNOTATIONS: usize = 12;
const MAX_GRAPH_UPDATES_PER_FRAME: usize = 4;

fn studio_text() -> egui::Color32 {
    egui::Color32::from_rgb(19, 29, 40)
}

fn studio_muted_text() -> egui::Color32 {
    egui::Color32::from_rgb(94, 109, 127)
}

fn studio_app_bg() -> egui::Color32 {
    egui::Color32::from_rgb(219, 231, 243)
}

fn studio_panel_surface() -> egui::Color32 {
    egui::Color32::from_rgb(238, 246, 252)
}

fn studio_panel_surface_alt() -> egui::Color32 {
    egui::Color32::from_rgb(246, 251, 255)
}

fn studio_stage_surface() -> egui::Color32 {
    egui::Color32::from_rgb(230, 240, 249)
}

fn studio_panel_tint() -> egui::Color32 {
    egui::Color32::from_rgb(210, 226, 242)
}

fn studio_border() -> egui::Color32 {
    egui::Color32::from_rgb(153, 179, 208)
}

fn studio_border_strong() -> egui::Color32 {
    egui::Color32::from_rgb(98, 137, 176)
}

fn studio_accent() -> egui::Color32 {
    egui::Color32::from_rgb(16, 112, 165)
}

fn studio_accent_soft() -> egui::Color32 {
    egui::Color32::from_rgb(206, 229, 247)
}

fn studio_mode_active() -> egui::Color32 {
    egui::Color32::from_rgb(24, 129, 187)
}

fn studio_mode_inactive() -> egui::Color32 {
    egui::Color32::from_rgb(226, 236, 246)
}

pub fn run_studio(settings: &AgentSettings) -> Result<()> {
    let runtime_handle = Handle::try_current().context("studio requires a tokio runtime")?;
    let workspace_root =
        std::env::current_dir().context("failed to resolve workspace root for studio")?;
    let subsystem_mapper = load_subsystem_mapper(settings, &workspace_root)?;

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
                subsystem_mapper,
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

fn load_subsystem_mapper(
    settings: &AgentSettings,
    workspace_root: &std::path::Path,
) -> Result<SubsystemMapper> {
    let Some(path) = settings.studio_subsystem_rules_file.as_deref() else {
        return Ok(SubsystemMapper::default());
    };

    let configured_path = PathBuf::from(path);
    let resolved_path = if configured_path.is_absolute() {
        configured_path
    } else {
        workspace_root.join(configured_path)
    };
    let mapper = SubsystemMapper::from_rules_file(&resolved_path).with_context(|| {
        format!(
            "failed to load STUDIO_SUBSYSTEM_RULES_FILE from {}",
            resolved_path.display()
        )
    })?;
    info!(
        rules_file = %resolved_path.display(),
        rule_count = mapper.rule_count(),
        "loaded studio subsystem mapping rules"
    );
    Ok(mapper)
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CanvasDiffMode {
    Live,
    BeforeAfterLatestTurn,
    FocusLatestTurn,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingTurnSnapshot {
    turn_id: u64,
    started_at: SystemTime,
    baseline_graph: Option<ArchitectureGraph>,
    intent_target_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CanvasTurnSnapshot {
    turn_id: u64,
    started_at: SystemTime,
    completed_at: SystemTime,
    baseline_revision: Option<u64>,
    outcome_revision: u64,
    changed_target_ids: Vec<String>,
    impact_target_ids: Vec<String>,
    intent_target_ids: Vec<String>,
    baseline_graph: Option<ArchitectureGraph>,
    outcome_graph: ArchitectureGraph,
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

    #[cfg(test)]
    fn last_trigger_label(&self) -> &str {
        self.last_refresh_trigger
            .as_deref()
            .unwrap_or("not yet refreshed")
    }
}

struct StudioApp {
    settings: AgentSettings,
    workspace_root: PathBuf,
    subsystem_mapper: SubsystemMapper,
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
    next_turn_snapshot_id: u64,
    pending_turn_snapshot: Option<PendingTurnSnapshot>,
    turn_snapshots: Vec<CanvasTurnSnapshot>,
    selected_snapshot_index: Option<usize>,
    snapshot_transition_pulse: bool,
    canvas_diff_mode: CanvasDiffMode,
    turn_summaries: Vec<CanvasTurnSummary>,
    theme_applied: bool,
    turn_in_flight: bool,
    runtime_disconnected: bool,
    graph_watch_disconnected: bool,
}

impl StudioApp {
    fn new(
        settings: AgentSettings,
        subsystem_mapper: SubsystemMapper,
        command_tx: UnboundedSender<StudioCommand>,
        event_rx: UnboundedReceiver<StudioEvent>,
        graph_update_rx: UnboundedReceiver<GraphRefreshUpdate>,
        graph_watch_handle: GraphWatchHandle,
        workspace_root: PathBuf,
    ) -> Self {
        Self {
            settings,
            workspace_root,
            subsystem_mapper,
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
            next_turn_snapshot_id: 1,
            pending_turn_snapshot: None,
            turn_snapshots: Vec::new(),
            selected_snapshot_index: None,
            snapshot_transition_pulse: false,
            canvas_diff_mode: CanvasDiffMode::Live,
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
            egui::FontId::new(24.0, egui::FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Button,
            egui::FontId::new(13.4, egui::FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Body,
            egui::FontId::new(14.1, egui::FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Small,
            egui::FontId::new(11.2, egui::FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Monospace,
            egui::FontId::new(11.5, egui::FontFamily::Monospace),
        );

        let mut visuals = egui::Visuals::light();
        visuals.override_text_color = Some(studio_text());
        visuals.panel_fill = studio_app_bg();
        visuals.extreme_bg_color = egui::Color32::from_rgb(255, 255, 255);
        visuals.faint_bg_color = studio_panel_tint();
        visuals.code_bg_color = egui::Color32::from_rgb(233, 242, 251);
        visuals.window_fill = studio_panel_surface_alt();
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
        visuals.widgets.noninteractive.bg_fill = studio_panel_surface_alt();
        visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, studio_text());
        visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(236, 245, 253);
        visuals.widgets.inactive.weak_bg_fill = egui::Color32::from_rgb(229, 240, 250);
        visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(220, 235, 247);
        visuals.widgets.active.bg_fill = egui::Color32::from_rgb(207, 227, 244);
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
            .fill(studio_panel_surface_alt())
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
            let accent_rect =
                egui::Rect::from_min_size(ui.next_widget_position(), egui::vec2(3.0, 22.0));
            ui.painter()
                .rect_filled(accent_rect, 2.0, studio_accent().gamma_multiply(0.9));
            ui.add_space(8.0);
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
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new("Studio")
                    .heading()
                    .strong()
                    .color(studio_text()),
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
        ui.add_space(2.0);
        ui.horizontal_wrapped(|ui| {
            Self::chip(
                ui,
                format!("{} / {}", self.settings.model_provider, self.settings.model),
                egui::Color32::from_rgb(231, 243, 252),
                studio_border(),
                studio_muted_text(),
            );
            let refresh = self
                .graph_surface
                .last_refresh_trigger
                .as_deref()
                .unwrap_or("not yet refreshed");
            Self::chip(
                ui,
                format!("graph {refresh}"),
                egui::Color32::from_rgb(235, 242, 250),
                studio_border(),
                studio_muted_text(),
            );
            Self::chip(
                ui,
                truncate_ui_text(&self.canvas_status, 52),
                egui::Color32::from_rgb(233, 246, 240),
                egui::Color32::from_rgb(143, 184, 167),
                egui::Color32::from_rgb(35, 104, 81),
            );
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
        let prior_graph = self.canvas.graph().cloned();
        let trigger = update.trigger.label().to_owned();
        self.graph_surface
            .apply_refresh(prior_graph.as_ref(), &update.graph, &trigger);
        self.canvas
            .apply(CanvasOp::set_scene_graph(update.graph.clone()));

        if matches!(
            update.trigger,
            GraphRefreshTrigger::TurnCompleted | GraphRefreshTrigger::TurnCompletedAndFilesChanged
        ) {
            self.maybe_finalize_turn_snapshot(update.graph, SystemTime::now());
        }

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
        let selected_snapshot = self.selected_snapshot().cloned();
        let overlay_snapshot = if self.canvas_diff_mode == CanvasDiffMode::BeforeAfterLatestTurn {
            selected_snapshot.as_ref()
        } else {
            None
        };
        let focus_snapshot = if self.canvas_diff_mode == CanvasDiffMode::FocusLatestTurn {
            selected_snapshot.as_ref()
        } else {
            None
        };
        let mode_snapshot = overlay_snapshot.or(focus_snapshot);
        let effective_changed = mode_snapshot
            .map(|snapshot| snapshot.changed_target_ids.as_slice())
            .unwrap_or(self.graph_surface.changed_target_ids.as_slice());
        let effective_impact = mode_snapshot
            .map(|snapshot| snapshot.impact_target_ids.as_slice())
            .unwrap_or(self.graph_surface.impact_target_ids.as_slice());
        let recent_activity = self
            .turn_summaries
            .iter()
            .map(|summary| ArchitectureActivitySummary {
                user_message: summary.user_message.as_str(),
                assistant_preview: summary.assistant_preview.as_str(),
                tool_call_count: summary.tool_call_count,
            })
            .collect::<Vec<_>>();

        self.next_draw_command_sequence = self.next_draw_command_sequence.saturating_add(1);
        let batch = ArchitectureOverviewRenderer::render(ArchitectureOverviewRenderInput {
            graph: &graph,
            subsystem_mapper: &self.subsystem_mapper,
            changed_target_ids: effective_changed,
            impact_target_ids: effective_impact,
            show_impact_overlay: self.graph_surface.impact_overlay_enabled,
            before_graph: overlay_snapshot.and_then(|snapshot| snapshot.baseline_graph.as_ref()),
            show_before_after_overlay: overlay_snapshot.is_some(),
            show_focus_mode: self.canvas_diff_mode == CanvasDiffMode::FocusLatestTurn
                && mode_snapshot.is_some(),
            tool_cards: &self.canvas_tool_cards,
            turn_in_flight: self.turn_in_flight,
            canvas_status: &self.canvas_status,
            recent_activity: &recent_activity,
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
                self.canvas_status =
                    format!("Running turn for: {}", summarize_for_canvas(&message));
                self.pending_turn_snapshot = Some(PendingTurnSnapshot {
                    turn_id: self.next_turn_snapshot_id,
                    started_at,
                    baseline_graph: self.canvas.graph().cloned(),
                    intent_target_ids: Vec::new(),
                });
                self.next_turn_snapshot_id = self.next_turn_snapshot_id.saturating_add(1);
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
                self.pending_turn_snapshot = None;
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
        let compact_width = ui.available_width() < 320.0;
        let composer_section_height = if compact_width { 170.0 } else { 188.0 };

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
            if !compact_width {
                let workspace_label =
                    truncate_ui_text(&self.workspace_root.display().to_string(), 38);
                Self::chip(
                    ui,
                    format!("root {workspace_label}"),
                    egui::Color32::from_rgb(239, 246, 252),
                    studio_border(),
                    studio_muted_text(),
                );
            }
        });

        Self::card_frame(ui).show(ui, |ui| {
            egui::ScrollArea::vertical()
                .stick_to_bottom(true)
                .max_height((ui.available_height() - composer_section_height).max(140.0))
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
                .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(27, 84, 136)))
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
        let compact_toolbar = ui.available_width() < 840.0;
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Canvas")
                    .heading()
                    .strong()
                    .color(studio_text()),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                egui::Frame::new()
                    .fill(egui::Color32::from_rgb(214, 229, 243))
                    .stroke(egui::Stroke::new(1.0, studio_border()))
                    .corner_radius(11)
                    .inner_margin(egui::Margin::symmetric(7, 4))
                    .show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            if ui.button("Fit").clicked() {
                                self.canvas_viewport.fit_to_view();
                            }
                            let has_snapshots = !self.turn_snapshots.is_empty();
                            let selected_index = self.selected_snapshot_index();
                            if ui
                                .add_enabled(has_snapshots, egui::Button::new("←"))
                                .clicked()
                            {
                                self.select_previous_snapshot();
                                self.render_architecture_overview_scene();
                            }
                            let snapshot_label = if let Some(index) = selected_index {
                                format!("{}/{}", index + 1, self.turn_snapshots.len())
                            } else {
                                "0/0".to_owned()
                            };
                            ui.label(
                                egui::RichText::new(snapshot_label)
                                    .small()
                                    .strong()
                                    .color(studio_muted_text()),
                            );
                            if ui
                                .add_enabled(has_snapshots, egui::Button::new("→"))
                                .clicked()
                            {
                                self.select_next_snapshot();
                                self.render_architecture_overview_scene();
                            }
                            let before_after_selected =
                                self.canvas_diff_mode == CanvasDiffMode::BeforeAfterLatestTurn;
                            let before_after_label = if before_after_selected {
                                if compact_toolbar {
                                    "B/A On"
                                } else {
                                    "Before/After On"
                                }
                            } else if compact_toolbar {
                                "B/A"
                            } else {
                                "Before/After"
                            };
                            if self
                                .mode_toggle_button(ui, before_after_label, before_after_selected)
                                .clicked()
                            {
                                self.canvas_diff_mode = if before_after_selected {
                                    CanvasDiffMode::Live
                                } else {
                                    CanvasDiffMode::BeforeAfterLatestTurn
                                };
                                self.render_architecture_overview_scene();
                            }
                            let focus_selected =
                                self.canvas_diff_mode == CanvasDiffMode::FocusLatestTurn;
                            let focus_label = if focus_selected {
                                if compact_toolbar { "F On" } else { "Focus On" }
                            } else {
                                "Focus"
                            };
                            if self
                                .mode_toggle_button(ui, focus_label, focus_selected)
                                .clicked()
                            {
                                self.canvas_diff_mode = if focus_selected {
                                    CanvasDiffMode::Live
                                } else {
                                    CanvasDiffMode::FocusLatestTurn
                                };
                                self.render_architecture_overview_scene();
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
        if let Some(snapshot) = self.selected_snapshot() {
            let pulse = ui.ctx().animate_bool(
                ui.id().with("snapshot-transition-pulse"),
                self.snapshot_transition_pulse,
            );
            let pulse_fill =
                egui::Color32::from_rgb(220, 239, 253).gamma_multiply(0.9 + pulse * 0.2);
            let pulse_stroke = if pulse > 0.01 {
                studio_accent().gamma_multiply(0.72 + pulse * 0.28)
            } else {
                studio_border()
            };
            ui.horizontal_wrapped(|ui| {
                Self::chip(
                    ui,
                    format!("turn {}", snapshot.turn_id),
                    pulse_fill,
                    pulse_stroke,
                    studio_muted_text(),
                );
                let baseline = snapshot
                    .baseline_revision
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "n/a".to_owned());
                Self::chip(
                    ui,
                    format!("rev {baseline} → {}", snapshot.outcome_revision),
                    egui::Color32::from_rgb(236, 245, 253),
                    studio_border(),
                    studio_muted_text(),
                );
                Self::chip(
                    ui,
                    format!("changed {}", snapshot.changed_target_ids.len()),
                    egui::Color32::from_rgb(255, 237, 218),
                    egui::Color32::from_rgb(223, 167, 108),
                    egui::Color32::from_rgb(153, 100, 34),
                );
                Self::chip(
                    ui,
                    format!("impact {}", snapshot.impact_target_ids.len()),
                    egui::Color32::from_rgb(224, 240, 251),
                    egui::Color32::from_rgb(146, 184, 214),
                    egui::Color32::from_rgb(43, 97, 140),
                );
            });
        }

        let surface_height = ui.available_height().max(240.0);
        egui::Frame::new()
            .fill(studio_stage_surface())
            .stroke(egui::Stroke::new(1.0, studio_border_strong()))
            .corner_radius(12)
            .inner_margin(egui::Margin::symmetric(8, 8))
            .show(ui, |ui| self.render_canvas_surface(ui, surface_height));
    }

    fn mode_toggle_button(&self, ui: &mut egui::Ui, label: &str, selected: bool) -> egui::Response {
        let anim = ui
            .ctx()
            .animate_bool(ui.id().with(format!("mode-{label}")), selected);
        let fill = if selected {
            studio_mode_active().gamma_multiply(0.65 + (0.35 * anim))
        } else {
            studio_mode_inactive()
        };
        let text_color = if selected {
            egui::Color32::from_rgb(247, 252, 255)
        } else {
            studio_text()
        };
        let stroke = if selected {
            studio_accent()
        } else {
            studio_border_strong()
        };
        ui.add(
            egui::Button::new(
                egui::RichText::new(label)
                    .small()
                    .strong()
                    .color(text_color),
            )
            .fill(fill)
            .stroke(egui::Stroke::new(1.0, stroke)),
        )
    }

    fn maybe_finalize_turn_snapshot(
        &mut self,
        outcome_graph: ArchitectureGraph,
        completed_at: SystemTime,
    ) {
        let Some(pending) = self.pending_turn_snapshot.take() else {
            return;
        };
        let snapshot = CanvasTurnSnapshot {
            turn_id: pending.turn_id,
            started_at: pending.started_at,
            completed_at,
            baseline_revision: pending.baseline_graph.as_ref().map(|graph| graph.revision),
            outcome_revision: outcome_graph.revision,
            changed_target_ids: self.graph_surface.changed_target_ids.clone(),
            impact_target_ids: self.graph_surface.impact_target_ids.clone(),
            intent_target_ids: pending.intent_target_ids,
            baseline_graph: pending.baseline_graph,
            outcome_graph,
        };
        self.turn_snapshots.push(snapshot);
        if self.turn_snapshots.len() > MAX_TURN_SNAPSHOTS {
            let extra = self.turn_snapshots.len() - MAX_TURN_SNAPSHOTS;
            self.turn_snapshots.drain(0..extra);
        }
        self.selected_snapshot_index = self.turn_snapshots.len().checked_sub(1);
        self.bump_snapshot_transition();
    }

    fn selected_snapshot_index(&self) -> Option<usize> {
        let last_index = self.turn_snapshots.len().checked_sub(1)?;
        Some(
            self.selected_snapshot_index
                .unwrap_or(last_index)
                .min(last_index),
        )
    }

    fn selected_snapshot(&self) -> Option<&CanvasTurnSnapshot> {
        let index = self.selected_snapshot_index()?;
        self.turn_snapshots.get(index)
    }

    fn select_previous_snapshot(&mut self) {
        let Some(current) = self.selected_snapshot_index() else {
            return;
        };
        let next = current.saturating_sub(1);
        if next != current {
            self.selected_snapshot_index = Some(next);
            self.bump_snapshot_transition();
        }
    }

    fn select_next_snapshot(&mut self) {
        let Some(current) = self.selected_snapshot_index() else {
            return;
        };
        let last_index = self.turn_snapshots.len().saturating_sub(1);
        let next = (current + 1).min(last_index);
        if next != current {
            self.selected_snapshot_index = Some(next);
            self.bump_snapshot_transition();
        }
    }

    fn bump_snapshot_transition(&mut self) {
        self.snapshot_transition_pulse = !self.snapshot_transition_pulse;
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
            .exact_height(78.0)
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
                        .fill(studio_panel_surface())
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
                        .fill(studio_panel_surface())
                        .inner_margin(egui::Margin::symmetric(6, 8)),
                )
                .show(ctx, |ui| self.render_chat_rail(ui));
        }

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(studio_panel_surface())
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
        CanvasDiffMode, CanvasOp, CanvasState, CanvasTurnSnapshot, GraphSurfaceState,
        MAX_GRAPH_UPDATES_PER_FRAME, PendingTurnSnapshot, StudioApp, StudioCommand, StudioEvent,
        SubsystemMapper, build_highlight_node_ids, graph_change_delta, spawn_runtime_worker,
        summarize_for_canvas,
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
    async fn snapshot_navigation_moves_selection_within_bounds() {
        let workspace_root = create_workspace_root("studio-snapshot-selection");
        let (command_tx, _command_rx) = unbounded_channel();
        let (_event_tx, event_rx) = unbounded_channel();
        let (_graph_update_tx, graph_update_rx) = unbounded_channel();
        let runtime_handle = Handle::current();
        let (graph_watch_handle, _graph_watch_rx) =
            spawn_graph_watch_worker(&runtime_handle, workspace_root.clone());
        let mut app = StudioApp::new(
            studio_test_settings(8),
            SubsystemMapper::default(),
            command_tx,
            event_rx,
            graph_update_rx,
            graph_watch_handle.clone(),
            workspace_root.clone(),
        );
        app.selected_snapshot_index = Some(1);
        app.turn_snapshots = vec![
            CanvasTurnSnapshot {
                turn_id: 1,
                started_at: UNIX_EPOCH,
                completed_at: UNIX_EPOCH,
                baseline_revision: Some(1),
                outcome_revision: 2,
                changed_target_ids: vec![],
                impact_target_ids: vec![],
                intent_target_ids: vec![],
                baseline_graph: None,
                outcome_graph: graph_for_test(2, &["module:crate"], &[]),
            },
            CanvasTurnSnapshot {
                turn_id: 2,
                started_at: UNIX_EPOCH,
                completed_at: UNIX_EPOCH,
                baseline_revision: Some(2),
                outcome_revision: 3,
                changed_target_ids: vec![],
                impact_target_ids: vec![],
                intent_target_ids: vec![],
                baseline_graph: None,
                outcome_graph: graph_for_test(3, &["module:crate"], &[]),
            },
        ];

        app.select_previous_snapshot();
        assert_eq!(app.selected_snapshot_index(), Some(0));

        app.select_previous_snapshot();
        assert_eq!(app.selected_snapshot_index(), Some(0));

        app.select_next_snapshot();
        assert_eq!(app.selected_snapshot_index(), Some(1));

        app.select_next_snapshot();
        assert_eq!(app.selected_snapshot_index(), Some(1));

        graph_watch_handle.shutdown();
        remove_dir_if_exists(&workspace_root);
    }

    #[tokio::test]
    async fn maybe_finalize_turn_snapshot_records_baseline_and_outcome() {
        let workspace_root = create_workspace_root("studio-snapshot-record");
        let settings = studio_test_settings(8);
        let (command_tx, _command_rx) = unbounded_channel();
        let (_event_tx, event_rx) = unbounded_channel();
        let (_graph_update_tx, graph_update_rx) = unbounded_channel();
        let runtime_handle = Handle::current();
        let (graph_watch_handle, _graph_watch_rx) =
            spawn_graph_watch_worker(&runtime_handle, workspace_root.clone());
        let mut app = StudioApp::new(
            settings,
            SubsystemMapper::default(),
            command_tx,
            event_rx,
            graph_update_rx,
            graph_watch_handle.clone(),
            workspace_root.clone(),
        );

        let baseline = graph_for_test(1, &["module:crate"], &[]);
        app.graph_surface.changed_target_ids = vec!["module:crate::tools".to_owned()];
        app.graph_surface.impact_target_ids = vec!["module:crate".to_owned()];
        app.pending_turn_snapshot = Some(PendingTurnSnapshot {
            turn_id: 9,
            started_at: UNIX_EPOCH,
            baseline_graph: Some(baseline.clone()),
            intent_target_ids: vec!["module:crate::tools".to_owned()],
        });

        let outcome = graph_for_test(2, &["module:crate", "module:crate::tools"], &[]);
        app.maybe_finalize_turn_snapshot(outcome.clone(), UNIX_EPOCH);

        assert_eq!(app.turn_snapshots.len(), 1);
        let snapshot = &app.turn_snapshots[0];
        assert_eq!(snapshot.turn_id, 9);
        assert_eq!(snapshot.baseline_revision, Some(1));
        assert_eq!(snapshot.outcome_revision, 2);
        assert_eq!(snapshot.changed_target_ids, ["module:crate::tools"]);
        assert_eq!(snapshot.impact_target_ids, ["module:crate"]);
        assert_eq!(snapshot.intent_target_ids, ["module:crate::tools"]);
        assert_eq!(snapshot.baseline_graph.as_ref(), Some(&baseline));
        assert_eq!(snapshot.outcome_graph, outcome);

        graph_watch_handle.shutdown();
        remove_dir_if_exists(&workspace_root);
    }

    #[tokio::test]
    async fn render_architecture_scene_emits_before_after_overlay_when_enabled() {
        let workspace_root = create_workspace_root("studio-overlay-mode");
        let settings = studio_test_settings(8);
        let (command_tx, _command_rx) = unbounded_channel();
        let (_event_tx, event_rx) = unbounded_channel();
        let (_graph_update_tx, graph_update_rx) = unbounded_channel();
        let runtime_handle = Handle::current();
        let (graph_watch_handle, _graph_watch_rx) =
            spawn_graph_watch_worker(&runtime_handle, workspace_root.clone());
        let mut app = StudioApp::new(
            settings,
            SubsystemMapper::default(),
            command_tx,
            event_rx,
            graph_update_rx,
            graph_watch_handle.clone(),
            workspace_root.clone(),
        );

        let baseline = graph_for_test(1, &["module:crate"], &[]);
        let outcome = graph_for_test(2, &["module:crate", "module:crate::tools"], &[]);
        app.canvas.apply(CanvasOp::set_scene_graph(outcome));
        app.graph_surface.changed_target_ids = vec!["module:crate::tools".to_owned()];
        app.graph_surface.impact_target_ids = vec!["module:crate".to_owned()];
        app.canvas_diff_mode = CanvasDiffMode::BeforeAfterLatestTurn;
        app.turn_snapshots.push(super::CanvasTurnSnapshot {
            turn_id: 1,
            started_at: UNIX_EPOCH,
            completed_at: UNIX_EPOCH,
            baseline_revision: Some(1),
            outcome_revision: 2,
            changed_target_ids: vec!["module:crate::tools".to_owned()],
            impact_target_ids: vec!["module:crate".to_owned()],
            intent_target_ids: Vec::new(),
            baseline_graph: Some(baseline),
            outcome_graph: graph_for_test(2, &["module:crate", "module:crate::tools"], &[]),
        });

        app.render_architecture_overview_scene();

        let has_overlay_summary = app
            .canvas
            .draw_scene()
            .shapes()
            .into_iter()
            .any(|shape| shape.id == "overlay:before-after-summary");
        assert!(has_overlay_summary);

        graph_watch_handle.shutdown();
        remove_dir_if_exists(&workspace_root);
    }

    #[tokio::test]
    async fn render_architecture_scene_focus_mode_dims_unchanged_nodes() {
        let workspace_root = create_workspace_root("studio-focus-mode");
        let settings = studio_test_settings(8);
        let (command_tx, _command_rx) = unbounded_channel();
        let (_event_tx, event_rx) = unbounded_channel();
        let (_graph_update_tx, graph_update_rx) = unbounded_channel();
        let runtime_handle = Handle::current();
        let (graph_watch_handle, _graph_watch_rx) =
            spawn_graph_watch_worker(&runtime_handle, workspace_root.clone());
        let mut app = StudioApp::new(
            settings,
            SubsystemMapper::default(),
            command_tx,
            event_rx,
            graph_update_rx,
            graph_watch_handle.clone(),
            workspace_root.clone(),
        );

        app.canvas.apply(CanvasOp::set_scene_graph(graph_for_test(
            3,
            &["module:crate", "module:crate::tools"],
            &[],
        )));
        app.canvas_diff_mode = CanvasDiffMode::FocusLatestTurn;
        app.turn_snapshots.push(super::CanvasTurnSnapshot {
            turn_id: 1,
            started_at: UNIX_EPOCH,
            completed_at: UNIX_EPOCH,
            baseline_revision: Some(2),
            outcome_revision: 3,
            changed_target_ids: vec!["module:crate::tools".to_owned()],
            impact_target_ids: Vec::new(),
            intent_target_ids: Vec::new(),
            baseline_graph: Some(graph_for_test(2, &["module:crate"], &[])),
            outcome_graph: graph_for_test(3, &["module:crate", "module:crate::tools"], &[]),
        });

        app.render_architecture_overview_scene();

        let unchanged = app
            .canvas
            .draw_scene()
            .shapes()
            .into_iter()
            .find(|shape| shape.id == "node:module:crate")
            .and_then(|shape| shape.style.fill_color.as_deref());
        assert_eq!(unchanged, Some("#d9e2ec"));

        graph_watch_handle.shutdown();
        remove_dir_if_exists(&workspace_root);
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
            SubsystemMapper::default(),
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
        assert_eq!(
            app.canvas.draw_scene().last_sequence(),
            Some(MAX_GRAPH_UPDATES_PER_FRAME as u64)
        );

        app.drain_graph_updates();
        assert_eq!(
            app.canvas.graph().map(|graph| graph.revision),
            Some(total_updates as u64)
        );
        assert_eq!(
            app.canvas.draw_scene().last_sequence(),
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
            studio_subsystem_rules_file: None,
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
