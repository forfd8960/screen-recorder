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
    capture::{
        content_filter::{DisplayInfo, WindowInfo},
        engine::CaptureEngine,
        permissions::{check_mic_permission, check_screen_permission},
    },
    config::settings::{RecordingSettings, load_settings},
    encode::pipeline::EncodingPipeline,
    error::AppError,
    ui::{main_window, preview_panel, settings_panel},
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
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RecorderCommand {
    /// Begin a capture session using the current settings.
    Start,
    /// Stop an in-progress capture and transition to preview mode.
    Stop,
    /// Accept the preview — persist the file to `output_dir`.
    Accept,
    /// Discard the preview — delete the temporary file and return to idle.
    Discard,
    /// Re-check screen recording TCC permission (used from the onboarding panel).
    RetryPermission,
    /// Dismiss the current non-fatal error banner (e.g. mic-unavailable).
    ClearError,
    /// Update the capture region in settings (sent from the region picker).
    UpdateRegion(crate::config::settings::CaptureRegion),
    /// Refresh the available display and window lists (sent when settings panel opens).
    RefreshContent,
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
        // Start the engine first to obtain the actual capture dimensions.
        // Frames are buffered in the channel (capacity 120) until the
        // encoding pipeline thread begins draining them.
        let (width, height) = engine.start(settings).await?;
        let pipeline = EncodingPipeline::new(settings, video_rx, audio_rx, width, height)?;
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
    /// Available displays, populated on startup and on settings-panel open.
    pub available_displays: Vec<DisplayInfo>,
    /// Available windows, populated on startup and on settings-panel open.
    pub available_windows: Vec<WindowInfo>,
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
            // T050 – preview keyboard shortcuts (⌘ Return / ⌘ ⌫)
            if matches!(state.recording_status, RecordingStatus::Previewing) {
                ctx.input(|i| {
                    if i.modifiers.command && i.key_pressed(egui::Key::Enter) {
                        preview_panel::handle_accept(&self.cmd_tx);
                    }
                    if i.modifiers.command && i.key_pressed(egui::Key::Backspace) {
                        let path = state.preview_path.clone();
                        preview_panel::handle_discard(path.as_deref(), &self.cmd_tx);
                    }
                });
            }

            // T047 – show preview panel when Previewing, main window otherwise.
            if matches!(state.recording_status, RecordingStatus::Previewing) {
                preview_panel::show(ctx, &state, &self.cmd_tx);
            } else {
                main_window::show(ctx, &state, &self.cmd_tx);
            }

            // Settings panel (bottom) is always visible.
            settings_panel::show(ctx, &state, &self.cmd_tx);
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
#[allow(clippy::too_many_lines)]
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

    // T043: Populate available displays and windows asynchronously at startup.
    match tokio::task::spawn_blocking(|| {
        use crate::capture::content_filter::{list_displays, list_windows};
        let displays = list_displays().unwrap_or_default();
        let windows = list_windows().unwrap_or_default();
        (displays, windows)
    })
    .await
    {
        Ok((displays, windows)) => {
            if let Ok(mut s) = state.lock() {
                info!(
                    available_displays = displays.len(),
                    available_windows = windows.len(),
                    "display and window lists populated"
                );
                s.available_displays = displays;
                s.available_windows = windows;
            }
        }
        Err(e) => warn!("failed to populate display/window lists: {e}"),
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

                // T034: check microphone permission before starting.
                // If denied and capture_mic is requested, fall back to
                // video-only and emit a non-blocking MicrophoneUnavailable banner.
                let settings = if settings.capture_mic && !check_mic_permission().await {
                    warn!("microphone permission denied — falling back to video-only recording");
                    if let Ok(mut s) = state.lock() {
                        s.last_error = Some(AppError::MicrophoneUnavailable);
                    }
                    RecordingSettings {
                        capture_mic: false,
                        ..settings
                    }
                } else {
                    settings
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

            // T048 – Accept: transition to Saving and stay there.
            // Phase 7 (save_panel) will handle the actual file move and
            // transition back to Idle once saving completes.
            RecorderCommand::Accept => {
                if let Ok(mut s) = state.lock() {
                    if matches!(s.recording_status, RecordingStatus::Previewing) {
                        s.recording_status = RecordingStatus::Saving;
                        info!(?s.preview_path, "recording accepted — transitioning to Saving");
                    } else {
                        warn!("Accept command received outside Previewing state — ignoring");
                    }
                } else {
                    error!("AppState poisoned — could not accept recording");
                }
            }

            // T049 – Discard: clean up temp file (best-effort) and return to Idle.
            // Note: handle_discard() in preview_panel may have already deleted
            // the file when triggered from a button click; the attempt here is
            // a safety net for keyboard-shortcut paths and future callers.
            RecorderCommand::Discard => {
                let preview = if let Ok(mut s) = state.lock() {
                    s.recording_status = RecordingStatus::Idle;
                    s.preview_path.take()
                } else {
                    error!("AppState poisoned — could not discard recording");
                    continue;
                };

                if let Some(path) = preview {
                    match tokio::fs::remove_file(&path).await {
                        Ok(()) => info!(?path, "discarded preview file deleted"),
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                            // Already deleted by the UI handler — this is expected.
                            info!(?path, "preview file already removed before Discard command");
                        }
                        Err(e) => {
                            error!(?path, error = %e, "failed to delete discarded preview file");
                        }
                    }
                }
            }

            RecorderCommand::RetryPermission => {
                info!("re-checking screen recording permission");
                match check_screen_permission().await {
                    Ok(_) => {
                        info!("screen recording permission granted");
                        if let Ok(mut s) = state.lock() {
                            s.last_error = None;
                        }
                    }
                    Err(e) => {
                        warn!("screen recording permission still denied: {e}");
                        if let Ok(mut s) = state.lock() {
                            s.last_error = Some(AppError::PermissionDenied);
                        }
                    }
                }
            }

            // T035: user dismissed the non-fatal error banner.
            RecorderCommand::ClearError => {
                if let Ok(mut s) = state.lock() {
                    // Only clear non-fatal errors (not PermissionDenied, which
                    // requires the onboarding flow to resolve).
                    if !matches!(s.last_error, Some(AppError::PermissionDenied)) {
                        s.last_error = None;
                        info!("non-fatal error banner dismissed by user");
                    }
                }
            }

            // T042/T043: region picker selection changed.
            RecorderCommand::UpdateRegion(region) => {
                if let Ok(mut s) = state.lock() {
                    if s.recording_status.is_idle() {
                        info!(?region, "capture region updated");
                        s.settings.region = region;
                    } else {
                        warn!("UpdateRegion ignored while recording is in progress");
                    }
                }
            }

            // T043: refresh available display and window lists.
            RecorderCommand::RefreshContent => {
                let state_ref = Arc::clone(&state);
                tokio::task::spawn_blocking(move || {
                    use crate::capture::content_filter::{list_displays, list_windows};
                    let displays = list_displays().unwrap_or_default();
                    let windows = list_windows().unwrap_or_default();
                    if let Ok(mut s) = state_ref.lock() {
                        info!(
                            available_displays = displays.len(),
                            available_windows = windows.len(),
                            "display/window lists refreshed"
                        );
                        s.available_displays = displays;
                        s.available_windows = windows;
                    }
                });
            }
        }
    }

    info!("command_loop exited — channel closed");
}
