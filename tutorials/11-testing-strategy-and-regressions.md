# Tutorial 11: Testing Strategy and Regressions

## Goal

Teach readers how to verify each layer and prevent regressions in recorder behavior.

## Topics

- Unit test boundaries:
  - settings serialization and defaults
  - region validation and mapping helpers
  - command guard predicates
  - output filename and finalize behavior
- UI-adjacent helper tests:
  - preview accept/discard command dispatch
  - file deletion side effects
- Integration tests in `tests/`:
  - settings roundtrip
  - save roundtrip
  - preview flow
  - audio pipeline and PTS normalizer
- Feature-gated integration workflow (`--features integration`)
- CI quality gates:
  - `fmt`, `clippy`, `test`, dependency checks

## Deliverable

- A repeatable test matrix that catches behavioral regressions early.
