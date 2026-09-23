//! Token types for KiCad S-expression lexer

use std::fmt;

/// Token kind enumeration
#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    /// Left parenthesis `(`
    LParen,
    /// Right parenthesis `)`
    RParen,
    /// String literal `"..."` or `'...'`
    String(String),
    /// Number literal (integer or float)
    Number(f64),
    /// Boolean literal `yes` or `no`
    Bool(bool),
    /// Identifier (unquoted symbol)
    Identifier(String),
    /// End of file
    Eof,
}

/// A token with location information
#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub line: usize,
    pub column: usize,
}

impl Token {
    pub fn new(kind: TokenKind, line: usize, column: usize) -> Self {
        Self { kind, line, column }
    }
}

impl fmt::Display for TokenKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TokenKind::LParen => write!(f, "("),
            TokenKind::RParen => write!(f, ")"),
            TokenKind::String(s) => write!(f, "\"{}\"", s),
            TokenKind::Number(n) => write!(f, "{}", n),
            TokenKind::Bool(b) => write!(f, "{}", b),
            TokenKind::Identifier(s) => write!(f, "{}", s),
            TokenKind::Eof => write!(f, "EOF"),
        }
    }
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} at line {}, column {}",
            self.kind, self.line, self.column
        )
    }
}
