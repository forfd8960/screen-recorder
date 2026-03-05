//! Save panel — shown while `RecordingStatus::Saving` is active.
//!
//! Lets the user choose output folder and explicitly confirm save.

use std::time::Duration;

use egui::{Color32, RichText};
use tokio::sync::mpsc::UnboundedSender;

use crate::{
    app::{AppState, RecorderCommand, RecordingStatus},
    error::AppError,
    output::save,
};

/// Renders the save panel for one egui frame.
pub fn show(ctx: &egui::Context, state: &AppState, cmd_tx: &UnboundedSender<RecorderCommand>) {
    if !matches!(state.recording_status, RecordingStatus::Saving) {
        return;
    }

    egui::CentralPanel::default().show(ctx, |ui| {
        ui.heading("Save Recording");
        ui.add_space(8.0);

        ui.label(
            RichText::new(format!("Output folder: {}", state.settings.output_dir.display()))
                .color(Color32::GRAY),
        );

        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.button("Change Folder").clicked()
                && let Some(dir) = rfd::FileDialog::new()
                    .set_title("Choose Output Folder")
                    .set_directory(&state.settings.output_dir)
                    .pick_folder()
            {
                let _ = cmd_tx.send(RecorderCommand::SetOutputDir(dir));
            }

            if ui.button("Save").clicked() {
                let _ = cmd_tx.send(RecorderCommand::Accept);
            }
        });

        if let Some(AppError::Io { source }) = &state.last_error {
            ui.add_space(12.0);
            ui.colored_label(
                Color32::from_rgb(220, 100, 60),
                format!("⚠ Save failed: {source}"),
            );
            if ui.button("Retry").clicked() {
                let _ = cmd_tx.send(RecorderCommand::Accept);
            }
        }
    });
}

/// Renders completion toast for successful save and auto-hides after 5 seconds.
pub fn render_completion_toast(ctx: &egui::Context, state: &AppState) {
    const TOAST_DURATION_SECS: f32 = 5.0;

    let Some(toast) = &state.success_toast else {
        return;
    };

    if toast.shown_at.elapsed().as_secs_f32() > TOAST_DURATION_SECS {
        return;
    }

    ctx.request_repaint_after(Duration::from_millis(100));

    egui::Area::new("save_completion_toast".into())
        .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-16.0, 16.0))
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::default()
                .fill(Color32::from_rgb(40, 150, 70))
                .corner_radius(egui::CornerRadius::same(8))
                .inner_margin(egui::Margin::symmetric(12_i8, 8_i8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("✓ {}", toast.message))
                                .color(Color32::WHITE)
                                .strong(),
                        );

                        if ui
                            .small_button(RichText::new("Show in Finder").color(Color32::WHITE))
                            .clicked()
                            && let Err(e) = save::reveal_in_finder(&toast.saved_path)
                        {
                            tracing::warn!(?toast.saved_path, "reveal_in_finder failed: {e}");
                        }
                    });
                });
        });
}
