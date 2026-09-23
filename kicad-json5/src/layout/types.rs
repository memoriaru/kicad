//! Layout constraint types for auto-placement engine.

use serde::Deserialize;

/// Resolved placement result for a component
#[derive(Debug, Clone, Copy)]
pub struct PlacementResult {
    pub x: f64,
    pub y: f64,
    pub rotation: f64,
}

/// Package body dimensions for collision detection
#[derive(Debug, Clone, Copy)]
pub struct BodyDims {
    pub width: f64,
    pub height: f64,
}

/// Placement constraint entry — supports fixed coordinates and multiple constraint types.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum PlacementEntry {
    /// Fixed coordinate (backward compatible with existing templates)
    Fixed { x: f64, y: f64 },
    /// Place near a specified pin (e.g. decoupling capacitors)
    NearPin {
        near_pin: String,
        max_distance_mil: f64,
        side: Option<String>,
        clearance_mil: Option<f64>,
    },
    /// Place between two pins (e.g. feedback resistors)
    BetweenPins {
        pin_a: String,
        pin_b: String,
        offset_mil: Option<f64>,
        side: Option<String>,
    },
    /// Offset from an already-placed component
    OffsetFrom {
        offset_from: String,
        dx_mil: f64,
        dy_mil: f64,
    },
}

impl PlacementEntry {
    pub fn as_fixed(&self) -> Option<(f64, f64)> {
        match self {
            PlacementEntry::Fixed { x, y } => Some((*x, *y)),
            _ => None,
        }
    }
}
