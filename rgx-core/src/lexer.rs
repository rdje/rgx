//! Lexical analyzer for regex patterns
//!
//! This module implements a lexer that converts regex pattern strings into
//! a stream of tokens, handling all Perl regex features including our custom
//! code execution blocks.

use crate::ast::{AnchorType, CharRange, ConditionalTest, RecursionTarget, Regex};
use crate::token::{LexError, Position, Token, TokenWithPos};
use crate::{trace_decision, trace_enter, trace_exit};
use std::str::Chars;

/// Regex lexer that converts pattern strings to tokens
pub struct Lexer<'a> {
    /// Character iterator
    chars: Chars<'a>,
    /// Current character being processed
    current: Option<char>,
    /// Current position in input
    position: Position,
}

impl<'a> Lexer<'a> {
    /// Create a new lexer for the given input
    #[must_use]
    pub fn new(input: &'a str) -> Self {
        trace_enter!("lexer", "Lexer::new", "input_len={}", input.len());
        let mut lexer = Self {
            chars: input.chars(),
            current: None,
            position: Position::start(),
        };
        lexer.advance(); // Prime the first character
        trace_exit!(
            "lexer",
            "Lexer::new",
            "ok=true,current={:?},offset={}",
            lexer.current,
            lexer.position.offset
        );
        lexer
    }

    fn current_char_snapshot(&self) -> String {
        self.current
            .map_or_else(|| "<eof>".to_string(), |c| format!("{c:?}"))
    }

