use crate::c2::dfa::{DfaSearchOutcome, LazyDfa};
use crate::c2::Classification;
use crate::error::Result;
use crate::events::MatchEvent;
use crate::execution::{
    CodeBlockValue, ExecContext, ExecResult, ExecutionManager, MatchContinuation, MatchOutcome,
};
use crate::pattern::CompiledPattern;
use crate::vm::{CompiledCharClass, PrefixFilter, RegexVM};
use crate::{trace_decision, trace_enter, trace_exit};
use std::sync::{Arc, Mutex, OnceLock};

/// Execution mode that controls performance vs feature tradeoffs
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ExecutionMode {
    /// Maximum performance, pure regex matching only
    Pure,
    /// Code execution in sandboxed environments only
    Safe,
    /// Enables the native-callback path in addition to the sandboxed backends
    Full,
}

/// Controls how alternation matches are selected.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MatchSemantics {
    /// Leftmost-first: the first alternative that matches wins (PCRE2/Perl default).
    /// `a|ab` on "ab" → matches "a".
    LeftmostFirst,
    /// Leftmost-longest: at each position, the longest match wins (POSIX semantics).
    /// `a|ab` on "ab" → matches "ab".
    LeftmostLongest,
}

impl Default for MatchSemantics {
    fn default() -> Self {
        Self::LeftmostFirst
    }
}

/// Result of a partial-match query.
#[derive(Clone, Debug, PartialEq)]
pub enum PartialMatchResult {
    /// A full match was found.
    Full(MatchResult),
    /// The input ended mid-potential-match. More data might complete the match.
    /// Contains the byte offset where the partial match started.
    Partial(usize),
    /// No match and no partial match — the pattern cannot match this input
    /// even with more data appended.
    NoMatch,
}

/// Match result with position information
#[derive(Clone, Debug, PartialEq)]
pub struct MatchResult {
    /// Start position in bytes
    pub start: usize,
    /// End position in bytes
    pub end: usize,
    /// Capture groups as `(start, end)` byte pairs.
    ///
    /// Index 0 is the overall match. Indices 1..N correspond to numbered
    /// capture groups. `None` means the group did not participate in the match.
    pub groups: Vec<Option<(usize, usize)>>,
    /// 1-based top-level branch number for top-level alternation matches.
    ///
    /// `None` when the pattern has no top-level alternation.
    pub matched_branch_number: Option<usize>,
    /// Last non-boolean code-block value observed on the winning match path.
    ///
    /// This is `None` when the winning path produced only predicate-style
    /// success/failure results.
    pub code_result: Option<CodeBlockValue>,
    /// Name of the last `(*MARK:name)` / `(*:name)` verb encountered on the
    /// winning match path (PCRE2 "mark" concept). `None` when no mark was hit.
    ///
    /// Substitute templates expose this as `${*MARK}` / `$*MARK`, and users
    /// can read it directly from the match result to understand which
    /// alternation branch or control-verb region produced the match.
    pub last_mark: Option<String>,
}

/// High-performance regex execution engine
pub struct Engine {
    /// The compiled VM for pattern execution
    vm: RegexVM,
    /// Execution mode for this engine
    mode: ExecutionMode,
    /// AST kept alongside the engine so the lazy artifact builders
    /// (`c2_dfa`, `c2_forward_unanchored_dfa`, `c2_reverse_dfa`,
    /// `jit_program`) can re-derive eligibility on first access. A
    /// single owned clone, taken once at [`Engine::new`].
    ast: crate::ast::Regex,
    /// C2 lazy DFA for `is_match` dispatch (step 5b). Built lazily on
    /// first access via `OnceLock` — only realised if/when the
    /// dispatch chain actually reaches for the anchored DFA. Inner
    /// `Option` indicates ineligibility (pattern outside the C2
    /// subset or contains assertions the DFA can't model). Wrapped in
    /// `Mutex` because the DFA's `transition` method mutates its
    /// state cache and public `Regex` API methods are `&self`.
    c2_dfa: OnceLock<Option<Mutex<LazyDfa>>>,
    /// C2 lazy DFA built over the **forward-unanchored** NFA.
    /// Companion to [`Self::c2_reverse_dfa`] in the **reverse-DFA
    /// pipeline**: a single O(n) forward sweep (via this DFA) finds
    /// the END of the leftmost match anywhere in the input, then the
    /// reverse DFA walks backward from that end to find the START,
    /// and finally Pike-VM recovers captures over the known span.
    /// Replaces the per-position anchored-DFA scan (which was O(n ·
    /// candidate_positions)) on the fast path. Lazy — built on first
    /// access from the reverse-DFA pipeline driver.
    c2_forward_unanchored_dfa: OnceLock<Option<Mutex<LazyDfa>>>,
    /// C2 lazy DFA built over the **reverse-anchored** NFA. The
    /// reverse half of the reverse-DFA pipeline: walks backward from
    /// a known match-end to find the leftmost match-start in a single
    /// bounded sweep. Lazy — built on first access alongside the
    /// other C2 DFAs.
    c2_reverse_dfa: OnceLock<Option<Mutex<LazyDfa>>>,
    /// C1 JIT-compiled program for native dispatch (step 5). Lazy —
    /// `JitProgram` codegen via Cranelift takes meaningful time, so
    /// deferring it to the first match call removes the cost from
    /// every `Regex::compile` and pays it only when the dispatch
    /// chain actually reaches for the JIT path. Wrapped in `Mutex`
    /// because `cranelift_jit::JITModule`'s internal symbol table is
    /// interior-mutable, mirroring the `c2_dfa` pattern. Gated on
    /// `feature = "jit"`.
    #[cfg(feature = "jit")]
    jit_program: OnceLock<Option<Mutex<crate::c1::JitProgram>>>,
}

/// Convert a VM-level `Match` into a public `MatchResult`.
pub(crate) fn vm_match_to_result(m: crate::vm::Match) -> MatchResult {
    MatchResult {
        start: m.start,
        end: m.end,
        groups: m.groups,
        matched_branch_number: m.matched_alternative.map(|id| id + 1),
        code_result: m.code_result,
        last_mark: m.last_mark,
    }
}

/// Convert a `PikeMatch` (from `c2::pike`) into the public
/// [`MatchResult`] shape used throughout the rest of the API. Mirrors
/// the helper in `lib.rs`; duplicated here so the engine-internal
/// dispatch methods (`try_dfa_find_first` / `try_dfa_find_all`) don't
/// have to round-trip through `lib.rs`.
///
/// `matched_branch_number` and `code_result` are always `None` for
/// C2-dispatched patterns by construction (the dispatch eligibility
/// checks exclude top-level alternation and inline code blocks).
fn pike_match_to_match_result(m: crate::c2::PikeMatch) -> MatchResult {
    MatchResult {
        start: m.start,
        end: m.end,
        groups: m.groups,
        matched_branch_number: None,
        code_result: None,
        last_mark: None,
    }
}

/// Allocate a fresh capture-slot buffer sized for `num_groups`
/// user-numbered capture groups. Layout matches the JIT'd function's
/// captures_ptr contract: `[i64; 2 * (num_groups + 1)]`, with each
/// pair representing `(start, end)` for one group, and `-1` meaning
/// "unset". Group 0 is included alongside the user groups, so the
/// buffer always has at least 2 slots.
///
/// Initialised to `-1` in every slot — see [`reset_capture_buffer`]
/// for the inter-call reset that prepares the buffer for re-use.
/// C1 step 4b.
#[cfg(feature = "jit")]
fn new_capture_buffer(num_groups: u32) -> Vec<i64> {
    vec![-1i64; 2 * (num_groups as usize + 1)]
}

/// Reset every slot in the capture buffer to `-1`. Called between
/// JIT'd-function invocations so the buffer is in the contract-required
/// state. The JIT'd function expects every slot to be `-1` on entry
/// and writes the actual capture spans during execution. C1 step 4b.
#[cfg(feature = "jit")]
fn reset_capture_buffer(captures: &mut [i64]) {
    for slot in captures.iter_mut() {
        *slot = -1;
    }
}

/// Convert a JIT'd-function result `(start, end)` plus the post-call
/// captures buffer into the public [`MatchResult`] shape. C1 step 4b.
///
/// The JIT'd function returns the new match end position via its
/// return value AND populates the caller-provided captures buffer
/// with `(start, end)` pairs for every capture group 0..=num_groups.
/// This helper:
///
/// 1. Reads each `(start, end)` slot pair from the buffer.
/// 2. Treats `(-1, -1)` as "group did not participate" (the JIT
///    initializes every slot to -1 before invocation).
/// 3. Forces group 0 to `(start, end)` from the JIT's return value
///    even if the program's bytecode didn't emit `SaveStart(0)` /
///    `SaveEnd(0)` ops — the JIT-eligible subset always treats
///    group 0 as the overall match span. (In practice the bytecode
///    *does* emit these, but the helper is robust either way.)
///
/// `matched_branch_number` and `code_result` are always `None` for
/// the JIT path: top-level alternation patterns are excluded from
/// JIT dispatch in `build_jit_program_if_eligible`, and code blocks
/// are excluded by the eligibility check.
#[cfg(feature = "jit")]
fn jit_match_to_result(start: usize, end: usize, captures: &[i64], num_groups: u32) -> MatchResult {
    let total_groups = num_groups as usize + 1; // include group 0
    let mut groups = Vec::with_capacity(total_groups);
    // Group 0 is always the overall match span — force it from the
    // JIT's (start, end) regardless of what the buffer says.
    groups.push(Some((start, end)));
    // Groups 1..=num_groups come from the captures buffer.
    for g in 1..=num_groups as usize {
        let s_slot = captures[2 * g];
        let e_slot = captures[2 * g + 1];
        if s_slot < 0 || e_slot < 0 {
            groups.push(None);
        } else {
            #[allow(clippy::cast_sign_loss)] // checked >= 0 above
            groups.push(Some((s_slot as usize, e_slot as usize)));
        }
    }
    MatchResult {
        start,
        end,
        groups,
        matched_branch_number: None,
        code_result: None,
        last_mark: None,
    }
}

