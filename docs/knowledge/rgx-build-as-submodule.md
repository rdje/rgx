---
id: rgx-build-as-submodule
title: How to build RGX (incl. as a git submodule) — two working paths
answers:
  - "how do I build RGX"
  - "how do I build RGX as a git submodule"
  - "cargo build fails couldn't read return_annotation_parser.rs"
  - "how do I build rgx-core without PGEN"
  - "what is the simple command to build RGX and its dependencies"
  - "downstream project cannot compile RGX"
date: 2026-06-15
status: current
tags: [build, downstream, submodule, pgen, make]
evidence: docs/INTEGRATION.md (downstream consumer guide); Makefile (root targets build/bootstrap/...); README "Build, test, run"
reverify: make -C . bootstrap
---

RGX builds two ways; a project that git-submodules RGX has **zero-friction**
options:

1. **Default (with PGEN) — one command:** `make` (= `make build`) at the RGX
   root. It inits `subs/pgen`, runs the one-time PGEN `regex_parser_bootstrap`
   only if the generated parser is missing (idempotent), then `cargo build`. A
   submodule consumer runs `make -C path/to/rgx bootstrap` once, then their own
   `cargo build` finds PGEN's parser. (Plain `cargo build` cannot self-bootstrap
   — cargo compiles the `pgen` dep before RGX's build script and `subs/pgen` is
   read-only; true zero-step needs a PGEN-upstream `build.rs`.)
2. **Pgen-free reference build:** `cargo build -p rgx-core --no-default-features`
   uses the recursive-descent `parser.rs` reference backend — **no PGEN, no
   bootstrap, no serde/wasmtime/cranelift**. A `cargo check --no-default-features`
   step in `run-local-ci.sh` keeps it from bit-rotting. (Note: the reference
   parser supports less syntax than PGEN — the default path is the full engine.)

Full precise recipe (prerequisites, submodule topology, feature flags, the
pgen-free option, the C ABI, troubleshooting): **`docs/INTEGRATION.md`** — the
downstream consumer / handoff guide. Fixed under the `BUILD-FLOW` task-tree
(LINKEDSPEC `RGX-BUILD-REPRO`). Related: [[pgen-build-regenerate]] (why the
generated parser is missing on a cold clone).
