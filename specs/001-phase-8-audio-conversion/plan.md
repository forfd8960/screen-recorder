# Implementation Plan: macOS Screen Recorder

**Branch**: `main` | **Date**: 2026-03-04 | **Spec**: [0002-tech-spec.md](./0002-tech-spec.md)  
**Input**: [instructions.md](./instructions.md) · [0001-research.md](./0001-research.md) · [0002-tech-spec.md](./0002-tech-spec.md)

---

## Summary

Build a macOS desktop screen recorder in Rust that captures video (ScreenCaptureKit) and
microphone audio, encodes to MP4 via `AVAssetWriter` (hardware H.264/HEVC through
VideoToolbox), and presents a minimal egui UI with start/stop controls, region selection,
preview-before-save, and configurable quality and frame-rate settings. The pipeline is
zero-copy from SCK IOSurface through to the hardware encoder; no CPU-side pixel
conversion occurs in the hot path.

---

## Technical Context

**Language/Version**: Rust stable (latest), edition 2024  
**Primary Dependencies**: `screencapturekit 0.3`, `objc2-av-foundation 0.2`, `eframe/egui 0.27`, `tokio 1`  
**Storage**: Local filesystem — temp MP4 in `$TMPDIR`, final destination chosen by user  
**Testing**: `cargo test`, `rstest` (parameterized), `mockall` (trait mocks); TCC-gated integration tests skipped in headless CI  
**Target Platform**: macOS 12.3 (Monterey) minimum; validated on latest macOS release  
**Project Type**: Desktop application (single binary, `.app` bundle optional via `cargo-bundle`)  
**Performance Goals**: < 10 % CPU at 1080p/30 FPS on Apple Silicon; < 1 % frame-drop rate  
**Constraints**: MP4 output only; offline; no network entitlements; < 150 MB RSS delta over idle  
**Scale/Scope**: Single-user, single-display recording; no multi-track timeline editing

---

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-evaluated after Phase 1 design.*

- [x] Uses stable Rust toolchain (`rust-toolchain.toml` pinned to `stable`) and edition 2024.  
      `unsafe` is allowed only in `capture/engine.rs` and `encode/pipeline.rs` for unavoidable
      Objective-C FFI; each block carries a `// SAFETY:` comment and `#[allow(unsafe_code)]`
      scoped to the module.
- [x] Error handling uses `thiserror`-based `AppError` enum throughout; `Result<T, AppError>`
      is the return type on all fallible functions; no `unwrap()`/`expect()` in production paths.
- [x] Capture pipeline is non-blocking: SCK callback uses `try_send` on a bounded `tokio::sync::mpsc`
      channel; encoding runs in `spawn_blocking`; the main egui thread is never stalled.
- [x] Observability plan: `tracing` + `tracing-subscriber` for structured logs; `frames_dropped`
      counter at channel back-pressure point; `tracing::error!` at every error site with
      `frame_index` and `pts` context fields.
- [x] Mandatory verification gates: `cargo fmt`, `cargo clippy --all-targets --all-features`,
      `cargo test` required before every merge.
- [x] `cargo audit` and `cargo deny` run in CI on every PR; triggered immediately because
      the dependency list (`screencapturekit`, `objc2-*`) includes C-FFI crates.

**Constitution Check Decision**: PASS. No unjustified violations.

---

## Project Structure

### Documentation

```text
specs/
├── instructions.md          # Product requirements (input)
├── 0001-research.md         # Architecture research (input)
├── 0002-tech-spec.md        # Technical specification (input)
└── 0003-impl-plan.md        # This file
```

### Source Code Layout

