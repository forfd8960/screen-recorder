# Tasks: macOS Screen Recorder

**Input**: [0003-impl-plan.md](./0003-impl-plan.md) · [0002-tech-spec.md](./0002-tech-spec.md) · [instructions.md](./instructions.md)  
**Date**: 2026-03-04

**Tests**: Tests are REQUIRED by the constitution. Test tasks appear before their implementation counterparts within each user story phase.

**Format**: `[ID] [P?] [Story?] Description with file path`  
- **[P]**: Parallelizable (different files, no shared dependencies)  
- **[Story]**: User story label (US1–US8)

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Repository skeleton, toolchain lock, cargo policy, lint configuration.  
All tasks here are independent and can run in parallel.

- [X] T001 [P] Initialize `Cargo.toml` with all dependencies from tech spec §13 (screencapturekit, objc2-av-foundation, eframe, tokio, thiserror, serde, tracing, uuid, rfd, etc.)
- [X] T002 [P] Create `rust-toolchain.toml` pinned to `stable` channel
- [X] T003 [P] Create `deny.toml` with cargo-deny policy: no GPL deps, no yanked crates, no unmaintained crates
- [X] T004 [P] Add top-level lint attributes to `src/main.rs`: `#![deny(unsafe_code)]`, `#![warn(clippy::all, clippy::pedantic, clippy::nursery)]`, `#![warn(rust_2018_idioms)]`
- [X] T005 [P] Create `assets/Info.plist` with `NSScreenCaptureUsageDescription` and `NSMicrophoneUsageDescription` keys
- [X] T006 [P] Create `src/ui/mod.rs`, `src/capture/mod.rs`, `src/encode/mod.rs`, `src/output/mod.rs`, `src/config/mod.rs` stub modules
- [X] T007 [P] Create `.github/workflows/ci.yml` skeleton: `cargo fmt --check`, `cargo clippy`, `cargo test`, `cargo build --release`, `cargo deny check`, `cargo audit` on `macos-14` runner
- [X] T008 Verify `cargo build` compiles the stub project with zero errors and zero warnings

**Checkpoint**: Project compiles, lint gates pass, CI skeleton exists.

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Core domain types and infrastructure that every user story depends on.
Must be 100% complete before any user story phase starts.

**⚠️ CRITICAL**: No user story work can begin until this phase is complete.

- [X] T009 Implement `src/error.rs`: `AppError` enum with all variants (`PermissionDenied`, `NoShareableContent`, `StreamCreation`, `EncodingError`, `Io`, `MicrophoneUnavailable`) using `thiserror`
- [X] T010 [P] Implement `src/config/settings.rs`: `Resolution`, `VideoQuality`, `CaptureRegion`, `RecordingSettings` structs with `serde` derives and `Default` impl; `load_settings()` and `save_settings()` using `~/Library/Application Support/screen-recorder/settings.json`
- [X] T011 [P] Implement `src/app.rs`: `RecordingStatus` enum (`Idle`, `Recording { started_at }`, `Previewing`, `Saving`); `AppState` struct; `RecorderCommand` enum; `tokio::sync::mpsc::UnboundedSender<RecorderCommand>` wired into egui `App`
- [X] T012 [P] Wire `tokio::runtime::Runtime` creation in `src/main.rs`; pass handle into `App::new`; call `eframe::run_native`
- [X] T013 [P] Initialize `tracing-subscriber` with `EnvFilter` in `src/main.rs`; respects `RUST_LOG` env var
- [X] T014 Write integration test `tests/settings_roundtrip.rs`: `Default` → serialize → write → read → deserialize → assert fields equal; covers `Resolution`, `VideoQuality`, `CaptureRegion` variants
- [X] T015 [P] Write unit test in `src/config/settings.rs` module: `RecordingSettings::default()` produces valid values; `frame_rate` ∈ {24, 30, 60} variants compile correctly
- [X] T016 Verify: `cargo test` passes T014/T015; `cargo fmt --check` passes; `cargo clippy -- -D warnings` passes

**Checkpoint**: Foundation ready — all user story phases can now start.

---