/// Position-iterator that skips bytes the pattern's prefix can't match.
///
/// Wraps the existing [`RegexVM`]'s compile-time prefix filter so the C2
/// dispatch path (DFA + Pike-VM) reuses the same scan-skip the existing
/// backtracking VM uses. Without this, classifier-positive patterns
/// like `\b\w+@\w+\.\w+\b` (filter = `Word`) and `(\d{4})-(\d{2})-(\d{2})`
/// (filter = `Digit`) would scan every byte instead of jumping to
/// candidate positions, which makes the C2 dispatch ~2-3x slower than
/// the existing VM on those patterns.
///
/// Resolution rules per filter variant:
/// - `Byte(b)` → SIMD-accelerated `memchr::memchr`
/// - `Digit` / `Word` / `Space` → tight scalar loop testing the byte
/// - `CharClass(id)` → tight scalar loop calling
///   [`PrefixFilter::matches`] with the program's char-class table
/// - `None` → identity (every position is a candidate)
struct PrefixScanner<'a> {
    filter: PrefixFilter,
    char_classes: &'a [CompiledCharClass],
}

impl<'a> PrefixScanner<'a> {
    /// Build a scanner from a VM. Combines the C2 program's
    /// `c2_prefix_byte` with the VM's `PrefixFilter` — the former is
    /// always at least as precise as `PrefixFilter::Byte`, so we prefer
    /// the byte form when both are available (no behaviour change, but
    /// keeps the byte-oriented hot path consistent across all dispatch
    /// loops). Returns a scanner that the dispatch loops use to find
    /// the next candidate scan position.
    fn new(vm: &'a RegexVM, c2_prefix_byte: Option<u8>) -> Self {
        let filter = match c2_prefix_byte {
            Some(byte) => PrefixFilter::Byte(byte),
            None => vm.prefix_filter(),
        };
        Self {
            filter,
            char_classes: vm.char_classes(),
        }
    }

    /// Find the next byte position at-or-after `start` (inclusive) that
    /// might match the pattern's prefix. Returns `None` when no
    /// candidate position exists in `input[start..]`. The returned
    /// position is always `<= input.len()`; for filters that test
    /// byte content the returned position is always `< input.len()`.
    #[inline]
    fn next_candidate(&self, input: &[u8], start: usize) -> Option<usize> {
        match self.filter {
            PrefixFilter::None => {
                if start <= input.len() {
                    Some(start)
                } else {
                    None
                }
            }
            PrefixFilter::Byte(b) => {
                if start >= input.len() {
                    None
                } else {
                    memchr::memchr(b, &input[start..]).map(|offset| start + offset)
                }
            }
            PrefixFilter::Digit => {
                let mut pos = start;
                while pos < input.len() {
                    if input[pos].is_ascii_digit() {
                        return Some(pos);
                    }
                    pos += 1;
                }
                None
            }
            PrefixFilter::Word => {
                let mut pos = start;
                while pos < input.len() {
                    let b = input[pos];
                    if b.is_ascii_alphanumeric() || b == b'_' {
                        return Some(pos);
                    }
                    pos += 1;
                }
                None
            }
            PrefixFilter::Space => {
                let mut pos = start;
                while pos < input.len() {
                    if crate::vm::pcre2_is_space_byte(input[pos]) {
                        return Some(pos);
                    }
                    pos += 1;
                }
                None
            }
            PrefixFilter::CharClass(_) => {
                let mut pos = start;
                while pos < input.len() {
                    if self.filter.matches(input[pos], self.char_classes) {
                        return Some(pos);
                    }
                    pos += 1;
                }
                None
            }
        }
    }
}

/// Build a `Mutex<LazyDfa>` for the given AST + C2 program if the
/// pattern is DFA-eligible. Returns `None` if the pattern lacks a C2
/// program (Pike-VM not eligible) or if the AST contains constructs
/// the DFA can't handle (assertions, lazy quantifiers — see
/// [`crate::c2::program::is_c2_dfa_eligible`]). C2 step 5b.
fn build_dfa_if_eligible(
    ast: &crate::ast::Regex,
    c2_program: &Option<crate::c2::CompiledC2Program>,
) -> Option<Mutex<LazyDfa>> {
    let c2 = c2_program.as_ref()?;
    if !crate::c2::program::is_c2_dfa_eligible(ast) {
        return None;
    }
    // Clone the NFA and byte-class map into Arcs so the DFA can own
    // shared references. Cloning is O(states + transitions) — small
    // for typical patterns.
    let nfa = Arc::new(c2.forward_anchored.clone());
    let bcm = Arc::new(c2.byte_class_map.clone());
    LazyDfa::new(nfa, bcm, LazyDfa::DEFAULT_STATE_LIMIT)
        .ok()
        .map(Mutex::new)
}

/// Build a `Mutex<LazyDfa>` over the **forward-unanchored** NFA for
/// the given AST + C2 program if the pattern is DFA-eligible. The
/// forward half of the reverse-DFA pipeline: a single call to
/// `find_match_at(input, p)` walks the DFA once from `p` and
/// returns the end of the leftmost match starting at any position
/// `≥ p`, replacing the per-position anchored scan loop.
///
/// Returns `None` for the same reasons [`build_dfa_if_eligible`]
/// does (pattern outside the C2 subset or contains assertions the
/// DFA can't model).
fn build_forward_unanchored_dfa_if_eligible(
    ast: &crate::ast::Regex,
    c2_program: &Option<crate::c2::CompiledC2Program>,
) -> Option<Mutex<LazyDfa>> {
    let c2 = c2_program.as_ref()?;
    if !crate::c2::program::is_c2_dfa_eligible(ast) {
        return None;
    }
    let nfa = Arc::new(c2.forward_unanchored.clone());
    let bcm = Arc::new(c2.byte_class_map.clone());
    LazyDfa::new(nfa, bcm, LazyDfa::DEFAULT_STATE_LIMIT)
        .ok()
        .map(Mutex::new)
}

/// Build a `Mutex<LazyDfa>` over the **reverse-anchored** NFA for
/// the given AST + C2 program if the pattern is DFA-eligible. The
/// reverse half of the reverse-DFA pipeline: once the forward
/// unanchored DFA finds the END of a match, this DFA walks backward
/// from that end to find the START of the leftmost match in a
/// single bounded pass.
///
/// Returns `None` for the same reasons [`build_dfa_if_eligible`]
/// does (pattern outside the C2 subset or contains assertions the
/// DFA can't model).
fn build_reverse_dfa_if_eligible(
    ast: &crate::ast::Regex,
    c2_program: &Option<crate::c2::CompiledC2Program>,
) -> Option<Mutex<LazyDfa>> {
    let c2 = c2_program.as_ref()?;
    if !crate::c2::program::is_c2_dfa_eligible(ast) {
        return None;
    }
    // Build the lazy DFA from the reverse-anchored NFA. Same
    // construction path as the forward DFA — just a different
    // input NFA. The byte-class map is shared with the forward
    // DFA (cloned into a fresh Arc).
    let nfa = Arc::new(c2.reverse_anchored.clone());
    let bcm = Arc::new(c2.byte_class_map.clone());
    LazyDfa::new(nfa, bcm, LazyDfa::DEFAULT_STATE_LIMIT)
        .ok()
        .map(Mutex::new)
}

/// Try to JIT-compile the given program into a `JitProgram`.
/// Returns `None` if the pattern is outside the JIT-eligible
/// subset (the most common case — anything with captures,
/// lookaround, code blocks, etc. is rejected by the C1 decoder),
/// or if any other JIT host error occurs (e.g. the host
/// architecture isn't supported by Cranelift), or if the pattern
/// has top-level alternation (the JIT path doesn't track
/// `matched_branch_number`, mirroring the C2 dispatch's exclusion).
///
/// `Some(Mutex<JitProgram>)` means the engine should consider
/// dispatching through the JIT for this pattern. The runtime
/// gating in [`Engine::should_use_jit`] still applies (event
/// observers, runtime safety limits).
///
/// C1 step 5.
#[cfg(feature = "jit")]
fn build_jit_program_if_eligible(
    ast: &crate::ast::Regex,
    program: &crate::vm::Program,
) -> Option<Mutex<crate::c1::JitProgram>> {
    // Skip JIT dispatch for top-level alternation. The JIT'd
    // function returns only the match span, not the matched
    // branch number — and the existing API contract sets
    // `MatchResult.matched_branch_number = Some(branch_idx)` for
    // top-level alternation. Routing these patterns through the
    // JIT would silently drop the branch number. The C2 dispatch
    // path excludes top-level alternation for the same reason
    // (see `c2::program::is_c2_dispatch_eligible`).
    if crate::c2::program::has_top_level_alternation(ast) {
        return None;
    }
    crate::c1::compile_program_to_jit_program(program)
        .ok()
        .map(Mutex::new)
}

impl Engine {
    /// Create new engine from compiled pattern
    ///
    /// # Errors
    /// Returns an error if engine initialization fails for the given compiled pattern.
    pub fn new(pattern: &CompiledPattern) -> Result<Self> {
        trace_enter!(
            "engine",
            "Engine::new",
            "mode={:?},bytecode_len={}",
            pattern.mode,
            pattern.program.code.len()
        );
        let execution_manager =
            if pattern.program.flags.has_code_blocks && pattern.mode != ExecutionMode::Pure {
                Some(Arc::new(ExecutionManager::new()))
            } else {
                None
            };
        // C2 DFA caches and the JIT program are now built lazily on
        // first dispatch (see `should_dispatch_to_*` helpers and
        // `should_use_jit`). Construction of those artifacts is
        // 17-33% of total compile time on JIT-eligible patterns, and
        // typically only one of the four (anchored DFA / forward-
        // unanchored DFA / reverse-anchored DFA / JIT) is used per
        // match. Eager construction wastes 75% of that work; lazy
        // construction defers each artifact to its first need and
        // skips the rest entirely. See ROADMAP.md "Performance:
        // close the PCRE2 compile-time gap to <5x" technique #1.
        let vm = RegexVM::with_execution_manager(pattern.program.clone(), execution_manager);
        let engine = Self {
            vm,
            mode: pattern.mode,
            ast: pattern.ast.clone(),
            c2_dfa: OnceLock::new(),
            c2_forward_unanchored_dfa: OnceLock::new(),
            c2_reverse_dfa: OnceLock::new(),
            #[cfg(feature = "jit")]
            jit_program: OnceLock::new(),
        };
        trace_exit!("engine", "Engine::new", "ok=true,mode={:?}", engine.mode);
        Ok(engine)
    }