```text
screen-recorder/
├── Cargo.toml
├── Cargo.lock
├── rust-toolchain.toml
├── deny.toml                # cargo-deny policy
├── build.rs                 # (optional) plist / Info.plist generation
├── assets/
│   └── Info.plist           # NSScreenCaptureUsageDescription, NSMicrophoneUsageDescription
├── src/
│   ├── main.rs              # #![deny(unsafe_code)] at crate root; tokio Runtime; eframe::run_native
│   ├── app.rs               # App struct, egui::App impl, RecorderCommand dispatch
│   ├── error.rs             # AppError (thiserror)
│   ├── ui/
│   │   ├── mod.rs
│   │   ├── main_window.rs   # Start/Stop button, status badge, elapsed timer
│   │   ├── settings_panel.rs# Resolution, FPS, region, quality, mic, output folder
│   │   ├── preview_panel.rs # QuickTime launch or BGRA frame texture
│   │   └── save_panel.rs    # rfd::FileDialog, atomic save
│   ├── capture/
│   │   ├── mod.rs
│   │   ├── permissions.rs   # SCShareableContent enumeration, TCC check, onboarding
│   │   ├── content_filter.rs# SCContentFilter builder, self-exclusion
│   │   ├── engine.rs        # #[allow(unsafe_code)]; SCStream lifecycle; SCStreamOutputTrait
│   │   └── audio.rs         # Mic config, captureMicrophone guard (macOS 15+)
│   ├── encode/
│   │   ├── mod.rs
│   │   ├── pipeline.rs      # #[allow(unsafe_code)]; AVAssetWriter + AVAssetWriterInput
│   │   ├── sync.rs          # PtsNormalizer
│   │   └── temp_file.rs     # $TMPDIR/<uuid>.mp4 path management
│   ├── output/
│   │   ├── mod.rs
│   │   └── save.rs          # fs::rename (atomic); NSWorkspace::open for Finder reveal
│   └── config/
│       ├── mod.rs
│       └── settings.rs      # RecordingSettings, load/save JSON, Default impl
└── tests/
    ├── settings_roundtrip.rs# Integration: load → mutate → save → reload
    └── pts_normalizer.rs    # Unit: PtsNormalizer edge cases
```

**Structure Decision**: Single-project layout (Option 1). All platform-native calls are
isolated in `capture/` and `encode/` modules; all other modules are pure-Rust.

---

## Complexity Tracking

No constitution violations requiring justification.

---

## Phase 0: Research Resolutions

All unknowns from the technical context are resolved; no open NEEDS CLARIFICATION items
remain. The table below records the key decisions derived from `0001-research.md`.

| Decision | Rationale | Alternatives Rejected |
|---|---|---|
| **`screencapturekit` crate** as the SCK binding | Safe, idiomatic, IOSurface-aware, async-executor compatible; maintained actively | `scap` (immature, cross-platform abstractions remove zero-copy), raw C bindings (unfeasible timeline) |
| **AVAssetWriter** via `objc2-av-foundation` for encoding/muxing | Targets macOS 12.3+; native zero-copy CMSampleBuffer pass-through; full control over codec/bitrate | `SCRecordingOutput` (macOS 15+ only, loses M2–14 compatibility and frame-intercept capability); `ffmpeg-next` (breaks zero-copy, high complexity) |
| **NV12 (`kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange`) pixel format** | GPU performs RGB→YUV; hardware encoder ingests natively; no CPU conversion | BGRA (requires CPU-side color conversion at 4K/60FPS → thermal throttle) |
| **Bounded `tokio::sync::mpsc` channel** (capacity 120) for frame handoff | Decouples SCK GCD queue from encoding `spawn_blocking` task; back-pressure drops frames rather than stalling capture | `crossbeam` (adds dependency without benefit); unbounded channel (OOM risk on encoder stall) |
| **`PtsNormalizer`** with shared base-time normalization | Single common time-base guarantees A/V sync; handles SCK's variable-rate frame delivery on static screens | Per-stream normalization (produces audio/video drift) |
| **QuickTime for preview** (Phase M5 default) | Zero Rust code for MP4 decode; reliable; avoids `AVPlayerLayer` objc2 complexity | `image` crate frame-by-frame decode (slow, complex); `AVPlayerLayer` via objc2 (deferred to open question) |
| **`thiserror` domain errors + `anyhow` at `main`** | Domain errors in library layers; anyhow at application boundary only | `anyhow` everywhere (loses precision for UI error routing); `Box<dyn Error>` (loses type dispatch) |
| **egui/eframe** for UI | Pure Rust, immediate-mode, minimal overhead, no separate UI thread | Iced (more complex, heavier), AppKit/SwiftUI via objc2 (high FFI cost) |

---

## Phase 1: Design & Contracts

### Data Model (`data-model.md` summary)

#### Entities

| Entity | Key Fields | Notes |
|---|---|---|
| `RecordingSettings` | `resolution: Resolution`, `frame_rate: u32`, `region: CaptureRegion`, `capture_mic: bool`, `output_dir: PathBuf`, `quality: VideoQuality` | Serialized to `~/Library/Application Support/screen-recorder/settings.json` |
| `CaptureRegion` | `FullScreen { display_id: u32 }`, `Window { window_id: u32 }`, `Area { rect: CGRect }` | Maps 1:1 to `SCContentFilter` variants |
| `Resolution` | `Native`, `P1080`, `P720` | Native = display's native pixel size; others are fixed heights, width computed from aspect ratio |
| `VideoQuality` | `High` (8 Mbps), `Medium` (4 Mbps), `Low` (2 Mbps) | Mapped to `AVVideoAverageBitRateKey` in compression settings |
| `AppState` | `settings`, `recording_status: RecordingStatus`, `orchestrator: Option<RecordingOrchestrator>`, `preview_path`, `last_error` | Shared behind `Arc<Mutex<AppState>>`; updated from tokio tasks, read by egui render loop |
| `RecordingStatus` | `Idle`, `Recording { started_at }`, `Previewing`, `Saving` | Drives UI panel visibility |

