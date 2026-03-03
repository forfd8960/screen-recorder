# Command Templates Conventions

This directory contains reusable command templates for Speckit workflows.

## Naming

- Use lowercase file names.
- Use one command per file.
- Recommended format: `<command>.md` (example: `constitution.md`).

## Required Content

Each command template SHOULD include:

1. **Purpose**: what the command changes and why.
2. **Required Steps**: deterministic ordered steps.
3. **Validation Rules**: concrete checks that can be verified.
4. **Scope/Constraints**: what the command must not do.

## Quality Rules

- Keep instructions tool-agnostic where possible.
- Prefer declarative language (`MUST`, `MUST NOT`, `SHOULD` with rationale).
- Avoid vendor-specific assumptions unless strictly required.
- Keep edits minimal, explicit, and reviewable.

## Minimum Validation Checklist

- No unresolved placeholder tokens remain.
- Version/date fields use explicit rules where applicable.
- Cross-template sync requirements are listed when applicable.
- Output/report expectations are explicit and testable.

## Repository Alignment

Templates in this directory must align with:

- `.specify/memory/constitution.md`
- `.specify/templates/plan-template.md`
- `.specify/templates/spec-template.md`
- `.specify/templates/tasks-template.md`

Agent instructions should also stay aligned for the same command names:

- `.github/agents/speckit.constitution.agent.md`
- `.github/agents/speckit.specify.agent.md`
- `.github/agents/speckit.plan.agent.md`
- `.github/agents/speckit.tasks.agent.md`
- `.github/agents/speckit.implement.agent.md`
