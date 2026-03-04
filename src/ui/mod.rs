//! User-interface layer (egui panels).
//!
//! Sub-modules:
//! - [`main_window`]    – Start/Stop button, status badge, elapsed timer
//! - [`settings_panel`] – Resolution, FPS, region, quality, mic, output folder
//! - [`preview_panel`]  – `QuickTime` launch and Accept/Discard buttons
//! - [`save_panel`]     – Folder picker and completion toast

pub mod main_window;
pub mod settings_panel;
