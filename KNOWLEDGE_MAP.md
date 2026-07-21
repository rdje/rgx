# Knowledge Map

> **AUTO-GENERATED — DO NOT EDIT.** Regenerate with `knowledge-map/scripts/gen_knowledge_map.sh`.
> Source of truth = YAML front-matter in: `docs/knowledge docs/decisions`. Edit the fact files, never this map.
> A fact is any `.md` whose front-matter has a non-empty `answers:` list.
> **8** facts · **50** question keys.

## Questions → fact

- "are the conformance residuals bugs" -> [pcre2-conformance-ratchet](docs/knowledge/pcre2-conformance-ratchet.md) · 2026-06-15 · reverify: `grep -n PASS_BASELINE rgx-core/tests/pcre2_conformance.rs`
- "can I make a code change without a task" -> [governance-entrypoints](docs/knowledge/governance-entrypoints.md) · 2026-06-15 · reverify: `sed -n '1,12p' AGENTS.md`
- "cargo build fails couldn't read return_annotation_parser.rs" -> [rgx-build-as-submodule](docs/knowledge/rgx-build-as-submodule.md) · 2026-06-15 · reverify: `make -C . bootstrap`
- "cargo build fails missing generated parser" -> [pgen-build-regenerate](docs/knowledge/pgen-build-regenerate.md) · 2026-06-15 · reverify: `ls subs/pgen/generated/regex_parser.rs subs/pgen/generated/return_annotation_parser.rs`
- "do cloned exec contexts get their own step budget" -> [vm-limits-whole-attempt](docs/knowledge/vm-limits-whole-attempt.md) · 2026-07-21 · reverify: `grep -n "step_budget_exhausted\|absorb_sub_budget\|clone_exec_context" rgx-core/src/vm.rs`
- "do set_max_steps limits cover lookbehind and lookahead bodies" -> [vm-limits-whole-attempt](docs/knowledge/vm-limits-whole-attempt.md) · 2026-07-21 · reverify: `grep -n "step_budget_exhausted\|absorb_sub_budget\|clone_exec_context" rgx-core/src/vm.rs`
- "downstream project cannot compile RGX" -> [rgx-build-as-submodule](docs/knowledge/rgx-build-as-submodule.md) · 2026-06-15 · reverify: `make -C . bootstrap`
- "how are RGX's rules enforced" -> [doctrine-enforcement](docs/knowledge/doctrine-enforcement.md) · 2026-07-21 · reverify: `./scripts/check_doctrines.sh --scope ci`
- "how do I add a new doctrine or rule check" -> [doctrine-enforcement](docs/knowledge/doctrine-enforcement.md) · 2026-07-21 · reverify: `./scripts/check_doctrines.sh --scope ci`
- "how do I build RGX as a git submodule" -> [rgx-build-as-submodule](docs/knowledge/rgx-build-as-submodule.md) · 2026-06-15 · reverify: `make -C . bootstrap`
- "how do I build RGX" -> [rgx-build-as-submodule](docs/knowledge/rgx-build-as-submodule.md) · 2026-06-15 · reverify: `make -C . bootstrap`
- "how do I build rgx-core without PGEN" -> [rgx-build-as-submodule](docs/knowledge/rgx-build-as-submodule.md) · 2026-06-15 · reverify: `make -C . bootstrap`
- "how do I regenerate the PGEN regex parser" -> [pgen-build-regenerate](docs/knowledge/pgen-build-regenerate.md) · 2026-06-15 · reverify: `ls subs/pgen/generated/regex_parser.rs subs/pgen/generated/return_annotation_parser.rs`
- "how does memory and continuity work in RGX" -> [governance-entrypoints](docs/knowledge/governance-entrypoints.md) · 2026-06-15 · reverify: `sed -n '1,12p' AGENTS.md`
- "how fast is PGEN parse vs PCRE2 compile" -> [pgen-1104-verified-blocked](docs/knowledge/pgen-1104-verified-blocked.md) · 2026-07-21 · reverify: `cargo run --release -p rgx-core --example pgen_compile_perf_dump --features pgen-parser`
- "how is work tracked in RGX" -> [governance-entrypoints](docs/knowledge/governance-entrypoints.md) · 2026-06-15 · reverify: `sed -n '1,12p' AGENTS.md`
- "how many PGEN bugs has RGX filed" -> [pgen-sole-open-bug](docs/knowledge/pgen-sole-open-bug.md) · 2026-07-21 · reverify: `grep -l '^status: open' pgen-issues/PGEN-RGX-*.yaml   # must print nothing`
- "how many execution dispatch loops does the VM have" -> [vm-limits-whole-attempt](docs/knowledge/vm-limits-whole-attempt.md) · 2026-07-21 · reverify: `grep -n "step_budget_exhausted\|absorb_sub_budget\|clone_exec_context" rgx-core/src/vm.rs`
- "how many steps does a recursive palindrome pattern need" -> [vm-limits-whole-attempt](docs/knowledge/vm-limits-whole-attempt.md) · 2026-07-21 · reverify: `grep -n "step_budget_exhausted\|absorb_sub_budget\|clone_exec_context" rgx-core/src/vm.rs`
- "is PGEN regex parsing fast now" -> [pgen-1104-verified-blocked](docs/knowledge/pgen-1104-verified-blocked.md) · 2026-07-21 · reverify: `cargo run --release -p rgx-core --example pgen_compile_perf_dump --features pgen-parser`
- "is PGEN-RGX-0078 still open" -> [pgen-sole-open-bug](docs/knowledge/pgen-sole-open-bug.md) · 2026-07-21 · reverify: `grep -l '^status: open' pgen-issues/PGEN-RGX-*.yaml   # must print nothing`
- "should RGX optimize its own compile path" -> [pgen-1104-verified-blocked](docs/knowledge/pgen-1104-verified-blocked.md) · 2026-07-21 · reverify: `cargo run --release -p rgx-core --example pgen_compile_perf_dump --features pgen-parser`
- "subs/pgen generated files are missing" -> [pgen-build-regenerate](docs/knowledge/pgen-build-regenerate.md) · 2026-06-15 · reverify: `ls subs/pgen/generated/regex_parser.rs subs/pgen/generated/return_annotation_parser.rs`
- "what PGEN issue tracks compile-time performance" -> [pgen-sole-open-bug](docs/knowledge/pgen-sole-open-bug.md) · 2026-07-21 · reverify: `grep -l '^status: open' pgen-issues/PGEN-RGX-*.yaml   # must print nothing`
- "what are PASS_BASELINE / FAIL_BASELINE" -> [pcre2-conformance-ratchet](docs/knowledge/pcre2-conformance-ratchet.md) · 2026-06-15 · reverify: `grep -n PASS_BASELINE rgx-core/tests/pcre2_conformance.rs`
- "what does adopting the fast PGEN pin require" -> [pgen-1104-verified-blocked](docs/knowledge/pgen-1104-verified-blocked.md) · 2026-07-21 · reverify: `cargo run --release -p rgx-core --example pgen_compile_perf_dump --features pgen-parser`
- "what does the pre-commit hook run" -> [doctrine-enforcement](docs/knowledge/doctrine-enforcement.md) · 2026-07-21 · reverify: `./scripts/check_doctrines.sh --scope ci`
- "what does the untracked subs/pgen status mean" -> [pgen-build-regenerate](docs/knowledge/pgen-build-regenerate.md) · 2026-06-15 · reverify: `ls subs/pgen/generated/regex_parser.rs subs/pgen/generated/return_annotation_parser.rs`
- "what dominates Regex::compile time" -> [pgen-1104-verified-blocked](docs/knowledge/pgen-1104-verified-blocked.md) · 2026-07-21 · reverify: `cargo run --release -p rgx-core --example pgen_compile_perf_dump --features pgen-parser`
- "what happened to the PGEN speed campaign" -> [pgen-1104-verified-blocked](docs/knowledge/pgen-1104-verified-blocked.md) · 2026-07-21 · reverify: `cargo run --release -p rgx-core --example pgen_compile_perf_dump --features pgen-parser`
- "what is RGX's PCRE2 conformance pass count" -> [pcre2-conformance-ratchet](docs/knowledge/pcre2-conformance-ratchet.md) · 2026-06-15 · reverify: `grep -n PASS_BASELINE rgx-core/tests/pcre2_conformance.rs`
- "what is absorb_sub_budget" -> [vm-limits-whole-attempt](docs/knowledge/vm-limits-whole-attempt.md) · 2026-07-21 · reverify: `grep -n "step_budget_exhausted\|absorb_sub_budget\|clone_exec_context" rgx-core/src/vm.rs`
- "what is the Code-Change Doctrine" -> [governance-entrypoints](docs/knowledge/governance-entrypoints.md) · 2026-06-15 · reverify: `sed -n '1,12p' AGENTS.md`
- "what is the doctrine enforcement architecture" -> [doctrine-enforcement](docs/knowledge/doctrine-enforcement.md) · 2026-07-21 · reverify: `./scripts/check_doctrines.sh --scope ci`
- "what is the merge gate for parsing / VM / compiler changes" -> [pcre2-conformance-ratchet](docs/knowledge/pcre2-conformance-ratchet.md) · 2026-06-15 · reverify: `grep -n PASS_BASELINE rgx-core/tests/pcre2_conformance.rs`
- "what is the only open PGEN bug" -> [pgen-sole-open-bug](docs/knowledge/pgen-sole-open-bug.md) · 2026-07-21 · reverify: `grep -l '^status: open' pgen-issues/PGEN-RGX-*.yaml   # must print nothing`
- "what is the simple command to build RGX and its dependencies" -> [rgx-build-as-submodule](docs/knowledge/rgx-build-as-submodule.md) · 2026-06-15 · reverify: `make -C . bootstrap`
- "what should an agent read first" -> [governance-entrypoints](docs/knowledge/governance-entrypoints.md) · 2026-06-15 · reverify: `sed -n '1,12p' AGENTS.md`
- "what stops a code change landing without a task tree" -> [doctrine-enforcement](docs/knowledge/doctrine-enforcement.md) · 2026-07-21 · reverify: `./scripts/check_doctrines.sh --scope ci`
- "where does the VM check max_steps" -> [vm-limits-whole-attempt](docs/knowledge/vm-limits-whole-attempt.md) · 2026-07-21 · reverify: `grep -n "step_budget_exhausted\|absorb_sub_budget\|clone_exec_context" rgx-core/src/vm.rs`
- "where is the conformance ratchet baseline" -> [pcre2-conformance-ratchet](docs/knowledge/pcre2-conformance-ratchet.md) · 2026-06-15 · reverify: `grep -n PASS_BASELINE rgx-core/tests/pcre2_conformance.rs`
- "where is the task tree / how do I pick the next task" -> [governance-entrypoints](docs/knowledge/governance-entrypoints.md) · 2026-06-15 · reverify: `sed -n '1,12p' AGENTS.md`
- "which PGEN pin is shipped" -> [pgen-1104-verified-blocked](docs/knowledge/pgen-1104-verified-blocked.md) · 2026-07-21 · reverify: `cargo run --release -p rgx-core --example pgen_compile_perf_dump --features pgen-parser`
- "which PGEN-RGX reports are still open" -> [pgen-sole-open-bug](docs/knowledge/pgen-sole-open-bug.md) · 2026-07-21 · reverify: `grep -l '^status: open' pgen-issues/PGEN-RGX-*.yaml   # must print nothing`
- "why are there 4 PCRE2 conformance residuals" -> [pcre2-conformance-ratchet](docs/knowledge/pcre2-conformance-ratchet.md) · 2026-06-15 · reverify: `grep -n PASS_BASELINE rgx-core/tests/pcre2_conformance.rs`
- "why did a variable-length lookbehind hang despite set_max_steps" -> [vm-limits-whole-attempt](docs/knowledge/vm-limits-whole-attempt.md) · 2026-07-21 · reverify: `grep -n "step_budget_exhausted\|absorb_sub_budget\|clone_exec_context" rgx-core/src/vm.rs`
- "why does include of return_annotation_parser.rs fail" -> [pgen-build-regenerate](docs/knowledge/pgen-build-regenerate.md) · 2026-06-15 · reverify: `ls subs/pgen/generated/regex_parser.rs subs/pgen/generated/return_annotation_parser.rs`
- "why is the conformance harness step cap 64M" -> [vm-limits-whole-attempt](docs/knowledge/vm-limits-whole-attempt.md) · 2026-07-21 · reverify: `grep -n "step_budget_exhausted\|absorb_sub_budget\|clone_exec_context" rgx-core/src/vm.rs`
- "why is the new fast PGEN pin not adopted" -> [pgen-sole-open-bug](docs/knowledge/pgen-sole-open-bug.md) · 2026-07-21 · reverify: `grep -l '^status: open' pgen-issues/PGEN-RGX-*.yaml   # must print nothing`
- "why was my commit blocked" -> [doctrine-enforcement](docs/knowledge/doctrine-enforcement.md) · 2026-07-21 · reverify: `./scripts/check_doctrines.sh --scope ci`

