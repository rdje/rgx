# Durable Agent Memory Architecture

A portable, **harness-agnostic** standard for giving an AI coding agent memory that
survives **session loss, application/machine crashes, a switch to a different AI
model, and a switch to a different harness** (Claude Code, Codex, Cursor, Aider,
Continue, Copilot, a custom runner, …) — and that is **hard for any harness to ignore**.

This file is intentionally **project-agnostic**: it contains no nouns specific to any
one codebase. Drop it into any repository that already uses a **task-tree** work
tracking system and it applies as-is.

> One-line thesis: **memory survives only when it is in the repository, in git, in
> small structured units, discoverable from a tool-neutral entrypoint, and enforced by
> mechanical gates.** Anything else is cache.

---

## 0. How to use this file

1. Copy `MEMORY_ARCHITECTURE.md` to your repo root and commit it.
2. Wire the tool-neutral bootstrap pointers (§7) so any harness's agent is routed here.
3. Adopt the four layers (§3) and the write/read paths (§4–§5).
4. Demote `MEMORY.md` to the bounded resume pointer (§6).
5. Install the enforcement (§9) so non-compliance fails fast and cannot merge.
6. Run the adoption checklist (§11) once; follow the maintenance discipline (§10) forever.

If you only remember one rule: **information that exists only in the live conversation
is not yet saved — route it to a layer and commit it before the turn ends.**

---

## 1. The problem this solves

Two memories people reach for both fail the durability test:

- **An ever-growing `MEMORY.md`.** An append-only prose log. It conflates *current
  state*, *durable facts*, and *history* in one file that grows without bound, costs
  tokens to reload, goes stale at the top, and is hard for a *different* model to
  parse reliably. It also duplicates information git already holds.
- **Harness home-directory memory** (a tool's `~/.<tool>/…` store, rules that live
  outside the repo, chat scrollback). Invisible to any *other* tool, so a switch of
  harness or model loses it; not in version control, so a machine loss erases it.

The fix is not a new database. It is to place memory **in the repo, in git, in small
structured units, reachable from one tool-neutral entrypoint, and guarded by gates
that make the compliant path the easy path.**

---

## 2. The four durability properties

Any information that must survive belongs in a store that has **all four** properties.
If a store is missing even one, treat it as *cache*, never as the system of record.

| # | Property | What it buys you |
|---|---|---|
| 1 | **In-repo & git-tracked** | Survives session loss & app/machine crash (it's committed, and pushed it's off-machine). Survives harness/model switch (any tool that clones the repo sees it). Git is also a time machine: nothing committed is ever truly lost. |
| 2 | **Structured & addressable** | One unit per file/record, named, so a reader loads the *relevant* unit, not a monolith. Read cost stays bounded as the project grows. |
| 3 | **Plain-text & self-describing** | Markdown / JSON / YAML, with the *convention documented in the repo itself*. Any model in any harness can both **read and correctly write** it. |
| 4 | **Reachable from a tool-neutral entrypoint** | A fresh agent, told only "read the repo entrypoint," discovers the whole system — even though every harness auto-reads a *different* bootstrap file. |

---

## 3. The four memory layers (separate by lifecycle)

Mixing information with different lifecycles into one file is the root cause of the
blob. Keep four layers, each with its own lifecycle:

| Layer | Holds | Lifecycle | Lives in | Read when |
|---|---|---|---|---|
| **A — Resume pointer** | where we are *now*; the single next action; latest commit; any in-flight uncommitted work | **Overwritten** each update; **hard size cap** | one bounded file — the demoted `MEMORY.md` | first, on every resume |
| **B — Work memory** | what is being built/decided, per unit of work: goal, status, frontier, decisions, verification, commit refs | **Append within a unit**; the unit is the addressable file | the **task-tree** files + their index | the active unit on resume; any unit on demand |
| **C — Decision / fact records** | durable cross-cutting facts: constraints, learnings, conventions, preferences, environment quirks, "tried X, failed because Y" | **Append once, dedupe, supersede** (never silently rewrite) | one file per record (ADR-style) under `docs/decisions/` + an index | when relevant, by topic/index |
| **D — Audit trail** | the full history of *what changed and when* | **Append-only, immutable** | `git log` (+ a human-readable `CHANGELOG`) | on demand only — never reloaded wholesale |