## Phase 3: User Story 1 — Start/Stop Recording (Priority: P1) 🎯 MVP

**Goal**: User can open the app, click Start, recording begins; clicking Stop finalizes the recording and transitions to Previewing state.

**Independent Test**: Launch app → click Start → observe status changes to "Recording" with elapsed timer → click Stop → `AppState::recording_status` transitions to `Previewing` and a temp MP4 path is stored.

### Tests for User Story 1 (REQUIRED) ⚠️

> Write these tests FIRST; they must FAIL before implementation begins.

- [ ] T017 [P] [US1] Write unit test in `src/capture/permissions.rs`: `check_screen_permission` returns `Err(AppError::PermissionDenied)` when shareable content arrays are empty (mock via trait)
- [ ] T018 [P] [US1] Write unit test in `src/encode/sync.rs`: `PtsNormalizer::normalize` — first call sets base, subsequent calls return monotonically increasing relative times; handles static-screen frame gaps; handles audio-arrives-before-video ordering
- [ ] T019 [US1] Write integration test `tests/pts_normalizer.rs`: full scenario — 5 video PTS values with a 2 s gap in the middle — assert output times are all non-negative and correctly offset

### Implementation for User Story 1

- [ ] T020 [US1] Implement `src/capture/permissions.rs`: async `check_screen_permission() -> Result<bool, AppError>` calling `SCShareableContent::get_shareable_content_with_completion_handler`; return `Err(AppError::PermissionDenied)` if arrays empty
- [ ] T021 [US1] Implement `src/encode/sync.rs`: `PtsNormalizer` struct with `base_time: Option<CMTime>` and `normalize(&mut self, pts: CMTime) -> CMTime`; shared via `Arc<Mutex<PtsNormalizer>>`
- [ ] T022 [US1] Implement `src/encode/temp_file.rs`: `TempFile` RAII struct; generates path `$TMPDIR/screen-recorder/<uuid>.mp4`; deletes file on `Drop` unless `keep()` was called; creates parent directory if missing
- [ ] T023 [US1] Implement `src/capture/engine.rs`: `CaptureEngine` struct with bounded `tokio::sync::mpsc` channels (capacity 120 each for video and audio); `SCStreamOutputTrait` impl using `try_send`; `frames_dropped: AtomicU64` counter; `#[allow(unsafe_code)]` scoped to module with per-block `// SAFETY:` comments
- [ ] T024 [US1] Implement `CaptureEngine::start() -> Result<(), AppError>` and `CaptureEngine::stop() -> Result<(), AppError>` in `src/capture/engine.rs`
- [ ] T025 [US1] Implement `src/encode/pipeline.rs`: `EncodingPipeline::new(settings, video_rx, audio_rx, stop_rx)`; `AVAssetWriter` + `AVAssetWriterInput` for video (H.264, `expectsMediaDataInRealTime = true`, BT.709 color profile) and audio (AAC 48 kHz stereo 128 kbps); `finish() -> Result<PathBuf, AppError>`; `#[allow(unsafe_code)]` with `// SAFETY:` comments
- [ ] T026 [US1] Implement async write loop in `src/encode/pipeline.rs`: `spawn_blocking` task; `select!` over `video_rx`, `audio_rx`, `stop_rx`; `is_ready_for_more_media_data` guard before each append; call `PtsNormalizer::normalize` for both streams
- [ ] T027 [US1] Implement `RecordingOrchestrator` in `src/app.rs`: owns `CaptureEngine` + `EncodingPipeline`; `start() -> Result<(), AppError>` wires channels and starts both; `stop() -> Result<(), AppError>` sends stop signal, awaits `finish()`, stores `preview_path`, transitions `RecordingStatus` to `Previewing`
- [ ] T028 [US1] Implement `src/ui/main_window.rs`: Start button (disabled if `RecordingStatus ≠ Idle` or permission denied); Stop button (visible only when `Recording`); status badge (green "● Recording" / grey "Ready"); dispatch `RecorderCommand::Start` / `::Stop` on click
- [ ] T029 [US1] Wire TCC onboarding in `src/app.rs`: if `check_screen_permission` returns `Err(PermissionDenied)`, set `AppState::last_error`; render onboarding panel with "Open System Settings" deep-link via `NSWorkspace::openURL`

