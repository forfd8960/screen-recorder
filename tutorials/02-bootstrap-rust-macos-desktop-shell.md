# Tutorial 02: Bootstrap Rust + macOS Desktop Shell

## Goal

Set up a working desktop app shell with logging, Tokio runtime, and eframe/egui window.

## Topics

- Crate structure:
  - `main.rs` binary entry
  - `lib.rs` module exports and lint policy
- Dependency setup in `Cargo.toml`:
  - `eframe`, `egui`, `tokio`, `tracing`, `thiserror`, `serde`
  - Apple framework crates (`screencapturekit`, `objc2-*`)
- Strict lints and quality gates:
  - deny unsafe at crate root
  - clippy/rustfmt workflow
- Runtime startup path:
  - initialize tracing (`EnvFilter`)
  - create Tokio runtime
  - launch `eframe::run_native`
- First UI skeleton:
  - render static main panel and status placeholder

## Deliverable

- App window launches successfully and logs startup events.
