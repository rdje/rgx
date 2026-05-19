# PGEN Integration

Most regex engines write their own parser. PCRE2 has one, Oniguruma has one, Rust's `regex` crate has `regex-syntax`. RGX took a different bet: it consumes an external parser generator called **PGEN** as a git submodule and delegates all of regex syntax to it.

This chapter is about why we made that decision, how the integration works, and what it means day to day.

## What PGEN is

PGEN is a parser generator with its own repository, its own release cadence, and its own test suite. It is not specific to regex — it is a general-purpose tool for turning grammar specifications into parsers. RGX is one of its consumers.

PGEN ships a grammar for the regex language. That grammar describes everything PCRE2 understands: literals, classes, escapes, groups, quantifiers, lookarounds, conditionals, subroutines, code blocks, backtracking verbs, extended character classes, and so on. When PGEN is compiled, that grammar produces a parser that reads pattern strings and emits a parse tree.

RGX consumes PGEN through the `subs/pgen` git submodule, currently pinned to release **1.1.81** (integration contract 1.1.83) at commit `db6f8c68`. Fresh clones need `git clone --recurse-submodules` or a subsequent `git submodule update --init --recursive` — this is covered in the [Contributing](./contributing.md) chapter.

## Why bother with an external parser

The decision to externalize the parser was not obvious. Here is the case for it:

**1. Single source of truth for regex syntax.** Regex grammars are notoriously ambiguous. Questions like "does `{1,2}` count as a quantifier inside a character class?" have surprising answers that differ between PCRE2 versions and between PCRE2 and Perl. When the parser lives inside RGX, every one of those edge cases has to be re-discovered and re-tested every time we touch the parser. When the parser lives in PGEN, those decisions are made once, in one place, by people whose job is grammar correctness.

**2. Contract-driven development.** PGEN and RGX interact through a written contract: `PGEN_REGEX_PARSER_INTEGRATION_CONTRACT.md`. The contract specifies what RGX expects from PGEN (node shapes, error signatures, position metadata) and what PGEN promises to deliver. When either side wants to change the boundary, it proposes a contract update, we discuss, and then we ship. This forces us to think about the interface, not just the implementation.

**3. Independent evolution.** PGEN can ship a new release with a new regex syntax form, and RGX can adopt it (or not) by bumping the submodule pin. Conversely, RGX can iterate on the VM without touching the parser at all. The two codebases move at their own speed.

**4. PCRE2 catch-up is cheaper.** When PCRE2 10.47 added returned-capture subroutine forms like `(?R(grouplist))`, the syntax change was a PGEN grammar update. RGX only needed to decide whether to wire the new AST shape through the compiler. That's backlog item **A12**, and the parser half of it is basically done.

**5. RGX's parser code does not need to be world-class.** If we wrote our own parser, it would need hand-written recursive descent with careful error recovery. Instead, RGX has a thin adapter layer (`parsing.rs`, ~a few hundred lines) that translates PGEN's output into RGX's AST. The hard work of tokenization, error recovery, and ambiguity resolution lives in PGEN.

The case against this arrangement is real too: RGX takes a dependency on a separate private repository, and "hosted CI on a private repo with a private submodule" required extra plumbing. We live with that tradeoff because the correctness wins are larger than the operational cost.

## The integration contract

The contract lives at the repository root as `PGEN_REGEX_PARSER_INTEGRATION_CONTRACT.md`. It is the authoritative document for what crosses the boundary. In practice, the contract covers:

- **AST node shapes.** PGEN promises to emit nodes whose structure RGX can walk. The RGX-side `parsing.rs` adapter expects specific variants.
- **Source positions.** Every PGEN node carries `byte_offset`, `line`, and `column`. RGX uses these for compile-time error messages with caret highlighting (the B9 backlog item).
- **Error signatures.** When PGEN fails to parse a pattern, it returns a structured error with a category, a span, and a message. RGX maps these to `CompileError` with consistent wording.
- **Feature availability.** Some PGEN features are not yet wired into RGX's compiler. The contract specifies how those should be reported — either as compile errors or as successful parses that the compiler then rejects with "unsupported" diagnostics.

A companion document, `PGEN_PARSER_ISSUE_REPORTING_PROTOCOL.md`, defines the workflow when RGX finds a bug in PGEN. We do not just file a plain issue on PGEN's tracker — we write a structured report with a minimal reproducer, the expected AST, and the observed AST.

A third document, `PGEN_REGEX_PARSER_INTEGRATION_COMPLAINT.md`, is RGX's **caveat list**: things we wish PGEN did differently but have decided to work around rather than block on. This is an unusual document and worth explaining — it is the place where we admit "we know this is suboptimal, here is our workaround, and here is why we are not asking PGEN to fix it today." Keeping that list visible prevents us from pretending the boundary is perfect.

