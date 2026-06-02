# Tutorial 04: Settings Model and Persistence

## Goal

Create strongly typed recording settings and persist them across launches.

## Topics

- Domain types with `serde`:
  - `Resolution`, `VideoQuality`, `CaptureRegion`, `Rect`
  - tagged enums for stable JSON schema
- `RecordingSettings` as single source of truth
- Defaults and validation:
  - frame rates (24/30/60)
  - quality-to-bitrate mapping
- Settings file path strategy:
  - `~/Library/Application Support/screen-recorder/settings.json`
- Persistence helpers:
  - `load_settings`, `save_settings`, read/write helpers
- Backward-compatible evolution tips for tutorial readers

## Deliverable

- Settings survive app restart and deserialize safely.
