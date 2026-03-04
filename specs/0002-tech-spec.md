# Technical Specification: macOS Screen Recorder (Rust)

**Version**: 1.0  
**Date**: 2026-03-04  
**Status**: Draft  
**Based on**: [0001-research.md](./0001-research.md) · [instructions.md](./instructions.md)

---

## 1. Overview

This document specifies the technical architecture, component design, data flow, crate selection, and implementation plan for a macOS screen recorder application written in Rust. The application is a lightweight desktop tool providing screen + microphone recording, MP4 export, and a simple egui-based UI with start/stop controls, region selection, preview before save, and quality/frame-rate settings.

### 1.1 Design Principles

- **Zero-copy first**: leverage ScreenCaptureKit's GPU-backed IOSurface pipeline; never copy pixel data to CPU unless strictly necessary.
- **Native macOS frameworks**: prefer `screencapturekit`, `objc2-av-foundation`, and `CoreMedia` over cross-platform abstractions.
- **Memory safety over convenience**: avoid `unwrap()`/`expect()` in production paths; propagate errors with `thiserror`-defined types.
- **Minimal dependency surface**: add a crate only when it provides clear value; audit with `cargo deny`.
- **Thin UI layer**: egui handles the control surface only; all media logic lives in separate modules.

---

## 2. System Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                          egui Application                           │
│  ┌──────────┐  ┌───────────────┐  ┌──────────────┐  ┌───────────┐ │
│  │ MainWindow│  │SettingsPanel │  │ PreviewPanel │  │ SavePanel │ │
│  └────┬─────┘  └───────┬───────┘  └──────┬───────┘  └─────┬─────┘ │
│       └────────────────┴──────────────────┴────────────────┘       │
│                              AppState (Arc<Mutex<>>)                │
└──────────────────────────────────┬──────────────────────────────────┘
                                   │ commands
                    ┌──────────────▼──────────────┐
                    │      RecordingOrchestrator   │
                    │  (owns capture + encode      │
                    │   lifecycle, tokio tasks)    │
                    └──────┬───────────────┬───────┘
                           │               │
             ┌─────────────▼─┐         ┌───▼────────────────┐
             │  CaptureEngine│         │  EncodingPipeline   │
             │               │         │                     │
             │ screencapturekit        │ AVAssetWriter       │
             │ SCStream       │  buf   │ (objc2-av-foundation│
             │ CMSampleBuffer ├────────► + core-media)       │
             │ IOSurface      │ channel│                     │
             └───────────────┘         └─────────┬───────────┘
                                                 │ MP4 file
                                        ┌────────▼──────────┐
                                        │   Output Manager  │
                                        │  (tmp → final dst)│
                                        └───────────────────┘
