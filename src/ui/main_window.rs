//! Main recorder window panel.
//!
//! [`show`] is the only public entry point; it is called once per egui frame
//! from [`crate::app::App::update`].  All UI state is derived from
//! [`AppState`] — this module never mutates state directly; it only enqueues
//! [`RecorderCommand`]s on `cmd_tx`.

use std::time::Duration;

use egui::{Color32, RichText, Ui};
use tokio::sync::mpsc::UnboundedSender;

use crate::{
    app::{AppState, RecorderCommand, RecordingStatus},
    error::AppError,
};

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Renders the entire recorder window for one egui frame.
///
/// `state`   — read-only snapshot of the current application state.
/// `cmd_tx`  — channel to enqueue commands for the async command loop.
pub fn show(ctx: &egui::Context, state: &AppState, cmd_tx: &UnboundedSender<RecorderCommand>) {
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.heading("Screen Recorder");
        ui.add_space(8.0);

        render_status(ui, &state.recording_status, ctx);
        ui.add_space(12.0);

        render_controls(ui, state, cmd_tx);

        if let Some(ref err) = state.last_error {
            ui.add_space(12.0);
            render_error(ui, err, cmd_tx);
        }
    });
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn render_status(ui: &mut Ui, status: &RecordingStatus, ctx: &egui::Context) {
    match status {
        RecordingStatus::Idle => {
            ui.label(
                RichText::new("● Ready")
                    .color(Color32::from_rgb(160, 160, 160))
                    .strong(),
            );
        }

        RecordingStatus::Recording { started_at } => {
            let elapsed = started_at.elapsed();
            let (h, m, s) = format_elapsed(elapsed);
            ui.label(
                RichText::new(format!("● Recording  {h:02}:{m:02}:{s:02}"))
                    .color(Color32::from_rgb(60, 200, 60))
                    .strong(),
            );
            // Keep the clock ticking.
            ctx.request_repaint_after(Duration::from_secs(1));
        }

        RecordingStatus::Previewing => {
            ui.label(
                RichText::new("● Preview ready")
                    .color(Color32::from_rgb(220, 190, 50))
                    .strong(),
            );
        }

        RecordingStatus::Saving => {
            ui.label(
                RichText::new("● Saving…")
                    .color(Color32::from_rgb(80, 160, 240))
                    .strong(),
            );
        }
    }
}

fn render_controls(ui: &mut Ui, state: &AppState, cmd_tx: &UnboundedSender<RecorderCommand>) {
    let is_idle = state.recording_status.is_idle();
    let is_recording = state.recording_status.is_recording();
    let permission_denied = matches!(&state.last_error, Some(AppError::PermissionDenied));

    ui.horizontal(|ui| {
        let start_enabled = is_idle && !permission_denied;
        if ui
            .add_enabled(start_enabled, egui::Button::new("Start"))
            .clicked()
        {
            let _ = cmd_tx.send(RecorderCommand::Start);
        }

        if ui
            .add_enabled(is_recording, egui::Button::new("Stop"))
            .clicked()
        {
            let _ = cmd_tx.send(RecorderCommand::Stop);
        }
    });

    if matches!(state.recording_status, RecordingStatus::Previewing) {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Save").clicked() {
                let _ = cmd_tx.send(RecorderCommand::Accept);
            }
            if ui.button("Discard").clicked() {
                let _ = cmd_tx.send(RecorderCommand::Discard);
            }
        });
    }
}

fn render_error(ui: &mut Ui, err: &AppError, cmd_tx: &UnboundedSender<RecorderCommand>) {
    if matches!(err, AppError::PermissionDenied) {
        ui.colored_label(
            Color32::from_rgb(220, 100, 60),
            "⚠ Screen Recording permission required",
        );
        ui.add_space(4.0);
        ui.label("1. Click the button below to open System Settings.");
        ui.label("2. Enable this app under Screen Recording.");
        ui.label(
            "3. Quit and relaunch this app — macOS requires a restart to apply the permission.",
        );
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.button("🔒  Open System Settings").clicked() {
                open_privacy_settings();
            }
            if ui.button("⏻  Quit").clicked() {
                std::process::exit(0);
            }
        });
    } else if matches!(err, AppError::MicrophoneUnavailable) {
        // T035: non-blocking yellow banner — does not prevent recording.
        egui::Frame::default()
            .fill(Color32::from_rgb(255, 220, 80))
            .inner_margin(egui::Margin::symmetric(8_i8, 6_i8))
            .corner_radius(egui::CornerRadius::same(4))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.colored_label(
                        Color32::from_rgb(80, 60, 0),
                        "⚠ Microphone unavailable — recording with video only",
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("✕").on_hover_text("Dismiss").clicked() {
                            let _ = cmd_tx.send(RecorderCommand::ClearError);
                        }
                    });
                });
            });
    } else {
        ui.colored_label(Color32::from_rgb(220, 100, 60), format!("⚠ {err}"));
    }
}

/// Opens the Screen Recording privacy pane in System Settings.
fn open_privacy_settings() {
    // Works on macOS 13+ (Ventura) and macOS 12 (Monterey).
    let url = "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture";
    let _ = std::process::Command::new("open").arg(url).spawn();
}

const fn format_elapsed(d: Duration) -> (u64, u64, u64) {
    let total_secs = d.as_secs();
    let h = total_secs / 3600;
    let m = (total_secs % 3600) / 60;
    let s = total_secs % 60;
    (h, m, s)
}
