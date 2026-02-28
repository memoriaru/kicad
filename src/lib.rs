//! KiCad S-expression to JSON5 Compiler
//!
//! This crate provides tools to convert KiCad schematic files (.kicad_sch)
//! from S-expression format to JSON5 format.

pub mod codegen;
pub mod error;
pub mod ir;
pub mod lexer;
pub mod parser;

pub use codegen::{Json5Config, Json5Generator};
pub use error::{Error, Result};
pub use ir::Schematic;
pub use lexer::Lexer;
pub use parser::Parser;

use std::path::Path;

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
