//! Symbol library definitions

use super::component::Pin;

/// A symbol definition from the library
#[derive(Debug, Clone)]
pub struct Symbol {
    /// Library ID (e.g., "Device:R")
    pub lib_id: String,
    /// Reference prefix (e.g., "R")
    pub reference: String,
    /// Default value
    pub value: Option<String>,
    /// Footprint
    pub footprint: Option<String>,
    /// Pin definitions
    pub pins: Vec<Pin>,
    /// Whether pin numbers are hidden
    pub pin_numbers_hidden: bool,
    /// Whether pin names are hidden
    pub pin_names_hidden: bool,
    /// Whether this is a power symbol
    pub is_power: bool,
    /// Include in BOM
    pub in_bom: bool,
    /// Include on board
    pub on_board: bool,
}

impl Symbol {
    pub fn new(lib_id: impl Into<String>) -> Self {
        Self {
            lib_id: lib_id.into(),
            reference: String::new(),
            value: None,
            footprint: None,
            pins: Vec::new(),
            pin_numbers_hidden: false,
            pin_names_hidden: false,
            is_power: false,
            in_bom: true,
            on_board: true,
        }
    }
}