## Facts (by id)

### doctrine-enforcement
_RGX doctrines are machine-enforced — scripts/check_doctrines.sh is the registry+driver_

- **answers:** how are RGX's rules enforced | what stops a code change landing without a task tree | how do I add a new doctrine or rule check | what does the pre-commit hook run | why was my commit blocked | what is the doctrine enforcement architecture
- **date:** 2026-07-21 · **status:** current
- **evidence:** `DOCTRINE_ENFORCEMENT.md §10; scripts/check_doctrines.sh (DOCTRINES array); scripts/git-hooks/pre-commit`
- **reverify:** `./scripts/check_doctrines.sh --scope ci`
- **source:** [`docs/knowledge/doctrine-enforcement.md`](docs/knowledge/doctrine-enforcement.md)

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

### pgen-1104-verified-blocked
_The fast PGEN pin is ADOPTED (1.1.106) — parse 26.3x faster; the compile bottleneck is now RGX-side_

- **answers:** is PGEN regex parsing fast now | how fast is PGEN parse vs PCRE2 compile | what dominates Regex::compile time | should RGX optimize its own compile path | what happened to the PGEN speed campaign | what does adopting the fast PGEN pin require | which PGEN pin is shipped
- **date:** 2026-07-21 · **status:** current
- **evidence:** `pgen-issues/artifacts/PGEN-RGX-0078/measurements/ratio_table_1.1.106_adopted.md; pgen_parse_p50_1.1.106_adopted.txt`
- **reverify:** `cargo run --release -p rgx-core --example pgen_compile_perf_dump --features pgen-parser`
- **source:** [`docs/knowledge/pgen-1104-verified-blocked.md`](docs/knowledge/pgen-1104-verified-blocked.md)

