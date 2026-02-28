//! Net and wire definitions

/// A net in the schematic
#[derive(Debug, Clone)]
pub struct Net {
    /// Net ID (number)
    pub id: u32,
    /// Net name
    pub name: String,
    /// Net type (e.g., "power", "signal")
    pub net_type: Option<String>,
}

impl Net {
    pub fn new(id: u32, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            net_type: None,
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

/// Stroke style for wires and shapes
#[derive(Debug, Clone)]
pub struct Stroke {
    pub width: f64,
    pub stroke_type: String,
}

impl Default for Stroke {
    fn default() -> Self {
        Self {
            width: 0.0,
            stroke_type: "default".to_string(),
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
}

/// A junction point (dot where wires connect)
#[derive(Debug, Clone)]
pub struct Junction {
    /// Position (x, y)
    pub position: (f64, f64),
    /// Diameter
    pub diameter: f64,
}
