# The JIT Compiler (C1)

The previous chapters introduced the backtracking VM and the NFA/DFA hybrid (C2). This chapter is about the third execution tier — the **just-in-time compiler** that translates RGX bytecode into native machine code via Cranelift. Internally it's called **C1**: the first engineering improvement track on the RGX roadmap, sequenced after C2 so its constant-factor speedup compounds on top of C2's algorithmic-class improvement.

C1 is fully shipped and on by default as of the production cutover (step 8). For every JIT-eligible pattern, the public `Regex` API now dispatches to native code generated at `Regex::compile` time. Patterns the JIT can't handle continue to run on the existing dispatch chain (DFA → Pike-VM → backtracking VM) unchanged.

## Why JIT-compile a regex

The backtracking VM and the NFA/DFA hybrid both spend most of their time in **opcode dispatch**: a `match` over the current opcode byte, decoding operands, and calling the right per-opcode helper. Even with the optimised Rust release build, the dispatch overhead is real — for tight inner loops on simple patterns, it dominates the actual matching work.

A JIT eliminates that overhead by **compiling each opcode into native instructions inline**. There is no opcode dispatch loop, no operand decode, no function-call indirection through the interpreter — the bytecode for `\d+x` becomes a straight-line sequence of byte loads, comparisons, and conditional branches. On x86_64 this is the difference between ~10 cycles per character (interpreter) and ~3 cycles per character (JIT), which compounds across long inputs.

PCRE2's JIT delivers roughly a 5–10x speedup over its interpreter on typical patterns. The Rust `regex` crate doesn't ship a JIT — it relies entirely on its NFA/DFA hybrid for speed. RGX wants both: the algorithmic improvements of C2 *and* the constant-factor wins of a JIT, layered so they compound.

## What "C1" is

C1 is the cluster of code under `rgx-core/src/c1/`:

```text
rgx-core/src/c1/
├── codegen.rs      Bytecode → Cranelift IR translation; the bulk of C1
├── jit.rs          JitHost (Cranelift JITModule wrapper) + JitProgram handle
└── runtime.rs      C-ABI helper functions JIT'd code calls into
```

It exports three pieces:

| Component | What it does | When it runs |
|-----------|--------------|--------------|
| **Eligibility check** | `is_jit_eligible(program)` walks the bytecode and decides whether the JIT will accept the pattern. | Compile time, every pattern. |
| **Code generator** | `compile_program(program, host)` decodes the bytecode and emits Cranelift IR. | Compile time, for JIT-eligible patterns. |
| **Runtime helpers** | C-ABI functions for char-class testing and word-boundary checks the JIT'd code calls via indirect calls. | Run time, from inside JIT'd functions. |

The pieces are wired into the public `Regex` API through a **4-tier dispatch chain** (described below). Patterns that fall outside the JIT-eligible subset never reach C1 at all — they continue to run on the C2 hybrid or the backtracking VM unchanged.

## Why Cranelift

There are roughly three options for building a Rust-side JIT:

1. **Hand-written x86_64 assembler** (the PCRE2 approach). Maximum control, smallest dependency, no abstraction overhead. But tied to one architecture, requires a separate aarch64 backend, and every new opcode lowering means writing assembly twice.
2. **`dynasm-rs`** — assembler DSL inside Rust source. Lower-level than Cranelift, slightly faster compilation, but still per-architecture and lacks Cranelift's optimizer.
3. **Cranelift** (`cranelift-codegen`, `cranelift-frontend`, `cranelift-jit`). A real compiler IR with an optimizer, multiple ISA backends (x86_64, aarch64, riscv64), and a stable JIT module abstraction. Heavier dependency (~2 MiB closure) but architecture-portable from day one.

RGX picked Cranelift because cross-platform support is part of the value proposition — RGX runs on macOS / Linux / Windows on both x86_64 and aarch64, and a JIT that supported only one architecture would be a step backwards. Cranelift's IR also makes future opcode additions safer: the optimizer catches a lot of mistakes that hand-written assembly would silently propagate.

The decision is **reversible**. The codegen layer in `c1/codegen.rs` is the boundary between RGX-internal data structures and Cranelift IR. If a future version of RGX wants to switch to `dynasm-rs` or hand-rolled assembly, the bytecode walker and opcode-translation table stay the same and only the IR-emission helpers change.

## The JIT-eligible subset

