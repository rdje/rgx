//! Abstract Syntax Tree for regex patterns
//!
//! This module defines the complete AST for representing any regex pattern,
//! including all Perl regex features and our custom code execution blocks.

use crate::{trace_decision, trace_enter, trace_exit};
use std::collections::HashMap;

/// Main regex AST node representing any regex pattern
#[derive(Debug, Clone, PartialEq)]
pub enum Regex {
    // Basic patterns
    /// Match a literal character
    Char(char),
    /// Character class like [a-z] or [^0-9]  
    CharClass(CharClass),
    /// Dot metacharacter (matches any character except newline by default)
    Dot,

    // Predefined character classes
    /// \d or \D (digits)
    Digit { negated: bool },
    /// \w or \W (word characters)
    Word { negated: bool },
    /// \s or \S (whitespace)
    Space { negated: bool },

    // Unicode classes
    /// \p{...} or \P{...} (Unicode properties)
    UnicodeClass { name: String, negated: bool },
    /// PCRE2/Perl extended character class syntax (?[...])
    ExtendedCharClass { content: String },

    // Quantified expressions
    /// Expression with quantifier (* + ? {n,m})
    Quantified {
        expr: Box<Regex>,
        quantifier: Quantifier,
    },

    // Sequences and alternation
    /// Sequence of patterns (concatenation)
    Sequence(Vec<Regex>),
    /// Alternation with | operator
    Alternation(Vec<Regex>),

    // Groups
    /// Grouping with (...), (?:...), (?<name>...)
    Group {
        expr: Box<Regex>,
        kind: GroupKind,
        index: Option<u32>,   // Capture group number (1, 2, 3...)
        name: Option<String>, // Named capture (?<name>...)
    },

    // Assertions
    /// Lookahead (?=...) or negative lookahead (?!...)
    Lookahead { expr: Box<Regex>, positive: bool },
    /// Lookbehind (?<=...) or negative lookbehind (?<!...)
    Lookbehind { expr: Box<Regex>, positive: bool },
    /// Anchors like ^, $, \A, \Z, \z
    Anchor(AnchorType),
    /// Word boundaries \b and \B
    WordBoundary { positive: bool },

    // Advanced features
    /// Backreference to capture group \1, \2, etc.
    Backreference(u32),
    /// Conditional patterns (?(condition)yes|no)
    Conditional {
        condition: ConditionalTest,
        true_branch: Box<Regex>,
        false_branch: Option<Box<Regex>>,
    },
    /// Recursive patterns (?R), (?1), (?&name)
    Recursion { target: RecursionTarget },

    // Code execution (rgx's unique feature!)
    /// Code block (?{lua:...}) or (?{js:...})
    CodeBlock { lang: String, code: String },

    // Special
    /// Empty pattern (epsilon)
    Empty,
}

/// Type of grouping construct
#[derive(Debug, Clone, PartialEq)]
pub enum GroupKind {
    /// Regular capturing group (...)
    Capturing,
    /// Non-capturing group (?:...)
    NonCapturing,
    /// Atomic group (?>...) - no backtracking
    Atomic,
    /// Branch-reset group (?|...) - parser-recognized, runtime semantics pending
    BranchReset,
}

/// Quantifier specification
#[derive(Debug, Clone, PartialEq)]
pub enum Quantifier {
    /// ? quantifier (0 or 1)
    ZeroOrOne { lazy: bool },
    /// * quantifier (0 or more)
    ZeroOrMore { lazy: bool },
    /// + quantifier (1 or more)
    OneOrMore { lazy: bool },
    /// {n,m} quantifier (n to m repetitions)
    Range {
        min: u32,
        max: Option<u32>,
        lazy: bool,
    },
}

/// Character class definition [a-z], [^0-9], etc.
#[derive(Debug, Clone, PartialEq)]
pub enum CharClass {
    /// \d or \D (digits)
    Digit { negated: bool },
    /// \w or \W (word characters)
    Word { negated: bool },
    /// \s or \S (whitespace)
    Space { negated: bool },
    /// \p{...} or \P{...} (Unicode properties)
    UnicodeClass { name: String, negated: bool },
    /// Custom character class like [abc] or [a-z]
    Custom {
        ranges: Vec<CharRange>,
        negated: bool,
    },
}

/// Range of characters in a character class
#[derive(Debug, Clone, PartialEq)]
pub struct CharRange {
    /// Start character (inclusive)
    pub start: char,
    /// End character (inclusive)  
    pub end: char,
}

impl CharRange {
    /// Create a single character range
    pub fn single(ch: char) -> Self {
        trace_enter!("ast", "CharRange::single", "ch='{}'({})", ch, ch as u32);
        let range = Self { start: ch, end: ch };
        trace_exit!(
            "ast",
            "CharRange::single",
            "ok=true,start='{}'({}),end='{}'({})",
            range.start,
            range.start as u32,
            range.end,
            range.end as u32
        );
        range
    }

    /// Create a character range from start to end
    pub fn range(start: char, end: char) -> Self {
        trace_enter!(
            "ast",
            "CharRange::range",
            "start='{}'({}),end='{}'({})",
            start,
            start as u32,
            end,
            end as u32
        );
        trace_decision!(
            "ast",
            "start <= end",
            start <= end,
            "character range ordering check"
        );
        let range = Self { start, end };
        trace_exit!(
            "ast",
            "CharRange::range",
            "ok=true,start='{}'({}),end='{}'({})",
            range.start,
            range.start as u32,
            range.end,
            range.end as u32
        );
        range
    }
}

