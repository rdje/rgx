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
2. Run mandatory quality gate commands:
   - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --all`
   - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
   - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
   - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
   - policy: clippy warnings are currently tolerated; clippy errors are not allowed.
3. Update live docs as needed (`CHANGES.md`, `MEMORY.md`, optionally `DEVELOPMENT_NOTES.md`, `README.md`, and relevant docs).
   - `README.md` should be updated when project objective, onboarding links, or key path maps change.
   - `README.md` does not need updates on every commit.
4. Run pre-commit status:
   - `git --no-pager status --short`
5. Stage exactly the files shown in that status output (no hidden extras).
6. Prepare `git_message_brief.txt` with:
   - concise title
   - brief bullet summary
   - required `Co-Authored-By` trailer(s)
7. Commit:
   - `git commit -F git_message_brief.txt`
8. Post-commit cleanup:
   - clear brief file: `: > git_message_brief.txt`
9. Post-commit verification:
   - `git --no-pager status --short git_message_brief.txt`
   - `git ls-files --error-unmatch git_message_brief.txt >/dev/null 2>&1; echo TRACKED:$?`
   - expected: `TRACKED:1` (untracked)
10. Final repository check:
   - `git --no-pager status --short`
   - expected clean working tree.

## Non-negotiable invariants
- Never commit without a fresh pre-commit `git status`.
- Never stage files that were not in the captured pre-commit status set.
- Never leave `git_message_brief.txt` populated after commit.
- Never allow `git_message_brief.txt` to become tracked.
- Never proceed to commit with unresolved clippy errors.
- Clippy warnings are tolerated for now unless policy changes.
- Keep commits task-scoped and validation-backed.

## Handoff usage
- New AI should read `MEMORY.md` first, then `COMMIT.md`.
- `MEMORY.md` explains *what* is happening now.
- `COMMIT.md` explains *how* to finalize work safely and consistently.
