# Tutorial 09: UI Panels and Shortcuts

## Goal

Assemble the full egui UI as thin command-dispatching panels.

## Topics

- Panel composition in `App::update`:
  - main panel vs preview panel vs save panel
  - always-visible bottom settings panel
- Main window:
  - recording status badge and elapsed timer
  - Start/Stop buttons with state-aware enabling
- Settings panel:
  - resolution, frame rate, quality controls
  - region picker with display/window lists
  - refresh content command
- Preview and save panels:
  - accept/discard controls
  - folder picker and retry save
  - completion toast
- Shortcut strategy:
  - global shortcuts in capture layer
  - app-local shortcuts in egui input handling

## Deliverable

- Complete UI works as a pure command frontend with minimal logic leakage.
