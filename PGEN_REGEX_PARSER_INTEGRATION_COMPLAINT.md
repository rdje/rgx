# PGEN regex parser integration complaint
This file records the exact integration complaints found while reviewing only the published PGEN regex integration surface.

Scope was intentionally limited to:
- `PGEN_REGEX_PARSER_INTEGRATION_CONTRACT.md`
- `rust/docs/EMBEDDING_API_CONTRACT.md`
- `PGEN_PARSER_ISSUE_REPORTING_PROTOCOL.md`
- `LIVE_ACHIEVEMENT_STATUS.md`

The advertised parser surface appears real. The complaints below are about missing, incomplete, or not-usable parts of the contract as written.

## 1. The regex integration contract has no contract version or update stamp
- `rust/docs/EMBEDDING_API_CONTRACT.md` exposes `EMBEDDING_API_VERSION = "1.1.0"` and `EMBEDDING_API_SCHEMA_VERSION = 1`.
- `PGEN_REGEX_PARSER_INTEGRATION_CONTRACT.md` has no corresponding version field, schema version, or last-updated field.
- Consequence: a downstream consumer cannot say "I integrated regex contract version X" or file a bug against an exact downstream contract revision.

## 2. Generated-backend enablement is not documented, only detection is
- `PGEN_REGEX_PARSER_INTEGRATION_CONTRACT.md` says downstreams should require the generated regex backend and inspect `parser_embedding_api_contract().supports_regex_generated_backend`.
- `rust/docs/EMBEDDING_API_CONTRACT.md` defines `E_BACKEND_UNAVAILABLE` as "generated backend requested without `generated_parsers` feature".
- Missing contract detail: the regex integration contract never states which build configuration or feature selection is required to make `supports_regex_generated_backend=true`.
- Consequence: the document tells downstreams how to detect failure, but not how to satisfy the requirement.

## 3. The published stable API list is incomplete relative to the published embedding contract
- `PGEN_REGEX_PARSER_INTEGRATION_CONTRACT.md` lists only these generic regex-family entry points:
  - `parse_grammar_profile(...)`
  - `parse_grammar_profile_result(...)`
  - `parse_grammar_profile_ast_dump(...)`
- `rust/docs/EMBEDDING_API_CONTRACT.md` also publishes:
  - `parse_grammar_profile_with_limits(...)`
  - `parse_grammar_profile_with_limits_result(...)`
  - `parse_grammar_profile_ast_dump_result(...)`
  - `parse_grammar_profile_ast_dump_with_limits(...)`
  - `parse_grammar_profile_ast_dump_with_limits_result(...)`
  - named string APIs such as `parse_grammar_profile_named(...)` and `parse_grammar_profile_ast_dump_named(...)`
- `PGEN_PARSER_ISSUE_REPORTING_PROTOCOL.md` explicitly recommends the named APIs for embedded-only downstream repro bundles.
- Consequence: a downstream that reads the regex integration contract first cannot tell whether those additional generic and named APIs are part of the intended stable regex integration surface or merely generic extras.

## 4. The AST-dump promise is transport-stable, not schema-stable
- `PGEN_REGEX_PARSER_INTEGRATION_CONTRACT.md` says the AST dump surface is stable as a JSON payload contract and that truncation behavior is stable.
- The same document also says it does not promise a stable internal Rust AST node schema.
- `rust/docs/EMBEDDING_API_CONTRACT.md` defines `AstDumpPayload` only as canonical JSON bytes plus truncation metadata.
- Missing contract detail: there is no stable regex AST JSON schema, node taxonomy, field set, or construct-to-JSON mapping for `parse_regex_default_ast_dump(...)`, `parse_grammar_profile_ast_dump(...)`, or `generated/regex.json`.
- Consequence: a downstream can reliably transport JSON output, but cannot safely build a machine consumer of regex AST structure from the published contract alone.

## 5. Parse failure diagnostics are not machine-localizable through the stable API
- `rust/docs/EMBEDDING_API_CONTRACT.md` defines `ParseDiagnostic` as stable `code` plus human-readable `message`.
- `PGEN_REGEX_PARSER_INTEGRATION_CONTRACT.md` tells downstreams to treat `E_PARSE_FAILURE` as a first-class expected mode.
- Missing contract detail: no stable failure position, line, column, span, offending rule, or structured parser-location payload is defined on the regex integration surface.
- Consequence: if a downstream needs exact failure location, it must scrape human text instead of consuming a stable machine field.

## 6. `generated/regex.json` is listed as a source-of-truth artifact, but its downstream role is undefined
- `PGEN_REGEX_PARSER_INTEGRATION_CONTRACT.md` lists `generated/regex.json` under "Tracked frontend JSON artifact".
- The document never says whether downstreams should consume it, diff it, vendor it, or ignore it.
- No separate schema/stability statement is given for that file.
- Consequence: the file is named as part of the contract surface, but not explained in a way that is actionable for a downstream integrator.

## 7. The human-readable grammar scope summary is too coarse for the job the document assigns to itself
- The document says it is what downstream projects should read first when deciding how to embed the regex parser.
- Its "Current Grammar Scope Notes" only enumerate broad families such as alternation, groups, quantifiers, anchors, lookarounds, named groups, inline modifiers, conditionals, and embedded code-block syntax.
- Missing contract detail: there is no stable capability manifest or exclusions list for subforms inside those families.
- Consequence: the document is not sufficient by itself to decide dialect fit; a downstream still has to inspect `grammars/regex.ebnf` directly.

## 8. The gate commands are not portable as written
- `PGEN_REGEX_PARSER_INTEGRATION_CONTRACT.md` uses:
  - `make -C rust SHELL=/bin/bash ...`
  - `make -C rust SHELL=/opt/homebrew/bin/bash ...`
- `/opt/homebrew/bin/bash` is a host-specific path.
- Consequence: the "Validation / Release Gates" section is not portable downstream integration guidance.

## Bottom line
The published regex integration contract is real and the advertised embedding surface is not fabricated. The problems are contract-quality problems:
- missing versioned downstream contract identity,
- incomplete API guidance,
- missing machine-usable AST and error-location structure,
- undefined role for `generated/regex.json`,
- and non-portable gate instructions.

That means the document is usable for high-level confidence and very basic accept/reject integration, but it is still incomplete as a precise downstream integration contract.
