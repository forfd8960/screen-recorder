# /speckit.analyze Command Template

Purpose: Perform a read-only consistency analysis across `spec.md`, `plan.md`, and `tasks.md` against constitution rules.

## Required Steps

1. Resolve active feature paths and load required artifacts.
2. Build requirement/task coverage mapping and identify gaps.
3. Detect ambiguity, duplication, inconsistency, and underspecification.
4. Validate constitution alignment and classify violations by severity.
5. Output a compact analysis report with actionable next steps.

## Validation Rules

- Analysis is strictly read-only (no file modifications).
- Report includes severity, locations, and concrete recommendations.
- Coverage summary includes requirement-to-task mapping.
- Constitution MUST-level conflicts are flagged as CRITICAL.

## Scope and Constraints

- Do not propose silent constitution bypasses.
- Keep findings high-signal and bounded.
- Prefer deterministic, repeatable outputs.
