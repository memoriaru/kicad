//! Symbol library definitions

use super::component::{Pin, Property};
use super::graphic::{GraphicElement, SymbolUnit};

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
    /// Pin name offset
    pub pin_name_offset: f64,
    /// Whether this is a power symbol
    pub is_power: bool,
    /// Exclude from simulation
    pub exclude_from_sim: bool,
    /// Include in BOM
    pub in_bom: bool,
    /// Include on board
    pub on_board: bool,
    /// Include in position files (v10+)
    pub in_pos_files: bool,
    /// Duplicate pin numbers are jumpers (v10+)
    pub duplicate_pin_numbers_are_jumpers: bool,
    /// Graphic elements for the default unit
    pub graphics: Vec<GraphicElement>,
    /// Units for multi-unit symbols
    pub units: Vec<SymbolUnit>,
    /// Properties (Reference, Value, Footprint, Datasheet, etc.)
    pub properties: Vec<Property>,
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
            pin_name_offset: 0.254,
            is_power: false,
            exclude_from_sim: false,
            in_bom: true,
            on_board: true,
            in_pos_files: true,
            duplicate_pin_numbers_are_jumpers: false,
            graphics: Vec::new(),
            units: Vec::new(),
            properties: Vec::new(),
        }
    }

    /// Get graphics for a specific unit and style
    pub fn get_unit_graphics(&self, unit_id: u32, style_id: u32) -> Option<&Vec<GraphicElement>> {
        self.units
            .iter()
            .find(|u| u.unit_id == unit_id && u.style_id == style_id)
            .map(|u| &u.graphics)
    }
}
