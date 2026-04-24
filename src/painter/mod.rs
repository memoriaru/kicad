//! Painter module - renders schematic elements to graphics primitives
//!
//! Each Painter is responsible for converting schematic elements (pins, wires, symbols, etc.)
//! into graphics primitives (circles, polylines, polygons, etc.) that can be rendered.

mod pin_painter;
mod wire_painter;
mod symbol_painter;
mod label_painter;
mod junction_painter;

pub use pin_painter::{PinPainter, PinGraphic, PinType, PinShape, PinOrientation};
pub use wire_painter::{WirePainter, WireSegment};
pub use symbol_painter::{SymbolPainter, SymbolInstance, Mirror};
pub use label_painter::{LabelPainter, Label, LabelType, LabelShape};
pub use junction_painter::{JunctionPainter, Junction};

use crate::render_core::{Point, Color, Matrix, BoundingBox};
use crate::render_core::graphics::{Circle, Arc, Polyline, Polygon, Bezier, Stroke, Fill, StrokeStyle};
use crate::layer::{Layer, LayerId, LayerElement, LayerElementType};

/// Painter trait - converts schematic elements to graphics primitives
pub trait Painter {
    /// Get the layers this painter renders to
    fn layers(&self) -> Vec<LayerId>;

    /// Get the bounding box of the element
    fn bbox(&self) -> BoundingBox;

    /// Paint the element to the given layers
    fn paint(&self, layers: &mut crate::layer::LayerSet);
}
