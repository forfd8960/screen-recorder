# /speckit.constitution Command Template

Purpose: Update `.specify/memory/constitution.md` and keep dependent templates synchronized.

## Required Steps

1. Load current constitution and detect unresolved placeholders.
2. Derive concrete values from repository context and user input.
3. Apply semantic versioning bump rules and set amendment date.
4. Propagate governance changes into related templates:
   - `.specify/templates/plan-template.md`
   - `.specify/templates/spec-template.md`
   - `.specify/templates/tasks-template.md`
5. Prepend an HTML `Sync Impact Report` to constitution.
6. Validate:
   - No unexplained bracket placeholders remain.
   - Dates use `YYYY-MM-DD`.
   - Principles are declarative and testable.

## Notes

- Use generic agent wording; avoid tool- or vendor-specific identity assumptions.
- Keep constitution heading hierarchy unchanged.
- Prefer minimal, explicit, and reviewable edits.
