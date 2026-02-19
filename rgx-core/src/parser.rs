//! Regex pattern parser
//!
//! This module implements a recursive descent parser that converts tokens
//! from the lexer into an Abstract Syntax Tree (AST).

use crate::ast::{Regex, Quantifier, GroupKind, CharClass};
use crate::lexer::Lexer;
use crate::token::{LexError, Token, TokenWithPos};

/// Parser for regex patterns
pub struct Parser<'a> {
    lexer: Lexer<'a>,
    current_token: Option<TokenWithPos>,
}

impl<'a> Parser<'a> {
    /// Create a new parser for the given input
    pub fn new(input: &'a str) -> Result<Self, LexError> {
        let mut lexer = Lexer::new(input);
        let current_token = Some(lexer.next_token()?);
        
        Ok(Self {
            lexer,
            current_token,
        })
    }

    /// Get the current token without consuming it
    fn peek(&self) -> Option<&Token> {
        self.current_token.as_ref().map(|t| &t.token)
    }

    /// Consume the current token and advance to the next
    fn advance(&mut self) -> Result<Option<TokenWithPos>, LexError> {
        let current = self.current_token.take();
        
        if let Some(ref token) = current {
            if token.token != Token::EOF {
                self.current_token = Some(self.lexer.next_token()?);
            }
        }
        
        Ok(current)
    }

    /// Parse the entire regex pattern
    pub fn parse(&mut self) -> Result<Regex, LexError> {
        let result = self.parse_alternation()?;
        
        // Ensure we've consumed all tokens
        if let Some(token) = &self.current_token {
            if token.token != Token::EOF {
                return Err(LexError::UnexpectedEOF {
                    expected: "end of input".to_string(),
                    position: token.position.clone(),
                });
            }
        }
        
        Ok(result)
    }

    /// Parse alternation: expr | expr | expr
    fn parse_alternation(&mut self) -> Result<Regex, LexError> {
        let mut alternatives = vec![self.parse_sequence()?];

        while matches!(self.peek(), Some(Token::Alternation)) {
            self.advance()?; // consume '|'
            alternatives.push(self.parse_sequence()?);
        }

        if alternatives.len() == 1 {
            Ok(alternatives.into_iter().next().unwrap())
        } else {
            Ok(Regex::Alternation(alternatives))
        }
    }

    /// Parse sequence: expr expr expr
    fn parse_sequence(&mut self) -> Result<Regex, LexError> {
        let mut elements = Vec::new();

        while let Some(token) = self.peek() {
            match token {
                Token::EOF | Token::Alternation | Token::GroupEnd => break,
                _ => {
                    elements.push(self.parse_quantified()?);
                }
            }
        }

        match elements.len() {
            0 => Ok(Regex::Empty),
            1 => Ok(elements.into_iter().next().unwrap()),
            _ => Ok(Regex::Sequence(elements)),
        }
    }

    /// Parse quantified expression: expr?, expr*, expr+, expr{n,m}
    fn parse_quantified(&mut self) -> Result<Regex, LexError> {
        let expr = self.parse_atom()?;

        let quantifier = match self.peek() {
            Some(Token::Question) => {
                self.advance()?;
                Some(Quantifier::ZeroOrOne { lazy: false })
            }
            Some(Token::QuestionLazy) => {
                self.advance()?;
                Some(Quantifier::ZeroOrOne { lazy: true })
            }
            Some(Token::Star) => {
                self.advance()?;
                Some(Quantifier::ZeroOrMore { lazy: false })
            }
            Some(Token::StarLazy) => {
                self.advance()?;
                Some(Quantifier::ZeroOrMore { lazy: true })
            }
            Some(Token::Plus) => {
                self.advance()?;
                Some(Quantifier::OneOrMore { lazy: false })
            }
            Some(Token::PlusLazy) => {
                self.advance()?;
                Some(Quantifier::OneOrMore { lazy: true })
            }
            Some(Token::Repeat { min, max, lazy }) => {
                let min = *min;
                let max = *max;
                let lazy = *lazy;
                self.advance()?;
                Some(Quantifier::Range { min, max, lazy })
            }
            _ => None,
        };

        if let Some(q) = quantifier {
            Ok(Regex::Quantified {
                expr: Box::new(expr),
                quantifier: q,
            })
        } else {
            Ok(expr)
        }
    }

