//! Error types for the KiCad to JSON5 compiler

use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Lexer error at line {line}, column {column}: {message}")]
    LexerError {
        line: usize,
        column: usize,
        message: String,
    },

    #[error("Parser error at line {line}, column {column}: {message}")]
    ParserError {
        line: usize,
        column: usize,
        message: String,
    },

    #[error("Unexpected token: expected {expected}, found {found}")]
    UnexpectedToken { expected: String, found: String },

    #[error("Unexpected end of file")]
    UnexpectedEof,

    #[error("Invalid S-expression: {0}")]
    InvalidSExpr(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON serialization error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("Invalid UUID: {0}")]
    InvalidUuid(String),

    #[error("JSON5 parse error: {0}")]
    Json5Parse(String),
}
