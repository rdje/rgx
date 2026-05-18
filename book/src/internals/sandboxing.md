# Sandboxing & Security

RGX is not just a regex matcher. It is a pattern engine that can call into Lua, JavaScript, Rhai, WebAssembly, and native Rust closures **mid-match**. That power is the whole point of the host integration layers — but it also means RGX has to think seriously about what happens when the code running inside a pattern is not trusted.

This chapter is about the security model: what is safe, what is not, how the sandboxes work, and what the threat model is.

## The three execution modes

Everything starts with `ExecutionMode`. Every compiled `Regex` is bound to a mode, set at compile time, and the mode determines which code-block backends are allowed to run.

```rust,no_run
pub enum ExecutionMode {
    /// Pure regex. No code blocks allowed.
    Pure,
    /// Sandboxed code blocks: Lua, JavaScript, Rhai, WASM.
    Safe,
    /// Everything, including native Rust callbacks.
    Full,
}
```

The rules are:

| Mode | Pure regex | Lua / JS / Rhai | WASM | Native Rust |
|------|:----------:|:---------------:|:----:|:-----------:|
| `Pure` | yes | **rejected at compile time** | rejected | rejected |
| `Safe` | yes | yes (sandboxed) | yes (wasmtime) | rejected |
| `Full` | yes | yes | yes | yes |

The key property: if a pattern uses a code block and the mode does not allow that backend, **compilation fails**. There is no "oops I forgot to set the mode" path. You cannot accidentally execute native Rust from a pattern authored by an untrusted user unless you explicitly asked for `ExecutionMode::Full`.

## The threat model

RGX's threat model is this: **a hostile actor can supply patterns, code blocks, and input text, but cannot modify the host binary.** Given that, what should be safe?

The answer depends on which mode you use:

- **`Pure`**: it is always safe to run untrusted patterns. The worst the attacker can do is a catastrophic backtrack, which is bounded by `set_max_steps` and friends.
- **`Safe`**: it is safe to run untrusted patterns **that include sandboxed code blocks**. The sandboxes are designed to prevent filesystem access, network access, process spawning, and unbounded resource use. The worst an attacker can do is waste CPU until the step limit kicks in.
- **`Full`**: patterns are assumed to come from a trusted source (your own code, or carefully reviewed configuration). `Full` exists because some applications need to call out to native Rust code that a sandbox cannot provide — but the assumption is that **the pattern author is trusted**.

The distinction matters. A WAF that compiles rules uploaded through a web UI should use `Safe` (and probably with `(?{...})` disabled entirely via a separate flag). A data pipeline that compiles its own patterns with its own callbacks can use `Full`.

## The Lua sandbox

When you write `(?{lua:arg[1] > 10})`, RGX spins up an `mlua` instance and loads your code into a restricted environment. The restrictions are:

- **No `io` library.** File read/write is not available. There is no way to open a file, list a directory, or touch the filesystem.
- **No `os` library.** No process spawning, no environment variable access, no system clock, no `os.exit`.
- **No `debug` library.** The debug library can be used to bypass Lua's safety guarantees, so it is removed.
- **No `require`.** Module loading is disabled. Your code block cannot import anything; it can only use the functions and variables exposed by RGX.
- **No `package`.** Same reason as `require`.
- **What is available:** basic Lua primitives (`string`, `math`, `table`, `tostring`, `tonumber`, arithmetic and string operations), plus the RGX-provided context: `arg[0]`, `arg[1]`..., `named`, `vars`, `pos`, `match_start`, `match_end`, `match_length`, `branch_number`, `text`, and the `rgx.emit_numeric` / `rgx.emit_replacement` helpers.

This is implemented by creating a fresh `mlua::Lua` instance per evaluation with a pre-curated globals table. Nothing that could escape the sandbox is in scope.

## The JavaScript sandbox

JavaScript is provided by `rquickjs` — a Rust binding over QuickJS. The sandbox:

- **No `eval`.** Arbitrary code evaluation is disabled.
- **No `Function` constructor.** Same reason: constructing functions from strings is code injection by another name.
- **No `fetch`.** There is no networking at all. QuickJS does not ship a `fetch` and we do not add one.
- **No `process`, `require`, or module loading.** Not available in QuickJS by default, and we do not add them.
- **Memory limit: 10 MB per evaluation.** QuickJS supports hard memory caps through its runtime API, and we set a conservative limit. A code block that tries to allocate a huge array will fail cleanly with an out-of-memory error rather than taking the process down.
- **Stack limit: 256 KB.** Prevents deep recursion from blowing the native stack.
- **What is available:** standard JavaScript primitives (arithmetic, string methods, `Array`, `Object`, `JSON`, `Math`), plus the RGX-provided context (`arg`, `named`, `vars`, `pos`, `rgx.emitNumeric`, `rgx.emitReplacement`).

The memory and stack limits are the important difference from Lua. Lua 5.4 does not have easy per-invocation memory caps, so the Lua sandbox relies on the step limit and the lack of dangerous APIs rather than a resource ceiling. JavaScript has cheaper memory limits and we use them.

## The Rhai sandbox

Rhai is a scripting language designed for embedding in Rust, and its threat model is very similar to ours. The Rhai sandbox:

- **No filesystem access.** Rhai itself does not ship filesystem APIs.
- **No network access.** Same story.
- **Restricted stdlib.** We expose only the numeric, string, and collection primitives that code blocks actually need.
- **Built-in operation limits.** Rhai supports operation counters natively, and we wire them to the engine's step limit so Rhai evaluations cannot run unbounded.
- **What is available:** Rhai's standard operations plus the context functions (`current_match`, `named`, `variable`, `emit_numeric`, `emit_replacement`).

Rhai is the youngest of the three inline languages in RGX. It was added after Lua and JS, following the same source-body contract shape where practical, which means the behavioral contract (bare expressions vs `return ...`, result emission helpers) is consistent across the three languages.

## The WASM sandbox

WebAssembly is the strongest sandbox RGX offers, which is why `(?{wasm:module:function})` is allowed in `ExecutionMode::Safe`.

WASM runs via `wasmtime`. The sandbox properties:

- **WASM is a memory-isolated instruction set.** A WASM module cannot touch the host's memory outside of its linear memory region. It cannot read the RGX process's stack, heap, or environment.
- **No system calls.** WASM has no syscalls except the ones the host explicitly imports. RGX imports only the `rgx` host functions (current position, captures, variables, `emit_numeric`, `emit_replacement`). There is no `open`, no `write`, no `socket`.
- **Configurable fuel/step limits.** wasmtime supports fuel-based execution limits that count instructions. A runaway WASM module cannot loop forever.
- **Module signature checks.** RGX expects WASM modules to expose exported `() -> i32` predicate functions. Modules that do not match the expected signature fail cleanly at registration time.

The practical result: you can load a WASM module supplied by an untrusted user with high confidence that it cannot break out. It is the one place where RGX will run untrusted **code** (not just untrusted regex) without qualification.

## Native callbacks: the trusted zone

`ExecutionMode::Full` unlocks native Rust callbacks registered via `Regex::register_native`. These are **not sandboxed at all** — a native callback is a Rust closure that runs in the RGX process with full access to everything Rust can do.

This is on purpose. Native callbacks exist so applications can wire in their own business logic efficiently: querying a local cache, calling into a database connection pool, consulting an in-memory rule engine. That kind of logic cannot be sandboxed without losing its speed and ergonomics.

The implication: **`Full` mode is only for trusted patterns.** If your code can call `register_native` and also compile patterns from untrusted input, you have a privilege escalation waiting to happen. The common pattern is to register native callbacks at startup in trusted code and then compile user patterns in `Safe` mode so the callbacks are simply not reachable from the user's regex.

## Resource limits: the other half of safety

Sandboxes prevent API misuse. Resource limits prevent CPU and memory exhaustion. RGX has three configurable safety limits on every compiled `Regex`:

```rust
# use rgx_core::Regex;
# let re = Regex::compile(r"\d+")?;
re.set_max_steps(Some(100_000));              // abort after N VM steps
re.set_max_backtrack_frames(Some(1_000));     // cap backtrack stack size
re.set_max_recursion_depth(Some(50));         // cap (?R)/subroutine depth
# Ok::<(), Box<dyn std::error::Error>>(())
```

