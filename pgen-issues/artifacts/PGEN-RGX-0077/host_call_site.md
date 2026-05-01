# Host Call Site — PGEN-RGX-0077

This note documents how the RGX host reaches PGEN's regex parser
for the family-wide reproduction of PGEN-RGX-0077. Per
`PGEN_PARSER_ISSUE_REPORTING_PROTOCOL.md` "Recommended Artifact
Names", `host_call_site.md` describes the host call surface so
PGEN reviewers can rebuild the call graph without navigating the
downstream repository tree.

## How RGX reaches PGEN

RGX's parser adapter lives at `rgx-core/src/parsing.rs`. The
top-level call into PGEN is, in pseudocode:

```rust
// rgx-core/src/parsing.rs (paraphrased — actual code lives at the
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
  `serde_json::Value` and walked via a hybrid Json+envelope walker
  following the per-rule shapes in
  `subs/pgen/docs/regex_parser_book/`.

For this report's reproduction the additional helpers used are:

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
committed at `rgx-core/examples/pgen_quoted_run_dump.rs` in the
RGX tree. A self-contained copy is included alongside this note
as `pgen_quoted_run_dump.rs` so the bundle does not require
checking out RGX. The helper is invoked with:

```bash
cargo run --release -p rgx-core --example pgen_quoted_run_dump
```

It walks 17 reproducer patterns covering the full `\Q...\E
quantifier` family plus control cases and writes:

- `pgen_contract.json` (once)
- `pgen_parse_outcomes/<name>.json` (one per pattern)
- `pgen_ast_dumps/<name>.json` (one per pattern)
- `pgen_inputs/<name>.txt` (the exact source text, per protocol §3)

## Reproducing on PGEN's own checkout

If PGEN wants to reproduce without checking out RGX, the
following invocation against PGEN's own `parseability_probe`
binary captures the equivalent single-pattern probe. This is
the canonical bug-revealing case (PGEN-RGX-0074's documented
canonical pattern):

```bash
printf '%s' '\Qab*\E{2,}' > repro_input.txt

PGEN_TRACE_VERBOSITY=debug \
cargo run --manifest-path rust/Cargo.toml \
    --features generated_parsers \
    --bin parseability_probe -- \
    --parse-dump-ast-pretty regex repro_input.txt pgen_ast_dump.json \
    --profile regex_default \
    --trace --trace-log-file pgen_trace.log
```

The resulting `pgen_ast_dump.json` is byte-identical to the one
already attached to this report under `pgen_ast_dump.json` and
confirms the bug surface end-to-end with no RGX-side dependency.

## Where the bug surfaces in RGX's adapter

After the typed AST is deserialised into `serde_json::Value`,
RGX walks the `concatenation` array via
`PgenAstAdapter::convert_typed_concatenation`. For every element
of the array, the walker calls `convert_typed_piece(item)` which
expects a typed `{type:"piece", atom, quantifier}` object. When
the typed shape is `[[<3 pieces>]]` (one extra wrap from the
`piece_quoted_run_quantified` array not being flattened), the
walker enters the OUTER concat loop, sees one item which is an
ARRAY (not an object), and surfaces:

```text
pgen AST contract mismatch: expected typed piece object, got array
```

This is a defensive guard: the walker doesn't try to unwrap or
absorb the malformed shape — it surfaces the contract mismatch
so it can be triaged upstream. The same approach was used for
PGEN-RGX-0075 (multi-piece dropped) and PGEN-RGX-0076 (posix_class
truncated).

## Test impact in RGX's lib suite

No RGX lib test exercises the `\Q...\E quantifier` shape directly
— the legacy parser-path tests use distinct fixtures. The bug
surfaces through the PCRE2 conformance harness
(`rgx-core/tests/pcre2_conformance.rs`) at testinput1:6794
`/\Qab*\E{2,}/` plus 7 sibling cases in the same family. These
contribute to the `-16` regression observed against the
`PASS_BASELINE = 12_709` ratchet immediately after bumping PGEN
to a release containing the bug.

## Relationship to PGEN-RGX-0075

PGEN-RGX-0075 fixed the `$1`-on-Quantified codegen so multi-piece
concatenations (`"abc"`, `"hello"`) surfaced all pieces. The fix
removed the auto-peel in three codegen sites
(`generate_positional_ref`, `generate_value_extraction`,
`generate_quantified_extraction`).

PGEN-RGX-0077 looks like an adjacent issue that 0075's fix and
regression-lock test did not cover: when one element of the
`piece+` Quantified is itself the result of an annotation that
returns an array (`piece_quoted_run_quantified -> [$2**, ...]`),
the parent `[$1**]` flatten-spread does NOT spread that nested
array into the parent. Per the documented `**` semantics
("if any piece's content is itself a Sequence, unwraps one level
so the children appear inline"), the spread should unwrap the
array. Empirically it doesn't.

PGEN's 0075 regression-lock test
(`regex_parser_pgen_rgx_0075_multi_piece_concatenation_surfaces_all_pieces`
in `rust/src/embedding_api.rs:2962`) only covers simple
single-piece-each-position concatenations (`"a"`, `"ab"`, `"abc"`,
`"hello"`) — none of which exercise the `piece_quoted_run_quantified`
array-returning path. A regression-lock test for the family
covered in this report would prevent future regressions on this
exact path.