**Checkpoint**: App compiles; Start → Recording → Stop → Previewing state flow works end-to-end; temp MP4 file exists in `$TMPDIR/screen-recorder/`.

---

## Phase 4: User Story 2 — Record Screen + Microphone (Priority: P1)

**Goal**: Output MP4 contains a synchronized audio track from the microphone; if permission is denied the app continues video-only with a visible banner.

**Independent Test**: Record 5 s with mic → `ffprobe` shows both a video stream (H.264) and an audio stream (AAC) with duration drift < 100 ms; deny mic → non-blocking banner appears → video-only MP4 still produced.

### Tests for User Story 2 (REQUIRED) ⚠️

- [ ] T030 [P] [US2] Write unit test in `src/capture/audio.rs`: `build_audio_config(capture_mic: true)` sets `capturesAudio = true`; `build_audio_config(capture_mic: false)` sets `capturesAudio = false`; `captureMicrophone` field set only on macOS 15+ (compile-time or runtime guard)
- [ ] T031 [P] [US2] Write integration test `tests/audio_pipeline.rs` (gated `#[cfg_attr(not(feature = "integration"), ignore)]`): mock audio `CMSampleBuffer` appended to `EncodingPipeline` produces non-zero audio track duration in finalized MP4

### Implementation for User Story 2

- [ ] T032 [US2] Implement `src/capture/audio.rs`: `build_audio_config(settings: &RecordingSettings) -> SCStreamConfiguration` fragment setting `captures_audio`, `sample_rate = 48_000`, `channel_count = 2`; conditional `captureMicrophone` property guarded by runtime `os_version >= 15.0` check via `NSProcessInfo`
- [ ] T033 [US2] Update `CaptureEngine` in `src/capture/engine.rs`: merge audio config from `capture/audio.rs` into `SCStreamConfiguration` builder; ensure audio `CMSampleBuffer` objects flow through `audio_tx` channel to `EncodingPipeline`
- [ ] T034 [US2] Update `RecordingOrchestrator::start()` in `src/app.rs`: check mic permission before starting; if denied and `settings.capture_mic = true`, set `settings.capture_mic = false`, emit `AppError::MicrophoneUnavailable`, continue with video-only
- [ ] T035 [US2] Add mic-unavailable banner to `src/ui/main_window.rs`: non-blocking yellow banner shown when `AppState::last_error == Some(MicrophoneUnavailable)`; dismissible; does not block recording
- [ ] T036 [US2] Add `tracing::warn!` call with `pts_secs` and `stream = "audio"` context in the audio branch of the `SCStreamOutputTrait` implementation

**Checkpoint**: `ffprobe` on recorded MP4 shows two streams (video + audio) with correct codecs; mic-denied path shows banner and produces video-only file.

---

## Phase 5: User Story 3 — Select Recording Area (Priority: P1)

**Goal**: User can choose full screen (specific display), a specific window, or a custom rectangular area; the selected region is applied to `SCContentFilter` before recording starts.

**Independent Test**: Configure Full Screen → record → video resolution matches display dimensions; configure Window → record → video shows only that window (including when occluded).

### Tests for User Story 3 (REQUIRED) ⚠️

- [ ] T037 [P] [US3] Write unit tests in `src/capture/content_filter.rs`: `build_filter(FullScreen { display_id: 1 })` produces a filter with the correct display; `build_filter(Window { window_id: 42 })` produces a window filter; self-exclusion entry is always present in `excludingApplications`
- [ ] T038 [P] [US3] Write unit test: `build_filter(Area { rect })` with zero-width rect returns `Err(AppError::InvalidRegion)` (add `InvalidRegion` variant to `AppError`)

### Implementation for User Story 3

