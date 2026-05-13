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
    Digit {
        /// Whether the class is negated (\D)
        negated: bool,
    },
    /// \w or \W (word characters)
    Word {
        /// Whether the class is negated (\W)
        negated: bool,
    },
    /// \s or \S (whitespace)
    Space {
        /// Whether the class is negated (\S)
        negated: bool,
    },

    // Unicode classes
    /// \p{...} or \P{...} (Unicode properties)
    UnicodeClass {
        /// Unicode property name
        name: String,
        /// Whether the class is negated (\P)
        negated: bool,
    },
    /// PCRE2/Perl extended character class syntax (?[...])
    ExtendedCharClass {
        /// Raw content of the extended character class
        content: String,
    },

    // Quantified expressions
    /// Expression with quantifier (* + ? {n,m})
    Quantified {
        /// The expression being quantified
        expr: Box<Regex>,
        /// The quantifier applied to the expression
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
        /// The expression inside the group
        expr: Box<Regex>,
        /// The type of group construct
        kind: GroupKind,
        /// Capture group number (1, 2, 3...), if capturing
        index: Option<u32>,
        /// Named capture identifier (?<name>...), if named
        name: Option<String>,
    },

    // Assertions
    /// Lookahead (?=...) or negative lookahead (?!...)
    Lookahead {
        /// The assertion expression
        expr: Box<Regex>,
        /// Whether this is a positive lookahead
        positive: bool,
        /// PCRE2 `(*napla:...)` / `(*nanla:...)` non-atomic lookahead.
        /// Default `false` (atomic). When `true`, codegen emits the
        /// body inline so its alternation backtrack frames live on
        /// the outer `ctx.backtrack_stack` — outer can backtrack INTO
        /// the assertion body.
        non_atomic: bool,
    },
    /// Lookbehind (?<=...) or negative lookbehind (?<!...)
    Lookbehind {
        /// The assertion expression
        expr: Box<Regex>,
        /// Whether this is a positive lookbehind
        positive: bool,
        /// PCRE2 `(*naplb:...)` / `(*nanlb:...)` non-atomic lookbehind.
        /// Default `false`. Mirrors `Lookahead.non_atomic`.
        non_atomic: bool,
    },
    /// Anchors like ^, $, \A, \Z, \z
    Anchor(AnchorType),
    /// Word boundaries \b and \B
    WordBoundary {
        /// Whether this is a positive word boundary (\b vs \B)
        positive: bool,
    },

    // Advanced features
    /// Backreference to capture group \1, \2, etc.
    Backreference(u32),
    /// Named backreference \k<name> or \k'name'
    NamedBackreference(String),
    /// Relative backreference \g<+1> or \g<-1> (resolved at compile time)
    RelativeBackreference(i32),
    /// Conditional patterns (?(condition)yes|no)
    Conditional {
        /// The condition to evaluate
        condition: ConditionalTest,
        /// Pattern to match when the condition is true
        true_branch: Box<Regex>,
        /// Optional pattern to match when the condition is false
        false_branch: Option<Box<Regex>>,
    },
    /// Recursive patterns (?R), (?1), (?&name)
    Recursion {
        /// The recursion target (entire pattern, group, or named group)
        target: RecursionTarget,
    },
    /// PCRE2 10.47+ returned-capture subroutine: (?1(1,2)) or (?&name(1,name2))
    ///
    /// Like a subroutine call, but captures from the specified groups are
    /// returned to the caller instead of being discarded.
    ReturnedCaptureSubroutine {
        /// The subroutine target (group number or name)
        target: RecursionTarget,
        /// Groups whose captures should be returned to the caller. Each
        /// entry resolves to a numeric group id at codegen via
        /// `recursion_target_to_id`; relative entries are normalised
        /// to absolute by `resolve_relative_conditionals` before then.
        returned_groups: Vec<RecursionTarget>,
    },

    // Code execution (rgx's unique feature!)
    /// Code block (?{lua:...}) or (?{js:...})
    CodeBlock {
        /// The scripting language identifier
        lang: String,
        /// The code to execute
        code: String,
    },
    /// PCRE2 callout (?C) or (?C123)
    ///
    /// Invokes a host-registered callout by number during matching.
    /// Compiled as `(?{native:__callout_N})` internally.
    Callout(u32),

    // Inline flag groups
    /// Scoped flag modifier group (?m:...), (?i:...), (?s:...), etc.
    FlagGroup {
        /// Active flag characters (e.g., "m", "mi", "s")
        flags: String,
        /// Inner expression affected by the flags
        expr: Box<Regex>,
    },

    // PCRE2 special assertions
    /// \K — Match reset: resets the reported match start to the current position
    MatchReset,
    /// \R — Newline sequence: matches \r\n, \r, \n, \x0B, \x0C, \x85, \u{2028}, \u{2029}
    NewlineSequence,
    /// \X — Extended grapheme cluster: matches one full grapheme (base + combining marks)
    GraphemeCluster,

    // Match control
    /// (*ACCEPT) - Force immediate match acceptance
    Accept,
    /// (*COMMIT) - If the match fails after this point, abort the entire search
    Commit,
    /// (*PRUNE) - If the match fails after this point, fail the current attempt immediately
    Prune,
    /// (*SKIP) - If the match fails after this point, restart search at
    /// the skip position. The optional name is for the `(*SKIP:name)`
    /// form which interacts with `(*MARK:name)` to restart search at
    /// the position of the most recent matching mark instead of the
    /// position of `(*SKIP)` itself. `None` is the unnamed `(*SKIP)`
    /// which records `ctx.pos` directly.
    Skip(Option<String>),
    /// (*THEN) - If the current alternative fails after this point, skip to the next alternative
    Then,
    /// (*MARK:name) / (*:name) - Set a named mark (no-op for match behavior)
    Mark(String),

    // Special
    /// Empty pattern (epsilon)
    Empty,
    /// Unescaped whitespace from a `whitespace_literal` PGEN node.
    ///
    /// Inside `(?x:...)` extended-mode groups the compiler strips these.
    /// Outside extended mode the compiler lowers them to `Char(c)`.
    /// Escaped whitespace (`\ `, `\t`, etc.) never uses this variant; it
    /// arrives as a normal `Char` via the `escape` rule.
    WhitespaceLiteral(char),
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
    /// Branch-reset group (?|...) - alternatives share capture group numbering
    BranchReset,
}

