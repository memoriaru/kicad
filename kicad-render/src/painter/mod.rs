//! Painter module - renders schematic elements to graphics primitives
//!
//! Each Painter is responsible for converting schematic elements (pins, wires, symbols, etc.)
//! into graphics primitives (circles, polylines, polygons, etc.) that can be rendered.

mod junction_painter;
mod label_painter;
mod pin_painter;
mod sheet_painter;
mod symbol_painter;
mod wire_painter;

pub use junction_painter::{Junction, JunctionPainter};
pub use label_painter::{Label, LabelPainter, LabelShape, LabelType};
pub use pin_painter::{PinGraphic, PinOrientation, PinPainter, PinShape, PinType};
pub use sheet_painter::{SheetInstance, SheetPainter, SheetPinRender, SheetPropertyRender};
pub use symbol_painter::{Mirror, SymbolInstance, SymbolPainter};
pub use wire_painter::{WirePainter, WireSegment};

use crate::render_core::BoundingBox;

/// Painter trait - converts schematic elements to graphics primitives
pub trait Painter {
    /// Get the bounding box of the element
    fn bbox(&self) -> BoundingBox;

    /// Paint the element to the given layers
    fn paint(&self, layers: &mut crate::layer::LayerSet);
}
