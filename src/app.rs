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
use tracing::{error, info, warn};

use crate::{
    capture::{engine::CaptureEngine, permissions::check_screen_permission},
    config::settings::{RecordingSettings, load_settings},
    encode::pipeline::EncodingPipeline,
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
// RecordingOrchestrator
// ---------------------------------------------------------------------------

/// Coordinates the capture and encoding pipelines for a single recording session.
///
/// Created by [`RecordingOrchestrator::start`] and consumed by
/// [`RecordingOrchestrator::stop`].  The orchestrator is kept as a **local
/// variable** inside [`command_loop`] and is never placed inside the shared
/// [`AppState`] — this avoids holding a `Mutex` lock around heavy pipeline
/// objects.
pub struct RecordingOrchestrator {
    engine: CaptureEngine,
    pipeline: EncodingPipeline,
}

impl RecordingOrchestrator {
    /// Starts capture and encoding for the given settings.
    ///
    /// 1. Creates a [`CaptureEngine`] and an [`EncodingPipeline`] (which
    ///    starts a dedicated encoding thread immediately).
    /// 2. Starts the `SCStream` capture session.
    ///
    /// # Errors
    ///
    /// Returns the underlying [`AppError`] if permission is denied, no display
    /// is available, the stream fails to start, or the temp file cannot be
    /// created.
    pub async fn start(settings: &RecordingSettings) -> Result<Self, AppError> {
        let (mut engine, video_rx, audio_rx) = CaptureEngine::new();
        let pipeline = EncodingPipeline::new(settings, video_rx, audio_rx)?;
        engine.start(settings).await?;
        Ok(Self { engine, pipeline })
    }

    /// Stops capture and waits for the encoding pipeline to finish.
    ///
    /// Dropping the engine's sender halves signals the encoding thread, which
    /// then finalizes the `AVAssetWriter` and sends back the output path.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] if stopping the stream or finalizing the output
    /// file fails.
    pub async fn stop(mut self) -> Result<PathBuf, AppError> {
        // Stop capture (drops frame channel senders → signals encoding thread).
        self.engine.stop().await?;
        // Await the encoding thread's completion and retrieve the output path.
        self.pipeline.finish().await
    }
}

// ---------------------------------------------------------------------------
// AppState
// ---------------------------------------------------------------------------

/// Shared application state.
///
/// Protected by an `Arc<Mutex<…>>` so it can be accessed from both the egui
/// render thread and the async command loop.
///
/// The [`RecordingOrchestrator`] is **not** stored here; it lives as a local
/// variable inside [`command_loop`] to avoid locking around heavy pipeline
/// objects during each UI frame.
#[derive(Default)]
pub struct AppState {
    /// User-facing recording configuration.
    pub settings: RecordingSettings,
    /// Current recording pipeline state.
    pub recording_status: RecordingStatus,
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

    // -----------------------------------------------------------------------
    // T029 — TCC onboarding: check screen-recording permission at startup.
    // If denied, set last_error so the UI shows the onboarding screen.
    // -----------------------------------------------------------------------
    match check_screen_permission().await {
        Ok(_) => info!("screen recording permission granted"),
        Err(AppError::PermissionDenied) => {
            warn!("screen recording permission denied — requesting onboarding");
            if let Ok(mut s) = state.lock() {
                s.last_error = Some(AppError::PermissionDenied);
            }
        }
        Err(e) => {
            warn!("permission check failed: {e}");
        }
    }

    // The active orchestrator lives here — not inside AppState — to avoid
    // holding the mutex lock around heavy pipeline objects.
    let mut orch: Option<RecordingOrchestrator> = None;

    while let Some(cmd) = rx.recv().await {
        info!(?cmd, "received recorder command");

        match cmd {
            RecorderCommand::Start => {
                if orch.is_some() {
                    warn!("Start command received while already recording — ignoring");
                    continue;
                }

                let settings = if let Ok(s) = state.lock() {
                    s.settings.clone()
                } else {
                    error!("AppState poisoned — cannot start recording");
                    continue;
                };

                match RecordingOrchestrator::start(&settings).await {
                    Ok(o) => {
                        orch = Some(o);
                        if let Ok(mut s) = state.lock() {
                            s.recording_status = RecordingStatus::Recording {
                                started_at: Instant::now(),
                            };
                            s.last_error = None;
                            info!("recording started");
                        }
                    }
                    Err(e) => {
                        error!("failed to start recording: {e}");
                        if let Ok(mut s) = state.lock() {
                            s.last_error = Some(e);
                        }
                    }
                }
            }

            RecorderCommand::Stop => {
                if let Some(active_orch) = orch.take() {
                    match active_orch.stop().await {
                        Ok(path) => {
                            if let Ok(mut s) = state.lock() {
                                s.preview_path = Some(path.clone());
                                s.recording_status = RecordingStatus::Previewing;
                                info!(?path, "recording stopped — previewing");
                            }
                        }
                        Err(e) => {
                            error!("failed to stop recording: {e}");
                            if let Ok(mut s) = state.lock() {
                                s.recording_status = RecordingStatus::Idle;
                                s.last_error = Some(e);
                            }
                        }
                    }
                } else {
                    warn!("Stop command received while not recording — ignoring");
                }
            }

            RecorderCommand::Accept => {
                if let Ok(mut s) = state.lock() {
                    s.recording_status = RecordingStatus::Saving;
                    // Phase 4: move temp file to output_dir here.
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
