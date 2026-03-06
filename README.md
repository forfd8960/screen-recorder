# screen-recorder

[![Rust](https://img.shields.io/badge/Rust-stable-orange)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/platform-macOS%2012.3%2B-blue)](#requirements)
[![Status](https://img.shields.io/badge/status-active%20development-yellow)](#current-status)

A native macOS screen recorder built with Rust, ScreenCaptureKit, and AVFoundation.

It captures screen + optional microphone audio, writes MP4 (H.264 + AAC), supports preview-before-save, and provides local/global shortcuts.

## Current Status

This repository is in active development.

- ✅ Implemented through **Phase 9** (shortcuts) in `specs/0004-tasks.md`
- ✅ Audio write path enabled in encoder (`src/encode/pipeline.rs`)
- ✅ Recording, preview/discard, save flow, and settings persistence are available
- ⚠️ Phase 10 (background stability hardening) and parts of Phase 11 (polish/docs) are still pending

## Screenshots

Screenshots are not committed yet. You can add them under `assets/` and reference them here.

Example placeholders:

```md
![Main Window](assets/screenshot-main.png)
![Settings Panel](assets/screenshot-settings.png)
![Preview Panel](assets/screenshot-preview.png)
```

## Feature Overview

- Screen capture via ScreenCaptureKit
- Optional microphone capture (fallback to video-only if mic is unavailable)
- MP4 output via AVAssetWriter
  - Video: H.264
  - Audio: AAC (when mic capture is enabled and available)
- Region selection
  - Full screen (display picker)
  - Single window (window picker)
  - Area (beta UI + settings path)
- Preview workflow
  - Open in QuickTime
  - Accept (save) / Discard (delete temp file)
- Save workflow
  - Select output folder
  - Timestamped filename generation
  - Reveal in Finder
  - Completion toast
- Persistent settings
  - Resolution (`Native`, `1080p`, `720p`)
  - Frame rate (`24`, `30`, `60`)
  - Quality (`Low`, `Medium`, `High`)
  - Capture region
  - Microphone toggle
  - Output directory
- Keyboard shortcuts
  - Global: `⌘⇧R` start, `⌘⇧S` stop
  - In-app preview: `⌘↩` accept, `⌘⌫` discard

## Architecture

The app is command-driven with clear module boundaries:

- **UI** (`src/ui/*`)
  - egui panels: `main_window`, `settings_panel`, `preview_panel`, `save_panel`
  - Emits `RecorderCommand`
- **Orchestration** (`src/app.rs`)
  - Owns `AppState`
  - Runs async `command_loop` on Tokio
  - Coordinates `CaptureEngine` + `EncodingPipeline` via `RecordingOrchestrator`
- **Capture** (`src/capture/*`)
  - Permission checks (`permissions.rs`)
  - Region/content filter (`content_filter.rs`)
  - Screen/audio stream lifecycle (`engine.rs`, `audio.rs`)
- **Encoding** (`src/encode/*`)
  - AVAssetWriter pipeline (`pipeline.rs`)
  - Temporary output lifecycle (`temp_file.rs`)
  - PTS utilities (`sync.rs`)
- **Output + Config**
  - Finalize/move/reveal output (`src/output/save.rs`)
  - Load/save settings (`src/config/settings.rs`)

### Runtime Flow

1. User triggers `Start` (button or shortcut)
2. `command_loop` validates state and permissions
3. `CaptureEngine` starts ScreenCaptureKit stream and publishes sample buffers
4. `EncodingPipeline` drains video/audio channels and writes MP4
5. User triggers `Stop`
6. Encoder finalizes temp file and app enters `Previewing`
7. User accepts (save) or discards (delete temp file)

## Tech Stack

- **Language**: Rust (edition 2024)
- **Runtime**: Tokio
- **UI**: eframe / egui
- **Capture**: `screencapturekit`
- **Encoding**: `objc2-av-foundation` (`AVAssetWriter`)
- **ObjC bridge**: `objc2`, `objc2-foundation`, `objc2-core-media`, `objc2-core-video`, `objc2-app-kit`
- **Serialization**: `serde`, `serde_json`
- **Logging**: `tracing`, `tracing-subscriber`
- **Testing**: Rust unit/integration tests (with `integration` feature gate)

## Requirements

- macOS 12.3+
- Rust stable toolchain (pinned by `rust-toolchain.toml`)
- Screen Recording permission
- Microphone permission (optional; required for audio track)

## Quick Start

```bash
# Build (dev) + ad-hoc sign
make build

# Run
make run

# Run with debug logs
make run-debug
```

Release build:

```bash
make build-release
```

Or directly:

```bash
cargo build --release
```

## Usage

1. Launch app: `make run`
2. Grant Screen Recording (and optionally Microphone) permission
3. Configure settings in the bottom settings panel
4. Press `Start` or `⌘⇧R`
5. Press `Stop` or `⌘⇧S`
6. In Preview:
   - `Accept` / `⌘↩` to save
   - `Discard` / `⌘⌫` to delete

## Configuration

Settings file:

`~/Library/Application Support/screen-recorder/settings.json`

Default output directory:

- Desktop
- Fallback: home directory
- Fallback: `/tmp`

## Keyboard Shortcuts

| Action | Shortcut | Scope |
|---|---|---|
| Start recording | `⌘⇧R` | Global + app-local |
| Stop recording | `⌘⇧S` | Global + app-local |
| Accept preview | `⌘↩` | App-local |
| Discard preview | `⌘⌫` | App-local |

## Development

Quality gates:

```bash
make fmt-check
make lint
make test
```

Integration-gated tests:

```bash
cargo test --features integration
```

Security/dependency checks:

```bash
make audit
cargo deny check
```

## Troubleshooting

### Permission prompt does not appear

- Run `make reset-tcc`
- Relaunch app
- Re-grant permissions in System Settings

### Recording has no microphone audio

- Ensure microphone permission is granted
- Ensure mic capture is enabled in settings
- Check logs with `RUST_LOG=screen_recorder=debug make run`

### App cannot save file

- Confirm output directory exists and is writable
- Try changing folder in Save panel
- Retry save from the error prompt

## FAQ

**Q: Does this support macOS below 12.3?**  
A: No, ScreenCaptureKit requires macOS 12.3+.

**Q: Does it support HEVC/ProRes export?**  
A: Not yet; current implementation targets MP4/H.264 (+ AAC audio).

**Q: Is area capture fully implemented?**  
A: Area mode exists in settings/UI; full sub-region behavior remains part of ongoing implementation work.

## Roadmap

Near-term items from tasks plan:

- Background recording stability hardening (Phase 10)
- Error routing and observability polish (Phase 11)
- Final regression and release validation

See `specs/0004-tasks.md` for details.

## Contributing

Contributions are welcome.

1. Create a branch from `main`
2. Keep changes focused and well-tested
3. Run quality gates locally:
   - `make fmt-check`
   - `make lint`
   - `make test`
4. Open a PR with a clear summary and verification notes

## Repository Layout

```text
src/
  app.rs
  main.rs
  lib.rs
  error.rs
  capture/
  encode/
  output/
  config/
  ui/
tests/
specs/
assets/
```

## License

No license file has been added yet.
