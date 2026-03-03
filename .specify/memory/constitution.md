<!--
Sync Impact Report
- Version change: 0.0.0 -> 1.0.0
- Modified principles:
	- Template Principle 1 -> I. Rust-First Safety and Reliability
	- Template Principle 2 -> II. Performance-Driven Capture Pipeline
	- Template Principle 3 -> III. Test and Quality Gates (NON-NEGOTIABLE)
	- Template Principle 4 -> IV. Observability and Operability by Default
	- Template Principle 5 -> V. Minimal, Typed, Layered Architecture
- Added sections:
	- Platform and Security Constraints
	- Delivery Workflow and Review Gates
- Removed sections:
	- None
- Templates requiring updates:
	- ✅ updated: .specify/templates/plan-template.md
	- ✅ updated: .specify/templates/spec-template.md
	- ✅ updated: .specify/templates/tasks-template.md
	- ✅ updated: .specify/templates/commands/README.md
	- ✅ updated: .specify/templates/commands/constitution.md
	- ✅ updated: .specify/templates/commands/specify.md
	- ✅ updated: .specify/templates/commands/plan.md
	- ✅ updated: .specify/templates/commands/tasks.md
	- ✅ updated: .specify/templates/commands/implement.md
	- ✅ updated: .specify/templates/commands/analyze.md
	- ✅ updated: .specify/templates/commands/clarify.md
	- ✅ updated: .specify/templates/commands/checklist.md
	- ✅ updated: .specify/templates/commands/taskstoissues.md
	- ✅ updated: .github/agents/speckit.constitution.agent.md
	- ✅ updated: .github/agents/speckit.specify.agent.md
	- ✅ updated: .github/agents/speckit.plan.agent.md
	- ✅ updated: .github/agents/speckit.tasks.agent.md
	- ✅ updated: .github/agents/speckit.implement.agent.md
- Runtime guidance references:
	- ✅ verified aligned: AGENTS.md
	- ✅ verified aligned: docs/instructions.md
	- ✅ verified aligned: docs/Building macOS Screen Recorder with Rust.md
	- ✅ updated: docs/speckit-alignment-audit.md
- Deferred TODOs:
	- None
-->

# Screen Recorder Constitution

## Core Principles

### I. Rust-First Safety and Reliability
All production code MUST use stable Rust with edition 2021 or newer. `unsafe` code is
prohibited unless no safe alternative exists, and any exception MUST be isolated,
documented, and code-reviewed with explicit justification. Recoverable failures MUST use
`Result<T, E>` with domain-specific errors; `panic!`, `unwrap()`, and `expect()` are
forbidden in production paths.

Rationale: screen and audio capture is long-running and user-facing; predictable failure
behavior is mandatory for trust and stability.

### II. Performance-Driven Capture Pipeline
Capture and encoding paths MUST prefer macOS-native, hardware-accelerated APIs
(ScreenCaptureKit and related Apple media frameworks) and avoid unnecessary CPU-bound
buffer conversion. Independent async operations MUST not block each other, and external
or long-running calls MUST have timeouts/cancellation paths.

Rationale: recording quality and thermal behavior directly depend on low-overhead,
zero-copy oriented pipelines.

### III. Test and Quality Gates (NON-NEGOTIABLE)
Every change MUST pass `cargo fmt`, `cargo clippy --all-targets --all-features`, and
`cargo test` before merge. CI MUST enforce release buildability and dependency/security
checks (`cargo build --release`, `cargo audit`, and `cargo deny`). Work that fails these
gates cannot be merged.

Rationale: strict, automated quality gates prevent regressions in a concurrency- and
media-heavy codebase.

### IV. Observability and Operability by Default
Runtime behavior MUST be diagnosable through structured logging (`tracing`), clear error
context, and safe user-visible error messages. Sensitive values (tokens, secrets,
credentials, raw private data) MUST never be logged. Performance-sensitive paths MUST
emit actionable diagnostics needed to investigate dropped frames, sync drift, or audio
capture failures.

Rationale: capture failures are hard to reproduce; operability determines support cost
and recovery time.

### V. Minimal, Typed, Layered Architecture
New functionality MUST keep handlers/UI thin and move business logic into explicit
service/domain layers. API/data-transfer models MUST be separated from domain models, and
strong types MUST be preferred over loosely typed maps for core flows. Designs MUST
choose the simplest solution that satisfies current requirements and avoid speculative
abstractions.

Rationale: clear boundaries improve testability, reduce coupling, and keep iteration
speed high as the product scope grows.

## Platform and Security Constraints

- The product target is the latest macOS and MUST respect Apple privacy/permission flows
	for screen and microphone capture.
- Required permission descriptions and entitlements MUST be declared correctly before
	release packaging.
- Input and configuration data MUST be validated at boundaries, with explicit size/format
	limits for external or user-provided values.
- Secrets MUST come from environment or secure stores and MUST never be committed.

## Delivery Workflow and Review Gates

- Plans (`/speckit.plan`) MUST pass Constitution Check before implementation.
- Specs (`/speckit.specify`) MUST include mandatory non-functional requirements:
	safety/reliability, performance, observability, security/privacy, and verification.
- Tasks (`/speckit.tasks`) MUST include required test and verification tasks, not optional
	placeholders.
- Pull requests MUST include evidence of local/CI quality gates and note any approved
	constitution exceptions.

## Governance

This constitution is the highest-priority engineering policy for this repository.
Conflicts with lower-level guidance are resolved in favor of this document.

Amendment process:
1. Propose change with rationale and impacted principles/sections.
2. Update dependent templates and workflow guidance in the same change.
3. Record version bump rationale using semantic versioning.

Versioning policy:
- MAJOR: incompatible governance change, principle removal, or principle redefinition.
- MINOR: new principle/section or materially expanded mandatory guidance.
- PATCH: wording clarifications, typo fixes, and non-semantic refinements.

Compliance review expectations:
- Every feature plan and PR review MUST include a constitution compliance check.
- Exceptions MUST be time-bounded, documented, and approved during review.

**Version**: 1.0.0 | **Ratified**: 2026-03-03 | **Last Amended**: 2026-03-03
