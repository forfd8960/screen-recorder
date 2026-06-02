# Tutorial 08: Preview, Accept/Discard, and Save Flow

## Goal

Implement post-recording UX where users preview the temp file, then accept/save or discard/delete.

## Topics

- Stop behavior in command loop:
  - finalize pipeline
  - transition to `Previewing`
- Preview panel design:
  - open in QuickTime
  - keyboard shortcuts (`Cmd+Enter`, `Cmd+Backspace`)
- Accept behavior:
  - transition `Previewing -> Saving`
  - finalize temp file into output directory
  - reveal in Finder
  - success toast payload
- Discard behavior:
  - best-effort temp file deletion
  - transition back to `Idle`
- Save failures and retry paths

## Deliverable

- Full stop -> preview -> save/discard flow works reliably.