Rule of thumb: **A is rewritten · B grows by unit · C grows by record · D is queried,
not loaded.** Nothing is appended to A. Nothing durable lives only in the chat.

Why this maps onto task-trees: a task-tree system already gives you layer **B** —
structured, per-unit, in-repo, with status/frontier/verification/commit logs. This
standard adds the *other three* layers around it so the task-trees aren't doing jobs
they're bad at (current-state pointer, cross-cutting facts, history).

---

## 4. The WRITE path — how information enters and is guaranteed to survive

When the agent learns, decides, or finishes something, route it **immediately, before
ending the turn**, by asking *"what kind of thing is this?"*:

- **A finished or decided step of tracked work** → update its **task-tree unit (B)**:
  status, verification, commit ref, frontier. Then commit.
- **A durable fact that will matter beyond this unit of work** (a constraint, an
  environment quirk, a user preference, an architecture decision, a failed approach
  and why) → write a **decision record (C)**: one dated, titled file with
  *Context → Decision → Consequences*. Link it from the related task-trees.
- **A change to "where we are / what's next"** → **overwrite** the resume pointer (A).
  Never append to it.
- **The change itself** (the diff) → a **commit (D)** whose subject carries the
  work-unit id, so history is greppable by unit.

**The durability guarantee:** a turn is not complete until the right layer holds the
information **and it is committed**. Commit *small and often* — an uncommitted change
survives nothing. Push regularly — an unpushed commit does not survive a machine loss.

---

## 5. The READ path — resuming in any harness, after any interruption

A fresh agent (same or different model/harness) resumes deterministically and with
**bounded** reading:

1. Read the **tool-neutral entrypoint (§7)** → it names this file and the task-tree +
   commit conventions.
2. Read the **resume pointer (A)** → current commit, active work unit, the single next
   action, any in-flight uncommitted work.
3. Open the **active task-tree unit (B)** → its frontier row *is* the precise next step.
4. Pull only the **decision records (C)** relevant to that step.
5. Consult **`git log` (D)** only if deeper history is needed.

A resume reads A + one unit of B + a few C records — never a monolith.

**Composed retrieval layer — the Knowledge Map.** The layers above are organised by
*lifecycle* (now / work / decisions / history); retrieval, though, is organised by
*question*. The **Knowledge Map** (`KNOWLEDGE_MAP_ARCHITECTURE.md`, the `knowledge-map/`
bundle) adds exactly that cross-cut: a machine-derived, question-keyed index
(`KNOWLEDGE_MAP.md`) over small front-mattered fact cards (`docs/knowledge/`, plus any
`docs/decisions/` record given an `answers:` block). Before re-deriving any fact from code
or runtime, grep it — re-deriving an already-logged fact is *archaeology*. It is **additive**
(replaces nothing, converts nothing), **derived** (never hand-edited), and gated by the same
pre-commit hook + `run_ci.sh` step as this architecture (so it cannot drift). Write a card
lazily — when a durable fact is established or archaeology is caught.

---

## 6. Demoting `MEMORY.md` to a bounded resume pointer

`MEMORY.md` becomes **only** layer A. Hard rules:

- **Size cap** — keep it to roughly one screen (≤ ~50 lines). If it exceeds the cap,
  information is in the wrong layer; move it down to B or C. *(This cap is mechanically
  enforced — §9.)*
- **Overwrite, don't append** — it always describes *now*, never the journey.
- **No history** — that's git (D) and the task-tree logs (B).
- **Prefer derived over hand-written** — a small script can regenerate the
  current-state block from `git log` + each tree's frontier row, so it cannot drift.

Existing bloat is **not deleted** — it is already preserved in git history. You simply
stop carrying it forward.

**Resume-pointer template** (the entire contents of a demoted `MEMORY.md`):

```markdown
# MEMORY — resume pointer (layer A; overwrite-only, keep ≤ ~50 lines)

## How to resume
- Read `MEMORY_ARCHITECTURE.md` (the memory system) and `README.md` (the project).
- Work is tracked in task-trees under `<tasks-dir>/`; follow `<commit-workflow-doc>`.
- Durable facts/decisions live in `docs/decisions/`.

## Current state (OVERWRITE this block each update — do not append)
- latest_commit: `<hash>` — "<subject>"   (ahead of origin: <N>; push at ~<threshold>)
- active_work_unit: `<TASK-TREE-ID>`  →  frontier leaf: `<LEAF-ID>` (<status>)
- next_action: <one concrete sentence>
- in_flight_uncommitted: <none | what is staged/unsaved and how to finish it>
- blockers: <none | what and who-owns>
```

