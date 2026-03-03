# /speckit.tasks Command Template

Purpose: Generate an executable, dependency-aware task list mapped to user stories and quality gates.

## Required Steps

1. Load `plan.md` and `spec.md` as required inputs.
2. Map each user story to an independently deliverable task phase.
3. Create setup and foundational phases before story implementation phases.
4. Include required tests and verification tasks per constitution and plan gates.
5. Mark parallelizable tasks with `[P]` and preserve dependency order.
6. Ensure every task includes an exact path and an actionable verb.

## Validation Rules

- Every task follows checklist format: `- [ ] T### [P?] [US?] Description with file path`.
- Each user story has implementation tasks and verification coverage.
- Cross-cutting quality tasks exist (lint/test/security checks where applicable).
- No placeholder sample tasks remain in final generated output.

## Scope and Constraints

- Organize by user value delivery, not by technical layer alone.
- Keep tasks granular enough for direct execution by an LLM or developer.
- Avoid hidden dependencies and same-file parallel conflicts.