/// Quantifier specification
#[derive(Debug, Clone, PartialEq)]
pub enum Quantifier {
    /// ? quantifier (0 or 1)
    ZeroOrOne {
        /// Whether the quantifier is lazy (non-greedy)
        lazy: bool,
    },
    /// * quantifier (0 or more)
    ZeroOrMore {
        /// Whether the quantifier is lazy (non-greedy)
        lazy: bool,
    },
    /// + quantifier (1 or more)
    OneOrMore {
        /// Whether the quantifier is lazy (non-greedy)
        lazy: bool,
    },
    /// {n,m} quantifier (n to m repetitions)
    Range {
        /// Minimum number of repetitions
        min: u32,
        /// Maximum number of repetitions (None means unbounded)
        max: Option<u32>,
        /// Whether the quantifier is lazy (non-greedy)
        lazy: bool,
    },
}

/// Character class definition [a-z], [^0-9], etc.
#[derive(Debug, Clone, PartialEq)]
pub enum CharClass {
    /// \d or \D (digits)
    Digit {
        /// Whether the class is negated (\D)
        negated: bool,
    },
    /// \w or \W (word characters)
    Word {
        /// Whether the class is negated (\W)
        negated: bool,
    },
    /// \s or \S (whitespace)
    Space {
        /// Whether the class is negated (\S)
        negated: bool,
    },
    /// \p{...} or \P{...} (Unicode properties)
    UnicodeClass {
        /// Unicode property name
        name: String,
        /// Whether the class is negated (\P)
        negated: bool,
    },
    /// Custom character class like [abc] or [a-z]
    Custom {
        /// Character ranges included in the class
        ranges: Vec<CharRange>,
        /// Whether the class is negated ([^...])
        negated: bool,
        /// Optional override ranges used when the surrounding
        /// pattern is compiled with case-insensitive mode. Populated
        /// by the parser when a class item is `\P{Lu/Ll/Lt}`: under
        /// /i, those properties case-close through PCRE2's `L&`
        /// (cased-letter class), so the complement expands to
        /// `\P{L&}` (non-cased-letters). The codegen case-fold
        /// expansion then unions the folds of literal members
        /// correctly. `None` means the class needs no /i-specific
        /// override (codegen falls back to `ranges`).
        ci_override_ranges: Option<Vec<CharRange>>,
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
    #[must_use]
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
    #[must_use]
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
    /// \G - End of previous match (or start of string if no previous match)
    PreviousMatchEnd,
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
    Lookahead {
        /// The lookahead assertion expression
        expr: Box<Regex>,
        /// Whether this is a positive lookahead test
        positive: bool,
    },
    /// Lookbehind test (?(?<=...)...) or (?(?<!...)...)
    Lookbehind {
        /// The lookbehind assertion expression
        expr: Box<Regex>,
        /// Whether this is a positive lookbehind test
        positive: bool,
    },
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
    /// (?+1), (?-1) - Recurse relative group (resolved at compile time)
    RelativeGroup(i32),
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
    #[must_use]
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
    pub fn register_named_group(&mut self, name: impl Into<String>) -> u32 {
        let name = name.into();
        trace_enter!(
            "ast",
            "ParseContext::register_named_group",
            "name={},group_counter_before={},named_groups_len_before={}",
            name,
            self.group_counter,
            self.named_groups.len()
        );
        let number = self.next_group_number();
        let _replaced_existing = self.named_groups.insert(name.clone(), number).is_some();
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
    #[must_use]
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

        let name_num = ctx.register_named_group("test");
        assert_eq!(name_num, 3);
        assert_eq!(ctx.get_named_group("test"), Some(3));
        assert_eq!(ctx.get_named_group("missing"), None);
    }
}
