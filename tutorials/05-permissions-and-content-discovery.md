# Tutorial 05: Permissions and Content Discovery

## Goal

Handle macOS permission onboarding and enumerate displays/windows for capture choices.

## Topics

- TCC basics on macOS:
  - screen recording permission behavior
  - microphone permission fallback strategy
- Implement `check_screen_permission` with `SCShareableContent::get()`
- Error mapping for permission-denied UX
- Discovering capture targets:
  - `list_displays()` and `list_windows()`
  - lightweight DTOs (`DisplayInfo`, `WindowInfo`) for UI
- Building content filters:
  - Full screen, window, area
  - self-exclusion so recorder window is not captured

## Deliverable

- App can detect permission state and show available capture targets.
