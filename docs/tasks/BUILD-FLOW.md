# BUILD-FLOW: Zero-friction build for downstream / submodule consumers

## Metadata

- Tree ID: `BUILD-FLOW`
- Status: `done`
- Roadmap lane: `Tooling / build UX (downstream adoption)`
- Created: `2026-06-15`
- Last updated: `2026-06-15`
- Owner: repo-local workflow
- Downstream report: `linkedspec/docs/tasks/RGX-BUILD-REPRO.md`

## Goal

A project that git-submodules RGX must be able to build it with **zero friction** —
one simple, documented command that hides the PGEN bootstrap and produces a
working `rgx-core`. Fix the two cold-clone build failures LINKEDSPEC reported:
**Path A** (default features — PGEN's generated parser file missing, bootstrap
not automated/discoverable) and **Path B** (`--no-default-features` — the
pgen-free reference build has bit-rotted: 36 compile errors).

## Non-Goals

- Do NOT modify `subs/pgen` tracked content (read-only — ADR 0002). The deepest
  zero-step fix (pgen self-bootstrapping via its OWN `build.rs`) is a PGEN-side
  change; RGX cannot make it. RGX's job is the simple build command + a clean,
  discoverable flow, and to keep its own non-default build compiling.
- Not publishing to crates.io (parked — ADR 0003 / `RELEASE-CRATESIO`).
- No engine/semantic behavior change on the default path.

## Acceptance Criteria

- A single user-friendly command builds RGX + all deps from a cold clone /
  submodule (hides the PGEN `regex_parser_bootstrap`); documented in README.
- `cargo build -p rgx-core --no-default-features` compiles cleanly (B1/B2/B3
  fixed) — the pgen-free reference build is restored.
- Full `./scripts/run-local-ci.sh` green (+ PCRE2 conformance ratchet for the
  parsing-touching Path B leaf); downstream repro expectations addressed.

## Task Tree

- ID: `BUILD-FLOW`
  Status: `done`
  Goal: `Downstream/submodule consumers build RGX with zero friction.`
  Children: `.1` `.2` `.3` `.4` (all `done`)

- ID: `BUILD-FLOW.1`
  Status: `done`
  Goal: `Path A — add a single user-friendly build entrypoint (root Makefile) that hides PGEN: ensures submodules + runs the regex_parser_bootstrap when the generated parser is missing + cargo build. Idempotent; works from a cold clone / as a submodule. Document it prominently in README ("Building RGX"). Note the PGEN-upstream build.rs option for true zero-step cargo (out of RGX's scope — read-only submodule).`
  Acceptance: `make / make build works from a tree without subs/pgen/generated/*; README documents it for direct + submodule-consumer use; idempotent (re-run is a no-op when already bootstrapped). Non-gate-affecting (root Makefile + docs) — verify by running it.`
  Verification: `Root Makefile added: targets build (default)/bootstrap/submodules/test/gate/clean/help. bootstrap is idempotent — only runs regex_parser_bootstrap when subs/pgen/generated/{regex_parser,return_annotation_parser}.rs is missing; submodules inits only subs/pgen (build dep, not the heavy subs/pcre2). Verified: make help lists targets; make bootstrap on a tree with the parser present → "already generated — nothing to bootstrap" (rc 0). README "Build, test, run" leads with `make`, documents the submodule-consumer one-liner (make -C path/to/rgx bootstrap), and the Build note explains why true zero-step cargo isn't possible from RGX (pgen compiles first; subs/pgen read-only). Non-gate-affecting (root Makefile is not in the gate pathspec).`
  Commit: `see Commit Log (leaf BUILD-FLOW.1)`