### pgen-build-regenerate
_Fresh clone / PGEN bump needs the generated parser regenerated before cargo build_

- **answers:** cargo build fails missing generated parser | subs/pgen generated files are missing | how do I regenerate the PGEN regex parser | why does include of return_annotation_parser.rs fail | what does the untracked subs/pgen status mean
- **date:** 2026-06-15 · **status:** current
- **evidence:** `README.md "Build note"; docs/decisions/0002-pgen-submodule-readonly-regenerate.md`
- **reverify:** `ls subs/pgen/generated/regex_parser.rs subs/pgen/generated/return_annotation_parser.rs`
- **source:** [`docs/knowledge/pgen-build-regenerate.md`](docs/knowledge/pgen-build-regenerate.md)

### pgen-sole-open-bug
_Zero open PGEN-RGX reports — all 91 (0001–0091) closed as of 2026-07-21_

- **answers:** which PGEN-RGX reports are still open | is PGEN-RGX-0078 still open | what is the only open PGEN bug | what PGEN issue tracks compile-time performance | why is the new fast PGEN pin not adopted | how many PGEN bugs has RGX filed
- **date:** 2026-07-21 · **status:** current
- **evidence:** `grep -l '^status: open' pgen-issues/PGEN-RGX-*.yaml returns NOTHING; 0089/0090/0091 closed with verification notes on pin d9d41c28`
- **reverify:** `grep -l '^status: open' pgen-issues/PGEN-RGX-*.yaml   # must print nothing`
- **source:** [`docs/knowledge/pgen-sole-open-bug.md`](docs/knowledge/pgen-sole-open-bug.md)

