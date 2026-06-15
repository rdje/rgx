# Integrating & Building RGX — Downstream Consumer Guide

This is the **handoff/integration document** for any project that consumes RGX
(as a git submodule, a path dependency, a vendored copy, or via the C ABI) and
wants a **fully featured, fully functional RGX** — i.e. the real engine with the
**PGEN regex parser**.

If you only need the one-liner: from an RGX checkout, run **`make`**. The rest of
this document is the precise "why and how" so an integrator never has to guess.

---

## 1. TL;DR

```bash
# Standalone: build a fully-featured RGX from a fresh clone
git clone --recurse-submodules https://github.com/rdje/rgx.git
cd rgx
make                       # bootstraps the PGEN parser (once) + cargo build

# As a git submodule of YOUR project: bootstrap RGX once, then build your workspace
git submodule update --init --recursive path/to/rgx
make -C path/to/rgx bootstrap     # generates PGEN's parser (idempotent)
cargo build                       # your workspace now finds rgx-core + PGEN
```

`make` is **idempotent** — re-running it after the parser is generated is a
no-op plus a normal incremental `cargo build`.

---

## 2. What "fully featured" means

RGX's regex syntax is parsed by **PGEN** (vendored as the `subs/pgen`
submodule). The **default** build is the fully-featured engine:

- `rgx-core` default features = **`std` + `pgen-parser` + `jit`**.
  - `pgen-parser` — the PGEN-backed parser (the full PCRE2-class syntax surface). Pulls `pgen`, `serde`, `serde_json`, `serde_stacker`.
  - `jit` — the Cranelift JIT backend (default-on; ~2 MiB of deps). Opt out with `--no-default-features` + re-adding the features you want.
- A pgen-free build (`--no-default-features`) exists but uses the lighter
  **recursive-descent reference parser**, which supports **less syntax**. It is
  **not** the fully-featured engine — see §7.

So: **fully featured = the default features = needs the PGEN parser generated.**

---

## 3. Prerequisites

| Requirement | Version / note |
| --- | --- |
| Rust toolchain | **≥ 1.95** (the workspace MSRV; `cargo`, `rustc`) |
| `git` | for submodule checkout |
| `make` + a POSIX shell (`bash`) | the one-time PGEN bootstrap is a Makefile target |
| Platform | Linux or macOS (developed/tested on both) |
| Perl | **not** required — the PGEN bootstrap uses a hand-written Rust EBNF frontend |
| C compiler | only for `rgx-capi` (the C ABI) and its smoke test; not for `rgx-core` |

First builds are slow (PGEN + wasmtime/cranelift + any language runtimes
compile). Subsequent builds are incremental.

---

## 4. Submodule topology — what you actually need

RGX has two submodules under `subs/`:

| Submodule | Purpose | Needed to build/use RGX? |
| --- | --- | --- |
| `subs/pgen` | the **regex parser** (PGEN) | **YES** — required for the default (fully-featured) build |
| `subs/pcre2` | the PCRE2 `testinput` corpus for the differential conformance harness | **NO** — test-only; skip it for a build |

`make bootstrap` initializes **only `subs/pgen`** (the build dependency), not the
heavy `subs/pcre2`. If you ran a blanket `git submodule update --init
--recursive` you'll also fetch `subs/pcre2` — harmless, just slower.

---

## 5. Why a plain `cargo build` isn't enough on a cold checkout

Since PGEN commit `0ed2b2ad`, PGEN no longer tracks its **generated** parser
artifacts (`subs/pgen/generated/*` — too large to vendor). On a fresh checkout
those files are absent, so `cargo build` fails while compiling the `pgen` crate:

```
error: couldn't read subs/pgen/rust/src/../../generated/return_annotation_parser.rs
```

That generated parser must exist **before** cargo compiles `pgen`. `make
bootstrap` runs the one-time generation for you (under the hood:
`make -C subs/pgen/rust SHELL=/bin/bash regex_parser_bootstrap`). This is the
exact step the project's own CI runs on a cold tree.

> **Note on true zero-step `cargo build`:** it is not achievable from RGX alone
> — cargo compiles the `pgen` dependency *before* RGX's own build script, and
> `subs/pgen` is read-only from RGX. Making bare `cargo build` work with no
> bootstrap would require **PGEN's own crate to self-generate via its `build.rs`**
> (a PGEN-upstream change). Until then, the one-time `make bootstrap` is the
> supported flow.

---

## 6. Depending on RGX from your Cargo project

Add `rgx-core` as a path dependency pointing at the submodule:

```toml
# your-project/Cargo.toml
[dependencies]
rgx-core = { path = "path/to/rgx/rgx-core" }       # default = std + pgen-parser + jit
```

Then, **once** (and again after any `subs/pgen` bump — see §8):

```bash
make -C path/to/rgx bootstrap
```

…and your `cargo build` works. Minimal use:

```rust
use rgx_core::Regex;
let re = Regex::compile(r"\d+")?;
assert_eq!(re.find("answer is 42")?.as_str(), "42");
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Optional features (embedded code blocks)

| Feature | Enables |
| --- | --- |
| `lua` | `(?{lua:…})` code blocks (mlua) |
| `javascript` | `(?{js:…})` code blocks (rquickjs) |
| `rhai` | `(?{rhai:…})` code blocks |
| `wasm` | `(?{wasm:…})` modules (wasmtime) |
| `all-languages` | all of the above |

```toml
rgx-core = { path = "path/to/rgx/rgx-core", features = ["lua", "javascript"] }
```

---

## 7. The pgen-free alternative (`--no-default-features`)

If you want a **lighter** RGX with **no PGEN bootstrap** and without
serde/wasmtime/cranelift in your tree:

```bash
cargo build -p rgx-core --no-default-features
```

This uses the recursive-descent **reference parser** — no generated files, no
`subs/pgen` needed. **Caveat:** the reference parser supports **less syntax**
than PGEN (the default path is the full engine). Use it only if its supported
subset covers your patterns. This build is CI-guarded against bit-rot but is not
the fully-featured engine.

---

## 8. After a PGEN submodule bump

`subs/pgen` is **read-only** from RGX. After updating the pin (or on a fresh
clone), re-generate the parser and rebuild from clean binaries:

```bash
make -C path/to/rgx bootstrap     # regenerates subs/pgen/generated/*
# then rebuild; do not trust a stale target/ from before the bump
```

The expected clean git state shows only untracked content under `subs/pgen`
(`?? subs/pgen` — the regenerated `generated/*`); never stage it.

---

## 9. Verifying your build

```bash
make test                                   # runs the rgx-core suite (bootstraps first)
cargo run --bin rgx -- "cat|dog" "I have a cat"   # CLI smoke (binary is named `rgx`)
make gate                                   # the full local CI gate (the merge gate)
```

---

## 10. Non-Rust consumers — the C ABI (`rgx-capi`)

For C/C++/Go/Python/… link against the C ABI crate:

- `rgx-capi` builds `cdylib` + `staticlib` (+ `rlib`); the generated header is
  `rgx-capi/include/rgx.h` (committed).
- Build it like any other crate (it depends on `rgx-core`, so the same PGEN
  bootstrap applies): `make bootstrap` once, then `cargo build -p rgx-capi`.
- Surface today (Phase 1): compile / free / retain / is_match / find_first /
  last_error / runtime-version + stable error codes. More phases are deferred
  (see `docs/A9_LANGUAGE_BINDINGS_DESIGN.md`).

---

## 11. Troubleshooting

| Symptom | Cause → fix |
| --- | --- |
| `couldn't read .../generated/return_annotation_parser.rs` | PGEN parser not generated → `make bootstrap` (or `make`). |
| Cryptic failures right after a `subs/pgen` bump | stale generated parser / stale `target/` → `make bootstrap`, then rebuild. |
| `cannot find type CharRange` / non-exhaustive match (only with `--no-default-features`) | you're on an old RGX; the pgen-free build was fixed in `BUILD-FLOW` — update RGX. |
| Want it lighter / no PGEN | use `--no-default-features` (§7), accepting the reduced syntax. |

---

## 12. Current pins / versions

- Workspace MSRV: **Rust 1.95**.
- PGEN regex parser pin: `subs/pgen` at **`db6f8c68`** (release **1.1.81** /
  integration contract **1.1.83**).
- Binary name: **`rgx`** (the `rgx-cli` package installs a `rgx` binary).
- crates.io: **not published yet** (vendoring / submodule is the supported path
  today — see `docs/decisions/0003`).

---

## 13. Where to go next

- `README.md` — project overview + the build section.
- The RGX Book (`book/src/`) — every feature, with examples.
- `KNOWLEDGE_MAP.md` — grep it for a logged fact before re-deriving (see
  `docs/knowledge/rgx-build-as-submodule.md`).
- `pgen-issues/` + `PGEN_PARSER_ISSUE_REPORTING_PROTOCOL.md` — if you hit a
  parser correctness issue (it's fixed upstream in PGEN, not worked around in
  RGX — `docs/decisions/0001`).
