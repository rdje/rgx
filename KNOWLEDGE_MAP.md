# Knowledge Map

> **AUTO-GENERATED — DO NOT EDIT.** Regenerate with `knowledge-map/scripts/gen_knowledge_map.sh`.
> Source of truth = YAML front-matter in: `docs/knowledge docs/decisions`. Edit the fact files, never this map.
> A fact is any `.md` whose front-matter has a non-empty `answers:` list.
> **5** facts · **28** question keys.

## Questions → fact

- "are the conformance residuals bugs" -> [pcre2-conformance-ratchet](docs/knowledge/pcre2-conformance-ratchet.md) · 2026-06-15 · reverify: `grep -n PASS_BASELINE rgx-core/tests/pcre2_conformance.rs`
- "can I make a code change without a task" -> [governance-entrypoints](docs/knowledge/governance-entrypoints.md) · 2026-06-15 · reverify: `sed -n '1,12p' AGENTS.md`
- "cargo build fails couldn't read return_annotation_parser.rs" -> [rgx-build-as-submodule](docs/knowledge/rgx-build-as-submodule.md) · 2026-06-15 · reverify: `make -C . bootstrap`
- "cargo build fails missing generated parser" -> [pgen-build-regenerate](docs/knowledge/pgen-build-regenerate.md) · 2026-06-15 · reverify: `ls subs/pgen/generated/regex_parser.rs subs/pgen/generated/return_annotation_parser.rs`
- "did 0078 replace 0073" -> [pgen-sole-open-bug](docs/knowledge/pgen-sole-open-bug.md) · 2026-06-15 · reverify: `grep -l '^status: open' pgen-issues/PGEN-RGX-*.yaml`
- "downstream project cannot compile RGX" -> [rgx-build-as-submodule](docs/knowledge/rgx-build-as-submodule.md) · 2026-06-15 · reverify: `make -C . bootstrap`
- "how do I build RGX as a git submodule" -> [rgx-build-as-submodule](docs/knowledge/rgx-build-as-submodule.md) · 2026-06-15 · reverify: `make -C . bootstrap`
- "how do I build RGX" -> [rgx-build-as-submodule](docs/knowledge/rgx-build-as-submodule.md) · 2026-06-15 · reverify: `make -C . bootstrap`
- "how do I build rgx-core without PGEN" -> [rgx-build-as-submodule](docs/knowledge/rgx-build-as-submodule.md) · 2026-06-15 · reverify: `make -C . bootstrap`
- "how do I regenerate the PGEN regex parser" -> [pgen-build-regenerate](docs/knowledge/pgen-build-regenerate.md) · 2026-06-15 · reverify: `ls subs/pgen/generated/regex_parser.rs subs/pgen/generated/return_annotation_parser.rs`
- "how does memory and continuity work in RGX" -> [governance-entrypoints](docs/knowledge/governance-entrypoints.md) · 2026-06-15 · reverify: `sed -n '1,12p' AGENTS.md`
- "how is work tracked in RGX" -> [governance-entrypoints](docs/knowledge/governance-entrypoints.md) · 2026-06-15 · reverify: `sed -n '1,12p' AGENTS.md`
- "is PGEN-RGX-0073 still open" -> [pgen-sole-open-bug](docs/knowledge/pgen-sole-open-bug.md) · 2026-06-15 · reverify: `grep -l '^status: open' pgen-issues/PGEN-RGX-*.yaml`
- "subs/pgen generated files are missing" -> [pgen-build-regenerate](docs/knowledge/pgen-build-regenerate.md) · 2026-06-15 · reverify: `ls subs/pgen/generated/regex_parser.rs subs/pgen/generated/return_annotation_parser.rs`
- "what PGEN issue tracks compile-time performance" -> [pgen-sole-open-bug](docs/knowledge/pgen-sole-open-bug.md) · 2026-06-15 · reverify: `grep -l '^status: open' pgen-issues/PGEN-RGX-*.yaml`
- "what are PASS_BASELINE / FAIL_BASELINE" -> [pcre2-conformance-ratchet](docs/knowledge/pcre2-conformance-ratchet.md) · 2026-06-15 · reverify: `grep -n PASS_BASELINE rgx-core/tests/pcre2_conformance.rs`
- "what does the untracked subs/pgen status mean" -> [pgen-build-regenerate](docs/knowledge/pgen-build-regenerate.md) · 2026-06-15 · reverify: `ls subs/pgen/generated/regex_parser.rs subs/pgen/generated/return_annotation_parser.rs`
- "what is RGX's PCRE2 conformance pass count" -> [pcre2-conformance-ratchet](docs/knowledge/pcre2-conformance-ratchet.md) · 2026-06-15 · reverify: `grep -n PASS_BASELINE rgx-core/tests/pcre2_conformance.rs`
- "what is the Code-Change Doctrine" -> [governance-entrypoints](docs/knowledge/governance-entrypoints.md) · 2026-06-15 · reverify: `sed -n '1,12p' AGENTS.md`
- "what is the merge gate for parsing / VM / compiler changes" -> [pcre2-conformance-ratchet](docs/knowledge/pcre2-conformance-ratchet.md) · 2026-06-15 · reverify: `grep -n PASS_BASELINE rgx-core/tests/pcre2_conformance.rs`
- "what is the only open PGEN bug" -> [pgen-sole-open-bug](docs/knowledge/pgen-sole-open-bug.md) · 2026-06-15 · reverify: `grep -l '^status: open' pgen-issues/PGEN-RGX-*.yaml`
- "what is the simple command to build RGX and its dependencies" -> [rgx-build-as-submodule](docs/knowledge/rgx-build-as-submodule.md) · 2026-06-15 · reverify: `make -C . bootstrap`
- "what should an agent read first" -> [governance-entrypoints](docs/knowledge/governance-entrypoints.md) · 2026-06-15 · reverify: `sed -n '1,12p' AGENTS.md`
- "where is the conformance ratchet baseline" -> [pcre2-conformance-ratchet](docs/knowledge/pcre2-conformance-ratchet.md) · 2026-06-15 · reverify: `grep -n PASS_BASELINE rgx-core/tests/pcre2_conformance.rs`
- "where is the task tree / how do I pick the next task" -> [governance-entrypoints](docs/knowledge/governance-entrypoints.md) · 2026-06-15 · reverify: `sed -n '1,12p' AGENTS.md`
- "which PGEN-RGX reports are still open" -> [pgen-sole-open-bug](docs/knowledge/pgen-sole-open-bug.md) · 2026-06-15 · reverify: `grep -l '^status: open' pgen-issues/PGEN-RGX-*.yaml`
- "why are there 4 PCRE2 conformance residuals" -> [pcre2-conformance-ratchet](docs/knowledge/pcre2-conformance-ratchet.md) · 2026-06-15 · reverify: `grep -n PASS_BASELINE rgx-core/tests/pcre2_conformance.rs`
- "why does include of return_annotation_parser.rs fail" -> [pgen-build-regenerate](docs/knowledge/pgen-build-regenerate.md) · 2026-06-15 · reverify: `ls subs/pgen/generated/regex_parser.rs subs/pgen/generated/return_annotation_parser.rs`