```

### 2.1 Process and Thread Model

| Thread / Task | Runtime | Responsibility |
|---|---|---|
| Main thread | OS | egui render loop (`eframe`), keyboard shortcut polling |
| Tokio async runtime | `tokio::runtime::Runtime` | Orchestrator, capture async callbacks, file I/O |
| SCK dispatch queue | macOS GCD | Delivers `CMSampleBuffer` frames (handled in SCStream callback) |
| Encoding serial queue | `tokio::task::spawn_blocking` | `AVAssetWriterInput.appendSampleBuffer` writes (must not block capture queue) |

---

## 3. Crate Selection

| Crate | Version (min) | Purpose |
|---|---|---|
| `screencapturekit` | 0.3 | Safe Rust bindings for Apple ScreenCaptureKit; SCStream, SCShareableContent, SCContentFilter, SCStreamConfiguration, SCRecordingOutput |
| `objc2` | 0.5 | Low-level Objective-C runtime messaging |
| `objc2-av-foundation` | 0.2 | AVAssetWriter, AVAssetWriterInput — video/audio muxing |
| `objc2-core-media` | 0.2 | CMSampleBuffer, CMTime, CMFormatDescription |
| `core-foundation` | 0.10 | CFRetain/CFRelease wrappers, CFString |
| `core-graphics` | 0.24 | CGDisplay, display enumeration |
| `eframe` / `egui` | 0.27 | Desktop UI framework (egui renderer + native window via `eframe`) |
| `tokio` | 1 (features: `full`) | Async runtime, `mpsc` channels, `spawn_blocking` |
| `thiserror` | 1 | Domain error enums |
| `anyhow` | 1 | Error propagation in `main` and CLI boundary only |
| `serde` + `serde_json` | 1 | Settings serialization |
| `tracing` + `tracing-subscriber` | 0.1 | Structured logging |
| `uuid` | 1 | Temporary file name generation |
| `dirs` | 5 | User home/desktop default path |

> **Not used**: `scap`, `xcap`, `ffmpeg-next` — cross-platform abstractions that sacrifice macOS zero-copy performance. FFmpeg is out of scope given MP4-only export on macOS 12.3+.

---

## 4. Module Structure

```
src/
├── main.rs                  # eframe::run_native entry point, tokio runtime init
├── app.rs                   # AppState, egui App impl, event dispatch
├── ui/
│   ├── mod.rs
│   ├── main_window.rs       # Start/Stop button, status indicator
│   ├── settings_panel.rs    # Resolution, frame rate, region picker, mic toggle
│   ├── preview_panel.rs     # Video preview (texture upload via egui)
│   └── save_panel.rs        # Folder picker, save confirmation
├── capture/
│   ├── mod.rs
│   ├── engine.rs            # SCStream lifecycle, SCStreamOutputTrait impl
│   ├── permissions.rs       # TCC check, onboarding guide
│   ├── content_filter.rs    # SCShareableContent enumeration, SCContentFilter
│   └── audio.rs             # Microphone capture config (captureMicrophone)
├── encode/
│   ├── mod.rs
│   ├── pipeline.rs          # AVAssetWriter + AVAssetWriterInput setup
│   ├── sync.rs              # PTS normalizer, base-time tracking
│   └── temp_file.rs         # Temp output path management
├── output/
│   ├── mod.rs
│   └── save.rs              # Move tmp → final destination, open in Finder
├── config/
│   ├── mod.rs
│   └── settings.rs          # RecordingSettings struct, load/save JSON
└── error.rs                 # AppError enum (thiserror)
```

---

## 5. Core Data Types

### 5.1 `RecordingSettings`

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RecordingSettings {
    pub resolution:   Resolution,      // e.g. Native | P1080 | P720
    pub frame_rate:   u32,             // 24 | 30 | 60
    pub region:       CaptureRegion,   // FullScreen(display_id) | Window(window_id) | Area(CGRect)
    pub capture_mic:  bool,
    pub output_dir:   PathBuf,
    pub quality:      VideoQuality,    // High | Medium | Low  (maps to VideoToolbox bitrate)
}
```

### 5.2 `CaptureRegion`

```rust
#[derive(Debug, Clone)]
pub enum CaptureRegion {
    FullScreen { display_id: u32 },
    Window     { window_id:  u32 },
    Area       { rect: CGRect },
}
```

### 5.3 `AppState`

```rust
pub struct AppState {
    pub settings:         RecordingSettings,
    pub recording_status: RecordingStatus,    // Idle | Recording | Previewing | Saving
    pub orchestrator:     Option<RecordingOrchestrator>,
    pub preview_path:     Option<PathBuf>,
    pub last_error:       Option<AppError>,
}

pub enum RecordingStatus {
    Idle,
    Recording { started_at: std::time::Instant },
    Previewing,
    Saving,
}
```

### 5.4 `AppError`

```rust
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Screen capture permission denied")]
    PermissionDenied,
    #[error("No shareable content available")]
    NoShareableContent,
    #[error("Failed to create SCStream: {0}")]
    StreamCreation(String),
    #[error("Encoding pipeline error: {0}")]
    EncodingError(String),
    #[error("File I/O error: {source}")]
    Io { #[from] source: std::io::Error },
    #[error("Microphone unavailable; recording video only")]
    MicrophoneUnavailable,
}
```

---

## 6. Phase-by-Phase Implementation

### 6.1 Phase 1: Permissions & Target Acquisition

**File**: `capture/permissions.rs`, `capture/content_filter.rs`

1. On startup, call `SCShareableContent::get_shareable_content_with_completion_handler` asynchronously.
2. If the returned display/window arrays are empty, present an onboarding screen directing the user to **System Settings → Privacy & Security → Screen Recording**.
3. `Info.plist` **must** include:
   - `NSScreenCaptureUsageDescription` — mandatory
   - `NSMicrophoneUsageDescription` — required if `capture_mic = true`
4. If sandboxed, `entitlements.plist` **must** include:
   - `com.apple.security.device.audio-input` = `true`
5. Self-exclusion: always pass `[NSRunningApplication currentApplication]` to `SCContentFilter`'s `excludingApplications` array.
6. For macOS 14+, optionally expose `SCContentSharingPicker` as an alternative region picker in the settings UI.

### 6.2 Phase 2: Stream Configuration

