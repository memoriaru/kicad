//! Layout engine — constraint-based auto-placement for schematics and PCBs.

pub mod board_layout;
pub mod grouping;
pub mod pin_provider;
pub mod solver;
pub mod types;

pub use board_layout::{apply_to, auto_layout, BoardLayout, DividerLine, Section, SectionLabel};
pub use grouping::{
    build_peripheral_constraint, group_by_ic, is_connector_component, is_ic_component,
    ComponentGroup,
};
pub use pin_provider::{
    collect_pin_positions, FootprintPinProvider, PinPositionProvider, SymbolPinProvider,
};
pub use solver::PlacementSolver;
pub use types::{BodyDims, PlacementEntry, PlacementResult};