## Facts (by id)

### governance-entrypoints
_How RGX tracks work and memory — task-trees, Code-Change Doctrine, memory layers_

- **answers:** how is work tracked in RGX | where is the task tree / how do I pick the next task | can I make a code change without a task | how does memory and continuity work in RGX | what should an agent read first | what is the Code-Change Doctrine
- **date:** 2026-06-15 · **status:** current
- **evidence:** `AGENTS.md; docs/TASK_TREE.md; MEMORY_ARCHITECTURE.md; CLAUDE.md`
- **reverify:** `sed -n '1,12p' AGENTS.md`
- **source:** [`docs/knowledge/governance-entrypoints.md`](docs/knowledge/governance-entrypoints.md)

### pcre2-conformance-ratchet
_PCRE2 conformance ratchet — baseline location, value, and the 4 by-design residuals_

- **answers:** what is RGX's PCRE2 conformance pass count | where is the conformance ratchet baseline | what are PASS_BASELINE / FAIL_BASELINE | why are there 4 PCRE2 conformance residuals | are the conformance residuals bugs | what is the merge gate for parsing / VM / compiler changes
- **date:** 2026-06-15 · **status:** current
- **evidence:** `rgx-core/tests/pcre2_conformance.rs:3408 (PASS_BASELINE=12_806 / FAIL_BASELINE=4 / PANIC_BASELINE=0 / SKIP_BASELINE=0)`
- **reverify:** `grep -n PASS_BASELINE rgx-core/tests/pcre2_conformance.rs`
- **source:** [`docs/knowledge/pcre2-conformance-ratchet.md`](docs/knowledge/pcre2-conformance-ratchet.md)

### pgen-build-regenerate
_Fresh clone / PGEN bump needs the generated parser regenerated before cargo build_

- **answers:** cargo build fails missing generated parser | subs/pgen generated files are missing | how do I regenerate the PGEN regex parser | why does include of return_annotation_parser.rs fail | what does the untracked subs/pgen status mean
- **date:** 2026-06-15 · **status:** current
- **evidence:** `README.md "Build note"; docs/decisions/0002-pgen-submodule-readonly-regenerate.md`
- **reverify:** `ls subs/pgen/generated/regex_parser.rs subs/pgen/generated/return_annotation_parser.rs`
- **source:** [`docs/knowledge/pgen-build-regenerate.md`](docs/knowledge/pgen-build-regenerate.md)

### pgen-sole-open-bug
_PGEN-RGX-0078 is the sole open PGEN bug; it replaces 0073; all others addressed_

- **answers:** which PGEN-RGX reports are still open | is PGEN-RGX-0073 still open | what is the only open PGEN bug | did 0078 replace 0073 | what PGEN issue tracks compile-time performance
- **date:** 2026-06-15 · **status:** current
- **evidence:** `pgen-issues/PGEN-RGX-0078.yaml (status open); PGEN-RGX-0073.yaml (status closed, superseded_by 0078)`
- **reverify:** `grep -l '^status: open' pgen-issues/PGEN-RGX-*.yaml`
- **source:** [`docs/knowledge/pgen-sole-open-bug.md`](docs/knowledge/pgen-sole-open-bug.md)

### rgx-build-as-submodule
_How to build RGX (incl. as a git submodule) — two working paths_

- **answers:** how do I build RGX | how do I build RGX as a git submodule | cargo build fails couldn't read return_annotation_parser.rs | how do I build rgx-core without PGEN | what is the simple command to build RGX and its dependencies | downstream project cannot compile RGX
- **date:** 2026-06-15 · **status:** current
- **evidence:** `docs/INTEGRATION.md (downstream consumer guide); Makefile (root targets build/bootstrap/...); README "Build, test, run`
- **reverify:** `make -C . bootstrap`
- **source:** [`docs/knowledge/rgx-build-as-submodule.md`](docs/knowledge/rgx-build-as-submodule.md)