**File**: `capture/engine.rs`

```
SCStreamConfiguration:
  - width / height          → from RecordingSettings.resolution
  - minimumFrameInterval    → CMTime(1, frame_rate)
  - pixelFormat             → kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange  (NV12)
  - capturesAudio           → true
  - sampleRate              → 48_000
  - channelCount            → 2
  - captureMicrophone       → settings.capture_mic  (macOS 15.0+ only; guarded by #[cfg])
```

> **Critical**: Never use `kCVPixelFormatType_32BGRA` for continuous video recording. NV12 keeps the GPU performing the RGB→YUV conversion and avoids a costly CPU-side conversion before the hardware encoder.

### 6.3 Phase 3: Data Ingestion

**File**: `capture/engine.rs`

Implement `SCStreamOutputTrait`:

```rust
fn stream_did_output_sample_buffer(
    &self,
    stream: &SCStream,
    sample_buffer: CMSampleBuffer,
    output_type: SCStreamOutputType,
) {
    match output_type {
        SCStreamOutputType::Screen => {
            let _ = self.video_tx.try_send(sample_buffer);  // lock-free channel
        }
        SCStreamOutputType::Audio => {
            let _ = self.audio_tx.try_send(sample_buffer);
        }
        _ => {}
    }
}
```

- Use `tokio::sync::mpsc` bounded channels (capacity: ~120 frames = 2 s buffer at 60 FPS).
- **Never** lock the pixel buffer in the SCK callback; pass the opaque `CMSampleBuffer` to the encoding task.
- Handle back-pressure: if the channel is full, drop the frame and increment a `frames_dropped` metric.

### 6.4 Phase 4: PTS Normalization & Synchronization

**File**: `encode/sync.rs`

```rust
pub struct PtsNormalizer {
    base_time: Option<CMTime>,
}

impl PtsNormalizer {
    /// Returns a PTS relative to the first received sample.
    pub fn normalize(&mut self, pts: CMTime) -> CMTime {
        let base = *self.base_time.get_or_insert(pts);
        CMTimeSubtract(pts, base)
    }
}
```

- A single `PtsNormalizer` instance is shared between the video and audio encoding tasks (behind `Arc<Mutex<PtsNormalizer>>`).
- If SCK drops frames on static screens, the normalized PTS gaps are preserved faithfully — `AVAssetWriter` handles sparse video tracks correctly.
- Both video and audio samples use the same normalizer to guarantee a common time base of zero.

### 6.5 Phase 5: Encoding Pipeline (AVAssetWriter)

**File**: `encode/pipeline.rs`

Target: macOS 12.3+. Uses `AVAssetWriter` via `objc2-av-foundation`.

```
AVAssetWriter (output: temp_uuid.mp4, fileType: .mp4)
  ├── AVAssetWriterInput (video)
  │     mediaType:          .video
  │     outputSettings:     H.264 (AVVideoCodecTypeH264) or HEVC (AVVideoCodecTypeHEVC)
  │                         width/height from settings
  │                         AVVideoCompressionPropertiesKey → bitrate from VideoQuality
  │     expectsMediaDataInRealTime: true
  └── AVAssetWriterInput (audio)
        mediaType:          .audio
        outputSettings:     AAC, 44100 Hz, 2ch, 128 kbps
        expectsMediaDataInRealTime: true
```

**Write loop** (runs in `tokio::task::spawn_blocking`):

```
loop {
    select! {
        Some(buf) = video_rx.recv() => {
            if video_input.is_ready_for_more_media_data() {
                let pts = normalizer.normalize(buf.presentation_timestamp());
                video_input.append_sample_buffer_with_pts(buf, pts)?;
            }
        }
        Some(buf) = audio_rx.recv() => {
            if audio_input.is_ready_for_more_media_data() {
                let pts = normalizer.normalize(buf.presentation_timestamp());
                audio_input.append_sample_buffer_with_pts(buf, pts)?;
            }
        }
        _ = stop_signal.recv() => break,
    }
}
video_input.mark_as_finished();
audio_input.mark_as_finished();
asset_writer.finish_writing().await?;
```

**Color profile**: pass `kCVImageBufferICCProfileKey = BT.709` in the pixel buffer attachments to avoid washed-out colors after YUV→display conversion.

### 6.6 macOS 15+ Fast Path (SCRecordingOutput)

For deployments targeting exclusively macOS 15.0 (Sequoia) and later, the application may optionally substitute the AVAssetWriter pipeline with `SCRecordingOutput`:

- Attach an `SCRecordingOutput` (configured with H.264 codec and MP4 container) to the `SCStream` before starting.
- Remove the `video_tx`/`audio_tx` channels and the entire encoding pipeline.
- The OS handles all synchronization, muxing, and hardware encoding.

This path is guarded behind a runtime `os_version >= 15.0` check (using `NSProcessInfo.operatingSystemVersion`). The default build targets macOS 12.3+.

---

## 7. UI Architecture (egui / eframe)

### 7.1 Application Loop

```rust
eframe::run_native(
    "Screen Recorder",
    options,
    Box::new(|cc| Box::new(App::new(cc, tokio_handle))),
)
```

The `tokio::runtime::Runtime` is created in `main.rs` and its handle is passed into `App`. All UI interactions dispatch `RecorderCommand` events through a `tokio::sync::mpsc::UnboundedSender`.

### 7.2 Screen States

```
Idle ──Start──► Recording ──Stop──► Previewing ──Accept──► Saving ──Done──► Idle
                                          └──Discard──────────────────────► Idle
```

### 7.3 Panel Descriptions

| Panel | Widgets | Notes |
|---|---|---|
| `MainWindow` | Start/Stop button, status badge, elapsed timer, mic indicator | Status badge color: green (recording), grey (idle) |
| `SettingsPanel` | Resolution dropdown, FPS spinner, region picker (Full Screen / Window / Area), quality slider, mic toggle, output folder picker | Shown as collapsible sidebar; disabled during recording |
| `PreviewPanel` | `egui::Image` texture, play/pause button, accept/discard buttons | Load temp MP4 frame-by-frame via a background decode task using `image` crate; or use system `open` to preview in QuickTime |
| `SavePanel` | Folder path display, `rfd::FileDialog` trigger, Save button | `rfd` crate provides native macOS file picker dialog |

### 7.4 Keyboard Shortcuts

| Action | Default Shortcut |
|---|---|
| Start recording | `⌘ Shift R` |
| Stop recording | `⌘ Shift S` |
| Accept preview | `⌘ Return` |
| Discard recording | `⌘ Delete` |

Keyboard shortcuts are registered via `egui::Context::input` per-frame polling. Global shortcuts (when the app is in background) are registered with `NSEvent::addGlobalMonitorForEvents` (wrapped via `objc2`).

---

## 8. Configuration Persistence

**File**: `config/settings.rs`

Settings are serialized to JSON and stored at `~/Library/Application Support/screen-recorder/settings.json`.

```rust
pub fn load_settings() -> RecordingSettings {
    let path = settings_path();
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_settings(settings: &RecordingSettings) -> Result<(), AppError> {
    let path = settings_path();
    std::fs::create_dir_all(path.parent().unwrap())?;
    let json = serde_json::to_string_pretty(settings)?;
    std::fs::write(path, json)?;
    Ok(())
}
```

---

## 9. Output File Strategy

1. On recording start, generate a temporary file path: `$TMPDIR/screen-recorder/<uuid>.mp4`.
2. `AVAssetWriter` writes to this temp path.
3. On successful finalization, the file is moved to the user-selected destination via `std::fs::rename` (atomic on same volume; falls back to copy+delete across volumes).
4. On discard, the temp file is deleted.
5. Display the final path in a toast notification and offer "Show in Finder" via `NSWorkspace::activateFileViewerSelectingURLs`.

---

## 10. Error Handling Strategy

- All capture and encoding errors propagate via `Result<T, AppError>`.
- On non-recoverable error (e.g., `StreamCreation`), the orchestrator sends an `AppEvent::Error(AppError)` to the UI channel; the UI displays a modal dialog.
- On `MicrophoneUnavailable`, the system continues with video-only and shows a non-blocking banner.
- On `PermissionDenied`, the UI transitions to an onboarding screen with a deep-link button to open System Settings.
- `tracing::error!` is called at every error site with contextual fields (e.g., `frame_index`, `pts`).

---

## 11. Security and Privacy

| Requirement | Implementation |
|---|---|
| Screen recording permission | TCC check on startup; graceful onboarding if denied |
| Microphone permission | `NSMicrophoneUsageDescription` in `Info.plist`; audio disabled if denied |
| Sandbox audio entitlement | `com.apple.security.device.audio-input = true` in entitlements if sandboxed |
| No unsafe pixel access | Pixel buffer locked only when thumbnailing; raw pointer never exposed past lock guard |
| No network access | App is fully offline; no network entitlements declared |
| Secret handling | No secrets or tokens; no `.env` needed |