    /// Returns the lazy DFA for this engine if the pattern is DFA-
    /// eligible AND the runtime state allows DFA dispatch (no event
    /// observer, no runtime safety limits — same constraints as
    /// [`Engine::should_dispatch_to_c2`]).
    ///
    /// Also returns `None` for pure-literal patterns: the existing VM
    /// has a `memchr::memmem::Finder` fast path that bypasses the VM
    /// entirely for those, and that fast path is faster than anything
    /// the C2 engines can do for a pure literal pattern.
    ///
    /// The returned `Mutex` must be locked by the caller. The DFA's
    /// `transition` method mutates its state cache, so the lock is
    /// required even from `&self`.
    #[doc(hidden)]
    pub fn should_dispatch_to_dfa(&self) -> Option<&Mutex<LazyDfa>> {
        let dfa = self
            .c2_dfa
            .get_or_init(|| build_dfa_if_eligible(&self.ast, &self.vm.program.c2_program))
            .as_ref()?;
        if self.vm.has_event_observer() {
            return None;
        }
        if self.vm.has_runtime_match_limits() {
            return None;
        }
        if self.vm.has_literal_finder() {
            return None;
        }
        Some(dfa)
    }

    /// Returns the forward-unanchored lazy DFA for this engine if
    /// the pattern is DFA-eligible. The forward half of the
    /// reverse-DFA pipeline: a single call to `find_match_at(input,
    /// p)` walks the DFA once and returns the end of the leftmost
    /// match anywhere at or after `p`.
    ///
    /// Same runtime gating as [`Self::should_dispatch_to_dfa`]. The
    /// returned `Mutex` must be locked by the caller because the
    /// DFA's cache is interior-mutable.
    #[doc(hidden)]
    pub fn should_dispatch_to_forward_unanchored_dfa(&self) -> Option<&Mutex<LazyDfa>> {
        let dfa = self
            .c2_forward_unanchored_dfa
            .get_or_init(|| {
                build_forward_unanchored_dfa_if_eligible(&self.ast, &self.vm.program.c2_program)
            })
            .as_ref()?;
        if self.vm.has_event_observer() {
            return None;
        }
        if self.vm.has_runtime_match_limits() {
            return None;
        }
        if self.vm.has_literal_finder() {
            return None;
        }
        Some(dfa)
    }

    /// Returns the reverse-anchored lazy DFA for this engine if the
    /// pattern is DFA-eligible. The reverse half of the reverse-DFA
    /// pipeline: walks backward from a known match-end to find the
    /// leftmost match-start in one pass.
    ///
    /// Same runtime gating as [`Self::should_dispatch_to_dfa`].
    #[doc(hidden)]
    pub fn should_dispatch_to_reverse_dfa(&self) -> Option<&Mutex<LazyDfa>> {
        let dfa = self
            .c2_reverse_dfa
            .get_or_init(|| build_reverse_dfa_if_eligible(&self.ast, &self.vm.program.c2_program))
            .as_ref()?;
        if self.vm.has_event_observer() {
            return None;
        }
        if self.vm.has_runtime_match_limits() {
            return None;
        }
        if self.vm.has_literal_finder() {
            return None;
        }
        Some(dfa)
    }

    /// Try to answer `is_match` via the lazy DFA. Returns:
    /// - `Some(true)` / `Some(false)` if the DFA produced a definitive answer
    /// - `None` if the DFA isn't available, the pattern isn't DFA-
    ///   eligible, runtime state forbids DFA dispatch, or the cache
    ///   exhausted during the scan
    ///
    /// The caller (`Regex::is_match` in `lib.rs`) falls back to the
    /// Pike-VM (and ultimately the existing backtracking VM) when this
    /// returns `None`.
    ///
    /// Fast path: the **forward-unanchored** DFA walks the input once
    /// and reports whether any match exists — O(n) instead of O(n ·
    /// candidate_positions). Falls back to the per-position anchored
    /// scan only if the unanchored cache exhausts.
    /// Try to answer `is_match` via the Aho-Corasick literal-set
    /// fast path. Returns `Some(true/false)` if the pattern is a
    /// top-level alternation of pure ASCII literals and AC produced
    /// a definitive answer, `None` otherwise (caller falls through
    /// to the regular dispatch chain).
    ///
    /// O(n + m) instead of the O(n × m) cost the backtracking VM
    /// pays for top-level literal alternations, which the C2
    /// dispatch chain excludes (Pike-VM doesn't track
    /// `matched_branch_number`).
    #[doc(hidden)]
    pub fn try_ac_is_match(&self, input: &[u8]) -> Option<bool> {
        if self.vm.has_event_observer() {
            return None;
        }
        let ac = self.vm.program.ac_literal_set.as_ref()?;
        Some(ac.is_match(input))
    }

    /// Try to answer `find_first` via the Aho-Corasick literal-set
    /// fast path. Returns `Some(Some(match))` / `Some(None)` if AC
    /// produced a definitive answer, `None` to fall through.
    ///
    /// AC is configured with `MatchKind::LeftmostFirst` so the
    /// returned match honours PCRE2's alternation semantics: when
    /// two branches could match at the same position, the first one
    /// in source order wins. The 0-based pattern_id from AC is
    /// translated to a 1-based `matched_branch_number` on the
    /// returned `MatchResult`.
    #[doc(hidden)]
    pub fn try_ac_find_first(&self, input: &[u8]) -> Option<Option<MatchResult>> {
        if self.vm.has_event_observer() {
            return None;
        }
        let ac = self.vm.program.ac_literal_set.as_ref()?;
        let Some(m) = ac.find(input) else {
            return Some(None);
        };
        Some(Some(MatchResult {
            start: m.start(),
            end: m.end(),
            // Top-level literal alternation has no nested capture
            // groups by construction (only `Char` and `Sequence` of
            // `Char` are eligible — see `ac::pure_ascii_literal`),
            // so the captures vec is just the whole-match span.
            groups: vec![Some((m.start(), m.end()))],
            // 1-based branch number: AC pattern_id is 0-based.
            matched_branch_number: Some(m.pattern().as_usize() + 1),
            code_result: None,
            last_mark: None,
        }))
    }

    /// Try to answer `find_all` via the Aho-Corasick literal-set
    /// fast path. Returns `Some(matches)` if AC produced a definitive
    /// list of all non-overlapping matches, `None` to fall through.
    ///
    /// AC's `find_iter` yields non-overlapping leftmost-first matches
    /// directly. No empty-match advance rule is needed because the
    /// eligibility check rejects empty literal branches.
    #[doc(hidden)]
    pub fn try_ac_find_all(&self, input: &[u8]) -> Option<Vec<MatchResult>> {
        if self.vm.has_event_observer() {
            return None;
        }
        let ac = self.vm.program.ac_literal_set.as_ref()?;
        let results: Vec<MatchResult> = ac
            .find_iter(input)
            .map(|m| MatchResult {
                start: m.start(),
                end: m.end(),
                groups: vec![Some((m.start(), m.end()))],
                matched_branch_number: Some(m.pattern().as_usize() + 1),
                code_result: None,
                last_mark: None,
            })
            .collect();
        Some(results)
    }

    #[doc(hidden)]
    pub fn try_dfa_is_match(&self, input: &[u8]) -> Option<bool> {
        // Inner-literal fast-fail: if the pattern requires a specific
        // byte to appear in any match (`@` for an email pattern, `-`
        // for a date pattern), and the input doesn't contain that
        // byte, no match can exist. memchr is SIMD-accelerated and
        // ~10-30x faster than the DFA walk on bulk no-match inputs.
        if let Some(b) = self.required_inner_byte_for_dispatch() {
            if memchr::memchr(b, input).is_none() {
                return Some(false);
            }
        }
        // Reverse-DFA-pipeline fast path: one O(n) walk of the
        // forward-unanchored DFA answers is_match directly.
        if let Some(forward_mutex) = self.should_dispatch_to_forward_unanchored_dfa() {
            if let Ok(mut forward) = forward_mutex.lock() {
                match forward.find_match_at(input, 0) {
                    DfaSearchOutcome::Match(_) => return Some(true),
                    DfaSearchOutcome::NoMatch => return Some(false),
                    DfaSearchOutcome::Exhausted => {
                        // Fall through to the anchored fallback.
                    }
                }
            }
        }
        let dfa_mutex = self.should_dispatch_to_dfa()?;
        let mut dfa = dfa_mutex.lock().ok()?;
        // Per-position anchored scan (pre-pipeline fallback). The
        // simulator might exhaust its cache mid-scan; in that case
        // bail and let the caller fall back further.
        for start in 0..=input.len() {
            match dfa.find_match_at(input, start) {
                DfaSearchOutcome::Match(_) => return Some(true),
                DfaSearchOutcome::NoMatch => continue,
                DfaSearchOutcome::Exhausted => return None,
            }
        }
        Some(false)
    }

