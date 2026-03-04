//! Application state, orchestration, and command loop.
//!
//! [`App`] is the top-level `eframe::App` implementation.  All UI code
//! delegates to [`crate::ui::main_window::show`]; the business logic lives
//! in [`command_loop`].

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Instant,
};

use tokio::{
    runtime::Handle,
    sync::mpsc::{self, UnboundedReceiver, UnboundedSender},
};
use tracing::{error, info};

use crate::{
    config::settings::{RecordingSettings, load_settings},
    error::AppError,
    ui::main_window,
};

// ---------------------------------------------------------------------------
// RecordingStatus
// ---------------------------------------------------------------------------

/// Observed execution state of the recording pipeline.
#[derive(Debug, Clone, Default)]
pub enum RecordingStatus {
    /// No recording in progress; idle UI.
    #[default]
    Idle,
    /// Capture pipeline is running.
    Recording {
        /// Monotonic instant at which recording was started.
        started_at: Instant,
    },
    /// Recording has stopped; encoded file is ready to preview/trim.
    Previewing,
    /// Accepted — file is being moved to the output directory.
    Saving,
}

impl RecordingStatus {
    /// Returns `true` while the capture pipeline is running.
    #[must_use]
    pub const fn is_recording(&self) -> bool {
        matches!(self, Self::Recording { .. })
    }

    /// Returns `true` when the recorder is in the default [`Self::Idle`] state.
    #[must_use]
    pub const fn is_idle(&self) -> bool {
        matches!(self, Self::Idle)
    }
}

// ---------------------------------------------------------------------------
// RecorderCommand
// ---------------------------------------------------------------------------

/// Commands sent from UI event handlers to the async [`command_loop`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecorderCommand {
    /// Begin a capture session using the current settings.
    Start,
    /// Stop an in-progress capture and transition to preview mode.
    Stop,
    /// Accept the preview — persist the file to `output_dir`.
    Accept,
    /// Discard the preview — delete the temporary file and return to idle.
    Discard,
}

// ---------------------------------------------------------------------------
// RecordingOrchestrator (Phase 3 placeholder)
// ---------------------------------------------------------------------------

/// Coordinates the capture and encoding pipelines for a single recording session.
///
/// This is a placeholder struct that will be expanded in Phase 3
/// (see `src/capture/engine.rs` and `src/encode/pipeline.rs`).
pub struct RecordingOrchestrator {
    _private: (),
}

impl RecordingOrchestrator {
    /// Creates a new orchestrator bound to the provided settings.
    ///
    /// Expanded to start the actual `SCStream` in Phase 3.
    #[must_use]
    pub const fn new(_settings: &RecordingSettings) -> Self {
        Self { _private: () }
    }
}

// ---------------------------------------------------------------------------
// AppState
// ---------------------------------------------------------------------------

/// Shared application state.
///
/// Protected by an `Arc<Mutex<…>>` so it can be accessed from both the egui
/// render thread and the async command loop.
#[derive(Default)]
pub struct AppState {
    /// User-facing recording configuration.
    pub settings: RecordingSettings,
    /// Current recording pipeline state.
    pub recording_status: RecordingStatus,
    /// Active orchestrator, present only while a session is in progress.
    pub orchestrator: Option<RecordingOrchestrator>,
    /// Path to the temporary encoded file, populated during preview.
    pub preview_path: Option<PathBuf>,
    /// Most recent non-fatal error, displayed in the UI.
    pub last_error: Option<AppError>,
}

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

/// Top-level `eframe` application struct.
pub struct App {
    /// Shared application state; also held by the async command loop.
    pub state: Arc<Mutex<AppState>>,
    /// Sender half of the command channel — given to UI widgets.
    pub cmd_tx: UnboundedSender<RecorderCommand>,
}

impl App {
    /// Creates a new `App`, loading persisted settings and starting the async
    /// command loop on the provided Tokio runtime handle.
    #[must_use]
    pub fn new(_cc: &eframe::CreationContext<'_>, rt: &Handle) -> Self {
        let settings = load_settings();
        let state = Arc::new(Mutex::new(AppState {
            settings,
            ..Default::default()
        }));

        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();

        let state_clone = Arc::clone(&state);
        rt.spawn(command_loop(state_clone, cmd_rx));

        Self { state, cmd_tx }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Ok(state) = self.state.lock() {
            main_window::show(ctx, &state, &self.cmd_tx);
        } else {
            error!("AppState mutex is poisoned — skipping frame render");
        }
    }
}

// ---------------------------------------------------------------------------
// command_loop
// ---------------------------------------------------------------------------

/// Async task that processes [`RecorderCommand`] messages from the UI.
///
/// Runs on the Tokio runtime and mutates [`AppState`] in response to each
/// command.  All mutex acquisitions use `if let Ok(…)` to avoid panicking
/// on a poisoned lock.
async fn command_loop(state: Arc<Mutex<AppState>>, mut rx: UnboundedReceiver<RecorderCommand>) {
    info!("command_loop started");

    while let Some(cmd) = rx.recv().await {
        info!(?cmd, "received recorder command");

        match cmd {
            RecorderCommand::Start => {
                let settings = if let Ok(s) = state.lock() {
                    s.settings.clone()
                } else {
                    error!("AppState poisoned — cannot start recording");
                    continue;
                };

                let orchestrator = RecordingOrchestrator::new(&settings);

                if let Ok(mut s) = state.lock() {
                    s.orchestrator = Some(orchestrator);
                    s.recording_status = RecordingStatus::Recording {
                        started_at: Instant::now(),
                    };
                    s.last_error = None;
                    info!("recording started");
                } else {
                    error!("AppState poisoned — could not transition to Recording");
                }
            }

            RecorderCommand::Stop => {
                if let Ok(mut s) = state.lock() {
                    // Phase 3 will flush the pipeline; for now just transition state.
                    s.orchestrator = None;
                    s.recording_status = RecordingStatus::Previewing;
                    info!("recording stopped — previewing");
                } else {
                    error!("AppState poisoned — could not stop recording");
                }
            }

            RecorderCommand::Accept => {
                if let Ok(mut s) = state.lock() {
                    s.recording_status = RecordingStatus::Saving;
                    // Phase 3: move temp file to output_dir here.
                    s.preview_path = None;
                    s.recording_status = RecordingStatus::Idle;
                    info!("recording accepted and saved");
                } else {
                    error!("AppState poisoned — could not accept recording");
                }
            }

            RecorderCommand::Discard => {
                let preview = if let Ok(mut s) = state.lock() {
                    s.recording_status = RecordingStatus::Idle;
                    s.preview_path.take()
                } else {
                    error!("AppState poisoned — could not discard recording");
                    continue;
                };

                if let Some(path) = preview {
                    if let Err(e) = tokio::fs::remove_file(&path).await {
                        error!(?path, error = %e, "failed to delete discarded preview file");
                    } else {
                        info!(?path, "discarded preview file deleted");
                    }
                }
            }
        }
    }

    info!("command_loop exited — channel closed");
}
