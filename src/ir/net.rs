//! Net and wire definitions

use super::graphic::{Stroke, TextEffects};

/// How a net should be rendered in the schematic output
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RenderHint {
    /// Connect same-net pins with wire segments (default)
    #[default]
    Wire,
    /// Place a net label at each pin
    Label,
    /// Generate a KiCad power symbol instance
    Power,
}

impl RenderHint {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "wire" => Some(Self::Wire),
            "label" => Some(Self::Label),
            "power" => Some(Self::Power),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Wire => "wire",
            Self::Label => "label",
            Self::Power => "power",
        }
    }
}

/// A net in the schematic
#[derive(Debug, Clone)]
pub struct Net {
    /// Net ID (number)
    pub id: u32,
    /// Net name
    pub name: String,
    /// Net type (e.g., "power", "signal")
    pub net_type: Option<String>,
    /// How this net should be rendered (wire/label/power)
    pub render: RenderHint,
}

impl Net {
    pub fn new(id: u32, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            net_type: None,
            render: RenderHint::default(),
        }
    }
}

/// A wire segment
#[derive(Debug, Clone)]
pub struct Wire {
    /// Start point (x, y)
    pub start: (f64, f64),
    /// End point (x, y)
    pub end: (f64, f64),
    /// Net ID (optional, inferred from connections)
    pub net_id: Option<u32>,
    /// Stroke style
    pub stroke: Stroke,
}

impl Wire {
    pub fn new(start: (f64, f64), end: (f64, f64)) -> Self {
        Self {
            start,
            end,
            net_id: None,
            stroke: Stroke::default(),
        }
    }
}

/// A label on a net
#[derive(Debug, Clone)]
pub struct Label {
    /// Label text
    pub text: String,
    /// Position (x, y, rotation)
    pub position: (f64, f64, f64),
    /// Label type (e.g., "label", "global_label", "hierarchical_label")
    pub label_type: String,
    /// Net name (for global labels)
    pub net_name: Option<String>,
    /// Shape for global/hierarchical labels (e.g., "input", "output", "bidirectional")
    pub shape: String,
    /// Text effects (font, justify, etc.)
    pub effects: TextEffects,
}

impl Label {
    pub fn new(text: impl Into<String>, label_type: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            position: (0.0, 0.0, 0.0),
            label_type: label_type.into(),
            net_name: None,
            shape: "passive".to_string(),
            effects: TextEffects::default(),
        }
    }
}

/// A junction point (dot where wires connect)
#[derive(Debug, Clone)]
pub struct Junction {
    /// Position (x, y)
    pub position: (f64, f64),
    /// Diameter
    pub diameter: f64,
}

/// A no-connect symbol (X mark indicating unconnected pin)
#[derive(Debug, Clone)]
pub struct NoConnect {
    /// Position (x, y)
    pub position: (f64, f64),
    /// UUID
    pub uuid: Option<String>,
}

impl NoConnect {
    pub fn new(position: (f64, f64)) -> Self {
        Self {
            position,
            uuid: None,
        }
    }
}

/// A bus (thick line representing multiple signals)
#[derive(Debug, Clone)]
pub struct Bus {
    /// Points defining the bus path
    pub points: Vec<(f64, f64)>,
    /// Stroke style
    pub stroke: Stroke,
}

impl Bus {
    pub fn new() -> Self {
        Self {
            points: Vec::new(),
            stroke: Stroke::default(),
        }
    }
}

/// A bus entry (diagonal line connecting wire to bus)
#[derive(Debug, Clone)]
pub struct BusEntry {
    /// Position (x, y)
    pub position: (f64, f64),
    /// Size (dx, dy) - typically a small diagonal offset
    pub size: (f64, f64),
    /// Stroke style
    pub stroke: Stroke,
}

impl BusEntry {
    pub fn new(position: (f64, f64), size: (f64, f64)) -> Self {
        Self {
            position,
            size,
            stroke: Stroke::default(),
        }
    }
}
