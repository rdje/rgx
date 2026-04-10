# C1: JIT Compilation — Design Proposal

**Status**: design proposal — step 0 of the C1 phased plan.
**Target**: production-quality JIT compilation for the RGX engine, sequenced as the second of the two tier-0 perf pushes (C2 NFA/DFA hybrid shipped 2026-04-11; C1 begins now).
**Sign-off required before any C1 implementation lands.**

This document is the C1 counterpart to `docs/C2_NFA_DFA_DESIGN.md`. C2 changed the algorithmic class for the no-backtracking subset; C1 multiplies whatever engine ends up running by a constant factor (~5-10x typical) by translating the per-pattern bytecode into native machine code. C1 is sequenced after C2 so the JIT has both engines to target — the existing backtracking VM AND the C2 Pike-VM/DFA paths — and so the C1 wins compound on top of C2's algorithmic-class wins instead of duplicating them.

C1 is the kind of work where the *design* takes longer than any single implementation step. The choices below are not negotiable details — they shape what RGX looks like for years.

---

## 1. Goals and non-goals

### 1.0 Priority order — non-negotiable

**100% accuracy first. Lightning-fast second.** Speed targets in this document only apply *after* correctness is proved. A C1 step that delivers a 10x speedup but produces a single byte-for-byte divergence from the interpreter on a single input does not land. The differential gate (§13) is the merge condition, not a nice-to-have, and "the JIT is faster" is never an argument for shipping a path that produces different results from the interpreter.

This is the SOTA-first preference applied to C1. The same order applied to C2 and is why C2 went from "shipped Pike-VM dispatch" to "rolled back to nested-quantifier gate" between captures `c2-step8-prefix` and `c2-step8-final` — accuracy in the dispatch decision came first; the speed numbers came once the dispatch was provably correct on every test.

Concretely, this means:
- Every step ≥ 4 is gated on **zero divergences** in the differential test harness.
- A step that introduces new JIT codegen for an opcode family must include unit + differential tests for every input shape that opcode family handles.
- A step that touches the dispatch chain must include a runtime check (in debug builds) that the JIT'd path's `MatchResult` equals the interpreter's `MatchResult` slot-for-slot.
- A step that improves performance MUST NOT regress correctness, even by one input.
- If a JIT codegen bug is found at any point, the bug fix takes priority over every other planned step.

### 1.1 Goals

- **Correctness equivalence** with the interpreter. Every pattern + input pair produces byte-for-byte identical results (match span, capture groups, branch number, code-block return values) on JIT'd and interpreted execution. Differential testing is the merge gate, same as C2.
- **5-10x speedup** on VM-bound patterns vs the current interpreter loop, **conditional on correctness**. This is the order of magnitude PCRE2's JIT delivers and is the realistic ceiling for native-code regex execution on x86_64 and aarch64. This target is meaningless without §1.0.
- **Cross-platform**. x86_64 Linux / x86_64 macOS / aarch64 macOS / aarch64 Linux at minimum on the first pass. x86_64 Windows and aarch64 Windows on the second pass. 32-bit targets explicitly out of scope.
- **Graceful fallback**. When JIT compilation fails (unsupported opcode, code-gen error, allocation failure, target architecture not supported), the engine falls back to the interpreter without changing the public API or surprising the user. The fallback is invisible — the same `Regex` object continues to work.
- **Zero overhead when disabled**. Users who don't want the JIT (security-restricted environments, embedded targets, debugging) can disable it via a feature flag or runtime toggle. With JIT off, the binary is the same size and shape as today.
- **Honest debug story**. JIT'd execution must remain debuggable: traces, structured events, capture trail, step counter, all backtracking-verb behaviour. The JIT must not silently disable observability.
- **Two-track docs from day one**. Per CLAUDE.md, every C1 step ships with a Book chapter section AND live continuity doc updates (CHANGES, MEMORY, BACKLOG, RUST_CODEBASE_ANALYSIS). The Book's `internals/jit-compiler.md` chapter is part of step 0's deliverables alongside this design doc.

**Non-goals.**

- **AOT compilation** (ahead-of-time compilation to a separate binary). Out of scope. RGX patterns are typically supplied at runtime; AOT would change the public API.
- **WASM target for the JIT itself**. The JIT generates native x86_64/aarch64 code; it does NOT generate WASM. RGX-WASM continues to use the interpreter — the WASM environment doesn't have native code generation primitives anyway.
- **Inline code blocks (`(?{lang:…})`) JIT'd into native**. Code blocks invoke host scripting languages (Lua, JS, Rhai, native Rust callbacks) and are bound to host runtimes. Their dispatch points stay as VM call-outs even in JIT'd code.
- **Lookaround / backreference JIT'ing on the first pass**. Lookaround and backref opcodes can be JIT'd, but their interaction with backtracking state is complex enough that the first pass leaves them on the interpreter path. The dispatch decision is per-pattern, not per-call.
- **Replacing the interpreter**. The interpreter is permanent. C1 is a fast path for the patterns where it pays off, not a replacement.
- **DFA JIT'ing**. The C2 lazy DFA's hot loop is already two array lookups per byte. A JIT could specialize the inner loop, but the wins are smaller (and harder to measure). DFA JIT'ing is a follow-up after the interpreter JIT lands.

---

## 2. Architectural overview

The C1 compiler is a **backend** for the existing RGX `Program`. It takes the same compiled bytecode the interpreter consumes and emits a native function with the same semantics. The interpreter and the JIT'd function are interchangeable from the engine's point of view.

```text
                      RGX compile pipeline
   Pattern  ─►  PGEN  ─►  AST  ─►  Compiler  ─►  Program (bytecode)
                                                       │
                                  ┌────────────────────┴────────────┐
                                  │                                  │
                                  ▼                                  ▼
                         (existing path)                       (C1 path)
                                  │                                  │
                                  │                                  ▼
                                  │                       JitCompiler::compile
                                  │                                  │
                                  │                                  ▼
                                  │                       Native function pointer
                                  │                       JitFunction { fn(*const ExecContext) -> bool }
                                  │                                  │
                                  ▼                                  ▼
                          RegexVM::execute_at  ◄─────────────  JitFunction.call
                                                  fallback
```

