//! Intermediate Representation (IR) module

pub mod board;
mod component;
mod graphic;
mod net;
mod schematic;
mod symbol;

pub use board::{Board, Footprint};
pub use component::{
    InstancePath, InstanceProject, Instances, Mirror, Pin, PinInstance, Property, SymbolInstance,
};
pub use graphic::{
    Arc, Circle, Fill, FillType, Font, GraphicElement, HorizontalAlign, Justify, PinGraphic,
    PinShape, PinType, Polyline, Rectangle, Stroke, StrokeType, SymbolUnit, Text, TextEffects,
    VerticalAlign,
};
pub use net::{Bus, BusEntry, Junction, Label, Net, NoConnect, RenderHint, Wire};
pub use schematic::{Metadata, Paper, Schematic, Sheet, SheetPin, SheetProperty, TitleBlock};
pub use symbol::Symbol;
