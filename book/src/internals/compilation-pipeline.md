# Compilation Pipeline

This chapter follows a pattern from the moment you hand it to `Regex::compile` to the moment it becomes runnable bytecode. If the previous chapter gave you the map, this one walks the road.

The compilation pipeline has four stages. Each stage has a clear input, a clear output, and its own tests. None of them touch the input text — compilation is entirely about **turning a pattern string into a plan for matching**. The match itself happens later, in the VM.

```text
        pattern string
              │
              ▼
   ┌─────────────────────┐
   │  Stage 1: Parse     │   PGEN (or legacy recursive-descent)
   │     ↓               │
   │   AST               │   ast.rs
   └─────────┬───────────┘
             │
             ▼
   ┌─────────────────────┐
   │  Stage 2: Normalize │   flag lowering, class expansion
   │     ↓               │   conditional resolution
   │   Normalized AST    │
   └─────────┬───────────┘
             │
             ▼
   ┌─────────────────────┐
   │  Stage 3: Emit      │   compiler.rs / OptimizingCompiler
   │     ↓               │
   │   Bytecode Program  │   pattern.rs
   └─────────┬───────────┘
             │
             ▼
   ┌─────────────────────┐
   │  Stage 4: Optimize  │   prefix filter extraction
   │     ↓               │   literal fast path detection
   │   Optimized Program │   scanning strategy selection
   └─────────────────────┘
```

Every stage is a pure function over its input. That makes the whole pipeline reproducible: given the same pattern, you get the same bytecode, every time. It also makes each stage independently testable. When parity tests catch a mismatch with PCRE2, we can localize the bug to a single stage because we can dump the intermediate form and compare.

## Stage 1: Parsing

The default parser backend is **PGEN**, consumed via the `pgen-parser` Cargo feature and the `subs/pgen` submodule. When you call `Regex::compile("(\\d+)")`, the call flows into `parsing.rs`, which asks PGEN to parse the pattern.

PGEN's output is a grammar-level parse tree. That tree is not RGX's internal AST — it carries grammar-specific node kinds, source positions, and metadata that PGEN uses for error reporting. RGX's `parsing.rs` contains an **adapter** that walks PGEN's tree and builds an `ast::Expr` in RGX's own vocabulary.

RGX's AST is intentionally small. A pattern like `(\d+)` becomes something shaped like this:

```text
Expr::Group {
    index: 1,
    name: None,
    inner: Box::new(Expr::Plus {
        greedy: true,
        inner: Box::new(Expr::Class(CharClass::Digit)),
    }),
}
```

Every regex construct in the RGX codebase has a node type in `ast.rs`. Lookarounds, conditionals, subroutines, code blocks, extended char classes — they all live here. If a feature cannot be expressed in the AST, it does not exist in RGX.

The recursive-descent parser in `parser.rs` is kept as a fallback. It is older than PGEN, less complete, and has a single-constant local switch to enable it when debugging a parser divergence. In normal builds it is compiled but unused.

## Stage 2: AST normalization

The raw AST from Stage 1 is a faithful representation of the source syntax. Before the compiler can emit bytecode, a handful of transformations simplify it.

**Flag toggles.** Inline flags like `(?i)`, `(?m)`, `(?s)`, `(?x)` are resolved into per-node modes. The compiler does not want to track a global "are we currently case-insensitive" state while it walks the tree — every node carries its own flags, computed once, during normalization.

**Extended char class lowering.** A pattern like `(?[[a-z] & [aeiou]])` arrives from the parser as an extended-class expression with set operators. Normalization walks the expression, resolves the set algebra at compile time, and produces a plain `CharClass` with the final character set. This is why extended classes have zero runtime cost — they are compiled away.

**Conditional resolution.** `(?(1)yes|no)` is resolved into a conditional node that references capture group 1. `(?(+1)yes|no)` becomes `(?(2)yes|no)` after relative-to-absolute conversion. `(?(VERSION>=10.40)...)` (not yet shipped) would be resolved at compile time to just the matching branch. All of this happens in normalization.

**Alternation flattening.** `a|(b|c)|d` becomes a flat `Alt([a, b, c, d])`. The VM's split logic is much simpler when alternations are flat lists.

