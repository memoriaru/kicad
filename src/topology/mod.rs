//! Circuit topology extraction module
//!
//! This module provides tools for extracting semantic topology from
//! KiCad schematic IR. The topology represents circuit structure
//! without visual details like coordinates and rotations.
//!
//! # Overview
//!
//! The topology extraction process:
//! 1. **Classification**: Components and nets are classified by type
//! 2. **Connection Graph**: Build adjacency relationships between components
//! 3. **Module Identification**: Recognize common circuit patterns
//! 4. **Summary Generation**: Create AI-friendly output
//!
//! # Example
//!
//! ```ignore
//! use kicad_json5::{Schematic, topology::extract_topology};
//!
//! let schematic = parse_schematic(...);
//! let summary = extract_topology(&schematic);
//!
//! println!("Power domains: {:?}", summary.power_domains);
//! println!("Components: {:?}", summary.component_summary);
//! ```

mod classify;
mod connectivity;
mod extractor;
mod patterns;
mod summary;
mod types;

pub use classify::{classify_component, classify_net, extract_voltage};
pub use extractor::{extract_topology, TopologyExtractor};
pub use patterns::{builtin_patterns, ConnectionPattern, ModulePattern, PatternMatcher};
pub use summary::{
    ComponentSummary, FunctionalModule, PowerDomain, SignalPath, TopologySummary,
    TopologySummaryBuilder,
};
pub use types::{
    CircuitTopology, ComponentInfo, ComponentKind, NetKind, PinConnection, TopologyEdge,
    TopologyNode,
};
