# HOST INTEGRATION ARCHITECTURE
Live design document for deep host-engine integration in rgx.

## Purpose
Define the architecture for making rgx a **programmable matching engine** — where the regex grammar provides structure and the host application drives intelligence through deep, well-defined bidirectional interaction.

This document is both a design reference and an implementation plan. Every layer described here is intended to be shipped with SOTA-level quality.

## Design principle
The regex engine is an execution substrate, not just a pattern matcher. The host and engine form a cooperative system where:
- The engine handles syntax, compilation, and VM execution.
- The host provides domain logic, external data, and steering decisions.
- Both sides communicate through typed, structured interfaces — never through string hacks or side channels.

## Core philosophy: regex-level power, not language-level complexity

Existing tools require you to leave the regex world to get host interaction:
- sed `s///e` shells out — you write Bash, not regex.
- Perl `(?{code})` embeds Perl — you're locked to one language.
- PCRE2 `(?C)` gives a callback number — no data, no steering, no inline code.

RGX's approach: **the pattern IS the integration surface.** You stay at the regex level and the host comes to you. The syntax `(?{native:validate})` is still a regex construct — it compiles, it participates in backtracking, it's zero-width. But it reaches into the host environment for logic that regex alone can't express.

This means:
- A Rust program registers callbacks, compiles a pattern, and matches. The pattern calls Rust functions mid-match.
- A Python program (via future bindings) does the same — the pattern calls Python functions.
- A CLI user specifies `--var` and `--wasm-module` flags — the pattern calls host-provided logic without writing any code beyond the regex.
- The host language doesn't matter. The regex is the common language.

This is sed's `s///e` and Perl's `(?{code})` **on steroids, decoupled from any specific host language, and with full bidirectional data exchange.** You write patterns, not programs. The host registers capabilities. The engine connects them.

## Architecture overview

```
Host Application
    │
    ├── registers callbacks, variables, modules
    ├── provides async resolvers
    ├── receives structured match events
    │
    ▼
┌─────────────────────────────────────────┐
│  RGX Engine                             │
│                                         │
│  Pattern ──► PGEN ──► AST ──► VM        │
│                              │          │
│              ┌───────────────┤          │
│              │ Host Bridge   │          │
│              │               │          │
│              │ • Predicates  │ ◄──────► Host callbacks
│              │ • Steering    │ ◄──────► Host decisions
│              │ • Data in/out │ ◄──────► Host variables/results
│              │ • Events      │ ──────► Host observers
│              │ • Async I/O   │ ◄──────► Host resolvers
│              └───────────────┘          │
└─────────────────────────────────────────┘
```

## The five interaction layers

Each layer builds on the previous. Layers 1-2 are shipped. Layers 3-5 are planned.

---

### Layer 1 — Data Exchange

**Status: `shipped`**

Host passes data into the engine before matching. Engine passes structured results back after matching.

**Host → Engine:**
- `Regex::set_variable(name, value)` — inject host-provided key-value pairs
- Variables are snapshotted into each code-block evaluation
- Available to Lua/JS/Rhai/native/WASM via `vars.name` / `variable(name)`

**Engine → Host:**
- `MatchResult.code_result` — last non-boolean value from winning-path code block (`Numeric(f64)` or `Replacement(String)`)
- `MatchResult.matched_branch_number` — 1-based top-level alternative ID
- `Regex::find_first_numeric_with_code(...)` — collect `Numeric` payloads
- `Regex::find_all_numeric_with_code(...)` — collect all `Numeric` payloads in match order
- `Regex::replace_first_with_code(...)` / `replace_all_with_code(...)` — consume `Replacement` payloads

**Inline-language emission helpers:**
- Lua/JS: `rgx.emit_numeric(value)`, `rgx.emit_replacement(text)`
- Rhai: `emit_numeric(value)`, `emit_replacement(text)`
- WASM: `rgx.emit_numeric(value: f64)`, `rgx.emit_replacement(ptr, len)`

