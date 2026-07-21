# DOCTRINE-ADOPT: adopt the Doctrine Enforcement Architecture

## Metadata

- Tree ID: `DOCTRINE-ADOPT`
- Status: `active`
- Roadmap lane: `Governance / process (4th portable architecture)`
- Created: `2026-07-21`
- Last updated: `2026-07-21`
- Owner: repo-local workflow

## Goal

Adopt the **Doctrine Enforcement Architecture** (devised by the sibling PGEN
project, standard at `pgen/DOCTRINE_ENFORCEMENT.md`) as RGX's 4th portable
architecture, alongside task-trees (1), the memory architecture (2), and the
Knowledge Map (3). Outcome: **every mechanizable RGX doctrine is paired with a
deterministic check, all checks run from one registry+driver, and both the git
hook (E3) and CI (E4) gate on that driver** — so compliance is provable and
re-checkable rather than a "trust me" claim.

Director directive (2026-07-21): "adopt that new doctrine enforcement."

## Non-Goals

- Not re-litigating any existing doctrine's content — this mechanizes the rules
  RGX already has (`CLAUDE.md`, `COMMIT.md`, `docs/TASK_TREE.md`,
  `docs/decisions/`), it does not invent new ones.
- Not claiming literal impossibility of violation. Per the standard §9, the
  honest claim is "expensive, visible, and blocked at every active gate"; local
  hooks remain `--no-verify`-bypassable and CI is the real backstop.
- Not porting PGEN-specific doctrines (EBNF-source-of-truth, regex
  self-hosting, oracle-anchor sync) — those govern PGEN's grammar work.

## Acceptance Criteria

- `scripts/check_doctrines.sh` exists (registry + driver, §5 of the standard):
  runs every registered check, collects ALL results, prints a per-doctrine
  PASS/FAIL/SKIP report, exits nonzero iff any failed, and **meta-checks** that
  every registered enforcer exists and is executable.
- Every registered check obeys the §4 contract (exit code = verdict, explains
  on breach to stderr, deterministic, repo-reading, scope-aware, path-agnostic).
- E3: `scripts/git-hooks/pre-commit` runs the driver (replacing its ad-hoc
  check stack). E4: `scripts/run-local-ci.sh` (which hosted CI executes) runs
  the driver.
- The registry and the human-readable manifest (`DOCTRINE_ENFORCEMENT.md` §10)
  are kept in lockstep **by a check**, not by discipline.
- Full `./scripts/run-local-ci.sh` green (this is a `scripts/*` change ⇒
  gate-affecting); the adoption commit is itself governed by the new checks.

## Task Tree

- ID: `DOCTRINE-ADOPT`
  Status: `active`
  Goal: `RGX doctrines are mechanically enforced via one registry+driver gated at E3+E4.`
  Children: `.1` `.2` `.3`

- ID: `DOCTRINE-ADOPT.1`
  Status: `done`
  Goal: `Port the kit + write RGX's doctrine checks + wire E1–E4: DOCTRINE_ENFORCEMENT.md (standard, §10 replaced by RGX's live registry); scripts/check_doctrines.sh (driver+registry with hook/ci scoping); new checks check_code_change_leaf.sh, check_two_track_docs.sh, check_pgen_submodule_readonly.sh, check_gate_receipt.sh (extracted from the hook), check_doctrine_registry_sync.sh; register the existing check_memory_architecture.sh + knowledge-map check; pre-commit delegates to the driver; run-local-ci.sh runs it; discovery pointers updated.`
  Acceptance: `All acceptance criteria above; the driver reports 7 doctrines; each new check demonstrated to BITE (negative test) and to pass on the clean tree; check-ci-paths.sh required_paths extended so the new scripts cannot vanish silently.`
  Verification: `2026-07-21 — MET. Driver green at both scopes (--scope ci: 4 PASS / 3 honest SKIP; --scope hook: 7). NEGATIVE TESTS (each check proven to bite, then restored): CODE-CHANGE-LEAF exit 1 on code staged with no docs/tasks file → exit 0 once staged; TWO-TRACK-DOCS exit 1 with no CHANGES.md entry staged; GATE-RECEIPT exit 1 on absent/stale receipt; DOCTRINE-REGISTRY-SYNC exit 1 on an undocumented registry id → 0 restored; PGEN-READONLY exit 1 on an edited tracked file in subs/pgen → 0 after checkout; driver meta-check exit 1 on a registry entry pointing at a nonexistent script. Full ./scripts/run-local-ci.sh green (receipt stamped) — see Verification Log.`
  Commit: `DOCTRINE-ADOPT.1 — adopt the Doctrine Enforcement Architecture`

