use crate::pattern::CompiledPattern;
use crate::error::Result;
use crate::vm::RegexVM;

/// Execution mode that controls performance vs feature tradeoffs
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ExecutionMode { 
    /// Maximum performance, pure regex matching only
    Pure, 
    /// Code execution in sandboxed environments only
    Safe, 
    /// All features including native callbacks
    Full 
}

/// Match result with position information
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchResult { 
    /// Start position in bytes
    pub start: usize, 
    /// End position in bytes
    pub end: usize,
    /// 1-based top-level branch number for top-level alternation matches.
    ///
    /// `None` when the pattern has no top-level alternation.
    pub matched_branch_number: Option<usize>,
}

/// High-performance regex execution engine
pub struct Engine {
    /// The compiled VM for pattern execution
    vm: RegexVM,
    /// Execution mode for this engine
    mode: ExecutionMode,
}

impl Engine {
    /// Create new engine from compiled pattern
    pub fn new(pattern: &CompiledPattern) -> Result<Self> {
        let vm = RegexVM::new(pattern.program.clone());
        
        Ok(Self { 
            vm,
            mode: pattern.mode,
        })
    }

    /// Find all non-overlapping matches in the input
    pub fn find_all(&self, text: &[u8]) -> Vec<MatchResult> {
        // Convert bytes to string for VM processing
        let text_str = match std::str::from_utf8(text) {
            Ok(s) => s,
            Err(_) => return Vec::new(), // Invalid UTF-8
        };
        
        self.vm.find_all(text_str)
            .into_iter()
            .map(|m| MatchResult { 
                start: m.start, 
                end: m.end,
                matched_branch_number: m.matched_alternative.map(|id| id + 1),
            })
            .collect()
    }

    /// Find the first match in the input
    pub fn find_first(&self, text: &[u8]) -> Option<MatchResult> {
        // Convert bytes to string for VM processing
        let text_str = std::str::from_utf8(text).ok()?;
        
        self.vm.find_first(text_str)
            .map(|m| MatchResult { 
                start: m.start, 
                end: m.end,
                matched_branch_number: m.matched_alternative.map(|id| id + 1),
            })
    }

    /// Test if pattern matches the input (fastest operation)
    pub fn is_match(&self, text: &[u8]) -> bool {
        // Convert bytes to string for VM processing
        if let Ok(text_str) = std::str::from_utf8(text) {
            self.vm.is_match(text_str)
        } else {
            false // Invalid UTF-8 cannot match
        }
    }
}

