# /speckit.plan Command Template

Purpose: Produce an implementation plan that is constitution-compliant and ready for task decomposition.

## Required Steps

1. Load current `spec.md` and `.specify/memory/constitution.md`.
2. Fill `plan.md` from `.specify/templates/plan-template.md` with concrete technical context.
3. Resolve unknowns through explicit research outputs when needed.
4. Complete Constitution Check with pass/fail evidence for each gate.
5. Define structure decisions and phase outputs (`research.md`, `data-model.md`, `contracts/`, `quickstart.md`) where applicable.
6. Re-check constitution alignment after design decisions are documented.

## Validation Rules

- Constitution gates are explicitly evaluated and justified.
- Performance, reliability, security, and observability concerns are represented.
- Plan includes clear phase outputs and dependency order.
- Unknowns are either resolved or marked with bounded follow-up actions.

## Scope and Constraints

- Do not skip constitution checks.
- Avoid speculative architecture not required by current scope.
- Keep decisions auditable with short rationale statements.