    /// Try to answer `find_first` via the lazy DFA. Returns:
    /// - `Some(Some(match))` if the DFA found a match (captures
    ///   recovered via Pike-VM at the matched start position)
    /// - `Some(None)` if the DFA confirmed no match exists
    /// - `None` if the DFA isn't available or the cache exhausted —
    ///   the caller falls back to Pike-VM
    ///
    /// **Reverse-DFA pipeline fast path.** When both the forward-
    /// unanchored and reverse-anchored DFAs are available, `find_first`
    /// runs as a single forward-then-reverse sweep:
    ///   1. forward-unanchored DFA walks `input` once to find the END
    ///      of the leftmost match (O(n), one pass over the input)
    ///   2. reverse-anchored DFA walks backward from that end to find
    ///      the START of the leftmost match (O(match_length))
    ///   3. Pike-VM `pike_captures_at` recovers capture groups over
    ///      the known span.
    ///
    /// The forward-unanchored DFA preserves leftmost-first semantics
    /// because the unanchoring lazy prefix `(?s:.)*?` states are
    /// pruned from any NFA state set that also contains the accept
    /// state (see `c2/dfa.rs::prune_lazy_prefix_if_accepting`).
    /// Without the prune, subset construction's natural leftmost-
    /// longest behaviour would overwrite the first match end with a
    /// later one.
    ///
    /// If either DFA is unavailable (e.g., top-level alternation
    /// blocked by dispatch eligibility, runtime flags forbid DFA
    /// dispatch) or exhausts mid-walk, the function falls through to
    /// the per-position anchored DFA scan with
    /// [`crate::c2::prefix_scanner::PrefixScanner`] skip acceleration
    /// — the pre-pipeline default path.
    #[doc(hidden)]
    pub fn try_dfa_find_first(&self, input: &[u8]) -> Option<Option<MatchResult>> {
        let c2 = self.vm.program.c2_program.as_ref()?;
        // Inner-literal fast-fail (see `try_dfa_is_match` for rationale).
        if let Some(b) = self.required_inner_byte_for_dispatch() {
            if memchr::memchr(b, input).is_none() {
                return Some(None);
            }
        }
        // Reverse-DFA pipeline fast path. Only prefer it when the
        // per-position scan has no prefix hint — otherwise the scan's
        // memchr/byte-class skip acceleration dominates because it
        // jumps directly to candidate positions instead of running the
        // forward-unanchored DFA over every byte of the input.
        if self.pipeline_dispatch_preferred(c2) {
            if let Some(forward_mutex) = self.should_dispatch_to_forward_unanchored_dfa() {
                if let Some(reverse_mutex) = self.should_dispatch_to_reverse_dfa() {
                    if let Some(outcome) =
                        self.try_pipeline_find_first(input, c2, forward_mutex, reverse_mutex)
                    {
                        return Some(outcome);
                    }
                    // Pipeline exhausted or failed to reconcile — fall
                    // through to the per-position anchored scan below.
                }
            }
        }
        let dfa_mutex = self.should_dispatch_to_dfa()?;
        let scanner = PrefixScanner::new(&self.vm, c2.c2_prefix_byte);
        let mut dfa = dfa_mutex.lock().ok()?;
        let mut start = 0usize;
        while start <= input.len() {
            let Some(candidate) = scanner.next_candidate(input, start) else {
                return Some(None);
            };
            start = candidate;
            match dfa.find_match_at(input, start) {
                DfaSearchOutcome::Match(_) => {
                    // DFA confirms a match starts at this position.
                    // Recover captures via Pike-VM at this exact start.
                    drop(dfa);
                    let pike_match = crate::c2::pike::pike_captures_at(c2, input, start)?;
                    return Some(Some(pike_match_to_match_result(pike_match)));
                }
                DfaSearchOutcome::NoMatch => start += 1,
                DfaSearchOutcome::Exhausted => return None,
            }
        }
        Some(None)
    }

    /// Returns the pattern's required-interior byte for dispatch
    /// fast-fail, if any. The byte is taken from the C2 program's
    /// `c2_required_inner_byte` field but is only returned when
    /// runtime state allows the fast-fail to fire — specifically,
    /// when no event observer is attached. (Event observers are
    /// position-by-position callbacks; skipping the DFA walk would
    /// silently elide every observer event from a no-match path,
    /// which violates the observer contract.)
    ///
    /// Runtime match limits (`max_steps`, `max_backtrack_frames`,
    /// `max_recursion_depth`) are unaffected — the fast-fail returns
    /// before any of those counters could be touched, and a no-match
    /// answer is identical regardless of the limits in force.
    fn required_inner_byte_for_dispatch(&self) -> Option<u8> {
        if self.vm.has_event_observer() {
            return None;
        }
        self.vm.program.c2_program.as_ref()?.c2_required_inner_byte
    }

    /// True when the reverse-DFA pipeline is preferred over the
    /// per-position anchored scan for `find_first`.
    ///
    /// The per-position scan is O(candidate_positions * match_length)
    /// and uses [`PrefixScanner`] + `memchr` / byte-class predicates to
    /// jump between candidate positions. When the pattern exposes any
    /// prefix hint (literal byte, `\d`/`\w`/`\s`, or a custom char
    /// class) the per-position scan is close to O(match_length) per
    /// candidate and typically faster than the pipeline's three full
    /// DFA sweeps.
    ///
    /// The pipeline is an O(n) unconditional sweep — its win comes
    /// from patterns whose prefix is unrestricted ([`PrefixFilter::None`]
    /// and `c2_prefix_byte == None`), where the per-position scan
    /// degenerates into running the anchored DFA at every byte.
    fn pipeline_dispatch_preferred(&self, c2: &crate::c2::CompiledC2Program) -> bool {
        c2.c2_prefix_byte.is_none()
            && matches!(self.vm.prefix_filter(), crate::vm::PrefixFilter::None)
    }

    /// Reverse-DFA pipeline driver for `find_first`. Returns
    /// `Some(Some(match))` on success, `Some(None)` when the forward
    /// sweep confirms no match exists anywhere, or `None` to signal
    /// the caller to fall through to the per-position scan (cache
    /// exhausted, mutex unavailable, or a reconciliation failure).
    ///
    /// The pipeline runs three bounded DFA passes in sequence:
    ///
    /// 1. **Forward-unanchored, stop at first accept** — one O(n)
    ///    walk returns the END of the leftmost match. Stopping at
    ///    first accept is the leftmost-first signal; letting the
    ///    walk continue would bias toward leftmost-longest semantics
    ///    (subset construction's natural behaviour).
    /// 2. **Reverse-anchored from that end** — walks backward to find
    ///    the leftmost START position at which the pattern begins.
    /// 3. **Forward-anchored from that start** — completes the
    ///    greedy extension: for patterns like `a+` on `"baaab"`,
    ///    step 1 finds the first accept at end=2, but the greedy
    ///    `+` should extend through all three `a`s to end=4. Re-running
    ///    the DFA anchored at the recovered start captures this
    ///    greedy tail.
    ///
    /// Then Pike-VM recovers capture groups at the recovered start;
    /// the Pike-VM's end offset serves as a cross-check against the
    /// DFA's greedy end, and a disagreement triggers fallback rather
    /// than a bogus span.
    fn try_pipeline_find_first(
        &self,
        input: &[u8],
        c2: &crate::c2::CompiledC2Program,
        forward_mutex: &Mutex<LazyDfa>,
        reverse_mutex: &Mutex<LazyDfa>,
    ) -> Option<Option<MatchResult>> {
        let first_accept_end = {
            let mut forward = forward_mutex.lock().ok()?;
            match forward.find_first_accept_at(input, 0) {
                DfaSearchOutcome::Match(end) => end,
                DfaSearchOutcome::NoMatch => return Some(None),
                DfaSearchOutcome::Exhausted => return None,
            }
        };
        let start = {
            let mut reverse = reverse_mutex.lock().ok()?;
            match reverse.find_match_start_at_reverse(input, first_accept_end) {
                DfaSearchOutcome::Match(start) => start,
                // Forward found a match but reverse couldn't locate the
                // start — treat as a reconciliation failure and fall
                // back rather than report a bogus span.
                DfaSearchOutcome::NoMatch | DfaSearchOutcome::Exhausted => return None,
            }
        };
        // Greedy-extension pass: run the forward-anchored DFA from the
        // recovered start to find the greedy end. For patterns without
        // greedy tails (like `\w\w`) this produces the same end as
        // `first_accept_end`; for greedy patterns (like `a+`) it
        // extends the match as far as the body allows.
        let anchored_dfa = self.should_dispatch_to_dfa()?;
        let greedy_end = {
            let mut dfa = anchored_dfa.lock().ok()?;
            match dfa.find_match_at(input, start) {
                DfaSearchOutcome::Match(end) => end,
                DfaSearchOutcome::NoMatch | DfaSearchOutcome::Exhausted => return None,
            }
        };
        let pike_match = crate::c2::pike::pike_captures_at(c2, input, start)?;
        let mut match_result = pike_match_to_match_result(pike_match);
        // `pike_captures_at` starts the VM at `start`; its end offset
        // should agree with the anchored DFA's greedy end. If they
        // disagree, fall back rather than trust either in isolation.
        if match_result.end != greedy_end {
            return None;
        }
        debug_assert_eq!(match_result.start, start);
        match_result.start = start;
        match_result.end = greedy_end;
        Some(Some(match_result))
    }