The eligibility check (`c1/codegen.rs::is_jit_eligible`) walks the program once and decides whether C1 can lower it. The check uses two layers: a quick reject from `ProgramFlags`, then a bytecode walk that catches opcodes the flags don't cover. The eligible subset includes:

- **Literals** — single-byte (`Char(b)`) and multi-byte UTF-8 (`Char` with 2-, 3-, or 4-byte payloads, e.g. `é`, `日`, `🦀`).
- **Built-in ASCII char classes** — `\d`, `\D`, `\w`, `\W`, `\s`, `\S` lowered as inline byte tests.
- **Custom char classes** — `[abc]`, `[a-z]`, `[^0-9]`, `[а-я]` lowered as indirect calls to the `rgx_runtime_char_class_match_at` runtime helper (which handles UTF-8 decode + bitmap test + Unicode range search).
- **Anchors** — `\A`, `\z`, `\b`, `\B` lowered as inline position comparisons (`\b` / `\B` use a runtime helper for the word-boundary test).
- **Control flow** — `Split`, `SplitLazy`, `Jump`, `SetAlternative` lowered as conditional / unconditional branches plus the backtrack stack management.
- **All six optimized quantifiers** — greedy `+`, `*`, `?` and lazy `+?`, `*?`, `??` via decoder unfolding into the underlying `Split` + `Jump` shapes.
- **Capture groups 1..=16** — `SaveStart(g)` / `SaveEnd(g)` for any group id, with backtrack-correct undo via per-frame capture snapshots.
- **Top-level alternation tracking** — though top-level alternation patterns themselves are excluded from JIT dispatch because the JIT'd function doesn't track `matched_branch_number`.

The exclusions are everything that needs runtime context the JIT can't model:

- **Backreferences** (`\1`, `\k<name>`) — would require tracking captured substrings and comparing against them at runtime.
- **Lookaround** (`(?=…)`, `(?<=…)`, …) — context-dependent.
- **Recursion / subroutines** (`(?R)`, `(?1)`, `(?&name)`) — needs a call stack the JIT'd function doesn't have.
- **Inline code blocks** — they invoke host code mid-match.
- **Atomic groups + possessive quantifiers** — these are *defined* in terms of backtracking suppression and only have meaning in the existing VM.
- **Backtracking verbs** (`(*COMMIT)`, `(*SKIP)`, …) — same.
- **`\K`** — moves the match start retroactively.
- **More than 16 capture groups** — the per-backtrack-frame capture snapshot grows linearly with the group count, so the JIT caps the stack budget at 16 user groups (`C1_MAX_USER_GROUPS`).

If none of the exclusions appear, the eligibility check returns `true` and `compile_program` lowers the bytecode into Cranelift IR.

## The JIT'd function shape

Every JIT-compiled program produces a single Cranelift function with the same C ABI signature:

```rust,ignore
type JittedFn = unsafe extern "C" fn(
    text: *const u8,
    text_len: usize,
    pos: usize,
    captures_ptr: *mut i64,
    char_classes_ptr: *const u8,
    char_classes_len: usize,
    max_steps: u64,
    max_bt_frames: u64,
) -> isize;
```

The function tests the pattern at *exactly* `pos` (it does not scan — the engine layer handles scanning). It returns:

- `>= 0`: the new position after a successful match (`pos + match_length`).
- `-1`: the pattern did not match at `pos`.
- `-2` (`JIT_LIMIT_EXCEEDED_SENTINEL`): a runtime safety limit (`max_steps` or `max_bt_frames`) was exceeded.

The eight parameters thread the per-call state:

- `text` / `text_len` — the input slice.
- `pos` — the byte position to test the pattern at.
- `captures_ptr` — a `[i64; 2 * (num_groups + 1)]` buffer the JIT writes capture spans into. Each pair `(captures_ptr[2*g], captures_ptr[2*g+1])` is the `(start, end)` of group `g`, with `-1` meaning unset. The caller initialises every slot to `-1` before each call.
- `char_classes_ptr` / `char_classes_len` — the program's `[CompiledCharClass]` slice. Used by `JitOp::CharClass` to call into `rgx_runtime_char_class_match_at`.
- `max_steps` / `max_bt_frames` — user-configured runtime safety limits. `0` means unlimited.

The signature is **frozen** for the C1 v1 release. Adding new helpers or capabilities means extending the runtime helper layer or the codegen layer; the C ABI shape stays the same so existing JIT'd functions and runtime layers stay binary-compatible.

