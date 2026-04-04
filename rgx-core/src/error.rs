use thiserror::Error;

/// Convenience result type defaulting to [`RgxError`].
pub type Result<T> = std::result::Result<T, RgxError>;

/// Top-level error type for the rgx crate.
#[derive(Debug, Error)]
pub enum RgxError {
    /// A pattern failed to compile.
    #[error("pattern compile error: {0}")]
    Compile(String),

    /// A runtime engine error occurred during matching.
    #[error("engine error: {0}")]
    Engine(String),
}
