# CLAUDE.md — Project rules for AI assistants

This file is loaded automatically at the start of every Claude Code session.
These rules are non-negotiable.

## Commit workflow

Follow `COMMIT.md` exactly. The critical step most likely to be skipped:

### Step 3: Live docs sync (HARD GATE)

**Before every `git commit`, check and update each of these files:**

1. `CHANGES.md` — new entry for every shipped feature/fix
2. `docs/BACKLOG.md` — mark completed items
3. `MEMORY.md` — append dated session notes (NEVER delete old entries)
4. `README.md` — PGEN version pins, submodule refs, doc index (when changed)
5. `RUST_CODEBASE_ANALYSIS.md` — when architecture/roadmap changed
6. `DEVELOPMENT_NOTES.md` — when durable engineering understanding changed

**Do not run `git commit` until this checklist is done.**

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
- `docs/BACKLOG.md` has the task inventory with completion status.

## Testing

- Run `cargo fmt`, `cargo test -p rgx-core`, `cargo test -p rgx-cli`, `cargo clippy --workspace --all-targets` before committing.
- Zero clippy errors allowed. Warnings tolerated.
- Timing-sensitive tests (e.g., tail_file) should be `#[ignore]`.
