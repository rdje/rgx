# PGEN-RGX-0085 — Host call site & verification harness

## How RGX reaches the failing PGEN call

```
Regex::compile(pattern)                       rgx-core/src/lib.rs
  └─ Compiler::compile(pattern)               rgx-core/src/compiler.rs
       └─ parsing::parse_pattern(pattern)     rgx-core/src/parsing.rs   (feature: pgen-parser, default)
            └─ PgenParser::new().parse_pattern(pattern)
                 └─ pgen::embedding_api::parse_grammar_profile_named("regex", "regex_default", pattern)
                      └─ pgen::generated_parsers::regex::RegexParser::parse_pattern
                           └─ parse_alternation → parse_alternative → parse_concatenation
                              → parse_piece → parse_atom → parse_group
                              → parse_capturing_group → parse_pattern → …            (recurses, 1 frame-chain per `(` level)
```

The recursion is **entirely inside PGEN's generated parser**
(`pgen::generated_parsers::regex::RegexParser::*`, in
`generated/regex_parser.rs`). It has no internal depth/recursion
guard, so the recursion depth tracks the pattern's `(` nesting depth
1:1 and overflows the thread stack on deeply nested input. The
result is a hard **process abort** (`SIGSEGV` on the stack guard
page → Rust runtime prints `has overflowed its stack` →
`SIGABRT`), not a recoverable `Result::Err`.

## Minimal RGX-independent reproduction (what PGEN should run)

No RGX code is needed — call the embedding API directly:

```rust
// Deep enough to overflow even an 8 MiB main-thread stack:
let pattern = "(".repeat(200_000) + "a" + &")".repeat(200_000);
let _ = pgen::embedding_api::parse_grammar_profile_named(
    "regex", "regex_default", &pattern,
);
// Observed: process aborts (SIGSEGV → SIGABRT) before any
// ParseOutcome / ParseDiagnostic is produced.
```

Smaller deterministic repro for a typical worker-thread budget
(libtest / async runtime stacks are commonly ≤ 2–8 MiB), see
`repro_input_small.txt` (5,000 levels). The exact 200,000-level
file is `repro_input.txt`.

`parseability_probe` reproduction (PGEN checkout, per the reporting
protocol §A):

```bash
export PGEN_TRACE_VERBOSITY=debug
cargo run --manifest-path rust/Cargo.toml --features generated_parsers \
  --bin parseability_probe -- \
  --parse regex pgen-issues/artifacts/PGEN-RGX-0085/repro_input.txt \
  --profile regex_default --trace --trace-log-file pgen_trace.log
# Observed: abort (SIGSEGV → SIGABRT); pgen_trace.log truncates at
# the overflow with no diagnostic emitted.
```

## What PGEN needs to ship the fix (and test it before release)

1. **Root cause**: unbounded recursion in the generated
   recursive-descent parser; no parenthesis-nesting / recursion
   ceiling. (Contrast: PCRE2 defaults to a 250-paren nest limit and
   returns a clean compile error past it; the `regex` crate defaults
   to `nest_limit = 250`.)
2. **Fix shape (PGEN's call)**: a configurable internal nesting /
   recursion-depth ceiling that returns a clean `ParseDiagnostic`
   (with `byte_offset` / `line` / `column` at the offending `(`)
   instead of recursing until the stack guard page faults — or an
   iterative parse for the offending rule cluster.
3. **Pre-release verification gate** PGEN can adopt as the
   ledger's "validating regression/gate proof":

   ```rust
   // Must return Err(ParseDiagnostic), NOT abort the process.
   let deep = "(".repeat(200_000) + "a" + &")".repeat(200_000);
   let outcome = pgen::embedding_api::parse_grammar_profile_named(
       "regex", "regex_default", &deep);
   assert!(outcome.is_err(), "deep nesting must be a clean parse error");

   // And a within-limit pattern still parses unchanged:
   let ok = "(".repeat(64) + "a" + &")".repeat(64);
   assert!(pgen::embedding_api::parse_grammar_profile_named(
       "regex", "regex_default", &ok).is_ok());
   ```

   Run it on a **bounded-stack thread** so a regressed/guardless
   build deterministically fails instead of passing on a large main
   stack:

   ```rust
   std::thread::Builder::new()
       .stack_size(2 * 1024 * 1024)   // ≈ libtest worker budget
       .spawn(|| { /* assertions above */ })
       .unwrap().join().unwrap();
   ```

4. **Control artifact**: `pgen_parse_outcome_control.json` shows a
   depth-10 pattern parsing correctly — the parser is *correct*, it
   only lacks a recursion ceiling.

## RGX-side status (no doctrine-forbidden workaround)

Per CLAUDE.md / `feedback_no_pgen_workarounds`, RGX did **not** add
adapter code that absorbs or rewrites PGEN's AST. RGX's mitigation,
pending PGEN's fix, is:

- **Pre-PGEN input validation**: reject patterns whose paren-nesting
  exceeds a deterministic ceiling (`MAX_NESTING_DEPTH = 1000`, 4× the
  PCRE2 / `regex` ecosystem default of 250) with a clean
  `RgxError::compile(...)` **before** PGEN is invoked.
- **Stack provisioning**: run PGEN's *correct, unmodified* parse on a
  `stacker`-grown segment — exact parity with the `serde_stacker`
  treatment RGX already applies to PGEN's JSON deserialization.

PGEN's parser still owns the real fix; this report is the upstream
channel so PGEN gains its own recursion guard and RGX's pre-PGEN
ceiling becomes belt-and-suspenders rather than the sole protection.
```
