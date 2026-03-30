//! Token definitions for regex lexical analysis
//!
//! This module defines all the tokens that can appear in a regex pattern,
//! from simple characters to complex constructs like code blocks.

use crate::ast::{AnchorType, CharRange, ConditionalTest, RecursionTarget};
use crate::{trace_enter, trace_exit};

/// Tokens produced by the lexer
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Literals
    /// A literal character to match
    Char(char),

    // Character classes and shortcuts
    /// Character class [a-z], [^0-9], etc.
    CharClass {
        ranges: Vec<CharRange>,
        negated: bool,
    },
    /// Dot metacharacter .
    Dot,

    // Predefined character classes
    /// \d (digits)
    Digit,
    /// \D (non-digits)  
    DigitNeg,
    /// \w (word characters)
    Word,
    /// \W (non-word characters)
    WordNeg,
    /// \s (whitespace)
    Space,
    /// \S (non-whitespace)
    SpaceNeg,
    /// \b (word boundary)
    WordBoundary,
    /// \B (non-word boundary)
    WordBoundaryNeg,

    // Unicode classes
    /// \p{Name} (Unicode property class)
    UnicodeClass {
        name: String,
    },
    /// \P{Name} (negated Unicode property class)
    UnicodeClassNeg {
        name: String,
    },

    // Quantifiers
    /// * (zero or more, greedy)
    Star,
    /// + (one or more, greedy)
    Plus,
    /// ? (zero or one, greedy)
    Question,
    /// *+ (zero or more, possessive)
    StarPossessive,
    /// ++ (one or more, possessive)
    PlusPossessive,
    /// ?+ (zero or one, possessive)
    QuestionPossessive,
    /// *? (zero or more, lazy)
    StarLazy,
    /// +? (one or more, lazy)
    PlusLazy,
    /// ?? (zero or one, lazy)
    QuestionLazy,
    /// {n}, {n,}, {n,m}, {n,m}? (counted repetition)
    Repeat {
        min: u32,
        max: Option<u32>,
        lazy: bool,
        possessive: bool,
    },

    // Groups
    /// ( - Start of capturing group
    GroupStart,
    /// (?<name> - Start of named capturing group
    NamedGroupStart {
        name: String,
    },
    /// (?:  - Start of non-capturing group
    NonCapturingGroupStart,
    /// (?>  - Start of atomic group (no backtracking)
    AtomicGroupStart,
    /// (?| - Start of a branch-reset group
    BranchResetGroupStart,
    /// ) - End of any group
    GroupEnd,

    // Lookaround assertions
    /// (?= - Positive lookahead
    LookaheadPos,
    /// (?! - Negative lookahead
    LookaheadNeg,
    /// (?<= - Positive lookbehind
    LookbehindPos,
    /// (?<! - Negative lookbehind
    LookbehindNeg,

    // Code execution blocks (rgx's unique feature!)
    /// (?{lang:code}) - Code execution block
    CodeBlock {
        lang: String,
        code: String,
    },

    // Conditionals and recursion
    /// (?(...) - Conditional pattern start
    ConditionalStart {
        condition: ConditionalTest,
    },
    /// (?R), (?1), (?&name) - Recursion
    Recursion {
        target: RecursionTarget,
    },

    // Other constructs
    /// | - Alternation
    Alternation,
    /// ^, $, \A, \Z, \z - Anchors
    Anchor(AnchorType),
    /// \1, \2, etc. - Backreferences
    Backreference(u32),

    // Flags and modifiers
    /// (?flags: - Inline flag modifiers (?i:...), (?m:...), etc.
    FlagModifier {
        flags: String,
    },

    // End of input
    EOF,
}

/// Position information for error reporting
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Position {
    /// Byte offset in the input string
    pub offset: usize,
    /// Line number (1-based)
    pub line: usize,
    /// Column number (1-based)
    pub column: usize,
}

impl Position {
    /// Create a new position
    pub fn new(offset: usize, line: usize, column: usize) -> Self {
        trace_enter!(
            "token",
            "Position::new",
            "offset={},line={},column={}",
            offset,
            line,
            column
        );
        let position = Self {
            offset,
            line,
            column,
        };
        trace_exit!(
            "token",
            "Position::new",
            "ok=true,offset={},line={},column={}",
            position.offset,
            position.line,
            position.column
        );
        position
    }

    /// Create position at start of input
    pub fn start() -> Self {
        trace_enter!("token", "Position::start");
        let position = Self::new(0, 1, 1);
        trace_exit!(
            "token",
            "Position::start",
            "ok=true,offset={},line={},column={}",
            position.offset,
            position.line,
            position.column
        );
        position
    }
}

/// Token with position information
#[derive(Debug, Clone, PartialEq)]
pub struct TokenWithPos {
    /// The token
    pub token: Token,
    /// Position in the input where this token starts
    pub position: Position,
}