    /// Try to answer `find_all` via the lazy DFA. Returns:
    /// - `Some(matches)` if the DFA produced a definitive list of all
    ///   non-overlapping matches (captures recovered via Pike-VM)
    /// - `None` if the DFA isn't available or the cache exhausted at
    ///   any point during the scan
    ///
    /// Same advance rules as `pike_find_all` and the existing VM:
    /// after a non-empty match next scan starts at end, after an empty
    /// match next scan starts one byte later, and an empty match
    /// adjacent to a previous non-empty match is dropped.
    ///
    /// Uses the same reverse-DFA pipeline as [`Self::try_dfa_find_first`]
    /// when the pattern has no prefix hint — the forward-unanchored DFA
    /// finds each match's first-accept end in one O(n - pos) walk,
    /// the reverse-anchored DFA locates the leftmost start bounded to
    /// `>= pos` so the reverse walk can't report a start inside a
    /// previously-consumed span, and the forward-anchored DFA runs
    /// the greedy extension. For prefix-rich patterns the scan falls
    /// through to the per-position anchored DFA scan with
    /// [`PrefixScanner`] skip acceleration.
    #[doc(hidden)]
    pub fn try_dfa_find_all(&self, input: &[u8]) -> Option<Vec<MatchResult>> {
        let c2 = self.vm.program.c2_program.as_ref()?;
        // Inner-literal fast-fail (see `try_dfa_is_match` for rationale).
        if let Some(b) = self.required_inner_byte_for_dispatch() {
            if memchr::memchr(b, input).is_none() {
                return Some(Vec::new());
            }
        }
        // Reverse-DFA pipeline fast path for find_all. Same gate as
        // find_first — use only when no prefix hint is available.
        if self.pipeline_dispatch_preferred(c2) {
            if let Some(forward_mutex) = self.should_dispatch_to_forward_unanchored_dfa() {
                if let Some(reverse_mutex) = self.should_dispatch_to_reverse_dfa() {
                    if let Some(results) =
                        self.try_pipeline_find_all(input, c2, forward_mutex, reverse_mutex)
                    {
                        return Some(results);
                    }
                    // Pipeline exhausted or reconciled against itself —
                    // fall through to the per-position scan.
                }
            }
        }
        let dfa_mutex = self.should_dispatch_to_dfa()?;
        let scanner = PrefixScanner::new(&self.vm, c2.c2_prefix_byte);
        let mut results = Vec::new();
        let mut start = 0usize;
        let mut prev_non_empty_end: Option<usize> = None;
        while start <= input.len() {
            let Some(candidate) = scanner.next_candidate(input, start) else {
                break;
            };
            start = candidate;
            let outcome = {
                let mut dfa = dfa_mutex.lock().ok()?;
                dfa.find_match_at(input, start)
            };
            match outcome {
                DfaSearchOutcome::Match(end) => {
                    let is_empty = end == start;
                    if is_empty && Some(start) == prev_non_empty_end {
                        start += 1;
                        continue;
                    }
                    let pike_match = crate::c2::pike::pike_captures_at(c2, input, start)?;
                    results.push(pike_match_to_match_result(pike_match));
                    prev_non_empty_end = if is_empty { None } else { Some(end) };
                    start = if is_empty { start + 1 } else { end };
                }
                DfaSearchOutcome::NoMatch => start += 1,
                DfaSearchOutcome::Exhausted => return None,
            }
        }
        Some(results)
    }

    /// Reverse-DFA pipeline driver for `find_all`. Returns
    /// `Some(results)` on success (including an empty vec when the
    /// pattern has no matches), or `None` to signal the caller to
    /// fall through to the per-position scan (cache exhausted, mutex
    /// unavailable, or a reconciliation failure).
    ///
    /// Each iteration runs the same 3-pass pipeline as
    /// [`Self::try_pipeline_find_first`] but seeded at the current
    /// scan position:
    ///
    /// 1. Forward-unanchored `find_first_accept_at(input, pos)` —
    ///    first-accept end of the next leftmost match at-or-after
    ///    `pos`. `NoMatch` means the scan is done.
    /// 2. Reverse-anchored `find_match_start_at_reverse_bounded(
    ///    input, first_accept_end, pos)` — the bound prevents the
    ///    reverse walk from locating a start position inside a
    ///    previously-consumed span.
    /// 3. Forward-anchored `find_match_at(input, start)` — greedy end.
    /// 4. Pike-VM `pike_captures_at(c2, input, start)` — captures.
    ///
    /// The find_all advance rules (non-empty → end, empty adjacent
    /// to a previous non-empty → skip by 1, empty otherwise →
    /// `start + 1`) are applied on each iteration. An exhaustion or
    /// reconciliation failure at any pass aborts with `None`.
    fn try_pipeline_find_all(
        &self,
        input: &[u8],
        c2: &crate::c2::CompiledC2Program,
        forward_mutex: &Mutex<LazyDfa>,
        reverse_mutex: &Mutex<LazyDfa>,
    ) -> Option<Vec<MatchResult>> {
        let anchored_dfa = self.should_dispatch_to_dfa()?;
        let mut results = Vec::new();
        let mut pos = 0usize;
        let mut prev_non_empty_end: Option<usize> = None;
        while pos <= input.len() {
            let first_accept_end = {
                let mut forward = forward_mutex.lock().ok()?;
                match forward.find_first_accept_at(input, pos) {
                    DfaSearchOutcome::Match(end) => end,
                    DfaSearchOutcome::NoMatch => break,
                    DfaSearchOutcome::Exhausted => return None,
                }
            };
            let start = {
                let mut reverse = reverse_mutex.lock().ok()?;
                match reverse.find_match_start_at_reverse_bounded(input, first_accept_end, pos) {
                    DfaSearchOutcome::Match(start) => start,
                    DfaSearchOutcome::NoMatch | DfaSearchOutcome::Exhausted => return None,
                }
            };
            let greedy_end = {
                let mut dfa = anchored_dfa.lock().ok()?;
                match dfa.find_match_at(input, start) {
                    DfaSearchOutcome::Match(end) => end,
                    DfaSearchOutcome::NoMatch | DfaSearchOutcome::Exhausted => return None,
                }
            };
            let is_empty = greedy_end == start;
            if is_empty && Some(start) == prev_non_empty_end {
                pos += 1;
                continue;
            }
            let pike_match = crate::c2::pike::pike_captures_at(c2, input, start)?;
            let mut match_result = pike_match_to_match_result(pike_match);
            if match_result.end != greedy_end {
                return None;
            }
            debug_assert_eq!(match_result.start, start);
            match_result.start = start;
            match_result.end = greedy_end;
            results.push(match_result);
            prev_non_empty_end = if is_empty { None } else { Some(greedy_end) };
            pos = if is_empty { start + 1 } else { greedy_end };
        }
        Some(results)
    }

    /// Pike-VM `is_match` dispatch with `PrefixScanner` skip acceleration.
    ///
    /// Mirrors [`Self::try_dfa_is_match`] for patterns that are Pike-VM
    /// eligible but DFA-ineligible (i.e., contain zero-width assertions
    /// like `\b` / `^` / `\A`, or contain lazy quantifiers). Returns
    /// `Some(true)` / `Some(false)` if the Pike-VM produced a definitive
    /// answer, or `None` if Pike-VM dispatch is disabled (the caller
    /// falls back to the existing backtracking VM).
    #[doc(hidden)]
    pub fn try_pike_is_match(&self, input: &[u8]) -> Option<bool> {
        let c2 = self.should_dispatch_to_c2()?;
        let scanner = PrefixScanner::new(&self.vm, c2.c2_prefix_byte);
        let mut start = 0usize;
        while let Some(candidate) = scanner.next_candidate(input, start) {
            if crate::c2::pike::pike_is_match_at(c2, input, candidate) {
                return Some(true);
            }
            start = candidate + 1;
        }
        // The scanner consumes positions strictly less than `input.len()`
        // for byte-content filters; for an empty-match pattern with
        // `PrefixFilter::None` we still need to try `start == input.len()`.
        if matches!(scanner.filter, PrefixFilter::None) {
            for pos in 0..=input.len() {
                if crate::c2::pike::pike_is_match_at(c2, input, pos) {
                    return Some(true);
                }
            }
        }
        Some(false)
    }

    /// Pike-VM `find_first` dispatch with `PrefixScanner` skip acceleration.
    ///
    /// Mirrors [`Self::try_dfa_find_first`] for the Pike-VM tier:
    /// scans only candidate positions reported by [`PrefixScanner`],
    /// runs the capture-tracking simulator at each, returns the first
    /// match (or `Some(None)` if none). Returns `None` when Pike-VM
    /// dispatch is disabled.
    #[doc(hidden)]
    pub fn try_pike_find_first(&self, input: &[u8]) -> Option<Option<MatchResult>> {
        let c2 = self.should_dispatch_to_c2()?;
        let scanner = PrefixScanner::new(&self.vm, c2.c2_prefix_byte);
        let mut start = 0usize;
        while let Some(candidate) = scanner.next_candidate(input, start) {
            if let Some(pike_match) = crate::c2::pike::pike_captures_at(c2, input, candidate) {
                return Some(Some(pike_match_to_match_result(pike_match)));
            }
            start = candidate + 1;
        }
        // For zero-width patterns the scanner stops one byte short of
        // input.len(). Re-try the trailing position with `PrefixFilter::None`.
        if matches!(scanner.filter, PrefixFilter::None) {
            if let Some(pike_match) = crate::c2::pike::pike_captures_at(c2, input, input.len()) {
                return Some(Some(pike_match_to_match_result(pike_match)));
            }
        }
        Some(None)
    }

    /// Pike-VM `find_all` dispatch with `PrefixScanner` skip acceleration.
    ///
    /// Same advance rules as [`Self::try_dfa_find_all`]: after a
    /// non-empty match the next scan starts at the match end; after
    /// an empty match the next scan starts one byte later; an empty
    /// match adjacent to a previous non-empty match is dropped.
    /// Returns `None` when Pike-VM dispatch is disabled.
    #[doc(hidden)]
    pub fn try_pike_find_all(&self, input: &[u8]) -> Option<Vec<MatchResult>> {
        let c2 = self.should_dispatch_to_c2()?;
        let scanner = PrefixScanner::new(&self.vm, c2.c2_prefix_byte);
        let mut results = Vec::new();
        let mut start = 0usize;
        let mut prev_non_empty_end: Option<usize> = None;
        while start <= input.len() {
            let candidate = match scanner.next_candidate(input, start) {
                Some(c) => c,
                None => {
                    // For zero-width / empty-match patterns the
                    // PrefixFilter::None scanner reports every position
                    // already, so there's nothing to retry. For
                    // byte-content filters there's nothing left to
                    // examine.
                    break;
                }
            };
            start = candidate;
            let Some(pike_match) = crate::c2::pike::pike_captures_at(c2, input, start) else {
                start += 1;
                continue;
            };
            let end = pike_match.end;
            let is_empty = end == start;
            if is_empty && Some(start) == prev_non_empty_end {
                start += 1;
                continue;
            }
            results.push(pike_match_to_match_result(pike_match));
            prev_non_empty_end = if is_empty { None } else { Some(end) };
            start = if is_empty { start + 1 } else { end };
        }
        Some(results)
    }

    // ============================================================
    // C1 step 5 — JIT dispatch
    // ============================================================