---

### Layer 2 — Predicate Callbacks

**Status: `shipped`**

Engine calls host-provided code at predicate checkpoints during matching. The callback inspects current match state and returns pass/fail (or a richer result).

**Syntax:** `(?{lang:code})` where `lang` is one of:
- `lua` — sandboxed Lua 5.4 (feature-gated)
- `js` / `javascript` — sandboxed QuickJS (feature-gated)
- `rhai` — sandboxed Rhai (feature-gated)
- `native` — Rust closure registered on the compiled `Regex` (`ExecutionMode::Full` only)
- `wasm` — registered WASM module (`ExecutionMode::Safe` or `Full`, feature-gated)

**Execution context available to callbacks:**
- `arg[0]` / `current_match()` — current overall match prefix
- `arg[1]`, `arg[2]`, ... — completed numbered captures
- `named.group_name` / `named(name)` — completed named captures
- `vars.name` / `variable(name)` — host-provided variables
- `pos` — current byte position
- `match_start`, `match_end`, `match_length` — current match attempt metadata
- `branch_number` — 1-based top-level alternative ID (when applicable)
- `text` — full input text

**Return values:**
- `ExecResult::Success` / `Failure` — predicate pass/fail
- `ExecResult::Numeric(f64)` — pass with numeric payload
- `ExecResult::Replacement(String)` — pass with replacement payload

**Safety model:**
- `ExecutionMode::Pure` — rejects all code blocks
- `ExecutionMode::Safe` — sandboxed backends only (Lua, JS, Rhai, WASM)
- `ExecutionMode::Full` — all backends including native

---

### Layer 3 — Match Steering

**Status: `shipped`**

Host tells the engine HOW to proceed after a callback, not just pass/fail. This transforms callbacks from passive predicates into active match controllers.

**Proposed `SteerResult` enum:**
```rust
pub enum SteerResult {
    /// Continue matching normally from the current position.
    Continue,
    /// Fail this path and backtrack.
    Fail,
    /// Force-accept the match at the current position.
    Accept,
    /// Advance the input position by `n` bytes before continuing.
    Skip(usize),
    /// Restart the match attempt from the current position with fresh state.
    Retry,
    /// Abort the entire match search (no more positions will be tried).
    Abort,
}
```

**How it integrates:**
- A new `ExecResult::Steer(SteerResult)` variant extends the existing callback return type.
- The VM's code-block dispatch checks for `Steer` results and acts accordingly.
- `Accept` sets `match_start_override` and returns true (similar to `\K` + immediate match).
- `Skip(n)` advances `ctx.pos` by `n` and continues VM execution.
- `Abort` sets a flag that the scanning loop checks to stop early.
- `Retry` resets captures and restarts `execute_at` from the current position.

**Use cases:**
- A callback that checks an external rule engine and says "skip ahead 50 bytes" for log scanning.
- A callback that says "abort, we've found what we need" for early termination.
- A callback that forces acceptance when domain logic determines the match is valid even if the regex hasn't finished.

**Implementation plan:**
1. Add `SteerResult` enum to `rgx-core/src/execution.rs`.
2. Add `ExecResult::Steer(SteerResult)` variant.
3. Update VM code-block dispatch in `execute_at` and `execute_subexpr` to handle steering.
4. Update `find_first_scanning` and `find_all` to check for abort flags.
5. Expose through the native callback API and inline-language bindings.
6. Add comprehensive tests for each steering action.

---

### Layer 4 — Structured Events

**Status: `planned`**

Engine emits structured events to the host at defined points during matching, without blocking the match. This enables debugging, profiling, coverage analysis, and telemetry.

