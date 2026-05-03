//! KiCad Schematic Renderer
//!
//! A 1:1 port of KiCanvas JS schematic renderer to Rust.
//! Supports SVG export and WASM Canvas rendering.

pub mod render_core;
pub mod renderer;
pub mod layer;
pub mod painter;
pub mod text;
pub mod constants;
pub mod bridge;
pub mod schematic_renderer;

pub use render_core::{
    Point, Angle, AngleExt,
    Matrix,
    Color,
    BoundingBox,
};
pub use render_core::graphics::{
    Circle, Arc, Polyline, Polygon, Bezier, Stroke, Fill, StrokeStyle,
};
pub use renderer::{Renderer, RenderContext, RenderBackend, SvgRenderer};
pub use layer::{Layer, LayerSet, LayerId, LayerElement, LayerElementType};
pub use text::{parse_markup, markup_to_svg_tspans, ParsedMarkup, TextSegment, TextStyle};