    fn token_kind(token: &Token) -> &'static str {
        match token {
            Token::Char(_) => "Char",
            Token::CharClass { .. } => "CharClass",
            Token::Dot => "Dot",
            Token::Digit => "Digit",
            Token::DigitNeg => "DigitNeg",
            Token::Word => "Word",
            Token::WordNeg => "WordNeg",
            Token::Space => "Space",
            Token::SpaceNeg => "SpaceNeg",
            Token::WordBoundary => "WordBoundary",
            Token::WordBoundaryNeg => "WordBoundaryNeg",
            Token::UnicodeClass { .. } => "UnicodeClass",
            Token::UnicodeClassNeg { .. } => "UnicodeClassNeg",
            Token::ExtendedCharClass { .. } => "ExtendedCharClass",
            Token::Star => "Star",
            Token::Plus => "Plus",
            Token::Question => "Question",
            Token::StarPossessive => "StarPossessive",
            Token::PlusPossessive => "PlusPossessive",
            Token::QuestionPossessive => "QuestionPossessive",
            Token::StarLazy => "StarLazy",
            Token::PlusLazy => "PlusLazy",
            Token::QuestionLazy => "QuestionLazy",
            Token::Repeat { .. } => "Repeat",
            Token::GroupStart => "GroupStart",
            Token::NamedGroupStart { .. } => "NamedGroupStart",
            Token::NonCapturingGroupStart => "NonCapturingGroupStart",
            Token::AtomicGroupStart => "AtomicGroupStart",
            Token::BranchResetGroupStart => "BranchResetGroupStart",
            Token::GroupEnd => "GroupEnd",
            Token::LookaheadPos => "LookaheadPos",
            Token::LookaheadNeg => "LookaheadNeg",
            Token::LookbehindPos => "LookbehindPos",
            Token::LookbehindNeg => "LookbehindNeg",
            Token::CodeBlock { .. } => "CodeBlock",
            Token::ConditionalStart { .. } => "ConditionalStart",
            Token::Recursion { .. } => "Recursion",
            Token::Alternation => "Alternation",
            Token::Anchor(_) => "Anchor",
            Token::Backreference(_) => "Backreference",
            Token::FlagModifier { .. } => "FlagModifier",
            Token::EOF => "EOF",
        }
    }

    /// Advance to the next character
    fn advance(&mut self) {
        if let Some('\n') = self.current {
            self.position.line += 1;
            self.position.column = 1;
        } else {
            self.position.column += 1;
        }

        self.current = self.chars.next();
        if self.current.is_some() {
            self.position.offset += 1;
        }
    }

    /// Peek at the next character without consuming it
    fn peek(&self) -> Option<char> {
        self.chars.as_str().chars().next()
    }

    /// Get the current position
    fn current_position(&self) -> Position {
        self.position
    }

    /// Get the next token from the input
    ///
    /// # Errors
    ///
    /// Returns [`LexError`] when the remaining input cannot be tokenized into a
    /// valid regex token stream.
    pub fn next_token(&mut self) -> Result<TokenWithPos, LexError> {
        trace_enter!(
            "lexer",
            "Lexer::next_token",
            "current={},offset={}",
            self.current_char_snapshot(),
            self.position.offset
        );
        let start_pos = self.current_position();
        let token_result: Result<Token, LexError> = match self.current {
            None => {
                trace_decision!(
                    "lexer",
                    "self.current.is_none()",
                    true,
                    "emit EOF token at lexical boundary"
                );
                Ok(Token::EOF)
            }
            Some('\\') => self.parse_escape(),
            Some('.') => {
                self.advance();
                Ok(Token::Dot)
            }
            Some('^') => {
                self.advance();
                Ok(Token::Anchor(AnchorType::Start))
            }
            Some('$') => {
                self.advance();
                Ok(Token::Anchor(AnchorType::End))
            }
            Some('*') => Ok(self.parse_star()),
            Some('+') => Ok(self.parse_plus()),
            Some('?') => Ok(self.parse_question()),
            Some('|') => {
                self.advance();
                Ok(Token::Alternation)
            }

            Some('(') => self.parse_group(),
            Some(')') => {
                self.advance();
                Ok(Token::GroupEnd)
            }

            Some('[') => self.parse_character_class(),
            Some('{') => self.parse_repeat_quantifier(),

            Some(c) => {
                self.advance();
                Ok(Token::Char(c))
            }
        };

        let result = token_result.map(|token| TokenWithPos::new(token, start_pos));
        match &result {
            Ok(token) => trace_exit!(
                "lexer",
                "Lexer::next_token",
                "ok=true,token_kind={},offset={}",
                Self::token_kind(&token.token),
                token.position.offset
            ),
            Err(err) => trace_exit!("lexer", "Lexer::next_token", "ok=false,error={}", err),
        }
        result
    }

    /// Parse escape sequences like \d, \w, \n, etc.
    fn parse_escape(&mut self) -> Result<Token, LexError> {
        trace_enter!(
            "lexer",
            "Lexer::parse_escape",
            "current={},offset={}",
            self.current_char_snapshot(),
            self.position.offset
        );
        let start_pos = self.current_position();
        self.advance(); // Skip the backslash

        let result = match self.current {
            None => Err(LexError::UnexpectedEOF {
                expected: "escape sequence".to_string(),
                position: start_pos,
            }),

            // Predefined character classes
            Some('d') => {
                self.advance();
                Ok(Token::Digit)
            }
            Some('D') => {
                self.advance();
                Ok(Token::DigitNeg)
            }
            Some('w') => {
                self.advance();
                Ok(Token::Word)
            }
            Some('W') => {
                self.advance();
                Ok(Token::WordNeg)
            }
            Some('s') => {
                self.advance();
                Ok(Token::Space)
            }
            Some('S') => {
                self.advance();
                Ok(Token::SpaceNeg)
            }
            Some('b') => {
                self.advance();
                Ok(Token::WordBoundary)
            }
            Some('B') => {
                self.advance();
                Ok(Token::WordBoundaryNeg)
            }

            // Anchors
            Some('A') => {
                self.advance();
                Ok(Token::Anchor(AnchorType::AbsStart))
            }
            Some('Z') => {
                self.advance();
                Ok(Token::Anchor(AnchorType::AbsEnd))
            }
            Some('z') => {
                self.advance();
                Ok(Token::Anchor(AnchorType::AbsEndNoNL))
            }

            // Backreferences
            Some(c) if c.is_ascii_digit() => self.parse_backreference(),

            // Unicode property classes
            Some('p') => self.parse_unicode_class(false),
            Some('P') => self.parse_unicode_class(true),

            // Literal escape sequences
            Some('n') => {
                self.advance();
                Ok(Token::Char('\n'))
            }
            Some('t') => {
                self.advance();
                Ok(Token::Char('\t'))
            }
            Some('r') => {
                self.advance();
                Ok(Token::Char('\r'))
            }
            Some('f') => {
                self.advance();
                Ok(Token::Char('\u{0C}'))
            }
            Some('a') => {
                self.advance();
                Ok(Token::Char('\u{07}'))
            }
            Some('e') => {
                self.advance();
                Ok(Token::Char('\u{1B}'))
            }

            // Escaped metacharacters
            Some(
                '.' | '^' | '$' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '\\',
            ) => {
                let ch = self.current.unwrap();
                self.advance();
                Ok(Token::Char(ch))
            }

            // Hex escapes \x{...} or \xFF
            Some('x') => self.parse_hex_escape(),

            // Octal escapes \777
            Some(c) if c.is_ascii_digit() => self.parse_octal_escape(),

            Some(c) => {
                let sequence = format!("\\{c}");
                Err(LexError::InvalidEscape {
                    sequence,
                    position: start_pos,
                })
            }
        };
        match &result {
            Ok(token) => trace_exit!(
                "lexer",
                "Lexer::parse_escape",
                "ok=true,token_kind={}",
                Self::token_kind(token)
            ),
            Err(err) => trace_exit!("lexer", "Lexer::parse_escape", "ok=false,error={}", err),
        }
        result
    }

    /// Parse Unicode property class \p{Name} or \P{Name}
    fn parse_unicode_class(&mut self, negated: bool) -> Result<Token, LexError> {
        trace_enter!(
            "lexer",
            "Lexer::parse_unicode_class",
            "negated={},current={},offset={}",
            negated,
            self.current_char_snapshot(),
            self.position.offset
        );
        let start_pos = self.current_position();
        self.advance(); // Skip 'p' or 'P'
        let has_open_brace = self.current == Some('{');
        trace_decision!(
            "lexer",
            "self.current == Some('{')",
            has_open_brace,
            "unicode class must open with '{{'"
        );
        if !has_open_brace {
            trace_exit!(
                "lexer",
                "Lexer::parse_unicode_class",
                "ok=false,error=missing '{{' in unicode class"
            );
            return Err(LexError::InvalidUnicodeClass {
                name: "missing {".to_string(),
                position: start_pos,
            });
        }
        self.advance(); // Skip '{'

        let mut name = String::new();
        while let Some(c) = self.current {
            if c == '}' {
                self.advance(); // Skip '}'
                break;
            }
            name.push(c);
            self.advance();
        }

        if name.is_empty() {
            trace_exit!(
                "lexer",
                "Lexer::parse_unicode_class",
                "ok=false,error=empty unicode class name"
            );
            return Err(LexError::InvalidUnicodeClass {
                name: "empty class name".to_string(),
                position: start_pos,
            });
        }
        let token = if negated {
            Token::UnicodeClassNeg { name }
        } else {
            Token::UnicodeClass { name }
        };
        trace_exit!(
            "lexer",
            "Lexer::parse_unicode_class",
            "ok=true,token_kind={}",
            Self::token_kind(&token)
        );
        Ok(token)
    }

    /// Parse backreference \1, \2, etc.
    fn parse_backreference(&mut self) -> Result<Token, LexError> {
        trace_enter!(
            "lexer",
            "Lexer::parse_backreference",
            "current={},offset={}",
            self.current_char_snapshot(),
            self.position.offset
        );
        let start_pos = self.current_position();
        let mut number_str = String::new();

        while let Some(c) = self.current {
            if c.is_ascii_digit() {
                number_str.push(c);
                self.advance();
            } else {
                break;
            }
        }

        let number: u32 = if let Ok(number) = number_str.parse() {
            number
        } else {
            trace_exit!(
                "lexer",
                "Lexer::parse_backreference",
                "ok=false,error=invalid backreference number parse {}",
                number_str
            );
            return Err(LexError::InvalidBackreference {
                number: number_str.clone(),
                position: start_pos,
            });
        };
        let in_valid_range = number != 0 && number <= 99;
        trace_decision!(
            "lexer",
            "number != 0 && number <= 99",
            in_valid_range,
            "backreference number must be within 1..=99"
        );
        if !in_valid_range {
            trace_exit!(
                "lexer",
                "Lexer::parse_backreference",
                "ok=false,error=invalid backreference number {}",
                number
            );
            return Err(LexError::InvalidBackreference {
                number: number_str,
                position: start_pos,
            });
        }
        let token = Token::Backreference(number);
        trace_exit!(
            "lexer",
            "Lexer::parse_backreference",
            "ok=true,token_kind={},number={}",
            Self::token_kind(&token),
            number
        );
        Ok(token)
    }

    /// Parse hex escape \x{...} or \xFF
    fn parse_hex_escape(&mut self) -> Result<Token, LexError> {
        trace_enter!(
            "lexer",
            "Lexer::parse_hex_escape",
            "current={},offset={}",
            self.current_char_snapshot(),
            self.position.offset
        );
        let start_pos = self.current_position();
        self.advance(); // Skip 'x'
        let is_braced_hex = self.current == Some('{');
        trace_decision!(
            "lexer",
            "self.current == Some('{')",
            is_braced_hex,
            "choose braced vs two-digit hex escape format"
        );
        let hex_digits = if is_braced_hex {
            // \x{1234} format
            self.advance(); // Skip '{'
            let mut digits = String::new();
            while let Some(c) = self.current {
                if c == '}' {
                    self.advance(); // Skip '}'
                    break;
                } else if c.is_ascii_hexdigit() {
                    digits.push(c);
                    self.advance();
                } else {
                    trace_exit!(
                        "lexer",
                        "Lexer::parse_hex_escape",
                        "ok=false,error=invalid braced hex digit {}",
                        c
                    );
                    return Err(LexError::InvalidEscape {
                        sequence: format!("\\x{{{digits}"),
                        position: start_pos,
                    });
                }
            }
            digits
        } else {
            // \xFF format
            let mut digits = String::new();
            for _ in 0..2 {
                if let Some(c) = self.current {
                    if c.is_ascii_hexdigit() {
                        digits.push(c);
                        self.advance();
                    } else {
                        break;
                    }
                }
            }
            digits
        };

        if hex_digits.is_empty() {
            trace_exit!(
                "lexer",
                "Lexer::parse_hex_escape",
                "ok=false,error=empty hex digits"
            );
            return Err(LexError::InvalidEscape {
                sequence: "\\x".to_string(),
                position: start_pos,
            });
        }

        let Ok(code_point) = u32::from_str_radix(&hex_digits, 16) else {
            trace_exit!(
                "lexer",
                "Lexer::parse_hex_escape",
                "ok=false,error=invalid hex digits {}",
                hex_digits
            );
            return Err(LexError::InvalidEscape {
                sequence: format!("\\x{hex_digits}"),
                position: start_pos,
            });
        };

        let Some(ch) = char::from_u32(code_point) else {
            trace_exit!(
                "lexer",
                "Lexer::parse_hex_escape",
                "ok=false,error=invalid hex code point {}",
                code_point
            );
            return Err(LexError::InvalidEscape {
                sequence: format!("\\x{hex_digits}"),
                position: start_pos,
            });
        };
        let token = Token::Char(ch);
        trace_exit!(
            "lexer",
            "Lexer::parse_hex_escape",
            "ok=true,token_kind={},code_point={}",
            Self::token_kind(&token),
            code_point
        );
        Ok(token)
    }

    /// Parse octal escape \777
    fn parse_octal_escape(&mut self) -> Result<Token, LexError> {
        trace_enter!(
            "lexer",
            "Lexer::parse_octal_escape",
            "current={},offset={}",
            self.current_char_snapshot(),
            self.position.offset
        );
        let start_pos = self.current_position();
        let mut octal_digits = String::new();

        // Collect up to 3 octal digits
        for _ in 0..3 {
            if let Some(c) = self.current {
                if ('0'..='7').contains(&c) {
                    octal_digits.push(c);
                    self.advance();
                } else {
                    break;
                }
            }
        }

        if octal_digits.is_empty() {
            trace_exit!(
                "lexer",
                "Lexer::parse_octal_escape",
                "ok=false,error=empty octal digits"
            );
            return Err(LexError::InvalidEscape {
                sequence: "\\".to_string(),
                position: start_pos,
            });
        }

        let Ok(code_point) = u32::from_str_radix(&octal_digits, 8) else {
            trace_exit!(
                "lexer",
                "Lexer::parse_octal_escape",
                "ok=false,error=invalid octal digits {}",
                octal_digits
            );
            return Err(LexError::InvalidEscape {
                sequence: format!("\\{octal_digits}"),
                position: start_pos,
            });
        };

        // Octal escapes are limited to byte values (0-255)
        let is_byte_range = code_point <= 255;
        trace_decision!(
            "lexer",
            "code_point <= 255",
            is_byte_range,
            "octal escapes are limited to byte-range values"
        );
        if !is_byte_range {
            trace_exit!(
                "lexer",
                "Lexer::parse_octal_escape",
                "ok=false,error=octal code point out of range {}",
                code_point
            );
            return Err(LexError::InvalidEscape {
                sequence: format!("\\{octal_digits}"),
                position: start_pos,
            });
        }

        let Some(ch) = char::from_u32(code_point) else {
            trace_exit!(
                "lexer",
                "Lexer::parse_octal_escape",
                "ok=false,error=invalid octal code point {}",
                code_point
            );
            return Err(LexError::InvalidEscape {
                sequence: format!("\\{octal_digits}"),
                position: start_pos,
            });
        };
        let token = Token::Char(ch);
        trace_exit!(
            "lexer",
            "Lexer::parse_octal_escape",
            "ok=true,token_kind={},code_point={}",
            Self::token_kind(&token),
            code_point
        );
        Ok(token)
    }

    /// Parse *, *?, and *+ quantifiers
    fn parse_star(&mut self) -> Token {
        trace_enter!(
            "lexer",
            "Lexer::parse_star",
            "current={},offset={}",
            self.current_char_snapshot(),
            self.position.offset
        );
        self.advance(); // Skip '*'
        let token = if self.current == Some('?') {
            self.advance(); // Skip '?'
            Token::StarLazy
        } else if self.current == Some('+') {
            self.advance(); // Skip '+'
            Token::StarPossessive
        } else {
            Token::Star
        };
        trace_exit!(
            "lexer",
            "Lexer::parse_star",
            "ok=true,token_kind={}",
            Self::token_kind(&token)
        );
        token
    }

    /// Parse +, +?, and ++ quantifiers
    fn parse_plus(&mut self) -> Token {
        trace_enter!(
            "lexer",
            "Lexer::parse_plus",
            "current={},offset={}",
            self.current_char_snapshot(),
            self.position.offset
        );
        self.advance(); // Skip '+'
        let token = if self.current == Some('?') {
            self.advance(); // Skip '?'
            Token::PlusLazy
        } else if self.current == Some('+') {
            self.advance(); // Skip '+'
            Token::PlusPossessive
        } else {
            Token::Plus
        };
        trace_exit!(
            "lexer",
            "Lexer::parse_plus",
            "ok=true,token_kind={}",
            Self::token_kind(&token)
        );
        token
    }

    /// Parse ?, ??, and ?+ quantifiers
    fn parse_question(&mut self) -> Token {
        trace_enter!(
            "lexer",
            "Lexer::parse_question",
            "current={},offset={}",
            self.current_char_snapshot(),
            self.position.offset
        );
        self.advance(); // Skip '?'
        let token = if self.current == Some('?') {
            self.advance(); // Skip second '?'
            Token::QuestionLazy
        } else if self.current == Some('+') {
            self.advance(); // Skip '+'
            Token::QuestionPossessive
        } else {
            Token::Question
        };
        trace_exit!(
            "lexer",
            "Lexer::parse_question",
            "ok=true,token_kind={}",
            Self::token_kind(&token)
        );
        token
    }

    /// Parse group constructs: (...), (?:...), (?<name>...), (?=...), (?!...), (?<=...), (?<!...), (?>...), (?|...), (?[...]), (?(...)), (?{lang:code})
    fn parse_group(&mut self) -> Result<Token, LexError> {
        trace_enter!(
            "lexer",
            "Lexer::parse_group",
            "current={},offset={}",
            self.current_char_snapshot(),
            self.position.offset
        );
        let start_pos = self.current_position();
        self.advance(); // Skip '('

        // Simple capturing group
        if self.current != Some('?') {
            trace_decision!(
                "lexer",
                "self.current != Some('?')",
                true,
                "emit capturing group start token"
            );
            trace_exit!(
                "lexer",
                "Lexer::parse_group",
                "ok=true,token_kind={}",
                Self::token_kind(&Token::GroupStart)
            );
            return Ok(Token::GroupStart);
        }
        trace_decision!(
            "lexer",
            "self.current != Some('?')",
            false,
            "dispatching to special group syntax parser"
        );

        self.advance(); // Skip '?'

        let result = match self.current {
            Some('(') => self.parse_conditional_start(start_pos),
            Some('{') => {
                self.advance(); // Skip '{'

                // Parse language name up to ':'
                let mut lang = String::new();
                while let Some(c) = self.current {
                    if c == ':' {
                        break;
                    }
                    if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                        lang.push(c);
                        self.advance();
                    } else {
                        return Err(LexError::InvalidGroupSyntax {
                            position: start_pos,
                        });
                    }
                }

                if lang.is_empty() {
                    return Err(LexError::InvalidGroupSyntax {
                        position: start_pos,
                    });
                }

                if self.current != Some(':') {
                    return Err(LexError::ExpectedColon {
                        position: start_pos,
                    });
                }
                self.advance(); // Skip ':'

                // Parse code until terminating "})"
                let mut code = String::new();
                loop {
                    match self.current {
                        Some('}') if self.peek() == Some(')') => {
                            self.advance(); // Skip '}'
                            self.advance(); // Skip ')'
                            break;
                        }
                        Some(c) => {
                            code.push(c);
                            self.advance();
                        }
                        None => {
                            return Err(LexError::UnterminatedCodeBlock {
                                position: start_pos,
                            });
                        }
                    }
                }

                Ok(Token::CodeBlock { lang, code })
            }
            Some(':') => {
                self.advance(); // Skip ':'
                Ok(Token::NonCapturingGroupStart)
            }
            Some('=') => {
                self.advance(); // Skip '='
                Ok(Token::LookaheadPos)
            }
            Some('!') => {
                self.advance(); // Skip '!'
                Ok(Token::LookaheadNeg)
            }
            Some('>') => {
                self.advance(); // Skip '>'
                Ok(Token::AtomicGroupStart)
            }
            Some('|') => {
                self.advance(); // Skip '|'
                Ok(Token::BranchResetGroupStart)
            }
            Some('[') => self.parse_extended_char_class(start_pos),
            Some('R') => {
                self.advance(); // Skip 'R'
                if self.current == Some(')') {
                    self.advance(); // Skip ')'
                    Ok(Token::Recursion {
                        target: RecursionTarget::Entire,
                    })
                } else {
                    Err(LexError::InvalidGroupSyntax {
                        position: start_pos,
                    })
                }
            }
            Some('&') => {
                self.advance(); // Skip '&'
                let mut name = String::new();
                while let Some(c) = self.current {
                    if c == ')' {
                        break;
                    }
                    if c.is_ascii_alphanumeric() || c == '_' {
                        name.push(c);
                        self.advance();
                    } else {
                        return Err(LexError::InvalidGroupSyntax {
                            position: start_pos,
                        });
                    }
                }

                if name.is_empty() || self.current != Some(')') {
                    return Err(LexError::InvalidGroupSyntax {
                        position: start_pos,
                    });
                }
                self.advance(); // Skip ')'
                Ok(Token::Recursion {
                    target: RecursionTarget::NamedGroup(name),
                })
            }
            Some(c) if c.is_ascii_digit() => {
                let mut number_str = String::new();
                while let Some(d) = self.current {
                    if d.is_ascii_digit() {
                        number_str.push(d);
                        self.advance();
                    } else {
                        break;
                    }
                }

                if self.current != Some(')') {
                    return Err(LexError::InvalidGroupSyntax {
                        position: start_pos,
                    });
                }
                self.advance(); // Skip ')'

                let group_num =
                    number_str
                        .parse::<u32>()
                        .map_err(|_| LexError::InvalidGroupSyntax {
                            position: start_pos,
                        })?;
                if group_num == 0 {
                    return Err(LexError::InvalidGroupSyntax {
                        position: start_pos,
                    });
                }
                Ok(Token::Recursion {
                    target: RecursionTarget::Group(group_num),
                })
            }
            Some('<') => {
                self.advance(); // Skip '<'

                if self.current == Some('=') {
                    self.advance(); // Skip '='
                    return Ok(Token::LookbehindPos);
                }
                if self.current == Some('!') {
                    self.advance(); // Skip '!'
                    return Ok(Token::LookbehindNeg);
                }
                let mut name = String::new();

                while let Some(c) = self.current {
                    if c == '>' {
                        self.advance(); // Skip '>'
                        break;
                    }
                    if c.is_ascii_alphanumeric() || c == '_' {
                        name.push(c);
                        self.advance();
                    } else {
                        return Err(LexError::InvalidGroupSyntax {
                            position: start_pos,
                        });
                    }
                }

                if name.is_empty() {
                    return Err(LexError::InvalidGroupSyntax {
                        position: start_pos,
                    });
                }

                Ok(Token::NamedGroupStart { name })
            }
            _ => Err(LexError::InvalidGroupSyntax {
                position: start_pos,
            }),
        };
        match &result {
            Ok(token) => trace_exit!(
                "lexer",
                "Lexer::parse_group",
                "ok=true,token_kind={}",
                Self::token_kind(token)
            ),
            Err(err) => trace_exit!("lexer", "Lexer::parse_group", "ok=false,error={}", err),
        }
        result
    }

    fn parse_extended_char_class(&mut self, start_pos: Position) -> Result<Token, LexError> {
        self.advance(); // Skip '[' after "(?"
        let mut content = String::new();
        let mut bracket_depth = 1usize;
        let mut escaped = false;

        loop {
            match self.current {
                Some(ch) if escaped => {
                    content.push(ch);
                    escaped = false;
                    self.advance();
                }
                Some('\\') => {
                    content.push('\\');
                    escaped = true;
                    self.advance();
                }
                Some('[') => {
                    bracket_depth += 1;
                    content.push('[');
                    self.advance();
                }
                Some(']') if bracket_depth == 1 && self.peek() == Some(')') => {
                    self.advance(); // Skip ']'
                    self.advance(); // Skip ')'
                    break;
                }
                Some(']') => {
                    bracket_depth = bracket_depth.saturating_sub(1);
                    content.push(']');
                    self.advance();
                }
                Some(ch) => {
                    content.push(ch);
                    self.advance();
                }
                None => {
                    return Err(LexError::UnterminatedGroup {
                        position: start_pos,
                    })
                }
            }
        }

        Ok(Token::ExtendedCharClass { content })
    }

    /// Parse conditional start:
    /// - (?(1)...)
    /// - (?(+1)...)
    /// - (?(-1)...)
    /// - (?(<name>)...)
    /// - (?(name)...)
    /// - (?(R)...)
    /// - (?(R1)...)
    /// - (?(R&name)...)
    /// - (?(?=expr)...)
    /// - (?(?!expr)...)
    /// - (?(?<=expr)...)
    /// - (?(?<!expr)...)
    ///
    /// This returns only the condition-start token. Branch expressions and
    /// the closing group ')' are parsed by the parser stage.
    fn parse_conditional_start(&mut self, start_pos: Position) -> Result<Token, LexError> {
        trace_enter!(
            "lexer",
            "Lexer::parse_conditional_start",
            "current={},offset={}",
            self.current_char_snapshot(),
            self.position.offset
        );
        self.advance(); // Skip '(' after "(?"

        let condition = match self.current {
            Some(c) if c.is_ascii_digit() => {
                let mut number_str = String::new();
                while let Some(d) = self.current {
                    if d.is_ascii_digit() {
                        number_str.push(d);
                        self.advance();
                    } else {
                        break;
                    }
                }

                let group_num =
                    number_str
                        .parse::<u32>()
                        .map_err(|_| LexError::InvalidGroupSyntax {
                            position: start_pos,
                        })?;
                if group_num == 0 {
                    return Err(LexError::InvalidGroupSyntax {
                        position: start_pos,
                    });
                }
                ConditionalTest::GroupExists(group_num)
            }
            Some(sign @ ('+' | '-')) => {
                let sign = if sign == '-' { -1 } else { 1 };
                self.advance(); // Skip '+' or '-'

                let mut number_str = String::new();
                while let Some(d) = self.current {
                    if d.is_ascii_digit() {
                        number_str.push(d);
                        self.advance();
                    } else {
                        break;
                    }
                }

                let group_offset =
                    number_str
                        .parse::<i32>()
                        .map_err(|_| LexError::InvalidGroupSyntax {
                            position: start_pos,
                        })?;
                if group_offset == 0 {
                    return Err(LexError::InvalidGroupSyntax {
                        position: start_pos,
                    });
                }
                ConditionalTest::RelativeGroupExists(sign * group_offset)
            }
            Some('<') => {
                self.advance(); // Skip '<'
                let mut name = String::new();
                while let Some(c) = self.current {
                    if c == '>' {
                        self.advance(); // Skip '>'
                        break;
                    }
                    if c.is_ascii_alphanumeric() || c == '_' {
                        name.push(c);
                        self.advance();
                    } else {
                        return Err(LexError::InvalidGroupSyntax {
                            position: start_pos,
                        });
                    }
                }

                if name.is_empty() {
                    return Err(LexError::InvalidGroupSyntax {
                        position: start_pos,
                    });
                }
                ConditionalTest::NamedGroupExists(name)
            }
            Some(c) if c.is_ascii_alphabetic() || c == '_' => {
                if self.current == Some('R') && self.peek() == Some('&') {
                    self.advance(); // Skip 'R'
                    self.advance(); // Skip '&'

                    let mut name = String::new();
                    match self.current {
                        Some(ch) if ch.is_ascii_alphabetic() || ch == '_' => {
                            name.push(ch);
                            self.advance();
                        }
                        _ => {
                            return Err(LexError::InvalidGroupSyntax {
                                position: start_pos,
                            });
                        }
                    }

                    while let Some(ch) = self.current {
                        if ch.is_ascii_alphanumeric() || ch == '_' {
                            name.push(ch);
                            self.advance();
                        } else {
                            break;
                        }
                    }

                    ConditionalTest::RecursionNamed(name)
                } else {
                    let mut name = String::new();
                    while let Some(ch) = self.current {
                        if ch.is_ascii_alphanumeric() || ch == '_' {
                            name.push(ch);
                            self.advance();
                        } else {
                            break;
                        }
                    }

                    if name.is_empty() {
                        return Err(LexError::InvalidGroupSyntax {
                            position: start_pos,
                        });
                    }
                    if let Some(group_text) = name.strip_prefix('R') {
                        if group_text.is_empty() {
                            ConditionalTest::RecursionAny
                        } else if group_text.chars().all(|ch| ch.is_ascii_digit()) {
                            let group = group_text.parse::<u32>().map_err(|_| {
                                LexError::InvalidGroupSyntax {
                                    position: start_pos,
                                }
                            })?;
                            if group == 0 {
                                return Err(LexError::InvalidGroupSyntax {
                                    position: start_pos,
                                });
                            }
                            ConditionalTest::RecursionGroup(group)
                        } else if name == "DEFINE" {
                            ConditionalTest::Define
                        } else {
                            ConditionalTest::NamedGroupExists(name)
                        }
                    } else if name == "DEFINE" {
                        ConditionalTest::Define
                    } else {
                        ConditionalTest::NamedGroupExists(name)
                    }
                }
            }
            Some('?') => {
                self.advance(); // Skip '?' in condition test
                match self.current {
                    Some('=') => {
                        self.advance(); // Skip '='
                        let expr = self.parse_conditional_subexpression_ast(start_pos)?;
                        ConditionalTest::Lookahead {
                            expr: Box::new(expr),
                            positive: true,
                        }
                    }
                    Some('!') => {
                        self.advance(); // Skip '!'
                        let expr = self.parse_conditional_subexpression_ast(start_pos)?;
                        ConditionalTest::Lookahead {
                            expr: Box::new(expr),
                            positive: false,
                        }
                    }
                    Some('<') => {
                        self.advance(); // Skip '<'
                        let positive = match self.current {
                            Some('=') => true,
                            Some('!') => false,
                            _ => {
                                return Err(LexError::InvalidGroupSyntax {
                                    position: start_pos,
                                });
                            }
                        };
                        self.advance(); // Skip '=' or '!'
                        let expr = self.parse_conditional_subexpression_ast(start_pos)?;
                        ConditionalTest::Lookbehind {
                            expr: Box::new(expr),
                            positive,
                        }
                    }
                    _ => {
                        return Err(LexError::InvalidGroupSyntax {
                            position: start_pos,
                        });
                    }
                }
            }
            _ => {
                return Err(LexError::InvalidGroupSyntax {
                    position: start_pos,
                });
            }
        };

        let has_condition_close = self.current == Some(')');
        trace_decision!(
            "lexer",
            "self.current == Some(')')",
            has_condition_close,
            "conditional test must terminate with ')'"
        );
        if !has_condition_close {
            trace_exit!(
                "lexer",
                "Lexer::parse_conditional_start",
                "ok=false,error=missing ')' after condition test"
            );
            return Err(LexError::InvalidGroupSyntax {
                position: start_pos,
            });
        }
        self.advance(); // Skip ')' ending condition test
        trace_exit!(
            "lexer",
            "Lexer::parse_conditional_start",
            "ok=true,token_kind={}",
            Self::token_kind(&Token::ConditionalStart {
                condition: condition.clone()
            })
        );

        Ok(Token::ConditionalStart { condition })
    }

    /// Parse the condition sub-expression text of a lookaround conditional and
    /// build its AST.
    ///
    /// Leaves `self.current` positioned on the closing ')' of the condition.
    fn parse_conditional_subexpression_ast(
        &mut self,
        start_pos: Position,
    ) -> Result<Regex, LexError> {
        trace_enter!(
            "lexer",
            "Lexer::parse_conditional_subexpression_ast",
            "current={},offset={}",
            self.current_char_snapshot(),
            self.position.offset
        );
        let mut expr_text = String::new();
        let mut paren_depth = 0usize;
        let mut in_char_class = false;
        let mut escaped = false;

        loop {
            match self.current {
                None => {
                    trace_exit!(
                        "lexer",
                        "Lexer::parse_conditional_subexpression_ast",
                        "ok=false,error=unterminated conditional subexpression"
                    );
                    return Err(LexError::UnterminatedGroup {
                        position: start_pos,
                    });
                }
                Some(ch) => {
                    if escaped {
                        expr_text.push(ch);
                        escaped = false;
                        self.advance();
                        continue;
                    }

                    if ch == '\\' {
                        expr_text.push(ch);
                        escaped = true;
                        self.advance();
                        continue;
                    }

                    if in_char_class {
                        expr_text.push(ch);
                        if ch == ']' {
                            in_char_class = false;
                        }
                        self.advance();
                        continue;
                    }

                    match ch {
                        '[' => {
                            in_char_class = true;
                            expr_text.push(ch);
                            self.advance();
                        }
                        '(' => {
                            paren_depth += 1;
                            expr_text.push(ch);
                            self.advance();
                        }
                        ')' => {
                            if paren_depth == 0 {
                                break;
                            }
                            paren_depth -= 1;
                            expr_text.push(ch);
                            self.advance();
                        }
                        _ => {
                            expr_text.push(ch);
                            self.advance();
                        }
                    }
                }
            }
        }

        let mut parser = match crate::parser::Parser::new(&expr_text) {
            Ok(parser) => parser,
            Err(err) => {
                trace_exit!(
                    "lexer",
                    "Lexer::parse_conditional_subexpression_ast",
                    "ok=false,error={}",
                    err
                );
                return Err(err);
            }
        };
        let result = parser.parse();
        match &result {
            Ok(_) => trace_exit!(
                "lexer",
                "Lexer::parse_conditional_subexpression_ast",
                "ok=true,expr_len={}",
                expr_text.len()
            ),
            Err(err) => trace_exit!(
                "lexer",
                "Lexer::parse_conditional_subexpression_ast",
                "ok=false,error={}",
                err
            ),
        }
        result
    }

    /// Parse character class [abc], [^abc], [a-z], etc.
    fn parse_character_class(&mut self) -> Result<Token, LexError> {
        trace_enter!(
            "lexer",
            "Lexer::parse_character_class",
            "current={},offset={}",
            self.current_char_snapshot(),
            self.position.offset
        );
        let start_pos = self.current_position();
        self.advance(); // Skip '['

        let negated = if self.current == Some('^') {
            self.advance(); // Skip '^'
            true
        } else {
            false
        };

        let mut ranges = Vec::new();

        while let Some(c) = self.current {
            if c == ']' && !ranges.is_empty() {
                self.advance(); // Skip ']'
                break;
            }

            if c == '\\' {
                self.advance(); // Skip '\'
                if let Some(escaped) = self.current {
                    ranges.push(CharRange::single(match escaped {
                        'n' => '\n',
                        't' => '\t',
                        'r' => '\r',
                        '\\' => '\\',
                        ']' => ']',
                        '-' => '-',
                        '^' => '^',
                        c => c, // Other escapes handled elsewhere
                    }));
                    self.advance();
                }
            } else {
                let start_char = c;
                self.advance();

                if self.current == Some('-') && self.peek() != Some(']') {
                    // Character range a-z
                    self.advance(); // Skip '-'
                    if let Some(end_char) = self.current {
                        if end_char < start_char {
                            trace_exit!(
                                "lexer",
                                "Lexer::parse_character_class",
                                "ok=false,error=invalid range {}-{}",
                                start_char,
                                end_char
                            );
                            return Err(LexError::InvalidCharRange {
                                start: start_char,
                                end: end_char,
                                position: self.current_position(),
                            });
                        }
                        ranges.push(CharRange::range(start_char, end_char));
                        self.advance();
                    } else {
                        trace_exit!(
                            "lexer",
                            "Lexer::parse_character_class",
                            "ok=false,error=unterminated character class after '-'"
                        );
                        return Err(LexError::UnterminatedCharClass {
                            position: start_pos,
                        });
                    }
                } else {
                    // Single character
                    ranges.push(CharRange::single(start_char));
                }
            }
        }

        if ranges.is_empty() {
            trace_exit!(
                "lexer",
                "Lexer::parse_character_class",
                "ok=false,error=empty or unterminated character class"
            );
            return Err(LexError::UnterminatedCharClass {
                position: start_pos,
            });
        }
        trace_exit!(
            "lexer",
            "Lexer::parse_character_class",
            "ok=true,ranges={},negated={}",
            ranges.len(),
            negated
        );
        Ok(Token::CharClass { ranges, negated })
    }

    /// Parse repeat quantifier {n}, {n,}, {n,m}, {n,m}?, {n,m}+
    fn parse_repeat_quantifier(&mut self) -> Result<Token, LexError> {
        trace_enter!(
            "lexer",
            "Lexer::parse_repeat_quantifier",
            "current={},offset={}",
            self.current_char_snapshot(),
            self.position.offset
        );
        let start_pos = self.current_position();
        self.advance(); // Skip '{'

        let mut content = String::new();
        while let Some(c) = self.current {
            if c == '}' {
                self.advance(); // Skip '}'
                break;
            }
            content.push(c);
            self.advance();
        }

        // Parse the content: "n", "n,", "n,m"
        let parts: Vec<&str> = content.split(',').collect();
        trace_decision!(
            "lexer",
            "parts.len() <= 2",
            parts.len() <= 2,
            "repeat quantifier form must be one of {{n}}, {{n,}}, {{n,m}}"
        );

        let (min, max) = match parts.as_slice() {
            [min_str] => {
                // {n} - exactly n times
                let min: u32 = min_str.parse().map_err(|_| LexError::InvalidRepeat {
                    text: content.clone(),
                    position: start_pos,
                })?;
                (min, Some(min))
            }
            [min_str, ""] => {
                // {n,} - n or more times
                let min: u32 = min_str.parse().map_err(|_| LexError::InvalidRepeat {
                    text: content.clone(),
                    position: start_pos,
                })?;
                (min, None)
            }
            [min_str, max_str] => {
                // {n,m} - between n and m times
                let min: u32 = min_str.parse().map_err(|_| LexError::InvalidRepeat {
                    text: content.clone(),
                    position: start_pos,
                })?;
                let max: u32 = max_str.parse().map_err(|_| LexError::InvalidRepeat {
                    text: content.clone(),
                    position: start_pos,
                })?;
                if max < min {
                    return Err(LexError::InvalidRepeat {
                        text: content,
                        position: start_pos,
                    });
                }
                (min, Some(max))
            }
            _ => {
                trace_exit!(
                    "lexer",
                    "Lexer::parse_repeat_quantifier",
                    "ok=false,error=invalid repeat text {}",
                    content
                );
                return Err(LexError::InvalidRepeat {
                    text: content,
                    position: start_pos,
                });
            }
        };

        // Check for lazy or possessive suffix
        let (lazy, possessive) = if self.current == Some('?') {
            self.advance(); // Skip '?'
            (true, false)
        } else if self.current == Some('+') {
            self.advance(); // Skip '+'
            (false, true)
        } else {
            (false, false)
        };
        trace_exit!(
            "lexer",
            "Lexer::parse_repeat_quantifier",
            "ok=true,min={},max={:?},lazy={},possessive={}",
            min,
            max,
            lazy,
            possessive
        );
        Ok(Token::Repeat {
            min,
            max,
            lazy,
            possessive,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokenize_all(input: &str) -> Result<Vec<Token>, LexError> {
        let mut lexer = Lexer::new(input);
        let mut tokens = Vec::new();

        loop {
            let token_pos = lexer.next_token()?;
            if token_pos.token == Token::EOF {
                break;
            }
            tokens.push(token_pos.token);
        }

        Ok(tokens)
    }

    #[test]
    fn test_simple_literals() {
        let tokens = tokenize_all("abc").unwrap();
        assert_eq!(
            tokens,
            vec![Token::Char('a'), Token::Char('b'), Token::Char('c')]
        );
    }

    #[test]
    fn test_quantifiers() {
        let tokens = tokenize_all("a*b+c?").unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::Char('a'),
                Token::Star,
                Token::Char('b'),
                Token::Plus,
                Token::Char('c'),
                Token::Question
            ]
        );
    }

    #[test]
    fn test_lazy_quantifiers() {
        let tokens = tokenize_all("a*?b+?c??").unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::Char('a'),
                Token::StarLazy,
                Token::Char('b'),
                Token::PlusLazy,
                Token::Char('c'),
                Token::QuestionLazy
            ]
        );
    }

    #[test]
    fn test_possessive_quantifiers() {
        let tokens = tokenize_all("a*+b++c?+d{2,5}+").unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::Char('a'),
                Token::StarPossessive,
                Token::Char('b'),
                Token::PlusPossessive,
                Token::Char('c'),
                Token::QuestionPossessive,
                Token::Char('d'),
                Token::Repeat {
                    min: 2,
                    max: Some(5),
                    lazy: false,
                    possessive: true,
                }
            ]
        );
    }

    #[test]
    fn test_character_classes() {
        let tokens = tokenize_all(r"\d\w\s").unwrap();
        assert_eq!(tokens, vec![Token::Digit, Token::Word, Token::Space]);
    }

    #[test]
    fn test_basic_groups() {
        let tokens = tokenize_all("(abc)").unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::GroupStart,
                Token::Char('a'),
                Token::Char('b'),
                Token::Char('c'),
                Token::GroupEnd
            ]
        );
    }

    #[test]
    fn test_non_capturing_groups() {
        let tokens = tokenize_all("(?:ab)").unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::NonCapturingGroupStart,
                Token::Char('a'),
                Token::Char('b'),
                Token::GroupEnd,
            ]
        );
    }

    #[test]
    fn test_code_block_token() {
        let tokens = tokenize_all("(?{lua:return arg[0] ~= nil})").unwrap();
        assert_eq!(
            tokens,
            vec![Token::CodeBlock {
                lang: "lua".to_string(),
                code: "return arg[0] ~= nil".to_string(),
            },]
        );
    }

    #[test]
    fn test_recursion_tokens() {
        let tokens = tokenize_all("(?R)(?1)(?&name)").unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::Recursion {
                    target: RecursionTarget::Entire,
                },
                Token::Recursion {
                    target: RecursionTarget::Group(1),
                },
                Token::Recursion {
                    target: RecursionTarget::NamedGroup("name".to_string()),
                },
            ]
        );
    }

    #[test]
    fn test_branch_reset_group_tokens() {
        let tokens = tokenize_all("(?|a|b)").unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::BranchResetGroupStart,
                Token::Char('a'),
                Token::Alternation,
                Token::Char('b'),
                Token::GroupEnd,
            ]
        );
    }

    #[test]
    fn test_extended_char_class_tokens() {
        let tokens = tokenize_all("(?[[a-z]])").unwrap();
        assert_eq!(
            tokens,
            vec![Token::ExtendedCharClass {
                content: "[a-z]".to_string(),
            }]
        );
    }

    #[test]
    fn test_conditional_tokens_group_exists() {
        let tokens = tokenize_all("(?(1)yes|no)").unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::ConditionalStart {
                    condition: ConditionalTest::GroupExists(1),
                },
                Token::Char('y'),
                Token::Char('e'),
                Token::Char('s'),
                Token::Alternation,
                Token::Char('n'),
                Token::Char('o'),
                Token::GroupEnd,
            ]
        );
    }

    #[test]
    fn test_conditional_tokens_relative_group_exists_positive() {
        let tokens = tokenize_all("(?(+1)yes|no)").unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::ConditionalStart {
                    condition: ConditionalTest::RelativeGroupExists(1),
                },
                Token::Char('y'),
                Token::Char('e'),
                Token::Char('s'),
                Token::Alternation,
                Token::Char('n'),
                Token::Char('o'),
                Token::GroupEnd,
            ]
        );
    }

    #[test]
    fn test_conditional_tokens_relative_group_exists_negative() {
        let tokens = tokenize_all("(?(-1)yes|no)").unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::ConditionalStart {
                    condition: ConditionalTest::RelativeGroupExists(-1),
                },
                Token::Char('y'),
                Token::Char('e'),
                Token::Char('s'),
                Token::Alternation,
                Token::Char('n'),
                Token::Char('o'),
                Token::GroupEnd,
            ]
        );
    }

    #[test]
    fn test_conditional_tokens_named_group_exists() {
        let tokens = tokenize_all("(?(<word>)a|b)").unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::ConditionalStart {
                    condition: ConditionalTest::NamedGroupExists("word".to_string()),
                },
                Token::Char('a'),
                Token::Alternation,
                Token::Char('b'),
                Token::GroupEnd,
            ]
        );
    }

    #[test]
    fn test_conditional_tokens_negative_lookbehind_condition() {
        let tokens = tokenize_all("(?(?<!z)a|b)").unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::ConditionalStart {
                    condition: ConditionalTest::Lookbehind {
                        expr: Box::new(Regex::Char('z')),
                        positive: false,
                    },
                },
                Token::Char('a'),
                Token::Alternation,
                Token::Char('b'),
                Token::GroupEnd,
            ]
        );
    }

    #[test]
    fn test_conditional_tokens_bare_named_group_exists() {
        let tokens = tokenize_all("(?(word)a|b)").unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::ConditionalStart {
                    condition: ConditionalTest::NamedGroupExists("word".to_string()),
                },
                Token::Char('a'),
                Token::Alternation,
                Token::Char('b'),
                Token::GroupEnd,
            ]
        );
    }

    #[test]
    fn test_conditional_tokens_define_condition() {
        let tokens = tokenize_all("(?(DEFINE)a)").unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::ConditionalStart {
                    condition: ConditionalTest::Define,
                },
                Token::Char('a'),
                Token::GroupEnd,
            ]
        );
    }

    #[test]
    fn test_conditional_tokens_recursion_any() {
        let tokens = tokenize_all("(?(R)a|b)").unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::ConditionalStart {
                    condition: ConditionalTest::RecursionAny,
                },
                Token::Char('a'),
                Token::Alternation,
                Token::Char('b'),
                Token::GroupEnd,
            ]
        );
    }

    #[test]
    fn test_conditional_tokens_recursion_group() {
        let tokens = tokenize_all("(?(R1)a|b)").unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::ConditionalStart {
                    condition: ConditionalTest::RecursionGroup(1),
                },
                Token::Char('a'),
                Token::Alternation,
                Token::Char('b'),
                Token::GroupEnd,
            ]
        );
    }

    #[test]
    fn test_conditional_tokens_recursion_named() {
        let tokens = tokenize_all("(?(R&word)a|b)").unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::ConditionalStart {
                    condition: ConditionalTest::RecursionNamed("word".to_string()),
                },
                Token::Char('a'),
                Token::Alternation,
                Token::Char('b'),
                Token::GroupEnd,
            ]
        );
    }

    #[test]
    fn test_conditional_tokens_lookahead_condition() {
        let tokens = tokenize_all("(?(?=ab)x|y)").unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::ConditionalStart {
                    condition: ConditionalTest::Lookahead {
                        expr: Box::new(Regex::Sequence(vec![Regex::Char('a'), Regex::Char('b')])),
                        positive: true,
                    },
                },
                Token::Char('x'),
                Token::Alternation,
                Token::Char('y'),
                Token::GroupEnd,
            ]
        );
    }
    #[test]
    fn test_conditional_tokens_negative_lookahead_condition() {
        let tokens = tokenize_all("(?(?!ab)x|y)").unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::ConditionalStart {
                    condition: ConditionalTest::Lookahead {
                        expr: Box::new(Regex::Sequence(vec![Regex::Char('a'), Regex::Char('b')])),
                        positive: false,
                    },
                },
                Token::Char('x'),
                Token::Alternation,
                Token::Char('y'),
                Token::GroupEnd,
            ]
        );
    }

    #[test]
    fn test_conditional_tokens_lookbehind_condition() {
        let tokens = tokenize_all("(?(?<=z)a|b)").unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::ConditionalStart {
                    condition: ConditionalTest::Lookbehind {
                        expr: Box::new(Regex::Char('z')),
                        positive: true,
                    },
                },
                Token::Char('a'),
                Token::Alternation,
                Token::Char('b'),
                Token::GroupEnd,
            ]
        );
    }

    #[test]
    fn test_named_groups() {
        let tokens = tokenize_all("(?<word>ab)").unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::NamedGroupStart {
                    name: "word".to_string()
                },
                Token::Char('a'),
                Token::Char('b'),
                Token::GroupEnd,
            ]
        );
    }

    #[test]
    fn test_lookaround_group_tokens() {
        let tokens = tokenize_all("(?=a)(?!b)(?<=c)(?<!d)").unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::LookaheadPos,
                Token::Char('a'),
                Token::GroupEnd,
                Token::LookaheadNeg,
                Token::Char('b'),
                Token::GroupEnd,
                Token::LookbehindPos,
                Token::Char('c'),
                Token::GroupEnd,
                Token::LookbehindNeg,
                Token::Char('d'),
                Token::GroupEnd,
            ]
        );
    }

    #[test]
    fn test_character_class() {
        let tokens = tokenize_all("[abc]").unwrap();
        assert_eq!(tokens.len(), 1);
        match &tokens[0] {
            Token::CharClass { ranges, negated } => {
                assert!(!negated);
                assert_eq!(ranges.len(), 3);
            }
            _ => panic!("Expected CharClass token"),
        }
    }

    #[test]
    fn test_repeat_quantifier() {
        let tokens = tokenize_all("a{2,5}").unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::Char('a'),
                Token::Repeat {
                    min: 2,
                    max: Some(5),
                    lazy: false,
                    possessive: false,
                }
            ]
        );
    }

    #[test]
    fn test_anchors() {
        let tokens = tokenize_all("^abc$").unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::Anchor(AnchorType::Start),
                Token::Char('a'),
                Token::Char('b'),
                Token::Char('c'),
                Token::Anchor(AnchorType::End)
            ]
        );
    }

    #[test]
    fn test_alternation() {
        let tokens = tokenize_all("a|b|c").unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::Char('a'),
                Token::Alternation,
                Token::Char('b'),
                Token::Alternation,
                Token::Char('c')
            ]
        );
    }

    #[test]
    fn test_escaped_literals() {
        let tokens = tokenize_all(r"\(\)\[\]").unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::Char('('),
                Token::Char(')'),
                Token::Char('['),
                Token::Char(']')
            ]
        );
    }
}
