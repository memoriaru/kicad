//! Scanner for KiCad S-expression tokenization

use super::token::{Token, TokenKind};
use crate::error::{Error, Result};

/// Lexer for KiCad S-expression format
pub struct Lexer<'a> {
    source: &'a str,
    chars: Vec<char>,
    pos: usize,
    line: usize,
    column: usize,
}

impl<'a> Lexer<'a> {
    /// Create a new lexer from source string
    pub fn new(source: &'a str) -> Self {
        Self {
            source,
            chars: source.chars().collect(),
            pos: 0,
            line: 1,
            column: 1,
        }
    }

    /// Get the current character without consuming it
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    /// Get the next character without consuming it
    fn peek_next(&self) -> Option<char> {
        self.chars.get(self.pos + 1).copied()
    }

    /// Advance to the next character
    fn advance(&mut self) -> Option<char> {
        let ch = self.chars.get(self.pos).copied();
        if let Some(c) = ch {
            self.pos += 1;
            if c == '\n' {
                self.line += 1;
                self.column = 1;
            } else {
                self.column += 1;
            }
        }
        ch
    }

    /// Skip whitespace and comments
    fn skip_whitespace(&mut self) {
        loop {
            match self.peek() {
                Some(' ') | Some('\t') | Some('\n') | Some('\r') => {
                    self.advance();
                }
                Some(';') => {
                    // Skip line comment
                    while let Some(ch) = self.peek() {
                        if ch == '\n' {
                            break;
                        }
                        self.advance();
                    }
                }
                Some('#') => {
                    // Skip # comment (alternate)
                    while let Some(ch) = self.peek() {
                        if ch == '\n' {
                            break;
                        }
                        self.advance();
                    }
                }
                _ => break,
            }
        }
    }

    /// Read a string literal
    fn read_string(&mut self, quote: char) -> Result<String> {
        let mut result = String::new();
        self.advance(); // consume opening quote

        loop {
            match self.peek() {
                None => {
                    return Err(Error::LexerError {
                        line: self.line,
                        column: self.column,
                        message: "Unterminated string".to_string(),
                    })
                }
                Some(ch) if ch == quote => {
                    self.advance(); // consume closing quote
                    break;
                }
                Some('\\') => {
                    self.advance();
                    match self.peek() {
                        Some('n') => result.push('\n'),
                        Some('t') => result.push('\t'),
                        Some('r') => result.push('\r'),
                        Some('\\') => result.push('\\'),
                        Some('"') => result.push('"'),
                        Some('\'') => result.push('\''),
                        Some(ch) => result.push(ch),
                        None => {
                            return Err(Error::LexerError {
                                line: self.line,
                                column: self.column,
                                message: "Unterminated escape sequence".to_string(),
                            })
                        }
                    }
                    self.advance();
                }
                Some(ch) => {
                    result.push(ch);
                    self.advance();
                }
            }
        }

        Ok(result)
    }

    /// Read a number literal
    fn read_number(&mut self, first: char) -> Result<f64> {
        let mut num_str = String::from(first);

        // Read integer part
        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() {
                num_str.push(ch);
                self.advance();
            } else {
                break;
            }
        }

        // Read decimal part
        if self.peek() == Some('.') {
            num_str.push('.');
            self.advance();
            while let Some(ch) = self.peek() {
                if ch.is_ascii_digit() {
                    num_str.push(ch);
                    self.advance();
                } else {
                    break;
                }
            }
        }

