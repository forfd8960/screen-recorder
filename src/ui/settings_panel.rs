//! Settings panel — recording region picker.
//!
//! Rendered as a collapsible [`egui::TopBottomPanel`] below the main window.
//! The entire panel is disabled when the recording status is not `Idle`
//! (T042 requirement).
//!
//! # Region options
//!
//! * **Full Screen** — `ComboBox` populated from [`AppState::available_displays`].
//!   Selecting a display dispatches [`RecorderCommand::UpdateRegion`].
//! * **Window** — `ComboBox` populated from [`AppState::available_windows`].
//! * **Area** — Shows a placeholder noting that sub-region capture is a P1
//!   open question pending macOS 14.2+ feature enablement.
//!
//! A "Refresh" button re-dispatches [`RecorderCommand::RefreshContent`] to
//! repopulate the display/window lists on demand.

use egui::{Color32, ComboBox, RichText, Ui};
use tokio::sync::mpsc::UnboundedSender;

use crate::{
    app::{AppState, RecorderCommand},
    config::settings::{CaptureRegion, Resolution, VideoQuality},
};

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Renders the settings panel for one egui frame.
///
/// The panel is shown as a collapsible bottom panel.  All region controls are
/// disabled while the recording pipeline is active.
pub fn show(ctx: &egui::Context, state: &AppState, cmd_tx: &UnboundedSender<RecorderCommand>) {
    egui::TopBottomPanel::bottom("settings_panel")
        .resizable(false)
        .show(ctx, |ui| {
            ui.add_space(4.0);
            ui.collapsing("⚙ Recording Settings", |ui| {
                render_video_output_controls(ui, state, cmd_tx);
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(8.0);
                render_region_picker(ui, state, cmd_tx);
            });
            ui.add_space(4.0);
        });
}

fn render_video_output_controls(
    ui: &mut Ui,
    state: &AppState,
    cmd_tx: &UnboundedSender<RecorderCommand>,
) {
    let is_idle = state.recording_status.is_idle();

    ui.add_enabled_ui(is_idle, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Resolution:").strong());
            ComboBox::from_id_salt("resolution_picker")
                .selected_text(match state.settings.resolution {
                    Resolution::Native => "Native",
                    Resolution::P1080 => "1080p",
                    Resolution::P720 => "720p",
                })
                .show_ui(ui, |ui| {
                    for (label, value) in [
                        ("Native", Resolution::Native),
                        ("1080p", Resolution::P1080),
                        ("720p", Resolution::P720),
                    ] {
                        let selected = state.settings.resolution == value;
                        if ui.selectable_label(selected, label).clicked() && !selected {
                            let _ = cmd_tx.send(RecorderCommand::UpdateResolution(value));
                        }
                    }
                });
        });

        ui.horizontal(|ui| {
            ui.label(RichText::new("Frame Rate:").strong());
            ComboBox::from_id_salt("fps_picker")
                .selected_text(format!("{} FPS", state.settings.frame_rate))
                .show_ui(ui, |ui| {
                    for fps in [24_u32, 30, 60] {
                        let selected = state.settings.frame_rate == fps;
                        if ui
                            .selectable_label(selected, format!("{fps} FPS"))
                            .clicked()
                            && !selected
                        {
                            let _ = cmd_tx.send(RecorderCommand::UpdateFrameRate(fps));
                        }
                    }
                });
        });

        ui.horizontal(|ui| {
            ui.label(RichText::new("Quality:").strong());
            for (label, value) in [
                ("Low", VideoQuality::Low),
                ("Medium", VideoQuality::Medium),
                ("High", VideoQuality::High),
            ] {
                let selected = state.settings.quality == value;
                if ui.selectable_label(selected, label).clicked() && !selected {
                    let _ = cmd_tx.send(RecorderCommand::UpdateQuality(value));
                }
            }
        });
    });

    if !is_idle {
        ui.colored_label(
            Color32::from_rgb(120, 120, 120),
            "Resolution/FPS/Quality controls are disabled while recording.",
        );
    }
}

// ---------------------------------------------------------------------------
// Region picker
// ---------------------------------------------------------------------------