- [ ] T039 [US3] Add `AppError::InvalidRegion(String)` variant to `src/error.rs`
- [ ] T040 [US3] Implement `src/capture/content_filter.rs`: `list_displays() -> Result<Vec<SCDisplay>, AppError>`; `list_windows() -> Result<Vec<SCWindow>, AppError>`; `build_filter(region: &CaptureRegion) -> Result<SCContentFilter, AppError>` with self-exclusion and `InvalidRegion` validation for zero-size `Area` rect
- [ ] T041 [US3] Update `CaptureEngine::start()` in `src/capture/engine.rs` to call `build_filter` with `settings.region` and pass result to `SCStream` initialization
- [ ] T042 [US3] Implement region picker in `src/ui/settings_panel.rs`: Full Screen combo box (populated from `list_displays()` async call on panel open); Window combo box (populated from `list_windows()`); Area option (shows placeholder text referencing open-question status); entire picker disabled while `RecordingStatus ≠ Idle`
- [ ] T043 [US3] Populate display/window lists asynchronously in `src/app.rs` on startup (after permission check); store in `AppState`; refresh when settings panel opens

**Checkpoint**: Selecting Full Screen and pressing Start records the correct display; selecting a Window records only that window.

---

## Phase 6: User Story 4 — Preview Before Save (Priority: P2)

**Goal**: After stopping, the recording is immediately opened in QuickTime Player for review; user can Accept (proceed to save) or Discard (delete temp file).

**Independent Test**: Record 3 s → Stop → QuickTime opens temp MP4 and plays it → Accept button transitions `RecordingStatus` to `Saving`; Discard button deletes temp file and returns to `Idle`.

### Tests for User Story 4 (REQUIRED) ⚠️

- [ ] T044 [P] [US4] Write unit test in `src/ui/preview_panel.rs`: `handle_discard(temp_path)` deletes the file and emits `RecorderCommand::Discard`; `handle_accept()` emits `RecorderCommand::Accept`
- [ ] T045 [P] [US4] Write integration test `tests/preview_flow.rs` (gated `#[cfg_attr(not(feature = "integration"), ignore)]`): `TempFile` created → `keep()` called → `AppState` transitions through `Previewing` → `Discard` → file is deleted → status returns to `Idle`

### Implementation for User Story 4

- [ ] T046 [US4] Add `RecorderCommand::Accept` and `RecorderCommand::Discard` variants to the command enum in `src/app.rs`
- [ ] T047 [US4] Implement `src/ui/preview_panel.rs`: shown when `RecordingStatus == Previewing`; calls `NSWorkspace::openURL(preview_path)` via `objc2` to launch QuickTime; renders Accept and Discard buttons
- [ ] T048 [US4] Handle `RecorderCommand::Accept` in orchestrator: transition `RecordingStatus` to `Saving`
- [ ] T049 [US4] Handle `RecorderCommand::Discard` in orchestrator: call `std::fs::remove_file(preview_path)`; log `tracing::info!` on success; transition `RecordingStatus` to `Idle`; clear `AppState::preview_path`
- [ ] T050 [US4] Register app-local shortcut `⌘ Return` → `RecorderCommand::Accept` and `⌘ Delete` → `RecorderCommand::Discard` in `src/app.rs` via `egui::Context::input_mut`

**Checkpoint**: Stop → QuickTime opens automatically; Accept advances to Saving; Discard removes temp file and shows Idle UI.

---

## Phase 7: User Story 5 — Save to Chosen Folder as MP4 (Priority: P2)

**Goal**: User selects a destination folder and saves the recording as a timestamped MP4; the app shows a completion toast with a "Show in Finder" button.

**Independent Test**: Accept preview → change output folder via picker → Save → MP4 file exists at `<folder>/screen-recording-YYYY-MM-DD-HH-MM-SS.mp4`; re-opening app shows the chosen folder persisted in settings.

### Tests for User Story 5 (REQUIRED) ⚠️

- [ ] T051 [P] [US5] Write unit test in `src/output/save.rs`: `generate_filename()` returns string matching `screen-recording-YYYY-MM-DD-HH-MM-SS.mp4` format using regex; timestamps differ across consecutive calls
- [ ] T052 [P] [US5] Write integration test `tests/save_roundtrip.rs`: create temp file → `finalize(src, dst_dir, name)` → assert file exists at destination and source is gone; simulate cross-volume by copying to a different temp dir and asserting fallback copy+delete path works

### Implementation for User Story 5

