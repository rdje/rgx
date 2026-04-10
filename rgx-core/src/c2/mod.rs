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
//! C2 is being built incrementally per the §15 phased plan. Each step
//! ships production-quality code; nothing here is throwaway.
//!
//! - **Step 0**: design proposal landed. ✅
//! - **Step 1**: pattern classifier — metadata only, no runtime dispatch. ✅
//! - **Step 2**: byte-class equivalence partitioning — standalone module. ✅
//! - **Step 3a**: forward Thompson NFA construction (anchored + unanchored). ✅
//! - **Step 3b**: reverse NFA construction + `CompiledC2Program` assembly. ✅
//! - **Step 4a**: sparse-set Pike-VM (`is_match` / `find_first` /
//!   `find_all` without captures) plus a differential test against the
//!   existing backtracking VM for match spans. ✅
//! - **Step 4b (this commit)**: Pike-VM capture tracking via per-thread
//!   capture buffers. New `pike_captures` / `pike_captures_all` API
//!   plus extended differential test that compares capture group
//!   positions byte-for-byte against the existing VM. ✅
//! - **Step 4c**: engine dispatch wiring (route classifier-positive
//!   patterns through Pike-VM via the public `Regex` API). (planned)
//! - **Step 5**: lazy forward DFA cache. (planned)
//! - **Step 6**: lazy reverse DFA cache. (planned)
//! - **Step 7**: literal prefix integration. (planned)
//! - **Step 8**: production cutover, benchmarks, Book chapter. (planned)

pub mod byte_class;
pub mod classifier;
pub mod dfa;
pub mod nfa;
pub mod pike;
pub mod program;

pub use byte_class::ByteClassMap;
pub use classifier::{classify, Classification, ExclusionReason};
pub use dfa::{DfaStateId, LazyDfa};
pub use nfa::{reverse_ast, CaptureTag, Nfa, NfaState, NfaStateId, ZeroWidthAssertion};
pub use pike::{
    pike_captures, pike_captures_all, pike_find_all, pike_find_first, pike_is_match, PikeMatch,
};
pub use program::CompiledC2Program;