**Proposed `MatchEvent` enum:**
```rust
pub enum MatchEvent {
    /// A match attempt is starting at the given input position.
    MatchAttemptStarted { position: usize },
    /// A match attempt completed (succeeded or failed).
    MatchAttemptCompleted { position: usize, matched: bool },
    /// A top-level alternation branch was entered.
    BranchEntered { branch: u32, position: usize },
    /// A top-level alternation branch was exited.
    BranchExited { branch: u32, matched: bool },
    /// A capture group completed.
    CaptureCompleted { group: u32, name: Option<String>, start: usize, end: usize },
    /// A backtrack occurred.
    BacktrackOccurred { position: usize, stack_depth: usize },
    /// A recursion/subroutine was entered.
    RecursionEntered { target: u32, depth: u32, position: usize },
    /// A recursion/subroutine was exited.
    RecursionExited { target: u32, matched: bool },
    /// A code block was evaluated.
    CodeBlockEvaluated { language: String, result: bool, position: usize },
}
```

**How it integrates:**
- `Regex::on_event(callback)` registers an event observer.
- The observer is a `Fn(&MatchEvent)` closure stored on the `Regex` or `Engine`.
- The VM emits events at the appropriate points during execution.
- Events are fire-and-forget — they do not affect match behavior.
- An `EventFilter` can be set to reduce noise (e.g., only capture events, or only backtrack events).

**Use cases:**
- **Debugger**: step through match execution, visualize backtracking.
- **Profiler**: count backtracks per position, identify hot paths.
- **Coverage**: track which branches and groups were exercised.
- **Telemetry**: feed match metrics into observability pipelines.
- **ML/AI**: collect match trace data for pattern optimization.

**Implementation plan:**
1. Add `MatchEvent` enum to a new `rgx-core/src/events.rs` module.
2. Add event observer storage to `ExecutionManager` or `RegexVM`.
3. Insert event emission calls at key points in the VM execution loop.
4. Add `Regex::on_event(...)` public API.
5. Add `EventFilter` for selective observation.
6. Ensure zero overhead when no observer is registered (compile-time or runtime gating).

---

### Layer 5 — Async/External I/O

**Status: `planned` (hardest layer)**

Callbacks can suspend the match, perform async I/O, and resume. This transforms the regex engine from a synchronous text processor into an async query engine.

**Proposed API:**
```rust
re.register_async_native("check_blocklist", |ctx| async {
    let ip = ctx.named("ip").unwrap();
    let blocked = blocklist_service.check(ip).await;
    if blocked { ExecResult::Failure } else { ExecResult::Success }
});

// Matching becomes async
let result = re.find_first_async("input text").await;
```

**How it integrates:**
- The VM execution loop becomes suspendable at code-block checkpoints.
- When an async callback is encountered, the VM saves its full state (position, captures, backtrack stack, trail) and yields.
- The async runtime polls the callback future.
- When the future resolves, the VM resumes from the saved state.
- Synchronous callbacks continue to work with zero overhead.

**Architecture options:**
1. **Coroutine-style VM**: The VM itself becomes a state machine that can be polled. Each `execute_at` call returns `Poll::Pending` when an async callback is encountered.
2. **Thread-per-callback**: Async callbacks run on a separate thread, the VM blocks on a channel. Simpler but less efficient.
3. **Continuation-passing**: The VM saves state to a `MatchContinuation` struct and returns. The host calls `resume(continuation, callback_result)` to continue.

**Recommended approach:** Option 3 (continuation-passing) — it's the most explicit, doesn't require the VM to be a Rust `Future`, and works with any async runtime (tokio, async-std, smol, or none).

**Use cases:**
- **WAF/Security**: pattern matches structure, async callback queries threat intelligence.
- **Data enrichment**: pattern extracts fields, async callback enriches from external APIs.
- **Database-backed validation**: pattern identifies candidates, async callback validates against a live database.
- **Distributed systems**: pattern routes, async callback checks service health.

