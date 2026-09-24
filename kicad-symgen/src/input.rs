use anyhow::{Context, Result};

use crate::model::*;

/// Load a SymbolSpec from a JSON5 file
pub fn from_json5_file(path: &str) -> Result<SymbolSpec> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read input file: {}", path))?;
    let spec: SymbolSpec = json5::from_str(&content)
        .with_context(|| format!("Failed to parse JSON5 from: {}", path))?;
    Ok(spec)
}
