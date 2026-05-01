# PGEN-RGX-0074 — host call site

## Where RGX consumes the bad AST

`rgx-core/src/parsing.rs` invokes `pgen::embedding_api::parse_grammar_profile_named("regex", "regex_default", pattern)` and then walks the returned AST tree to lower it into RGX's own `crate::ast::Regex` enum (function `convert_pgen_ast` and friends in the same file). The Quantified-node-with-a-quoted-block-child shape that PGEN currently emits is faithfully translated into a `Regex::Quantified { expr, quantifier }` whose `expr` is the entire literal sequence — at which point the bug is locked in. The downstream VM then matches the whole sequence as the quantifier's operand instead of the trailing character.

There is no RGX-side adapter that re-attaches the quantifier; per `CLAUDE.md` parsing issues are fixed in PGEN, not patched around in RGX.

## How the bug surfaces in tests

The PCRE2 conformance corpus exercises the family at:

- `subs/pcre2/testdata/testinput1` line 6794: `/\Qab*\E{2,}/` on subject `"ab***"` — expected match `"ab***"`, RGX returns no match.

The conformance ratchet (`cargo test -p rgx-core --test pcre2_conformance --release -- --ignored`) classifies this as a `false negative` failure. Other family members (`\Qabc\E{2}`, `\Qab*\E?`, etc.) are not in the corpus today but have been confirmed to misbehave the same way via the bundled `pgen_qe_dump` example.

## Fix-in-PGEN expected outcomes

After PGEN rebases:

1. RGX bumps the `subs/pgen` submodule to the fix commit.
2. `cargo build -p rgx-core --example pgen_qe_dump && target/debug/examples/pgen_qe_dump | rg '"Quantified"'` should show the Quantified node bound to a single trailing character node, not to a Terminal `\Q…\E` block.
3. `cargo test -p rgx-core --test pcre2_conformance --release -- --ignored --nocapture` should pass with the conformance pass-count incremented by ≥ 1 (testinput1:6794 alone). Re-baseline `PASS_BASELINE`/`FAIL_BASELINE` in `rgx-core/tests/pcre2_conformance.rs` accordingly.
4. The 1118 RGX lib tests must still all pass; in particular the existing `\Q…\E`-without-quantifier coverage is unchanged.