**Implementation plan:**
1. Define `MatchContinuation` struct that captures full VM state.
2. Add `ExecResult::Suspend` variant that pauses VM execution.
3. Add `Regex::resume(continuation, result)` method.
4. Add `Regex::find_first_async(...)` convenience wrapper for async runtimes.
5. Ensure synchronous paths have zero overhead from the async machinery.
6. Add integration tests with mock async callbacks.

---

## Domain applications

| Domain | Pattern example | Host integration |
|--------|----------------|-----------------|
| **Network security** | `(?<ip>\d+\.\d+\.\d+\.\d+)(?{native:check_threat})` | Async threat intel lookup mid-match |
| **Log analysis** | `(?<ts>timestamp)(?{native:in_window})\s+(?<level>ERROR\|WARN)` | Time-window filtering without post-processing |
| **Protocol parsing** | `(?<header>...)(?{native:validate_checksum})(?<body>...)` | Structural parse + semantic validation in one pass |
| **Data pipelines** | `(?<field>...)(?{native:transform})` | Transform-on-extract, not extract-then-transform |
| **Language tooling** | `(?<ident>[a-zA-Z_]\w*)(?{native:resolve_symbol})` | Symbol table lookup during tokenization |
| **Business rules** | `(?<sku>SKU-\d+)(?{native:in_stock})` | Inventory check wired into pattern matching |
| **ML/AI** | `(?<entity>...)(?{native:classify})` | Inline classification during extraction |
| **Distributed systems** | `(?<route>...)(?{native:service_health})` | Route validation with live health checks |

---

### Layer 6 — File-Backed Matching

**Status: `planned`**

Engine connects directly to filesystem files, matching against contents that may be static or still being written. Combined with host callbacks, this creates a reactive file-processing pipeline.

**Proposed API:**
```rust
// Match against an existing file
let matches = re.match_file("access.log")?;

// Match with callback on each hit
re.register_native("on_error", |ctx| { alert(ctx); ExecResult::Success })?;
re.scan_file("access.log")?;  // triggers on_error for each match

// Tail a file (streaming — watches for new content)
let handle = re.tail_file("app.log", TailOptions {
    follow: true,          // keep watching after EOF
    from: FilePosition::End, // start from current end
    on_match: |m| { process(m); },
})?;

// Stop tailing
handle.stop();
```

**Key design decisions:**

1. **Memory management**: Do NOT read the entire file into memory. Use memory-mapped I/O (`mmap`) for existing files, or chunked reading with overlap for streaming files. The overlap region must be at least as large as the maximum possible match to avoid splitting matches across chunks.

2. **Streaming/tailing**: For files still being written, the engine watches for new content (via `inotify` on Linux, `kqueue` on macOS, or polling as fallback). New content is appended to the active scan buffer. Matches that span the old/new boundary are handled correctly.

3. **Line-oriented mode**: For log-style files, offer a line-oriented scan where each line is matched independently. This avoids cross-line buffer management and is the common case for log monitoring.

4. **Callback integration**: When scanning a file, each match can trigger registered callbacks (native, Lua, JS, etc.) with the full `ExecContext` including captures, variables, and branch number. This is the key integration point — the engine becomes a reactive file processor.

5. **Performance target**: File scanning must be I/O-bound, not CPU-bound. The regex matching per line/chunk should be fast enough that disk read speed is the bottleneck, not pattern matching. This means the scanning optimizations (memchr prefix skip, class filter) are critical.

**Modes:**

| Mode | Description | Use case |
|------|-------------|----------|
| `match_file` | Scan entire file, return all matches | Batch processing |
| `scan_file` | Scan entire file, trigger callbacks per match | Reactive processing |
| `tail_file` | Watch file for new content, trigger callbacks | Live monitoring |
| `match_file_lines` | Line-oriented scan, return matches per line | Log analysis |
| `scan_file_lines` | Line-oriented scan, trigger callbacks per line | Log monitoring |

