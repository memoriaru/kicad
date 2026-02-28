//! Intermediate Representation (IR) module

mod component;
mod net;
mod schematic;
mod symbol;

pub use component::{Pin, PinInstance, SymbolInstance};
pub use net::{Net, Wire, Label, Junction};
pub use schematic::{Metadata, Paper, Schematic, TitleBlock};
pub use symbol::Symbol;
