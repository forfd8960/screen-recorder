//! Preview panel — shown after recording stops.
//!
//! Renders an Accept/Discard UI while [`RecordingStatus::Previewing`] is
//! active.  On demand the file is opened in `QuickTime` Player via the macOS
//! `open -a "QuickTime Player"` command.
//!
//! # Keyboard shortcuts
//!
//! `⌘ Return` → Accept\
//! `⌘ ⌫`     → Discard\
//! (Registered in [`crate::app::App::update`] via `egui::Context::input`.)
//!
//! # Testable helpers
//!
//! [`handle_accept`] and [`handle_discard`] are pure functions that send
//! commands and optionally touch the file system, making them unit-testable
//! without a running egui context.

use std::path::Path;

use egui::{Color32, RichText};
use tokio::sync::mpsc::UnboundedSender;

use crate::{
    app::{AppState, RecorderCommand, RecordingStatus},
    error::AppError,
};

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Renders the preview panel for one egui frame.
///
/// Does nothing when `state.recording_status` is not
/// [`RecordingStatus::Previewing`].  Intended to be called from
/// [`crate::app::App::update`] in place of (not alongside) the main window's
/// `CentralPanel` when the status is `Previewing`.
pub fn show(ctx: &egui::Context, state: &AppState, cmd_tx: &UnboundedSender<RecorderCommand>) {
    if !matches!(state.recording_status, RecordingStatus::Previewing) {
        return;
    }

    egui::CentralPanel::default().show(ctx, |ui| {
        ui.centered_and_justified(|ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(16.0);
                ui.heading("🎬  Recording Preview");
                ui.add_space(8.0);

                match &state.preview_path {
                    Some(path) => {
                        ui.label(
                            RichText::new(format!("📁  {}", path.display()))
                                .color(Color32::GRAY)
                                .small(),
                        );

                        ui.add_space(12.0);

                        if ui.button("▶  Open in QuickTime Player").clicked()
                            && let Err(e) = open_in_quicktime(path)
                        {
                            tracing::warn!("failed to open QuickTime: {e}");
                        }
                    }
                    None => {
                        ui.label(
                            RichText::new("⚠  No preview file available.")
                                .color(Color32::from_rgb(200, 60, 60)),
                        );
                    }
                }

                ui.add_space(24.0);
                ui.separator();
                ui.add_space(16.0);

                ui.horizontal(|ui| {
                    let btn_width = 180.0;

                    // Accept button.
                    let accept = egui::Button::new(
                        RichText::new("✓  Accept  (⌘ ↩)").color(Color32::from_rgb(255, 255, 255)),
                    )
                    .fill(Color32::from_rgb(30, 140, 30))
                    .min_size(egui::vec2(btn_width, 36.0));

                    if ui.add(accept).clicked() {
                        handle_accept(cmd_tx);
                    }

                    ui.add_space(16.0);

                    // Discard button.
                    let discard = egui::Button::new(
                        RichText::new("✗  Discard  (⌘ ⌫)").color(Color32::from_rgb(255, 255, 255)),
                    )
                    .fill(Color32::from_rgb(180, 40, 40))
                    .min_size(egui::vec2(btn_width, 36.0));

                    if ui.add(discard).clicked() {
                        let path = state.preview_path.clone();
                        handle_discard(path.as_deref(), cmd_tx);
                    }
                });

                ui.add_space(8.0);
                ui.label(
                    RichText::new(
                        "Accept — save recording to output folder   ·   \
                         Discard — permanently delete temp file",
                    )
                    .color(Color32::GRAY)
                    .small(),
                );
            });
        });
    });
}

// ---------------------------------------------------------------------------
// Public helpers (testable without an egui context)
// ---------------------------------------------------------------------------

/// Opens `path` in `QuickTime` Player using the macOS `open` command.
///
/// Equivalent to running `open -a "QuickTime Player" <path>` in a shell.
///
/// # Errors
///
/// Returns [`AppError::Io`] if the process cannot be spawned; returns
/// [`AppError::StreamCreation`] if `open` exits with a non-zero status.
pub fn open_in_quicktime(path: &Path) -> Result<(), AppError> {
    let status = std::process::Command::new("open")
        .arg("-a")
        .arg("QuickTime Player")
        .arg(path)
        .status()?;

    if status.success() {
        Ok(())
    } else {
        Err(AppError::StreamCreation(format!(
            "`open -a \"QuickTime Player\"` exited with {status}"
        )))
    }
}

/// Emits [`RecorderCommand::Accept`] on `cmd_tx`.
///
/// File persistence is handled asynchronously inside the command loop; this
/// function only enqueues the intent.
pub fn handle_accept(cmd_tx: &UnboundedSender<RecorderCommand>) {
    let _ = cmd_tx.send(RecorderCommand::Accept);
}

/// Synchronously removes `path` from disk, then emits [`RecorderCommand::Discard`].
///
/// Deletion errors are logged at `warn` level but do not prevent the status
/// transition — the command is always sent.
pub fn handle_discard(path: Option<&Path>, cmd_tx: &UnboundedSender<RecorderCommand>) {
    if let Some(p) = path {
        match std::fs::remove_file(p) {
            Ok(()) => tracing::info!(path = ?p, "preview file deleted on discard"),
            Err(e) => tracing::warn!(path = ?p, error = %e, "failed to delete preview file"),
        }
    }
    let _ = cmd_tx.send(RecorderCommand::Discard);
}

// ---------------------------------------------------------------------------
// Unit tests (T044)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    // -----------------------------------------------------------------------
    // handle_accept
    // -----------------------------------------------------------------------

    /// `handle_accept` must emit exactly one `RecorderCommand::Accept`.
    #[test]
    fn handle_accept_emits_accept_command() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        handle_accept(&tx);
        let cmd = rx.try_recv().expect("Accept command not sent");
        assert!(
            matches!(cmd, RecorderCommand::Accept),
            "expected Accept, got {cmd:?}"
        );
        // No extra commands.
        assert!(
            rx.try_recv().is_err(),
            "unexpected extra command in channel"
        );
    }

    // -----------------------------------------------------------------------
    // handle_discard
    // -----------------------------------------------------------------------

    /// `handle_discard` deletes the file and emits `RecorderCommand::Discard`.
    #[test]
    fn handle_discard_deletes_file_and_emits_discard_command() {
        let (tx, mut rx) = mpsc::unbounded_channel();

        // Create a real temp file.
        let dir = tempfile::tempdir().expect("could not create temp dir");
        let path = dir.path().join("preview.mp4");
        std::fs::write(&path, b"fake mp4 data").expect("write failed");
        assert!(path.exists(), "file must exist before discard");

        handle_discard(Some(&path), &tx);

        // File must have been deleted.
        assert!(!path.exists(), "file must be deleted after handle_discard");

        // Command must have been sent.
        let cmd = rx.try_recv().expect("Discard command not sent");
        assert!(
            matches!(cmd, RecorderCommand::Discard),
            "expected Discard, got {cmd:?}"
        );
    }

    /// `handle_discard(None, …)` emits command even when there is no path.
    #[test]
    fn handle_discard_with_no_path_still_sends_command() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        handle_discard(None, &tx);
        let cmd = rx.try_recv().expect("Discard command not sent");
        assert!(matches!(cmd, RecorderCommand::Discard));
    }
}