**Capture numbering.** Numbered captures are assigned in left-to-right source order. Branch-reset groups `(?|...)` reuse the same capture numbers across branches, and that renumbering happens here so later passes see the final numbers.

After normalization, every AST node carries exactly the information the compiler needs to emit bytecode. No further tree rewrites happen after this point.

## Stage 3: Bytecode emission

The compiler in `compiler.rs` (and its optimizing sibling in `vm.rs::OptimizingCompiler`) walks the normalized AST and emits a sequence of opcodes. The bytecode format lives in `pattern.rs` as the `Program` type.

RGX's opcodes look like the classic Rob Pike VM opcodes, extended with RGX-specific operations:

| Opcode family | Examples | Purpose |
|---------------|----------|---------|
| Atoms | `Char(b)`, `Class(ix)`, `Any`, `AnyDot` | Match a single byte/class/character |
| Anchors | `BeginText`, `EndText`, `WordBoundary` | Assert position, no consumption |
| Control flow | `Jump(target)`, `Split(t1, t2)`, `Match` | Sequencing and branching |
| Captures | `SaveStart(g)`, `SaveEnd(g)` | Record capture boundaries |
| Backreferences | `Backref(g)` | Re-match a captured group |
| Quantifiers | `Repeat(...)`, counted variants | Handle bounded loops efficiently |
| Lookarounds | `Lookahead(prog)`, `Lookbehind(prog, len)` | Run nested programs without consuming |
| Code blocks | `CallCode(id)` | Invoke a registered host callback |
| Subroutines | `Call(target)`, `Return` | Recursion and named subroutine calls |
| Verbs | `Commit`, `Prune`, `Skip`, `Fail`, `Accept` | Backtracking control verbs |

Compilation is straightforward recursive traversal. Each AST node emits a sequence of opcodes, often with backpatching for forward jumps. A classic alternation `a|b` compiles to:

```text
   0: Split 3, 5
   3: Char 'a'
   4: Jump 6
   5: Char 'b'
   6: Match
```

Let's look at something more interesting. The pattern `\d+` — "one or more digits" — compiles into roughly this:

```text
   0: SaveStart 0            ; begin capture group 0 (whole match)
   1: Class <digit>          ; must match at least one digit
   2: Split 1, 4             ; try more digits, else exit
   4: SaveEnd 0              ; end capture group 0
   5: Match
```

The `Split` at position 2 is what makes the `+` greedy: the first target (back to position 1) tries another digit, the second target (forward to position 4) exits the loop. Greedy splits list the "keep matching" target first; lazy quantifiers (`+?`) list the "stop matching" target first. That single ordering change is the difference between greedy and lazy.

Possessive quantifiers (`++`, `*+`, `?+`) get compiled through an atomic-group wrapper: after the loop exits, an `Accept`-like opcode prevents backtracking into the loop body. This is why possessive quantifiers are faster — they guarantee the VM will never revisit those positions.

### Compact vs inline quantifier codegen

The `\d+` pseudo-bytecode above is idealised. In practice RGX picks between two codegen strategies for each `X?` / `X+` / `X*`:

- **Compact subexpr opcodes** (`QuestionGreedy`, `PlusGreedy`, `StarGreedy`, and their `Lazy` siblings). A single opcode carries a length-prefixed body buffer. The VM steps through the body in a local frame-stack that is torn down when the iteration returns — one frame per quantifier, O(1) memory regardless of input size. Perfect for simple bodies like `\d+`, `a+`, `.*?`, `[abc]?`.
- **Inline Split-based loops** (Thompson-style). When the body contains an alternation or an inner quantifier, the local frame-stack would lose the per-iteration branch state, so backtracking into the inner alternation from outside the loop would fail. For those bodies the compiler inlines the loop:
  ```text
    <body>                 ; mandatory first iteration (X+ only)
    LOOP:
      Split EXIT           ; skip-branch pushed to the global backtrack stack
      <body>
      Jump LOOP            ; signed i16 back-edge
    EXIT:
  ```
  Splits pushed inside `<body>` land on the same stack as the loop's skip-branch, so `(?:a+|ab)+c` on `"aabc"` can still retry the `ab` alternative after the first iteration greedily took `aa` and the trailing `c` failed.