## How the codegen works

`compile_program` is a two-pass walker over the bytecode:

1. **Pass 1**: `decode_program` walks the bytecode and decodes each opcode into a `JitOp` enum variant. The `JitOp` representation decouples the bytecode-walking concern (operand sizes, length prefixes, inline subprograms inside optimized quantifier opcodes) from the codegen concern (semantic opcode kind, lowering shape).

2. **Pass 2**: each `JitOp` is emitted as IR into a dedicated Cranelift basic block. The function's mutable state — `pos`, `bt_top`, `step_counter`, plus the function params — lives in Cranelift `Variable`s rather than block parameters; Cranelift's SSA pass auto-inserts phi nodes wherever multiple predecessors converge with different values.

The IR layout per function looks like this:

```text
entry block
   │
   │  Load function params into Variables
   │  Initialise bt_top, step_counter to 0
   │
   ▼
op_block[0]
   │  emit_step_limit_check
   │  Op-specific IR (load byte, compare, advance pos, …)
   │  On success: jump to op_block[1]
   │  On failure: jump to failure_dispatch_block
   ▼
op_block[1]
   │  …
   ▼
…
   ▼
op_block[N-1]  (Match)
   │  jump to success_block
   │
   ▼
success_block
   │  return current pos
   ▼
fail_block
   │  return -1
   ▼
limit_abort_block
   │  return -2
   ▼
failure_dispatch_block
   │  Pop a backtrack frame, restore captures snapshot,
   │  dispatch to op_blocks[saved_pc] via Switch (br_table)
```

The backtrack stack is **stack-allocated** at function entry: 256 frames × `frame_bytes_for(num_groups)` bytes. Each frame holds `(saved_pc: i64, saved_pos: i64, captures_snapshot: [i64; 2*(num_groups+1)])`. On a `Split` push, the entire captures buffer is snapshotted into the frame; on a backtrack-pop, the snapshot is restored back into the buffer in one shot (an unrolled load/store sequence). This is a simpler equivalent to the per-modification trail approach the design doc originally sketched — both are byte-for-byte equivalent under the differential gate.

The per-op step-counter check (`emit_step_limit_check`) emits an inline increment + comparison at the start of every op:

```text
step_counter += 1
if max_steps != 0 && step_counter > max_steps {
    jump limit_abort_block
}
```

This mirrors the interpreter's main-loop pattern. When `max_steps == 0` (the default), the comparison short-circuits and the loop runs unlimited.

## The runtime helper layer

Cranelift can lower simple ops directly to native instructions (loads, stores, comparisons, branches), but it cannot inline anything that requires Rust runtime support (UTF-8 handling, char class lookups). Those go through a **runtime helper layer** in `c1/runtime.rs`.

Two helpers are wired into the codegen at C1 v1:

```rust,ignore
// Word boundary test for `\b` / `\B`.
pub unsafe extern "C" fn rgx_runtime_word_boundary_test(
    text: *const u8,
    text_len: usize,
    pos: usize,
) -> bool;

// Char class match at the given position. Returns the number of
// bytes consumed (1..=4) on success or 0 on no match.
pub unsafe extern "C" fn rgx_runtime_char_class_match_at(
    text: *const u8,
    text_len: usize,
    pos: usize,
    char_classes_ptr: *const u8,
    char_classes_len: usize,
    class_id: u32,
    negated: u32,
) -> u32;
```

Each helper has a stable C ABI signature. The helpers are registered with the Cranelift `JITBuilder` symbol table in `JitHost::new`; each compiled function imports them via `import_word_boundary_helper` / `import_char_class_helper` and calls them via Cranelift indirect calls.

The helpers are minimal on purpose: they handle the parts of regex matching that need runtime support, and nothing else. Future versions might add helpers for backreferences, lookaround, multi-byte literal scans, or anything else the JIT can't lower inline — each new helper extends the codegen subset without changing the JIT'd function shape.

## The capture trail (per-frame snapshot)

The hardest part of any regex JIT is **capture group correctness under backtracking**. Backtracking can write to a capture slot, then unwind that write when the branch fails. The interpreter handles this with a `Vec<TrailEntry>` log per execution: every save pushes an entry, every backtrack pops entries down to a saved length.

