//! Fluent builder for host variables.
//!
//! Build complex variable structures without ever touching [`Value`] directly.
//!
//! # Examples
//!
//! ```rust,no_run
//! # use rgx_core::{Regex, ExecutionMode};
//! let re = Regex::with_mode(r".", ExecutionMode::Full).unwrap();
//!
//! // Scalars
//! re.vars()
//!     .set("threshold", 100_i64)
//!     .set("rate", 0.08)
//!     .set("debug", true)
//!     .set("name", "alice");
//!
//! // Arrays
//! re.vars()
//!     .list("allowed")
//!         .push("cat")
//!         .push("dog")
//!         .push("bird")
//!         .done();
//!
//! // Nested hashes
//! re.vars()
//!     .hash("server")
//!         .set("host", "localhost")
//!         .set("port", 8080_i64)
//!         .hash("tls")
//!             .set("enabled", true)
//!             .set("cert", "/path/to/cert.pem")
//!             .done()
//!         .list("allowed_origins")
//!             .push("https://example.com")
//!             .push("https://api.example.com")
//!             .done()
//!         .done();
//! ```

use crate::execution::Value;
use crate::Regex;

// ---------------------------------------------------------------------------
// Trait: allow builders to accept a child's completed value
// ---------------------------------------------------------------------------

/// Trait that lets child builders commit their result to any parent.
///
/// You do not need to implement this trait yourself — it is used internally by
/// [`VarsBuilder`], [`HashBuilder`], and [`ArrayBuilder`] to wire up the
/// `done()` return path.
pub trait CommitValue {
    /// Accept a named value from a completed child builder.
    #[must_use]
    fn commit(self, name: String, value: Value) -> Self;
}

// ===========================================================================
// VarsBuilder — top-level entry point
// ===========================================================================

/// Fluent builder for setting host variables on a compiled regex.
///
/// Obtained via [`Regex::vars`]. Every method consumes and returns `self` so
/// calls can be chained in a single expression. Variables are committed to the
/// regex eagerly — each `set()` call (and each `done()` on a child builder)
/// writes the value immediately.
pub struct VarsBuilder<'a> {
    regex: &'a Regex,
}

impl<'a> VarsBuilder<'a> {
    pub(crate) fn new(regex: &'a Regex) -> Self {
        Self { regex }
    }

    /// Set a scalar variable (string, integer, float, or boolean).
    #[must_use]
    pub fn set<V: Into<Value>>(self, name: &str, value: V) -> Self {
        let _ = self.regex.set_typed_variable(name, value.into());
        self
    }

    /// Start building an array variable. Call `.push()` to add elements and
    /// `.done()` to commit the array and return to the `VarsBuilder`.
    #[must_use]
    pub fn list(self, name: &str) -> ArrayBuilder<Self> {
        ArrayBuilder {
            parent: self,
            name: name.to_string(),
            items: Vec::new(),
        }
    }

    /// Start building a hash / map variable. Call `.set()` to add entries,
    /// `.hash()` / `.list()` to nest further, and `.done()` to commit the map
    /// and return to the `VarsBuilder`.
    #[must_use]
    pub fn hash(self, name: &str) -> HashBuilder<Self> {
        HashBuilder {
            parent: self,
            name: name.to_string(),
            entries: Vec::new(),
        }
    }
}

impl CommitValue for VarsBuilder<'_> {
    fn commit(self, name: String, value: Value) -> Self {
        let _ = self.regex.set_typed_variable(name, value);
        self
    }
}

// ===========================================================================
// ArrayBuilder
// ===========================================================================

/// Collects array elements. Created by [`VarsBuilder::list`] or
/// [`HashBuilder::list`].
///
/// The parent type `P` carries any lifetime constraints (e.g. the `'a` from
/// `VarsBuilder<'a>`) so this struct does not need its own lifetime parameter.
pub struct ArrayBuilder<P: CommitValue> {
    parent: P,
    name: String,
    items: Vec<Value>,
}

impl<P: CommitValue> ArrayBuilder<P> {
    /// Append a value to the array.
    #[must_use]
    pub fn push<V: Into<Value>>(mut self, value: V) -> Self {
        self.items.push(value.into());
        self
    }

    /// Finish the array and return to the parent builder.
    ///
    /// The collected values are committed as `Value::Array`.
    #[must_use]
    pub fn done(self) -> P {
        self.parent.commit(self.name, Value::Array(self.items))
    }
}

// ===========================================================================
// HashBuilder
// ===========================================================================

/// Collects key-value entries for a map. Created by [`VarsBuilder::hash`] or
/// by nesting inside another `HashBuilder`.
///
/// The parent type `P` carries any lifetime constraints so this struct does not
/// need its own lifetime parameter.
pub struct HashBuilder<P: CommitValue> {
    parent: P,
    name: String,
    entries: Vec<(String, Value)>,
}

