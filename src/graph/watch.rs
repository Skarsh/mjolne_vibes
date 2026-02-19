use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use anyhow::{Context, Result};
use tokio::runtime::Handle;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::time::{Duration, Instant, interval};
use tracing::{debug, warn};

use crate::graph::{ArchitectureGraph, build_rust_workspace_graph};

const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(400);
const DEFAULT_DEBOUNCE_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphRefreshTrigger {
    Startup,
    FilesChanged,
    TurnCompleted,
    TurnCompletedAndFilesChanged,
}

impl GraphRefreshTrigger {
    pub fn label(self) -> &'static str {
        match self {
            Self::Startup => "startup",
            Self::FilesChanged => "files_changed",
            Self::TurnCompleted => "turn_completed",
            Self::TurnCompletedAndFilesChanged => "turn_completed+files_changed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphRefreshUpdate {
    pub graph: ArchitectureGraph,
    pub trigger: GraphRefreshTrigger,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphWatchConfig {
    pub poll_interval: Duration,
    pub debounce_interval: Duration,
}

impl Default for GraphWatchConfig {
    fn default() -> Self {
        Self {
            poll_interval: DEFAULT_POLL_INTERVAL,
            debounce_interval: DEFAULT_DEBOUNCE_INTERVAL,
        }
    }
}

#[derive(Debug, Clone)]
pub struct GraphWatchHandle {
    command_tx: UnboundedSender<GraphWatchCommand>,
}

impl GraphWatchHandle {
    pub fn notify_turn_completed(&self) {
        let _ = self.command_tx.send(GraphWatchCommand::TurnCompleted);
    }

    pub fn shutdown(&self) {
        let _ = self.command_tx.send(GraphWatchCommand::Shutdown);
    }
}

#[derive(Debug)]
enum GraphWatchCommand {
    TurnCompleted,
    Shutdown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RustFileFingerprint {
    relative_path: String,
    modified_ms: u128,
    byte_len: u64,
}

pub fn spawn_graph_watch_worker(
    handle: &Handle,
    workspace_root: PathBuf,
) -> (GraphWatchHandle, UnboundedReceiver<GraphRefreshUpdate>) {
    spawn_graph_watch_worker_with_config(handle, workspace_root, GraphWatchConfig::default())
}

fn spawn_graph_watch_worker_with_config(
    handle: &Handle,
    workspace_root: PathBuf,
    config: GraphWatchConfig,
) -> (GraphWatchHandle, UnboundedReceiver<GraphRefreshUpdate>) {
    let (command_tx, command_rx) = unbounded_channel();
    let (update_tx, update_rx) = unbounded_channel();
    let watch_handle = GraphWatchHandle { command_tx };

    let _task = handle.spawn(run_graph_watch_loop(
        workspace_root,
        config,
        command_rx,
        update_tx,
    ));

    (watch_handle, update_rx)
}

async fn run_graph_watch_loop(
    workspace_root: PathBuf,
    config: GraphWatchConfig,
    mut command_rx: UnboundedReceiver<GraphWatchCommand>,
    update_tx: UnboundedSender<GraphRefreshUpdate>,
) {
    let mut revision: u64 = 0;
    let mut ticker = interval(config.poll_interval);
    let mut pending_trigger = Some(GraphRefreshTrigger::Startup);
    let mut refresh_deadline = Some(Instant::now() + config.debounce_interval);
    let mut last_fingerprint = match collect_workspace_fingerprint(&workspace_root) {
        Ok(fingerprint) => fingerprint,
        Err(error) => {
            warn!(
                root = %workspace_root.display(),
                error = %error,
                "failed to compute initial graph watch fingerprint"
            );
            Vec::new()
        }
    };

    loop {
        tokio::select! {
            maybe_command = command_rx.recv() => {
                match maybe_command {
                    Some(GraphWatchCommand::TurnCompleted) => {
                        pending_trigger = Some(merge_trigger(
                            pending_trigger,
                            GraphRefreshTrigger::TurnCompleted
                        ));
                        refresh_deadline = Some(Instant::now() + config.debounce_interval);
                    }
                    Some(GraphWatchCommand::Shutdown) | None => break,
                }
            }
            _ = ticker.tick() => {
                match collect_workspace_fingerprint(&workspace_root) {
                    Ok(fingerprint) => {
                        if fingerprint != last_fingerprint {
                            last_fingerprint = fingerprint;
                            pending_trigger = Some(merge_trigger(
                                pending_trigger,
                                GraphRefreshTrigger::FilesChanged
                            ));
                            refresh_deadline = Some(Instant::now() + config.debounce_interval);
                        }
                    }
                    Err(error) => {
                        warn!(
                            root = %workspace_root.display(),
                            error = %error,
                            "failed to collect graph watch fingerprint"
                        );
                    }
                }
            }
        }

        if let (Some(deadline), Some(trigger)) = (refresh_deadline, pending_trigger)
            && Instant::now() >= deadline
        {
            match build_rust_workspace_graph(&workspace_root, revision.saturating_add(1)) {
                Ok(graph) => {
                    revision = graph.revision;
                    if update_tx
                        .send(GraphRefreshUpdate { graph, trigger })
                        .is_err()
                    {
                        break;
                    }
                }
                Err(error) => {
                    warn!(
                        root = %workspace_root.display(),
                        trigger = trigger.label(),
                        error = %error,
                        "graph refresh failed"
                    );
                }
            }

            debug!(
                root = %workspace_root.display(),
                trigger = trigger.label(),
                revision,
                "graph refresh completed"
            );
            pending_trigger = None;
            refresh_deadline = None;
        }
    }
}

fn merge_trigger(
    existing: Option<GraphRefreshTrigger>,
    incoming: GraphRefreshTrigger,
) -> GraphRefreshTrigger {
    match (existing, incoming) {
        (None, next) => next,
        (Some(GraphRefreshTrigger::Startup), next) => next,
        (Some(GraphRefreshTrigger::FilesChanged), GraphRefreshTrigger::TurnCompleted)
        | (Some(GraphRefreshTrigger::TurnCompleted), GraphRefreshTrigger::FilesChanged)
        | (Some(GraphRefreshTrigger::TurnCompletedAndFilesChanged), _)
        | (_, GraphRefreshTrigger::TurnCompletedAndFilesChanged) => {
            GraphRefreshTrigger::TurnCompletedAndFilesChanged
        }
        (Some(current), GraphRefreshTrigger::Startup) => current,
        (Some(current), next) if current == next => current,
        (Some(_), next) => next,
    }
}

fn collect_workspace_fingerprint(workspace_root: &Path) -> Result<Vec<RustFileFingerprint>> {
    let mut files = Vec::new();
    collect_rust_files_recursive(workspace_root, workspace_root, &mut files)?;
    files.sort_by_key(|path| path_to_slash_string(path.as_path()));

    let mut fingerprint = Vec::with_capacity(files.len());
    for relative_path in files {
        let absolute_path = workspace_root.join(&relative_path);
        let metadata = fs::metadata(&absolute_path).with_context(|| {
            format!(
                "failed to read metadata for `{}`",
                absolute_path.as_path().display()
            )
        })?;
        let modified_ms = metadata
            .modified()
            .ok()
            .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
            .map(|value| value.as_millis())
            .unwrap_or(0);
        fingerprint.push(RustFileFingerprint {
            relative_path: path_to_slash_string(&relative_path),
            modified_ms,
            byte_len: metadata.len(),
        });
    }

    Ok(fingerprint)
}

fn collect_rust_files_recursive(
    workspace_root: &Path,
    current_dir: &Path,
    files: &mut Vec<PathBuf>,
) -> Result<()> {
    let mut entries = fs::read_dir(current_dir)
        .with_context(|| format!("failed to list directory `{}`", current_dir.display()))?
        .collect::<std::io::Result<Vec<_>>>()
        .with_context(|| format!("failed to read entries in `{}`", current_dir.display()))?;
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let path = entry.path();
        let file_type = entry
            .file_type()
            .with_context(|| format!("failed to inspect `{}`", path.display()))?;
        if file_type.is_dir() {
            let name = entry.file_name().to_string_lossy().to_string();
            if should_skip_dir(&name) {
                continue;
            }
            collect_rust_files_recursive(workspace_root, &path, files)?;
            continue;
        }

        if !file_type.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        let relative_path = path.strip_prefix(workspace_root).with_context(|| {
            format!(
                "failed to strip workspace root `{}` from `{}`",
                workspace_root.display(),
                path.display()
            )
        })?;
        files.push(relative_path.to_path_buf());
    }

    Ok(())
}

fn should_skip_dir(name: &str) -> bool {
    matches!(
        name,
        "target" | ".git" | ".idea" | ".vscode" | "node_modules"
    )
}

fn path_to_slash_string(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::Duration;

    use tokio::runtime::Handle;
    use tokio::time::timeout;

    use crate::test_support::{remove_dir_if_exists, temp_path};

    use super::{
        GraphRefreshTrigger, GraphWatchConfig, collect_workspace_fingerprint, merge_trigger,
        spawn_graph_watch_worker_with_config,
    };

    #[test]
    fn merge_trigger_combines_turn_and_file_updates() {
        assert_eq!(
            merge_trigger(
                Some(GraphRefreshTrigger::FilesChanged),
                GraphRefreshTrigger::TurnCompleted
            ),
            GraphRefreshTrigger::TurnCompletedAndFilesChanged
        );
        assert_eq!(
            merge_trigger(
                Some(GraphRefreshTrigger::TurnCompleted),
                GraphRefreshTrigger::FilesChanged
            ),
            GraphRefreshTrigger::TurnCompletedAndFilesChanged
        );
        assert_eq!(
            merge_trigger(
                Some(GraphRefreshTrigger::Startup),
                GraphRefreshTrigger::FilesChanged
            ),
            GraphRefreshTrigger::FilesChanged
        );
    }

    #[test]
    fn workspace_fingerprint_changes_after_file_update() {
        let root = temp_path("graph-watch-fingerprint");
        fs::create_dir_all(root.join("src")).expect("src should be created");
        fs::write(root.join("src/lib.rs"), "mod alpha;\n").expect("lib should be written");
        fs::write(root.join("src/alpha.rs"), "pub fn value() -> u8 { 1 }\n")
            .expect("alpha should be written");

        let before = collect_workspace_fingerprint(&root).expect("fingerprint should work");
        std::thread::sleep(Duration::from_millis(2));
        fs::write(root.join("src/alpha.rs"), "pub fn value() -> u8 { 2 }\n")
            .expect("alpha should be updated");
        let after = collect_workspace_fingerprint(&root).expect("fingerprint should work");
        assert_ne!(before, after);

        remove_dir_if_exists(&root);
    }

    #[tokio::test]
    async fn watch_worker_emits_startup_and_turn_completion_updates() {
        let root = temp_path("graph-watch-worker");
        fs::create_dir_all(root.join("src")).expect("src should be created");
        fs::write(root.join("src/lib.rs"), "mod alpha;\n").expect("lib should be written");
        fs::write(root.join("src/alpha.rs"), "pub fn value() -> u8 { 1 }\n")
            .expect("alpha should be written");

        let (watch_handle, mut update_rx) = spawn_graph_watch_worker_with_config(
            &Handle::current(),
            root.clone(),
            GraphWatchConfig {
                poll_interval: Duration::from_millis(25),
                debounce_interval: Duration::from_millis(40),
            },
        );

        let startup = timeout(Duration::from_secs(2), update_rx.recv())
            .await
            .expect("startup update should arrive")
            .expect("startup update should be present");
        assert_eq!(startup.trigger, GraphRefreshTrigger::Startup);

        watch_handle.notify_turn_completed();
        let turn = timeout(Duration::from_secs(2), update_rx.recv())
            .await
            .expect("turn update should arrive")
            .expect("turn update should be present");
        assert_eq!(turn.trigger, GraphRefreshTrigger::TurnCompleted);

        watch_handle.shutdown();
        remove_dir_if_exists(&root);
    }
}
