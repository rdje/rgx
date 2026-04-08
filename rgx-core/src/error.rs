use thiserror::Error;

/// Convenience result type defaulting to [`RgxError`].
pub type Result<T> = std::result::Result<T, RgxError>;

/// Top-level error type for the rgx crate.
#[derive(Debug, Error)]
pub enum RgxError {
    /// A pattern failed to compile.
    #[error("{}", .0.format())]
    Compile(CompileError),

    /// A runtime engine error occurred during matching.
    #[error("engine error: {0}")]
    Engine(String),
}

impl RgxError {
    /// Create a compile error from a plain message (no span).
    pub(crate) fn compile(message: impl Into<String>) -> Self {
        Self::Compile(CompileError {
            message: message.into(),
            pattern: None,
            offset: None,
        })
    }

    /// Create a compile error with span information for caret highlighting.
    pub(crate) fn compile_at(message: impl Into<String>, pattern: &str, offset: usize) -> Self {
        Self::Compile(CompileError {
            message: message.into(),
            pattern: Some(pattern.to_string()),
            offset: Some(offset),
        })
    }
}

/// Detailed compilation error with optional source location.
#[derive(Debug, Clone)]
pub struct CompileError {
    /// Human-readable error description.
    pub message: String,
    /// The original pattern (when available).
    pub pattern: Option<String>,
    /// Byte offset into the pattern where the error was detected.
    pub offset: Option<usize>,
}

impl CompileError {
    /// Format with span highlighting when location is available.
    ///
    /// Produces output like:
    /// ```text
    /// regex compile error: unclosed group
    ///   (abc[def
    ///       ^
    /// ```
    pub fn format(&self) -> String {
        let mut out = format!("regex compile error: {}", self.message);
        if let (Some(pattern), Some(offset)) = (&self.pattern, self.offset) {
            out.push_str("\n  ");
            out.push_str(pattern);
            out.push_str("\n  ");
            let caret_pos = offset.min(pattern.len());
            for (i, ch) in pattern.char_indices() {
                if i >= caret_pos {
                    break;
                }
                if ch == '\t' {
                    out.push('\t');
                } else {
                    out.push(' ');
                }
            }
            out.push('^');
        }
        out
    }
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.format())
    }
}