fn render_region_picker(ui: &mut Ui, state: &AppState, cmd_tx: &UnboundedSender<RecorderCommand>) {
    let is_idle = state.recording_status.is_idle();

    ui.add_enabled_ui(is_idle, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Capture region:").strong());

            // ---- variant selector ----
            let current_variant = match &state.settings.region {
                CaptureRegion::FullScreen { .. } => "Full Screen",
                CaptureRegion::Window { .. } => "Window",
                CaptureRegion::Area { .. } => "Area",
            };

            ComboBox::from_id_salt("region_type")
                .selected_text(current_variant)
                .show_ui(ui, |ui| {
                    let is_full = matches!(state.settings.region, CaptureRegion::FullScreen { .. });
                    let is_win = matches!(state.settings.region, CaptureRegion::Window { .. });
                    let is_area = matches!(state.settings.region, CaptureRegion::Area { .. });

                    if ui.selectable_label(is_full, "Full Screen").clicked() && !is_full {
                        let _ =
                            cmd_tx.send(RecorderCommand::UpdateRegion(CaptureRegion::FullScreen {
                                display_id: 0,
                            }));
                    }
                    if ui.selectable_label(is_win, "Window").clicked() && !is_win {
                        // Pick first available window, if any.
                        let wid = state.available_windows.first().map_or(0, |w| w.window_id);
                        let _ = cmd_tx.send(RecorderCommand::UpdateRegion(CaptureRegion::Window {
                            window_id: wid,
                        }));
                    }
                    if ui.selectable_label(is_area, "Area (beta)").clicked() && !is_area {
                        use crate::config::settings::Rect;
                        let _ = cmd_tx.send(RecorderCommand::UpdateRegion(CaptureRegion::Area {
                            rect: Rect {
                                x: 0.0,
                                y: 0.0,
                                width: 1280.0,
                                height: 720.0,
                            },
                        }));
                    }
                });
        });

        // ---- variant-specific controls ----
        ui.add_space(4.0);
        match &state.settings.region {
            CaptureRegion::FullScreen { display_id } => {
                render_display_picker(ui, *display_id, state, cmd_tx);
            }
            CaptureRegion::Window { window_id } => {
                render_window_picker(ui, *window_id, state, cmd_tx);
            }
            CaptureRegion::Area { rect } => {
                render_area_info(ui, rect);
            }
        }

        ui.add_space(4.0);
        if ui.small_button("⟳ Refresh lists").clicked() {
            let _ = cmd_tx.send(RecorderCommand::RefreshContent);
        }
    });

    if !is_idle {
        ui.colored_label(
            Color32::from_rgb(120, 120, 120),
            "Region picker is disabled while recording.",
        );
    }
}

// ---------------------------------------------------------------------------
// Display combo-box (FullScreen)
// ---------------------------------------------------------------------------

fn render_display_picker(
    ui: &mut Ui,
    current_display_id: u32,
    state: &AppState,
    cmd_tx: &UnboundedSender<RecorderCommand>,
) {
    if state.available_displays.is_empty() {
        ui.label(RichText::new("No displays found — click ⟳ Refresh").italics());
        return;
    }

    let selected_label = state
        .available_displays
        .iter()
        .find(|d| d.display_id == current_display_id)
        .map_or_else(
            || format!("Display {current_display_id}"),
            ToString::to_string,
        );

    ComboBox::from_id_salt("display_picker")
        .selected_text(selected_label)
        .show_ui(ui, |ui| {
            for d in &state.available_displays {
                let selected = d.display_id == current_display_id;
                if ui.selectable_label(selected, d.to_string()).clicked() && !selected {
                    let _ = cmd_tx.send(RecorderCommand::UpdateRegion(CaptureRegion::FullScreen {
                        display_id: d.display_id,
                    }));
                }
            }
        });
}

// ---------------------------------------------------------------------------
// Window combo-box
// ---------------------------------------------------------------------------

fn render_window_picker(
    ui: &mut Ui,
    current_window_id: u32,
    state: &AppState,
    cmd_tx: &UnboundedSender<RecorderCommand>,
) {
    if state.available_windows.is_empty() {
        ui.label(RichText::new("No windows found — click ⟳ Refresh").italics());
        return;
    }

    let selected_label = state
        .available_windows
        .iter()
        .find(|w| w.window_id == current_window_id)
        .map_or_else(
            || format!("Window {current_window_id}"),
            ToString::to_string,
        );

    ComboBox::from_id_salt("window_picker")
        .selected_text(selected_label)
        .show_ui(ui, |ui| {
            for w in &state.available_windows {
                let selected = w.window_id == current_window_id;
                if ui.selectable_label(selected, w.to_string()).clicked() && !selected {
                    let _ = cmd_tx.send(RecorderCommand::UpdateRegion(CaptureRegion::Window {
                        window_id: w.window_id,
                    }));
                }
            }
        });
}

// ---------------------------------------------------------------------------
// Area info
// ---------------------------------------------------------------------------

fn render_area_info(ui: &mut Ui, rect: &crate::config::settings::Rect) {
    ui.label(
        RichText::new(format!(
            "Area: ({:.0}, {:.0})  {:.0} × {:.0} pts",
            rect.x, rect.y, rect.width, rect.height
        ))
        .small(),
    );
    ui.colored_label(
        Color32::from_rgb(160, 120, 40),
        "⚠ Sub-region capture requires macOS 14.2+ (set_content_rect).\n\
         The full display will be recorded until the feature flag is enabled.",
    );
}
