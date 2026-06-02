# Tutorial 10: Error Handling and Observability

## Goal

Introduce a robust error model and logs that make failures understandable in development and support.

## Topics

- Designing `AppError`:
  - permission, stream, encoding, region, I/O variants
  - user-facing vs diagnostic messages
- Error flow through layers:
  - capture/encode/output return `Result<T, AppError>`
  - command loop updates `last_error`
- UI error presentation patterns:
  - blocking onboarding vs non-fatal banners
  - retry and dismissal commands
- Structured logs with `tracing`:
  - startup logs
  - command-level logs
  - writer/capture diagnostics
- Practical debugging checklist:
  - missing permissions
  - mic unavailable fallback
  - save path and I/O failures

## Deliverable

- Failures are surfaced cleanly to both users and developers.
