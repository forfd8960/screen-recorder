# /speckit.implement Command Template

Purpose: Execute tasks in dependency order and deliver a verifiable implementation that satisfies constitution quality gates.

## Required Steps

1. Load implementation context from `tasks.md` and `plan.md`.
2. Validate prerequisites and checklist status before execution.
3. Execute tasks phase-by-phase, respecting dependency and parallel markers.
4. Enforce test-first ordering within each user story phase.
5. Mark completed tasks in `tasks.md` and report failures with next actions.
6. Run required verification suite before final handoff.

## Validation Rules

- All required tasks are complete or explicitly deferred with rationale.
- Required verification suite passes: `cargo fmt`, `cargo clippy --all-targets --all-features`, `cargo test`.
- Run `cargo audit` and `cargo deny` when dependency or security profile changed.
- Final implementation is traceable to user stories and acceptance criteria.

## Scope and Constraints

- Do not skip foundational phases or hidden dependencies.
- Avoid unrelated refactors while implementing planned scope.
- Keep progress updates concise, factual, and phase-based.
