# Architecture Overview

This chapter answers a simple question: **when you hand RGX a pattern and a piece of text, what actually happens?**

RGX is a layered system. The layers exist not because layering is fashionable, but because a regex engine has genuinely different concerns — turning characters into tokens is not the same problem as executing bytecode, and pretending otherwise leads to code that is hard to test and hard to optimize. Each layer has one job, a narrow interface to the next, and its own test suite that holds it accountable.

## The big picture

At the highest level RGX looks like this:

```text
        ┌──────────────────────────────────────────────────┐
        │                  Host Application                │
        │   (your Rust program, CLI user, future binding)  │
        └───────────────────┬──────────────────────────────┘
                            │  Regex::compile / find_first / find_all / ...
                            ▼
        ┌──────────────────────────────────────────────────┐
        │                      rgx-core                     │
        │                                                    │
        │   Pattern text                                     │
        │        │                                           │
        │        ▼                                           │
        │   ┌─────────┐   tokens   ┌─────────┐    AST        │
        │   │  Lexer  │──────────▶ │ Parser  │───────┐       │
        │   └─────────┘            └─────────┘       │       │
        │     (PGEN-backed parser path is default)   │       │
        │                                            ▼       │
        │                                       ┌──────────┐ │
        │                                       │ Compiler │ │
        │                                       └────┬─────┘ │
        │                                            │       │
        │                                       bytecode     │
        │                                            │       │
        │                                            ▼       │
        │                                       ┌──────────┐ │
        │                                  text │    VM    │ │
        │                                 ─────▶│          │ │
        │                                       └────┬─────┘ │
        │                                            │       │
        │                                      MatchResult   │
        └────────────────────────────────────────────┼───────┘
                                                     │
                                                     ▼
                                              host application
```

The pipeline is `Pattern text → Lexer → Parser → AST → Compiler → Bytecode → VM → MatchResult`. The stages are strictly ordered; nothing in the VM ever looks at the pattern string directly, and nothing in the parser ever touches input text. That separation is one of the few design rules RGX never bends.

## The crates in the workspace

RGX is a Cargo workspace with four crates. Each has a narrow mandate.

| Crate | Purpose | Depends on |
|-------|---------|------------|
| `rgx-core` | The engine itself — lexer, parser adapter, compiler, VM, public API | PGEN (via submodule), optional language runtimes |
| `rgx-cli` | The command-line binary. Argument parsing, pretty output, file mode | `rgx-core` |
| `rgx-bench` | Benchmark harnesses, PCRE2 differential parity suite, trend capture | `rgx-core`, `pcre2` |
| `rgx-wasm` | Scaffold for browser/WASM distribution | `rgx-core` |

If you are trying to understand RGX for the first time, the only crate that matters is `rgx-core`. Everything else is either a consumer (`rgx-cli`, `rgx-wasm`) or a measurement tool (`rgx-bench`).

## Where the code lives

Inside `rgx-core/src/`, the pipeline stages map directly onto files:

| Stage | File | What it does |
|-------|------|--------------|
| Lexer | `lexer.rs`, `token.rs` | Converts pattern text into tokens. Used by the legacy recursive-descent path. |
| Parser (adapter) | `parsing.rs` | Picks a parser backend (PGEN by default) and adapts its output to RGX's AST. |
| Parser (legacy) | `parser.rs` | Recursive-descent reference implementation. Kept as a fallback switch for debugging. |
| AST | `ast.rs` | The shared regex syntax tree that both parser backends produce. |
| Compiler | `compiler.rs`, `vm.rs::OptimizingCompiler` | Walks the AST and emits VM bytecode. Does prefix/literal analysis. |
| Bytecode | `pattern.rs` | The `Program` type — opcodes, capture metadata, prefix hints. |
| Backtracking VM | `vm.rs` | Interpreter that executes bytecode against input. The full-featured engine. |
| C2 hybrid | `c2/` | Sparse-set Pike-VM, lazy DFA cache, byte-class partitioning. The fast path for the no-backtracking subset. |
| Engine | `engine.rs` | Runtime dispatch (DFA → Pike-VM → backtracking VM), limits, scanning strategies. |
| Public API | `lib.rs` | `Regex`, `Captures`, `Match`, iterators — everything users actually touch. |

Around those are supporting modules: `events.rs` (Layer 4 observer API), `execution.rs` (code-block runtime and callback registration), `file.rs` (Layer 6 file-backed matching), `regex_set.rs` (multi-pattern matching), `bytes.rs` (`&[u8]` API), `cache.rs` (compilation cache), `error.rs`, and `log.rs`.

## The six host integration layers

RGX is not just a regex matcher — it is designed to be a **programmable matching engine**. That goal is realized as six stacked integration layers, each of which is optional and each of which is already shipped today.

