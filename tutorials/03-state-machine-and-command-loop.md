# Tutorial 03: State Machine and Command Loop

## Goal

Build the app core that separates UI events from business logic using a command channel.

## Topics

- Designing `RecorderCommand`:
  - `Start`, `Stop`, `Accept`, `Discard`, settings updates
- Building `AppState`:
  - shared state with `Arc<Mutex<_>>`
  - why orchestrator is not kept in shared state
- Recording lifecycle model:
  - `RecordingStatus` transitions and guards
- Async `command_loop`:
  - receive commands
  - validate state before executing
  - update state and errors safely
- UI integration pattern:
  - panels dispatch commands only
  - command loop owns side effects

## Deliverable

- Start/Stop commands and state transitions work in a no-op pipeline.
