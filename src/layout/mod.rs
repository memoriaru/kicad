//! Layout engine — constraint-based auto-placement for schematics and PCBs.

pub mod board_layout;
pub mod grouping;
pub mod pin_provider;
pub mod solver;
pub mod types;

pub use board_layout::{BoardLayout, DividerLine, Section, SectionLabel, auto_layout, apply_to};
pub use grouping::{ComponentGroup, build_peripheral_constraint, group_by_ic, is_ic_component, is_connector_component};
pub use pin_provider::{FootprintPinProvider, PinPositionProvider, SymbolPinProvider, collect_pin_positions};
pub use solver::PlacementSolver;
pub use types::{BodyDims, PlacementEntry, PlacementResult};