- ID: `DOCTRINE-ADOPT.2`
  Status: `pending`
  Goal: `Evidence-archetype hardening: an RGX analogue of PGEN's check_diagnosis_evidence.sh — require a code change's owning task leaf to carry a tool-backed WHY+WHERE diagnosis and a measured before→after (the "reasoned-from-evidence" pattern, standard §6), with the oracle leg re-running the cited deterministic command where possible.`
  Acceptance: `A code-change commit whose leaf lacks diagnosis/verification evidence is blocked; the check re-runs at least one cited oracle rather than only grepping for a signature (standard §9 anti-pattern).`
  Verification: `pending`
  Commit: `pending`

- ID: `DOCTRINE-ADOPT.3`
  Status: `pending`
  Goal: `Close the GATE-RECEIPT "mid-run edit" hole found in .1: have scripts/run-local-ci.sh snapshot rgx_gate_state_id at gate START, re-compute at the END, and REFUSE to stamp the receipt if they differ (the tree changed under the gate). Today the receipt is computed only at the end, so an edit made while the gate runs is silently certified with no staleness signal.`
  Acceptance: `Editing a gate-affecting file mid-run causes run-local-ci.sh to exit nonzero with "tree changed under the gate — re-run" and NO receipt written; an untouched run still stamps normally; negative test recorded; full gate green.`
  Verification: `pending`
  Commit: `pending`

## Current Frontier

| Order | Leaf | Status | Why next |
| --- | --- | --- | --- |
| 1 | `DOCTRINE-ADOPT.3` | `pending` | Closes a **found, reproduced** hole in an active gate (the mid-run-edit receipt blind spot). Small, deterministic, and it hardens the mechanism every other code change depends on. |
| 2 | `DOCTRINE-ADOPT.2` | `pending` | The evidence archetype is the remaining (hardest) leg — it needs an RGX analogue of PGEN's `TOOLBOX.md` acceptance checklist and an oracle re-run leg. |

## Decisions

- `2026-07-21`: **Scoped registry entries.** RGX's driver adds a `scope` field
  (`always` / `hook`) that PGEN's does not need: the gate-receipt doctrine and
  the staged-set doctrines are meaningful only at commit time (a receipt does
  not exist server-side, and CI has no staged set). CI runs `--scope ci`, which
  reports those entries as `SKIP (hook-scope)` rather than passing them
  vacuously — honest reporting over a fake green.
- `2026-07-21`: **The Book leg of the two-track-docs doctrine stays advisory.**
  `CHANGES.md` is required for every code change (unconditional in `COMMIT.md`),
  so it is hard-gated; "does the Book need a chapter?" is conditional on
  user-visible behaviour and cannot be mechanized without false positives. The
  check prints the Book reminder but does not fail on it (standard §6.1: a box
  with no re-runnable oracle stays advisory, never hard-gated).
- `2026-07-21`: **`DOCTRINE_ENFORCEMENT.md` §10 is RGX's own registry**, not a
  copy of PGEN's; provenance is credited in the file header. Sections 0–9 and
  11 are the portable standard, copied faithfully.

## Open Questions

- Should `NO-COAUTHORED-BY` (a `commit-msg`-scope rule, `feedback_no_coauthored_by`)
  become a registered doctrine? It is message-scope, not tree-scope, so it does
  not fit the driver's repo-state contract; currently handled in the
  `commit-msg` hook. Revisit in `.2`.

## Findings during `.1` (self-review of the shipped kit)