        // Parse the number
        num_str
            .parse()
            .map_err(|_| Error::LexerError {
                line: self.line,
                column: self.column,
                message: format!("Invalid number: {}", num_str),
            })
    }

    /// Read an identifier or keyword
    fn read_identifier(&mut self, first: char) -> TokenKind {
        let mut ident = String::from(first);

        while let Some(ch) = self.peek() {
            if is_identifier_char(ch) {
                ident.push(ch);
                self.advance();
            } else {
                break;
            }
        }

        // Check for boolean keywords
        match ident.as_str() {
            "yes" | "true" => TokenKind::Bool(true),
            "no" | "false" => TokenKind::Bool(false),
            _ => TokenKind::Identifier(ident),
        }
    }

    /// Get the next token
    pub fn next_token(&mut self) -> Result<Token> {
        self.skip_whitespace();

        let line = self.line;
        let column = self.column;

        match self.peek() {
            None => Ok(Token::new(TokenKind::Eof, line, column)),
            Some('(') => {
                self.advance();
                Ok(Token::new(TokenKind::LParen, line, column))
            }
            Some(')') => {
                self.advance();
                Ok(Token::new(TokenKind::RParen, line, column))
            }
            Some('"') => {
                let s = self.read_string('"')?;
                Ok(Token::new(TokenKind::String(s), line, column))
            }
            Some('\'') => {
                let s = self.read_string('\'')?;
                Ok(Token::new(TokenKind::String(s), line, column))
            }
            Some(ch) if ch.is_ascii_digit() || (ch == '-' && self.peek_next().map_or(false, |c| c.is_ascii_digit())) => {
                if ch == '-' {
                    self.advance(); // consume '-'
                    // Read the actual number (positive)
                    let first_digit = self.peek().ok_or_else(|| Error::LexerError {
                        line: self.line,
                        column: self.column,
                        message: "Expected digit after '-'".to_string(),
                    })?;
                    self.advance(); // consume first digit
                    let num = self.read_number(first_digit)?;
                    Ok(Token::new(TokenKind::Number(-num), line, column))
                } else {
                    self.advance();
                    let num = self.read_number(ch)?;
                    Ok(Token::new(TokenKind::Number(num), line, column))
                }
            }
            Some(ch) if is_identifier_start(ch) => {
                self.advance(); // consume the first character
                let kind = self.read_identifier(ch);
                Ok(Token::new(kind, line, column))
            }
            Some(ch) => Err(Error::LexerError {
                line,
                column,
                message: format!("Unexpected character: '{}'", ch),
            }),
        }
    }

    /// Collect all tokens
    pub fn collect_tokens(&mut self) -> Result<Vec<Token>> {
        let mut tokens = Vec::new();
        loop {
            let token = self.next_token()?;
            let is_eof = token.kind == TokenKind::Eof;
            tokens.push(token);
            if is_eof {
                break;
            }
        }
        Ok(tokens)
    }
}

/// Check if a character can start an identifier
fn is_identifier_start(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_' || ch == '+' || ch == '-' || ch == '*' || ch == '/' || ch == '=' || ch == '!' || ch == '<' || ch == '>' || ch == '#' || ch == ':' || ch == '.' || ch == '%' || ch == '~' || ch == '@' || ch == '$' || ch == '^' || ch == '&' || ch == '|' || ch == '?' || ch == '\\'
}

/// Check if a character can be part of an identifier
fn is_identifier_char(ch: char) -> bool {
    is_identifier_start(ch) || ch.is_ascii_digit()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty() {
        let mut lexer = Lexer::new("");
        let tokens = lexer.collect_tokens().unwrap();
        assert_eq!(tokens.len(), 1);
        assert!(matches!(tokens[0].kind, TokenKind::Eof));
    }

    #[test]
    fn test_parens() {
        let mut lexer = Lexer::new("()");
        let tokens = lexer.collect_tokens().unwrap();
        assert_eq!(tokens.len(), 3);
        assert!(matches!(tokens[0].kind, TokenKind::LParen));
        assert!(matches!(tokens[1].kind, TokenKind::RParen));
        assert!(matches!(tokens[2].kind, TokenKind::Eof));
    }

    #[test]
    fn test_string() {
        let mut lexer = Lexer::new("(symbol \"R1\")");
        let tokens = lexer.collect_tokens().unwrap();
        assert_eq!(tokens.len(), 5); // (, symbol, "R1", ), EOF
        if let TokenKind::String(s) = &tokens[2].kind {
            assert_eq!(s, "R1");
        } else {
            panic!("Expected string token");
        }
    }

    #[test]
    fn test_number() {
        let mut lexer = Lexer::new("(at 100.5 -50)");
        let tokens = lexer.collect_tokens().unwrap();
        if let TokenKind::Number(n) = tokens[2].kind {
            assert!((n - 100.5).abs() < 0.001);
        } else {
            panic!("Expected number token");
        }
        if let TokenKind::Number(n) = tokens[3].kind {
            assert!((n - (-50.0)).abs() < 0.001);
        } else {
            panic!("Expected number token");
        }
    }

    #[test]
    fn test_bool() {
        let mut lexer = Lexer::new("(in_bom yes) (on_board no)");
        let tokens = lexer.collect_tokens().unwrap();
        assert!(matches!(tokens[2].kind, TokenKind::Bool(true)));
        assert!(matches!(tokens[6].kind, TokenKind::Bool(false)));
    }

    #[test]
    fn test_comment() {
        let mut lexer = Lexer::new("(symbol ; this is a comment\n  R1)");
        let tokens = lexer.collect_tokens().unwrap();
        assert_eq!(tokens.len(), 5); // (, symbol, R1, ), EOF
        if let TokenKind::Identifier(s) = &tokens[2].kind {
            assert_eq!(s, "R1");
        } else {
            panic!("Expected identifier token");
        }
    }
}
