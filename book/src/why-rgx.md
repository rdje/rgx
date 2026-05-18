# Beyond regex: what rgx adds

Most users come to rgx looking for a regex engine. rgx is one — a fast, PCRE2-compatible engine with a Pike-VM, an NFA/DFA hybrid, a tagged DFA for captures, and (optionally) a JIT. The benchmarks tell that story: rgx matches or beats PCRE2 on every pattern in the headline bench corpus, and on capture-heavy patterns it's 47× faster.

But "fast regex engine" undersells what rgx actually is. rgx is a **programmable text-processing platform** that uses PCRE2 syntax as its surface. The regex is the access point; the engine underneath can run code, steer its own behavior, stream events, watch files, and return computed values — all driven by syntax that fits inside a regex literal.

This chapter explains the differentiators in concrete terms, when each one matters, and how rgx compares to the regex engines you might be switching from.

## The differentiator stack

Seven capabilities set rgx apart from a conventional regex engine:

### 1. Inline code blocks in five languages

A code block in a pattern runs *during* matching:

```rust,no_run
# use rgx_core::{Regex, ExecutionMode};
let re = Regex::with_mode(
    r"(\d{4})-(\d{2})-(\d{2})(?{lua: return tonumber(MATCH:sub(6,7)) <= 12 })",
    ExecutionMode::Safe,
)?;
# let _ = re;
# Ok::<(), Box<dyn std::error::Error>>(())
```

The supported embedded languages are **Lua, JavaScript, Rhai, WASM, and native Rust callbacks**. Each runs in a sandboxed VM owned by rgx; the host application doesn't have to ship a scripting interpreter to use them. See [Predicate Callbacks](./host-integration/predicate-callbacks.md) and [Sandboxing & Security](./internals/sandboxing.md).

No other regex engine in widespread use offers this. Pattern-embedded predicates in PCRE2 are restricted to `(?C)` callouts to host code; rgx's inline blocks carry their own execution.

### 2. Match steering from inside the pattern

A code block can move the match cursor:

```rust,no_run
# use rgx_core::{Regex, ExecutionMode, ExecResult, SteerResult};
# let re = Regex::with_mode(r"\w+(?{native:skip_quoted})", ExecutionMode::Full)?;
re.register_native("skip_quoted", |ctx| {
    if ctx.current_match().is_some_and(|s| s.starts_with('"')) {
        ExecResult::Steer(SteerResult::Skip(2 /* past the closing quote */))
    } else {
        ExecResult::Success
    }
})?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

`Steer` is a first-class return value from a callback. The engine accepts, fails, or *jumps* depending on what the code says. See [Match Steering](./host-integration/match-steering.md).

### 3. `code_result`: matches that return values

Every `MatchResult` has a `code_result: Option<CodeBlockValue>` field. Inline code blocks can return *typed values*, not just success/failure flags:

```rust,no_run
# use rgx_core::{Regex, CodeBlockValue};
let re = Regex::compile(r"(\d+)(?{lua: return tonumber(GROUP1) * 2})")?;
let m = re.find_first("the answer is 21").unwrap();
assert_eq!(m.code_result, Some(CodeBlockValue::Numeric(42.0)));
# Ok::<(), Box<dyn std::error::Error>>(())
```

Regex becomes a tiny DSL where matches carry computed values back out. Tokenizers, parsers, log enrichers, and validators all benefit.

### 4. Structured match events

Observers receive a stream of `MatchEvent`s as the engine works — match start, match end, capture set, group entry/exit, code block executions:

```rust,no_run
# use rgx_core::Regex;
let re = Regex::compile(r"(\w+)\s(\w+)")?;
re.on_event(|event| {
    println!("{:?}", event);
})?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

See [Structured Events](./host-integration/structured-events.md). Useful for streaming log scanners, IDE syntax highlighters, observability backends, and tests.

### 5. Async I/O and file watching

`Regex::tail_file(path, options, on_match)` streams a file as it grows, applying the regex to each new line:

```rust,no_run
# use rgx_core::Regex;
# use rgx_core::file::TailOptions;
let re = Regex::compile(r"ERROR\s+(\w+):\s*(.+)$")?;
// `tail_file` returns a `TailHandle` that controls the watch.
let _handle = re.tail_file("/var/log/app.log", TailOptions::default(), |m| {
    // `m` is a `FileMatch`: the matching line plus its MatchResult.
    println!("error on line {}: {}", m.line_number, m.line);
});
# Ok::<(), Box<dyn std::error::Error>>(())
```

See [Async I/O](./host-integration/async-io.md) and [File Matching & tail_file](./host-integration/file-matching.md). This is regex as a streaming primitive, not just a string operation.

### 6. Sandbox modes

`ExecutionMode::{Pure, Safe, Full}` controls what inline code can do:

| Mode | Lua / JS / Rhai / WASM | Native callbacks |
|---|---|---|
| `Pure` | rejected | rejected |
| `Safe` | allowed, sandboxed | rejected |
| `Full` | allowed | allowed |

This is the *trust knob* for accepting patterns from untrusted sources. A web server receiving regex patterns from users can compile in `Safe` mode and know no native code will execute. See [Sandboxing & Security](./internals/sandboxing.md).

### 7. PCRE2 conformance

The features above sit on top of an engine that runs PCRE2's testdata at **12,806 / 4 / 0 / 0** (~99.97% pass rate). Patterns written for Perl, PHP, R, Julia, Ruby's PCRE backends, or any PCRE2 binding will match the same strings in rgx, with the same captures.

## The embedded language set: why these five, not others?

A reasonable question when reading the differentiator list above: rgx embeds Lua, JavaScript, Rhai, WASM, and native Rust — why these specifically? Why not C, Python, or Julia? The answer is a real design choice, not an accident, and the choice has consequences users should understand before adopting rgx for a use case that depends on the embedded surface.

There are two distinct axes that get conflated when people first encounter rgx:

- **Embedded scripting host** — a language rgx *runs inside* the regex pattern, via `(?{lang:...})` blocks. The code executes within an interpreter rgx hosts in-process. The current set: Lua, JS, Rhai, WASM, native Rust.
- **FFI host language** — a language that *calls into* rgx from outside, through a C ABI boundary. Python, Go, Julia, C, Zig, Ruby, PHP, etc. The host language doesn't run inside the regex; rgx runs inside the host.

These are orthogonal. A Python user calling rgx through a binding is on the FFI axis. A `(?{lua:...})` block running inside the regex is on the embedded axis. Both can be true simultaneously: a Python program can compile a regex containing a Lua block, and the Lua executes inside rgx's Lua sandbox without Python ever being aware of it.

The embedded-set choice was made on three axes:

1. **Embed cost** — how heavy is the interpreter to link into rgx's binary?
2. **Sandboxability** — can untrusted patterns be executed safely?
3. **Design-space coverage** — does each option fill a unique role, or is it redundant with another?

The five embedded hosts were chosen for the design-space coverage they give:

| Host | Embed cost | Sandbox story | Niche it fills |
|---|---|---|---|
| **Lua** | ~200 KB | Mature — `lua_State` API restricts stdlib; industry standard for embedded scripting in Nginx, Redis, game engines, Wireshark | Tiny + fast + universal |
| **JavaScript** (QuickJS) | ~2 MB | Engine isolates per call; standard for plugin/extension systems | Familiar syntax for web developers |
| **Rhai** | Rust-native (no FFI) | Memory-safety guaranteed by Rust's invariants; tightest integration with rgx's type system | Zero-FFI option for Rust-only deployments |
| **WASM** | small runtime, modules are user-supplied | Bounded execution by design; capability-restricted by host | Catch-all compile-target for any language that can produce WASM |
| **native** Rust callbacks | none — registered closures | No sandbox by definition; gated behind `ExecutionMode::Full` | Maximum performance, full type safety |

Now apply the same three axes to the languages the rationale rules out:

| Candidate | Embed cost | Sandbox story | Verdict |
|---|---|---|---|
| **C** | TCC ~100 KB but limited; cling/LLVM-JIT ~50 MB | No sandbox — C is memory-unsafe by design; no bounded execution; arbitrary pointer arithmetic | **Rejected.** The `native:` Rust-callback role already covers "C-speed registered function." For C source compiled to bytecode, use the WASM path. |
| **CPython** | ~10 MB minimum | Not safely sandboxable. RestrictedPython is a workaround, not a security boundary. GIL is global — embedding stalls the entire regex on every Python call. | **Rejected.** Python's value with respect to rgx is the FFI axis (calling rgx FROM Python), not the embedded axis. |
| **libjulia** | ~100 MB+ | JIT-heavy; multi-second cold start; sandboxing essentially impossible | **Rejected.** Same as Python — calling rgx FROM Julia is the right shape, not embedding Julia INSIDE rgx. |

The principled boundary is: **embedded host = sandboxable, lightweight, embeddable**. C, Python, and Julia fail all three. They're perfectly good FFI hosts (and rgx aims to support them on that axis — see the next section), but not viable embedded hosts.

### The WASM back door

The WASM embedding deserves special attention because it widens the embedded set substantially without adding maintenance burden.

Any language that compiles to WebAssembly can run inside a `(?{wasm:...})` block. This covers C, C++, Go, AssemblyScript, Zig, Rust, Carbon, and a growing list of others. The user compiles their code to a `.wasm` module and rgx executes it in a sandboxed `wasmtime` engine. So "C inside a pattern" is technically already addressable — just not by linking C source directly into rgx itself.

The trade-off: WASM modules have a startup cost (module instantiation per execution) higher than the native scripting hosts. Use Lua/JS/Rhai for hot loops where the predicate runs millions of times; use WASM for cold-path enrichment where the language flexibility matters more than per-call latency.

### Future additions

If a new embedded host is ever added, it must clear the three-axis test:

- **Embed cost**: under ~5 MB, preferably under 1 MB. Larger interpreters dominate rgx's own binary and make the dependency unattractive for embedded / WASM / serverless deployments.
- **Sandboxability**: a real isolation boundary, not a "we trust our patterns" workaround. rgx accepts patterns from untrusted sources; the embedded language must too.
- **Design-space niche**: a unique role the existing five don't fill. "Familiar syntax" alone isn't a niche; that's an FFI-axis concern.

Candidates that *could* clear the bar in principle: a small embeddable Scheme (Chibi-Scheme, ~200 KB), Wren (~80 KB), Mun. None of these have user demand pulling for them today. The current five cover the design space sufficiently.

## How rgx compares

Compared to other regex engines, the differentiators land like this:

| Engine | PCRE2 syntax | Embedded scripting in patterns | Match steering | Streaming / file watch | Sandboxing |
|---|:-:|:-:|:-:|:-:|:-:|
| **rgx** | ✅ | ✅ Lua/JS/Rhai/WASM/native | ✅ | ✅ | ✅ |
| PCRE2 (C) | ✅ | `(?C)` callouts only | ✅ via callouts | — | — |
| Oniguruma | partial | — | — | — | — |
| Go `regexp` (RE2) | — | — | — | — | — |
| Rust `regex` | — | — | — | — | — |
| Python `re` | partial | — | — | — | — |
| Python `regex` | mostly | — | — | — | — |
| JavaScript `RegExp` | — | — | — | — | — |

The two-axis read: rgx is the only engine in the table that offers PCRE2 syntax *and* programmable text-processing primitives. Pick your second axis — "does my language already have PCRE2?" or "does it have embedded scripting in regex?" — and rgx is competitive on at least one of the two for every mainstream language.

## When rgx is the right choice

You should consider rgx when at least one of the following is true:

- **You're scanning logs, telemetry, or streamed data.** `tail_file`, observers, and async I/O make rgx a first-class streaming primitive, not just a string operation.
- **Your patterns need conditional logic.** A regex with `(?{lua: ...})` can branch on conditions a pure regex can't express: range checks, lookup tables, prior-match memory, side-channel data.
- **You parse a DSL where the regex IS the parser.** `code_result` returns typed values directly from matches. Builds tokenizers, validators, and small parsers without leaving the regex surface.
- **You accept regex from untrusted input.** Sandbox modes give you a trust boundary that engines without sandboxing can't.
- **You want PCRE2 semantics from a language whose stdlib regex isn't PCRE2.** Go, JavaScript, Rust, and others ship engines that diverge from PCRE2 in subtle ways. rgx is a portable PCRE2.

A regex without those needs is fine on whatever your language ships with. rgx adds a tax (binary size, compile time, dependency surface) you only want to pay if you'll use what it adds.

## From other languages

The previous section established that rgx's *embedded* scripting hosts are Lua, JS, Rhai, WASM, and native Rust. Languages like Python, Go, Julia, C, and Zig are not embedded hosts — and the design rationale explains why they shouldn't be. They are, however, perfectly good **FFI hosts** — languages that call rgx from outside.

For users on the FFI axis, rgx's value depends on which differentiators translate cleanly across the C ABI boundary.

The good news is that most of them do. Embedded scripting blocks run *inside rgx's sandbox*; the host language never sees the Lua/JS/Rhai/WASM interpreter. From Go, Python, Julia, or any C-FFI host:

```python
import rgx  # hypothetical Python binding
re = rgx.compile(r"(\d+)(?{js: return Number(GROUP1) > 100})")
re.find_first("count is 42")    # None
re.find_first("count is 200")   # match
```

The Python caller writes the JavaScript filter as a string in the pattern. rgx handles the JS execution internally. No Python-to-Rust callback per match. No FFI overhead per filter call. This works the same way from Go, Julia, Zig, Ruby, PHP — anywhere C interop is available.

What does NOT translate well across FFI is the case where the predicate callback itself is *written in the host language* (e.g. a Python function called from inside the regex hot loop). The per-call FFI cost dominates for short patterns. This is the same constraint that affects every C library called from Python/Go/etc.

A summary of how each differentiator behaves across FFI:

| Feature | Crosses FFI cleanly? | Why |
|---|:-:|---|
| Inline Lua / JS / Rhai / WASM blocks | ✅ | Embedded language runs inside rgx; the host language is uninvolved |
| `tail_file` / streaming | ✅ | File path in, event stream out |
| Sandbox modes | ✅ | A config flag |
| Structured events | ✅ | A stream of plain-data records |
| `code_result` | ✅ | A returned value of a tagged-union type |
| Host-language predicate callbacks | ❌ | cgo / ctypes per-call overhead dominates the hot loop |
| Host-language match steering | ❌ | Same reason — every callback is an FFI boundary crossing |

Five of the seven differentiators are FFI-friendly. The two that aren't are the rgx-specific extensions to Rust's `Fn` closure model that, by their nature, require a native-language hot path.

Language-binding status is tracked in [Project Status & Roadmap](./internals/project-status.md). The current strategy is to ship a stable C API first (via `cbindgen`), then layer thin idiomatic wrappers in the languages where the differentiator stack has the largest gap relative to the language's existing regex offering. The full design — C ABI surface, error/memory/threading models, 7-phase staging plan, per-language priority list — is documented in `docs/A9_LANGUAGE_BINDINGS_DESIGN.md` (Phase 0 landed 2026-05-13; Phase 1 scaffolding shipped as the `rgx-capi` crate). The **authoritative C ABI stability contract** for FFI consumers who load `librgx.{so,dylib,dll}` — SemVer mapping, the append-only error-code rule, per-function stability tiers, the deprecation policy, and the header-drift gate — is `rgx-capi/STABILITY.md`.

## Next

Continue to [Installation & First Match](./getting-started/first-match.md) for the regex-as-regex tour, or skip ahead to [Part IV: Host Integration](./host-integration/data-exchange.md) to see the programmable surface in action.