- [ ] T053 [US5] Implement `src/output/save.rs`: `generate_filename() -> String` using `std::time::SystemTime` formatted as `screen-recording-YYYY-MM-DD-HH-MM-SS.mp4`; `finalize(src: &Path, dst_dir: &Path, name: &str) -> Result<PathBuf, AppError>` using `std::fs::rename`, falling back to `std::fs::copy` + `std::fs::remove_file` on `EXDEV` cross-volume error
- [ ] T054 [US5] Implement "Show in Finder" action in `src/output/save.rs`: `reveal_in_finder(path: &Path) -> Result<(), AppError>` via `NSWorkspace::activateFileViewerSelectingURLs` wrapped with `objc2`
- [ ] T055 [US5] Implement `src/ui/save_panel.rs`: shown when `RecordingStatus == Saving`; displays current `output_dir`; "Change Folder" button triggers `rfd::AsyncFileDialog::pick_folder`, updates `settings.output_dir`, persists via `save_settings`; "Save" button calls `output::save::finalize` then `reveal_in_finder`
- [ ] T056 [US5] Add completion toast to `src/ui/save_panel.rs`: after successful save, display final file path for 5 s via timed egui label (`request_repaint_after`); include "Show in Finder" hyperlink button
- [ ] T057 [US5] Handle `AppError::Io` in `src/app.rs` save path: render modal dialog with OS error message and "Retry" button that re-invokes `finalize`

**Checkpoint**: Full record → preview → save flow produces a valid MP4 at the chosen destination; Finder reveals the file; settings persist across app restarts.

---

## Phase 8: User Story 6 — Configure Quality and Frame Rate (Priority: P2)

**Goal**: User-selected quality and frame rate settings are applied to the next recording; the values persist across app restarts.

**Independent Test**: Set quality = Low, FPS = 24 → record 5 s → `ffprobe` shows average bitrate ≈ 2 Mbps and frame rate ≈ 24 FPS; restart app → settings panel shows Low / 24.

### Tests for User Story 6 (REQUIRED) ⚠️

- [ ] T058 [P] [US6] Write unit test in `src/capture/engine.rs`: `build_stream_config(settings)` with `VideoQuality::Low` sets `minimumFrameInterval` for 24 FPS and `AVVideoAverageBitRateKey = 2_000_000`; same for `Medium` / `High`; `Resolution::P1080` sets `height = 1080`
- [ ] T059 [P] [US6] Write unit test `RecordingSettings` round-trip with all three `VideoQuality` and all three `Resolution` variants via serde JSON

### Implementation for User Story 6

- [ ] T060 [US6] Implement `build_stream_config(settings: &RecordingSettings) -> SCStreamConfiguration` in `src/capture/engine.rs`: width/height from `Resolution`; `minimumFrameInterval` from `frame_rate`; NV12 pixel format; audio enabled
- [ ] T061 [US6] Implement `VideoQuality → bitrate` mapping in `src/encode/pipeline.rs`: `High = 8_000_000`, `Medium = 4_000_000`, `Low = 2_000_000` bps; passed to `AVVideoAverageBitRateKey` in compression properties dict
- [ ] T062 [US6] Implement quality/FPS widgets in `src/ui/settings_panel.rs`: FPS `egui::ComboBox` with options {24, 30, 60}; quality `egui::Slider` or segmented control (Low / Medium / High); resolution `egui::ComboBox` (Native / 1080p / 720p); all disabled when `RecordingStatus ≠ Idle`; changes immediately update `AppState::settings` and call `save_settings`

**Checkpoint**: Quality and FPS controls visible in settings; `ffprobe` confirms applied values in output file; settings survive app restart.

---

## Phase 9: User Story 7 — Keyboard Shortcuts (Priority: P3)

**Goal**: `⌘⇧R` starts recording and `⌘⇧S` stops recording from any app; shortcuts fire the same code path as button clicks.

**Independent Test**: Launch app; switch to another app; press `⌘⇧R` → `AppState::recording_status` changes to `Recording` within 500 ms; press `⌘⇧S` → status transitions to `Previewing`.

### Tests for User Story 7 (REQUIRED) ⚠️

- [ ] T063 [P] [US7] Write unit test in `src/app.rs`: `handle_command(RecorderCommand::Start)` when status is `Idle` calls `orchestrator.start()`; `handle_command(RecorderCommand::Stop)` when status is `Recording` calls `orchestrator.stop()`; both are no-ops in incorrect states

### Implementation for User Story 7

- [ ] T064 [US7] Register global event monitor in `src/capture/engine.rs`'s `start()` via `NSEvent::addGlobalMonitorForEvents(matching: .keyDown)` using `objc2`; detect `⌘⇧R` → send `RecorderCommand::Start`; detect `⌘⇧S` → send `RecorderCommand::Stop`; store monitor reference, remove on `stop()`
- [ ] T065 [US7] Register app-local shortcuts in `src/app.rs` egui update loop via `ctx.input_mut`: `⌘⇧R` and `⌘⇧S` map to the same `RecorderCommand`s as global shortcuts; ensures shortcuts work when app is in focus without global monitor
- [ ] T066 [US7] Document shortcut registration cleanup: assert global monitor `removeMonitor` is called when `CaptureEngine` is dropped (implement in `Drop` impl of `CaptureEngine`)

**Checkpoint**: Recording starts and stops via keyboard while app is in background; no duplicate events fired when app is in foreground.

---

## Phase 10: User Story 8 — Background Recording Stability (Priority: P3)

**Goal**: Recording continues without interruption when the user switches to another app; the SCK capture loop is decoupled from egui window focus.

**Independent Test**: Start recording → Cmd-Tab to another app → work for 30 s → Cmd-Tab back → Stop → output MP4 duration ≈ 30 s with no dropped-frame spikes in the log.

### Tests for User Story 8 (REQUIRED) ⚠️

- [ ] T067 [P] [US8] Write unit test in `src/capture/engine.rs`: `CaptureEngine` holds reference to tokio task handle; task does not depend on `App` struct being alive; cancellation via `stop_tx` works even if egui loop is not ticking

### Implementation for User Story 8

- [ ] T068 [US8] Audit `src/capture/engine.rs`: confirm `SCStream` dispatch queue and `spawn_blocking` encoding task have no references to egui `Context`, `Arc<App>`, or window handles; add comment block documenting that the pipeline is focus-independent
- [ ] T069 [US8] Set `eframe::NativeOptions::app_id` and `eframe::NativeOptions::run_and_return = false` in `src/main.rs` so the eframe event loop continues even when the window loses focus
- [ ] T070 [US8] Verify `frames_dropped` counter stays at 0 during a 30 s background recording under light load; add `tracing::info!` summary emission in `CaptureEngine::stop()` reporting `frames_dropped` and `frames_encoded`

**Checkpoint**: 30 s background recording produces a valid MP4 with near-zero frame drops; log shows `frames_dropped = 0` in the summary.

---

## Phase 11: Polish & Cross-Cutting Concerns

**Purpose**: Error dialog completeness, observability audit, CI green, documentation.