### rgx-build-as-submodule
_How to build RGX (incl. as a git submodule) — two working paths_

- **answers:** how do I build RGX | how do I build RGX as a git submodule | cargo build fails couldn't read return_annotation_parser.rs | how do I build rgx-core without PGEN | what is the simple command to build RGX and its dependencies | downstream project cannot compile RGX
- **date:** 2026-06-15 · **status:** current
- **evidence:** `docs/INTEGRATION.md (downstream consumer guide); Makefile (root targets build/bootstrap/...); README "Build, test, run`
- **reverify:** `make -C . bootstrap`
- **source:** [`docs/knowledge/rgx-build-as-submodule.md`](docs/knowledge/rgx-build-as-submodule.md)

### vm-limits-whole-attempt
_Safety limits are whole-attempt — every VM dispatch loop shares one budget_

- **answers:** do set_max_steps limits cover lookbehind and lookahead bodies | how many execution dispatch loops does the VM have | where does the VM check max_steps | why did a variable-length lookbehind hang despite set_max_steps | what is absorb_sub_budget | do cloned exec contexts get their own step budget | why is the conformance harness step cap 64M | how many steps does a recursive palindrome pattern need
- **date:** 2026-07-21 · **status:** current
- **evidence:** `rgx-core/src/vm.rs — step_budget_exhausted / absorb_sub_budget; loops execute_at, execute_subexpr_inner_full, execute_at_continuation; clone sites probe_subexpr, execute_assertion_subexpr, execute_lookbehind_assertion. Tests rgx-core/tests/adversarial.rs::limits_bound_*`
- **reverify:** `grep -n "step_budget_exhausted\|absorb_sub_budget\|clone_exec_context" rgx-core/src/vm.rs`
- **source:** [`docs/knowledge/vm-limits-whole-attempt.md`](docs/knowledge/vm-limits-whole-attempt.md)
