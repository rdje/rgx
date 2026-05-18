# Host call site (RGX)

RGX consumes PGEN via `pgen::embedding_api::parse_grammar_profile_named("regex","regex_default", pattern)`
in `rgx-core/src/parsing.rs` (typed-AST walker `convert_typed_*`). RGX performs **no** `\NN`
backref/octal special-casing — it faithfully lowers PGEN's emitted atoms. The PCRE2 differential
conformance harness (`rgx-core/tests/pcre2_conformance.rs`, `cargo test --test pcre2_conformance --
--ignored`) is the oracle.

Discovered while verifying the combined PGEN release `65b845f0` (rel 1.1.77 / contract 1.1.79,
which bundles the 0084/0085/0086 fixes). 0085 + 0086 verified clean RGX-side; **0084's fix
(rel 1.1.76) is incomplete** — see this report's `partial_fix_and_regression`.
