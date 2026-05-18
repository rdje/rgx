# Publish Readiness Checklist

**Purpose.** This document enumerates the criteria that gate publishing rgx (or the PGEN regex parser carrying rgx) to a public package registry. Each criterion is here because the user has identified it as load-bearing — closing every item is a precondition for publication, not just a nice-to-have.

**Decision rule.** Stated by the user, 2026-05-13: *"I won't publish on crates.io unless I have all the deliverables necessary to publish. I want to do it right."*

That decision rule has three properties worth restating:

1. **It is the user's call when the bar is met.** No mechanical metric flips the switch; the criteria below are the user's working definition, and the user is the gatekeeper.
2. **The list is open-ended.** The original statement ended with "..." — new criteria may surface as work progresses.
3. **Closing individual items does NOT mean "publication is closer."** Each closure is work toward the bar; "ready to publish" is a single discrete event when every criterion is green.

**Publication target.** Per `MEMORY.md` (`project_release_strategy`), the public release vehicle is the PGEN regex parser, not the RGX crate(s) directly. The `rgx-capi` artefacts (`librgx.{so,dylib,a}` + `rgx.h`) can ship independently of crates.io — distribution there is through GitHub releases / package mirrors, not the Rust registry.

---

## Criteria

Status legend: **Not started** · **In progress** · **Complete** · **Blocked**.

### 1. API contract stabilization

**What "done" looks like.** A `STABILITY.md` (or equivalent) exists for every published surface, spelling out:

- SemVer mapping (what change kind warrants major / minor / patch increment).
- The append-only / never-renumber rule for error codes and opcodes.
- Deprecation policy with concrete timelines (e.g. "deprecated APIs survive N minor versions before removal").
- Per-function stability tier (stable / experimental / internal).
- CI gate that fails when the C header (`rgx-capi/include/rgx.h`) changes without a corresponding version bump.

**Surfaces this applies to.**

| Surface | Need contract? | Reason |
|---|---|---|
| `rgx-capi` (C ABI) | **YES, highest priority** | External non-Rust consumers will load `.so`/`.dylib` they can't recompile when signatures change. |
| `rgx-core` (Rust API) | Probably YES before publication | Once on crates.io, third-party Rust consumers will pin against semver guarantees. |
| `rgx-cli` (command-line UX) | Probably YES, lighter touch | Scripts pipe through the CLI; flag-rename surprises break user scripts. |
| PGEN regex grammar | OUT OF SCOPE for this doc | Lives in the PGEN project and is part of PGEN's own readiness bar. |

**Current state.** In progress. **`rgx-capi/STABILITY.md` drafted 2026-05-18** against the shipped Phase 1 surface — covers all five required elements: SemVer mapping (change → version digit), the append-only / never-renumber error-code rule (with the `-99` sentinel and `-4`/`-6` reserved gaps), the deprecation policy with concrete timelines (≥2 releases, replacement-ships-first), per-function stability tiers, and the header-drift CI-gate *contract* (§7) plus memory/threading invariants and a contributor checklist. The Phase 0 design doc (`docs/A9_LANGUAGE_BINDINGS_DESIGN.md` §4) is now superseded by STABILITY.md as the authoritative C-ABI contract. The CI *gate* itself (`scripts/check-capi-abi.sh` + wiring) is specified but not yet implemented — until then §7 is enforced by reviewer discipline.

**Next concrete step.** Implement the header-drift CI gate per STABILITY.md §7 (tracked: backlog "rgx-capi header-drift CI gate"). Then revisit the Rust-side (`rgx-core`) contract only when crates.io publication is on the immediate horizon or a third-party Rust consumer materializes.

---

### 2. Book 100% in sync with the codebase

**What "done" looks like.** Every public feature (Rust API + CLI + C ABI) has a corresponding section or chapter in `book/src/**`. No published feature is undocumented; no documented feature has been silently removed or renamed.

**Why this matters.** CLAUDE.md already enforces book-update-per-commit as a hard gate, so individual commits add documentation. The risk is **cumulative drift**: a feature lands documented at version X, then the implementation gradually evolves while the chapter doesn't follow. A cumulative audit catches that.

**Current state.** In progress (continuous). Enforced per-commit by CLAUDE.md. No periodic cumulative audit has run.

**Next concrete step.** Schedule a single cumulative audit pass before publication: for each `book/src/**/*.md`, verify the code examples still compile against current `rgx-core`, the API names still exist, and the CLI flag names still match `rgx --help`. Track findings as their own backlog items.

---

### 3. No showstopper bugs

**What "done" looks like.** PCRE2 conformance ratchet meets the user's judgment of "no showstopper" — *not* a mechanical metric, but practically:

- All ratchet **panics** are zero (currently: 0).
- All ratchet **skips** are zero (currently: 0).
- Remaining ratchet **failures** are classified, understood, and triaged to either "won't fix" or "tracked for a future commit." Each residual has a written reason it's not blocking.
- The host integration surfaces (Lua / JS / Rhai / WASM / native / steering / events / `tail_file`) have no known data-corruption or panic bugs.

**Current state.** Ratchet at **12,806 passing / 4 failing / 0 panics / 0 skips**. Of the 4 residuals:

- **testinput1:3910** — `\10` forward-reference. Tracked as BACKLOG #60, **BLOCKED ON PGEN-RGX-0084**. Not an RGX bug.
- **testinput2:6592/6595/6601** (3 cases) — cross-subexpr alt-frame promotion. Tracked as BACKLOG #59. Engine-side fix needed; not yet attempted.

