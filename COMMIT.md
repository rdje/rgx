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
- `RUST_CODEBASE_ANALYSIS.md`
  - Live roadmap-grounded analysis of the Rust workspace.
  - Update before Rust-focused commits when code changes alter architecture, feature readiness, validation results, or roadmap alignment.
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
2. Run the mandatory quality gate. **Run it through `./scripts/run-local-ci.sh`** — that is THE gate runner. It executes the steps below with `set -euo pipefail` and no exit-masking, and on success writes a tree-stamped green receipt that the pre-commit hook verifies (step 7). Do NOT hand-run a filtered subset and call the gate green.
   - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm`
   - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core` — the **FULL** suite: lib **+ integration tests (incl. `tests/stress_tests.rs`) + doc tests**, on the **default thread stack**. A filtered run (`-p rgx-core <name>` / `--lib` / `--test X`) is NOT the gate and must never be reported as "`cargo test -p rgx-core` => pass".
   - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
   - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-capi`
   - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
   - policy: clippy warnings are currently tolerated; clippy errors are not allowed.
   - **Exit-code integrity (non-negotiable).** A pipeline's exit status is its LAST command's. `cargo test … 2>&1 | tail`/`| grep`/`| head` returns the filter's exit (0), **masking a cargo failure or a SIGABRT**. This is exactly how the 2026-04-07 → 2026-05-18 deep-nesting gate failure was reported "green" for six weeks. When you must filter output, assert the real status: check `${PIPESTATUS[0]}`, use `set -o pipefail`, run unpiped, or — preferred — let `run-local-ci.sh` run it (it does this correctly). Never conclude "pass" from filtered output alone.
   - **Accuracy gate.** For any change touching parsing, the PGEN adapter, the VM/compiler, or the conformance harness, also run the PCRE2 conformance ratchet (`cargo test -p rgx-core --test pcre2_conformance -- --ignored`) and confirm `RATCHET OK`. `RGX_RUN_CONFORMANCE=1 ./scripts/run-local-ci.sh` folds this in.
3. **MANDATORY documentation sync — both tracks**. Check each and update if stale:

   **Track A: The RGX Book (user-facing, open to the world)**
   - [ ] `book/src/**` — new chapter or section for any user-visible change. The book must cover every aspect of RGX: features, architecture, rationale, design decisions, performance, sandboxing model. The book is what the world sees.

   **Track B: Live continuity docs (session-internal)**
   - [ ] `CHANGES.md` — new entry for every shipped feature/fix
   - [ ] `docs/BACKLOG.md` — mark completed items
   - [ ] `MEMORY.md` — append dated session notes (never delete old entries)
   - [ ] `README.md` — PGEN version pins, submodule references, doc index (when changed)
   - [ ] `RUST_CODEBASE_ANALYSIS.md` — when architecture/roadmap alignment changed
   - [ ] `DEVELOPMENT_NOTES.md` — when durable engineering understanding changed

   **The two tracks serve different audiences and are NOT interchangeable. Both must be updated. This step is a hard gate. Do not proceed to step 4 without completing it.**
4. Run pre-commit status:
   - `git --no-pager status --short`
5. Stage exactly the files shown in that status output (no hidden extras).
6. Prepare `git_message_brief.txt` with:
   - concise title (≤70 characters, active voice)
   - 2–5 line body explaining the *why* at a high level (the diff shows the *what*)
   - **No `Co-Authored-By` trailers.** Per user preference, RGX commit messages do not carry agent co-authorship trailers.
   - **Keep it brief.** The gory details belong in `CHANGES.md`; engineering rationale belongs in `DEVELOPMENT_NOTES.md`. The commit message is the headline, not the full ledger entry.
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
- Never finalize a Rust-focused commit without deciding whether `RUST_CODEBASE_ANALYSIS.md` changed.
- Keep the formatting gate scoped to RGX workspace packages so local external dependencies (for example the sibling `pgen` checkout) do not leak into RGX commit validation.
- The gate is "green" only when `./scripts/run-local-ci.sh` exited 0 for the *exact tree being committed* — proven by a fresh matching receipt, not by eyeballing filtered output. A red gate must never be self-reported green; if the gate fails, the commit does not happen.
- A tracked `pre-commit` hook (`scripts/git-hooks/pre-commit`, activated once via `./scripts/setup-hooks.sh`) blocks committing a worktree whose content has no fresh green receipt. Bypassing it (`git commit --no-verify`) is an explicit, loud, last-resort act that must be called out in the commit body and justified — never a silent default.

## Handoff usage
- New AI should read `MEMORY.md` first, then `COMMIT.md`.
- `MEMORY.md` explains *what* is happening now.
- `COMMIT.md` explains *how* to finalize work safely and consistently.
- `RUST_CODEBASE_ANALYSIS.md` explains how the current Rust workspace lines up with `ROADMAP.md`.