#### State Transitions

```
Idle
  │  StartRecording command
  ▼
Recording { started_at }
  │  StopRecording command → encoder finalizes → temp MP4 ready
  ▼
Previewing
  ├─ AcceptPreview → Saving → (on success) Idle
  └─ DiscardPreview → (delete temp file) → Idle
```

#### Validation Rules

- `frame_rate` ∈ {24, 30, 60}
- `output_dir` must exist and be writable (checked before recording starts)
- `region: Area` rect must have width > 0 and height > 0
- If `capture_mic = true` and mic permission is denied → downgrade to `capture_mic = false`, emit `AppError::MicrophoneUnavailable`

### Contracts

This is a standalone desktop application with no external API surface. The only interface
contracts are user-facing command bindings and the serialized settings schema.

#### Settings JSON Schema

```json
{
  "resolution": "Native | P1080 | P720",
  "frame_rate": 30,
  "region": {
    "type": "FullScreen",
    "display_id": 1
  },
  "capture_mic": true,
  "output_dir": "/Users/alice/Desktop",
  "quality": "High | Medium | Low"
}
```

Deserialization uses `serde_json`; unknown fields are ignored (`#[serde(deny_unknown_fields)]`
is intentionally **not** set, for forward-compatibility with future settings additions).

#### Keyboard Shortcut Contract

| Action | Shortcut | Scope |
|---|---|---|
| Start recording | `⌘ Shift R` | Global (via `NSEvent::addGlobalMonitorForEvents`) |
| Stop recording | `⌘ Shift S` | Global |
| Accept preview | `⌘ Return` | App-local (egui `Context::input`) |
| Discard recording | `⌘ Delete` | App-local |

---

## Phase 2: Implementation Milestones

### M1 — Project Scaffold & Permissions

**Goal**: Repository compiles and passes all quality gates; egui window opens; TCC permission
check runs on launch; onboarding screen appears when access is denied.

**Tasks**:

1. Initialize `Cargo.toml` with all dependencies listed in tech spec §13.
2. Create `rust-toolchain.toml` pinned to `stable`.
3. Add `deny.toml` for `cargo deny` (license + security policy).
4. Write `#![deny(unsafe_code)]`, `#![warn(clippy::all, ...)]` in `main.rs`.
5. Implement `error.rs`: `AppError` enum with all variants.
6. Implement `config/settings.rs`: `RecordingSettings`, `Default`, `load_settings`, `save_settings`.
7. Implement `capture/permissions.rs`: async `check_screen_permission() -> Result<bool, AppError>`.
8. Implement `app.rs`: `AppState`, `RecordingStatus`, minimal egui `App` rendering a status label.
9. Implement `ui/main_window.rs`: stub Start button (disabled until permission granted).
10. Add `assets/Info.plist` with `NSScreenCaptureUsageDescription` and `NSMicrophoneUsageDescription`.

**Verification Gates**:
- `cargo fmt --check` → pass
- `cargo clippy --all-targets --all-features -- -D warnings` → pass
- `cargo test` → pass (settings round-trip test in `tests/settings_roundtrip.rs`)
- `cargo build --release` → binary runs, egui window appears, permission dialog triggers on first launch
- `cargo deny check` → pass
- `cargo audit` → pass

---

### M2 — Capture Engine

**Goal**: `SCStream` starts successfully; NV12 frames are received and logged; PTS normalization
is correct; frame-drop counter is observable via `tracing`.

**Tasks**:

1. Implement `capture/content_filter.rs`:
   - `list_displays() -> Result<Vec<SCDisplay>, AppError>`
   - `list_windows() -> Result<Vec<SCWindow>, AppError>`
   - `build_filter(region: &CaptureRegion) -> SCContentFilter` (with self-exclusion)
2. Implement `capture/engine.rs`:
   - `SCStreamConfiguration` builder from `RecordingSettings` (NV12 pixel format, correct FPS, audio enabled)
   - `CaptureEngine` struct owning `video_tx` and `audio_tx` channels
   - `SCStreamOutputTrait` impl: `try_send` for both screen and audio buffers; `frames_dropped` counter via `tracing::warn!`
   - `start() -> Result<(), AppError>` and `stop() -> Result<(), AppError>`