The JIT'd function takes the same `ExecContext` the interpreter takes, performs the same opcode actions, and returns the same boolean (matched / not matched). It can read and write the capture array, the backtrack stack, the trail, the step counter — all the existing per-context state. Crucially: **the ExecContext layout is the public contract between the interpreter and the JIT**. If we change `ExecContext`, we update both code paths.

The 3-tier dispatch chain from C2 (DFA → Pike-VM → existing backtracking VM) gains a fourth axis: each tier can be either *interpreted* or *JIT'd*. The dispatch decision is:

```text
   should_dispatch_to_dfa()           ─►  DFA path (no JIT for now — see §6.4)
   should_dispatch_to_pike_vm()       ─►  Pike-VM path (no JIT in v1; possibly later)
   should_use_jit()                   ─►  JIT'd backtracking VM (C1 v1 target)
   else                               ─►  interpreted backtracking VM (existing)
```

The first version of C1 JIT's the **existing backtracking VM**, not the C2 engines. Rationale: the backtracking VM is where the bulk of patterns end up after the C2 dispatch gates short-circuit (see `book/src/internals/nfa-dfa-engine.md`). JIT'ing the backtracking VM is also strictly more impactful than JIT'ing the C2 engines, because:

1. The backtracking VM's per-opcode dispatch loop is the dominant cost on patterns where matches are common.
2. The C2 DFA already has a 2-lookup-per-byte hot loop that's hard to beat with JIT.
3. The C2 Pike-VM dispatches via sparse-set ops and epsilon-closure, which are NOT a tight bytecode interpreter loop — they're a different execution model where JIT wins are smaller.

C1 v2+ may extend JIT to the C2 paths if benchmarks justify it. C1 v1 stays focused.

---

## 3. Module layout

C1 introduces a new top-level module under `rgx-core/src/` that mirrors how `c2/` is laid out:

```text
rgx-core/src/c1/
├── mod.rs                     Re-exports + module docs
├── codegen.rs                 The Cranelift IR builder; takes Program → cranelift_codegen::ir::Function
├── jit.rs                     JIT host (Cranelift JITModule wrapper, function pointer storage)
├── runtime.rs                 Helper functions called from JIT'd code (memchr_byte, char_class_test, etc.)
├── fallback.rs                Interpreter fallback dispatch when JIT compilation fails
└── tests.rs                   Differential test harness against the interpreter
```

The `c1/` module is **completely standalone** through step 4 (the JIT host and the codegen layer). Engine wiring lands in step 5. This mirrors how C2 was built — every step is independently testable and reverts cleanly if something goes wrong.

The dispatch wiring lives in `engine.rs` alongside the existing C2 dispatch (`should_dispatch_to_dfa`, `should_dispatch_to_c2`). New methods land:
- `Engine::should_use_jit(&self) -> Option<&JitFunction>`
- `Engine::try_jit_find_first(&self, input: &[u8]) -> Option<Option<MatchResult>>`
- `Engine::try_jit_find_all(&self, input: &[u8]) -> Option<Vec<MatchResult>>`
- `Engine::try_jit_is_match(&self, input: &[u8]) -> Option<bool>`

These mirror the `try_dfa_*` and `try_pike_*` family that already exist for C2.

---

## 4. Code generator choice

This is the most consequential decision in the document. The choice constrains everything downstream — which opcodes are easy to compile, which are hard, what the binary size cost is, what the cross-platform story looks like, and how long the project takes.

### 4.1 Options surveyed

| Option | Description | Pros | Cons |
|---|---|---|---|
| **Cranelift** | The compiler backend used by Wasmtime. Already in RGX's dep tree (transitively via `wasmtime`). IR-based, multi-target, production-grade. | Already in dep tree (no new deps). Multi-target out of the box (x86_64, aarch64, riscv64). Production-grade — Bytecode Alliance maintains it. SSA-based IR is comfortable to lower into. Stable interface. | ~1-2MB binary size cost (already paid by wasmtime users; new for users not using WASM features). Compile time per pattern is non-trivial (~1-10ms typical). Less tight code than hand-written assembly. Requires understanding cranelift's IR conventions. |
| **dynasm-rs** | Inline assembler with Rust macros. Used by older Rust JITs. | Tight code generation. Minimal binary cost. Direct control over registers and calling conventions. | Per-target codegen — separate `dynasm` macros for x86_64 and aarch64. Less production-grade. Maintenance overhead grows with each target. Macros are fragile to compiler updates. |
| **Hand-written x86_64 + aarch64 emitters** | Build our own assembler from scratch using `iced-x86` (x86_64) and a hand-rolled aarch64 emitter. | Maximum control. Smallest binary. Easiest to debug at the instruction level. | Largest engineering effort. Per-target maintenance. Cross-platform support is N times the work. Realistically months of work just to reach feature parity with Cranelift. |
| **LLVM** | Use `inkwell` to invoke LLVM. | Best-in-class optimizer. Mature multi-target. | ~30-100MB binary size cost. Compile time per pattern is *seconds*, not milliseconds. Wildly overkill for regex bytecode. Out of scope for this project. |

### 4.2 Decision: Cranelift

**The first version of C1 uses Cranelift.** The rationale is:

1. **Already in the dependency tree.** RGX already pulls in `wasmtime` for the WASM execution backend, which transitively brings `cranelift_codegen` and `cranelift_jit`. Adding C1 with Cranelift adds **zero** new dependencies for users who already have the WASM backend enabled. For users who disable the WASM feature, Cranelift becomes a new direct dependency — but this is a deliberate, narrow addition.
2. **Multi-target out of the box.** A single C1 implementation produces working code on x86_64 and aarch64. The cross-platform validation matrix (§9) is per-target *testing*, not per-target *implementation*.
3. **Compile time is acceptable.** ~1-10ms per pattern is negligible against typical pattern compile times (hundreds of µs to a few ms for the existing PGEN+compiler pipeline). For patterns that are matched against millions of input bytes, the JIT compile cost is amortized to nothing.
4. **The IR is well-documented and SSA-based.** Lowering bytecode opcodes to Cranelift IR is mechanical: each opcode becomes a small basic block. There's no impedance mismatch.
5. **Production-grade maintenance.** Cranelift is maintained by the Bytecode Alliance with funded engineering. Security patches arrive promptly. We don't carry the maintenance burden of a hand-written assembler.

The cost is the ~1-2MB binary size hit for users who weren't already pulling in `wasmtime`. This is mitigated in two ways:
- **C1 is feature-gated.** A new `jit` Cargo feature controls whether `cranelift` is pulled in. Default-on for users who want speed; default-off is also supported.
- **The transitive overlap with wasmtime is real.** Most users running RGX with code blocks will already have wasmtime, so the marginal cost is small.

### 4.3 What we're NOT deciding here

- **Cranelift version pinning.** The first C1 commit will pin a specific Cranelift version compatible with the wasmtime version we already use. Bumping is a separate concern.
- **Custom Cranelift passes.** Cranelift has an extension API for custom passes. We're not using it on the first pass — the default optimizer is plenty.
- **Direct vs indirect calls into runtime helpers.** The first pass uses indirect calls (via function pointers) for runtime helpers like `memchr_byte`; direct calls are an optimization for later if profiling shows them.

---

## 5. What the JIT compiles

The JIT compiles **the existing `Program` bytecode** (`vm.rs::Program`) into a single Cranelift function that, when called, runs the same opcode sequence the interpreter would have run. The JIT'd function and the interpreter consume the same `ExecContext` so they are drop-in interchangeable.

### 5.1 The JIT'd function signature

```rust,ignore
type JittedFn = unsafe extern "C" fn(ctx: *mut ExecContext) -> bool;
```

Single argument: a pointer to the per-call `ExecContext` (already populated with input text, capture slots, backtrack stack, trail, etc.). Returns `true` on match, `false` otherwise. The function reads and mutates `ctx` directly — capture writes go into `ctx.captures`, the position advances `ctx.pos`, backtrack frames push to `ctx.backtrack`.

### 5.2 Per-opcode lowering

Every opcode that the interpreter handles becomes a small basic block in the Cranelift IR:

| Opcode family | Cranelift IR sketch |
|---|---|
| `Char(b)` | `cmp pos, len; jeq fail; load text[pos]; cmp eq b; jne fail; inc pos` |
| `CharClass(id)` | indirect call to `runtime::char_class_test(class_id, byte)`; jne fail; inc pos |
| `DigitAscii` / `WordAscii` / `SpaceAscii` | inline byte-class test (one comparison + branch); jne fail; inc pos |
| `Split(a, b)` | push backtrack frame {pc=b, pos=ctx.pos, trail_len=ctx.trail.len()}; jmp a |
| `Jump(target)` | unconditional jmp to target |
| `Match` | return true |
| `SaveStart(g)` / `SaveEnd(g)` | trail-push {group=g, slot=start/end, prev=ctx.captures[g].start/end}; store ctx.pos into ctx.captures[g].start/end |
| `Backtrack` | pop backtrack frame; truncate trail; jmp pc |
| `WordBoundary` / `NonWordBoundary` | indirect call to `runtime::word_boundary_test(text, pos)`; jne fail |
| `StartText` / `EndTextOrNL` / `EndText` | inline pos comparison |
| `Char` (multi-byte) | indirect call to `runtime::match_multibyte_char(text, pos, ch_bytes_addr, ch_len)` |
| **Lookahead** / **Lookbehind** | indirect call to `runtime::run_subprogram(ctx, subprogram_id)`; behavior matches the interpreter |
| **Backreference** | indirect call to `runtime::compare_capture(ctx, group_id, text)` |
| **Recursion** / **CallCode** / **Conditional** / backtracking verbs | indirect call to interpreter helpers (these stay on the interpreter path even in JIT'd code) |

The first pass aggressively inlines the simple opcodes (`Char`, `DigitAscii`, `WordAscii`, `SpaceAscii`, `Split`, `Jump`, `Match`, `SaveStart`/`SaveEnd`, `Backtrack`, `StartText`/`EndText`, anchors). The complex opcodes (lookaround, backreference, recursion, code blocks, backtracking verbs) call out to interpreter helpers via indirect calls. This means **JIT'd execution still runs through the interpreter for the hard cases**, but the per-opcode dispatch overhead is gone for the easy cases.

### 5.3 Patterns the JIT will NOT compile (v1)

The first version refuses to JIT patterns containing:
- **Backreferences** (`\1`, `\k<name>`, `\g{1}`) — the runtime helper exists but the JIT path is not the right place to optimize this on the first pass.
- **Lookahead / lookbehind** — same reasoning. Can be added later.
- **Recursion / subroutines** (`(?R)`, `(?1)`, `(?&name)`) — fundamentally interpreter-bound on the first pass.
- **Inline code blocks** (`(?{lang:…})`, native callbacks) — these dispatch to host scripting runtimes.
- **Backtracking verbs** with non-trivial state interactions (`(*COMMIT)`, `(*SKIP)`, `(*PRUNE)`, `(*MARK:name)`, `(*ACCEPT)`).
- **Conditionals** (`(?(cond)yes|no)`).
- **Atomic groups** (`(?>…)`) and **possessive quantifiers** (`a*+`, `a++`) — these need careful backtrack-stack manipulation.

The dispatch decision (`should_use_jit`) checks the program's `flags` and `classification` (already populated by the existing compiler) and returns `None` if any of the above are present. Patterns that don't qualify run on the existing interpreter unchanged.

### 5.4 Tiered execution

C1 v1 is **eager JIT**: every JIT-eligible pattern is JIT-compiled at `Regex::compile` time, before the first match attempt. Rationale:
- Pattern compile time is dominated by PGEN parsing and the existing optimizer pass; adding 1-10ms for Cranelift codegen is negligible.
- Eager JIT means the first match attempt is fast — no warm-up.
- A tiered "interpret first, JIT after N matches" scheme adds complexity (counters, transition logic, double dispatch) for a marginal win on patterns that are matched a small number of times.

If profiling shows the JIT compile cost matters for short-lived patterns, a tiered scheme can be added in C1 v2.

---

## 6. Capture handling

The hardest part of any regex JIT is capture group correctness. Backtracking can write to a capture slot, then unwind that write when the branch fails. The interpreter handles this with the trail (per `vm.rs`), and the JIT must produce identical trail behaviour.

### 6.1 The capture trail in JIT'd code

The interpreter's `TrailEntry` looks like:

```rust,ignore
struct TrailEntry {
    group: u32,
    which: SlotKind,    // start or end
    previous: Option<usize>,
}
```

Every `SaveStart(g)` / `SaveEnd(g)` opcode pushes a `TrailEntry` containing the previous value of the slot. Every backtrack pops trail entries (down to the saved `trail_len` from the `Frame`) and restores each slot.

The JIT'd `SaveStart(g)` and `SaveEnd(g)` lower to:

```text
load  prev = ctx.captures[g].start
store TrailEntry { group: g, which: SlotKind::Start, previous: prev }
                                   into ctx.trail[ctx.trail_len]
inc   ctx.trail_len
store ctx.pos into ctx.captures[g].start
```

The lowering is mechanical. The Cranelift IR has `load`/`store` instructions, struct field offsets are constants known at JIT-compile time, and the trail buffer is a flat `Vec<TrailEntry>` accessed by index.

The JIT'd `Backtrack` opcode lowers to a loop:

```text
loop:
  cmp ctx.trail_len, frame.saved_trail_len
  jeq done
  dec ctx.trail_len
  load entry = ctx.trail[ctx.trail_len]
  switch entry.which:
    case Start: store entry.previous into ctx.captures[entry.group].start
    case End:   store entry.previous into ctx.captures[entry.group].end
  jmp loop
done:
  pop_backtrack_frame
  jmp frame.pc
```

This is a tight inner loop that Cranelift can vectorize (or at least branchless-ify) on x86_64.

### 6.2 The differential test

Every JIT'd pattern's capture behaviour is tested against the interpreter on a corpus of inputs. The differential gate (§13) asserts that for every classifier-positive input, the JIT'd `MatchResult.groups` equals the interpreter's `MatchResult.groups` slot-for-slot, including unmatched groups (which are `None` on both sides).

---

## 7. Runtime helper layer

Cranelift can lower simple ops directly to native instructions (loads, stores, comparisons, branches), but it cannot inline anything that requires Rust runtime support (UTF-8 handling, char class lookups, callback dispatch). Those go through a **runtime helper layer** in `c1/runtime.rs`.

### 7.1 Helper functions

```rust,ignore
// c1/runtime.rs

#[no_mangle]
pub extern "C" fn rgx_runtime_char_class_test(
    char_classes: *const CompiledCharClass,
    class_id: u32,
    byte: u8,
) -> bool { /* … */ }

#[no_mangle]
pub extern "C" fn rgx_runtime_word_boundary_test(
    text: *const u8,
    text_len: usize,
    pos: usize,
) -> bool { /* … */ }

#[no_mangle]
pub extern "C" fn rgx_runtime_match_multibyte_char(
    text: *const u8,
    text_len: usize,
    pos: usize,
    expected: *const u8,
    expected_len: usize,
) -> bool { /* … */ }

#[no_mangle]
pub extern "C" fn rgx_runtime_compare_capture(
    ctx: *mut ExecContext,
    group_id: u32,
) -> bool { /* … */ }

#[no_mangle]
pub extern "C" fn rgx_runtime_run_subprogram(
    ctx: *mut ExecContext,
    subprogram_id: u32,
) -> bool { /* … */ }
```

Each helper has a stable C ABI signature. Cranelift emits indirect calls through function pointers registered in the JIT module's symbol table.

### 7.2 The helper interface IS a public contract

Once the helper signatures are defined, they cannot change without a coordinated update to both the JIT codegen and the runtime layer. Changes are version-locked: a JIT'd function compiled against helper signatures vN cannot be called from a runtime layer of version vN+1.

This is mitigated by versioning the JIT module: each `Engine` carries the JIT-version it was compiled against, and any mismatch falls back to the interpreter. In practice this only matters across major RGX releases.

### 7.3 No JIT'd allocation

The JIT'd function never allocates memory. All allocations (the trail, the backtrack stack, capture slots) happen in `ExecContext`, which is set up by the caller before invoking the JIT'd function. Cranelift cannot easily lower Rust allocator calls — and attempting it would couple the JIT to a specific allocator implementation. Instead, the JIT'd function reuses pre-allocated buffers from `ExecContext` and bails (returning false to fall back to the interpreter) if the buffers overflow.

---

## 8. Engine dispatch boundary

The JIT becomes the new top tier of the dispatch chain when it's available:

```text
                    Regex API call
                          │
                          ▼
            ┌─────────────────────────────┐
            │  should_dispatch_to_dfa?    │
            └──────────┬──────────────────┘
                       │ no
                       ▼
            ┌─────────────────────────────┐
            │  should_use_jit?            │  ◄── NEW in C1
            │  (JIT'd backtracking VM)    │
            └──────────┬──────────────────┘
                       │ no
                       ▼
            ┌─────────────────────────────┐
            │  should_dispatch_to_c2?     │
            │  (Pike-VM, nested-quant)    │
            └──────────┬──────────────────┘
                       │ no
                       ▼
            ┌─────────────────────────────┐
            │  Existing backtracking VM    │
            │  (interpreter)              │
            └─────────────────────────────┘
```

`should_use_jit` returns `Some(&JitFunction)` iff:
- The pattern was JIT-compiled at `Regex::compile` time AND succeeded.
- The runtime feature gates are clear: no event observer registered (the JIT can emit events but the v1 path simplifies to "no observer = JIT"), no runtime safety limits set (`max_steps` etc — same gating as C2 because the JIT'd code uses the same limit-check macros).
- The JIT was not disabled via the `jit-disabled` runtime toggle.

Note that the JIT tier sits **between** the DFA tier and the Pike-VM tier. The DFA always wins for DFA-eligible patterns (it's strictly faster). The JIT tier handles everything the DFA can't but the JIT can. The Pike-VM tier handles nested-quantifier patterns the JIT didn't take. The interpreter is the final fallback.

### 8.1 Why JIT is NOT inside the DFA path

The DFA's hot loop is two array lookups per byte. JIT'ing this would add Cranelift codegen complexity for a marginal win. The DFA is already faster than JIT'd interpreter execution on the patterns it handles, so there's nothing to gain on the first pass.

### 8.2 Why JIT is BEFORE the Pike-VM path

The Pike-VM path is gated on the nested-quantifier heuristic — it only fires for patterns at risk of catastrophic backtracking. For those patterns, the Pike-VM's O(nm) bound is the primary value, NOT raw speed. JIT'ing the existing backtracking VM and then falling through to Pike-VM for nested-quantifier patterns gives the user the best of both: speed for the common case, safety for the dangerous case.

If the user has set `set_max_steps(N)`, both JIT and Pike-VM paths are skipped (same as C2 v1). The interpreter handles step counting natively, and the JIT'd path's step-count check is structurally identical but uses Cranelift's branch instructions instead of the interpreter's `if`.

---

## 9. Cross-platform validation matrix

C1 must work on every target the rest of RGX works on. The validation matrix:

| Target triple | OS | Arch | v1 priority | Validation |
|---|---|---|---|---|
| `x86_64-unknown-linux-gnu` | Linux | x86_64 | **P0** | Full differential test suite + benchmarks. Primary CI target. |
| `x86_64-apple-darwin` | macOS | x86_64 | **P0** | Full differential test suite. Validated locally. |
| `aarch64-apple-darwin` | macOS | aarch64 | **P0** | Full differential test suite. Validated locally (Apple Silicon dev machine). |
| `aarch64-unknown-linux-gnu` | Linux | aarch64 | **P1** | Full differential test suite. CI target if available. |
| `x86_64-pc-windows-msvc` | Windows | x86_64 | **P1** | Full differential test suite. CI target. |
| `aarch64-pc-windows-msvc` | Windows | aarch64 | **P2** | Smoke test only initially. Full test in v2. |
| `wasm32-unknown-unknown` | (browser) | wasm | **N/A** | JIT disabled. RGX-WASM uses the interpreter only. |
| `wasm32-wasi` | (CLI) | wasm | **N/A** | Same. JIT disabled. |
| 32-bit targets (i686, armv7) | various | various | **N/A** | JIT disabled. Out of scope. |

The differential test harness from C2 generalizes: every C1-eligible pattern in the existing test suite gets executed twice (interpreted + JIT'd) and the results compared. This catches per-architecture codegen bugs, calling-convention bugs, and any subtle ABI mismatches between the JIT'd code and the runtime helper layer.

CI runs the full matrix on every push. The cross-platform gate is **zero failures across all P0 targets**. P1 targets gate the major release; P2 targets are best-effort.

---

## 10. Module layout (re-stated for clarity)

```text
rgx-core/src/c1/
├── mod.rs              Re-exports + module docs + JIT enablement detection
├── codegen.rs          Cranelift IR builder; one function per opcode lowering
├── jit.rs              JITModule wrapper; function pointer storage on Engine
├── runtime.rs          Runtime helper functions called from JIT'd code
├── fallback.rs         Fallback dispatch when JIT compilation fails
└── tests.rs            Differential test harness against the interpreter
```

`engine.rs` gains:
- `Engine::jit_function: Option<JitFunction>` field
- `Engine::should_use_jit() -> Option<&JitFunction>` accessor
- `Engine::try_jit_is_match(input: &[u8]) -> Option<bool>` dispatch method
- `Engine::try_jit_find_first(input: &[u8]) -> Option<Option<MatchResult>>` dispatch method
- `Engine::try_jit_find_all(input: &[u8]) -> Option<Vec<MatchResult>>` dispatch method

`lib.rs` gains a 4-tier dispatch chain in `Regex::is_match` / `find_first` / `find_all`:
```rust,ignore
let result = if let Some(dfa_result) = self.engine.try_dfa_*(text.as_bytes()) {
    dfa_result
} else if let Some(jit_result) = self.engine.try_jit_*(text.as_bytes()) {
    jit_result
} else if let Some(pike_result) = self.engine.try_pike_*(text.as_bytes()) {
    pike_result
} else {
    self.engine.find_*(text.as_bytes())
};
```

`vm.rs::Program` gains a `jit_eligible: bool` field populated at compile time.

---

## 11. Feature gating

C1 lives behind a Cargo feature flag `jit`, default-on for the `rgx-core` crate but cleanly disable-able for embedded / size-constrained / sandbox targets.

```toml
[features]
default = ["jit", "wasm-runtime"]
jit = ["dep:cranelift-codegen", "dep:cranelift-frontend", "dep:cranelift-jit"]
```

When `jit` is disabled:
- The `c1/` module is not compiled.
- `Engine::should_use_jit` always returns `None`.
- `Engine::try_jit_*` methods always return `None`.
- The dispatch chain becomes `DFA → Pike-VM → interpreter` (the C2 chain unchanged).
- Cranelift is not pulled in, saving ~1-2MB binary size.

The feature toggle is verified by an extra CI job that builds with `--no-default-features --features wasm-runtime` (and the equivalent for `--no-default-features` alone) and runs the full test suite. The interpreter must remain a complete implementation; the JIT is purely additive.

---

## 12. What the existing path does NOT lose

C1 must preserve every property the existing engine ships:

- **Backtracking-verb semantics** (`(*COMMIT)`, `(*SKIP)`, `(*PRUNE)`, `(*MARK)`, `(*ACCEPT)`, `(*THEN)`) — all dispatched through interpreter helpers in JIT'd code.
- **Capture trail correctness** — the JIT'd `Backtrack` op produces identical trail-restore behaviour. Differential gate verifies.
- **Step-limit enforcement** — `set_max_steps` short-circuits JIT dispatch (same as C2). Interpreter handles step counting.
- **Event emission** — `set_event_observer` short-circuits JIT dispatch. Interpreter emits events.
- **Inline code blocks** — patterns with `(?{lua:…})`, `(?{js:…})`, `(?{native:…})`, `(?{rhai:…})` are NOT JIT-eligible. Run on the interpreter.
- **Suspendable matching** — `find_first_suspendable` continues to use the interpreter regardless of JIT availability. Resume semantics depend on the interpreter's `MatchContinuation` type, which the JIT can't easily produce.
- **Async / streaming** — same. The async path stays on the interpreter.
- **Lookaround** — runs on interpreter helpers in JIT'd code, OR pattern is JIT-ineligible if lookaround is used in a way the helper can't reach.

The principle is: **JIT or interpreter is invisible to the user**. Same `Regex::compile`, same `find_first`, same `MatchResult`. The JIT is a performance optimization, not a feature toggle.

---

## 13. Differential testing strategy

The differential gate from C2 generalizes. Every JIT-eligible pattern in the existing 902-test rgx-core suite is executed both interpreted and JIT'd, and the results compared:

```rust,ignore
fn assert_jit_interpreter_equivalence(pattern: &str, input: &str) {
    let regex = Regex::compile(pattern).unwrap();
    let interpreted = regex.find_first_interpreted(input);
    let jitted = regex.find_first_jit(input);
    assert_eq!(interpreted, jitted, "JIT/interp divergence on `{pattern}` x `{input}`");
}
```

(`find_first_interpreted` and `find_first_jit` are debug-only test hooks that bypass the dispatch chain.)

The comparison covers:
- Match span (`.start`, `.end`)
- Every capture slot (`Vec<Option<(usize, usize)>>`) including unmatched groups
- `matched_branch_number` for top-level alternations
- `code_result` for patterns with code blocks (must be identical because both paths run the same interpreter helpers)
- Boolean is_match results
- Find-all match-list contents in left-to-right order

Plus a new `tests/c1_jit_differential.rs` file with a hand-curated corpus of patterns specifically designed to stress JIT codegen:
- Simple literals (`abc`)
- Char classes (`[a-z]+`, `\d{3}`)
- Alternations (`cat|dog|bird`)
- Quantifiers including counted (`a{3,5}`)
- Anchors (`^foo$`, `\bword\b`)
- Capture groups including nested (`((a)b)`)
- Complex realistic patterns (email, URL, ISO date)
- Patterns at the edge of JIT eligibility (lookaround, backref, recursion — must fall back to interpreter)

The differential gate is **active from step 4 onward** (the first step where JIT'd code can actually execute against inputs).

---

## 14. Phased implementation plan

Each step is its own commit. Each step is gated on differential tests passing. Each step ships production-quality code per the SOTA-first preference. The plan mirrors C2's phased structure.

| Step | Module(s) added or modified | Differential gate |
|---|---|---|
| **0. This design proposal** | `docs/C1_JIT_COMPILATION_DESIGN.md` (this file), CHANGES.md, MEMORY.md, BACKLOG.md, README.md doc index, new `book/src/internals/jit-compiler.md` chapter (placeholder section "JIT design proposal landed") | N/A — doc only |
| **1. JIT host plumbing** | `c1/mod.rs`, `c1/jit.rs`, `c1/runtime.rs` skeleton, Cargo feature flag wiring, `cranelift_codegen` / `cranelift_frontend` / `cranelift_jit` as gated dependencies, smoke test that builds an empty Cranelift function and calls it | Standalone module compiles + smoke-test passes; does NOT touch the engine |
| **2. JIT eligibility check** | `c1/codegen.rs::is_jit_eligible(program)` walks the bytecode and decides if the JIT will accept the pattern. New `Program::jit_eligible: bool` field populated at compile time. Unit tests against the existing test corpus | Eligibility output verified against a hand-curated truth table |
| **3. Codegen for the easy opcodes** | `c1/codegen.rs::compile_program` translates the easy opcode subset (`Char`, `DigitAscii`, `WordAscii`, `SpaceAscii`, `Split`, `Jump`, `Match`, `SaveStart`, `SaveEnd`, `Backtrack`, `StartText`, `EndText`, `WordBoundary`, `NonWordBoundary`) into Cranelift IR. JIT-compiles to a function pointer. Unit tests on a small set of literal-only patterns | Standalone correctness — JIT'd literal patterns produce match/no-match correctly |
| **4. Capture trail in JIT'd code** | Capture writes via `SaveStart`/`SaveEnd`, trail entries pushed correctly, backtrack restoration verified. Differential test harness lands here. | **Differential gate active from this step onward.** Every JIT-eligible pattern in the test suite produces identical results to the interpreter. |
| **5. Engine dispatch wiring** | `engine.rs` gains `jit_function`, `should_use_jit`, `try_jit_is_match`, `try_jit_find_first`, `try_jit_find_all`. `lib.rs` 4-tier dispatch chain. Public API unchanged. | Differential gate; existing 902-test rgx-core suite runs through the JIT path for eligible patterns |
| **6. CharClass + multi-byte literal support** | `c1/codegen.rs` lowers `CharClass(id)` via runtime helper indirect call; multi-byte `Char` lowers via the multibyte runtime helper. Updates eligibility check. | Differential gate; benchmark capture |
| **7. Runtime safety helpers** | Step counter check, recursion depth check, backtrack frame limit check — all lowered as inline branches in JIT'd code (using the same atomic counters the interpreter uses). | Differential gate; the existing safety-limits test suite runs through the JIT path |
| **8. Production cutover, benchmarks, Book chapter** | Final dispatch wiring, full benchmark sweep with label-paired captures, the new `book/src/internals/jit-compiler.md` chapter expanded to its full form (per the two-track docs rule), `RUST_CODEBASE_ANALYSIS.md` updated to reflect C1 as a shipped engine | Differential gate; benchmark targets met (§15); zero regressions on the existing 902 tests |
| **9. (optional) Cross-platform CI matrix expansion** | Add aarch64 Linux + x86_64 Windows + aarch64 Windows CI jobs. Each runs the full differential gate. | All P0+P1 targets green |

**Estimated commit count**: 9-12 commits for the happy path. Realistic: 15-20 commits including the always-encountered architectural adjustments.

**Estimated timeline**: multi-week. Comparable to C2, possibly slightly longer because the cross-platform validation matrix is explicit.

---

## 15. Benchmark strategy

C1's success criterion is **measurable speedup vs the interpreter on the existing rgx-bench corpus**. The trend capture infrastructure already exists (`rgx-bench/src/bin/trend_capture.rs`) and was used to validate C2 step 8. C1 reuses it.

### 15.1 Targets

- **literal_simple**: should be roughly equivalent to the existing `memmem::Finder` fast path (which already bypasses the VM). The JIT may or may not engage here — the dispatch decision should be neutral (literal_finder gate stays active).
- **email_basic** (`\b\w+@\w+\.\w+\b`): currently runs on the interpreter at ~744ns find_first 1K. Target: ~150ns find_first 1K (5x speedup).
- **capture_groups** (`(\d{4})-(\d{2})-(\d{2})`): currently routed to the C2 DFA at ~283ns find_first 1K. The JIT does NOT touch this — DFA stays. Target: no regression.
- **A new C1-specific corpus** in `rgx-bench/src/lib.rs` with patterns the C2 DFA can't handle but the JIT can:
  - `\bERROR\s+\d+:\s*[\w\s]+` (log-line pattern with anchor and char classes)
  - `(?:GET|POST|PUT|DELETE)\s+/api/v\d+/\w+` (HTTP route pattern)
  - `^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}` (anchored ISO timestamp)
  - `\b[A-Z]{2,}_[A-Z0-9]+\b` (constant-name pattern)
  - Each measured on 1K, 10K, 100K input sizes for find_first and find_all

### 15.2 Trend capture

`cargo run --release --bin trend_capture -- --mode quick --label c1-step8-final --compare-against label:f708f7c` captures the post-cutover state and compares against the pre-C2 baseline. The comparison is not just C1-vs-interpreter — it's the full post-C1 dispatch chain (DFA → JIT → Pike-VM → interpreter) vs the original interpreter-only state.

A successful C1 cutover delivers:
- **5-10x speedup on JIT-eligible patterns** vs the interpreter (the C1 benchmark corpus).
- **No regressions on DFA-dispatched patterns** vs C2-final.
- **No regressions on pure-literal patterns** vs C2-final (literal_finder gate preserved).
- **No regressions on JIT-ineligible patterns** vs C2-final (interpreter unchanged).

If any of these regress, the cutover doesn't land for the affected workload — the dispatch decision can be tightened on a per-pattern-shape basis.

---

## 16. Open architectural questions

These are decisions the design doc does NOT make. They need answers either before step 1 starts or during the relevant implementation step.

| Question | When it needs an answer | My current lean |
|---|---|---|
| **Q1.** Should the JIT'd function take `*mut ExecContext` directly, or should it use a smaller "JIT view" struct that only exposes the fields the JIT touches? | Step 3 | Direct `*mut ExecContext`. Simpler, no struct copying, the field offsets are stable enough. |
| **Q2.** Should runtime helpers be `extern "C" fn` (C ABI) or `extern "Rust" fn` (Rust ABI)? | Step 1 | C ABI. Stable. Cranelift handles C ABI calling conventions cleanly across all targets. |
| **Q3.** Should JIT'd code generate trace events when `trace_log` is enabled, or fall back to the interpreter for tracing builds? | Step 5 | Fall back. The JIT path is for production speed; tracing wants the interpreter's instrumentation. |
| **Q4.** Should the JIT handle the literal-finder fast path itself, or should the dispatch stay as "literal_finder before JIT"? | Step 5 | Dispatch stays. The literal_finder is unbeatable; nothing the JIT can do for pure literals helps. |
| **Q5.** Should the JIT support `set_max_steps` natively (inline branch) or short-circuit dispatch when limits are set? | Step 7 | Short-circuit on the first pass; native inline check in step 7 if benchmarks justify it. |
| **Q6.** Should we cache JIT'd functions across `Regex::compile` calls (a process-wide JIT cache like the existing `RegexCache`)? | Step 8 | Yes, but as a follow-up. The compile cost is small enough that re-JIT'ing per-Regex is acceptable for v1. |
| **Q7.** Should the JIT fall back to interpreter mid-match if it hits a runtime helper that returns "I can't handle this"? | Step 5 | No mid-match fallback. The eligibility check at compile time is comprehensive — if the JIT accepts a pattern, it commits to handling every input. Mid-match fallback is fragile (state divergence). |
| **Q8.** Should we expose JIT presence as a public introspection method (`regex.is_jit_compiled() -> bool`)? | Step 8 | Yes, alongside `regex.uses_c2()` from C2 step 8. Useful for users and benchmarks. |
| **Q9.** Should `RegexSet` use the JIT for its individual patterns? | Step 8 | Yes, automatically — `RegexSet` builds individual `Regex` objects which go through the normal compile pipeline including JIT compilation. No special integration needed. |
| **Q10.** Should JIT'd functions be unloaded when the `Engine` is dropped, or pooled in a process-wide JIT module? | Step 1 | Per-Engine JITModule. Simpler ownership. JIT module lifetime is tied to Regex lifetime. Process-wide pooling is a future memory optimization if profiling shows it matters. |
| **Q11.** Should the JIT generate position-independent code (PIC)? | Step 3 | Yes — Cranelift defaults to PIC and there's no reason to override. PIC interacts cleanly with security policies (W^X, PaX) and shared library loading. |
| **Q12.** What's the maximum bytecode size a single pattern can have before we refuse to JIT it? | Step 2 | 64KB of source bytecode. Larger patterns are unusual and have diminishing JIT returns. Limit can be raised in v2. |

---

## 17. Risks and mitigations

| Risk | Mitigation |
|---|---|
| **JIT codegen bug produces incorrect output** | Differential test suite against the interpreter is the merge gate. Every step ≥ 4 is gated on zero divergence. |
| **JIT'd code crashes (segfault, illegal instruction)** | Same — differential gate runs every test in both modes; a crash is a hard failure. Plus, fuzzing target in `fuzz/` for JIT'd code that asserts non-crash on every input. |
| **Cranelift version incompatibility with wasmtime** | Pin both to compatible versions; CI catches incompatibilities at build time. |
| **Binary size grows beyond acceptable** | Feature-gate behind `jit` Cargo feature; users can opt out. CI tracks binary size deltas. |
| **JIT compile time is noticeable on short-lived patterns** | Tiered execution (interpret first, JIT after N matches) is a known follow-up. v1 pays the eager-JIT cost which is small in absolute terms (~1-10ms). |
| **Cross-platform codegen bug shows up only on aarch64 / Windows** | CI matrix runs the full differential gate on every P0+P1 target. P2 targets (aarch64 Windows) get smoke tests in v1 and full coverage in v2. |
| **Calling convention bugs at the JIT/runtime boundary** | All runtime helpers use stable C ABI; Cranelift handles the calling convention details. Cross-target tests catch ABI bugs. |
| **JIT'd code interacts badly with security policies (SELinux, App Sandbox, hardened malloc)** | Document the JIT memory allocation behaviour clearly; expose a runtime toggle to disable JIT entirely (`Regex::set_jit_enabled(false)` or similar). Users in restricted environments use the interpreter unchanged. |
| **Memory permission errors (W^X enforcement)** | Cranelift handles RWX → RX transitions correctly via `cranelift-jit`'s `Module::finalize_definitions`. Verified by smoke tests on each target. |
| **Branch prediction misses in JIT'd code that the interpreter doesn't have** | Cranelift's optimizer is reasonably good. If profiling shows specific patterns suffer, custom Cranelift passes are an option for v2. |
| **JIT cache thrashing if every pattern compiles a fresh function** | v1 ships per-Engine JIT modules; pattern caching across Engines is a follow-up if profiling shows it matters. |
| **The complexity of the JIT path makes engine-internal refactors harder** | Mitigated by keeping `c1/` standalone and well-tested. Engine refactors that touch dispatch boundaries also touch C1 dispatch — the differential test catches divergence. |

---

## 18. Out of scope for this document (and this project phase)

These are explicitly NOT addressed here:

- **AOT compilation** (compile patterns to a separate binary, link them in). Out of scope.
- **DFA JIT'ing**. The C2 lazy DFA hot loop is already efficient enough that JIT wins are marginal. Possibly v2 follow-up.
- **Pike-VM JIT'ing**. Same.
- **Multi-pattern JIT** (compile a `RegexSet` into a single JIT'd function). Speculative.
- **Tiered execution** (interpret first, JIT after N matches). Possible v2 follow-up if compile time shows up.
- **Custom Cranelift optimization passes**. Possible v2 if profiling justifies.
- **Symbolic debugging through JIT'd code** (gdb / lldb integration). The JIT will generate frame-pointer-friendly code so backtraces work, but full source-level debugging is out of scope.
- **WASM / browser JIT**. Different execution model entirely. RGX-WASM keeps the interpreter.
- **32-bit target support**. JIT disabled on 32-bit targets.
- **Hot patching / function swapping at runtime**. The JIT'd function pointer is set once at compile time and lives for the lifetime of the Engine.

---

## 19. References

- The PCRE2 JIT documentation — `https://www.pcre.org/current/doc/html/pcre2jit.html`. The design that defines the state of the art for backtracking-engine JIT.
- The Cranelift book — `https://cranelift.dev/`. The compiler backend RGX uses.
- Wasmtime's use of Cranelift — `https://github.com/bytecodealliance/wasmtime/tree/main/cranelift`. The largest production user of Cranelift; reading their integration is the best way to learn idiomatic Cranelift.
- Russ Cox, "Regular Expression Matching: the Virtual Machine Approach" (2009) — relevant background for the bytecode model the JIT compiles. https://swtch.com/~rsc/regexp/regexp2.html
- Andrew Gallant's `regex-automata` crate — `https://github.com/rust-lang/regex/tree/master/regex-automata`. Production Rust regex engine; their codegen choices are informative even though they don't JIT.
- The `dynasm-rs` crate — `https://github.com/CensoredUsername/dynasm-rs`. The alternative we considered and rejected.
- The existing RGX backtracking VM in `rgx-core/src/vm.rs` — the source of truth for differential testing.
- The C2 design proposal `docs/C2_NFA_DFA_DESIGN.md` — the structural template this document follows.

---

## 20. Sign-off

This document blocks all C1 implementation work until the user signs off.

**Reviewer**: Richard DJE

**Sign-off**: ☐ Approved as-is &nbsp;&nbsp; ☐ Approved with the following changes &nbsp;&nbsp; ☐ Needs revision

**Notes**:
