# Instructions

## Product Requirements

### Product Overview

Build a macOS screen recorder application using Rust with a lightweight desktop UI built on egui. The product focuses on reliable screen + microphone recording, MP4 export, and a simple workflow for start/stop, preview, and save.

### Target Users

- Content creators who need quick desktop recording
- Developers and educators creating demos/tutorials
- General users who need lightweight screen capture with audio

### Goals

- Make recording simple and fast for first-time users
- Keep CPU/memory usage efficient during recording
- Provide a clear pre-save preview and output control

### In Scope

- Screen recording (full screen or selected area)
- Microphone audio recording
- MP4 export only
- Basic recording controls (start/stop)
- Quality and frame-rate options
- Destination folder selection
- Keyboard shortcuts for start/stop
- Background recording support
- Preview before final save
- Compatibility with latest macOS

### Out of Scope

- Non-MP4 export formats
- Cloud upload/sharing workflows
- Multi-track editing or timeline editing
- Advanced post-processing effects

## Functional Requirements

I want to build a screen recorder app for MacOS with Rust. which support following features:

- FR-001: The system must record the user’s screen and produce a video output.
- FR-002: The system must capture microphone audio and mux it with recorded video.
- FR-003: The UI must provide start and stop recording controls.
- FR-004: The user must be able to select a recording region (full screen or area).
- FR-005: The system must export recordings in MP4 format only.
- FR-006: The user must be able to set video quality and frame rate before recording.
- FR-007: The system must allow recording to continue while the app is not in focus.
- FR-008: The system must provide preview playback of the recorded output before final save.
- FR-009: The user must be able to choose the destination folder before saving.
- FR-010: The system must support keyboard shortcuts for starting and stopping recording.
- FR-011: The app must operate correctly on the latest macOS release.

## Non-Functional Requirements

- NFR-001 (Performance): During 1080p recording, the app should maintain stable capture without noticeable UI lag.
- NFR-002 (Resource Efficiency): The app should avoid excessive CPU/memory spikes during normal recording sessions.
- NFR-003 (Reliability): Recording start/stop must complete successfully without data loss in expected scenarios.
- NFR-004 (Usability): A first-time user should be able to start recording within 30 seconds.
- NFR-005 (Compatibility): Core recording workflow must be validated on the latest macOS version.

### UI Requirement

Use `egui` for the user interface, which is a simple and efficient GUI library for Rust.

## User Stories

### User Story 1 — Start/Stop Recording (Priority: P1)

As a user, I want to quickly start and stop recording from a simple UI so that I can capture content without setup friction.

Acceptance criteria:
- Given the app is open, when I click Start, then recording begins within a short and visible response window.
- Given recording is active, when I click Stop, then recording is finalized and available for preview.

### User Story 2 — Record Screen + Microphone (Priority: P1)

As a user, I want the app to capture both screen and microphone audio so that tutorials and demos include narration.

Acceptance criteria:
- Given microphone permission is granted, when recording starts, then the output contains synchronized video and microphone audio.
- Given microphone permission is denied, when recording starts, then the app informs me and continues with video-only if I choose.

### User Story 3 — Select Recording Area (Priority: P1)

As a user, I want to choose full screen or a custom area so that only relevant content is captured.

Acceptance criteria:
- Given recording setup is visible, when I choose full screen, then the full display is captured.
- Given recording setup is visible, when I draw/select an area, then only that region is captured.

### User Story 4 — Preview Before Save (Priority: P2)

As a user, I want to preview the captured video before saving so that I can confirm quality and correctness.

Acceptance criteria:
- Given a recording is completed, when preview opens, then I can play back the recorded content.
- Given preview is open, when I accept, then I can proceed to save.

### User Story 5 — Save to Chosen Folder as MP4 (Priority: P2)

As a user, I want to choose a destination folder and save only as MP4 so that output is predictable and easy to share.

Acceptance criteria:
- Given preview is accepted, when I choose a folder and save, then an MP4 file is created in that folder.
- Given save is successful, then the app shows file location and completion feedback.

### User Story 6 — Configure Quality and Frame Rate (Priority: P2)

As a user, I want to set quality and frame rate so that I can balance output quality and performance.

Acceptance criteria:
- Given pre-record settings are visible, when I change quality/frame rate, then selected settings are used for the next recording.

### User Story 7 — Keyboard Shortcuts (Priority: P3)

As a user, I want keyboard shortcuts for start/stop so that I can control recording quickly while presenting.

Acceptance criteria:
- Given the app is running, when I press the start shortcut, then recording starts.
- Given recording is active, when I press the stop shortcut, then recording stops and finalizes.

### User Story 8 — Background Recording Stability (Priority: P3)

As a user, I want recording to continue while I use other apps so that workflow is uninterrupted.

Acceptance criteria:
- Given recording is active, when I switch to another app, then recording continues until stopped.

## Success Metrics

- 90% of users can start first recording in under 30 seconds.
- 95% of completed recordings can be previewed and saved successfully.
- 95% of saves generate valid MP4 output in chosen folder.