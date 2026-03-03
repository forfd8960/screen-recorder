# /speckit.checklist Command Template

Purpose: Generate requirement-quality checklists as “unit tests for English” from current feature context.

## Required Steps

1. Load feature context from `spec.md` and available design artifacts.
2. Determine checklist focus and intended audience.
3. Generate checklist items that test requirement quality (not implementation behavior).
4. Organize items by quality dimensions (completeness, clarity, consistency, measurability, coverage).
5. Save checklist under feature `checklists/` with stable item IDs.

## Validation Rules

- Items evaluate requirement quality, not runtime behavior.
- Item IDs are sequential and globally unique per checklist.
- At least 80% of items include traceability markers.
- Output path and summary metadata are reported.

## Scope and Constraints

- Avoid QA execution test cases.
- Keep wording objective and auditable.
- Prevent duplicate low-signal items.