| Layer | Name | What it adds | Key types |
|-------|------|--------------|-----------|
| 1 | Data Exchange | Host variables in, structured results out | `set_variable`, `MatchResult.code_result` |
| 2 | Predicate Callbacks | `(?{lang:code})` calls host code mid-match | `ExecResult`, `ExecutionMode` |
| 3 | Match Steering | Callbacks tell the engine how to proceed | `SteerResult`, `ExecResult::Steer` |
| 4 | Structured Events | Engine emits events during execution | `MatchEvent`, `Regex::on_event` |
| 5 | Async I/O | Callbacks can suspend and resume | `MatchContinuation`, `find_first_async` |
| 6 | File-Backed Matching | Scan files directly, tail logs | `match_file`, `scan_file`, `tail_file` |

Layer 1 is the foundation; Layer 5 is the capstone. Each layer is designed to have zero overhead when you don't use it — if you never call `on_event`, the event dispatch cost is effectively zero because observers are a `None` branch. If you never register a callback, the code-block path never runs.

These layers are what the host integration guide in Part IV of this book covers. From the VM's point of view, they are all variations on the same theme: the dispatch loop occasionally asks the host bridge "should I keep going, stop, skip, or suspend?" and the host answers through typed enums instead of side channels.

## Where PGEN fits in

PGEN is a separate project — a parser generator with its own repository and its own release cadence. RGX consumes PGEN as a git submodule pinned to a specific release (currently **1.1.10**). PGEN owns the **regex grammar** itself: the actual job of reading a pattern like `(?<year>\d{4})-\d{2}` and producing structured syntax is PGEN's responsibility.

RGX's `parsing.rs` is the adapter layer. It calls into PGEN, receives PGEN's parse tree, and translates that tree into RGX's own `ast.rs` types. The recursive-descent parser in `parser.rs` still exists but is gated behind a compile-time switch and is used only as a reference implementation when we need to debug an AST divergence.

The upshot: RGX has a single source of truth for regex syntax. When PCRE2 adds a new syntax form, the change happens in PGEN's grammar, PGEN ships a new release, RGX bumps the submodule pin, and the new syntax flows through the adapter into the existing compiler and VM. This is covered in depth in the [PGEN Integration](./pgen-integration.md) chapter.

## How a match travels through the system

Walk a single `find_first` call through the layers:

```text
 user code                  engine                  VM
─────────────              ─────────              ──────
 Regex::compile(p) ──────▶  parse(p)
                                │
                                ▼
                            AST (ast.rs)
                                │
                                ▼
                           compile(ast)
                                │
                                ▼
                           Program {opcodes,
                                    prefix_hint,
                                    groups, ...}

 re.find_first(t) ──────▶   scanning
                            strategy
                                │
                                ▼
                            ExecContext ────▶  dispatch loop
                                                    │
                                        ┌───────────┤
                                        │           │
                                   opcode step   callback?
                                        │           │
                                        └─────┬─────┘
                                              ▼
                                         MatchResult
 ◀──────────────────────────────────────  returned
```

The compile step happens once per pattern. The execution step happens once per `find_first` call. The two halves share nothing mutable — a compiled `Regex` is `Send + Sync` and cheap to clone into threads.

## Design invariants

A few rules apply everywhere in the codebase, and the test suite is built to enforce them:

1. **No stage touches a stage it is not adjacent to.** The compiler never sees tokens; the VM never sees the AST. Every hop is typed.
2. **Public behavior is validated from the public API.** Internal unit tests are fine, but a feature is not "shipped" until a test calls `Regex::compile` and checks the behavior through `find_first` or `find_all`.
3. **Declared capabilities do not count until they ship end-to-end.** An opcode that exists in `vm.rs` but is never emitted by the compiler does not mean the feature works. The capability matrix tracks what actually runs through the pipeline, not what has been scaffolded.
4. **Documentation follows verified behavior, not aspiration.** If a chapter in this book claims something works, the corresponding test exists.

These rules sound bureaucratic. In practice they are the reason the headline correctness number is a gate-enforced fact, not a slogan: RGX runs PCRE2 10.47's `testdata` at 12,806 / 4 / 0 / 0, and CI fails if that ratchet regresses. (The "~98% feature-family parity" figure quoted elsewhere is a separate, explicitly hand-maintained estimate — the conformance count is the measured one.)

## What to read next

- If you want to see how a pattern becomes bytecode, read [Compilation Pipeline](./compilation-pipeline.md).
- If you want to understand the backtracking execution model, read [The VM](./the-vm.md).
- If you want to understand the second engine that runs alongside the VM, read [The NFA/DFA Hybrid Engine](./nfa-dfa-engine.md).
- If you want the details of the parser boundary, read [PGEN Integration](./pgen-integration.md).
- If you want to see the numbers, read [Performance](./performance.md).

The rest of Part VI zooms in on each of those pieces, and the final chapter explains how to contribute.
