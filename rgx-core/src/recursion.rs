//! Stack-safe recursion guards for the compile pipeline.
//!
//! # Why this module exists
//!
//! `Regex::compile` descends recursively once (often several times)
//! per nesting level of the pattern:
//!
//! 1. PGEN's JSON AST dump is deserialized — already made stack-safe
//!    with [`serde_stacker`] at the `parsing.rs` deserialize site.
//! 2. The typed-AST walker in `parsing.rs` (`convert_typed_*`) walks
//!    that deeply-nested tree.
//! 3. The compiler (`compiler.rs`) runs ~11 recursive full-tree
//!    AST→AST passes plus recursive bytecode codegen.
//!
//! Only step 1 was protected. Steps 2 and 3 recursed on the
//! caller's thread stack with no growth, so a sufficiently nested
//! pattern (e.g. `(((…(a)*…)*)*`) would overflow the stack and
//! **abort the host process** (`SIGABRT`) instead of returning a
//! clean error. The overflow depth depended on the thread's stack
//! size and on whether `serde_stacker`'s heuristic happened to fire,
//! so it manifested *nondeterministically* — observed aborting at
//! nesting depth 16 on a default `libtest` thread while surviving
//! depths 20–40.
//!
//! For a library that advertises DoS-safety (`set_max_steps`,
//! `set_max_backtrack_frames`, `set_max_recursion_depth` for the
//! *runtime*), aborting the host process at *compile* time on
//! adversarial input is the missing compile-time analog. This
//! module supplies the two pieces that close it:
//!
//! - [`grow_stack`] — wrap each recursive descent so the stack
//!   grows on demand instead of overflowing. Same mechanism
//!   `serde_stacker` already applies to deserialization, applied
//!   consistently across the whole compile recursion.
//! - [`MAX_NESTING_DEPTH`] / [`too_deeply_nested`] — a deterministic
//!   upper bound that returns a clean [`crate::error::RgxError`]
//!   before the recursion (and the stack it would grow) becomes
//!   unbounded on pathological input.
//!
//! [`serde_stacker`]: https://docs.rs/serde_stacker

use crate::error::RgxError;

/// Maximum pattern nesting depth accepted by the compile pipeline.
///
/// Chosen to be **generous** and **bounded**:
///
/// - PCRE2's default parenthesis-nesting limit
///   (`PCRE2_CONFIG_PARENSLIMIT`) is 250, and the Rust `regex`
///   crate's default `nest_limit` is 250. A real-world pattern
///   essentially never approaches even that; this limit is 4× the
///   ecosystem norm so no legitimate (or PCRE2-conformance-corpus)
///   pattern is rejected.
/// - It is still a hard ceiling, so adversarial input cannot drive
///   [`grow_stack`] into unbounded stack allocation: a pattern
///   nested past this point is rejected with a clean compile error
///   in O(depth) without ever recursing that far.
///
/// Enforced by [`too_deeply_nested`], threaded as a depth counter
/// through the `parsing.rs` typed-AST walker (the earliest point
/// the structural nesting is visible, before the expensive compiler
/// passes run).
///
/// **Defense-in-depth as of PGEN 1.1.77 (PGEN-RGX-0085).** PGEN now
/// enforces its own `REGEX_MAX_NESTING_DEPTH = 250` ceiling at the
/// embedding-API boundary *before* its recursive-descent parser
/// runs, returning a clean parse error. Since 250 < 1000, PGEN's
/// stricter ceiling triggers first on the normal parse path — this
/// RGX-side limit is now belt-and-suspenders. It still matters for
/// the `Regex::from_ast` parser-bypass path (which skips PGEN
/// entirely) and as a backstop independent of the parser backend;
/// the value is deliberately kept at 1000 (unchanged behaviour for
/// every ≤250 pattern, which is every real-world / conformance
/// pattern).
pub(crate) const MAX_NESTING_DEPTH: usize = 1000;

/// Stack space (bytes) that must remain before [`grow_stack`]
/// allocates a fresh segment. The compile pipeline's debug-build
/// frames are large (the typed walker re-parses source slices; the
/// compiler clones AST subtrees across passes), so the red zone is
/// deliberately roomy — one more level must always fit.
const RED_ZONE: usize = 128 * 1024;

/// Size (bytes) of each stack segment [`grow_stack`] allocates when
/// the red zone is hit. Large enough that deep-but-legal patterns
/// trigger only a handful of growths; only ever allocated when a
/// pattern is actually deeply nested.
const NEW_STACK_SIZE: usize = 8 * 1024 * 1024;

/// Run `f` on a stack guaranteed not to overflow: if less than
/// [`RED_ZONE`] bytes remain, a fresh [`NEW_STACK_SIZE`]-byte
/// segment is allocated first. Cheap (a TLS read + compare) when
/// ample stack remains, so it is safe to call at the entry of every
/// recursive step.
///
/// This is the compile-pipeline analog of the `serde_stacker`
/// wrapper already applied to JSON deserialization — applied
/// consistently so *no* recursive descent in the pipeline can abort
/// the process.
#[inline]
pub(crate) fn grow_stack<R>(f: impl FnOnce() -> R) -> R {
    stacker::maybe_grow(RED_ZONE, NEW_STACK_SIZE, f)
}