- ID: `BUILD-FLOW.2`
  Status: `done`
  Goal: `Path B — fix the --no-default-features (pgen-free reference) build: B1 CharRange feature-gate (import gated on pgen-parser but posix_class_ranges() uses it unconditionally — parsing.rs:7293), B2 non-exhaustive ast::Regex match in the recursive-descent parser (parser.rs), B3 any residual edition/_ / unreachable issues surfaced once B1/B2 clear.`
  Acceptance: `cargo build -p rgx-core --no-default-features compiles with 0 errors; default build unchanged; full run-local-ci.sh green + PCRE2 conformance ratchet held (parsing-touching). GATE-AFFECTING.`
  Verification: `B1: moved CharRange to an UNGATED import in parsing.rs (it's a plain AST type used by ungated posix-class helpers; the default/PGEN build is semantically unchanged). B2: added the 12 missing PGEN-path variants to parser.rs::regex_kind (RelativeBackreference/ReturnedCaptureSubroutine/Callout/MatchReset/NewlineSequence/GraphemeCluster/Accept/Commit/Prune/Skip/Then/Mark) — explicit arms, no wildcard masking; parser.rs only compiles in the no-default build. B3: parser.rs ci_override_ranges: _ → None (the `_` was an invalid value expression; the reference parser computes no CI-override ranges). 36 → 0 errors. Default build still compiles unchanged. Added a durable guard: run-local-ci.sh now runs `cargo check -p rgx-core --no-default-features` so this family can't bit-rot again. **Full RGX_RUN_CONFORMANCE=1 run-local-ci.sh GREEN — the no-default-features check passed and pcre2_full_testdata_conformance ... ok (ratchet held 12,806/4); receipt matches tree.** GATE-AFFECTING.`
  Commit: `see Commit Log (leaf BUILD-FLOW.2)`

- ID: `BUILD-FLOW.3`
  Status: `done`
  Goal: `Verify end-to-end (simulate a cold-clone/submodule build via the new command + the pgen-free build) + add a fact card for the build flow + respond to the downstream repro (what was fixed / how to build) + close.`
  Acceptance: `Both build paths verified; KNOWLEDGE_MAP card for "how do I build RGX as a submodule"; downstream answer recorded; tree closed.`
  Verification: `Both paths verified in .1/.2 (make bootstrap idempotent rc0; --no-default-features 0 errors; default + conformance green). KM card rgx-build-as-submodule added (+ refreshed pgen-build-regenerate to point at make bootstrap); map → 5 facts / 28 keys, in sync. Downstream answer recorded below (for LINKEDSPEC RGX-BUILD-REPRO). Tree closed → Completed.`
  Commit: `see Commit Log (leaf BUILD-FLOW.3)`

- ID: `BUILD-FLOW.4`
  Status: `done`
  Goal: `Add a discoverable handoff/integration document that precisely describes how a downstream consumer builds a fully featured + functional RGX (+ PGEN regex parser): prerequisites, submodule topology, the make flow + why plain cargo build is cold-fragile, Cargo path-dep wiring + feature flags, the pgen-free option, the C ABI, post-bump steps, verification, troubleshooting, pins.`
  Acceptance: `docs/INTEGRATION.md exists, accurate to the workspace (MSRV 1.95; default = std+pgen-parser+jit; optional lua/js/rhai/wasm; rgx-capi C ABI; PGEN pin db6f8c68/1.1.81); linked from README (build section + doc index) and the KM card; KM in sync.`
  Verification: `docs/INTEGRATION.md authored (13 sections), grounded in verified facts from rgx-core/Cargo.toml (default features, MSRV), rgx-capi/Cargo.toml (crate-type), and the Makefile. README build section + docs/ index point to it; KM card rgx-build-as-submodule now names it as the canonical home (map regenerated, in sync). Docs-only / non-gate-affecting.`
  Commit: `see Commit Log (leaf BUILD-FLOW.4)`

## Current Frontier

| Order | Leaf | Status | Why next |
| --- | --- | --- | --- |
| — | — | — | All leaves `done`. `BUILD-FLOW` complete → Completed Task Trees in `docs/TASK_TREE.md`. |

(`.1`–`.3` all completed 2026-06-15 — `make` entrypoint (Path A); `--no-default-features` 36→0 + CI guard (Path B); KM card + downstream answer; full gate + conformance GREEN.)

## Downstream response (LINKEDSPEC `RGX-BUILD-REPRO`)

Both reported failures are fixed on RGX `main` (post-`b771c7b`):

