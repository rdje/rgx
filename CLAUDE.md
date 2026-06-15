# CLAUDE.md — Project rules for AI assistants

This file is loaded automatically at the start of every Claude Code session.
These rules are non-negotiable.

## Task-tree tracking (binding — Code-Change Doctrine)

RGX tracks all work with the repo-local task-tree workflow. The operating spec
is `docs/TASK_TREE.md`; the portable guide is `docs/TASK_TREE_README.md`; each
top-level task owns one file under `docs/tasks/`.

- **No code change may happen unless it is first owned by a task-tree leaf.**
  "Code change" = any edit to Rust sources, `Cargo.toml`/`Cargo.lock`, build
  scripts, generated artifacts, `.github/workflows/*`, `scripts/*`, or a
  `subs/pgen` pin bump. Before touching code, create or extend a tree so a leaf
  owns the change; implement only that leaf; then run the `COMMIT.md` workflow.
- **The leaf ID is the commit-traceability key** — put it in the commit subject
  or first body line (e.g. `(leaf TASKTREE-ADOPT.2)`). RGX does not use a
  separate slice-ID scheme.
- **Implement one leaf at a time.** Never implement a container node. If a leaf
  is too big, split it into child leaves first.
- **PNT** ("pick the next task") = select the first eligible leaf from the
  active tree's current frontier (`docs/TASK_TREE.md`) and roll.
- Keep node IDs stable forever; never renumber closed nodes.
- Pure non-code documentation slices (live docs, the Book, `pgen-issues/`,
  workflow docs) may be committed without a leaf — the doctrine governs **code**
  changes. When in doubt, require a leaf.
- The roadmap, the codebase, and the mdBook must stay locked together — no
  drift. The task tree is the execution ledger that keeps them aligned.

## Two separate documentation tracks

These serve different audiences and are NOT interchangeable. Both must be updated.

### The RGX Book (`book/`) — for the world

The Book is the public face of RGX. Open, transparent, comprehensive. **Every aspect of RGX must land here:**
- Every public feature (API + CLI)
- Internal architecture (engine, parser, VM, compiler pipeline)
- Design rationale (why these choices, what tradeoffs)
- Performance characteristics
- Safety/sandboxing model
- PGEN integration story
- Anything users, evaluators, or contributors might want to know

When shipping any feature or making a design decision: **does the book need a new section or chapter?** If yes, write it.

### Live continuity docs — for future sessions

`MEMORY.md`, `COMMIT.md`, `CHANGES.md`, `docs/BACKLOG.md`, `RUST_CODEBASE_ANALYSIS.md`. These exist to survive session loss and AI handoffs. They are not user-facing.

**Updating live docs does NOT satisfy the book requirement. Both tracks must be updated.**

## Commit workflow

Follow `COMMIT.md` exactly. The critical step most likely to be skipped:

### Step 3: Documentation sync (HARD GATE — both tracks)

**The Book (user-facing):**
1. `book/src/**` — new chapter or section for any user-visible change

**Live continuity docs (internal):**
2. `CHANGES.md` — new entry for every shipped feature/fix
3. `docs/BACKLOG.md` — mark completed items
4. `MEMORY.md` — append dated session notes (NEVER delete old entries)
5. `README.md` — PGEN version pins, submodule refs, doc index (when changed)
6. `RUST_CODEBASE_ANALYSIS.md` — when architecture/roadmap changed
7. `DEVELOPMENT_NOTES.md` — when durable engineering understanding changed

**Do not run `git commit` until both tracks are checked.**

## Git discipline

- **Never `git push` unless the user explicitly asks.** Commit freely; push on command only.
- Never amend published commits without asking.
- Never force-push.

## API design

- **Fluency is paramount.** The user-facing API must feel like driving a car, not assembling an engine.
- Zero ceremony for the 80% use case. One line should be enough.
- Prefer zero-argument flag setters (`.case_insensitive()` not `.case_insensitive(true)`).
- Every new public type must support idiomatic Rust: `Index`, `Iterator`, `Display` where appropriate.

## PGEN integration

- PGEN is the sole parser. No builtin parser fallback.
- File bug reports to `pgen-issues/` with repro artifacts per `PGEN_PARSER_ISSUE_REPORTING_PROTOCOL.md`.
- Verify PGEN parses correctly before assuming it's a PGEN bug — check with `parseability_probe` first.

## Session bootstrap

- Read `SESSION_BOOTSTRAP.md` → `README.md` → `MEMORY.md` → `COMMIT.md` in that order.
- `MEMORY.md` has dated session entries. Read the latest one for current state.
- Read `LIVE_ACHIEVEMENT_STATUS.md` (status board) → `docs/TASK_TREE.md` (workflow + active-tree index) → the active task files it names under `docs/tasks/`.
- `docs/BACKLOG.md` has the task inventory with completion status.

## Testing

- Run `cargo fmt`, `cargo test -p rgx-core`, `cargo test -p rgx-cli`, `cargo clippy --workspace --all-targets` before committing.
- Zero clippy errors allowed. Warnings tolerated.
- Timing-sensitive tests (e.g., tail_file) should be `#[ignore]`.
