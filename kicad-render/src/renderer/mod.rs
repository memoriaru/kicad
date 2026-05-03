//! Renderer abstraction trait
//!
//! Each renderer backend (SVG, Canvas) implements this trait

mod svg;

use crate::render_core::{Color, Point, Matrix, BoundingBox};
use crate::render_core::graphics::{Circle, Arc, Polyline, Polygon, Bezier, Rectangle, Stroke};

pub use svg::SvgRenderer;

/// Render backend type
pub enum RenderBackend {
    Svg,
    #[cfg(feature = "wasm")]
    Canvas,
}

/// Rendering context passed to renderers
#[derive(Debug, Clone)]
pub struct RenderContext {
    /// The view bounds
    pub bounds: BoundingBox,
    /// Scale factor (pixels per KiCad unit)
    pub scale: f64,
    /// Transform for world to screen coordinates
    pub transform: Matrix,
}

impl RenderContext {
    pub fn new(bounds: BoundingBox, scale: f64) -> Self {
        let transform = Matrix::identity();
        Self { bounds, scale, transform }
    }

    /// Create context for SVG rendering
    pub fn for_svg(bounds: BoundingBox, scale: f64) -> Self {
        Self {
            bounds,
            scale,
            transform: Matrix::translation(
                -bounds.x * scale,
                -bounds.y * scale,
            ),
        }
    }

    /// Create context with custom transform
    pub fn with_transform(bounds: BoundingBox, scale: f64, transform: Matrix) -> Self {
        Self { bounds, scale, transform }
    }
}

/// Renderer trait - implemented by each backend
pub trait Renderer {
    /// Get the current context
    fn context(&self) -> &RenderContext;

    /// Save current state to stack
    fn save(&mut self);

    /// Restore previous state from stack
    fn restore(&mut self);

    /// Set the current transform matrix
    fn set_transform(&mut self, transform: &Matrix);

    /// Draw a circle
    fn draw_circle(&mut self, circle: &Circle);

    /// Draw an arc
    fn draw_arc(&mut self, arc: &Arc);

    /// Draw a polyline
    fn draw_polyline(&mut self, polyline: &Polyline);

    /// Draw a polygon
    fn draw_polygon(&mut self, polygon: &Polygon);

    /// Draw a bezier curve
    fn draw_bezier(&mut self, bezier: &Bezier);

    /// Draw text with optional rotation and alignment
    /// - rotation: degrees (0 = horizontal, 90 = vertical)
    /// - text_anchor: "start" (default), "middle", "end" — SVG text-anchor
    /// - dominant_baseline: "" (default/auto), "central", "hanging" — SVG dominant-baseline
    fn draw_text(&mut self, position: &Point, text: &str, font_size: f64, color: &Color, bold: bool, rotation: f64, text_anchor: &str, dominant_baseline: &str);

    /// Draw a line (convenience)
    fn draw_line(&mut self, start: &Point, end: &Point, stroke: &Stroke) {
        let polyline = Polyline::from_points(&[(start.x, start.y), (end.x, end.y)], stroke.clone());
        self.draw_polyline(&polyline);
    }

    /// Draw a rectangle (convenience)
    fn draw_rect(&mut self, rect: &Rectangle, _fill_opt: Option<Color>, _stroke_opt: Option<Stroke>) {
        let polygon = rect.to_polygon();
        self.draw_polygon(&polygon);
    }
}
