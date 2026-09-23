//! Core topology types for circuit analysis
//!
//! These types represent the semantic structure of a circuit schematic,
//! abstracting away visual details like coordinates and rotations.

use std::collections::HashMap;

/// Represents a node in the circuit topology graph
#[derive(Debug, Clone, PartialEq)]
pub enum TopologyNode {
    /// A component instance (e.g., "R1", "U3")
    Component {
        /// Reference designator (e.g., "R1", "U3")
        reference: String,
        /// Kind of component
        kind: ComponentKind,
        /// Library ID (e.g., "Device:R")
        lib_id: String,
        /// Value (e.g., "10k", "100nF")
        value: Option<String>,
    },
    /// A net (electrical connection)
    Net {
        /// Net name (e.g., "VCC", "GND", "SDA")
        name: String,
        /// Kind of net
        kind: NetKind,
    },
    /// A specific pin on a component
    Pin {
        /// Component reference
        component: String,
        /// Pin number or name
        pin: String,
    },
}

/// Classification of component types based on library ID
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ComponentKind {
    /// Resistors (Device:R, Device:R_*)
    Resistor,
    /// Capacitors (Device:C, Device:C_*)
    Capacitor,
    /// Inductors (Device:L, Device:L_*)
    Inductor,
    /// Diodes and LEDs (Device:D, Device:LED, etc.)
    Diode,
    /// Transistors (Transistor_*, Device:Q_*)
    Transistor,
    /// Integrated circuits (MCU:*, IC:*, etc.)
    Ic,
    /// Connectors (Connector:*, Connector_Generic:*)
    Connector,
    /// Power regulators (Regulator_*, Device:Regulator*)
    Power,
    /// Crystal/Oscillator (Device:Crystal, Oscillator:*)
    Crystal,
    /// Switches (Switch:*, Device:SW_*)
    Switch,
    /// Fuse (Device:Fuse)
    Fuse,
    /// Unknown or unclassified component
    Unknown,
}

impl ComponentKind {
    /// Get a human-readable name for this component kind
    pub fn name(&self) -> &'static str {
        match self {
            ComponentKind::Resistor => "resistor",
            ComponentKind::Capacitor => "capacitor",
            ComponentKind::Inductor => "inductor",
            ComponentKind::Diode => "diode",
            ComponentKind::Transistor => "transistor",
            ComponentKind::Ic => "ic",
            ComponentKind::Connector => "connector",
            ComponentKind::Power => "power",
            ComponentKind::Crystal => "crystal",
            ComponentKind::Switch => "switch",
            ComponentKind::Fuse => "fuse",
            ComponentKind::Unknown => "unknown",
        }
    }
}

/// Classification of net types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NetKind {
    /// Power supply nets (VCC, 3V3, 5V, +12V, VIN, VOUT, etc.)
    Power,
    /// Ground nets (GND, AGND, DGND, GNDA, GNDD, VSS, etc.)
    Ground,
    /// Regular signal nets
    Signal,
    /// Data bus nets (multiple bits)
    Bus,
}

impl NetKind {
    /// Get a human-readable name for this net kind
    pub fn name(&self) -> &'static str {
        match self {
            NetKind::Power => "power",
            NetKind::Ground => "ground",
            NetKind::Signal => "signal",
            NetKind::Bus => "bus",
        }
    }
}

/// An edge in the topology graph representing a connection
#[derive(Debug, Clone, PartialEq)]
pub struct TopologyEdge {
    /// Source node
    pub from: TopologyNode,
    /// Target node
    pub to: TopologyNode,
    /// Net name that connects them
    pub net: String,
}

/// The main circuit topology structure
#[derive(Debug, Clone, Default)]
pub struct CircuitTopology {
    /// All nodes in the topology
    pub nodes: Vec<TopologyNode>,
    /// All edges (connections) in the topology
    pub edges: Vec<TopologyEdge>,
    /// Adjacency list: component reference -> connected components
    pub adjacency: HashMap<String, Vec<String>>,
    /// Net to connected pins mapping
    pub net_connections: HashMap<String, Vec<PinConnection>>,
}

/// A pin connection point
#[derive(Debug, Clone, PartialEq)]
pub struct PinConnection {
    /// Component reference
    pub component: String,
    /// Pin identifier
    pub pin: String,
    /// Pin name (if available)
    pub pin_name: Option<String>,
}

/// Component classification result with metadata
#[derive(Debug, Clone)]
pub struct ComponentInfo {
    /// Reference designator
    pub reference: String,
    /// Library ID
    pub lib_id: String,
    /// Component kind
    pub kind: ComponentKind,
    /// Value property
    pub value: Option<String>,
    /// Footprint
    pub footprint: Option<String>,
    /// Connected nets (pin -> net)
    pub connected_nets: HashMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_component_kind_name() {
        assert_eq!(ComponentKind::Resistor.name(), "resistor");
        assert_eq!(ComponentKind::Capacitor.name(), "capacitor");
        assert_eq!(ComponentKind::Ic.name(), "ic");
    }

    #[test]
    fn test_net_kind_name() {
        assert_eq!(NetKind::Power.name(), "power");
        assert_eq!(NetKind::Ground.name(), "ground");
        assert_eq!(NetKind::Signal.name(), "signal");
    }
}
