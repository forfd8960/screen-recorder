# /speckit.taskstoissues Command Template

Purpose: Convert generated tasks into GitHub issues with repository-safety checks.

## Required Steps

1. Resolve active feature paths and load `tasks.md`.
2. Read git remote and verify it is a GitHub repository URL.
3. Parse tasks into issue-ready units with clear titles/descriptions.
4. Create issues only in the repository matching the current remote.
5. Report created issue links and any skipped tasks.

## Validation Rules

- No issue creation if remote is non-GitHub.
- Repository owner/name must match remote URL exactly.
- Each created issue maps to at least one concrete task.
- Fail-safe behavior on parsing or permission errors.

## Scope and Constraints

- Never create issues in unrelated repositories.
- Do not mutate `tasks.md` during issue creation.
- Keep issue text concise and actionable.