No known data-corruption or panic bugs in the host integration surfaces.

**Next concrete step.** Land the BACKLOG #59 family fix (3 ratchet cases close together once it ships). Document the BACKLOG #60 case as "blocked on PGEN, won't ship until PGEN resolves" so it has an explicit triage outcome.

---

### 4. PGEN-RGX-0073: compile-time perf

**What "done" looks like.** PGEN's parse stage is no longer 65-99% of consumer-tool compile time, and no longer 2,230-3,482× slower than PCRE2 on parse alone. The exact threshold is PGEN's call, not RGX's, but practically: PGEN's regex parser parse-time should be in the same order of magnitude as PCRE2's.

**Why this matters for publication.** Per `project_release_strategy`, the public release vehicle is the PGEN regex parser. Publishing PGEN with the current compile-time profile would be "embarrassed-on-the-internet" territory.

**Current state.** Blocked. Lives in the PGEN project (`pgen-issues/PGEN-RGX-0073.yaml`); RGX cannot resolve it directly.

**Next concrete step.** Nothing actionable from the RGX side. RGX continues to file precise PGEN bug reports per the cluster-first protocol so PGEN has the inputs it needs.

---

### 5. Cross-platform CI validation

**What "done" looks like.** Every published artefact is CI-built and CI-tested on Linux, macOS, and (for the C ABI) Windows. Each platform runs the full sanity gate: `cargo test -p rgx-core`, `cargo test -p rgx-cli`, `cargo test -p rgx-capi` (including the C smoke harness), PCRE2 conformance.

**Current state.** In progress / partial. **Until 2026-05-18 hosted CI could not build at all** — `.github/workflows/ci.yml` pinned the toolchain to 1.88.0 while `Cargo.toml` MSRV is 1.95, and cargo hard-errors in that situation. This is *why* the deep-nesting `cargo test -p rgx-core` gate failure went uncaught by CI for ~6 weeks. Fixed 2026-05-18 (pin → 1.95.0, all three jobs; see CHANGES.md). Most testing still happens on macOS (the user's primary). The Phase 1 C smoke test is cfg-gated to Linux + macOS — Windows is on the roadmap per the A9 design doc §5 but not implemented.

**Next concrete step.** Stand up a GitHub Actions matrix (Linux / macOS / Windows × stable / MSRV) for the workspace. Add the C smoke test to the Linux job once it's reachable on CI hardware.

---

### 6. Documentation completeness beyond the book

**What "done" looks like.** A user landing on the GitHub repo finds:

- A README that explains what rgx is, who it's for, and where to start.
- A LICENSE clearly stated (Apache-2.0 per `Cargo.toml`).
- A CONTRIBUTING.md or pointer to one.
- Issue / PR templates that surface the PGEN bug-reporting protocol when relevant.
- A CHANGELOG (live: `CHANGES.md`) with publication-ready entries.

**Current state.** In progress. `CHANGES.md` is the live ledger (matches the bar). README + LICENSE exist. CONTRIBUTING.md, issue/PR templates, and a publication-shaped changelog (Keep-a-Changelog or similar) have not been audited.

**Next concrete step.** Audit the GitHub-surface-facing files (README/LICENSE/CONTRIBUTING/templates) once the other items get closer to green.

---

### 7. Performance baseline established and documented

**What "done" looks like.** A published performance story exists in `book/src/internals/performance.md` (chapter already exists) with reproducible benchmark numbers, methodology, the matrix of engines compared (DFA / Pike-VM / TDFA / JIT / fallback), and an honest comparison against PCRE2 + RE2 + Rust `regex` crate on a documented set of workloads.

**Current state.** In progress. The book has a Performance chapter; bench infrastructure exists (`rgx-core/benches/`). The C4 benchmark-CI regression gate is shipped. The published comparison story has not been written up to publication standard.

**Next concrete step.** Inventory current benchmarks vs. the workloads we'd want a publication comparison on; identify gaps; commission missing benches.

---

### 8. Security / safety review

**What "done" looks like.** A documented security posture covering:

- The sandbox modes for embedded scripting (`book/src/internals/sandboxing.md` chapter exists).
- The panic-safety of the FFI boundary (already in place via `panic::catch_unwind` in `rgx-capi`).
- The safety-limit defaults (`max_steps`, `max_backtrack_frames`, `max_recursion_depth`, `max_trail_entries`) and their rationale.
- A `SECURITY.md` or equivalent describing how to report vulnerabilities.

**Current state.** In progress. Sandboxing chapter exists. FFI panic catching is implemented. Safety limits are implemented and exposed. `SECURITY.md` has not been audited.

**Next concrete step.** Confirm `SECURITY.md` exists and is current; add fuzz-testing of the C ABI surface (Phase 1+) as part of the readiness work.

---

## How to update this document

- **Adding a criterion.** When the user identifies a new gate, add a numbered section with the same structure: *What "done" looks like* → *Current state* → *Next concrete step*. Bump the introduction sentence count if needed.
- **Updating status.** When a criterion's state changes, edit the *Current state* paragraph. Do NOT delete history — the prior state is implicit in the git log.
- **Closing a criterion.** Set the *Current state* to **Complete** with a one-line summary of how it was closed and when. Keep the criterion in the document — historical green items document what was checked.
- **This is not a backlog.** Per-engine-feature work belongs in `docs/BACKLOG.md`. This document tracks the cross-cutting gates only.
