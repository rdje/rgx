# Host Call Site — PGEN-RGX-0076

This note documents how the RGX host reaches PGEN's regex parser
for the family-wide reproduction of PGEN-RGX-0076. Per
`PGEN_PARSER_ISSUE_REPORTING_PROTOCOL.md` recommended artifact
names, `host_call_site.md` describes the host call surface so
PGEN reviewers can rebuild the call graph without navigating the
downstream repository tree.

## How RGX reaches PGEN

RGX's parser adapter lives at `rgx-core/src/parsing.rs`. The
top-level call into PGEN is, in pseudocode:

```rust
// rgx-core/src/parsing.rs (paraphrased — actual code is at the
// `PgenParser::parse_pattern` impl block).
let dump_outcome = pgen::embedding_api::parse_regex_default_ast_dump(
    pattern,
    &pgen::embedding_api::AstDumpOptions {
        pretty: false,
        max_ast_bytes: None,
    },
);
let dump = dump_outcome.ast_dump.expect("parse_full success");
let unified = serde_json::from_str::<serde_json::Value>(&dump.dump_json)?;
// Walk `unified` per the typed-Json contract documented in the
// regex_parser_book.
```

So RGX consumes:
- `pgen::embedding_api::parse_regex_default_ast_dump(pattern, AstDumpOptions)` (production path)
- The dump's `dump_json` string is deserialised into a
  `serde_json::Value` and walked via a hybrid Json+envelope
  walker following the per-rule shapes in
  `subs/pgen/docs/regex_parser_book/`.

For this report's reproduction, an additional helper binary is
used:

- `pgen::embedding_api::parser_embedding_api_contract()` — once,
  for the contract JSON.
- `pgen::embedding_api::parse_grammar_profile_named("regex",
  "regex_default", pattern)` — once per family member, for the
  parse outcome JSON.
- `pgen::embedding_api::parse_grammar_profile_ast_dump_named(
  "regex", "regex_default", pattern, &AstDumpOptions { pretty: true,
  max_ast_bytes: None })` — once per family member, for the
  pretty AST dump.

## The reproducer helper

The helper that produces the family-coverage artifacts is
committed at `rgx-core/examples/pgen_posix_class_dump.rs` in the
RGX tree. A self-contained copy is included alongside this note
as `pgen_posix_class_dump.rs` so the bundle does not require
checking out RGX. The helper is invoked with:

```bash
cargo run --release -p rgx-core --example pgen_posix_class_dump
```

It walks 21 reproducer patterns (every standard POSIX class name,
both negation polarities, multi-class bodies, classes mixed with
ranges/literals, classes inside `(*UCP)` pragma scope, and
classes inside `(?[ ... ])` extended classes) and writes:

- `pgen_contract.json` (once)
- `pgen_parse_outcomes/<name>.json` (one per pattern)
- `pgen_ast_dumps/<name>.json` (one per pattern)
- `pgen_inputs/<name>.txt` (the exact source text, per protocol §3)

## Reproducing on PGEN's own checkout

If PGEN wants to reproduce without checking out RGX, the
following invocation against PGEN's own `parseability_probe`
binary captures the equivalent single-pattern probe:

```bash
printf '%s' '[[:alpha:]]' > repro_input.txt

PGEN_TRACE_VERBOSITY=debug \
cargo run --manifest-path rust/Cargo.toml \
    --features generated_parsers \
    --bin parseability_probe -- \
    --parse-dump-ast-pretty regex repro_input.txt pgen_ast_dump.json \
    --profile regex_default \
    --trace --trace-log-file pgen_trace.log
```

The resulting `pgen_ast_dump.json` is byte-identical to the one
already attached to this report under `pgen_ast_dump.json` (the
single-pattern primary repro) and confirms the bug surface
end-to-end with no RGX-side dependency.

## Where the bug surfaces in RGX's adapter

After the typed AST is deserialised into `serde_json::Value`,
RGX walks the `class_body` array via
`PgenAstAdapter::convert_typed_class_item_array`. For typed
shapes `Value::String("[:")`, the dispatch returns:

```text
pgen AST contract mismatch: unrecognised class_item string: "[:"
```

This is a defensive guard: rather than silently misclassify the
truncated typed shape as a literal `[:` character pair, RGX
surfaces the contract mismatch so it can be triaged upstream.
The guard is the same shape that surfaced PGEN-RGX-0075 for
multi-piece concatenations.

## Test impact in RGX's lib suite

Two regression-pin tests fail with the same compile-error
contract mismatch:

- `tests::ucp_pragma_unicodefies_posix_classes`
- `tests::ucp_graph_includes_format_and_private_use`

Both are documented in `rgx-core/src/lib.rs` and would pass
again once PGEN ships the fix and RGX bumps the submodule pin.