    /// Returns the JIT-compiled program for this engine if the
    /// pattern is JIT-eligible AND the runtime state allows JIT
    /// dispatch (no event observer, no runtime safety limits —
    /// same constraints as [`Engine::should_dispatch_to_c2`]).
    ///
    /// Also returns `None` for pure-literal patterns: the existing
    /// VM has a `memchr::memmem::Finder` fast path that bypasses
    /// the VM entirely for those, and that fast path is faster
    /// than anything the JIT can do for a pure literal.
    ///
    /// The returned `Mutex` must be locked by the caller. The
    /// JIT host's symbol table is interior-mutable, so the lock
    /// is required even from `&self` — but it's held only briefly
    /// to retrieve the function pointer; the actual JIT'd-function
    /// call happens after the lock is released.
    #[cfg(feature = "jit")]
    #[doc(hidden)]
    pub fn should_use_jit(&self) -> Option<&Mutex<crate::c1::JitProgram>> {
        let jit = self
            .jit_program
            .get_or_init(|| build_jit_program_if_eligible(&self.ast, &self.vm.program))
            .as_ref()?;
        if self.vm.has_event_observer() {
            return None;
        }
        // Step 7: previously this gate excluded all
        // `has_runtime_match_limits` patterns. Now the JIT
        // enforces `max_steps` and `max_backtrack_frames`
        // inline (via `emit_step_limit_check` /
        // `emit_backtrack_push`'s user-limit check), so those
        // limits no longer disqualify a pattern. Recursion is
        // still excluded — the JIT doesn't lower `Call` opcodes,
        // so a recursion depth limit is meaningless for JIT'd
        // code, and patterns that USE recursion are already
        // rejected by the JIT eligibility check.
        if self.vm.has_recursion_depth_limit() {
            return None;
        }
        if self.vm.has_literal_finder() {
            return None;
        }
        Some(jit)
    }

    /// `should_use_jit` stub when the `jit` feature is disabled.
    /// Always returns `None`. Lets `lib.rs` dispatch chains call
    /// the same accessor regardless of the feature flag.
    #[cfg(not(feature = "jit"))]
    #[doc(hidden)]
    pub fn should_use_jit(&self) -> Option<()> {
        None
    }

    /// Try to answer `is_match` via the JIT. Returns:
    /// - `Some(true)` / `Some(false)` if the JIT path produced a
    ///   definitive answer
    /// - `None` if the JIT isn't available or runtime state forbids
    ///   JIT dispatch
    ///
    /// The caller (`Regex::is_match` in `lib.rs`) falls back to
    /// the C2 / Pike-VM / interpreter dispatch chain when this
    /// returns `None`.
    ///
    /// The JIT'd function tests the pattern at exactly one position;
    /// this method scans every position from 0..=input.len() and
    /// returns true on the first successful match. The scan loop
    /// uses the existing `PrefixScanner` for skip acceleration.
    ///
    /// Step 4b: a captures buffer is allocated and reset between
    /// calls so the JIT can write capture spans for groups 1+ even
    /// though `is_match` discards them — the JIT'd function signature
    /// always requires the buffer.
    #[cfg(feature = "jit")]
    #[doc(hidden)]
    pub fn try_jit_is_match(&self, input: &[u8]) -> Option<bool> {
        const LIMIT_SENTINEL: isize = crate::c1::JIT_LIMIT_EXCEEDED_SENTINEL as isize;
        // Step 7: when the JIT returns the limit-abort sentinel,
        // the engine treats the call as "no match" — same
        // user-visible behaviour as the interpreter, which
        // returns false from its main loop on limit overflow.
        // We do NOT return None (which would fall through to
        // the interpreter and double-execute the same hopeless
        // pattern); we return Some(false) directly, matching
        // what the interpreter would have done.
        let jit_mutex = self.should_use_jit()?;
        let func = self.jit_function_ptr(jit_mutex);
        let num_groups = self.vm.program.num_groups;
        let cc_ptr = self.vm.program.char_classes.as_ptr() as *const u8;
        let cc_len = self.vm.program.char_classes.len();
        // Step 7: thread the user-configured runtime safety
        // limits through to the JIT'd function as 7th and 8th
        // args. 0 = unlimited (the JIT's hard cap of
        // C1_BACKTRACK_STACK_FRAMES = 256 still applies for
        // backtrack frames). On limit overflow the JIT returns
        // -2 (JIT_LIMIT_EXCEEDED_SENTINEL); the scan loops below
        // detect that and stop scanning entirely.
        let max_steps = self.vm.max_steps();
        let max_bt_frames = self.vm.max_backtrack_frames();
        let mut captures = new_capture_buffer(num_groups);
        let scanner = PrefixScanner::new(&self.vm, None);
        let mut start = 0usize;
        while let Some(candidate) = scanner.next_candidate(input, start) {
            reset_capture_buffer(&mut captures);
            // SAFETY: input is alive for the call; the function
            // pointer is alive for the lifetime of the JitProgram
            // which is held by self via the Mutex; the captures
            // buffer is sized 2*(num_groups+1) and pre-initialised
            // to -1 in every slot; cc_ptr / cc_len describe the
            // program's char_classes Vec which lives as long as
            // self.vm and is never mutated after engine creation.
            let result = unsafe {
                func(
                    input.as_ptr(),
                    input.len(),
                    candidate,
                    captures.as_mut_ptr(),
                    cc_ptr,
                    cc_len,
                    max_steps,
                    max_bt_frames,
                )
            };
            if result >= 0 {
                return Some(true);
            }
            if result == LIMIT_SENTINEL {
                // Limit hit — stop scanning. Matches interpreter behavior.
                return Some(false);
            }
            start = candidate + 1;
        }
        // Empty-match patterns may still match at input.len() even
        // when no consuming-byte position was a candidate. Try the
        // trailing position once.
        if start <= input.len() {
            reset_capture_buffer(&mut captures);
            let result = unsafe {
                func(
                    input.as_ptr(),
                    input.len(),
                    input.len(),
                    captures.as_mut_ptr(),
                    cc_ptr,
                    cc_len,
                    max_steps,
                    max_bt_frames,
                )
            };
            if result >= 0 {
                return Some(true);
            }
            // (limit sentinel falls through to Some(false) below)
        }
        Some(false)
    }

    /// Try to answer `find_first` via the JIT. Returns the leftmost
    /// match found by scanning positions and calling the JIT'd
    /// function at each candidate, or `None` if the JIT isn't
    /// available.
    #[cfg(feature = "jit")]
    #[doc(hidden)]
    pub fn try_jit_find_first(&self, input: &[u8]) -> Option<Option<MatchResult>> {
        const LIMIT_SENTINEL: isize = crate::c1::JIT_LIMIT_EXCEEDED_SENTINEL as isize;
        let jit_mutex = self.should_use_jit()?;
        let func = self.jit_function_ptr(jit_mutex);
        let num_groups = self.vm.program.num_groups;
        let cc_ptr = self.vm.program.char_classes.as_ptr() as *const u8;
        let cc_len = self.vm.program.char_classes.len();
        // Step 7: thread the user-configured runtime safety
        // limits through to the JIT'd function as 7th and 8th
        // args. 0 = unlimited (the JIT's hard cap of
        // C1_BACKTRACK_STACK_FRAMES = 256 still applies for
        // backtrack frames). On limit overflow the JIT returns
        // -2 (JIT_LIMIT_EXCEEDED_SENTINEL); the scan loops below
        // detect that and stop scanning entirely.
        let max_steps = self.vm.max_steps();
        let max_bt_frames = self.vm.max_backtrack_frames();
        let mut captures = new_capture_buffer(num_groups);
        let scanner = PrefixScanner::new(&self.vm, None);
        let mut start = 0usize;
        while let Some(candidate) = scanner.next_candidate(input, start) {
            reset_capture_buffer(&mut captures);
            // SAFETY: see try_jit_is_match.
            let result = unsafe {
                func(
                    input.as_ptr(),
                    input.len(),
                    candidate,
                    captures.as_mut_ptr(),
                    cc_ptr,
                    cc_len,
                    max_steps,
                    max_bt_frames,
                )
            };
            if result >= 0 {
                #[allow(clippy::cast_sign_loss)] // checked >= 0
                let end = result as usize;
                return Some(Some(jit_match_to_result(
                    candidate, end, &captures, num_groups,
                )));
            }
            if result == LIMIT_SENTINEL {
                // Limit hit — stop scanning. Matches interpreter behavior.
                return Some(None);
            }
            start = candidate + 1;
        }
        // Try the trailing position for empty-match patterns.
        if start <= input.len() {
            reset_capture_buffer(&mut captures);
            let result = unsafe {
                func(
                    input.as_ptr(),
                    input.len(),
                    input.len(),
                    captures.as_mut_ptr(),
                    cc_ptr,
                    cc_len,
                    max_steps,
                    max_bt_frames,
                )
            };
            if result >= 0 {
                #[allow(clippy::cast_sign_loss)] // checked >= 0
                let end = result as usize;
                return Some(Some(jit_match_to_result(
                    input.len(),
                    end,
                    &captures,
                    num_groups,
                )));
            }
            // (limit sentinel falls through to Some(None) below)
        }
        Some(None)
    }

