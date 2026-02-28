//! Component and pin definitions

use std::collections::HashMap;

/// A pin definition in a symbol library
#[derive(Debug, Clone)]
pub struct Pin {
    pub number: String,
    pub name: String,
    pub pin_type: String,
}

/// A pin instance on a component
#[derive(Debug, Clone)]
pub struct PinInstance {
    pub number: String,
    pub name: String,
    pub pin_type: String,
    pub net_id: Option<u32>,
    pub net_name: Option<String>,
}

/// A symbol instance in the schematic
#[derive(Debug, Clone)]
pub struct SymbolInstance {
    /// Library ID (e.g., "Device:R")
    pub lib_id: String,
    /// Reference designator (e.g., "R1")
    pub reference: String,
    /// Value (e.g., "10k")
    pub value: String,
    /// Footprint (e.g., "Resistor_SMD:R_0805")
    pub footprint: Option<String>,
    /// Position (x, y, rotation)
    pub position: (f64, f64, f64),
    /// Pin instances
    pub pins: Vec<PinInstance>,
    /// Additional properties
    pub properties: HashMap<String, String>,
    /// UUID
    pub uuid: Option<String>,
    /// Unit number (for multi-unit symbols)
    pub unit: u32,
}

impl SymbolInstance {
    pub fn new(lib_id: impl Into<String>, reference: impl Into<String>) -> Self {
        Self {
            lib_id: lib_id.into(),
            reference: reference.into(),
            value: String::new(),
            footprint: None,
            position: (0.0, 0.0, 0.0),
            pins: Vec::new(),
            properties: HashMap::new(),
            uuid: None,
            unit: 1,
        }
    }
}