---

## 12. Performance Targets

| Scenario | Target |
|---|---|
| CPU usage during 1080p/30FPS recording | < 10% on Apple Silicon |
| CPU usage during 4K/60FPS recording | < 20% on Apple Silicon |
| Frame drop rate under normal load | < 1% |
| Recording start latency (click → first frame) | < 500 ms |
| Encoding pipeline memory (RSS delta) | < 150 MB above idle |
| App launch to ready-to-record | < 2 s |

---

## 13. Build Configuration

### `Cargo.toml` (relevant sections)

```toml
[package]
name    = "screen-recorder"
version = "0.1.0"
edition = "2024"

[dependencies]
screencapturekit    = "0.3"
objc2               = "0.5"
objc2-av-foundation = "0.2"
objc2-core-media    = "0.2"
core-foundation     = "0.10"
core-graphics       = "0.24"
eframe              = "0.27"
egui                = "0.27"
tokio               = { version = "1", features = ["full"] }
thiserror           = "1"
anyhow              = "1"
serde               = { version = "1", features = ["derive"] }
serde_json          = "1"
tracing             = "0.1"
tracing-subscriber  = { version = "0.3", features = ["env-filter"] }
uuid                = { version = "1", features = ["v4"] }
dirs                = "5"
rfd                 = "0.14"

[profile.release]
lto             = true
codegen-units   = 1
strip           = true
opt-level       = 3

[profile.dev]
incremental     = true
```

### `rust-toolchain.toml`

```toml
[toolchain]
channel = "stable"
```

### Lint Configuration (in `main.rs`)

```rust
#![deny(unsafe_code)]
#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![warn(rust_2018_idioms)]
```

> `unsafe_code` will need a targeted `#[allow(unsafe_code)]` in `capture/engine.rs` and `encode/pipeline.rs` for the unavoidable Objective-C FFI calls. Each `unsafe` block must include a `// SAFETY:` comment.

---

## 14. Testing Strategy

| Test Type | Scope | Tools |
|---|---|---|
| Unit | `PtsNormalizer`, `RecordingSettings` serialization, `AppError` display | `cargo test` in-module |
| Integration | Permission check flow, settings load/save round-trip | `tests/` directory |
| Manual / CI smoke | Full record → stop → preview → save flow on macOS 14+ | macOS GitHub Actions runner |
| Parameterized | Frame rate / resolution combinations for `SCStreamConfiguration` builder | `rstest` |
| Error path | Simulated permission denial, disk full condition | `mockall` for trait mocks |

> Screen capture tests that require TCC permission are skipped in headless CI (`#[cfg_attr(not(feature = "integration"), ignore)]`).

---

## 15. CI/CD Pipeline

```yaml
# .github/workflows/ci.yml (outline)
steps:
  - cargo fmt --check
  - cargo clippy --all-targets --all-features -- -D warnings
  - cargo test --all-targets
  - cargo build --release
  - cargo deny check
  - cargo audit
```

macOS runner required (`macos-14` for Apple Silicon) because `screencapturekit` and `objc2-av-foundation` link against system frameworks unavailable on Linux.

---

## 16. Delivery Milestones

| Milestone | Deliverable |
|---|---|
| M1 | Project scaffold: workspace layout, `Cargo.toml`, TCC permission check, egui window renders |
| M2 | Capture engine: SCStream starts, NV12 frames received, PTS normalization working |
| M3 | Encoding pipeline: AVAssetWriter produces valid MP4 from screen + mic |
| M4 | UI complete: start/stop, settings panel, keyboard shortcuts, background recording |
| M5 | Preview + save flow: temp file playback, folder picker, atomic move to destination |
| M6 | Polish: error dialogs, onboarding, metrics/logging, CI green |

---

## 17. Open Questions / Future Work

- **macOS 15 fast path**: After M3 validates AVAssetWriter path, evaluate shipping `SCRecordingOutput` as the default for macOS 15+ builds.
- **Area selection UX**: Drawing a selection overlay in egui requires a transparent click-through window; may require a minimal AppKit `NSWindow` overlay — to be prototyped in M4.
- **Preview implementation**: egui does not natively decode MP4 for playback. Options: (a) launch QuickTime via `NSWorkspace::open`, (b) decode frames with `image` + BGRA texture upload, (c) embed an `AVPlayerLayer` via `objc2`. Decision deferred to M5.
- **Notarization**: If distributed outside the App Store, the binary must be signed and notarized; `com.apple.developer.persistent-content-capture` requires Apple approval for use in production.