- **Portability defect found and fixed before landing — `mapfile` in
  `lib-code-change-scope.sh`.** The first draft built the code-change pathspec
  with `mapfile` (a bash-**4+** builtin). Stock macOS ships **bash 3.2** as
  `/bin/bash`, where `mapfile` does not exist — leaving the pathspec array
  EMPTY, and an empty pathspec makes `git diff --cached --name-only --` match
  **every** staged file. The failure mode was the dangerous kind: not a loud
  error but a **silent widening of the doctrine's scope** (a docs-only commit
  would have been blocked demanding a task-tree file). Fixed by writing the
  pathspec as literal arguments — no bash-4 builtins anywhere in the kit.
  Verified: `/bin/bash -n` syntax-clean for all six enforcers, and bash 3.2 vs
  5.3 now report the identical staged-code set (10 of 30 staged files); the
  driver runs green under 3.2. A comment in the lib records the trap so it is
  not reintroduced.
- **`scripts/setup-hooks.sh` refreshed** (banner described the pre-driver hook
  stack; chmod list missed the new enforcers). Initially deferred to `.2`
  because editing a `scripts/*` file mid-gate would have made the receipt
  certify untested content; folded back into `.1` once the portability fix
  forced a gate re-run anyway. Note the chmod is belt-and-braces only — git
  tracks mode `100755` for every enforcer, verified via `git ls-files -s`.
- **Process note:** both findings came from reviewing the kit *as if someone
  else had written it* (does it run on a stock clone? does a fresh clone get
  executable enforcers?).

- **⚠️ LATENT WEAKNESS FOUND IN THE `GATE-RECEIPT` MECHANISM ITSELF (pre-existing,
  not introduced by this leaf) — the "mid-run edit" hole.** `run-local-ci.sh`
  computes and stamps `rgx_gate_state_id` **at the END of the run**, over the
  worktree as it stands *then*. So if a gate-affecting file is edited **while the
  gate is running**, the resulting receipt certifies content that the test steps
  never saw — and, critically, it produces **no staleness signal**: the receipt
  matches the current tree, so `check_gate_receipt.sh` passes.
  **Observed live during this leaf:** the portability fix to
  `lib-code-change-scope.sh` was made while gate run #1 was mid-feature-matrix;
  run #1's end-of-run receipt then matched the edited tree (`check_gate_receipt.sh`
  → exit 0) despite the earlier steps having run against the pre-fix content.
  Resolved for this commit by discarding that certification and re-running the
  gate end-to-end against the final tree (run #2).
  **Proposed hardening (leaf `.3`):** snapshot `rgx_gate_state_id` at gate START,
  re-compute at the END, and refuse to stamp if they differ ("the tree changed
  under the gate — re-run"). Cheap, deterministic, and closes the hole for good.
  This is the E2 self-check catching a flaw in an E3/E4 mechanism — exactly the
  kind of thing the standard's §9 "honest limits" section exists to keep visible.

## Blockers

- None.

## Verification Log

| Date | Leaf | Checks | Result |
| --- | --- | --- | --- |
| `2026-07-21` | `.1` | `./scripts/check_doctrines.sh --scope ci` (4 PASS / 3 SKIP) and `--scope hook` (7 checked); 6 negative tests (one per new check + the driver meta-check), each restored after; `./scripts/check-ci-paths.sh`; full `./scripts/run-local-ci.sh` (receipt stamped for this exact tree) | `green` |

## Commit Log

| Leaf | Commit subject or reference | Notes |
| --- | --- | --- |
| `.1` | `DOCTRINE-ADOPT.1 — adopt the Doctrine Enforcement Architecture` | Kit + 7 doctrines + E3/E4 wiring; ADR `docs/decisions/0006`. The commit is itself governed by the new checks (it carried its tree file, its CHANGES.md entry, and a fresh receipt). |

## Changelog

- `2026-07-21`: Created on the director's directive to adopt PGEN's Doctrine
  Enforcement Architecture. `.1` = the kit + structural doctrines (in
  progress); `.2` = the evidence archetype (queued).