- **Path A (default features):** RGX now ships a root `Makefile`. Build RGX (and
  all deps) with **`make`**. As a submodule consumer, run **`make -C path/to/rgx
  bootstrap`** once (idempotent — generates PGEN's parser), then your own
  `cargo build` works. True zero-step `cargo build` is not possible from RGX
  alone (cargo compiles the `pgen` dep before RGX's build script; `subs/pgen` is
  read-only) — that would need a PGEN-upstream `build.rs`; flagged for the PGEN
  maintainer.
- **Path B (`--no-default-features`):** fixed — `cargo build -p rgx-core
  --no-default-features` now compiles (B1 CharRange import, B2 `regex_kind`
  exhaustiveness, B3 `ci_override_ranges` value). This is the lighter pgen-free
  reference build (no PGEN/serde/wasmtime/cranelift, no bootstrap) and is now CI-
  guarded against recurrence. Caveat: the reference parser supports less syntax
  than the PGEN default path.

crates.io publication remains parked (ADR 0003); these two paths unblock a
submodule consumer today.

## Decisions

- `2026-06-15`: True zero-step `cargo build` is impossible from RGX alone —
  cargo compiles the `pgen` dependency BEFORE `rgx-core`'s build script, and
  `subs/pgen` is read-only (so RGX can't add pgen's missing generated file or a
  pgen `build.rs`). Therefore Path A ships a **simple build command** (the
  user's explicit ask) that runs the bootstrap, and the true-zero-step option
  (pgen self-bootstrap via its own `build.rs`) is flagged as a PGEN-upstream
  improvement for the maintainer.
- `2026-06-15`: `--no-default-features` IS an intended config (the
  recursive-descent `parser.rs` reference backend, "retained for non-pgen-parser
  builds" per RUST_CODEBASE_ANALYSIS); its bit-rot is a real bug → fix it (B),
  not reject the config. The default path ("PGEN is the sole parser") is
  unchanged.

## Open Questions

- Does the pgen-free reference parser support enough syntax for a real consumer?
  Out of scope here — `.2` only restores *compilation*; capability is separate.

## Blockers

- None for `.1`. `.2`/`.3` need a working `./scripts/run-local-ci.sh`.

## Verification Log

| Date | Leaf | Checks | Result |
| --- | --- | --- | --- |
| `2026-06-15` | — | reproduced Path A (report) + Path B (36 errors at HEAD, `cargo build -p rgx-core --no-default-features`) | `reproduced` |
| `2026-06-15` | `.1` | `make help` lists targets; `make bootstrap` idempotent no-op when parser present (rc 0); README documents direct + submodule-consumer use; non-gate-affecting | `pass` |
| `2026-06-15` | `.2` | B1/B2/B3 fixed → `--no-default-features` 36→0 errors; default build unchanged; CI guard added (`cargo check --no-default-features`); **RGX_RUN_CONFORMANCE=1 run-local-ci.sh GREEN, ratchet held 12,806/4**; GATE-AFFECTING | `pass` |
| `2026-06-15` | `.3` | KM card `rgx-build-as-submodule` added (map → 5 facts/28 keys, in sync); downstream answer recorded; tree closed; docs-only | `pass` |
| `2026-06-15` | `.4` | `docs/INTEGRATION.md` authored (downstream handoff guide, 13 sections, fact-grounded); README + KM card route to it; map in sync; docs-only | `pass` |

## Commit Log

| Leaf | Commit subject or reference | Notes |
| --- | --- | --- |
| `.1` | `BUILD-FLOW.1 — add make build entrypoint that hides the PGEN bootstrap` (`aa0297b`) | docs/tooling (root Makefile + README); not pushed unless user asks. |
| `.2` | `BUILD-FLOW.2 — fix the --no-default-features (pgen-free) build` (`c8eb604`) | gate-affecting; full gate + conformance green; not pushed unless user asks. |
| `.3` | `BUILD-FLOW.3 — KM card + downstream response; close tree` (`53f809d`) | docs-only; not pushed unless user asks. |
| `.4` | `BUILD-FLOW.4 — add docs/INTEGRATION.md downstream handoff guide` | docs-only; not pushed unless user asks. |

## Changelog

- `2026-06-15`: Created from the LINKEDSPEC `RGX-BUILD-REPRO` downstream report
  (submodule consumer can't build RGX). Leaf owns the fix per the Code-Change
  Doctrine (touches build tooling + Rust source).
- `2026-06-15`: First three leaves completed; tree closed. Path A (`make`
  entrypoint) + Path B (`--no-default-features` restored 36→0 + CI guard) both
  shipped; full gate + PCRE2 conformance GREEN (ratchet held 12,806/4); KM card
  added; downstream answer recorded. A submodule consumer can now build RGX with
  one command. (Open follow-up for the PGEN maintainer: a pgen `build.rs` that
  self-generates the parser would enable true zero-step `cargo build`.)
- `2026-06-15`: Re-opened to add `.4` (user request) — a dedicated, discoverable
  handoff/integration document `docs/INTEGRATION.md` for downstream consumers,
  precisely describing how to build a fully featured RGX (+ PGEN). Linked from
  README + the KM card. Tree closed again with 4 leaves.