- [ ] T071 [P] Complete error dialog routing in `src/app.rs`: `PermissionDenied` → onboarding panel (deep-link to System Settings); `StreamCreation` → modal with "Try Again"; `EncodingError` → modal, clean up `TempFile`; `MicrophoneUnavailable` → non-blocking banner; `Io` → modal with OS error text
- [ ] T072 [P] Audit all `tracing::*` call sites across `src/`: ensure `frame_index` and `pts_secs` are present in video/audio callback spans; ensure no secrets or raw pixel pointers are logged
- [ ] T073 Add elapsed-timer widget to `src/ui/main_window.rs`: `egui::Context::request_repaint_after(Duration::from_secs(1))` in `update()`; display `HH:MM:SS` format during `RecordingStatus::Recording`
- [ ] T074 [P] Write `README.md`: build instructions (`cargo build --release`), macOS permissions setup guide, keyboard shortcut table, `RUST_LOG` usage for debug logging
- [ ] T075 [P] Create `tests/save_roundtrip.rs` and `tests/audio_pipeline.rs` if not already done in story phases; add `features = ["integration"]` flag in `Cargo.toml` to gate TCC-dependent tests
- [ ] T076 Perform final manual regression: Full record (1080p/30 FPS + mic) → Preview in QuickTime → Accept → Save to Desktop → `ffprobe` confirms H.264 video + AAC audio; repeat with area selection and FPS = 60
- [ ] T077 Run full verification suite and confirm all pass:
  - `cargo fmt --check`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo test --all-targets`
  - `cargo build --release`
  - `cargo deny check`
  - `cargo audit`

---

## Dependencies & Execution Order

### Phase Dependencies

```
Phase 1 (Setup)
    └─► Phase 2 (Foundational)  ◄── BLOCKS all user stories
              └─► Phase 3 (US1 — Start/Stop)         [P1, MVP]
              └─► Phase 4 (US2 — Screen+Mic)          [P1]
              └─► Phase 5 (US3 — Region Select)        [P1]
              └─► Phase 6 (US4 — Preview)              [P2, needs Phase 3]
              └─► Phase 7 (US5 — Save)                 [P2, needs Phase 6]
              └─► Phase 8 (US6 — Quality/FPS)          [P2]
              └─► Phase 9 (US7 — Shortcuts)            [P3, needs Phase 3]
              └─► Phase 10 (US8 — Background)          [P3, needs Phase 3]
                        └─► Phase 11 (Polish/CI)
```

### User Story Dependencies

| Story | Depends On | Can Parallel With |
|---|---|---|
| US1 Start/Stop | Phase 2 complete | US2, US3, US6 |
| US2 Screen+Mic | Phase 2 + US1 core engine (T023–T026) | US3, US6 |
| US3 Region Select | Phase 2 + US1 content_filter | US2, US6 |
| US4 Preview | US1 complete (stop → Previewing flow) | US5, US6 |
| US5 Save | US4 complete (Accept → Saving flow) | US6 |
| US6 Quality/FPS | Phase 2 complete | US1, US2, US3 |
| US7 Shortcuts | US1 complete | US8 |
| US8 Background | US1 complete | US7 |

### Parallel Opportunities Per Story

**Phase 1 (Setup)**: T001–T007 all parallelizable.

**Phase 2 (Foundational)**: T010, T011, T012, T013 parallelizable after T009 (AppError).

**Phase 3 (US1)**: T017, T018 (tests) in parallel; T020, T021, T022 in parallel; T023 and T025 in parallel after channels established.

**Phase 4 (US2)**: T030, T031 (tests) in parallel; T032, T033 in parallel; T035 independent.

**Phase 5 (US3)**: T037, T038 (tests) in parallel; T040, T043 in parallel after T039.

**Phase 6 (US4)**: T044, T045 (tests) in parallel; T046, T050 in parallel.

**Phase 7 (US5)**: T051, T052 (tests) in parallel; T053, T054 in parallel.

**Phase 8 (US6)**: T058, T059 (tests) in parallel; T060, T061 in parallel.

**Phase 11 (Polish)**: T071, T072, T073, T074, T075 all parallelizable.

---

## Implementation Strategy

**MVP Scope (Phase 1 + 2 + 3 only = T001–T029)**:  
Delivers US1 (Start/Stop Recording) — the app compiles, requests permission, starts/stops an SCK capture session, encodes to a temp MP4 via AVAssetWriter, and transitions to the Previewing state. This is the minimal viable recording loop; all other user stories layer on top.

**Suggested Sprint Order**:
1. Phase 1 + Phase 2 (scaffold + foundation) — ~1 day
2. Phase 3 (US1 MVP) — ~2 days
3. Phase 4 + 5 (US2 + US3, P1 completion) — ~2 days
4. Phase 8 (US6 quality/FPS, low-risk P2) in parallel with Phase 6 + 7 (US4 + US5 preview/save flow)
5. Phase 9 + 10 (US7 + US8, P3 polish)
6. Phase 11 (CI green, final regression)

**Format Validation**: All 77 tasks follow the required checklist format:
`- [ ] T### [P?] [US?] Description with file path`  
Setup/foundational tasks carry no `[US]` label by design. All user story tasks carry the matching `[US#]` label.