3. Implement `capture/audio.rs`: conditional `captureMicrophone` flag gated behind `#[cfg(target_os = "macos")]` version check.
4. Implement `encode/sync.rs`: `PtsNormalizer` with unit tests.
5. Wire `RecordingOrchestrator` in `app.rs` to call `CaptureEngine::start` and log received frame count.

**Verification Gates**:
- All quality gates from M1 (re-run)
- Unit test `tests/pts_normalizer.rs`: normalizer resets base, handles static-screen gaps, handles audio-before-video
- Manual smoke test: app starts recording, `tracing` output shows frame PTS values advancing, no frames dropped under light system load

---

### M3 — Encoding Pipeline (AVAssetWriter)

**Goal**: A complete `stop → encode → finalize` cycle produces a valid, playable MP4 file
with synchronized video and microphone audio tracks.

**Tasks**:

1. Implement `encode/temp_file.rs`: `TempFile` RAII struct; path at `$TMPDIR/screen-recorder/<uuid>.mp4`; `Delete` on drop unless `keep()` called.
2. Implement `encode/pipeline.rs`:
   - `EncodingPipeline::new(settings, video_rx, audio_rx, stop_rx) -> Self`
   - `AVAssetWriter` initialization targeting temp path, `.mp4` container
   - `AVAssetWriterInput` for video: H.264 (default) or HEVC based on `VideoQuality`; `expectsMediaDataInRealTime = true`; BT.709 color profile
   - `AVAssetWriterInput` for audio: AAC, 48 kHz, stereo, 128 kbps
   - Async write loop in `tokio::task::spawn_blocking`: `select!` over video_rx, audio_rx, stop_rx; `is_ready_for_more_media_data` check before each append
   - `finish() -> Result<PathBuf, AppError>`: `mark_as_finished` both inputs, `assetWriter.finishWriting`
3. Plumb `stop_tx` signal from `RecordingOrchestrator::stop()` into the encoding loop.
4. After `finish()`, update `AppState::preview_path` and transition to `RecordingStatus::Previewing`.

**Verification Gates**:
- All quality gates from M1 (re-run)
- Manual smoke test: record 5 s, stop → inspect output with `ffprobe`; must show:
  - Container: MP4
  - Video: H.264, expected resolution, close to target FPS
  - Audio: AAC, 48 kHz, stereo
  - Duration drift < 100 ms between audio and video tracks
- `cargo test` includes `tests/settings_roundtrip.rs` and `tests/pts_normalizer.rs`

---

### M4 — UI Complete (Controls, Settings, Shortcuts, Background)

**Goal**: Full UI feature set works; background recording is stable; keyboard shortcuts
function globally.

**Tasks**:

1. Implement `ui/settings_panel.rs`:
   - Resolution dropdown (`Native`, `1080p`, `720p`)
   - FPS spinner (24 / 30 / 60)
   - Region picker: `Full Screen` (display list from `list_displays()`), `Window` (window list), `Area` (placeholder — see Open Questions)
   - Quality slider (Low / Medium / High)
   - Mic toggle (disabled with warning banner if permission denied)
   - Output folder picker using `rfd::FileDialog::pick_folder`
   - Entire panel disabled when `RecordingStatus ≠ Idle`
2. Implement elapsed timer in `ui/main_window.rs`: updates every second via `egui::Context::request_repaint_after`.
3. Register global keyboard shortcuts via `NSEvent::addGlobalMonitorForEvents` in `capture/engine.rs` (Start = `⌘⇧R`, Stop = `⌘⇧S`); dispatch `RecorderCommand` to orchestrator channel.
4. Register app-local shortcuts in `app.rs` via `egui::Context::input_mut` (Accept = `⌘↩`, Discard = `⌘⌫`).
5. Background recording stability: verify capture loop is not linked to egui focus; add integration note that `NSApp` does not need to be in front.
6. Implement `RecordingStatus` badge: green dot + elapsed time when recording; grey + "Ready" when idle.

**Verification Gates**:
- All quality gates from M1 (re-run)
- Manual test: start via `⌘⇧R` while app is in background → recording continues → stop via `⌘⇧S` → temp file written
- Manual test: change each setting → verify `RecordingSettings` persisted to JSON after save
- Manual test: deny mic → banner visible → recording video-only succeeds

---

### M5 — Preview & Save Flow

**Goal**: User can preview the recording and save it to a chosen folder; discarding deletes
the temp file cleanly.

**Tasks**:

