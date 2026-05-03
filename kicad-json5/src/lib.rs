//! KiCad S-expression / JSON5 bidirectional compiler
//!
//! This crate provides tools to convert KiCad schematic files (.kicad_sch)
//! between S-expression and JSON5 formats, and extract circuit topology
//! for AI-friendly semantic analysis.

pub mod codegen;
pub mod error;
pub mod ir;
pub mod lexer;
pub mod parser;
pub mod topology;

pub use codegen::{Json5Config, Json5Generator, KicadVersion, SexprConfig, SexprGenerator};
pub use error::{Error, Result};
pub use ir::Schematic;
pub use lexer::Lexer;
pub use parser::{parse_json5, Parser};

use std::path::Path;

/// Detect input format from file extension
pub fn detect_input_format(path: &Path) -> InputFormat {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .as_deref()
    {
        Some("json5" | "json") => InputFormat::Json5,
        _ => InputFormat::Sexpr,
    }
}

/// Input file format
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputFormat {
    Sexpr,
    Json5,
}

/// Parse a schematic from source string, auto-detecting format
pub fn parse_schematic(source: &str, format: InputFormat) -> Result<Schematic> {
    match format {
        InputFormat::Sexpr => {
            let lexer = Lexer::new(source);
            let mut parser = Parser::new(lexer);
            parser.parse()
        }
        InputFormat::Json5 => parse_json5(source),
    }
}

/// Convert a KiCad schematic file to JSON5
pub fn convert_file(input: &Path, output: &Path) -> Result<()> {
    let source = std::fs::read_to_string(input)?;
    let json5 = convert_str(&source)?;
    std::fs::write(output, json5)?;
    Ok(())
}

/// Convert a KiCad schematic string to JSON5
pub fn convert_str(source: &str) -> Result<String> {
    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer);
    let schematic = parser.parse()?;
    let generator = Json5Generator::new();
    generator.generate(&schematic)
}