/// Types of anchors
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AnchorType {
    /// ^ - Start of line/string
    Start,
    /// $ - End of line/string
    End,
    /// \A - Absolute start of string
    AbsStart,
    /// \Z - Absolute end of string (before final newline)
    AbsEnd,
    /// \z - Absolute end of string
    AbsEndNoNL,
}

/// Condition tests for conditional patterns
#[derive(Debug, Clone, PartialEq)]
pub enum ConditionalTest {
    /// Test if capture group exists (?(1)...)
    GroupExists(u32),
    /// Test if a relative capture group exists (?(+1)...) or (?(-1)...)
    RelativeGroupExists(i32),
    /// Test if named group exists (?(<name>)...)  
    NamedGroupExists(String),
    /// Test whether the current match path is executing inside any recursion level (?(R)...)
    RecursionAny,
    /// Test whether the current recursion level is a specific numbered subroutine (?(R1)...)
    RecursionGroup(u32),
    /// Test whether the current recursion level is a specific named subroutine (?(R&name)...)
    RecursionNamed(String),
    /// DEFINE conditional (?(DEFINE)...)
    Define,
    /// Lookahead test (?(?=...)...) or (?(?!...)...)
    Lookahead { expr: Box<Regex>, positive: bool },
    /// Lookbehind test (?(?<=...)...) or (?(?<!...)...)
    Lookbehind { expr: Box<Regex>, positive: bool },
}

/// Recursion targets
#[derive(Debug, Clone, PartialEq)]
pub enum RecursionTarget {
    /// (?R) - Recurse entire pattern
    Entire,
    /// (?1) - Recurse specific group number
    Group(u32),
    /// (?&name) - Recurse named group
    NamedGroup(String),
}

/// Context information during AST construction
#[derive(Debug, Default)]
pub struct ParseContext {
    /// Current group counter for assigning capture group numbers
    pub group_counter: u32,
    /// Map of named groups to their assigned numbers
    pub named_groups: HashMap<String, u32>,
}

impl ParseContext {
    /// Create a new parse context
    pub fn new() -> Self {
        trace_enter!("ast", "ParseContext::new");
        let context = Self::default();
        trace_exit!(
            "ast",
            "ParseContext::new",
            "ok=true,group_counter={},named_groups_len={}",
            context.group_counter,
            context.named_groups.len()
        );
        context
    }

    /// Allocate a new capture group number
    pub fn next_group_number(&mut self) -> u32 {
        trace_enter!(
            "ast",
            "ParseContext::next_group_number",
            "group_counter_before={}",
            self.group_counter
        );
        self.group_counter += 1;
        trace_exit!(
            "ast",
            "ParseContext::next_group_number",
            "ok=true,group_counter_after={}",
            self.group_counter
        );
        self.group_counter
    }

    /// Register a named group
    pub fn register_named_group(&mut self, name: String) -> u32 {
        trace_enter!(
            "ast",
            "ParseContext::register_named_group",
            "name={},group_counter_before={},named_groups_len_before={}",
            name,
            self.group_counter,
            self.named_groups.len()
        );
        let number = self.next_group_number();
        let replaced_existing = self.named_groups.insert(name.clone(), number).is_some();
        trace_decision!(
            "ast",
            "replaced_existing",
            replaced_existing,
            "named group registration replacement check for name={}",
            name
        );
        trace_exit!(
            "ast",
            "ParseContext::register_named_group",
            "ok=true,name={},number={},named_groups_len_after={}",
            name,
            number,
            self.named_groups.len()
        );
        number
    }

    /// Look up a named group number
    pub fn get_named_group(&self, name: &str) -> Option<u32> {
        trace_enter!(
            "ast",
            "ParseContext::get_named_group",
            "name={},named_groups_len={}",
            name,
            self.named_groups.len()
        );
        let group_number = self.named_groups.get(name).copied();
        trace_decision!(
            "ast",
            "group_number.is_some()",
            group_number.is_some(),
            "named group lookup for name={}",
            name
        );
        trace_exit!(
            "ast",
            "ParseContext::get_named_group",
            "ok=true,name={},group_number={:?}",
            name,
            group_number
        );
        group_number
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quantifiers() {
        let zero_or_one = Quantifier::ZeroOrOne { lazy: false };
        let zero_or_more = Quantifier::ZeroOrMore { lazy: true };
        let one_or_more = Quantifier::OneOrMore { lazy: false };
        let range = Quantifier::Range {
            min: 2,
            max: Some(5),
            lazy: true,
        };

        assert_eq!(zero_or_one, Quantifier::ZeroOrOne { lazy: false });
        assert_eq!(zero_or_more, Quantifier::ZeroOrMore { lazy: true });
        assert_eq!(one_or_more, Quantifier::OneOrMore { lazy: false });
        assert_eq!(
            range,
            Quantifier::Range {
                min: 2,
                max: Some(5),
                lazy: true
            }
        );
    }

    #[test]
    fn test_char_range() {
        let single = CharRange::single('a');
        assert_eq!(single.start, 'a');
        assert_eq!(single.end, 'a');

        let range = CharRange::range('a', 'z');
        assert_eq!(range.start, 'a');
        assert_eq!(range.end, 'z');
    }

    #[test]
    fn test_parse_context() {
        let mut ctx = ParseContext::new();

        assert_eq!(ctx.next_group_number(), 1);
        assert_eq!(ctx.next_group_number(), 2);

        let name_num = ctx.register_named_group("test".to_string());
        assert_eq!(name_num, 3);
        assert_eq!(ctx.get_named_group("test"), Some(3));
        assert_eq!(ctx.get_named_group("missing"), None);
    }
}