1. Implement `ui/preview_panel.rs`:
   - Launch QuickTime Player via `NSWorkspace::openURL(temp_path)` (default path)
   - Display Accept and Discard buttons
   - Accept transitions state to `Saving`; Discard deletes temp file and returns to `Idle`
2. Implement `ui/save_panel.rs`:
   - Show current `output_dir` from settings
   - "Change Folder" button → `rfd::FileDialog::pick_folder`
   - "Save" button → call `output::save::finalize`
3. Implement `output/save.rs`:
   - `finalize(src: &Path, dst_dir: &Path, name: &str) -> Result<PathBuf, AppError>`
   - Use `std::fs::rename`; fall back to `std::fs::copy` + `std::fs::remove_file` if rename fails (cross-volume)
   - After save, show path in a one-time toast (via egui notification or label with timeout)
   - Offer "Show in Finder" button: `NSWorkspace::activateFileViewerSelectingURLs([final_path])`
4. Implement auto-generated filename: `screen-recording-YYYY-MM-DD-HH-MM-SS.mp4` using `std::time::SystemTime`.

**Verification Gates**:
- All quality gates from M1 (re-run)
- Manual test: record → stop → QuickTime opens temp file → Accept → save to Desktop → file exists, playable in QuickTime
- Manual test: Discard → temp file deleted → `$TMPDIR/screen-recorder/` directory empty
- Manual test: output folder picker changes destination → reflected in next save
- `cargo test`: add `tests/save_roundtrip.rs` validating `finalize` with a dummy file on current volume and cross-volume simulation

---

### M6 — Polish, Error Handling, Observability & CI

**Goal**: All error paths show user-friendly dialogs; `tracing` output is structured and useful;
CI is green on `macos-14` runner; `cargo deny` and `cargo audit` pass.

**Tasks**:

1. Complete error dialog routing in `app.rs`:
   - `AppError::PermissionDenied` → onboarding screen with "Open System Settings" deep-link button
   - `AppError::StreamCreation(_)` → modal dialog with error message and "Try Again" button
   - `AppError::EncodingError(_)` → modal dialog; clean up temp file
   - `AppError::MicrophoneUnavailable` → non-blocking banner (video-only continues)
   - `AppError::Io` → modal with path and OS error message
2. Add `tracing-subscriber` initialization in `main.rs` with `EnvFilter` (respects `RUST_LOG`).
3. Audit all `tracing::*` call sites: ensure `frame_index`, `pts_secs`, and `module_path!()` are included where relevant.
4. Add `frames_dropped` and `frames_encoded` counters to `CaptureEngine`; log summary on `stop()`.
5. Create `.github/workflows/ci.yml`:
   ```yaml
   runs-on: macos-14
   steps:
     - cargo fmt --check
     - cargo clippy --all-targets --all-features -- -D warnings
     - cargo test --all-targets
     - cargo build --release
     - cargo deny check
     - cargo audit
   ```
6. Add `deny.toml` enforcing: no GPL dependencies, no yanked crates, no unmaintained crates.
7. Write `README.md` with build instructions, permissions setup, and keyboard shortcut reference.

**Verification Gates**:
- CI workflow passes end-to-end on `macos-14` GitHub Actions runner
- `cargo fmt --check` → pass
- `cargo clippy --all-targets --all-features -- -D warnings` → pass (zero warnings)
- `cargo test --all-targets` → pass
- `cargo build --release` → pass
- `cargo deny check` → pass
- `cargo audit` → pass
- Manual regression: full record → preview → save flow works on latest macOS

---

## Mandatory Verification Gates (All Phases)

The following commands **must pass** before any milestone is considered complete and before
any code is merged:

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
cargo build --release
cargo deny check    # run when dependencies change
cargo audit         # run when dependencies change
```

These gates are enforced by `.github/workflows/ci.yml` on every PR.

---

## Open Questions / Deferred Decisions

| Question | Latest Decision | Deferred To |
|---|---|---|
| **Area selection UX** | Transparent click-through `NSWindow` overlay may be needed; a transparent egui window is prototyped first | M4 spike |
| **Preview implementation** | Default: launch QuickTime via `NSWorkspace::openURL`; evaluate `AVPlayerLayer` via `objc2` as an in-app alternative | M5 evaluation |
| **macOS 15+ fast path** | After M3 validates AVAssetWriter, evaluate `SCRecordingOutput` as the default for macOS 15+ runtime detection | Post-M3 decision |
| **App bundle / notarization** | `cargo-bundle` for `.app` packaging; notarization deferred until distribution is planned | Post-M6 |
| **`captureMicrophone` on macOS < 15** | Property unavailable; fall back to `AVCaptureDevice` microphone input on older macOS | M3 research spike |
