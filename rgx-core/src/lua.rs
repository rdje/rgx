//! Lua code execution backend for rgx patterns.
//!
//! This module provides sandboxed Lua execution for `(?{lua:...})` patterns.
//! The Lua environment is heavily restricted for security:
//! - No file I/O access
//! - No system calls
//! - No module loading
//! - Memory limits enforced
//!
//! # Usage
//!
//! ```rust,ignore
//! use rgx_core::lua::LuaEngine;
//!
//! let engine = LuaEngine::new()?;
//! let result = engine.execute("return arg[1] == 'hello'", &context);
//! ```

#[cfg(feature = "lua")]
pub use crate::execution::lua::LuaEngine;

#[cfg(not(feature = "lua"))]
/// Lua support is not enabled. Enable with `features = ["lua"]`.
pub type LuaEngine = crate::error::RgxError;
