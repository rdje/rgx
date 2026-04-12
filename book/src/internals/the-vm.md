# The VM

The compiler produced bytecode. The VM runs it. This chapter is about the runtime side of RGX — how the interpreter steps through opcodes, how it handles backtracking, how it finds matches without checking every position, and why the whole thing is fast enough to take PCRE2 seriously.

## Why a backtracking VM (and not a DFA)

Regex engines come in two big families. **DFA/NFA engines** like RE2 or Rust's `regex` crate convert the pattern into a state machine and run the text through it in guaranteed linear time. **Backtracking engines** like PCRE2, Perl, Python's `re`, and Oniguruma execute the pattern as a program that tries alternatives and unwinds when they fail.

RGX is the second kind.

Backtracking is the only model that can handle the features users actually want in a modern regex library:

- Backreferences (`\1`, `\k<name>`) — impossible in a pure DFA.
- Lookbehind with variable width — hard in a DFA, natural in a VM.
- Recursion and subroutines (`(?R)`, `(?&name)`) — needs a call stack.
- Embedded code blocks (`(?{lua:...})`) — needs the ability to call out mid-match.
- Backtracking verbs (`(*COMMIT)`, `(*SKIP)`, `(*SKIP:name)`, `(*PRUNE)`, `(*MARK:name)`) — meaningless in a DFA.

The tradeoff is that pathological patterns can blow up exponentially. `(a+)+b` on `"aaaaaaaaaaaaa"` with no `b` at the end is the classic example. RGX handles this with explicit safety limits (`set_max_steps`, `set_max_backtrack_frames`, `set_max_recursion_depth`) — see the [Safety Limits](../core-api/safety-limits.md) chapter and the [Sandboxing](./sandboxing.md) chapter later in this part.

This architectural choice puts RGX in the same family as PCRE2. The VM structure, opcode set, and runtime shape are deliberately similar, which is why PCRE2 is our benchmark target for both features and speed.

## The dispatch loop

At its heart, `vm.rs::RegexVM` is a boringly simple interpreter:

```rust,ignore
loop {
    let op = program.opcodes[ctx.pc];
    ctx.pc += 1;
    match op {
        Op::Char(b) => {
            if ctx.pos >= ctx.text.len() || ctx.text[ctx.pos] != b {
                if !try_backtrack(&mut ctx) { return None; }
                continue;
            }
            ctx.pos += 1;
        }
        Op::Class(ix) => { /* check class, advance or backtrack */ }
        Op::Split(a, b) => {
            push_backtrack(&mut ctx, b);
            ctx.pc = a;
        }
        Op::SaveStart(g) => {
            trail_record_start(&mut ctx, g);
            ctx.captures[g].start = ctx.pos;
        }
        // ... many more opcodes ...
        Op::Match => return Some(ctx.captures.clone()),
    }
}
```

This sketch hides details, but the shape is real. Every opcode either advances the program counter, branches, writes to the capture array, or triggers a backtrack. The dispatch loop is a single `match` on an enum, and the Rust compiler lowers it to a jump table that is competitive with hand-written C.

Trace logging calls are compiled away in release builds. In debug builds they produce structured events that the CLI can dump with `--trace-log`, which is how we debug tricky match behavior.

## ExecContext: the per-match state

Every call to `find_first` or `find_all` creates an `ExecContext` — a plain struct holding everything the VM needs to step forward:

```rust,ignore
pub struct ExecContext<'t> {
    pub text: &'t [u8],           // the input text (borrowed, not owned)
    pub pos: usize,               // current byte position in text
    pub pc: usize,                // program counter
    pub captures: Vec<Capture>,   // capture slots, pre-sized
    pub backtrack: Vec<Frame>,    // backtrack stack
    pub trail: Vec<TrailEntry>,   // capture trail for backtrack undo
    pub recursion_depth: u32,     // for (?R) / subroutine call depth
    pub step_count: u64,          // for DoS protection
    pub vars: &'t VarSnapshot,    // host-provided variables (Layer 1)
    pub observer: Option<&'t EventObserver>, // Layer 4 events
    // ... limits, mode flags, etc ...
}
```

Three details matter for performance:

**`text` is a borrowed slice, not a `Vec`.** Early versions of RGX copied the input into an owned `Vec<u8>` per match. That single allocation was responsible for a significant chunk of the ~50x slowdown versus PCRE2 on short inputs. Switching to `&[u8]` eliminated the copy and is one of the biggest wins in RGX's performance history.

**`captures` is pre-sized at compile time.** The compiler knows exactly how many capture groups exist in the program. The capture vector is sized once and reused across match attempts via `captures_read` — see `CaptureLocations` in Part II.

**`step_count` is checked every opcode.** This is how `set_max_steps` works. When the limit is `None`, the check is a single integer comparison and a branch that is always predicted correctly, so the overhead is effectively zero. When the limit is set, the VM aborts cleanly when exceeded.

## Scanning: finding where to try

`find_first("hello world", "world")` does not blindly start the VM at position 0. That would be `O(n*m)` in the worst case. Instead, the engine uses a **scanning strategy** selected at compile time based on what the optimizer found.

```text
              ┌──────────────────────────┐
              │   find_first(text)       │
              └───────────┬──────────────┘
                          │
                          ▼
              ┌──────────────────────────┐
              │ Is program.pure_literal? │
              └─────┬────────────────┬───┘
                    │yes             │no
                    ▼                ▼
           ┌──────────────┐  ┌───────────────────┐
           │ memmem::find │  │ Is it anchored?   │
           └──────────────┘  └────┬──────────────┘
                                  │
                              ┌───┴────────────┐
                              │yes             │no
                              ▼                ▼
                    ┌──────────────┐  ┌────────────────┐
                    │ VM at pos 0  │  │ PrefixHint?    │
                    └──────────────┘  └──┬─────────────┘
                                         │
                                     ┌───┴───────────┐
                                     │literal        │class/none
                                     ▼               ▼
                            ┌────────────────┐  ┌──────────────┐
                            │ memmem scan    │  │ byte-by-byte │
                            │ then VM confirm│  │ loop with    │
                            └────────────────┘  │ class filter │
                                                └──────────────┘
```

**Pure literal fast path.** If the pattern has no metacharacters at all, the engine skips the VM entirely and calls `memmem::find` from the `memchr` crate. This is what makes literal searches roughly 6.4x **faster** than PCRE2 on the benchmark suite — we are just calling the same SIMD-accelerated search primitives that ripgrep uses, with no VM overhead.

**Literal prefix scan.** If the pattern starts with a literal string like `hello(.*)`, the engine uses `memmem` to find candidate positions for `hello` and then runs the VM from each one. For patterns with strong literal prefixes, this is close to memmem speed.

**Class prefix scan.** If the pattern starts with a character class like `[A-Z]\w+`, the engine sweeps bytes until it finds one the class accepts, then runs the VM. Slower than memmem but much faster than trying every position.

**No prefix.** If the pattern has no useful prefix (say, `.*foo`), the engine walks byte by byte, trying the VM at each position. This is the slow case, and it is why patterns with anchors or strong prefixes outperform ones without.

## Backtracking with a trail

The hard part of any backtracking VM is making backtrack cheap. When the VM tries a branch and it fails, it needs to undo everything that branch did to the captures and the position, then retry the alternative — ideally without allocating.

RGX uses a **capture trail**. Every time the VM writes to a capture slot, it pushes a `TrailEntry` onto a trail stack:

```rust,ignore
pub struct TrailEntry {
    group: u32,
    which: SlotKind,    // start or end
    previous: Option<usize>,
}
```

When the VM creates a backtrack frame for a `Split`, it records the current trail length. If the branch fails and the VM pops that frame, it truncates the trail back to the saved length, walking backwards and restoring each capture slot to its previous value.

This is much cheaper than the naive approach of copying the entire capture array at every branch point. For patterns with N capture groups and K branches, the naive approach is `O(N*K)`; the trail approach is `O(M)` where `M` is the number of capture writes that actually happened on the branches taken — typically a tiny fraction of `N*K`.

This is the same technique PCRE2 uses. The ["Trail-based backtracking restores state correctly"](./testing-philosophy.md) claim in the testing philosophy is specifically designed to stress-test this code with deep backtracking through capture-modifying callbacks.

