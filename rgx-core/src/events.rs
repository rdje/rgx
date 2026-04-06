//! Structured match events for debugging, profiling, and observability.

/// Events emitted during regex matching.
///
/// Observers registered via `Regex::on_event` receive these events
/// at key execution points. Events are fire-and-forget — they do not
/// affect match behavior.
#[derive(Debug, Clone, PartialEq)]
pub enum MatchEvent {
    /// A match attempt is starting at the given input position.
    MatchAttemptStarted {
        /// Byte position in the input where the attempt begins.
        position: usize,
    },
    /// A match attempt completed (succeeded or failed).
    MatchAttemptCompleted {
        /// Byte position in the input where the attempt was made.
        position: usize,
        /// Whether the attempt produced a match.
        matched: bool,
    },
    /// A top-level alternation branch was entered.
    BranchEntered {
        /// Zero-based alternation branch index.
        branch: u32,
        /// Byte position at which the branch was entered.
        position: usize,
    },
    /// A capture group completed (`SaveEnd` executed).
    CaptureCompleted {
        /// Group number (0 = overall match).
        group: u32,
        /// Byte offset where the capture starts.
        start: usize,
        /// Byte offset where the capture ends.
        end: usize,
    },
    /// A backtrack occurred.
    BacktrackOccurred {
        /// Byte position after the backtrack was applied.
        position: usize,
        /// Backtrack stack depth before the frame was popped.
        stack_depth: usize,
    },
    /// A code block was evaluated.
    CodeBlockEvaluated {
        /// Language tag of the code block (e.g. "lua", "native").
        language: String,
        /// Whether the code block succeeded.
        succeeded: bool,
        /// Byte position at the time of evaluation.
        position: usize,
    },
}