C1 takes a different approach: each backtrack frame stores a **complete snapshot** of the captures buffer. On a `Split` push, the JIT emits an unrolled sequence of loads + stores that copies every capture slot into the trailing bytes of the frame. On a backtrack-pop, the unrolled mirror sequence copies the snapshot back into the buffer. The two approaches are byte-for-byte equivalent under the differential gate; the snapshot scheme is dramatically simpler in codegen terms (no separate trail buffer, no trail-restore loop, no per-`Save` bookkeeping).

The trade-off is that each backtrack frame is bigger: `16 + 16 * (num_groups + 1)` bytes instead of 16 bytes. With 256 frames and the 16-group cap, the bt_stack maxes out at ~72 KiB of function stack. Realistic patterns with 1–3 groups use ~8–16 KiB, comfortably within any thread's stack budget.

The 16-group cap exists *because* of this tradeoff. Patterns with more groups would inflate the stack budget linearly; the eligibility check rejects them and they fall through to the existing dispatch chain.

## Engine dispatch boundary

The public `Regex::is_match`, `Regex::find_first`, and `Regex::find_all` go through a **4-tier dispatch chain**:

```text
                ┌──────────────────────┐
                │  Regex API call       │
                └──────────┬───────────┘
                           │
                           ▼
              ┌────────────────────────────┐
              │ Engine::should_dispatch_   │
              │       to_dfa()?            │
              └─────────┬──────────────────┘
                        │ yes
                        ▼
              ┌────────────────────────────┐
              │  Lazy DFA scan              │
              │  (PrefixScanner accelerated)│
              └─────────┬──────────────────┘
                        │ exhausted or ineligible
                        ▼
              ┌────────────────────────────┐
              │ Engine::should_dispatch_   │
              │       to_c2()? (Pike-VM)    │
              └─────────┬──────────────────┘
                        │ yes (nested quantifier)
                        ▼
              ┌────────────────────────────┐
              │  Sparse-set Pike-VM scan    │
              │  (PrefixScanner accelerated)│
              └─────────┬──────────────────┘
                        │ ineligible
                        ▼
              ┌────────────────────────────┐
              │ Engine::should_use_jit()?  │
              └─────────┬──────────────────┘
                        │ yes
                        ▼
              ┌────────────────────────────┐
              │  JIT'd function call        │
              │  (PrefixScanner accelerated)│
              └─────────┬──────────────────┘
                        │ ineligible
                        ▼
              ┌────────────────────────────┐
              │   Existing backtracking VM  │
              └────────────────────────────┘
```

The JIT tier is the **fourth** in the chain, after DFA and Pike-VM. This is intentional and a deliberate deviation from the original design doc, which sketched JIT *before* Pike-VM. The reason is the same as the Pike-VM tier's positioning: **safety**.

Pike-VM has the "can't hang" property — it's bounded by O(nm) for any input. The existing backtracking VM does not, but RGX's safety limits (`set_max_steps`, `set_max_backtrack_frames`) cap its worst case. The JIT path is *also* a backtracking model — it inherits the same exponential risk for nested-quantifier patterns. So for any pattern Pike-VM can handle, we prefer Pike-VM. The JIT only kicks in for patterns that fall outside *both* DFA and Pike-VM eligibility — typically patterns with anchors, word boundaries, or lazy quantifiers that disqualify them from C2.

The current JIT win is therefore narrower than the design doc anticipated, but it's the right accuracy-first call. The order can be revisited as a performance optimization in a future cutover, but only with strong evidence that the JIT consistently beats Pike-VM on patterns Pike-VM handles.

A few additional gates short-circuit the JIT dispatch:

- **Top-level alternation**: patterns like `cat|dog|bird` are excluded from JIT dispatch because the JIT'd function returns only the match span (`isize`), not the matched branch number. Routing top-level alternation through the JIT would silently drop `MatchResult.matched_branch_number`. The C2 dispatch path excludes top-level alternation for the same reason.
- **Event observers**: if the user has registered a `MatchEvent` observer, the JIT path is skipped because JIT'd code doesn't emit structured events.
- **Recursion depth limit**: if `set_max_recursion_depth` is set, the JIT path is skipped. The JIT doesn't lower the `Call` opcode (recursion is JIT-ineligible), so a recursion limit is meaningless for JIT'd code.
- **Literal finder fast path**: if the pattern compiled to a pure-literal `memchr::memmem::Finder`, the JIT can't beat it. The gate returns `None`.

The `max_steps` and `max_backtrack_frames` limits are NOT exclusions any more — as of step 7, the JIT enforces them inline. Patterns with safety limits set still route through the JIT.

