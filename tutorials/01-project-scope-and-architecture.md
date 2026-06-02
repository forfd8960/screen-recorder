# Tutorial 01: Project Scope and Architecture

## Goal

Define what the app does, what "done" means, and how modules map to responsibilities before writing code.

## Topics

- Product scope:
  - Record screen to MP4 on macOS 12.3+
  - Optional microphone capture
  - Preview before save
  - Save/discard workflow
  - Keyboard shortcuts
- Core user flows:
  - Start -> Record -> Stop -> Preview -> Accept/Discard
- Module boundaries:
  - `src/app.rs`: orchestration and command loop
  - `src/capture/*`: permission + stream + region filter
  - `src/encode/*`: writer pipeline + PTS + temp file
  - `src/output/*`: finalize, naming, reveal in Finder
  - `src/config/*`: typed settings + persistence
  - `src/ui/*`: egui panels only dispatch commands
- State machine design:
  - `Idle`, `Recording`, `Previewing`, `Saving`
- Non-goals and trade-offs:
  - Why Area mode is feature-gated (`macos_14_2`)

## Deliverable

- A one-page architecture doc and sequence diagram of runtime flow.
