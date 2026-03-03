# /speckit.specify Command Template

Purpose: Create or update a feature specification from user intent while keeping it testable and implementation-agnostic.

## Required Steps

1. Parse feature intent and extract user outcomes, constraints, and scope boundaries.
2. Generate/update `spec.md` using the canonical section order from `.specify/templates/spec-template.md`.
3. Ensure user stories are independently testable and prioritized (P1/P2/P3).
4. Ensure requirements include both functional and non-functional requirements.
5. Limit unresolved clarifications to critical items only and mark them explicitly.
6. Run a quality pass to remove implementation details and ambiguous language.

## Validation Rules

- Mandatory sections are present and ordered per template.
- Requirements are declarative and measurable.
- Success criteria are technology-agnostic and verifiable.
- No placeholder text remains unless explicitly marked for clarification.

## Scope and Constraints

- Focus on WHAT and WHY, not HOW.
- Do not include code, API signatures, framework decisions, or architecture internals.
- Keep wording concise, explicit, and reviewable.
