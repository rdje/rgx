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
//! - **Step 2 (this commit)**: byte-class equivalence partitioning —
//!   standalone module, no engine wiring. ✅
//! - **Step 3**: forward + reverse Thompson NFA construction. (planned)
//! - **Step 4**: sparse-set Pike-VM with differential gate active. (planned)
//! - **Step 5**: lazy forward DFA cache. (planned)
//! - **Step 6**: lazy reverse DFA cache. (planned)
//! - **Step 7**: literal prefix integration. (planned)
//! - **Step 8**: production cutover, benchmarks, Book chapter. (planned)

pub mod byte_class;
pub mod classifier;

pub use byte_class::ByteClassMap;
pub use classifier::{classify, Classification, ExclusionReason};