---

## 7. The tool-neutral bootstrap entrypoint

Each harness auto-reads a *different* file: `CLAUDE.md`, `AGENTS.md`, `.cursorrules`,
`.github/copilot-instructions.md`, `GEMINI.md`, `.windsurfrules`, … Do **not**
duplicate the system into each — they drift. Instead:

- Put the system of record in **`README.md`** (every tool and human opens it) and in
  **this file**.
- Make each harness bootstrap file a **one-line pointer** to them.

A new agent in any harness, reading its native bootstrap file, is then routed to the
same place. Discovery is solved once, for all tools.

**`AGENTS.md` template** (tool-neutral; mirror the same pointer into `CLAUDE.md`,
`.cursorrules`, `.github/copilot-instructions.md`, etc.):

```markdown
# Agent bootstrap (read this first, whatever AI/harness you are)

1. Read `README.md` (project objective, layout, commands).
2. Read `MEMORY_ARCHITECTURE.md` (how memory + continuity work here — MANDATORY).
3. Resume from `MEMORY.md` (the resume pointer) → the active task-tree's frontier.
4. Track ALL work in task-trees under `<tasks-dir>/`; record durable facts in
   `docs/decisions/`; commit per `<commit-workflow-doc>` with the work-unit id in the
   subject.
5. Before committing, run `scripts/check_memory_architecture.sh` — CI runs it too.

Nothing important may live only in this conversation — route it to a layer and commit.
```

---

## 8. Git is the backbone

- **Commit small and often** — an uncommitted change is not memory.
- **Put the work-unit id in the commit subject** (`UNIT-ID.step — summary`) so
  `git log --grep` reconstructs any unit's full history. *(Enforced — §9.)*
- **Push regularly** — the remote is your crash insurance; an unpushed commit dies
  with the machine.
- Because every durable layer (A, B, C) is *tracked files*, **git versions the memory
  itself**: you can always recover what the state was at any past commit.

---

## 9. Enforcement — making it hard NOT to follow

A document a harness can read is a document a harness can ignore. To make
non-compliance *expensive and visible*, layer four mechanisms — defense in depth. Each
catches what the previous misses; together they make the easy path the compliant path.

**E1 — Ubiquitous bootstrap (discovery is unavoidable).** Ship a one-line pointer in
*every* harness's auto-read file, all pointing at the same system of record: `AGENTS.md`
(Codex, Amp, and a growing common convention), `CLAUDE.md` (Claude Code),
`.cursorrules` / `.cursor/rules` (Cursor), `.github/copilot-instructions.md` (Copilot),
`.windsurfrules` (Windsurf), `GEMINI.md` (Gemini CLI). Whatever tool a user brings, its
first read routes the agent here. Keep each to one line + a pointer so they can't drift.

**E2 — One self-check script (a single source of truth for the invariants).** A tracked
script (e.g. `scripts/check_memory_architecture.sh`) that exits **nonzero** on any
violation: `MEMORY.md` missing or over the line cap; a bootstrap file missing or not
pointing at `MEMORY_ARCHITECTURE.md` + `README.md`; `docs/decisions/` missing or its
index out of sync with the record files. Everything below calls this one script, so the
rules live in exactly one place and can't fork.

