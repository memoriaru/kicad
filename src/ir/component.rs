//! Component and pin definitions

use std::collections::HashMap;

use super::graphic::TextEffects;

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

/// Mirror direction for a symbol
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mirror {
    #[default]
    None,
    X,
    Y,
}

/// A property with full rendering information
#[derive(Debug, Clone)]
pub struct Property {
    /// Property name (e.g., "Reference", "Value")
    pub name: String,
    /// Property value (e.g., "R1", "10k")
    pub value: String,
    /// Position (x, y, rotation)
    pub position: (f64, f64, f64),
    /// Whether the property is hidden
    pub hide: bool,
    /// Text effects (font, justify, etc.)
    pub effects: TextEffects,
    /// Whether to show the property name (v10+, default: false)
    pub show_name: bool,
    /// Whether to prevent auto-placement (v10+, default: false)
    pub do_not_autoplace: bool,
}

impl Property {
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
            position: (0.0, 0.0, 0.0),
            hide: false,
            effects: TextEffects::default(),
            show_name: false,
            do_not_autoplace: false,
        }
    }
}

/// A path within an instances project block
#[derive(Debug, Clone)]
pub struct InstancePath {
    /// Hierarchical path UUID string
    pub path: String,
    /// Reference designator in this context
    pub reference: String,
    /// Unit number in this context
    pub unit: u32,
}

/// A project entry in the instances block
#[derive(Debug, Clone)]
pub struct InstanceProject {
    /// Project name
    pub name: String,
    /// Path entries
    pub paths: Vec<InstancePath>,
}

/// The instances block for a symbol instance
#[derive(Debug, Clone, Default)]
pub struct Instances {
    pub projects: Vec<InstanceProject>,
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
    /// Mirror direction
    pub mirror: Mirror,
    /// Pin instances
    pub pins: Vec<PinInstance>,
    /// Additional properties (simple name -> value map for backward compatibility)
    pub properties: HashMap<String, String>,
    /// Properties with full rendering information
    pub properties_ext: Vec<Property>,
    /// UUID
    pub uuid: Option<String>,
    /// Unit number (for multi-unit symbols)
    pub unit: u32,
    /// Exclude from simulation
    pub exclude_from_sim: bool,
    /// Include in bill of materials
    pub in_bom: bool,
    /// Include on board
    pub on_board: bool,
    /// Do Not Populate
    pub dnp: bool,
    /// Instance hierarchy (instances block)
    pub instances: Instances,
}

impl SymbolInstance {
    pub fn new(lib_id: impl Into<String>, reference: impl Into<String>) -> Self {
        Self {
            lib_id: lib_id.into(),
            reference: reference.into(),
            value: String::new(),
            footprint: None,
            position: (0.0, 0.0, 0.0),
            mirror: Mirror::None,
            pins: Vec::new(),
            properties: HashMap::new(),
            properties_ext: Vec::new(),
            uuid: None,
            unit: 1,
            exclude_from_sim: false,
            in_bom: true,
            on_board: true,
            dnp: false,
            instances: Instances::default(),
        }
    }
}
