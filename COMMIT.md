# COMMIT
Live commit workflow contract for `rgx`.

## Why this file exists
- Define an exact, repeatable commit workflow for AI and human contributors.
- Make commit behavior deterministic across session interruptions and AI handoffs.
- Prevent drift in staging, commit-message handling, and post-commit hygiene.

## When to run the commit workflow
- Run after each completed task or activity.
- Run after task-related docs/test updates are done.
- Prefer one focused commit per completed task (avoid mixing unrelated work).

## Files involved and what each one means
- `COMMIT.md`
  - This file.
  - Authoritative definition of commit workflow steps and invariants.
- `git_message_brief.txt`
  - Ephemeral commit-message buffer for `git commit -F git_message_brief.txt`.
  - Must be cleared after each commit.
  - Must remain untracked.
- `CHANGES.md`
  - Living historical change ledger (what changed, why, validation, impact).
  - Update before commit when a task changes behavior/tests/docs.
- `MEMORY.md`
  - Live continuity memory for cross-session resume/handoff.
  - Update before commit for any completed task.
- `DEVELOPMENT_NOTES.md`
  - Durable technical knowledge base and reliability snapshot.
  - Update when durable engineering understanding changes.
- Task-specific files
  - Code/tests/docs changed by the completed task (stage exactly these files from status output).

## Exact commit workflow (ordered)
1. Finish the task implementation and validation.
2. Update live docs as needed (`CHANGES.md`, `MEMORY.md`, optionally `DEVELOPMENT_NOTES.md` and relevant docs).
3. Run pre-commit status:
   - `git --no-pager status --short`
4. Stage exactly the files shown in that status output (no hidden extras).
5. Prepare `git_message_brief.txt` with:
   - concise title
   - brief bullet summary
   - required `Co-Authored-By` trailer(s)
6. Commit:
   - `git commit -F git_message_brief.txt`
7. Post-commit cleanup:
   - clear brief file: `: > git_message_brief.txt`
8. Post-commit verification:
   - `git --no-pager status --short git_message_brief.txt`
   - `git ls-files --error-unmatch git_message_brief.txt >/dev/null 2>&1; echo TRACKED:$?`
   - expected: `TRACKED:1` (untracked)
9. Final repository check:
   - `git --no-pager status --short`
   - expected clean working tree.

## Non-negotiable invariants
- Never commit without a fresh pre-commit `git status`.
- Never stage files that were not in the captured pre-commit status set.
- Never leave `git_message_brief.txt` populated after commit.
- Never allow `git_message_brief.txt` to become tracked.
- Keep commits task-scoped and validation-backed.

## Handoff usage
- New AI should read `MEMORY.md` first, then `COMMIT.md`.
- `MEMORY.md` explains *what* is happening now.
- `COMMIT.md` explains *how* to finalize work safely and consistently.