**E3 — Git hooks (fast local gate).** Tracked hooks under `.githooks/`, activated by
`git config core.hooksPath .githooks` (ship a one-line installer and name it in the
bootstrap): `pre-commit` runs the self-check (a non-compliant tree can't commit);
`commit-msg` rejects a subject lacking the work-unit id pattern (enforces §8). *Honest
limit:* hooks are local and a determined user can `--no-verify` or skip `hooksPath` —
they catch the common case cheaply; they are not the backstop.

**E4 — CI gate (the un-bypassable backstop).** A CI job runs the **same** self-check
script and validates that every commit subject on the branch carries a work-unit id.
CI is server-side: `--no-verify` doesn't reach it. A non-compliant branch **fails the
build and cannot merge**. This is what makes non-compliance genuinely hard — the work
does not land until it is compliant.

**Why layered, not a single wall:** discovery (E1) makes the rules unmissable; the
self-check (E2) makes them executable; hooks (E3) make violations fail *fast*; CI (E4)
makes them fail *unconditionally*. To land non-compliant work an agent or human would
have to defeat all four — and CI cannot be bypassed from a clone.

**Reference self-check script** (adapt paths; keep it the single source of truth):

```bash
#!/usr/bin/env bash
# scripts/check_memory_architecture.sh — fail nonzero on any memory-architecture breach.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"; cd "$ROOT"
CAP="${MEMORY_POINTER_LINE_CAP:-60}"; fail=0
note(){ printf 'memory-arch: %s\n' "$1" >&2; fail=1; }

[ -f MEMORY_ARCHITECTURE.md ] || note "MEMORY_ARCHITECTURE.md is missing"
[ -f MEMORY.md ] || note "MEMORY.md (resume pointer) is missing"
if [ -f MEMORY.md ]; then
  n=$(wc -l < MEMORY.md)
  [ "$n" -le "$CAP" ] || note "MEMORY.md is $n lines (> cap $CAP) — demote content to task-trees/decisions"
fi
for f in AGENTS.md CLAUDE.md; do
  [ -f "$f" ] || { note "$f bootstrap pointer is missing"; continue; }
  grep -q "MEMORY_ARCHITECTURE.md" "$f" || note "$f does not point at MEMORY_ARCHITECTURE.md"
done
[ -d docs/decisions ] || note "docs/decisions/ (layer C) is missing"
exit $fail
```

**Reference hooks** (`.githooks/pre-commit`, `.githooks/commit-msg`):

```bash
# .githooks/pre-commit
#!/usr/bin/env bash
exec "$(git rev-parse --show-toplevel)/scripts/check_memory_architecture.sh"
```
```bash
# .githooks/commit-msg
#!/usr/bin/env bash
# Require a work-unit id token in the subject (adapt the pattern to your scheme).
head -1 "$1" | grep -Eq '^[A-Z][A-Z0-9-]+(\.[0-9A-Za-z]+)?[ —:-]' \
  || { echo "commit-msg: subject must start with a WORK-UNIT-ID (e.g. UNIT-ID.step — …)"; exit 1; }
```
Install once per clone: `git config core.hooksPath .githooks`.

**CI step** (add to your existing pipeline so it can't be skipped):

```bash
bash scripts/check_memory_architecture.sh
# and, optionally, assert every new commit subject carries a unit id:
git log --format='%s' origin/main..HEAD | grep -vEq '^[A-Z][A-Z0-9-]+' && exit 1 || true
```

### 9.1 Reproduce this in any project — the agnostic enforcement kit

The enforcement above is deliberately **path-agnostic and copy-pasteable**, so any
project achieves the same "genuinely hard to be non-compliant" property by following
the same footsteps — no bespoke wiring. The kit is a fixed, small set of files plus
three commands:

**Copy these verbatim** (they make no project-specific assumptions):
- `MEMORY_ARCHITECTURE.md` — this standard.
- `scripts/check_memory_architecture.sh` — the single source of truth for the invariants.
- `.githooks/pre-commit`, `.githooks/commit-msg` — the local gate.

**Add these one-line pointer files** (one per harness you might use; each just points at
`README.md` + `MEMORY_ARCHITECTURE.md`):
- `AGENTS.md`, `CLAUDE.md`, `.cursorrules`, `.github/copilot-instructions.md`,
  `GEMINI.md`, `.windsurfrules`.

**Run these three commands once:**
```bash
mkdir -p docs/decisions && printf '# Decision records (layer C)\n' > docs/decisions/INDEX.md
git config core.hooksPath .githooks
# then add one line to your CI pipeline:  bash scripts/check_memory_architecture.sh
```

**The only knobs to adapt per project** (everything else is identical):
- `MEMORY_POINTER_LINE_CAP` — the resume-pointer size cap (env var; default in-script).
- the `commit-msg` subject regex — set it to your work-unit id scheme.
- `<tasks-dir>` and `<commit-workflow-doc>` — your task-tree directory and commit-workflow
  doc names, referenced in the bootstrap pointers.

Because the check script is the **single source of truth** and is written against
conventional, project-neutral paths, copying the kit reproduces the *same* four-layer
gate (E1 discovery · E2 self-check · E3 hooks · E4 CI) everywhere. Following these
footsteps, a different project — in any harness, with any model — lands non-compliant
work only by defeating all four layers, and CI (E4) cannot be defeated from a clone.

---

## 10. Maintenance — keeping memory from rotting

- **Compaction** — periodically distil transient notes into durable decision records
  (C) and drop the transcript. Memory is *curated*, not accreted.
- **Supersede, don't mutate** — when a fact changes, add a new record (or mark the old
  one `superseded by …`) so the audit trail stays honest.
- **Dedupe** — before writing a fact, check the index for an existing record and update
  it instead of forking a near-duplicate.
- **Cap enforcement** — if the resume pointer (A) grows past its cap, the self-check
  (§9) fails: that is the signal that content belongs down in B or C.

---

## 11. Adoption checklist (for a repo already using task-trees)

1. Add `MEMORY_ARCHITECTURE.md` at the repo root; commit.
2. Create `docs/decisions/` with an index for layer C. Seed it by moving durable facts
   currently buried in `MEMORY.md` (and any harness-home-dir memory) into dated records.
3. Trim `MEMORY.md` to the §6 resume-pointer template; its history stays in git.
4. Wire the bootstrap (§7): make `README.md` reference this file + the task-tree and
   commit conventions; add the one-line pointer to `AGENTS.md` and mirror it into every
   harness bootstrap file you might use (`CLAUDE.md`, `.cursorrules`,
   `.github/copilot-instructions.md`, …).
5. Install enforcement (§9): add `scripts/check_memory_architecture.sh`, the
   `.githooks/`, `git config core.hooksPath .githooks`, and the CI step.
6. Adopt the write path (§4) and read path (§5) as the working discipline.
7. *(Optional)* Add a script to regenerate the resume pointer's current-state block from
   `git log` + tree frontiers, so layer A is derived, not hand-maintained.

---

## 12. Anti-patterns

- ❌ An append-only `MEMORY.md` that grows every session.
- ❌ Memory in a harness home directory (lost on tool switch; untracked).
- ❌ Re-narrating git history into prose docs (duplication that goes stale).
- ❌ Durable facts living only in the conversation or chat scrollback.
- ❌ One giant file you must read top-to-bottom to find "what's next."
- ❌ Hand-maintained current-state that drifts from reality (prefer derived).
- ❌ A harness-specific rules file treated as the system of record instead of a pointer.
- ❌ Recommendations with no enforcement — a rule nothing checks is a rule nothing follows.

---

## 13. Durability matrix

How each failure mode is covered, layer by layer (✓ = survives; ✓\* = survives **if
pushed to a remote**):

| Failure mode | A — pointer | B — task-trees | C — decisions | D — git history |
|---|---|---|---|---|
| Session lost / context compaction | ✓ (re-read) | ✓ (frontier) | ✓ | ✓ |
| App / tool crash | ✓ | ✓ | ✓ | ✓ |
| Machine loss | ✓\* | ✓\* | ✓\* | ✓\* |
| Switch AI **harness** (Claude Code → Codex → …) | ✓ (in-repo) | ✓ | ✓ | ✓ |
| Switch AI **model** | ✓ (plain-text) | ✓ | ✓ | ✓ |

The single point of failure is **not committing / not pushing** — which is exactly what
the enforcement (§9) makes hard to do silently. Everything else is covered because
every layer is a tracked file. Hence the prime directive: *route to a layer, commit,
and push.*

---

## 14. Glossary

- **Task-tree** — the project's pre-existing, structured, per-unit work-tracking system
  (layer B). This standard assumes it exists; it does not redefine it.
- **Decision record (ADR)** — a small, dated, titled file capturing one durable
  decision/fact: *Context → Decision → Consequences*. The layer-C unit.
- **Resume pointer** — the demoted, bounded, overwrite-only `MEMORY.md` (layer A).
- **Tool-neutral entrypoint** — `README.md` + this file, reached from whatever bootstrap
  file a given harness happens to auto-read (§7).
- **Work-unit id** — the identifier (e.g. a task-tree leaf id) carried in commit
  subjects so git history is greppable by unit (§8).

---

*This document is itself an instance of the architecture it describes: a small,
structured, in-repo, git-tracked, self-describing unit — portable across any harness,
model, or session, and backed by mechanical enforcement so it is hard to ignore.*
