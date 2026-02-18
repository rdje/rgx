//! Lexical analyzer for regex patterns
//!
//! This module implements a lexer that converts regex pattern strings into
//! a stream of tokens, handling all Perl regex features including our custom
//! code execution blocks.

use crate::ast::{AnchorType, CharRange};
use crate::token::{LexError, Position, Token, TokenWithPos};
use std::str::Chars;

/// Regex lexer that converts pattern strings to tokens
pub struct Lexer<'a> {
    /// Original input string
    input: &'a str,
    /// Character iterator
    chars: Chars<'a>,
    /// Current character being processed
    current: Option<char>,
    /// Current position in input
    position: Position,
}

impl<'a> Lexer<'a> {
    /// Create a new lexer for the given input
    pub fn new(input: &'a str) -> Self {
        let mut lexer = Self {
            input,
            chars: input.chars(),
            current: None,
            position: Position::start(),
        };
        lexer.advance(); // Prime the first character
        lexer
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
        self.position.clone()
    }

    /// Get the next token from the input
    pub fn next_token(&mut self) -> Result<TokenWithPos, LexError> {
        let start_pos = self.current_position();

        let token = match self.current {
            None => Token::EOF,
            
            Some('\\') => self.parse_escape()?,
            Some('.') => { self.advance(); Token::Dot }
            Some('^') => { self.advance(); Token::Anchor(AnchorType::Start) }
            Some('$') => { self.advance(); Token::Anchor(AnchorType::End) }
            Some('*') => self.parse_star()?,
            Some('+') => self.parse_plus()?,
            Some('?') => self.parse_question()?,
            Some('|') => { self.advance(); Token::Alternation }
            
            Some('(') => self.parse_group()?,
            Some(')') => { self.advance(); Token::GroupEnd }
            
            Some('[') => self.parse_character_class()?,
            Some('{') => self.parse_repeat_quantifier()?,
            
            Some(c) => {
                self.advance();
                Token::Char(c)
            }
        };

        Ok(TokenWithPos::new(token, start_pos))
    }

    /// Parse escape sequences like \d, \w, \n, etc.
    fn parse_escape(&mut self) -> Result<Token, LexError> {
        let start_pos = self.current_position();
        self.advance(); // Skip the backslash

        match self.current {
            None => Err(LexError::UnexpectedEOF {
                expected: "escape sequence".to_string(),
                position: start_pos,
            }),

            // Predefined character classes
            Some('d') => { self.advance(); Ok(Token::Digit) }
            Some('D') => { self.advance(); Ok(Token::DigitNeg) }
            Some('w') => { self.advance(); Ok(Token::Word) }
            Some('W') => { self.advance(); Ok(Token::WordNeg) }
            Some('s') => { self.advance(); Ok(Token::Space) }
            Some('S') => { self.advance(); Ok(Token::SpaceNeg) }
            Some('b') => { self.advance(); Ok(Token::WordBoundary) }
            Some('B') => { self.advance(); Ok(Token::WordBoundaryNeg) }

            // Anchors
            Some('A') => { self.advance(); Ok(Token::Anchor(AnchorType::AbsStart)) }
            Some('Z') => { self.advance(); Ok(Token::Anchor(AnchorType::AbsEnd)) }
            Some('z') => { self.advance(); Ok(Token::Anchor(AnchorType::AbsEndNoNL)) }

            // Backreferences
            Some(c) if c.is_ascii_digit() => self.parse_backreference(),

            // Unicode property classes
            Some('p') => self.parse_unicode_class(false),
            Some('P') => self.parse_unicode_class(true),

            // Literal escape sequences
            Some('n') => { self.advance(); Ok(Token::Char('\n')) }
            Some('t') => { self.advance(); Ok(Token::Char('\t')) }
            Some('r') => { self.advance(); Ok(Token::Char('\r')) }
            Some('f') => { self.advance(); Ok(Token::Char('\u{0C}')) }
            Some('a') => { self.advance(); Ok(Token::Char('\u{07}')) }
            Some('e') => { self.advance(); Ok(Token::Char('\u{1B}')) }

            // Escaped metacharacters
            Some('.' | '^' | '$' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '\\') => {
                let ch = self.current.unwrap();
                self.advance();
                Ok(Token::Char(ch))
            }

            // Hex escapes \x{...} or \xFF
            Some('x') => self.parse_hex_escape(),

            // Octal escapes \777
            Some(c) if c.is_ascii_digit() => self.parse_octal_escape(),

            Some(c) => {
                let sequence = format!("\\{}", c);
                Err(LexError::InvalidEscape {
                    sequence,
                    position: start_pos,
                })
            }
        }
    }