## Host integration meets the dispatch loop

The six host integration layers all plug into the same dispatch loop.

**Layer 1/2 — data and callbacks.** When the VM hits a `CallCode(id)` opcode, it looks up the callback (native Rust, Lua, JS, Rhai, WASM) in the `Regex`'s callback registry, builds an execution context with the current position and captures, and invokes it. The return value drives the next VM step.

**Layer 3 — steering.** If the callback returns `ExecResult::Steer(SteerResult::Skip(n))`, the VM advances `ctx.pos` by `n` and continues. If it returns `Abort`, a flag is set that the outer scanning loop checks to stop iterating. If it returns `Accept`, the VM effectively jumps to the nearest `Match` opcode. Steering is a small extension to the dispatch loop — an extra match arm after the callback.

**Layer 4 — events.** The dispatch loop has a handful of `emit_event_if_observer!` macro calls at known points: match attempt start/end, branch entry, capture completion, backtrack. When no observer is registered, these compile to a single `if` that is always false, with no function call at all. When an observer is registered, they build a `MatchEvent` and invoke the observer callback.

**Layer 5 — async suspension.** If a callback returns `ExecResult::Suspend`, the VM captures its full state (pc, pos, captures, trail, backtrack frames, recursion depth) into a `MatchContinuation` and returns `ExecResult::Suspend` up the stack. The caller awaits the host-provided future and calls `Regex::resume(continuation, result)` to pick up where the VM left off. Crucially, the continuation is `Send + Sync`, so you can move it across threads.

**Layer 6 — file-backed matching.** This does not touch the VM at all. It wraps the existing `find_first` / `find_all` entry points with a file reader that feeds chunks of text into the engine. The VM does not care whether its input came from a `String` or a memory-mapped file.

## Fast paths on top of fast paths

Several patterns get their own specialized execution paths:

**Anchored patterns** (`^...` or `\A...`) skip the scanning loop entirely — the VM is invoked once at position 0 and that's it.

**Literal concatenations** (`foo.*bar` where both `foo` and `bar` are literals) use `memmem` twice: once to find `foo`, then VM execution to run `.*`, then `memmem` to find `bar`. The VM still runs, but the expensive part is delegated to the SIMD primitives.

**Short literals** that fit in a single SIMD register are searched with vectorized routines from the `memchr` crate. This is the same machinery that makes ripgrep fast.

**Single-character classes** use byte tables for `[abc]`-style classes and binary search over sorted range tables for Unicode property classes. The latter was a major performance fix: the old linear scan through property ranges was a hotspot on Unicode-heavy text.

## The backlog: what's NOT optimized yet

Honest accounting matters. RGX is fast, but it is not PCRE2 with JIT on every benchmark.

For a long time, the main thing RGX did not have was **JIT compilation**. That changed with the C1 production cutover: RGX now ships a Cranelift-based JIT that translates bytecode into native machine code for the JIT-eligible subset, on by default. See [The JIT Compiler](./jit-compiler.md) for the full design — that chapter explains the JIT-eligible subset, the per-frame capture snapshot, the runtime helper layer, and the dispatch decision.

RGX also has a **DFA hybrid** as of the C2 production cutover. For patterns in the no-backtracking subset, a Thompson NFA + lazy DFA cache runs alongside this VM and is preferred whenever it can deliver the answer. See [The NFA/DFA Hybrid Engine](./nfa-dfa-engine.md) for the full design — that chapter explains the dispatch chain, the per-position skip acceleration, and the two-pass capture recovery trick.

Together, the three execution tiers (DFA, Pike-VM, JIT, and the backtracking VM as the always-available fallback) push RGX to **3.16x faster than PCRE2 on literals** and **1.96x faster than PCRE2 on capture_groups** in the benchmark suite, while keeping the backtracking VM permanently in place for patterns that need its features.

## Next: the second engine

The backtracking VM is one third of the run-time story. The other two thirds are the C2 hybrid and the C1 JIT that run alongside it. Head to [The NFA/DFA Hybrid Engine](./nfa-dfa-engine.md) for the C2 design, then [The JIT Compiler](./jit-compiler.md) for the C1 design, then on to [PGEN Integration](./pgen-integration.md) for the parser boundary.