**Implementation plan:**
1. Add `rgx-core/src/file.rs` module with file-backed matching API.
2. Implement `match_file` using memory-mapped I/O for static files.
3. Implement `match_file_lines` using buffered line reading.
4. Implement `scan_file` / `scan_file_lines` with per-match callback dispatch.
5. Implement `tail_file` with platform-specific file watching (kqueue/inotify/polling).
6. Expose through the CLI: `rgx-cli --file path [--follow] [--line-mode] pattern`.
7. Add integration tests with temporary files.

---

## Performance target

The engine will never match PCRE2 cycle-for-cycle on raw pattern matching — PCRE2 is a C library with decades of optimization and an optional JIT compiler. But the gap should be small enough that the extra capabilities (host callbacks, code blocks, match steering, file integration) justify the cost.

**Current state (as of this session):**

| Benchmark | RGX vs PCRE2 |
|-----------|-------------|
| find_first literal 1K | ~51x slower |
| find_all literal 1K | ~30x slower |
| find_first capture 1K | ~31x slower |
| find_all capture 1K | ~22x slower |
| find_first email 1K | ~68x slower |

**Target:**

| Benchmark | Target | Rationale |
|-----------|--------|-----------|
| find_first literal | <10x | memchr + VM overhead |
| find_all literal | <10x | scanning loop is tight |
| find_first capture | <10x | class filter eliminates most positions |
| find_all capture | <10x | in-place scanning helps |
| find_first email | <15x | complex pattern, more VM work |

**How to close the gap:**
1. **Reduce per-position VM overhead**: the main dispatch loop still has trace logging calls (even when disabled, the macro evaluation has some cost), and each opcode does bounds checking that could be hoisted.
2. **Eliminate the text copy**: `ExecContext.text` is still `Vec<u8>` — changing to a borrowed `&[u8]` reference eliminates a major allocation per `find_first`.
3. **Pre-allocate capture and backtrack structures**: reuse them across match attempts instead of creating fresh vectors.
4. **Compile-time elimination of trace macros**: gate trace/debug logging behind `#[cfg(feature = "trace")]` so release builds have zero tracing overhead.
5. **Opcode fusion**: combine common sequences (e.g., `Char` + `Char` → string compare) to reduce dispatch overhead.

These are engineering optimizations, not algorithmic changes. The algorithms (memchr scanning, trail-based backtracking, binary search Unicode lookup) are already SOTA.

## Implementation priority

| Layer | Priority | Effort | Depends on |
|-------|----------|--------|------------|
| Performance — close the PCRE2 gap | **Critical** | Medium | — |
| Layer 3 — Match Steering | **High** | Medium | Layers 1-2 (shipped) |
| Layer 4 — Structured Events | **High** | Medium | Layer 1 (shipped) |
| Layer 6 — File-Backed Matching | **High** | Medium | Layers 1-2 + performance |
| Layer 5 — Async I/O | **Medium** | Hard | Layers 1-3 |

Performance work should run in parallel with feature layers — it's critical for file-backed matching to be practical. Layer 3 should be built first among the feature layers. Layer 6 depends on adequate performance plus Layers 1-2. Layer 5 is the capstone.

## Quality requirements
- Every layer must have comprehensive unit tests and integration tests.
- Every public API must have doc examples.
- Every interaction point must have explicit error handling — no panics in host-engine communication.
- The synchronous path must have zero overhead from async machinery.
- Event observers must have zero overhead when not registered.
- All callback execution must be deterministic under backtracking (callbacks may execute multiple times for the same position).
- The host bridge must be thread-safe when the engine is shared across threads.

## Relationship to existing documents
- `PROJECT_VISION.md` — this architecture realizes the "controlled embedded code execution" goal.
- `ROADMAP.md` — Layers 3-5 should be tracked as `Next` items.
- `docs/CAPABILITY_MATRIX.md` — each layer ships as a new capability status.
- `docs/USER_GUIDE.md` — Layer 2 is already documented; Layers 3-5 will need user-guide sections.
- `CHANGES.md` — each layer ships with a changelog entry.