/// Stack space (bytes) the compiler boundary requires before it
/// runs. Deliberately larger than any inherited thread stack
/// (`libtest` worker threads, async runtimes, and embedder threads
/// commonly run with ≤ 2–8 MiB) so [`compile_on_deep_stack`]
/// *always* trips `stacker`'s grow path and runs the whole
/// AST→bytecode pipeline on a guaranteed-deep segment.
const COMPILER_RED_ZONE: usize = 16 * 1024 * 1024;

/// Size (bytes) of the segment [`compile_on_deep_stack`] runs on.
/// Sized for a [`MAX_NESTING_DEPTH`]-deep AST walked by ~14
/// recursive compiler passes plus recursive bytecode/NFA codegen,
/// with the large debug-build frames those carry. Virtual address
/// space only — pages are committed lazily as the recursion
/// actually uses them, and `stacker` pools the segment for reuse
/// across calls on the same thread, so the amortised cost is one
/// allocation per worker thread, not per `compile`.
const COMPILER_SEGMENT: usize = 64 * 1024 * 1024;

/// Run the AST→bytecode compiler pipeline on a guaranteed-deep
/// stack.
///
/// The pipeline is *not* a single recursive function: it is ~14
/// independent recursive AST passes plus recursive bytecode/NFA
/// codegen spread across `compiler.rs`, `vm.rs`, `c2/`, and `ac.rs`.
/// Sprinkling [`grow_stack`] through every one of them would be a
/// large, error-prone diff in perf-critical code. Instead this wraps
/// the single function both `Compiler::compile` (parser path) and
/// `Compiler::compile_ast` (the parser-bypass `Regex::from_ast`
/// path) funnel into, forcing exactly one `stacker` grow up front
/// (because [`COMPILER_RED_ZONE`] exceeds any realistic inherited
/// stack) so every downstream recursion — bounded by
/// [`MAX_NESTING_DEPTH`] — has ample room and cannot abort the
/// process. Idempotent and cheap on repeat calls: `stacker` reuses
/// the pooled segment.
#[inline]
pub(crate) fn compile_on_deep_stack<R>(f: impl FnOnce() -> R) -> R {
    stacker::maybe_grow(COMPILER_RED_ZONE, COMPILER_SEGMENT, f)
}

/// Returns `true` when `depth` has exceeded [`MAX_NESTING_DEPTH`].
///
/// The check is `>` (not `>=`) so a pattern nested exactly
/// `MAX_NESTING_DEPTH` levels still compiles; only strictly deeper
/// input is rejected.
#[inline]
pub(crate) fn exceeds_nesting_limit(depth: usize) -> bool {
    depth > MAX_NESTING_DEPTH
}

/// Conservative upper bound on the parenthesis-nesting depth of a
/// pattern string, computed in a single non-recursive O(n) pass.
///
/// # Why a string pre-scan
///
/// The dominant unbounded recursion in the compile pipeline is
/// **PGEN's own generated recursive-descent parser**
/// (`parse_group → parse_pattern → … → parse_group`, one frame
/// chain per `(` level). PGEN is the sole parser and is read-only
/// from RGX, so RGX cannot add a recursion guard inside it. The
/// doctrine-compliant RGX-side response is to *reject pathological
/// input before invoking PGEN* — pure input validation, not a
/// workaround that absorbs malformed parser output (PGEN's output
/// is correct; it simply must not be handed crash-inducing input).
///
/// # Why conservative (over-approximating) is correct
///
/// The scan only honours `\` escaping; it deliberately does **not**
/// model character classes, `\Q…\E`, or `(?#…)` comments. Every
/// such simplification can only make the count *larger* than the
/// parser's true group-nesting recursion, never smaller — so a
/// pattern that would overflow PGEN's stack can never slip past
/// this gate. The only cost is rejecting some absurd inputs (e.g.
/// >[`MAX_NESTING_DEPTH`] literal `(` inside a class) slightly
/// earlier than strictly necessary; no realistic or
/// PCRE2-conformance pattern comes within orders of magnitude of
/// the limit.
pub(crate) fn pattern_nesting_depth(pattern: &str) -> usize {
    let mut depth: usize = 0;
    let mut max: usize = 0;
    let mut escaped = false;
    for b in pattern.bytes() {
        if escaped {
            escaped = false;
            continue;
        }
        match b {
            b'\\' => escaped = true,
            b'(' => {
                depth += 1;
                if depth > max {
                    max = depth;
                }
            }
            b')' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    max
}

/// The clean compile error returned for an over-nested pattern.
/// Deterministic and fast — produced before the pipeline recurses
/// (and grows the stack) without bound.
pub(crate) fn too_deeply_nested() -> RgxError {
    RgxError::compile(format!(
        "pattern nesting too deep: exceeds the {MAX_NESTING_DEPTH}-level \
         compile-time limit (raise the nesting or simplify the pattern). \
         This bound exists so adversarial input cannot exhaust the stack."
    ))
}
