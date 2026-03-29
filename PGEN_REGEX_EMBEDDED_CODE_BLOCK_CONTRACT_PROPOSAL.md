# PGEN regex embedded code-block contract proposal
This file proposes a concrete downstream-facing contract shape for embedded code blocks in PGEN's regex parser family.

It is written from the perspective of an RGX integrator that wants a robust parser contract without forcing PGEN to over-promise runtime behavior it does not own.

## Upstream adoption status as of `PGEN_REGEX_PARSER_INTEGRATION_CONTRACT.md` `1.1.0`
The upstream `1.1.0` contract already adopted a meaningful subset of the recommendations in this file:
- plain `(?{...})` is now defined as opaque generic payload,
- `lua`, `js`, and `javascript` are now defined as opaque source-body payload classes,
- parser-layer structural guarantees are now explicitly published for:
  - balanced braces
  - single-quoted strings
  - double-quoted strings
  - escaped characters
- runtime semantics remain explicitly outside parser scope.

What remains unapplied from the stronger version of this proposal:
- no published support for a `rhai` source-body tag yet,
- no published support for `native` or `wasm` tags,
- no guarantee of arbitrary valid Lua/JavaScript source acceptance,
- no published shielding guarantee for JavaScript comments/template literals or Lua long-bracket forms,
- and no stronger typed code-block AST contract beyond preserved payload/tag meaning.

## Goal
Make embedded code-block support precise enough that downstream projects can answer all of these questions from the published contract alone:
- what syntax is accepted,
- what the parser guarantees structurally,
- what each language tag means,
- whether the payload is opaque or language-validated,
- and what is intentionally outside parser scope.

## Recommended top-level contract split
PGEN should separate embedded code-block support into two independent layers:

1. Parser-layer guarantees
- how `(?{...})` and `(?{lang:...})` are recognized,
- how payload text is delimited and preserved,
- what diagnostics exist for malformed code-block syntax,
- and what AST/JSON shape is produced.

2. Runtime-layer non-guarantees or opt-in guarantees
- whether a tag maps to an executor,
- whether payload text is validated by a language frontend,
- whether the payload is executed, sandboxed, compiled, or only preserved.

Recommended rule:
- the regex parser contract should be strong about structural preservation,
- and conservative about runtime meaning unless a stronger per-tag contract is explicitly published.

## Recommended syntax surface
PGEN should publish whether all of the following are accepted:
- plain code block:
  - `(?{payload})`
- language-tagged code block:
  - `(?{lang:payload})`

The contract should then explicitly define the meaning of:
- no tag,
- known tags,
- unknown tags.

## Recommended meaning of untagged `(?{...})`
PGEN should choose exactly one of these and publish it:

1. Reject it
- simplest and least ambiguous

2. Preserve it as opaque generic code
- parser accepts it,
- AST preserves it,
- downstream runtime decides what to do

3. Assign one explicit default language
- only acceptable if that default is stable and documented

Recommended choice:
- preserve it as opaque generic code, or reject it outright
- do not leave it accepted-but-undefined

## Recommended per-tag contract classes
Not every tag should imply the same payload contract.

### Class A - Opaque source body tags
Recommended for:
- `lua`
- `js`
- `javascript`
- future `rhai`

Recommended meaning:
- parser guarantees structurally correct payload capture,
- parser preserves the tag and payload text,
- parser does not claim by itself that the payload is valid source text for that language unless a stronger contract is explicitly published,
- downstream runtime or backend may perform language validation separately.

### Class B - Reference-style tags
Recommended for:
- `native`
- `wasm`

Recommended meaning:
- payload is not arbitrary inline source code,
- payload is a stable reference format owned by the downstream runtime contract.

Examples:
- `native:callback_name`
- `native:module::function`
- `wasm:module:function`

Recommended rule:
- if a tag is reference-shaped, PGEN should say so explicitly rather than implying that arbitrary source code is valid there.

## Recommended parser robustness minimum
If PGEN accepts embedded code blocks at all, the parser should at minimum avoid premature termination on common lexical structures inside the payload.

Baseline structural guarantees should include:
- multiline payloads
- balanced braces when brace nesting is part of the payload model
- escaped characters
- single-quoted strings
- double-quoted strings
- comment forms that would otherwise contain misleading delimiters

If PGEN wants to claim that arbitrary valid source snippets are accepted for a language tag, then the contract should additionally cover that language's delimiter-relevant lexical forms.

Examples:

For JavaScript:
- template literals
- nested `${...}` inside template literals
- comment forms
- strings with escapes
- any delimiter ambiguity that could falsely terminate the code block

