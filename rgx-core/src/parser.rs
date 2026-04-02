//! Regex pattern parser
//!
//! This module implements a recursive descent parser that converts tokens
//! from the lexer into an Abstract Syntax Tree (AST).

use crate::ast::{CharClass, GroupKind, Quantifier, Regex};
use crate::lexer::Lexer;
use crate::token::{LexError, Token, TokenWithPos};
use crate::{trace_decision, trace_enter, trace_exit, trace_log};

/// Parser for regex patterns
pub struct Parser<'a> {
    lexer: Lexer<'a>,
    current_token: Option<TokenWithPos>,
}

impl<'a> Parser<'a> {
    fn wrap_quantified(expr: Regex, quantifier: Quantifier, possessive: bool) -> Regex {
        let quantified_expr = Regex::Quantified {
            expr: Box::new(expr),
            quantifier,
        };

        if possessive {
            Regex::Group {
                expr: Box::new(quantified_expr),
                kind: GroupKind::Atomic,
                index: None,
                name: None,
            }
        } else {
            quantified_expr
        }
    }

    /// Create a new parser for the given input
    pub fn new(input: &'a str) -> Result<Self, LexError> {
        trace_enter!("parser", "Parser::new", "input_len={}", input.len());
        let mut lexer = Lexer::new(input);
        let current_token = match lexer.next_token() {
            Ok(token) => Some(token),
            Err(err) => {
                trace_exit!("parser", "Parser::new", "ok=false,error={}", err);
                return Err(err);
            }
        };

        let initial_token = current_token
            .as_ref()
            .map(|token| format!("{:?}", token.token))
            .unwrap_or_else(|| "<none>".to_string());

        let parser = Self {
            lexer,
            current_token,
        };
        trace_exit!(
            "parser",
            "Parser::new",
            "ok=true,initial_token={}",
            initial_token
        );
        Ok(parser)
    }

    fn current_token_snapshot(&self) -> String {
        trace_enter!(
            "parser",
            "Parser::current_token_snapshot",
            "has_current_token={}",
            self.current_token.is_some()
        );
        let snapshot = self
            .peek()
            .map(|token| format!("{token:?}"))
            .unwrap_or_else(|| "<none>".to_string());
        trace_exit!(
            "parser",
            "Parser::current_token_snapshot",
            "ok=true,snapshot={}",
            snapshot
        );
        snapshot
    }

