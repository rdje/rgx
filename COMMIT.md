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
- For task-tree-managed work: run after **each completed leaf**, before
  selecting another leaf.

## Task-Tree Workflow Rule (binding)
RGX tracks work with the repo-local task-tree workflow
(`docs/TASK_TREE.md`). The **Code-Change Doctrine** is non-negotiable:

- **No code change may be committed unless a task-tree leaf owns it.** "Code
  change" = any edit to Rust sources, `Cargo.toml`/`Cargo.lock`, build scripts,
  generated artifacts, `.github/workflows/*`, `scripts/*`, or a `subs/pgen`
  pin bump. Before touching code, create/extend a tree so a leaf owns the
  change, implement only that leaf, then run this workflow.
- **The leaf ID goes in the commit subject or first body line** (e.g.
  `... (leaf TASKTREE-ADOPT.2)`). The leaf ID is RGX's commit-traceability key.
- **Update the owning `docs/tasks/<TREE>.md` file** when node status, frontier,
  blockers, decisions, validation, or completion evidence changes (this is part
  of the step-3 documentation-sync gate below).
- Pure non-code documentation slices (live docs, the Book, `pgen-issues/`,
  workflow docs) may be committed without a task-tree leaf, but still follow
  the rest of this workflow. When a change touches both, treat it as a code
  change and require a leaf.

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
  - The bounded **layer-A resume pointer** (`MEMORY_ARCHITECTURE.md`): current state / active frontier / next action only. **Overwrite-only, kept ≤ cap.**
  - Update (overwrite the current-state block) before commit for any completed task. Do NOT append — durable history is the task-trees + `docs/decisions/` + git.
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
   - [ ] `docs/tasks/<TREE>.md` — update the owning task tree (node status,
         frontier, verification log, commit log) for task-tree-managed work
   - [ ] `docs/TASK_TREE.md` — update the Active/Proposed/Completed tables when
         a tree's status or frontier changed
   - [ ] `LIVE_ACHIEVEMENT_STATUS.md` — update the lane→tree board / latest
         completed slice when project state changed
   - [ ] `CHANGES.md` — new entry for every shipped feature/fix
   - [ ] `docs/BACKLOG.md` — mark completed items
   - [ ] `MEMORY.md` — **overwrite** the layer-A resume pointer (current state / frontier / next action; ≤ cap). Do NOT append (history is git + task-trees).
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
   - **The leaf ID** in the subject or first body line for task-tree-managed
     work (e.g. `(leaf <TREE>.<path>)`).
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
- **Never commit a code change that is not owned by a task-tree leaf** (Code-Change Doctrine, `docs/TASK_TREE.md`).
- **Never finalize a task-tree leaf without updating its `docs/tasks/<TREE>.md` file** and naming the leaf ID in the commit subject or first body line.
- Commit one completed leaf at a time; do not select another leaf before the prior leaf's commit workflow has completed.
- Never finalize a Rust-focused commit without deciding whether `RUST_CODEBASE_ANALYSIS.md` changed.
- Keep the formatting gate scoped to RGX workspace packages so local external dependencies (for example the sibling `pgen` checkout) do not leak into RGX commit validation.
- The gate is "green" only when `./scripts/run-local-ci.sh` exited 0 for the *exact tree being committed* — proven by a fresh matching receipt, not by eyeballing filtered output. A red gate must never be self-reported green; if the gate fails, the commit does not happen.
- **Do not edit gate-affecting files while the gate is running.** The receipt is computed at the END of the run, so a mid-run edit is stamped as certified even though the test steps never saw it — and it produces *no* staleness signal (the receipt matches the tree, so the check passes). If you edit mid-run, discard that run and start the gate again. Hardening this into a mechanical refusal is tracked as `DOCTRINE-ADOPT.3`.
- A tracked `pre-commit` hook (`scripts/git-hooks/pre-commit`, activated once via `./scripts/setup-hooks.sh`) blocks committing a worktree whose content has no fresh green receipt. Bypassing it (`git commit --no-verify`) is an explicit, loud, last-resort act that must be called out in the commit body and justified — never a silent default.
- **This workflow is machine-enforced.** The hook derives the Knowledge Map, then runs `scripts/check_doctrines.sh --scope hook` — the doctrine registry + driver (`DOCTRINE_ENFORCEMENT.md`, ADR 0006). It blocks a commit that: stages code without an updated `docs/tasks/<TREE>.md` (`CODE-CHANGE-LEAF`, step 3 track B), stages code without a `CHANGES.md` entry (`TWO-TRACK-DOCS`), lacks a fresh matching gate receipt (`GATE-RECEIPT`, step 2), carries modified tracked content in `subs/pgen` (`PGEN-READONLY`), or breaches the memory-architecture / Knowledge-Map / registry-sync invariants. `scripts/run-local-ci.sh` runs the same driver with `--scope ci`, so CI enforces it server-side. Run `./scripts/check_doctrines.sh` yourself before committing to see the report early.

## Handoff usage
- New AI should read `MEMORY.md` (the resume pointer) first, then `COMMIT.md`.
- `MEMORY.md` explains *what* is happening now (current state + active frontier); deeper history is in the task-trees + git.
- `COMMIT.md` explains *how* to finalize work safely and consistently.
- `RUST_CODEBASE_ANALYSIS.md` explains how the current Rust workspace lines up with `ROADMAP.md`.
