//! Lexer module for KiCad S-expression tokenization

mod scanner;
mod token;

pub use scanner::Lexer;
pub use token::{Token, TokenKind};