The dispatch is AST-driven: a predicate scans the body for `Alternation` or a nested `Quantified` (non-Atomic groups are transparent; atomic groups discard their frames at the end and stay compact). A nullability check avoids inlining when the body can match empty — those patterns need the compact form's runtime empty-match detection to avoid infinite zero-width loops. Possessive quantifiers remain compact even over complex bodies because the atomic wrapper discards the frames anyway.

## Stage 4: Optimization

Once the bytecode exists, the compiler runs a handful of analysis passes that attach optimization hints to the `Program`. These do not change what the program matches — they only change how the VM scans for candidate positions.

**Prefix filter extraction.** The compiler walks the bytecode looking for a **required prefix**: a set of bytes that every match must start with. For `hello|help`, the prefix is `hel`. For `[A-Z]\w+`, the prefix is "any uppercase letter." The result is stored in the `Program` as a `PrefixHint`.

At runtime, the scanning loop uses this hint to skip positions that cannot possibly match. If the hint is a literal string, the VM uses `memchr::memmem` to jump directly to candidate positions. If the hint is a character class, the VM uses a class filter to skip impossible bytes. For patterns with a strong prefix, this can be the difference between 50x and 1.5x slower than PCRE2.

**Literal fast path.** If the pattern is nothing but a literal string — no classes, no quantifiers, no anchors — the compiler marks the program as a pure literal and the scanning loop skips the VM entirely, using `memmem` directly. This is why `find_first("the quick brown fox", "quick")` is roughly **6.4x faster** than PCRE2: there is no VM involved at all on the hot path.

**Anchor detection.** If the pattern starts with `^` or `\A`, the scanning loop only tries position 0 instead of sweeping the input. If it ends with `$` or `\z`, the loop can sometimes bail out early.

**Capture count.** The compiler counts how many capture groups the pattern has and sizes the capture vector once. Runtime allocations per match are avoided by reusing the vector across `find_all` iterations.

## Putting it all together: compiling `\d+`

Let's trace `\d+` through all four stages:

**Stage 1 (parse)** produces an AST roughly like:

```text
Expr::Plus {
    greedy: true,
    inner: Box::new(Expr::Class(CharClass::Digit)),
}
```

**Stage 2 (normalize)** adds an implicit whole-match capture wrapping the expression:

```text
Expr::Group {
    index: 0,
    name: None,
    inner: Box::new(Expr::Plus {
        greedy: true,
        inner: Box::new(Expr::Class(CharClass::Digit)),
    }),
}
```

**Stage 3 (emit)** produces bytecode:

```text
   0: SaveStart 0
   1: Class <digit>
   2: Split 1, 4
   4: SaveEnd 0
   5: Match
```

**Stage 4 (optimize)** attaches hints to the `Program`:

```text
Program {
    opcodes: [...as above...],
    prefix_hint: PrefixHint::Class(CharClass::Digit),
    capture_count: 1,
    anchored: false,
    pure_literal: None,
    ...
}
```

At `find_first` time, the engine looks at `prefix_hint`, uses the digit class to skip non-digit positions, and for each candidate position invokes the VM starting at opcode 0. The VM matches one or more digits, records the capture, hits `Match`, and returns.

Every pattern goes through this pipeline. A complex pattern like `^(?<year>\d{4})-(?<month>\d{2})-(?<day>\d{2})$` traverses exactly the same four stages — it just produces more opcodes.

## Why four stages

It is tempting to collapse these into one: walk the parse tree, emit bytecode, done. Several bugs and three rewrites later, the separation is intentional.

- **Normalization** needs the full AST before it can resolve flags and conditionals. Doing this during emission creates order-dependent bugs because the compiler has not yet seen later nodes.
- **Emission** benefits from a clean, already-normalized tree where every node has final flags and numbers. The emitter is straightforward and easy to audit.
- **Optimization** is easier on finished bytecode than on an AST because the prefix analysis can walk opcodes and prove what positions are reachable.

Each stage has tests that check its output directly. When a parity test fails, the first question is "which stage produced the wrong output?" — and because each stage is a pure function, the answer is usually obvious within minutes.

## Next: the VM

Compilation gives you a `Program`. Execution is a whole other chapter. Head to [The VM](./the-vm.md) to see how that bytecode actually runs against input text.
