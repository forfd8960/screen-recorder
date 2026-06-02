# Tutorial 06: Capture Engine and Streaming

## Goal

Implement `CaptureEngine` to start/stop ScreenCaptureKit streams and deliver frames to encoder channels.

## Topics

- `SCStreamConfiguration` from settings:
  - output dimensions and frame rate normalization
  - audio capture flags and format parameters
- Stream lifecycle:
  - create stream
  - register output handlers
  - `start_capture` and `stop_capture` via `spawn_blocking`
- Frame transport design:
  - bounded `tokio::mpsc` channels for video/audio
  - non-blocking `try_send` from callback thread
  - dropped-frame accounting with atomics
- Global shortcuts integration:
  - `Command+Shift+R` and `Command+Shift+S`

## Deliverable

- Frames are captured and pushed through channels with stable lifecycle control.
