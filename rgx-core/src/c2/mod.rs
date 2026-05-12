//! C2: NFA/DFA hybrid engine.
//!
//! C2 is the parallel engine for the no-backtracking subset of regex
//! patterns. It coexists with the existing backtracking VM in `vm.rs` —
//! patterns that fall outside the no-backtracking subset continue to run
//! on the VM unchanged.
//!
//! See `docs/C2_NFA_DFA_DESIGN.md` for the full SOTA design proposal,
//! including the no-backtracking subset definition (§4), sparse-set
//! Pike-VM (§7), lazy DFA cache (§8), two-pass capture recovery (§9),
//! engine dispatch boundary (§11), differential testing strategy (§13),
//! and the phased implementation plan (§15).
//!
//! # Implementation status
//!
//! C2 has shipped through Step 8. The public `Regex` API dispatches
//! through a 3-tier C2 chain — Aho–Corasick literal prefix
//! (`try_ac_*`), lazy DFA (`try_dfa_*`), and sparse-set Pike-VM
//! (`try_pike_*`) — before falling back to the backtracking VM.
//! Classifier-positive patterns reach the appropriate tier
//! automatically. The Book chapter
//! `book/src/internals/nfa-dfa-engine.md` documents the design and
//! dispatch logic in detail.
//!
//! - **Step 0**: design proposal landed. ✅
//! - **Step 1**: pattern classifier — metadata + runtime dispatch gate. ✅
//! - **Step 2**: byte-class equivalence partitioning. ✅
//! - **Step 3a**: forward Thompson NFA construction. ✅
//! - **Step 3b**: reverse NFA construction + `CompiledC2Program`. ✅
//! - **Step 4a**: sparse-set Pike-VM (`is_match` / `find_first` /
//!   `find_all`) with differential test against the VM. ✅
//! - **Step 4b**: Pike-VM capture tracking with per-thread buffers
//!   (`pike_captures` / `pike_captures_all`). ✅
//! - **Step 4c**: engine dispatch wiring (`engine.try_pike_*` routes
//!   through `Regex::find_first`, `find_all`, `is_match`). ✅
//! - **Step 5**: lazy forward DFA cache (`LazyDfa`, `try_dfa_*`). ✅
//! - **Step 6**: lazy reverse DFA cache + reverse-DFA pipeline for
//!   leftmost-first `find_first` / `find_all`. ✅
//! - **Step 7**: Aho–Corasick literal-prefix tier (`try_ac_*`). ✅
//! - **Step 8**: production cutover — public `Regex::uses_c2()` /
//!   `Regex::classification()` introspection promoted from
//!   doc-hidden (2026-05-11); Book chapter shipped. ✅

pub mod byte_class;
pub mod classifier;
pub mod dfa;
pub mod nfa;
pub mod pike;
pub mod program;
pub mod simd_scan;
pub mod tdfa;

pub use byte_class::ByteClassMap;
pub use classifier::{classify, Classification, ExclusionReason};
pub use dfa::{DfaStateId, LazyDfa};
pub use nfa::{reverse_ast, CaptureTag, Nfa, NfaState, NfaStateId, ZeroWidthAssertion};
pub use pike::{
    pike_captures, pike_captures_all, pike_captures_all_with_scratch, pike_captures_at,
    pike_captures_at_with_scratch, pike_find_all, pike_find_first, pike_is_match, pike_is_match_at,
    PikeMatch, PikeScratch,
};
pub use program::CompiledC2Program;
