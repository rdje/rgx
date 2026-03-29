# PGEN regex parser integration complaint
This file records the exact integration complaints found while reviewing only the published PGEN regex integration surface.

Scope was intentionally limited to:
- `PGEN_REGEX_PARSER_INTEGRATION_CONTRACT.md`
- `rust/docs/EMBEDDING_API_CONTRACT.md`
- `rust/src/embedding_api.rs`
- `PGEN_PARSER_ISSUE_REPORTING_PROTOCOL.md`
- `PGEN_USER_GUIDE.md`
- `LIVE_ACHIEVEMENT_STATUS.md`

The advertised parser surface appears real. The 2026-03-29 `1.1.0` contract refresh addressed most of the original integration complaints:
- the regex integration contract is now versioned and date-stamped,
- the stable regex/generic/named API list is now broad enough to match the embedding contract,
- parse diagnostics now expose stable machine-localizable location fields,
- the regex AST dump now has a published schema version and stable envelope/variant encoding,
- the role of the frontend JSON artifact is now clarified,
- the gate commands are no longer written in host-specific shell-path form,
- plain `(?{...})` is now defined as opaque generic payload,
- and `lua` / `js` / `javascript` payload classes are now explicitly described as opaque source-body payloads with a published structural guarantee subset.

The items below are the remaining live caveats. They are not blockers for starting RGX integration, but they are still important contract limitations worth discussing with PGEN.

## 1. The AST-dump contract is shape-stable, but not a fully stable semantic AST contract
- `PGEN_REGEX_PARSER_INTEGRATION_CONTRACT.md` now stabilizes schema version `1` for the recursive node envelope:
  - `rule_name`
  - `span.start`
  - `span.end`
  - `content`
  - externally tagged content variants such as `Terminal`, `Sequence`, `Alternative`, and `Quantified`
- The same document also says:
  - downstream consumers that interpret specific `rule_name` values should pin a parser release version,
  - and should rerun their own AST compatibility suite on upgrade.
- Consequence: the JSON transport shape is now stable enough to consume, but the detailed rule taxonomy and construct-to-`rule_name` mapping are still not a fully frozen semantic contract across parser upgrades.

## 2. Embedded code-block payload support is now specified, but intentionally narrow
- `PGEN_REGEX_PARSER_INTEGRATION_CONTRACT.md` now says:
  - plain `(?{...})` is preserved as opaque generic payload,
  - `lua`, `js`, and `javascript` payloads are preserved as opaque source-body payloads,
  - parser-layer structural handling currently guarantees:
    - balanced braces
    - single-quoted strings
    - double-quoted strings
    - escaped characters
- The same contract also explicitly says it does not promise:
  - arbitrary valid Lua or JavaScript source acceptance beyond those structural forms,
  - JavaScript comment/template-literal shielding,
  - or Lua long-bracket shielding.
- Consequence: this is now a real, usable parser-layer code-block contract, but it is still not a claim of “arbitrary valid JS/Lua source.” Downstreams must still treat the accepted payload model as intentionally narrower than a full language parser.

## 3. `native` and `wasm` tagged code blocks are still outside the published PGEN syntax contract
- The current contract explicitly says it does not promise:
  - `native` tagged embedded code blocks
  - `wasm` tagged embedded code blocks
- Consequence: RGX can no longer treat those as merely unspecified future tag classes; for the current published PGEN contract, they remain outside scope. If RGX wants PGEN to parse `native` / `wasm` code blocks too, that still requires an explicit future contract widening.

## 4. Runtime semantics for embedded code blocks remain explicitly out of scope
- `PGEN_REGEX_PARSER_INTEGRATION_CONTRACT.md` explicitly says it does not promise runtime execution semantics for embedded code blocks such as `(?{...})`.
- `PGEN_USER_GUIDE.md` likewise says the parser contract for code blocks is about acceptance, AST transport, and diagnostics, not runtime semantics.
- Consequence: parse acceptance of an embedded code block still does not imply that RGX can execute it, that the tag maps to a specific backend, or that the embedded payload has any standardized runtime behavior.

## 5. Host-language wrapper forms remain intentionally out of scope
- `PGEN_REGEX_PARSER_INTEGRATION_CONTRACT.md` explicitly says the current parser contract does not promise dedicated host-literal wrapper parsing such as `/pattern/flags`.
- Consequence: if a downstream needs source-language wrapper parsing, it still needs a separate wrapper layer above the published regex parser contract.

## Bottom line
The published regex integration contract is now good enough to start RGX integration.

The remaining caveats are now narrower and more precise:
- AST dump stability is now strong at the envelope/transport level, but not a fully frozen semantic rule taxonomy across upgrades.
- Embedded code blocks now have a published parser-layer contract, but that contract is intentionally narrower than “arbitrary valid Lua/JavaScript.”
- `native` and `wasm` tagged code blocks are not part of the current published PGEN regex syntax contract.
- Runtime semantics for code blocks remain intentionally outside the parser contract.
- Host-language wrapper parsing is still outside scope.

So the remaining discussion with PGEN is no longer about whether the regex parser can be integrated at all. It is about how explicit they want the long-term AST-upgrade discipline and embedded-code-block contract to be, and whether they want to widen the published tag set beyond the current generic / `lua` / `js` / `javascript` slice.
