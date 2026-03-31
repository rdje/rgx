Downstream host project: RGX

Default parser path:
- `rgx-core/src/parsing.rs`
- active default build uses the pinned `subs/pgen` backend

RGX-side motivating pattern:
- `\A(?<word>a(?(R&word)b|c)(?&word)?d)\z`

Observed host-side symptom:
- `Regex::compile(...)` fails before RGX runtime/compiler handling with:
  - `Compile("E_PARSE_FAILURE: generated regex parse failed: Parser did not consume full input at position 0")`

Reduced parser-only reproducer used for the PGEN issue bundle:
- `(?(R&word)a|b)`

Control sample that still parses on the same PGEN backend:
- `(?(R1)a|b)`
