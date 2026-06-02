# Tutorial 12: Production Hardening and Release Checklist

## Goal

Prepare the app for reliable real-world use, distribution, and maintenance.

## Topics

- Stability hardening:
  - graceful handling of empty frames and writer failures
  - channel sizing and backpressure trade-offs
  - retries and fallback paths
- macOS operational concerns:
  - permission reset/testing workflow
  - app relaunch expectations after TCC changes
- Build and verification:
  - dev vs release profile differences
  - lint/test/audit/deny commands
- Security and privacy:
  - do not log sensitive paths or identifiers unnecessarily
  - principle of least privilege for capture features
- Documentation and support:
  - troubleshooting section design
  - known limitations and roadmap communication

## Deliverable

- A release-ready checklist and maintenance playbook for future iterations.