For Lua:
- long-bracket strings
- long-bracket comments
- quoted strings with escapes

If those forms are not guaranteed, PGEN should not claim that arbitrary valid Lua or JavaScript payloads are accepted.

## Recommended contract levels
PGEN could publish one of these levels per tag.

### Level 1 - Structural-only
Meaning:
- parser recognizes the code block,
- parser preserves tag + payload text,
- parser emits stable diagnostics for malformed code-block syntax,
- parser makes no claim that payload text is valid source code in the tagged language.

This is the safest default contract.

### Level 2 - Structural capture plus backend validation
Meaning:
- Level 1 guarantees hold,
- and the downstream-supported backend is expected to validate/compile the payload for that tag.

This is the most practical contract for:
- `lua`
- `js`
- `javascript`
- `rhai`

### Level 3 - Full parser-owned subgrammar
Meaning:
- parser itself validates the tagged payload as source in that language,
- and may expose typed sub-AST structure or stronger diagnostics.

This is the strongest and most expensive option.
PGEN should not imply Level 3 behavior unless it is intentionally shipped and versioned.

## Recommended downstream-facing tag policy
PGEN should publish something close to the following.

### `lua`
- recommended contract level: Level 2
- payload meaning: opaque Lua source body preserved by the regex parser and expected to be validated by the Lua-capable downstream runtime

### `js` / `javascript`
- recommended contract level: Level 2
- payload meaning: opaque JavaScript source body preserved by the regex parser and expected to be validated by the JavaScript-capable downstream runtime

### `rhai`
- recommended contract level: Level 2
- payload meaning: opaque Rhai source body preserved by the regex parser and expected to be validated by the Rhai-capable downstream runtime

### `native`
- recommended contract level: reference-style Class B
- payload meaning: callback or symbol reference, not arbitrary inline native source

### `wasm`
- recommended contract level: reference-style Class B
- payload meaning: module/function reference or other explicitly documented runtime selector, not arbitrary inline wasm source

## Recommended AST contract additions
PGEN should consider making the AST and JSON contract explicit for code blocks.

Recommended stable fields:
- code-block kind:
  - tagged
  - untagged
- original tag text, if any
- raw payload text exactly as captured
- optionally:
  - payload contract class
  - payload contract level

At minimum, downstreams should not have to infer from prose whether a code block is:
- generic opaque,
- source-body style,
- or reference-style.

## Recommended diagnostics contract additions
PGEN should consider adding explicit diagnostics for code-block-specific parse failures such as:
- unterminated code block
- invalid tag syntax
- unsupported tag format
- malformed reference-style payload when the parser owns that validation

Even if the overall stable code stays `E_PARSE_FAILURE`, the human-readable message and structured location should clearly indicate that the failure occurred inside embedded code-block syntax rather than generic regex syntax.

## Proposed concise normative wording
PGEN could adopt wording close to this:

- The regex parser structurally supports embedded code blocks through `(?{...})` and `(?{lang:...})`.
- Structural support means the parser recognizes code-block syntax, preserves the payload text, and reports malformed code-block syntax with normal parser diagnostics.
- Unless explicitly stated otherwise for a specific language tag, structural support does not by itself mean that the payload is validated as source code in that language.
- `lua`, `js`, `javascript`, and any adopted `rhai` payloads are preserved as opaque source bodies for downstream validation/execution.
- `native` and `wasm` payloads are reference-style payloads, not arbitrary inline source code, and their exact allowed formats are defined by the downstream runtime contract.
- Untagged `(?{...})` blocks are either rejected or preserved as opaque generic payloads; they must not remain accepted with undefined meaning.

## Why this contract shape is useful for RGX
This split lets RGX integrate PGEN without requiring PGEN to own all execution semantics.

It gives RGX a clear path:
- trust PGEN for robust code-block recognition and preservation,
- map tags to RGX runtime backends,
- keep the everyday inline-language track centered on `lua` / `js` / `javascript` and future `rhai`,
- keep `native` and `wasm` reference-shaped,
- defer heavier runtimes such as Julia/Python until later product/runtime decisions,
- and let Lua/JavaScript/Rhai validation happen in the actual execution backend.

That produces a contract that is honest, implementable, and much less likely to break on edge-case snippets than an implicit claim of "arbitrary code for every selected language."

The current upstream `1.1.0` direction is closer to this proposal than the earlier contract was; the main remaining gap for RGX is whether PGEN eventually wants to publish `rhai` alongside `lua` / `js` / `javascript`, whether it wants to publish `native` / `wasm` tag support, and whether it wants to widen the structural shielding guarantees for source-body tags.
