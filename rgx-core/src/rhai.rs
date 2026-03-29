//! Rhai code execution backend for rgx patterns.
//!
//! This module provides sandboxed Rhai execution for `(?{rhai:...})` patterns.
//! Rhai is embedded in-process with no filesystem, network, or module-loading
//! surface wired into rgx.
//!
//! # Usage
//!
//! ```rust,ignore
//! use rgx_core::rhai::RhaiEngine;
//!
//! let engine = RhaiEngine::new()?;
//! let result = engine.execute(r#"vars["env"] == "prod""#, &context);
//! ```

#[cfg(feature = "rhai")]
pub use crate::execution::rhai::RhaiEngine;

#[cfg(not(feature = "rhai"))]
/// Rhai support is not enabled. Enable with `features = ["rhai"]`.
pub type RhaiEngine = crate::error::RgxError;
