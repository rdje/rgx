# PCRE2 Compatibility

rgx runs PCRE2 10.47's `testdata` corpus at **12,806 / 4 / 0 / 0** (pass / fail / panic / skip) — a gate-enforced number, not an estimate. The 4 residuals are individually documented and by-design (see [PCRE2 Conformance Residual](../internals/pcre2-conformance-residual.md)). Separately, a hand-maintained feature-area matrix (`docs/PCRE2_COMPATIBILITY_MATRIX.md`) tracks parity at roughly 98% of feature families — that figure is an estimate, the conformance count above is the measured fact. If you're migrating from PCRE2-based tools (`grep -P`, PHP, Perl), most patterns will work unchanged.

## What works

The following PCRE2 features are fully supported:

- **Character classes**: `\d`, `\w`, `\s`, `\h`, `\v`, `\R`, `\X`, `\N` and their negations
- **Unicode properties**: `\p{L}`, `\p{Lu}`, `\p{Greek}`, `\P{N}`, etc.
- **POSIX classes**: `[:alpha:]`, `[:digit:]`, etc.
- **Quantifiers**: greedy, lazy, possessive
- **Anchors**: `^`, `$`, `\A`, `\Z`, `\z`, `\b`, `\B`
- **Groups**: capturing, named (`(?P<name>...)`), non-capturing, atomic
- **Backreferences**: `\1`, `(?P=name)`
- **Alternation**: with branch number tracking
- **Lookaround**: positive/negative lookahead and lookbehind
- **Inline flags**: `(?i)`, `(?m)`, `(?s)`, `(?x)`, `(?U)`, scoped flags `(?i:...)`
- **Conditional patterns**: `(?(1)yes|no)`, `(?(name)yes|no)`
- **Special sequences**: `\K` (match reset), `\G` (end of previous match)
- **Extended grapheme clusters**: `\X`
- **Newline sequences**: `\R` (any newline)
- **Hex and Unicode escapes**: `\xHH`, `\x{HHHH}`, `\uHHHH`, `\cX`
- **Comment groups**: `(?#comment)`
- **Atomic groups**: `(?>...)`

## Remaining gap

| Feature | Status | Notes |
|---------|--------|-------|
| JIT compilation | Not implemented | rgx uses its own VM with SIMD acceleration instead |

PCRE2's JIT compiler translates regex bytecode to native machine code. rgx takes a different approach: a custom VM with SIMD-accelerated literal scanning, a lazy DFA, a tagged DFA for captures, and an optional Cranelift JIT. The honest performance picture is workload-dependent and is *not* a blanket "competitive with PCRE2-JIT" claim: rgx is faster than PCRE2 on literal-prefix and capture-heavy `find_all` workloads, slower on word-boundary patterns, and materially slower on first-time *compile* (the parser is the dominant cost). See [Performance](../internals/performance.md) for the measured per-pattern table and [Performance Measurement Methodology](../internals/measurement-methodology.md) for how it is measured and the freshness caveats.

## Engine model: rgx is Unicode-only

PCRE2 ships as three separate libraries — 8-bit, 16-bit, and 32-bit — and the 8-bit library has a *non-UTF* mode where a "character" is a single byte (0–255). **rgx has one engine and it is Unicode / code-point throughout** (conceptually equivalent to PCRE2's 8-bit library *with* `PCRE2_UTF`, or its 16-/32-bit libraries). There is no byte-only mode, by design.

This matters in exactly one observable place: a **bare octal escape whose value exceeds `\377`** (= 0o377 = 255).

- In PCRE2's 8-bit non-UTF library, `\400`, `\666`, `\777`, … cannot denote a character (they exceed one byte) so `pcre2_compile` raises *error 151, "octal value is greater than \377 in 8-bit non-UTF-8 mode"*.
- In rgx — and in PCRE2 under `,utf` or its 16-/32-bit libraries — the same escape is a perfectly valid code point: `\666` is U+01B6. (PCRE2 reads at most three octal digits, so `\6666666666` is U+01B6 followed by the literal text `6666666`.)

So a pattern like `\777` or `[\666]` **compiles in rgx** and matches the corresponding code point — the same result you get from `pcre2test … ,utf`. If you are porting patterns that *relied on* PCRE2's 8-bit-mode rejection of octal `>\377` as a validation signal, that rejection does not occur in rgx (nor in any UTF/wide PCRE2 mode); treat octal escapes as code points. This is the only behavioural consequence of the Unicode-only engine model, it is intentional and PCRE2-`,utf`-faithful, and it is documented in depth (with the parser-side rationale) in [PCRE2 Conformance Residuals → Bucket 6 / Cluster 6A](../internals/pcre2-conformance-residual.md).

## Migration guide

### From PCRE2 patterns

Most patterns work as-is. Copy them directly:

```rust
# use rgx_core::Regex;
// These are all valid PCRE2 patterns that work in rgx
let email = Regex::compile(r"[\w.+-]+@[\w.-]+\.\w{2,}")?;
let ipv4 = Regex::compile(r"\b\d{1,3}(?:\.\d{1,3}){3}\b")?;
let date = Regex::compile(r"\d{4}-\d{2}-\d{2}(?:T\d{2}:\d{2}:\d{2}(?:\.\d+)?Z?)?")?;
let url = Regex::compile(r"https?://[\w.-]+(?:/[\w./?&=#%-]*)?")?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

### From PCRE2 flags

| PCRE2 flag | rgx equivalent |
|------------|---------------|
| `PCRE2_CASELESS` | `RegexBuilder::case_insensitive()` or `(?i)` |
| `PCRE2_MULTILINE` | `RegexBuilder::multi_line()` or `(?m)` |
| `PCRE2_DOTALL` | `RegexBuilder::dot_matches_new_line()` or `(?s)` |
| `PCRE2_EXTENDED` | `RegexBuilder::ignore_whitespace()` or `(?x)` |
| `PCRE2_UNGREEDY` | `RegexBuilder::swap_greed()` or `(?U)` |

### From pcre2 crate

```text
// Before (pcre2 crate)
let re = pcre2::bytes::Regex::new(r"(?P<name>\w+)")?;
let caps = re.captures(b"hello")?.unwrap();
let name = std::str::from_utf8(&caps["name"])?;

// After (rgx)
let re = rgx_core::Regex::compile(r"(?P<name>\w+)")?;
let caps = re.captures("hello").unwrap();
let name = &caps["name"];
```

Key differences:
- rgx works with `&str` by default (use `BytesRegex` for `&[u8]`)
- `compile` instead of `new`
- No need for `bytes::` prefix for string matching
- Captures index directly to `&str`

## Feature parity tracking

The full PCRE2 compatibility matrix is maintained in the project repository (`docs/PCRE2_COMPATIBILITY_MATRIX.md`). The measured fact is the conformance ratchet: **12,806 pass / 4 fail / 0 panic / 0 skip** against PCRE2 10.47's `testdata`. The 4 failures are *not* "JIT-specific behavior" — they are a small set of individually-adjudicated, by-design residuals (e.g. RGX being Unicode/code-point-only where an 8-bit-non-UTF PCRE2 build differs). Each one is enumerated with its rationale in [PCRE2 Conformance Residual](../internals/pcre2-conformance-residual.md). JIT is a *performance* difference, not a conformance gap.
