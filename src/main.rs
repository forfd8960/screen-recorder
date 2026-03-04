// Crate-level lint policy (see AGENTS.md and specs/0003-impl-plan.md §Constitution Check).
// Unsafe code is forbidden at the crate root.  Individual modules that must
// call into Objective-C FFI (capture::engine, encode::pipeline) carry a
// module-level `#[allow(unsafe_code)]` attribute with per-block `// SAFETY:`
// comments.
#![deny(unsafe_code)]
#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![warn(rust_2018_idioms)]

use anyhow::Context as _;
use eframe::egui::ViewportBuilder;
use screen_recorder::app::App;
use tracing_subscriber::EnvFilter;

fn main() -> anyhow::Result<()> {
    // --- Tracing ---
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("screen_recorder=info")),
        )
        .init();

    // --- Tokio runtime (blocking calls spawned from capture/encode layers) ---
    let rt = tokio::runtime::Runtime::new().context("failed to start Tokio runtime")?;
    let rt_handle = rt.handle().clone();

    // --- egui / eframe window (blocks until the user closes the window) ---
    let native_options = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_title("Screen Recorder")
            .with_inner_size([480.0_f32, 320.0_f32]),
        ..Default::default()
    };

    eframe::run_native(
        "Screen Recorder",
        native_options,
        Box::new(move |cc| Ok(Box::new(App::new(cc, &rt_handle)))),
    )
    .map_err(|e| anyhow::anyhow!("eframe window error: {e}"))?;

    Ok(())
}