    fn regex_kind(node: &Regex) -> &'static str {
        trace_enter!("parser", "Parser::regex_kind");
        let kind = match node {
            Regex::Empty => "Empty",
            Regex::Char(_) => "Char",
            Regex::Dot => "Dot",
            Regex::CharClass(_) => "CharClass",
            Regex::Digit { .. } => "Digit",
            Regex::Word { .. } => "Word",
            Regex::Space { .. } => "Space",
            Regex::UnicodeClass { .. } => "UnicodeClass",
            Regex::ExtendedCharClass { .. } => "ExtendedCharClass",
            Regex::Anchor(_) => "Anchor",
            Regex::WordBoundary { .. } => "WordBoundary",
            Regex::Sequence(_) => "Sequence",
            Regex::Alternation(_) => "Alternation",
            Regex::Quantified { .. } => "Quantified",
            Regex::Group { .. } => "Group",
            Regex::Backreference(_) => "Backreference",
            Regex::Lookahead { .. } => "Lookahead",
            Regex::Lookbehind { .. } => "Lookbehind",
            Regex::CodeBlock { .. } => "CodeBlock",
            Regex::Conditional { .. } => "Conditional",
            Regex::Recursion { .. } => "Recursion",
        };
        trace_exit!("parser", "Parser::regex_kind", "ok=true,kind={}", kind);
        kind
    }

    /// Get the current token without consuming it
    fn peek(&self) -> Option<&Token> {
        trace_enter!(
            "parser",
            "Parser::peek",
            "has_current_token={}",
            self.current_token.is_some()
        );
        let token = self.current_token.as_ref().map(|t| &t.token);
        trace_decision!(
            "parser",
            "token.is_some()",
            token.is_some(),
            "peek current-token availability"
        );
        let token_snapshot = token
            .map(|current| format!("{current:?}"))
            .unwrap_or_else(|| "<none>".to_string());
        trace_exit!("parser", "Parser::peek", "ok=true,token={}", token_snapshot);
        token
    }

    /// Consume the current token and advance to the next
    fn advance(&mut self) -> Result<Option<TokenWithPos>, LexError> {
        trace_enter!(
            "parser",
            "Parser::advance",
            "current_token={}",
            self.current_token_snapshot()
        );
        let current = self.current_token.take();
        let consumed_token = current
            .as_ref()
            .map(|token| format!("{:?}", token.token))
            .unwrap_or_else(|| "<none>".to_string());
        let should_fetch_next = current
            .as_ref()
            .is_some_and(|token| token.token != Token::EOF);
        trace_decision!(
            "parser",
            "should_fetch_next",
            should_fetch_next,
            "fetch lexer token unless consumed token is EOF/None"
        );

        if should_fetch_next {
            self.current_token = match self.lexer.next_token() {
                Ok(token) => Some(token),
                Err(err) => {
                    trace_exit!("parser", "Parser::advance", "ok=false,error={}", err);
                    return Err(err);
                }
            };
        }

        let next_token = self
            .current_token
            .as_ref()
            .map(|token| format!("{:?}", token.token))
            .unwrap_or_else(|| "<none>".to_string());
        trace_exit!(
            "parser",
            "Parser::advance",
            "ok=true,consumed_token={},next_token={}",
            consumed_token,
            next_token
        );
        Ok(current)
    }

    /// Parse the entire regex pattern
    pub fn parse(&mut self) -> Result<Regex, LexError> {
        trace_enter!(
            "parser",
            "Parser::parse",
            "start_token={}",
            self.current_token_snapshot()
        );

        let result = match self.parse_alternation() {
            Ok(ast) => ast,
            Err(err) => {
                trace_exit!("parser", "Parser::parse", "ok=false,error={}", err);
                return Err(err);
            }
        };

        // Ensure we've consumed all tokens
        if let Some(token) = &self.current_token {
            if token.token != Token::EOF {
                trace_decision!(
                    "parser",
                    "post-parse token == EOF",
                    false,
                    "found trailing token {:?} at parser boundary",
                    token.token
                );
                trace_exit!(
                    "parser",
                    "Parser::parse",
                    "ok=false,error=trailing token {:?}",
                    token.token
                );
                return Err(LexError::UnexpectedEOF {
                    expected: "end of input".to_string(),
                    position: token.position.clone(),
                });
            }
        }
        trace_decision!(
            "parser",
            "post-parse token == EOF",
            true,
            "all tokens consumed successfully"
        );

        trace_exit!(
            "parser",
            "Parser::parse",
            "ok=true,node_kind={}",
            Self::regex_kind(&result)
        );
        Ok(result)
    }

    /// Parse alternation: expr | expr | expr
    fn parse_alternation(&mut self) -> Result<Regex, LexError> {
        trace_enter!(
            "parser",
            "Parser::parse_alternation",
            "token={}",
            self.current_token_snapshot()
        );
        let mut alternatives = match self.parse_sequence() {
            Ok(first) => vec![first],
            Err(err) => {
                trace_exit!(
                    "parser",
                    "Parser::parse_alternation",
                    "ok=false,error={}",
                    err
                );
                return Err(err);
            }
        };

        while matches!(self.peek(), Some(Token::Alternation)) {
            trace_decision!(
                "parser",
                "peek() == Token::Alternation",
                true,
                "consume alternation separator and parse next branch"
            );
            self.advance()?; // consume '|'
            let branch = match self.parse_sequence() {
                Ok(sequence) => sequence,
                Err(err) => {
                    trace_exit!(
                        "parser",
                        "Parser::parse_alternation",
                        "ok=false,error={}",
                        err
                    );
                    return Err(err);
                }
            };
            alternatives.push(branch);
            trace_log!(
                "parser",
                "parsed alternation branch count={}",
                alternatives.len()
            );
        }

        let alternation_present = alternatives.len() > 1;
        trace_decision!(
            "parser",
            "alternatives.len() > 1",
            alternation_present,
            "wrap into alternation node only when multiple branches exist"
        );
        let result = if alternation_present {
            Regex::Alternation(alternatives)
        } else {
            alternatives.into_iter().next().unwrap()
        };
        trace_exit!(
            "parser",
            "Parser::parse_alternation",
            "ok=true,node_kind={}",
            Self::regex_kind(&result)
        );
        Ok(result)
    }

    /// Parse sequence: expr expr expr
    fn parse_sequence(&mut self) -> Result<Regex, LexError> {
        trace_enter!(
            "parser",
            "Parser::parse_sequence",
            "token={}",
            self.current_token_snapshot()
        );
        let mut elements = Vec::new();

        while let Some(token) = self.peek() {
            match token {
                Token::EOF | Token::Alternation | Token::GroupEnd => break,
                _ => {
                    let element = match self.parse_quantified() {
                        Ok(node) => node,
                        Err(err) => {
                            trace_exit!(
                                "parser",
                                "Parser::parse_sequence",
                                "ok=false,error={}",
                                err
                            );
                            return Err(err);
                        }
                    };
                    elements.push(element);
                }
            }
        }

        let result = match elements.len() {
            0 => Regex::Empty,
            1 => elements.into_iter().next().unwrap(),
            _ => Regex::Sequence(elements),
        };
        trace_exit!(
            "parser",
            "Parser::parse_sequence",
            "ok=true,node_kind={}",
            Self::regex_kind(&result)
        );
        Ok(result)
    }

    /// Parse quantified expression: expr?, expr*, expr+, expr{n,m}
    fn parse_quantified(&mut self) -> Result<Regex, LexError> {
        trace_enter!(
            "parser",
            "Parser::parse_quantified",
            "token={}",
            self.current_token_snapshot()
        );
        let expr = match self.parse_atom() {
            Ok(node) => node,
            Err(err) => {
                trace_exit!(
                    "parser",
                    "Parser::parse_quantified",
                    "ok=false,error={}",
                    err
                );
                return Err(err);
            }
        };

        let quantifier = match self.peek() {
            Some(Token::Question) => {
                self.advance()?;
                Some((Quantifier::ZeroOrOne { lazy: false }, false))
            }
            Some(Token::QuestionLazy) => {
                self.advance()?;
                Some((Quantifier::ZeroOrOne { lazy: true }, false))
            }
            Some(Token::QuestionPossessive) => {
                self.advance()?;
                Some((Quantifier::ZeroOrOne { lazy: false }, true))
            }
            Some(Token::Star) => {
                self.advance()?;
                Some((Quantifier::ZeroOrMore { lazy: false }, false))
            }
            Some(Token::StarLazy) => {
                self.advance()?;
                Some((Quantifier::ZeroOrMore { lazy: true }, false))
            }
            Some(Token::StarPossessive) => {
                self.advance()?;
                Some((Quantifier::ZeroOrMore { lazy: false }, true))
            }
            Some(Token::Plus) => {
                self.advance()?;
                Some((Quantifier::OneOrMore { lazy: false }, false))
            }
            Some(Token::PlusLazy) => {
                self.advance()?;
                Some((Quantifier::OneOrMore { lazy: true }, false))
            }
            Some(Token::PlusPossessive) => {
                self.advance()?;
                Some((Quantifier::OneOrMore { lazy: false }, true))
            }
            Some(Token::Repeat {
                min,
                max,
                lazy,
                possessive,
            }) => {
                let min = *min;
                let max = *max;
                let lazy = *lazy;
                let possessive = *possessive;
                self.advance()?;
                Some((Quantifier::Range { min, max, lazy }, possessive))
            }
            _ => None,
        };

        let has_quantifier = quantifier.is_some();
        trace_decision!(
            "parser",
            "quantifier.is_some()",
            has_quantifier,
            "wrap atom into quantified AST node only when suffix quantifier is present"
        );
        let result = if let Some((q, possessive)) = quantifier {
            Self::wrap_quantified(expr, q, possessive)
        } else {
            expr
        };
        trace_exit!(
            "parser",
            "Parser::parse_quantified",
            "ok=true,node_kind={},quantified={}",
            Self::regex_kind(&result),
            has_quantifier
        );
        Ok(result)
    }

    /// Parse atomic expression: literals, groups, character classes, etc.
    fn parse_atom(&mut self) -> Result<Regex, LexError> {
        trace_enter!(
            "parser",
            "Parser::parse_atom",
            "token={}",
            self.current_token_snapshot()
        );
        let result = match self.peek() {
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
                Ok(Regex::CharClass(CharClass::UnicodeClass {
                    name,
                    negated: false,
                }))
            }

            Some(Token::UnicodeClassNeg { name }) => {
                let name = name.clone();
                self.advance()?;
                Ok(Regex::CharClass(CharClass::UnicodeClass {
                    name,
                    negated: true,
                }))
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

            Some(Token::ExtendedCharClass { content }) => {
                let content = content.clone();
                self.advance()?;
                Ok(Regex::ExtendedCharClass { content })
            }

            Some(Token::Recursion { target }) => {
                let target = target.clone();
                self.advance()?;
                Ok(Regex::Recursion { target })
            }

            Some(Token::ConditionalStart { condition }) => {
                let condition = condition.clone();
                self.advance()?; // consume conditional start token

                let true_branch = self.parse_sequence()?;
                let false_branch = if matches!(self.peek(), Some(Token::Alternation)) {
                    self.advance()?; // consume conditional branch separator '|'
                    Some(Box::new(self.parse_sequence()?))
                } else {
                    None
                };

                match self.peek() {
                    Some(Token::GroupEnd) => {
                        self.advance()?; // consume ')'
                        Ok(Regex::Conditional {
                            condition,
                            true_branch: Box::new(true_branch),
                            false_branch,
                        })
                    }
                    _ => Err(LexError::UnexpectedEOF {
                        expected: "closing parenthesis ')'".to_string(),
                        position: self
                            .current_token
                            .as_ref()
                            .map(|t| t.position.clone())
                            .unwrap_or_else(|| crate::token::Position::start()),
                    }),
                }
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
                        position: self
                            .current_token
                            .as_ref()
                            .map(|t| t.position.clone())
                            .unwrap_or_else(|| crate::token::Position::start()),
                    }),
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
                        position: self
                            .current_token
                            .as_ref()
                            .map(|t| t.position.clone())
                            .unwrap_or_else(|| crate::token::Position::start()),
                    }),
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
                        position: self
                            .current_token
                            .as_ref()
                            .map(|t| t.position.clone())
                            .unwrap_or_else(|| crate::token::Position::start()),
                    }),
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
                        position: self
                            .current_token
                            .as_ref()
                            .map(|t| t.position.clone())
                            .unwrap_or_else(|| crate::token::Position::start()),
                    }),
                }
            }

            Some(Token::BranchResetGroupStart) => {
                self.advance()?; // consume '(?|'
                let expr = self.parse_alternation()?;

                match self.peek() {
                    Some(Token::GroupEnd) => {
                        self.advance()?; // consume ')'
                        Ok(Regex::Group {
                            expr: Box::new(expr),
                            kind: GroupKind::BranchReset,
                            index: None,
                            name: None,
                        })
                    }
                    _ => Err(LexError::UnexpectedEOF {
                        expected: "closing parenthesis ')'".to_string(),
                        position: self
                            .current_token
                            .as_ref()
                            .map(|t| t.position.clone())
                            .unwrap_or_else(|| crate::token::Position::start()),
                    }),
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
                        position: self
                            .current_token
                            .as_ref()
                            .map(|t| t.position.clone())
                            .unwrap_or_else(|| crate::token::Position::start()),
                    }),
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
                        position: self
                            .current_token
                            .as_ref()
                            .map(|t| t.position.clone())
                            .unwrap_or_else(|| crate::token::Position::start()),
                    }),
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
                        position: self
                            .current_token
                            .as_ref()
                            .map(|t| t.position.clone())
                            .unwrap_or_else(|| crate::token::Position::start()),
                    }),
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
                        position: self
                            .current_token
                            .as_ref()
                            .map(|t| t.position.clone())
                            .unwrap_or_else(|| crate::token::Position::start()),
                    }),
                }
            }

            Some(Token::EOF) => Err(LexError::UnexpectedEOF {
                expected: "regex expression".to_string(),
                position: self
                    .current_token
                    .as_ref()
                    .map(|t| t.position.clone())
                    .unwrap_or_else(|| crate::token::Position::start()),
            }),

            Some(other) => Err(LexError::UnexpectedEOF {
                expected: format!("unexpected token: {:?}", other),
                position: self
                    .current_token
                    .as_ref()
                    .map(|t| t.position.clone())
                    .unwrap_or_else(|| crate::token::Position::start()),
            }),

            None => Err(LexError::UnexpectedEOF {
                expected: "regex expression".to_string(),
                position: crate::token::Position::start(),
            }),
        };
        match &result {
            Ok(node) => trace_exit!(
                "parser",
                "Parser::parse_atom",
                "ok=true,node_kind={}",
                Self::regex_kind(node)
            ),
            Err(err) => trace_exit!("parser", "Parser::parse_atom", "ok=false,error={}", err),
        }
        result
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
            _ => panic!("Expected sequence"),
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
            _ => panic!("Expected quantified"),
        }
    }

    #[test]
    fn test_parse_possessive_quantified() {
        let mut parser = Parser::new("a*+").unwrap();
        let ast = parser.parse().unwrap();

        match ast {
            Regex::Group {
                expr, kind, name, ..
            } => {
                assert!(matches!(kind, GroupKind::Atomic));
                assert_eq!(name, None);
                match *expr {
                    Regex::Quantified { expr, quantifier } => {
                        assert!(matches!(*expr, Regex::Char('a')));
                        assert!(matches!(quantifier, Quantifier::ZeroOrMore { lazy: false }));
                    }
                    _ => panic!("Expected quantified expression inside possessive atomic group"),
                }
            }
            _ => panic!("Expected possessive quantifier to lower into an atomic group"),
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
            _ => panic!("Expected alternation"),
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
                    _ => panic!("Expected sequence inside group"),
                }
            }
            _ => panic!("Expected group"),
        }
    }

    #[test]
    fn test_parse_non_capturing_group() {
        let mut parser = Parser::new("(?:abc)").unwrap();
        let ast = parser.parse().unwrap();

        match ast {
            Regex::Group {
                expr, kind, name, ..
            } => {
                assert!(matches!(kind, GroupKind::NonCapturing));
                assert_eq!(name, None);
                match *expr {
                    Regex::Sequence(elements) => {
                        assert_eq!(elements.len(), 3);
                        assert!(matches!(elements[0], Regex::Char('a')));
                    }
                    _ => panic!("Expected sequence inside non-capturing group"),
                }
            }
            _ => panic!("Expected non-capturing group"),
        }
    }

    #[test]
    fn test_parse_named_group() {
        let mut parser = Parser::new("(?<word>abc)").unwrap();
        let ast = parser.parse().unwrap();

        match ast {
            Regex::Group {
                expr, kind, name, ..
            } => {
                assert!(matches!(kind, GroupKind::Capturing));
                assert_eq!(name, Some("word".to_string()));
                match *expr {
                    Regex::Sequence(elements) => {
                        assert_eq!(elements.len(), 3);
                        assert!(matches!(elements[0], Regex::Char('a')));
                    }
                    _ => panic!("Expected sequence inside named group"),
                }
            }
            _ => panic!("Expected named capturing group"),
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
    fn test_parse_branch_reset_group() {
        let mut parser = Parser::new("(?|a|b)").unwrap();
        let ast = parser.parse().unwrap();

        match ast {
            Regex::Group {
                expr, kind, name, ..
            } => {
                assert!(matches!(kind, GroupKind::BranchReset));
                assert_eq!(name, None);
                match *expr {
                    Regex::Alternation(alternatives) => {
                        assert_eq!(alternatives.len(), 2);
                        assert!(matches!(alternatives[0], Regex::Char('a')));
                        assert!(matches!(alternatives[1], Regex::Char('b')));
                    }
                    other => {
                        panic!("Expected alternation inside branch-reset group, got: {other:?}")
                    }
                }
            }
            other => panic!("Expected branch-reset group, got: {other:?}"),
        }
    }

    #[test]
    fn test_parse_extended_char_class() {
        let mut parser = Parser::new("(?[[a-z]])").unwrap();
        let ast = parser.parse().unwrap();

        match ast {
            Regex::ExtendedCharClass { content } => {
                assert_eq!(content, "[a-z]");
            }
            other => panic!("Expected extended character class, got: {other:?}"),
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
                assert_eq!(
                    target,
                    crate::ast::RecursionTarget::NamedGroup("word".to_string())
                );
            }
            _ => panic!("Expected recursion node"),
        }
    }

    #[test]
    fn test_parse_conditional_group_exists() {
        let mut parser = Parser::new("(?(1)a|b)").unwrap();
        let ast = parser.parse().unwrap();

        match ast {
            Regex::Conditional {
                condition,
                true_branch,
                false_branch,
            } => {
                assert_eq!(condition, crate::ast::ConditionalTest::GroupExists(1));
                assert!(matches!(*true_branch, Regex::Char('a')));
                let false_branch = false_branch.expect("Expected false branch");
                assert!(matches!(*false_branch, Regex::Char('b')));
            }
            _ => panic!("Expected conditional node"),
        }
    }

    #[test]
    fn test_parse_conditional_relative_group_exists_positive() {
        let mut parser = Parser::new("(?(+1)a|b)").unwrap();
        let ast = parser.parse().unwrap();

        match ast {
            Regex::Conditional {
                condition,
                true_branch,
                false_branch,
            } => {
                assert_eq!(
                    condition,
                    crate::ast::ConditionalTest::RelativeGroupExists(1)
                );
                assert!(matches!(*true_branch, Regex::Char('a')));
                let false_branch = false_branch.expect("Expected false branch");
                assert!(matches!(*false_branch, Regex::Char('b')));
            }
            _ => panic!("Expected conditional node"),
        }
    }

    #[test]
    fn test_parse_conditional_relative_group_exists_negative() {
        let mut parser = Parser::new("(?(-1)a|b)").unwrap();
        let ast = parser.parse().unwrap();

        match ast {
            Regex::Conditional {
                condition,
                true_branch,
                false_branch,
            } => {
                assert_eq!(
                    condition,
                    crate::ast::ConditionalTest::RelativeGroupExists(-1)
                );
                assert!(matches!(*true_branch, Regex::Char('a')));
                let false_branch = false_branch.expect("Expected false branch");
                assert!(matches!(*false_branch, Regex::Char('b')));
            }
            _ => panic!("Expected conditional node"),
        }
    }

    #[test]
    fn test_parse_conditional_named_group_exists_without_false_branch() {
        let mut parser = Parser::new("(?(<word>)a)").unwrap();
        let ast = parser.parse().unwrap();

        match ast {
            Regex::Conditional {
                condition,
                true_branch,
                false_branch,
            } => {
                assert_eq!(
                    condition,
                    crate::ast::ConditionalTest::NamedGroupExists("word".to_string())
                );
                assert!(matches!(*true_branch, Regex::Char('a')));
                assert!(false_branch.is_none());
            }
            _ => panic!("Expected conditional node"),
        }
    }

    #[test]
    fn test_parse_conditional_bare_named_group_exists() {
        let mut parser = Parser::new("(?(word)a|b)").unwrap();
        let ast = parser.parse().unwrap();

        match ast {
            Regex::Conditional {
                condition,
                true_branch,
                false_branch,
            } => {
                assert_eq!(
                    condition,
                    crate::ast::ConditionalTest::NamedGroupExists("word".to_string())
                );
                assert!(matches!(*true_branch, Regex::Char('a')));
                let false_branch = false_branch.expect("Expected false branch");
                assert!(matches!(*false_branch, Regex::Char('b')));
            }
            _ => panic!("Expected conditional node"),
        }
    }

    #[test]
    fn test_parse_conditional_define_condition() {
        let mut parser = Parser::new("(?(DEFINE)a)").unwrap();
        let ast = parser.parse().unwrap();

        match ast {
            Regex::Conditional {
                condition,
                true_branch,
                false_branch,
            } => {
                assert_eq!(condition, crate::ast::ConditionalTest::Define);
                assert!(matches!(*true_branch, Regex::Char('a')));
                assert!(false_branch.is_none());
            }
            _ => panic!("Expected conditional node"),
        }
    }

    #[test]
    fn test_parse_conditional_recursion_any() {
        let mut parser = Parser::new("(?(R)a|b)").unwrap();
        let ast = parser.parse().unwrap();

        match ast {
            Regex::Conditional { condition, .. } => {
                assert_eq!(condition, crate::ast::ConditionalTest::RecursionAny);
            }
            _ => panic!("Expected conditional node"),
        }
    }

    #[test]
    fn test_parse_conditional_recursion_group() {
        let mut parser = Parser::new("(?(R1)a|b)").unwrap();
        let ast = parser.parse().unwrap();

        match ast {
            Regex::Conditional { condition, .. } => {
                assert_eq!(condition, crate::ast::ConditionalTest::RecursionGroup(1));
            }
            _ => panic!("Expected conditional node"),
        }
    }

    #[test]
    fn test_parse_conditional_recursion_named() {
        let mut parser = Parser::new("(?(R&word)a|b)").unwrap();
        let ast = parser.parse().unwrap();

        match ast {
            Regex::Conditional { condition, .. } => {
                assert_eq!(
                    condition,
                    crate::ast::ConditionalTest::RecursionNamed("word".to_string())
                );
            }
            _ => panic!("Expected conditional node"),
        }
    }

    #[test]
    fn test_parse_conditional_lookahead_condition() {
        let mut parser = Parser::new("(?(?=ab)x|y)").unwrap();
        let ast = parser.parse().unwrap();

        match ast {
            Regex::Conditional {
                condition,
                true_branch,
                false_branch,
            } => {
                match condition {
                    crate::ast::ConditionalTest::Lookahead { expr, positive } => {
                        assert!(positive);
                        assert_eq!(
                            *expr,
                            Regex::Sequence(vec![Regex::Char('a'), Regex::Char('b')])
                        );
                    }
                    other => panic!("Expected lookahead condition, got: {other:?}"),
                }
                assert!(matches!(*true_branch, Regex::Char('x')));
                let false_branch = false_branch.expect("Expected false branch");
                assert!(matches!(*false_branch, Regex::Char('y')));
            }
            _ => panic!("Expected conditional node"),
        }
    }

    #[test]
    fn test_parse_conditional_negative_lookahead_condition() {
        let mut parser = Parser::new("(?(?!ab)x|y)").unwrap();
        let ast = parser.parse().unwrap();

        match ast {
            Regex::Conditional {
                condition,
                true_branch,
                false_branch,
            } => {
                match condition {
                    crate::ast::ConditionalTest::Lookahead { expr, positive } => {
                        assert!(!positive);
                        assert_eq!(
                            *expr,
                            Regex::Sequence(vec![Regex::Char('a'), Regex::Char('b')])
                        );
                    }
                    other => panic!("Expected negative lookahead condition, got: {other:?}"),
                }
                assert!(matches!(*true_branch, Regex::Char('x')));
                let false_branch = false_branch.expect("Expected false branch");
                assert!(matches!(*false_branch, Regex::Char('y')));
            }
            _ => panic!("Expected conditional node"),
        }
    }

    #[test]
    fn test_parse_conditional_lookbehind_condition() {
        let mut parser = Parser::new("(?(?<=z)a|b)").unwrap();
        let ast = parser.parse().unwrap();

        match ast {
            Regex::Conditional {
                condition,
                true_branch,
                false_branch,
            } => {
                match condition {
                    crate::ast::ConditionalTest::Lookbehind { expr, positive } => {
                        assert!(positive);
                        assert_eq!(*expr, Regex::Char('z'));
                    }
                    other => panic!("Expected lookbehind condition, got: {other:?}"),
                }
                assert!(matches!(*true_branch, Regex::Char('a')));
                let false_branch = false_branch.expect("Expected false branch");
                assert!(matches!(*false_branch, Regex::Char('b')));
            }
            _ => panic!("Expected conditional node"),
        }
    }

    #[test]
    fn test_parse_conditional_negative_lookbehind_condition() {
        let mut parser = Parser::new("(?(?<!z)a|b)").unwrap();
        let ast = parser.parse().unwrap();

        match ast {
            Regex::Conditional {
                condition,
                true_branch,
                false_branch,
            } => {
                match condition {
                    crate::ast::ConditionalTest::Lookbehind { expr, positive } => {
                        assert!(!positive);
                        assert_eq!(*expr, Regex::Char('z'));
                    }
                    other => panic!("Expected negative lookbehind condition, got: {other:?}"),
                }
                assert!(matches!(*true_branch, Regex::Char('a')));
                let false_branch = false_branch.expect("Expected false branch");
                assert!(matches!(*false_branch, Regex::Char('b')));
            }
            _ => panic!("Expected conditional node"),
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
            _ => panic!("Expected digit character class"),
        }
    }
}