    /// Parse Unicode property class \p{Name} or \P{Name}
    fn parse_unicode_class(&mut self, negated: bool) -> Result<Token, LexError> {
        let start_pos = self.current_position();
        self.advance(); // Skip 'p' or 'P'

        if self.current != Some('{') {
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
            return Err(LexError::InvalidUnicodeClass {
                name: "empty class name".to_string(),
                position: start_pos,
            });
        }

        Ok(if negated {
            Token::UnicodeClassNeg { name }
        } else {
            Token::UnicodeClass { name }
        })
    }

    /// Parse backreference \1, \2, etc.
    fn parse_backreference(&mut self) -> Result<Token, LexError> {
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

        let number: u32 = number_str.parse().map_err(|_| LexError::InvalidBackreference {
            number: number_str.clone(),
            position: start_pos,
        })?;

        if number == 0 || number > 99 {
            return Err(LexError::InvalidBackreference {
                number: number_str,
                position: start_pos,
            });
        }

        Ok(Token::Backreference(number))
    }

    /// Parse hex escape \x{...} or \xFF  
    fn parse_hex_escape(&mut self) -> Result<Token, LexError> {
        let start_pos = self.current_position();
        self.advance(); // Skip 'x'

        let hex_digits = if self.current == Some('{') {
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
                    return Err(LexError::InvalidEscape {
                        sequence: format!("\\x{{{}", digits),
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
            return Err(LexError::InvalidEscape {
                sequence: "\\x".to_string(),
                position: start_pos,
            });
        }

        let code_point = u32::from_str_radix(&hex_digits, 16).map_err(|_| {
            LexError::InvalidEscape {
                sequence: format!("\\x{}", hex_digits),
                position: start_pos,
            }
        })?;

        let ch = char::from_u32(code_point).ok_or_else(|| LexError::InvalidEscape {
            sequence: format!("\\x{}", hex_digits),
            position: start_pos,
        })?;

        Ok(Token::Char(ch))
    }

    /// Parse octal escape \777
    fn parse_octal_escape(&mut self) -> Result<Token, LexError> {
        let start_pos = self.current_position();
        let mut octal_digits = String::new();

        // Collect up to 3 octal digits
        for _ in 0..3 {
            if let Some(c) = self.current {
                if c >= '0' && c <= '7' {
                    octal_digits.push(c);
                    self.advance();
                } else {
                    break;
                }
            }
        }

        if octal_digits.is_empty() {
            return Err(LexError::InvalidEscape {
                sequence: "\\".to_string(),
                position: start_pos,
            });
        }

        let code_point = u32::from_str_radix(&octal_digits, 8).map_err(|_| {
            LexError::InvalidEscape {
                sequence: format!("\\{}", octal_digits),
                position: start_pos,
            }
        })?;

        // Octal escapes are limited to byte values (0-255)
        if code_point > 255 {
            return Err(LexError::InvalidEscape {
                sequence: format!("\\{}", octal_digits),
                position: start_pos,
            });
        }

        let ch = char::from_u32(code_point).ok_or_else(|| LexError::InvalidEscape {
            sequence: format!("\\{}", octal_digits),
            position: start_pos,
        })?;

        Ok(Token::Char(ch))
    }

    /// Parse * and *? quantifiers
    fn parse_star(&mut self) -> Result<Token, LexError> {
        self.advance(); // Skip '*'
        if self.current == Some('?') {
            self.advance(); // Skip '?'
            Ok(Token::StarLazy)
        } else {
            Ok(Token::Star)
        }
    }

    /// Parse + and +? quantifiers
    fn parse_plus(&mut self) -> Result<Token, LexError> {
        self.advance(); // Skip '+'
        if self.current == Some('?') {
            self.advance(); // Skip '?'
            Ok(Token::PlusLazy)
        } else {
            Ok(Token::Plus)
        }
    }

    /// Parse ? and ?? quantifiers
    fn parse_question(&mut self) -> Result<Token, LexError> {
        self.advance(); // Skip '?'
        if self.current == Some('?') {
            self.advance(); // Skip second '?'
            Ok(Token::QuestionLazy)
        } else {
            Ok(Token::Question)
        }
    }

    /// Parse group constructs: (...), (?:...), (?<name>...)
    fn parse_group(&mut self) -> Result<Token, LexError> {
        let start_pos = self.current_position();
        self.advance(); // Skip '('
        
        // Simple capturing group
        if self.current != Some('?') {
            return Ok(Token::GroupStart);
        }
        
        self.advance(); // Skip '?'
        
        match self.current {
            Some(':') => {
                self.advance(); // Skip ':'
                Ok(Token::NonCapturingGroupStart)
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
                
                Ok(Token::NamedGroupStart { name })
            }
            _ => Err(LexError::InvalidGroupSyntax {
                position: start_pos,
            }),
        }
    }


    /// Parse character class [abc], [^abc], [a-z], etc.
    fn parse_character_class(&mut self) -> Result<Token, LexError> {
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
                            return Err(LexError::InvalidCharRange {
                                start: start_char,
                                end: end_char,
                                position: self.current_position(),
                            });
                        }
                        ranges.push(CharRange::range(start_char, end_char));
                        self.advance();
                    } else {
                        return Err(LexError::UnterminatedCharClass { position: start_pos });
                    }
                } else {
                    // Single character
                    ranges.push(CharRange::single(start_char));
                }
            }
        }

        if ranges.is_empty() {
            return Err(LexError::UnterminatedCharClass { position: start_pos });
        }

        Ok(Token::CharClass { ranges, negated })
    }

    /// Parse repeat quantifier {n}, {n,}, {n,m}, {n,m}?
    fn parse_repeat_quantifier(&mut self) -> Result<Token, LexError> {
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
                return Err(LexError::InvalidRepeat {
                    text: content,
                    position: start_pos,
                });
            }
        };

        // Check for lazy quantifier
        let lazy = if self.current == Some('?') {
            self.advance(); // Skip '?'
            true
        } else {
            false
        };

        Ok(Token::Repeat { min, max, lazy })
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
        assert_eq!(tokens, vec![
            Token::Char('a'),
            Token::Char('b'), 
            Token::Char('c')
        ]);
    }

    #[test]
    fn test_quantifiers() {
        let tokens = tokenize_all("a*b+c?").unwrap();
        assert_eq!(tokens, vec![
            Token::Char('a'),
            Token::Star,
            Token::Char('b'),
            Token::Plus,
            Token::Char('c'),
            Token::Question
        ]);
    }

    #[test]
    fn test_lazy_quantifiers() {
        let tokens = tokenize_all("a*?b+?c??").unwrap();
        assert_eq!(tokens, vec![
            Token::Char('a'),
            Token::StarLazy,
            Token::Char('b'),
            Token::PlusLazy,
            Token::Char('c'),
            Token::QuestionLazy
        ]);
    }

    #[test]
    fn test_character_classes() {
        let tokens = tokenize_all(r"\d\w\s").unwrap();
        assert_eq!(tokens, vec![
            Token::Digit,
            Token::Word,
            Token::Space
        ]);
    }

    #[test]
    fn test_basic_groups() {
        let tokens = tokenize_all("(abc)").unwrap();
        assert_eq!(tokens, vec![
            Token::GroupStart,
            Token::Char('a'),
            Token::Char('b'),
            Token::Char('c'),
            Token::GroupEnd
        ]);
    }
    
    #[test]
    fn test_non_capturing_groups() {
        let tokens = tokenize_all("(?:ab)").unwrap();
        assert_eq!(tokens, vec![
            Token::NonCapturingGroupStart,
            Token::Char('a'),
            Token::Char('b'),
            Token::GroupEnd,
        ]);
    }
    
    #[test]
    fn test_named_groups() {
        let tokens = tokenize_all("(?<word>ab)").unwrap();
        assert_eq!(tokens, vec![
            Token::NamedGroupStart { name: "word".to_string() },
            Token::Char('a'),
            Token::Char('b'),
            Token::GroupEnd,
        ]);
    }

    #[test]
    fn test_character_class() {
        let tokens = tokenize_all("[abc]").unwrap();
        assert_eq!(tokens.len(), 1);
        match &tokens[0] {
            Token::CharClass { ranges, negated } => {
                assert!(!negated);
                assert_eq!(ranges.len(), 3);
            },
            _ => panic!("Expected CharClass token")
        }
    }

    #[test]
    fn test_repeat_quantifier() {
        let tokens = tokenize_all("a{2,5}").unwrap();
        assert_eq!(tokens, vec![
            Token::Char('a'),
            Token::Repeat { min: 2, max: Some(5), lazy: false }
        ]);
    }

    #[test]
    fn test_anchors() {
        let tokens = tokenize_all("^abc$").unwrap();
        assert_eq!(tokens, vec![
            Token::Anchor(AnchorType::Start),
            Token::Char('a'),
            Token::Char('b'),
            Token::Char('c'),
            Token::Anchor(AnchorType::End)
        ]);
    }

    #[test]
    fn test_alternation() {
        let tokens = tokenize_all("a|b|c").unwrap();
        assert_eq!(tokens, vec![
            Token::Char('a'),
            Token::Alternation,
            Token::Char('b'),
            Token::Alternation,
            Token::Char('c')
        ]);
    }

    #[test]
    fn test_escaped_literals() {
        let tokens = tokenize_all(r"\(\)\[\]").unwrap();
        assert_eq!(tokens, vec![
            Token::Char('('),
            Token::Char(')'),
            Token::Char('['),
            Token::Char(']')
        ]);
    }
}
