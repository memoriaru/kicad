//! S-expression parser

use crate::error::{Error, Result};
use crate::ir::Schematic;
use crate::lexer::{Lexer, Token, TokenKind};

use super::ast::{Atom, SExpr};

/// Parser for KiCad S-expression format
pub struct Parser<'a> {
    lexer: Lexer<'a>,
    current: Option<Token>,
}

impl<'a> Parser<'a> {
    /// Create a new parser from a lexer
    pub fn new(lexer: Lexer<'a>) -> Self {
        Self {
            lexer,
            current: None,
        }
    }

    /// Advance to the next token
    fn advance(&mut self) -> Result<Token> {
        let token = self.lexer.next_token()?;
        self.current = Some(token.clone());
        Ok(token)
    }

    /// Get the current token without consuming it
    fn peek(&self) -> Option<&Token> {
        self.current.as_ref()
    }

    /// Expect a specific token kind
    #[allow(dead_code)]
    fn expect(&mut self, expected: &TokenKind) -> Result<Token> {
        let token = self.advance()?;
        if std::mem::discriminant(&token.kind) == std::mem::discriminant(expected) {
            Ok(token)
        } else {
            Err(Error::UnexpectedToken {
                expected: format!("{:?}", expected),
                found: format!("{:?}", token.kind),
            })
        }
    }

    /// Parse a single S-expression (internal)
    /// Uses the current token (already advanced)
    fn parse_sexpr_internal(&mut self) -> Result<SExpr> {
        let token = self.current.clone().ok_or(Error::UnexpectedEof)?;

        match token.kind {
            TokenKind::LParen => self.parse_list(),
            TokenKind::RParen => Err(Error::UnexpectedToken {
                expected: "S-expression".to_string(),
                found: ")".to_string(),
            }),
            TokenKind::String(s) => Ok(SExpr::Atom(Atom::String(s))),
            TokenKind::Number(n) => Ok(SExpr::Atom(Atom::Number(n))),
            TokenKind::Bool(b) => Ok(SExpr::Atom(Atom::Bool(b))),
            TokenKind::Identifier(s) => Ok(SExpr::Atom(Atom::Identifier(s))),
            TokenKind::Eof => Err(Error::UnexpectedEof),
        }
    }

    /// Parse a list S-expression
    fn parse_list(&mut self) -> Result<SExpr> {
        let mut items = Vec::new();

        loop {
            let token = self.advance()?;
            match token.kind {
                TokenKind::RParen => break,
                TokenKind::Eof => return Err(Error::UnexpectedEof),
                _ => {
                    // Put the token back by not consuming it
                    // We need to handle this differently
                    items.push(self.parse_sexpr_from_token(token)?);
                }
            }
        }

        Ok(SExpr::List(items))
    }

    /// Parse an S-expression starting from an already-consumed token
    fn parse_sexpr_from_token(&mut self, token: Token) -> Result<SExpr> {
        match &token.kind {
            TokenKind::LParen => {
                // Set current to LParen, then parse_list will advance to next token
                self.current = Some(token);
                self.parse_list()
            }
            TokenKind::String(s) => Ok(SExpr::Atom(Atom::String(s.clone()))),
            TokenKind::Number(n) => Ok(SExpr::Atom(Atom::Number(*n))),
            TokenKind::Bool(b) => Ok(SExpr::Atom(Atom::Bool(*b))),
            TokenKind::Identifier(s) => Ok(SExpr::Atom(Atom::Identifier(s.clone()))),
            TokenKind::RParen => Err(Error::UnexpectedToken {
                expected: "S-expression".to_string(),
                found: ")".to_string(),
            }),
            TokenKind::Eof => Err(Error::UnexpectedEof),
        }
    }

    /// Parse the entire input into an S-expression AST
    pub fn parse_sexpr(&mut self) -> Result<SExpr> {
        // Prime the lexer
        self.advance()?;
        let result = self.parse_sexpr_internal()?;

        // Advance to next token and ensure we've consumed all input
        self.advance()?;
        if let Some(token) = self.peek() {
            if token.kind != TokenKind::Eof {
                return Err(Error::ParserError {
                    line: token.line,
                    column: token.column,
                    message: format!("Unexpected token after S-expression: {:?}", token.kind),
                });
            }
        }

        Ok(result)
    }

    /// Parse the input into a structured Schematic
    pub fn parse(&mut self) -> Result<Schematic> {
        let sexpr = self.parse_sexpr()?;
        Schematic::from_sexpr(&sexpr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_list() {
        let lexer = Lexer::new("(symbol R1)");
        let mut parser = Parser::new(lexer);
        let result = parser.parse_sexpr().unwrap();

        println!("Parsed result: {:?}", result);
        assert!(result.is_list());
        let list = result.as_list().unwrap();
        println!("List length: {}", list.len());
        for (i, item) in list.iter().enumerate() {
            println!("  [{}]: {:?}", i, item);
        }
        assert_eq!(list.len(), 2);
        assert!(list[0].is_ident("symbol"));
        assert!(list[1].is_ident("R1"));
    }

    #[test]
    fn test_parse_nested_list() {
        let lexer = Lexer::new("(symbol \"R1\" (value \"10k\"))");
        let mut parser = Parser::new(lexer);
        let result = parser.parse_sexpr().unwrap();

        let list = result.as_list().unwrap();
        assert_eq!(list.len(), 3);
        assert!(list[0].is_ident("symbol"));
        assert_eq!(list[1].as_string(), Some("R1"));

        let nested = list[2].as_list().unwrap();
        assert_eq!(nested.len(), 2);
        assert!(nested[0].is_ident("value"));
        assert_eq!(nested[1].as_string(), Some("10k"));
    }

    #[test]
    fn test_parse_numbers() {
        let lexer = Lexer::new("(at 100.5 -50 90)");
        let mut parser = Parser::new(lexer);
        let result = parser.parse_sexpr().unwrap();

        let list = result.as_list().unwrap();
        assert_eq!(list.len(), 4);
        assert!((list[1].as_number().unwrap() - 100.5).abs() < 0.001);
        assert!((list[2].as_number().unwrap() - (-50.0)).abs() < 0.001);
        assert!((list[3].as_number().unwrap() - 90.0).abs() < 0.001);
    }

    #[test]
    fn test_parse_bool() {
        let lexer = Lexer::new("(in_bom yes)");
        let mut parser = Parser::new(lexer);

        // Parse first expression
        let result1 = parser.parse_sexpr().unwrap();
        let list1 = result1.as_list().unwrap();
        assert_eq!(list1[1].as_bool(), Some(true));

        // Test false as well
        let lexer2 = Lexer::new("(on_board no)");
        let mut parser2 = Parser::new(lexer2);
        let result2 = parser2.parse_sexpr().unwrap();
        let list2 = result2.as_list().unwrap();
        assert_eq!(list2[1].as_bool(), Some(false));
    }
}