**`set_max_steps`** is the primary DoS protection. Every opcode execution increments a step counter, and when the counter exceeds the limit the VM aborts with an error. Pathological patterns like `(a+)+b` on adversarial input terminate cleanly instead of spinning for seconds.

**`set_max_backtrack_frames`** caps the size of the backtrack stack. Deeply nested alternations with lots of backtracking cannot exhaust memory.

**`set_max_recursion_depth`** caps `(?R)` and `(?&name)` recursive calls. A maliciously recursive pattern cannot blow the native stack.

By default these limits are `None` (unlimited), which matches PCRE2's defaults. For any application that accepts patterns from untrusted sources, setting explicit limits is non-negotiable. The limits are cheap to check — each is a single integer comparison per opcode — and the DoS protection they provide is complete.

These are tracked as backlog items **A1** (step limits) and **A2** (memory limits). Both are shipped.

## Determinism under backtracking

One subtle safety property: callbacks may execute **multiple times at the same input position** because the VM can backtrack through a code block and re-invoke it. The host integration architecture explicitly requires callbacks to be deterministic under this condition, or at least to tolerate re-execution.

The test suite includes "trail-based backtracking restores state correctly" as a claim the engine must prove. Part of that test verifies that when the same callback fires 50 times during a backtrack sequence, the captures visible to the callback on each invocation are exactly what they would be if you ran the regex from scratch. If a callback sees inconsistent state, that is a VM bug and the test should catch it.

For stateful callbacks (e.g., one that writes to a log or updates a counter), the host needs to decide what to do. The simplest rule is "only act on the final winning match," which you get by deferring side effects until after `find_first` returns. A more sophisticated rule is to use the Layer 4 event observer, which fires at well-defined lifecycle points (match attempt start/end, capture completion) that do not double-fire on backtrack.

## What we do NOT do

A couple of things are worth listing explicitly:

- **We do not sandbox pattern parsing.** If PGEN has a bug that crashes on a malicious pattern, RGX crashes. We mitigate this with fuzzing (`cargo-fuzz` with four targets) and by treating parser bugs as high-severity, but the parser itself runs in-process.
- **We do not sandbox `Pure` mode at all.** Pure regex has no code blocks and no way to escape the engine — the only resource it can exhaust is CPU, which the step limits cover.
- **We do not encrypt patterns or obfuscate bytecode.** A user with access to your binary can see your patterns. Treat patterns as code, not as secrets.
- **We do not attempt to detect "evil" patterns automatically.** ReDoS detection in general is an undecidable problem. Explicit step limits are the pragmatic defense.

## Putting it together

The safety story for RGX is layered:

```text
┌─────────────────────────────────────────────────────┐
│ ExecutionMode:      Pure / Safe / Full              │
├─────────────────────────────────────────────────────┤
│ Code block backends (in Safe mode):                 │
│   - Lua: no io/os/debug/require                     │
│   - JS: no eval/Function/fetch, 10MB, 256KB stack   │
│   - Rhai: no I/O, built-in op limits                │
│   - WASM: wasmtime, memory isolation, fuel limits   │
├─────────────────────────────────────────────────────┤
│ Resource limits (all modes):                        │
│   - set_max_steps                                   │
│   - set_max_backtrack_frames                        │
│   - set_max_recursion_depth                         │
├─────────────────────────────────────────────────────┤
│ Determinism requirements:                           │
│   - callbacks must tolerate re-execution            │
│   - events fire at well-defined points              │
└─────────────────────────────────────────────────────┘
```

Used together, these let you run untrusted patterns safely. The mode picks which backends are allowed. The sandboxes prevent the allowed backends from breaking out. The limits prevent runaway execution. The determinism contract keeps state consistent across backtracks.

No security model is perfect, and we have tried to be honest about what each layer does and does not protect. If you are building a system where untrusted users can supply regexes, the recommended starting point is `ExecutionMode::Pure` with aggressive step limits. Move to `Safe` only when you need code blocks, and to `Full` only when the patterns come from a trusted source.

## Next: how we test all this

Sandboxes and limits are only as good as the tests that verify them. Head to [Testing Philosophy](./testing-philosophy.md) next to see how RGX tries to break itself.
