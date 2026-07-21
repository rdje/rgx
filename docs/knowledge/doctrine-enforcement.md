---
id: doctrine-enforcement
title: RGX doctrines are machine-enforced — scripts/check_doctrines.sh is the registry+driver
answers:
  - "how are RGX's rules enforced"
  - "what stops a code change landing without a task tree"
  - "how do I add a new doctrine or rule check"
  - "what does the pre-commit hook run"
  - "why was my commit blocked"
  - "what is the doctrine enforcement architecture"
date: 2026-07-21
status: current
tags: [governance, enforcement, hooks, ci]
evidence: DOCTRINE_ENFORCEMENT.md §10; scripts/check_doctrines.sh (DOCTRINES array); scripts/git-hooks/pre-commit
reverify: ./scripts/check_doctrines.sh --scope ci
---

RGX's doctrines are **not prose-only**. Per `DOCTRINE_ENFORCEMENT.md` (adopted
from PGEN 2026-07-21, ADR 0006), every mechanizable doctrine has a deterministic
check; `scripts/check_doctrines.sh` is the **registry + driver** that runs them
all, reports PASS/FAIL/SKIP per doctrine, and exits nonzero on any breach. The
pre-commit hook runs it (`--scope hook`, E3) and `scripts/run-local-ci.sh` runs
it (`--scope ci`, E4 — hosted CI executes that script).

**Enforced today (7):** `MEMORY-ARCH`, `KNOWLEDGE-MAP`, `PGEN-READONLY`,
`DOCTRINE-REGISTRY-SYNC` (all `always`-scope, structural) + `CODE-CHANGE-LEAF`,
`TWO-TRACK-DOCS`, `GATE-RECEIPT` (`hook`-scope: staged-set / local-receipt
semantics; honestly reported as SKIP in CI rather than passing vacuously).

**If a commit is blocked**, the driver's report names the doctrine and the check
prints the fix. Common ones: code staged without an updated `docs/tasks/<TREE>.md`
(CODE-CHANGE-LEAF), without a `CHANGES.md` entry (TWO-TRACK-DOCS), or without a
fresh `./scripts/run-local-ci.sh` receipt for exactly that tree (GATE-RECEIPT —
any further code edit invalidates it, so run the gate *last*).

**Adding a doctrine** = write `scripts/check_<id>.sh` obeying the §4 contract
(exit code is the verdict · explain on stderr · deterministic · reads the repo,
mutates nothing · scope-aware · path-agnostic) + one `DOCTRINES=()` registry line
+ one `DOCTRINE_ENFORCEMENT.md` §10 row. Never add enforcement logic to the hook.
`DOCTRINE-REGISTRY-SYNC` fails if you update one side and forget the other; the
driver's meta-check fails if a registered script is missing.

Honest limit: local hooks are `--no-verify`-bypassable, so CI is the backstop.
Related: [[governance-entrypoints]], [[pcre2-conformance-ratchet]].