impl TokenWithPos {
    /// Create a new token with position
    pub fn new(token: Token, position: Position) -> Self {
        trace_enter!(
            "token",
            "TokenWithPos::new",
            "token={:?},offset={},line={},column={}",
            token,
            position.offset,
            position.line,
            position.column
        );
        let token_with_pos = Self { token, position };
        trace_exit!(
            "token",
            "TokenWithPos::new",
            "ok=true,token={:?},offset={},line={},column={}",
            token_with_pos.token,
            token_with_pos.position.offset,
            token_with_pos.position.line,
            token_with_pos.position.column
        );
        token_with_pos
    }
}

/// Errors that can occur during lexical analysis
#[derive(Debug, Clone, PartialEq)]
pub enum LexError {
    /// Unexpected character in input
    UnexpectedChar { char: char, position: Position },
    /// Invalid escape sequence
    InvalidEscape {
        sequence: String,
        position: Position,
    },
    /// Unterminated character class [abc...
    UnterminatedCharClass { position: Position },
    /// Invalid character class range [z-a]
    InvalidCharRange {
        start: char,
        end: char,
        position: Position,
    },
    /// Unterminated group (...
    UnterminatedGroup { position: Position },
    /// Invalid group syntax (?xyz...)
    InvalidGroupSyntax { position: Position },
    /// Expected colon after language in code block (?{lang...
    ExpectedColon { position: Position },
    /// Unterminated code block (?{lang:code...
    UnterminatedCodeBlock { position: Position },
    /// Invalid repeat quantifier {x,y}
    InvalidRepeat { text: String, position: Position },
    /// Invalid Unicode class name \p{...}
    InvalidUnicodeClass { name: String, position: Position },
    /// Invalid backreference number \99999...
    InvalidBackreference { number: String, position: Position },
    /// Unexpected end of input
    UnexpectedEOF {
        expected: String,
        position: Position,
    },
}

impl std::fmt::Display for LexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LexError::UnexpectedChar { char, position } => {
                write!(
                    f,
                    "Unexpected character '{}' at line {}, column {}",
                    char, position.line, position.column
                )
            }
            LexError::InvalidEscape { sequence, position } => {
                write!(
                    f,
                    "Invalid escape sequence '{}' at line {}, column {}",
                    sequence, position.line, position.column
                )
            }
            LexError::UnterminatedCharClass { position } => {
                write!(
                    f,
                    "Unterminated character class at line {}, column {}",
                    position.line, position.column
                )
            }
            LexError::InvalidCharRange {
                start,
                end,
                position,
            } => {
                write!(
                    f,
                    "Invalid character range '{}-{}' at line {}, column {}",
                    start, end, position.line, position.column
                )
            }
            LexError::UnterminatedGroup { position } => {
                write!(
                    f,
                    "Unterminated group at line {}, column {}",
                    position.line, position.column
                )
            }
            LexError::InvalidGroupSyntax { position } => {
                write!(
                    f,
                    "Invalid group syntax at line {}, column {}",
                    position.line, position.column
                )
            }
            LexError::ExpectedColon { position } => {
                write!(
                    f,
                    "Expected ':' after language in code block at line {}, column {}",
                    position.line, position.column
                )
            }
            LexError::UnterminatedCodeBlock { position } => {
                write!(
                    f,
                    "Unterminated code block at line {}, column {}",
                    position.line, position.column
                )
            }
            LexError::InvalidRepeat { text, position } => {
                write!(
                    f,
                    "Invalid repeat quantifier '{}' at line {}, column {}",
                    text, position.line, position.column
                )
            }
            LexError::InvalidUnicodeClass { name, position } => {
                write!(
                    f,
                    "Invalid Unicode class '{}' at line {}, column {}",
                    name, position.line, position.column
                )
            }
            LexError::InvalidBackreference { number, position } => {
                write!(
                    f,
                    "Invalid backreference '\\{}' at line {}, column {}",
                    number, position.line, position.column
                )
            }
            LexError::UnexpectedEOF { expected, position } => {
                write!(
                    f,
                    "Unexpected end of input, expected {} at line {}, column {}",
                    expected, position.line, position.column
                )
            }
        }
    }
}

impl std::error::Error for LexError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_position() {
        let pos = Position::new(10, 2, 5);
        assert_eq!(pos.offset, 10);
        assert_eq!(pos.line, 2);
        assert_eq!(pos.column, 5);

        let start = Position::start();
        assert_eq!(start.offset, 0);
        assert_eq!(start.line, 1);
        assert_eq!(start.column, 1);
    }

    #[test]
    fn test_token_with_pos() {
        let token = Token::Char('a');
        let pos = Position::new(5, 1, 6);
        let token_pos = TokenWithPos::new(token.clone(), pos.clone());

        assert_eq!(token_pos.token, token);
        assert_eq!(token_pos.position, pos);
    }
}