## Differential testing

The C1 differential gate is the same model as C2: every JIT-eligible pattern in a hand-curated corpus is run through both the JIT and the **raw interpreter** (`RegexVM::find_first`, bypassing the public dispatch chain), and the results are asserted byte-for-byte equivalent. The interpreter is the canonical reference; the JIT must match it exactly.

The corpus lives in `rgx-core/src/c1/codegen.rs` under the `step3_*` / `step4b_*` / `step6_*` / `step7_*` test modules. It covers:

- Pure literals (single-byte and multi-byte UTF-8)
- Built-in ASCII char classes and their negated forms
- Custom char classes (positive, negated, ASCII bitmap, Unicode range)
- Anchors (`\A`, `\z`, `^`, `$`, `\b`, `\B`)
- All six optimized quantifier flavours (greedy `+`/`*`/`?` and lazy `+?`/`*?`/`??`)
- Capture groups 1..=16 with backtracking
- Combinations like `\b\d+\b`, `\w+@\w+\.\w+`, `(\w+)@(\w+)\.(\w+)`, `\Ahello\b`
- Runtime safety limits (`max_steps`, `max_bt_frames`)

**Why the raw VM as reference, not the public Regex API**: the public API dispatches through DFA / Pike-VM / JIT / interpreter, and the C2 DFA path implements leftmost-LONGEST semantics for negated char classes (e.g. `[^0-9]` against `"123abc"` returns `(3, 6)` — the longest run of non-digits). The raw VM implements leftmost-FIRST single-char semantics (`(3, 4)` — just the `'a'`). The JIT must match the VM (the design doc §1.0 reference) because that's what the user gets when no other dispatch tier handles the pattern. Comparing against the public API would conflate the JIT's correctness with the DFA's pre-existing semantics.

The differential gate ran clean across 200+ test patterns × 5–8 inputs each at every C1 step. Zero divergences from the VM reference.

## Performance impact

The JIT shines on patterns the existing VM uses by construction (the patterns C2 doesn't handle). For literal-heavy patterns (`memchr` fast path), pure regular patterns (DFA), and nested-quantifier patterns (Pike-VM), the JIT is shadowed by an earlier tier and never runs.

The patterns where the JIT actively wins are those with anchors, word boundaries, or lazy quantifiers that disqualify them from C2. The benchmark sweep at the production cutover quantifies the win on the existing rgx-bench corpus (see [Performance](./performance.md) for the latest numbers). For VM-bound patterns the JIT typically delivers a **2–4x constant-factor speedup** over the existing VM, on top of the per-character savings from the bytecode dispatch removal.

The JIT compile cost is small in practice (~1–10 ms for typical patterns) and happens once at `Regex::compile` time. There is no warm-up — the very first match attempt runs through the JIT'd code at full native speed.

## What's not in C1 yet

Three things on the C1 roadmap are deliberately deferred:

- **Backreference / lookaround / recursion lowering** — these need either runtime helpers that re-enter the interpreter mid-match (the design doc §5.2 sketches this) or fundamentally different codegen. C1 v1 stays out of the way and lets the existing dispatch chain handle them.
- **Tiered execution** (interpret first, JIT after N matches) — C1 v1 is **eager JIT**: every JIT-eligible pattern is JIT-compiled at `Regex::compile` time. A tiered scheme would amortize the compile cost over patterns matched many times, but adds complexity (counters, transition logic, double dispatch) for a marginal win on patterns matched a small number of times. The current eager scheme is simpler and the compile cost is small enough that it's not worth complicating the dispatch.
- **JIT-ahead-of-Pike-VM dispatch** — the original design doc §8 sketched JIT before Pike-VM. C1 v1 ships with JIT after Pike-VM for safety reasons (Pike-VM is the safety net for nested-quantifier patterns where the JIT could blow up exponentially). Re-ordering is a future optimization that requires benchmark evidence the JIT beats Pike-VM on the disputed pattern shapes.

Each is tracked in `docs/BACKLOG.md` as a future RGX improvement. The C1 v1 cutover ships everything that's safe to ship today; further work compounds on top.

## Next: PGEN integration

C1 is the third of three engines RGX runs (interpreter VM, NFA/DFA hybrid, JIT). All three are fed by the same compile pipeline, which is fed by the same parser — and in RGX the parser is an external project called PGEN. Head to [PGEN Integration](./pgen-integration.md) to see how the parser boundary works.