    /// Parse atomic expression: literals, groups, character classes, etc.
    fn parse_atom(&mut self) -> Result<Regex, LexError> {
        match self.peek() {
            Some(Token::Char(c)) => {
                let c = *c;
                self.advance()?;
                Ok(Regex::Char(c))
            }
            
            Some(Token::Dot) => {
                self.advance()?;
                Ok(Regex::Dot)
            }
            
            Some(Token::Anchor(anchor_type)) => {
                let anchor_type = *anchor_type;
                self.advance()?;
                Ok(Regex::Anchor(anchor_type))
            }
            
            Some(Token::Digit) => {
                self.advance()?;
                Ok(Regex::CharClass(CharClass::Digit { negated: false }))
            }
            
            Some(Token::DigitNeg) => {
                self.advance()?;
                Ok(Regex::CharClass(CharClass::Digit { negated: true }))
            }
            
            Some(Token::Word) => {
                self.advance()?;
                Ok(Regex::CharClass(CharClass::Word { negated: false }))
            }
            
            Some(Token::WordNeg) => {
                self.advance()?;
                Ok(Regex::CharClass(CharClass::Word { negated: true }))
            }
            
            Some(Token::Space) => {
                self.advance()?;
                Ok(Regex::CharClass(CharClass::Space { negated: false }))
            }
            
            Some(Token::SpaceNeg) => {
                self.advance()?;
                Ok(Regex::CharClass(CharClass::Space { negated: true }))
            }
            
            Some(Token::WordBoundary) => {
                self.advance()?;
                Ok(Regex::WordBoundary { positive: true })
            }
            
            Some(Token::WordBoundaryNeg) => {
                self.advance()?;
                Ok(Regex::WordBoundary { positive: false })
            }
            
            Some(Token::CharClass { ranges, negated }) => {
                let ranges = ranges.clone();
                let negated = *negated;
                self.advance()?;
                Ok(Regex::CharClass(CharClass::Custom { ranges, negated }))
            }
            
            Some(Token::UnicodeClass { name }) => {
                let name = name.clone();
                self.advance()?;
                Ok(Regex::CharClass(CharClass::UnicodeClass { name, negated: false }))
            }
            
            Some(Token::UnicodeClassNeg { name }) => {
                let name = name.clone();
                self.advance()?;
                Ok(Regex::CharClass(CharClass::UnicodeClass { name, negated: true }))
            }
            
            Some(Token::Backreference(n)) => {
                let n = *n;
                self.advance()?;
                Ok(Regex::Backreference(n))
            }

            Some(Token::CodeBlock { lang, code }) => {
                let lang = lang.clone();
                let code = code.clone();
                self.advance()?;
                Ok(Regex::CodeBlock { lang, code })
            }

            Some(Token::Recursion { target }) => {
                let target = target.clone();
                self.advance()?;
                Ok(Regex::Recursion { target })
            }
            
            Some(Token::GroupStart) => {
                self.advance()?; // consume '('
                let expr = self.parse_alternation()?;
                
                // Expect closing ')'
                match self.peek() {
                    Some(Token::GroupEnd) => {
                        self.advance()?; // consume ')'
                        Ok(Regex::Group {
                            expr: Box::new(expr),
                            kind: GroupKind::Capturing,
                            index: None, // Will be assigned during compilation
                            name: None,
                        })
                    }
                    _ => Err(LexError::UnexpectedEOF {
                        expected: "closing parenthesis ')'".to_string(),
                        position: self.current_token.as_ref()
                            .map(|t| t.position.clone())
                            .unwrap_or_else(|| crate::token::Position::start()),
                    })
                }
            }
            
            Some(Token::NonCapturingGroupStart) => {
                self.advance()?; // consume '(?:'
                let expr = self.parse_alternation()?;
                
                // Expect closing ')'
                match self.peek() {
                    Some(Token::GroupEnd) => {
                        self.advance()?; // consume ')'
                        Ok(Regex::Group {
                            expr: Box::new(expr),
                            kind: GroupKind::NonCapturing,
                            index: None,
                            name: None,
                        })
                    }
                    _ => Err(LexError::UnexpectedEOF {
                        expected: "closing parenthesis ')'".to_string(),
                        position: self.current_token.as_ref()
                            .map(|t| t.position.clone())
                            .unwrap_or_else(|| crate::token::Position::start()),
                    })
                }
            }
            
            Some(Token::NamedGroupStart { name }) => {
                let name = name.clone();
                self.advance()?; // consume '(?<name>'
                let expr = self.parse_alternation()?;
                
                // Expect closing ')'
                match self.peek() {
                    Some(Token::GroupEnd) => {
                        self.advance()?; // consume ')'
                        Ok(Regex::Group {
                            expr: Box::new(expr),
                            kind: GroupKind::Capturing,
                            index: None,
                            name: Some(name),
                        })
                    }
                    _ => Err(LexError::UnexpectedEOF {
                        expected: "closing parenthesis ')'".to_string(),
                        position: self.current_token.as_ref()
                            .map(|t| t.position.clone())
                            .unwrap_or_else(|| crate::token::Position::start()),
                    })
                }
            }

            Some(Token::AtomicGroupStart) => {
                self.advance()?; // consume '(?>'
                let expr = self.parse_alternation()?;

                // Expect closing ')'
                match self.peek() {
                    Some(Token::GroupEnd) => {
                        self.advance()?; // consume ')'
                        Ok(Regex::Group {
                            expr: Box::new(expr),
                            kind: GroupKind::Atomic,
                            index: None,
                            name: None,
                        })
                    }
                    _ => Err(LexError::UnexpectedEOF {
                        expected: "closing parenthesis ')'".to_string(),
                        position: self.current_token.as_ref()
                            .map(|t| t.position.clone())
                            .unwrap_or_else(|| crate::token::Position::start()),
                    })
                }
            }

            Some(Token::LookaheadPos) => {
                self.advance()?; // consume '(?='
                let expr = self.parse_alternation()?;

                // Expect closing ')'
                match self.peek() {
                    Some(Token::GroupEnd) => {
                        self.advance()?; // consume ')'
                        Ok(Regex::Lookahead {
                            expr: Box::new(expr),
                            positive: true,
                        })
                    }
                    _ => Err(LexError::UnexpectedEOF {
                        expected: "closing parenthesis ')'".to_string(),
                        position: self.current_token.as_ref()
                            .map(|t| t.position.clone())
                            .unwrap_or_else(|| crate::token::Position::start()),
                    })
                }
            }

            Some(Token::LookaheadNeg) => {
                self.advance()?; // consume '(?!'
                let expr = self.parse_alternation()?;

                // Expect closing ')'
                match self.peek() {
                    Some(Token::GroupEnd) => {
                        self.advance()?; // consume ')'
                        Ok(Regex::Lookahead {
                            expr: Box::new(expr),
                            positive: false,
                        })
                    }
                    _ => Err(LexError::UnexpectedEOF {
                        expected: "closing parenthesis ')'".to_string(),
                        position: self.current_token.as_ref()
                            .map(|t| t.position.clone())
                            .unwrap_or_else(|| crate::token::Position::start()),
                    })
                }
            }

            Some(Token::LookbehindPos) => {
                self.advance()?; // consume '(?<='
                let expr = self.parse_alternation()?;

                // Expect closing ')'
                match self.peek() {
                    Some(Token::GroupEnd) => {
                        self.advance()?; // consume ')'
                        Ok(Regex::Lookbehind {
                            expr: Box::new(expr),
                            positive: true,
                        })
                    }
                    _ => Err(LexError::UnexpectedEOF {
                        expected: "closing parenthesis ')'".to_string(),
                        position: self.current_token.as_ref()
                            .map(|t| t.position.clone())
                            .unwrap_or_else(|| crate::token::Position::start()),
                    })
                }
            }

            Some(Token::LookbehindNeg) => {
                self.advance()?; // consume '(?<!'
                let expr = self.parse_alternation()?;

                // Expect closing ')'
                match self.peek() {
                    Some(Token::GroupEnd) => {
                        self.advance()?; // consume ')'
                        Ok(Regex::Lookbehind {
                            expr: Box::new(expr),
                            positive: false,
                        })
                    }
                    _ => Err(LexError::UnexpectedEOF {
                        expected: "closing parenthesis ')'".to_string(),
                        position: self.current_token.as_ref()
                            .map(|t| t.position.clone())
                            .unwrap_or_else(|| crate::token::Position::start()),
                    })
                }
            }
            
            Some(Token::EOF) => {
                Err(LexError::UnexpectedEOF {
                    expected: "regex expression".to_string(),
                    position: self.current_token.as_ref()
                        .map(|t| t.position.clone())
                        .unwrap_or_else(|| crate::token::Position::start()),
                })
            }
            
            Some(other) => {
                Err(LexError::UnexpectedEOF {
                    expected: format!("unexpected token: {:?}", other),
                    position: self.current_token.as_ref()
                        .map(|t| t.position.clone())
                        .unwrap_or_else(|| crate::token::Position::start()),
                })
            }
            
            None => {
                Err(LexError::UnexpectedEOF {
                    expected: "regex expression".to_string(),
                    position: crate::token::Position::start(),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_literal() {
        let mut parser = Parser::new("abc").unwrap();
        let ast = parser.parse().unwrap();
        
        match ast {
            Regex::Sequence(elements) => {
                assert_eq!(elements.len(), 3);
                assert!(matches!(elements[0], Regex::Char('a')));
                assert!(matches!(elements[1], Regex::Char('b')));
                assert!(matches!(elements[2], Regex::Char('c')));
            }
            _ => panic!("Expected sequence")
        }
    }

    #[test]
    fn test_parse_quantified() {
        let mut parser = Parser::new("a*").unwrap();
        let ast = parser.parse().unwrap();
        
        match ast {
            Regex::Quantified { expr, quantifier } => {
                assert!(matches!(*expr, Regex::Char('a')));
                assert!(matches!(quantifier, Quantifier::ZeroOrMore { lazy: false }));
            }
            _ => panic!("Expected quantified")
        }
    }

    #[test]
    fn test_parse_alternation() {
        let mut parser = Parser::new("a|b").unwrap();
        let ast = parser.parse().unwrap();
        
        match ast {
            Regex::Alternation(alternatives) => {
                assert_eq!(alternatives.len(), 2);
                assert!(matches!(alternatives[0], Regex::Char('a')));
                assert!(matches!(alternatives[1], Regex::Char('b')));
            }
            _ => panic!("Expected alternation")
        }
    }

    #[test]
    fn test_parse_group() {
        let mut parser = Parser::new("(abc)").unwrap();
        let ast = parser.parse().unwrap();
        
        match ast {
            Regex::Group { expr, kind, .. } => {
                assert!(matches!(kind, GroupKind::Capturing));
                match *expr {
                    Regex::Sequence(elements) => {
                        assert_eq!(elements.len(), 3);
                        assert!(matches!(elements[0], Regex::Char('a')));
                    }
                    _ => panic!("Expected sequence inside group")
                }
            }
            _ => panic!("Expected group")
        }
    }
    
    #[test]
    fn test_parse_non_capturing_group() {
        let mut parser = Parser::new("(?:abc)").unwrap();
        let ast = parser.parse().unwrap();
        
        match ast {
            Regex::Group { expr, kind, name, .. } => {
                assert!(matches!(kind, GroupKind::NonCapturing));
                assert_eq!(name, None);
                match *expr {
                    Regex::Sequence(elements) => {
                        assert_eq!(elements.len(), 3);
                        assert!(matches!(elements[0], Regex::Char('a')));
                    }
                    _ => panic!("Expected sequence inside non-capturing group")
                }
            }
            _ => panic!("Expected non-capturing group")
        }
    }
    
    #[test]
    fn test_parse_named_group() {
        let mut parser = Parser::new("(?<word>abc)").unwrap();
        let ast = parser.parse().unwrap();
        
        match ast {
            Regex::Group { expr, kind, name, .. } => {
                assert!(matches!(kind, GroupKind::Capturing));
                assert_eq!(name, Some("word".to_string()));
                match *expr {
                    Regex::Sequence(elements) => {
                        assert_eq!(elements.len(), 3);
                        assert!(matches!(elements[0], Regex::Char('a')));
                    }
                    _ => panic!("Expected sequence inside named group")
                }
            }
            _ => panic!("Expected named capturing group")
        }
    }

    #[test]
    fn test_parse_atomic_group() {
        let mut parser = Parser::new("(?>ab)").unwrap();
        let ast = parser.parse().unwrap();

        match ast {
            Regex::Group { kind, .. } => {
                assert!(matches!(kind, GroupKind::Atomic));
            }
            _ => panic!("Expected atomic group"),
        }
    }

    #[test]
    fn test_parse_positive_lookahead() {
        let mut parser = Parser::new("(?=ab)c").unwrap();
        let ast = parser.parse().unwrap();

        match ast {
            Regex::Sequence(elements) => {
                assert_eq!(elements.len(), 2);
                match &elements[0] {
                    Regex::Lookahead { positive, .. } => assert!(*positive),
                    _ => panic!("Expected positive lookahead"),
                }
                assert!(matches!(elements[1], Regex::Char('c')));
            }
            _ => panic!("Expected sequence"),
        }
    }

    #[test]
    fn test_parse_negative_lookbehind() {
        let mut parser = Parser::new("(?<!x)a").unwrap();
        let ast = parser.parse().unwrap();

        match ast {
            Regex::Sequence(elements) => {
                assert_eq!(elements.len(), 2);
                match &elements[0] {
                    Regex::Lookbehind { positive, .. } => assert!(!*positive),
                    _ => panic!("Expected negative lookbehind"),
                }
                assert!(matches!(elements[1], Regex::Char('a')));
            }
            _ => panic!("Expected sequence"),
        }
    }

    #[test]
    fn test_parse_code_block() {
        let mut parser = Parser::new("(?{lua:return true})").unwrap();
        let ast = parser.parse().unwrap();

        match ast {
            Regex::CodeBlock { lang, code } => {
                assert_eq!(lang, "lua");
                assert_eq!(code, "return true");
            }
            _ => panic!("Expected code block"),
        }
    }

    #[test]
    fn test_parse_recursion_entire_pattern() {
        let mut parser = Parser::new("(?R)").unwrap();
        let ast = parser.parse().unwrap();

        match ast {
            Regex::Recursion { target } => {
                assert!(matches!(target, crate::ast::RecursionTarget::Entire));
            }
            _ => panic!("Expected recursion node"),
        }
    }

    #[test]
    fn test_parse_recursion_named_group() {
        let mut parser = Parser::new("(?&word)").unwrap();
        let ast = parser.parse().unwrap();

        match ast {
            Regex::Recursion { target } => {
                assert_eq!(target, crate::ast::RecursionTarget::NamedGroup("word".to_string()));
            }
            _ => panic!("Expected recursion node"),
        }
    }

    #[test]
    fn test_parse_character_class() {
        let mut parser = Parser::new("\\d").unwrap();
        let ast = parser.parse().unwrap();
        
        match ast {
            Regex::CharClass(CharClass::Digit { negated }) => {
                assert!(!negated);
            }
            _ => panic!("Expected digit character class")
        }
    }
}