impl<P: CommitValue> HashBuilder<P> {
    /// Set a scalar entry in the hash.
    #[must_use]
    pub fn set<V: Into<Value>>(mut self, key: &str, value: V) -> Self {
        self.entries.push((key.to_string(), value.into()));
        self
    }

    /// Start building a nested hash inside this hash.
    #[must_use]
    pub fn hash(self, key: &str) -> HashBuilder<Self> {
        HashBuilder {
            parent: self,
            name: key.to_string(),
            entries: Vec::new(),
        }
    }

    /// Start building a nested array inside this hash.
    #[must_use]
    pub fn list(self, key: &str) -> ArrayBuilder<Self> {
        ArrayBuilder {
            parent: self,
            name: key.to_string(),
            items: Vec::new(),
        }
    }

    /// Finish the hash and return to the parent builder.
    ///
    /// The collected entries are committed as `Value::Map`.
    #[must_use]
    pub fn done(self) -> P {
        self.parent.commit(self.name, Value::Map(self.entries))
    }
}

impl<P: CommitValue> CommitValue for HashBuilder<P> {
    fn commit(mut self, name: String, value: Value) -> Self {
        self.entries.push((name, value));
        self
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use crate::execution::ExecResult;
    use crate::{ExecutionMode, Regex};

    #[test]
    fn fluent_vars_simple() {
        let re = Regex::with_mode(r"(?{native:check})", ExecutionMode::Full).unwrap();
        re.vars()
            .set("threshold", 100_i64)
            .set("name", "alice")
            .set("debug", true);
        re.register_native("check", |ctx| {
            assert_eq!(ctx.var_int("threshold"), Some(100));
            assert_eq!(ctx.var_str("name").as_deref(), Some("alice"));
            assert_eq!(ctx.var_bool("debug"), Some(true));
            ExecResult::Success
        })
        .unwrap();
        assert!(re.is_match("x"));
    }

    #[test]
    fn fluent_vars_list() {
        let re = Regex::with_mode(r"(?{native:check})", ExecutionMode::Full).unwrap();
        re.vars()
            .list("tags")
            .push("alpha")
            .push("beta")
            .push("gamma")
            .done();
        re.register_native("check", |ctx| {
            let tags = ctx.var_array("tags").unwrap();
            assert_eq!(tags.len(), 3);
            ExecResult::Success
        })
        .unwrap();
        assert!(re.is_match("x"));
    }

    #[test]
    fn fluent_vars_hash() {
        let re = Regex::with_mode(r"(?{native:check})", ExecutionMode::Full).unwrap();
        re.vars()
            .hash("config")
            .set("port", 8080_i64)
            .set("host", "localhost")
            .done();
        re.register_native("check", |ctx| {
            let config = ctx.typed_variable("config").unwrap();
            let map = config.as_map().unwrap();
            assert_eq!(map.len(), 2);
            ExecResult::Success
        })
        .unwrap();
        assert!(re.is_match("x"));
    }

    #[test]
    fn fluent_vars_deep_nesting() {
        let re = Regex::with_mode(r"(?{native:check})", ExecutionMode::Full).unwrap();
        re.vars()
            .set("env", "prod")
            .hash("server")
            .set("host", "localhost")
            .set("port", 8080_i64)
            .hash("tls")
            .set("enabled", true)
            .done()
            .list("origins")
            .push("https://example.com")
            .push("https://api.example.com")
            .done()
            .done();
        re.register_native("check", |ctx| {
            assert_eq!(ctx.var_str("env").as_deref(), Some("prod"));
            let server = ctx.typed_variable("server").unwrap();
            assert!(server.as_map().is_some());
            ExecResult::Success
        })
        .unwrap();
        assert!(re.is_match("x"));
    }

    #[test]
    fn fluent_vars_mixed_chain() {
        let re = Regex::with_mode(r"(?{native:check})", ExecutionMode::Full).unwrap();
        re.vars()
            .set("env", "prod")
            .set("max_retries", 3_i64)
            .hash("db")
            .set("host", "db.example.com")
            .set("port", 5432_i64)
            .list("replicas")
            .push("replica1.example.com")
            .push("replica2.example.com")
            .done()
            .done();
        re.register_native("check", |ctx| {
            assert_eq!(ctx.var_str("env").as_deref(), Some("prod"));
            assert_eq!(ctx.var_int("max_retries"), Some(3));
            let db = ctx.typed_variable("db").unwrap();
            let map = db.as_map().unwrap();
            assert_eq!(map.len(), 3); // host, port, replicas
            ExecResult::Success
        })
        .unwrap();
        assert!(re.is_match("x"));
    }
}