    /// Try to answer `find_all` via the JIT. Returns the full list
    /// of non-overlapping matches found by scanning positions and
    /// calling the JIT'd function. Same advance rules as
    /// `try_dfa_find_all`: after a non-empty match the next scan
    /// starts at the match end; after an empty match the next scan
    /// starts one byte later; an empty match adjacent to a previous
    /// non-empty match is dropped.
    #[cfg(feature = "jit")]
    #[doc(hidden)]
    pub fn try_jit_find_all(&self, input: &[u8]) -> Option<Vec<MatchResult>> {
        const LIMIT_SENTINEL: isize = crate::c1::JIT_LIMIT_EXCEEDED_SENTINEL as isize;
        let jit_mutex = self.should_use_jit()?;
        let func = self.jit_function_ptr(jit_mutex);
        let num_groups = self.vm.program.num_groups;
        let cc_ptr = self.vm.program.char_classes.as_ptr() as *const u8;
        let cc_len = self.vm.program.char_classes.len();
        // Step 7: thread the user-configured runtime safety
        // limits through to the JIT'd function as 7th and 8th
        // args. 0 = unlimited (the JIT's hard cap of
        // C1_BACKTRACK_STACK_FRAMES = 256 still applies for
        // backtrack frames). On limit overflow the JIT returns
        // -2 (JIT_LIMIT_EXCEEDED_SENTINEL); the scan loops below
        // detect that and stop scanning entirely.
        let max_steps = self.vm.max_steps();
        let max_bt_frames = self.vm.max_backtrack_frames();
        let mut captures = new_capture_buffer(num_groups);
        let scanner = PrefixScanner::new(&self.vm, None);
        let mut results = Vec::new();
        let mut start = 0usize;
        let mut prev_non_empty_end: Option<usize> = None;
        while start <= input.len() {
            let Some(candidate) = scanner.next_candidate(input, start) else {
                break;
            };
            start = candidate;
            reset_capture_buffer(&mut captures);
            // SAFETY: see try_jit_is_match.
            let result = unsafe {
                func(
                    input.as_ptr(),
                    input.len(),
                    start,
                    captures.as_mut_ptr(),
                    cc_ptr,
                    cc_len,
                    max_steps,
                    max_bt_frames,
                )
            };
            if result == LIMIT_SENTINEL {
                // Limit hit — stop scanning, return matches
                // collected so far. Matches the interpreter's
                // behaviour of bailing out of the find_all loop
                // on limit overflow (no error, just no more
                // matches).
                break;
            }
            if result < 0 {
                start += 1;
                continue;
            }
            #[allow(clippy::cast_sign_loss)] // checked >= 0
            let end = result as usize;
            let is_empty = end == start;
            if is_empty && Some(start) == prev_non_empty_end {
                start += 1;
                continue;
            }
            results.push(jit_match_to_result(start, end, &captures, num_groups));
            prev_non_empty_end = if is_empty { None } else { Some(end) };
            start = if is_empty { start + 1 } else { end };
        }
        Some(results)
    }

    /// Retrieve the raw function pointer from the JIT program,
    /// transmuted to the typed `Step3aJittedFn` C ABI signature.
    /// Locks the mutex briefly, fetches the pointer, releases
    /// the lock, and returns the typed function. The function
    /// pointer is valid for the lifetime of the engine because
    /// the underlying `JitProgram` is owned by the Mutex which
    /// is owned by the engine.
    #[cfg(feature = "jit")]
    fn jit_function_ptr(
        &self,
        jit_mutex: &Mutex<crate::c1::JitProgram>,
    ) -> crate::c1::Step3aJittedFn {
        let jit = jit_mutex.lock().expect("JitProgram mutex poisoned");
        let raw = jit.raw_fn_ptr();
        // SAFETY: the function pointer was finalized by
        // `compile_program_to_jit_program` and points at executable
        // memory owned by the JitProgram, which is alive for the
        // lifetime of self. The signature `(i64, i64, i64) -> i64`
        // matches the `Step3aJittedFn` C ABI exactly.
        unsafe { std::mem::transmute(raw) }
    }

    /// C2 classification of the compiled pattern this engine was built for.
    ///
    /// At C2 step 4c, this remains the source of truth for "is the
    /// pattern classifier-positive". Engine dispatch is decided by
    /// [`Engine::c2_program`] which adds a structural eligibility check
    /// on top of the classification.
    #[doc(hidden)]
    pub fn classification(&self) -> &Classification {
        &self.vm.program.classification
    }

    /// The compiled C2 program for this engine, if the pattern is
    /// **structurally** eligible for Pike-VM dispatch (classifier
    /// positive plus the C2 structural eligibility checks). This is the
    /// raw compile-time check; runtime state (event observers, step
    /// limits) is not considered. Use [`Engine::should_dispatch_to_c2`]
    /// for the full dispatch decision.
    #[doc(hidden)]
    pub fn c2_program(&self) -> Option<&crate::c2::CompiledC2Program> {
        self.vm.program.c2_program.as_ref()
    }

    /// Returns `Some(c2_program)` iff this engine should route the
    /// next API call through the C2 Pike-VM. Combines the compile-time
    /// `c2_program` presence check with several gates:
    ///
    /// - **Runtime feature gates**: skips Pike-VM dispatch when an event
    ///   observer or any runtime safety limit (`max_steps`,
    ///   `max_backtrack_frames`, `max_recursion_depth`) is active. The
    ///   Pike-VM doesn't emit structured match events and isn't bounded
    ///   by these limits — patterns relying on either continue to run
    ///   on the existing backtracking VM.
    /// - **Pure-literal gate**: skips Pike-VM for patterns that the
    ///   existing VM handles via its `memchr::memmem::Finder` fast
    ///   path. That bypass is faster than anything Pike-VM can do for
    ///   a pure literal.
    /// - **Nested-quantifier gate**: Pike-VM dispatch only fires for
    ///   patterns with structurally nested quantifiers (`(a+)+`,
    ///   `(\w+\s+)+`, …). Those are the patterns where the existing
    ///   backtracking VM can blow up exponentially and Pike-VM's O(nm)
    ///   bound provides a strict improvement. Classifier-positive
    ///   patterns WITHOUT nested quantifiers — like `\b\w+@\w+\.\w+\b`
    ///   or `\d{3}-\d{2}-\d{4}` — run efficiently on the existing VM
    ///   (no exponential risk by construction) and dispatching them to
    ///   Pike-VM would be a 2-3x regression because Pike-VM's per-trial
    ///   cost (epsilon-closure of the start set, sparse-set ops per
    ///   byte) is higher than the existing VM's tight bytecode
    ///   interpreter loop. The nested-quantifier check is computed once
    ///   at compile time on the AST.
    ///
    /// When `None`, the caller falls back to the existing backtracking
    /// VM. The runtime checks are read on every call so that
    /// `Regex::on_event(...)` and `Regex::set_max_steps(...)` take
    /// effect immediately even if invoked AFTER `Regex::compile`.
    #[doc(hidden)]
    pub fn should_dispatch_to_c2(&self) -> Option<&crate::c2::CompiledC2Program> {
        let c2 = self.vm.program.c2_program.as_ref()?;
        if self.vm.has_event_observer() {
            return None;
        }
        if self.vm.has_runtime_match_limits() {
            return None;
        }
        if self.vm.has_literal_finder() {
            return None;
        }
        if !c2.c2_has_nested_quantifier {
            return None;
        }
        Some(c2)
    }

    /// Find all non-overlapping matches in the input
    #[must_use]
    pub fn find_all(&self, text: &[u8]) -> Vec<MatchResult> {
        trace_enter!(
            "engine",
            "Engine::find_all",
            "input_bytes={},mode={:?}",
            text.len(),
            self.mode
        );
        // Convert bytes to string for VM processing
        let text_str = match std::str::from_utf8(text) {
            Ok(s) => {
                trace_decision!(
                    "engine",
                    "std::str::from_utf8(text).is_ok()",
                    true,
                    "dispatching text to VM find_all path"
                );
                s
            }
            Err(err) => {
                trace_decision!(
                    "engine",
                    "std::str::from_utf8(text).is_ok()",
                    false,
                    "invalid UTF-8 input rejected: {}",
                    err
                );
                trace_exit!(
                    "engine",
                    "Engine::find_all",
                    "ok=true,matches=0,reason=invalid_utf8"
                );
                return Vec::new();
            } // Invalid UTF-8
        };

        let matches = self
            .vm
            .find_all(text_str)
            .into_iter()
            .map(vm_match_to_result)
            .collect::<Vec<_>>();
        trace_decision!(
            "engine",
            "matches.is_empty()",
            matches.is_empty(),
            "vm find_all produced {} matches",
            matches.len()
        );
        trace_exit!(
            "engine",
            "Engine::find_all",
            "ok=true,matches={}",
            matches.len()
        );
        matches
    }

    /// Find the first match, accepting a pre-validated `&str` directly.
    ///
    /// Used by `bytes::BytesRegex` which handles the UTF-8 boundary itself.
    #[must_use]
    pub(crate) fn vm_find_first(&self, text: &str) -> Option<MatchResult> {
        self.vm.find_first(text).map(vm_match_to_result)
    }

    /// Find all matches, accepting a pre-validated `&str` directly.
    #[must_use]
    pub(crate) fn vm_find_all(&self, text: &str) -> Vec<MatchResult> {
        self.vm
            .find_all(text)
            .into_iter()
            .map(vm_match_to_result)
            .collect()
    }

    /// Find the first match in the input
    #[must_use]
    pub fn find_first(&self, text: &[u8]) -> Option<MatchResult> {
        trace_enter!(
            "engine",
            "Engine::find_first",
            "input_bytes={},mode={:?}",
            text.len(),
            self.mode
        );
        // Convert bytes to string for VM processing
        let text_str = match std::str::from_utf8(text) {
            Ok(s) => {
                trace_decision!(
                    "engine",
                    "std::str::from_utf8(text).is_ok()",
                    true,
                    "dispatching text to VM find_first path"
                );
                s
            }
            Err(err) => {
                trace_decision!(
                    "engine",
                    "std::str::from_utf8(text).is_ok()",
                    false,
                    "invalid UTF-8 input rejected: {}",
                    err
                );
                trace_exit!(
                    "engine",
                    "Engine::find_first",
                    "ok=true,found=false,reason=invalid_utf8"
                );
                return None;
            }
        };

        let first = self.vm.find_first(text_str).map(vm_match_to_result);
        trace_decision!(
            "engine",
            "first.is_some()",
            first.is_some(),
            "vm find_first completed"
        );
        trace_exit!(
            "engine",
            "Engine::find_first",
            "ok=true,found={}",
            first.is_some()
        );
        first
    }

