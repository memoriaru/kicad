//! KiCad Schematic Renderer
//!
//! A 1:1 port of KiCanvas JS schematic renderer to Rust.
//! Supports SVG export and WASM Canvas rendering.

pub mod bridge;
pub mod constants;
pub mod layer;
pub mod painter;
pub mod pcb_renderer;
pub mod render_core;
pub mod renderer;
pub mod schematic_renderer;
pub mod text;

pub use layer::{Layer, LayerElement, LayerElementType, LayerId, LayerSet};
pub use render_core::graphics::{
    Arc, Bezier, Circle, Fill, Polygon, Polyline, Stroke, StrokeStyle,
};
pub use render_core::{Angle, AngleExt, BoundingBox, Color, Matrix, Point};
pub use renderer::{RenderBackend, RenderContext, Renderer, SvgRenderer};
pub use text::{markup_to_svg_tspans, parse_markup, ParsedMarkup, TextSegment, TextStyle};
