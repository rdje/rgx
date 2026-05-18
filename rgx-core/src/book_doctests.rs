//! Compiles The RGX Book's code examples as real `rgx-core` doctests.
//!
//! # Why this exists
//!
//! Users copy-paste examples straight out of the book
//! (<https://rdje.github.io/rgx>). An example that does not compile
//! and run is a broken promise — and exactly the kind of rot the
//! Testing-Philosophy chapter says "nothing in this book is a
//! promise that cannot be verified" forbids.
//!
//! `mdbook test` cannot help here: it shells out to `rustdoc`
//! without `--extern rgx_core=…`, so every `use rgx_core::…` in a
//! book snippet fails to resolve. Instead each chapter is pulled in
//! below with `#[doc = include_str!(…)]` under `#[cfg(doctest)]`.
//! `cargo test -p rgx-core --doc` then extracts and runs every
//! non-`ignore` ```` ```rust ```` block in those chapters **as a
//! doctest of this crate**, so `rgx_core` and its whole dependency
//! graph resolve natively — the snippet is compiled exactly as a
//! user pasting it into a project that depends on `rgx-core` would
//! experience.
//!
//! This rides the existing mandatory gate (`cargo test -p rgx-core`,
//! receipt-guarded, ratcheted) — no new CI job, no bypass.
//!
//! # Annotation contract (see Testing-Philosophy → "Verified book
//! examples")
//!
//! - ```` ```rust ````          → compiled **and run** (pure-API).
//! - ```` ```rust,no_run ````   → compiled & type-checked, not run
//!   (servers / IO / network / long-running).
//! - ```` ```rust,ignore ````   → last resort, each justified; not
//!   verified.
//! - Hidden `# ` lines carry imports / `fn main` so the *visible*
//!   snippet stays clean and copy-pasteable while the compiled unit
//!   is complete.
//!
//! # Coverage ratchet
//!
//! Chapters are added below incrementally (highest-traffic first).
//! The set only ever grows — re-`ignore`-ing a block to dodge the
//! gate is a regression. Newly-wired chapters are listed in
//! `book/.examples-verified-chapters`; `scripts/check-book-examples.sh`
//! enforces that the wired count never drops.
//!
//! `#[cfg(doctest)]` keeps all of this out of normal builds — it is
//! compiled only when `rustdoc` is collecting doctests.

#![cfg(doctest)]

/// Real-world → HTTP Router. Verified 2026-05-18 as the first
/// chapter on the ratchet (the example the gap was reported
/// against).
#[doc = include_str!("../../book/src/real-world/http-router.md")]
pub struct HttpRouter;

// --- Campaign increment 1 (2026-05-18): introduction + why-rgx +
// getting-started/* (highest-traffic chapters; see BACKLOG C12). ---

/// Introduction — the "Quick taste" example.
#[doc = include_str!("../../book/src/introduction.md")]
pub struct Introduction;

/// Beyond regex (why-rgx) — the seven-differentiators tour. The
/// lua / native / tail_file / observer fragments are `no_run`
/// (feature- or IO-gated); they are still compiled, which is what
/// caught the `SteerAction`→`SteerResult`,
/// `CodeBlockValue::Number`→`Numeric`, and
/// `with_event_observer`→`on_event` drift this chapter shipped.
#[doc = include_str!("../../book/src/why-rgx.md")]
pub struct WhyRgx;

/// Getting Started → Installation & First Match.
#[doc = include_str!("../../book/src/getting-started/first-match.md")]
pub struct GsFirstMatch;

/// Getting Started → Finding Matches.
#[doc = include_str!("../../book/src/getting-started/finding-matches.md")]
pub struct GsFindingMatches;

/// Getting Started → Capture Groups.
#[doc = include_str!("../../book/src/getting-started/capture-groups.md")]
pub struct GsCaptureGroups;

/// Getting Started → Replace & Split.
#[doc = include_str!("../../book/src/getting-started/replace-and-split.md")]
pub struct GsReplaceAndSplit;

/// Getting Started → RegexBuilder & Configuration.
#[doc = include_str!("../../book/src/getting-started/regex-builder.md")]
pub struct GsRegexBuilder;

// --- Campaign increment 2 (2026-05-18): core-api/* (the full
// public type system; all pure-API + thread-safe, run as doctests). ---

/// Core API → The Match Type.
#[doc = include_str!("../../book/src/core-api/match-type.md")]
pub struct CaMatchType;

/// Core API → Iterators.
#[doc = include_str!("../../book/src/core-api/iterators.md")]
pub struct CaIterators;

/// Core API → Position-Aware Matching.
#[doc = include_str!("../../book/src/core-api/position-aware.md")]
pub struct CaPositionAware;

/// Core API → RegexSet.
#[doc = include_str!("../../book/src/core-api/regex-set.md")]
pub struct CaRegexSet;

/// Core API → RegexCache (incl. the thread-safety example).
#[doc = include_str!("../../book/src/core-api/regex-cache.md")]
pub struct CaRegexCache;

/// Core API → BytesRegex.
#[doc = include_str!("../../book/src/core-api/bytes-regex.md")]
pub struct CaBytesRegex;

/// Core API → Safety Limits (incl. the compile-time nesting guard).
#[doc = include_str!("../../book/src/core-api/safety-limits.md")]
pub struct CaSafetyLimits;

/// Core API → Error Diagnostics.
#[doc = include_str!("../../book/src/core-api/error-diagnostics.md")]
pub struct CaErrorDiagnostics;
