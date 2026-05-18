# PGEN-RGX-0086 evidence — stale embedding-API version constants

Same `subs/pgen` pin in both columns: `08593d05`
("Live-doc catch-up for PGEN-RGX-0081 + 0082", 2026-05-05; parent
`9e7ca180` = the 0081/0082 *fix* commit).

## A. What the handoff surface reports (stale)

`subs/pgen/rust/src/embedding_api.rs` at the pin:

```
35: pub const REGEX_PARSER_INTEGRATION_CONTRACT_VERSION: &str = "1.1.31";
38: pub const REGEX_PARSER_RELEASE_VERSION: &str = "1.1.29";
444:    regex_integration_contract_version: REGEX_PARSER_INTEGRATION_CONTRACT_VERSION.to_string(),
445:    regex_parser_release_version:       REGEX_PARSER_RELEASE_VERSION.to_string(),
```

So `parser_embedding_api_contract()` → `regex_parser_release_version
= "1.1.29"`, `regex_integration_contract_version = "1.1.31"`
(captured live in `../PGEN-RGX-0085/pgen_contract.json`).

## B. What PGEN's own authoritative ledger says (true)

`subs/pgen/docs/contracts/PGEN_RELEASED_PARSER_BUG_LEDGER.md`, rows
for the very fixes this pin is named after:

- `REGEX-0082` (PGEN-RGX-0082) — "Fixed in" column:
  **`regex parser release 1.1.75; regex integration contract 1.1.77`**
- `REGEX-0081` (PGEN-RGX-0081) — "Fixed in" column:
  **`regex parser release 1.1.75; regex integration contract 1.1.77`**

The fixes are in the pinned tree (`9e7ca180` is the pin's parent),
and the ledger labels them released in 1.1.75 / 1.1.77.

## C. The contradiction

For the identical pinned commit, PGEN's ledger says the integrated
parser is release **1.1.75 / contract 1.1.77**, while the
embedding-API contract constants report **1.1.29 / 1.1.31** — a
~46-minor-version under-report. The constants were not bumped in
lockstep with the ledger's released-version labels (last bumped at
the REGEX-0074 era, "release 1.1.29; contract 1.1.31", ledger
line ~143).

`PGEN_PARSER_ISSUE_REPORTING_PROTOCOL.md` instructs downstreams to
copy version fields from this exact handoff surface ("not guessed
from memory"). A stale handoff surface makes every downstream
version record wrong-by-construction (it nearly caused RGX to
"correct" its accurate 1.1.75 docs *down* to 1.1.29).