## How the adapter works

The adapter lives in `rgx-core/src/parsing.rs`. The file has a handful of responsibilities.

**Backend selection.** A compile-time constant picks between PGEN and the legacy recursive-descent parser. In normal builds the constant is `true` (PGEN). When debugging an AST-level divergence, a developer can flip that switch locally, rebuild, and see whether the problem is in the parser adapter or downstream.

```text
const USE_PGEN: bool = true;

pub fn parse(pattern: &str) -> Result<Expr, CompileError> {
    if USE_PGEN {
        parse_pgen(pattern)
    } else {
        parser::parse(pattern)  // the reference recursive-descent path
    }
}
```

**Invoking PGEN.** The adapter calls into the PGEN crate (built from the submodule) with the pattern string. PGEN returns its own parse tree or a structured error.

**Walking PGEN's tree.** A recursive function visits every node in PGEN's tree and builds the corresponding `ast::Expr` node. For straightforward nodes the translation is one-to-one; for complex nodes (extended char classes, conditionals with multiple forms) the adapter has explicit handling for each PGEN variant.

**Error translation.** PGEN errors become `CompileError` values. The adapter attaches the source position from PGEN's error to the RGX error so the caller gets consistent diagnostic output.

**Feature gating.** Some PGEN nodes represent features RGX has not yet implemented at the compiler/VM level. The adapter either rejects them with a clear "feature X is recognized but not yet supported" error, or passes them through and lets the compiler produce the rejection later. The choice depends on how far RGX wants to go before it fails: rejecting at the adapter is faster, rejecting at the compiler gives better messages.

The adapter is not glamorous code, but it is critical. Most parser regressions turn out to be adapter bugs, and the unit tests in `parsing.rs` exist specifically to protect the boundary.

## Reporting bugs back to PGEN

Any integration of two projects will produce bugs at the boundary. RGX has a dedicated directory for tracking them: `pgen-issues/`.

Inside `pgen-issues/` are YAML files named `PGEN-RGX-0001.yaml`, `PGEN-RGX-0002.yaml`, and so on. Each file contains:

- A minimal reproducer (the regex pattern and, if relevant, sample input).
- The expected AST shape (what RGX needs).
- The observed AST shape (what PGEN actually emits).
- A severity label and a workaround, if one exists on the RGX side.
- The current status — open, filed upstream, fixed, workaround shipped.

This directory is the single place to look when someone asks "is this a known problem?" It is also what we attach to PGEN upstream when we file an issue formally. As of this writing there are more than a dozen entries; several have been fixed in PGEN releases and are kept for historical record.

The `pgen-issues/artifacts/` subdirectory holds additional supporting material — longer reproducers, parse traces, and comparison output.

## Version pinning

RGX pins PGEN to a **specific commit**, not a range. The current pin is:

- Release: **PGEN 1.1.81** (integration contract 1.1.83)
- Commit: `db6f8c68`
- Submodule path: `subs/pgen`

This choice is deliberate. Regex parsing is sensitive to grammar changes, and an accidental PGEN bump between RGX releases could silently change what patterns RGX accepts. Pinning to a commit means:

- CI always builds against the exact PGEN that RGX was tested against.
- Fresh clones get the tested version.
- Upgrades are deliberate: someone runs `git submodule update --remote subs/pgen`, reviews the diff, and commits the new pin with a matching changelog entry.

When a PGEN upgrade lands, the whole RGX test suite runs against the new parser. Any divergences — differently-shaped ASTs, new or missing errors, new accepted syntax — are surfaced as test failures and either fixed in the adapter or filed back to PGEN.

## What you see as a user

None of this is visible when you use RGX. You call `Regex::compile("(\\d+)")`, you get a compiled regex, and the fact that a parser generator was involved is an implementation detail.

The one place it leaks through is error messages. When a pattern fails to compile, the error carries a byte offset, a line, and a column provided by PGEN. The error formatter uses those to render a caret-highlighted message:

```text
error: unexpected character in group
  --> pattern:1:5
  |
1 | (?<$name>...)
  |     ^
  |
  = note: group names must start with a letter or underscore
```

That diagnostic would not be possible without PGEN's structured error output. This is the B9 backlog item ("syntax error diagnostics with spans") and it is shipped today because the parser gives us the data we need.

## Next: how fast is it?

The parser and the VM are the mechanical parts. The question everyone asks about a regex engine is "how fast?" Head to [Performance](./performance.md) for the real numbers, the optimizations that got us there, and the things we have not done yet.