    /// Test if pattern matches the input (fastest operation)
    #[must_use]
    pub fn is_match(&self, text: &[u8]) -> bool {
        trace_enter!(
            "engine",
            "Engine::is_match",
            "input_bytes={},mode={:?}",
            text.len(),
            self.mode
        );
        // Convert bytes to string for VM processing
        if let Ok(text_str) = std::str::from_utf8(text) {
            trace_decision!(
                "engine",
                "std::str::from_utf8(text).is_ok()",
                true,
                "dispatching text to VM is_match path"
            );
            let matched = self.vm.is_match(text_str);
            trace_decision!(
                "engine",
                "vm.is_match(text_str)",
                matched,
                "boolean match evaluation completed"
            );
            trace_exit!("engine", "Engine::is_match", "ok=true,matched={}", matched);
            matched
        } else {
            trace_decision!(
                "engine",
                "std::str::from_utf8(text).is_ok()",
                false,
                "invalid UTF-8 input rejected"
            );
            trace_exit!(
                "engine",
                "Engine::is_match",
                "ok=true,matched=false,reason=invalid_utf8"
            );
            false // Invalid UTF-8 cannot match
        }
    }

    /// Find the first match starting the scan at byte position `start`.
    ///
    /// Positions in the returned `MatchResult` are absolute (relative to the
    /// beginning of `text`, not relative to `start`).
    ///
    /// # Panics
    /// Panics if `start` is not on a UTF-8 character boundary.
    #[must_use]
    pub fn find_first_at(&self, text: &[u8], start: usize) -> Option<MatchResult> {
        let text_str = match std::str::from_utf8(text) {
            Ok(s) => s,
            Err(_) => return None,
        };
        self.vm
            .find_first_at(text_str, start)
            .map(vm_match_to_result)
    }

    /// Find all non-overlapping matches starting the scan at byte position `start`.
    ///
    /// # Panics
    /// Panics if `start` is not on a UTF-8 character boundary.
    #[must_use]
    pub fn find_all_at(&self, text: &[u8], start: usize) -> Vec<MatchResult> {
        let text_str = match std::str::from_utf8(text) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        self.vm
            .find_all_at(text_str, start)
            .into_iter()
            .map(vm_match_to_result)
            .collect()
    }

    /// Boolean match test starting the scan at byte position `start`.
    ///
    /// # Panics
    /// Panics if `start` is not on a UTF-8 character boundary.
    #[must_use]
    pub fn is_match_at(&self, text: &[u8], start: usize) -> bool {
        self.find_first_at(text, start).is_some()
    }

    /// Find the first match with support for async callback suspension.
    ///
    /// This is the suspendable counterpart to [`find_first`](Self::find_first).
    /// When an unregistered native callback is encountered, returns
    /// [`MatchOutcome::Suspended`] with a continuation that can be resumed
    /// after the callback is resolved externally.
    #[must_use]
    pub fn find_first_suspendable(&self, text: &[u8]) -> MatchOutcome {
        let Ok(text_str) = std::str::from_utf8(text) else {
            return MatchOutcome::Completed(None);
        };
        self.vm.find_first_suspendable(text_str)
    }

    /// Resume a suspended match after the caller resolves an async callback.
    ///
    /// See [`MatchContinuation`] for details on the continuation-passing protocol.
    #[must_use]
    pub fn resume(
        &self,
        continuation: MatchContinuation,
        callback_result: ExecResult,
    ) -> MatchOutcome {
        self.vm.resume(continuation, callback_result)
    }

    /// Named capture group map: group name → 1-based group number.
    #[must_use]
    pub fn named_groups(&self) -> &std::collections::HashMap<String, u32> {
        &self.vm.program.named_groups
    }

    /// Multi-id named capture group map: for every group name the
    /// `Vec<u32>` contains every registered id in AST order. For
    /// non-dupnames patterns each Vec has exactly one id. Consumers
    /// that need PCRE2 "any-of" semantics (substitute template
    /// interpolation with dupnames, etc.) should iterate this.
    #[must_use]
    pub fn named_groups_all(&self) -> &std::collections::HashMap<String, Vec<u32>> {
        &self.vm.program.named_groups_all
    }

    /// Number of capture groups in the compiled program (excluding group 0).
    #[must_use]
    pub fn num_groups(&self) -> u32 {
        self.vm.program.num_groups
    }

    /// Find the first match, or report a partial match when the input
    /// ends mid-potential-match.
    #[must_use]
    pub fn find_first_partial(&self, text: &[u8]) -> PartialMatchResult {
        let text_str = match std::str::from_utf8(text) {
            Ok(s) => s,
            Err(_) => return PartialMatchResult::NoMatch,
        };
        self.vm.find_first_partial(text_str)
    }

    /// Set the match semantics (leftmost-first or leftmost-longest).
    pub fn set_match_semantics(&self, semantics: MatchSemantics) {
        self.vm.set_match_semantics(semantics);
    }

    /// Set the maximum number of opcode steps per match attempt.
    ///
    /// Prevents exponential backtracking from hanging the engine on
    /// pathological patterns like `(a+)+b`. When the limit is reached,
    /// the match attempt fails (returns no-match). Pass `None` to
    /// remove the limit (default).
    pub fn set_max_steps(&self, limit: Option<u64>) {
        self.vm.set_max_steps(limit);
    }

    /// Set the maximum backtrack stack depth per match attempt.
    pub fn set_max_backtrack_frames(&self, limit: Option<u64>) {
        self.vm.set_max_backtrack_frames(limit);
    }

    /// Set the maximum recursion depth per match attempt.
    pub fn set_max_recursion_depth(&self, limit: Option<u64>) {
        self.vm.set_max_recursion_depth(limit);
    }

    /// Register a native callback on the engine's execution manager.
    ///
    /// # Errors
    /// Returns `RgxError::Engine` if no execution manager is attached to this engine.
    pub fn register_native<F>(&self, name: &str, callback: F) -> Result<()>
    where
        F: Fn(&ExecContext) -> ExecResult + Send + Sync + 'static,
    {
        self.vm.register_native(name, callback)
    }

    /// Register a named wasm module on the engine's execution manager.
    ///
    /// # Errors
    /// Returns `RgxError::Engine` if no execution manager is attached or the WASM module is invalid.
    pub fn register_wasm_module(&self, name: String, module_bytes: Vec<u8>) -> Result<()> {
        self.vm.register_wasm_module(name, module_bytes)
    }

    /// Register or replace a host-provided execution variable on the engine's execution manager.
    ///
    /// # Errors
    /// Returns `RgxError::Engine` if no execution manager is attached to this engine.
    pub fn set_variable(&self, name: &str, value: String) -> Result<()> {
        self.vm.set_variable(name, value)
    }

    /// Register or replace a typed host-provided execution variable on the engine's execution manager.
    ///
    /// # Errors
    /// Returns `RgxError::Engine` if no execution manager is attached to this engine.
    pub fn set_typed_variable(&self, name: &str, value: crate::execution::Value) -> Result<()> {
        self.vm.set_typed_variable(name, value)
    }

    /// Set a host variable with automatic type conversion.
    ///
    /// # Errors
    /// Returns `RgxError::Engine` if no execution manager is attached to this engine.
    pub fn set_var<V: Into<crate::execution::Value>>(&self, name: &str, value: V) -> Result<()> {
        self.set_typed_variable(name, value.into())
    }

    /// Register an event observer for structured match events.
    ///
    /// The observer receives [`MatchEvent`] values at key execution points.
    /// Only one observer may be active; calling this again replaces any
    /// previous observer.
    pub fn set_event_observer<F>(&self, observer: F)
    where
        F: Fn(&MatchEvent) + Send + Sync + 'static,
    {
        self.vm.set_event_observer(observer);
    }
}

#[cfg(test)]
mod reverse_dfa_pipeline_tests {
    //! Tests focused on the reverse-DFA pipeline dispatch wiring.
    //!
    //! The forward-unanchored + reverse-anchored DFAs are built for
    //! every DFA-eligible pattern. Only `is_match` currently consumes
    //! the forward-unanchored DFA as a single-pass O(n) fast path;
    //! `find_first` / `find_all` keep the per-position anchored scan
    //! because the unanchored DFA's leftmost-LONGEST subset
    //! construction doesn't match the leftmost-first find contract.
    //! These tests pin those behaviors.
    use crate::Regex;

    fn compile(pattern: &str) -> Regex {
        Regex::compile(pattern).expect("pattern compiles")
    }

    #[test]
    fn is_match_single_pass_fast_path_answers_true_for_middle_match() {
        // DFA-eligible pattern with a match in the middle of input.
        // Exercises the forward-unanchored DFA's one-pass O(n) scan.
        let re = compile(r"[a-z]+");
        assert!(re.is_match("123 abc 456"));
    }

    #[test]
    fn is_match_single_pass_fast_path_answers_false_for_no_match() {
        let re = compile(r"[a-z]+");
        assert!(!re.is_match("12345"));
    }

    #[test]
    fn is_match_single_pass_fast_path_on_empty_input() {
        // Edge case: empty input with a pattern that doesn't match
        // empty. Forward-unanchored DFA has nothing to walk.
        let re = compile(r"[a-z]+");
        assert!(!re.is_match(""));
    }

    #[test]
    fn is_match_single_pass_fast_path_on_zero_width_match() {
        // `a*` accepts at the start state immediately. The
        // forward-unanchored DFA's `is_accept(start_state)` branch
        // must return Match(0), not NoMatch.
        let re = compile(r"a*");
        assert!(re.is_match(""));
        assert!(re.is_match("xyz"));
    }

    #[test]
    fn is_match_and_find_first_agree_on_multi_position_literal() {
        // Regression pin: `a` on "xaxa" is the classic case where the
        // forward-unanchored DFA returns end=4 (last accept). If the
        // find_first path ever adopts the unanchored DFA without a
        // leftmost-first-aware algorithm, find_first would return
        // (3, 4) instead of (1, 2). is_match stays correct (true) in
        // both worlds.
        let re = compile(r"a");
        let input = "xaxa";
        assert!(re.is_match(input));
        let m = re.find_first(input).expect("match exists");
        assert_eq!((m.start, m.end), (1, 2), "find_first must be leftmost");
    }

    #[test]
    fn is_match_greedy_quantifier_with_multiple_accepts() {
        // `a+` on "aaa" has accepts at positions 1, 2, 3 (all same
        // match start). The forward-unanchored DFA's last-accept
        // semantics is correct here — single greedy match. is_match
        // returns true regardless.
        let re = compile(r"a+");
        assert!(re.is_match("aaa"));
    }
}
