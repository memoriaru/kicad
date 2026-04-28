//! Wire Painter - renders schematic wires to graphics primitives
//!
//! Based on KiCanvas JS `WirePainter` class

use crate::render_core::{Point, Color, BoundingBox};
use crate::render_core::graphics::{Polyline, Stroke};
use crate::layer::{LayerSet, LayerId, LayerElement, LayerElementType};
use super::Painter;

/// Wire segment in a schematic
#[derive(Debug, Clone)]
pub struct WireSegment {
    /// Start point
    pub start: Point,
    /// End point
    pub end: Point,
}

impl WireSegment {
    pub fn new(start: Point, end: Point) -> Self {
        Self { start, end }
    }

    /// Get the length of the wire
    pub fn length(&self) -> f64 {
        let dx = self.end.x - self.start.x;
        let dy = self.end.y - self.start.y;
        (dx * dx + dy * dy).sqrt()
    }

    /// Check if the wire is horizontal
    pub fn is_horizontal(&self) -> bool {
        (self.start.y - self.end.y).abs() < 1e-6
    }

    /// Check if the wire is vertical
    pub fn is_vertical(&self) -> bool {
        (self.start.x - self.end.x).abs() < 1e-6
    }

    /// Get the midpoint of the wire
    pub fn midpoint(&self) -> Point {
        Point::new(
            (self.start.x + self.end.x) / 2.0,
            (self.start.y + self.end.y) / 2.0,
        )
    }
}


/// Wire Painter - renders wires to graphics
pub struct WirePainter {
    /// Wire segments
    pub segments: Vec<WireSegment>,
    /// Wire color
    pub color: Color,
    /// Wire width in mm (default: WIRE_WIDTH_MM = 6 mils)
    pub width: f64,
}

impl WirePainter {
    /// Create a new wire painter
    pub fn new(segments: Vec<WireSegment>, color: Color) -> Self {
        Self {
            segments,
            color,
            width: 0.1524, // 6 mils
        }
    }

    /// Create a wire painter with custom width
    pub fn with_width(segments: Vec<WireSegment>, color: Color, width: f64) -> Self {
        Self {
            segments,
            color,
            width,
        }
    }

    /// Paint a single wire segment
    fn paint_segment(&self, layers: &mut LayerSet, segment: &WireSegment) {
        let layer = layers.get_layer_mut(LayerId::Wire).unwrap();

        let stroke = Stroke::new(self.width, self.color);

        let polyline = Polyline::from_points(
            &[(segment.start.x, segment.start.y), (segment.end.x, segment.end.y)],
            stroke,
        );

        layer.add_element(LayerElement::new(LayerElementType::Polyline(polyline)));
    }
}

impl Painter for WirePainter {
    fn bbox(&self) -> BoundingBox {
        if self.segments.is_empty() {
            return BoundingBox::empty();
        }

        let mut bbox = BoundingBox::empty();
        for segment in &self.segments {
            bbox.expand_point(segment.start.x, segment.start.y);
            bbox.expand_point(segment.end.x, segment.end.y);
        }
        bbox
    }

    fn paint(&self, layers: &mut LayerSet) {
        for segment in &self.segments {
            self.paint_segment(layers, segment);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wire_segment() {
        let segment = WireSegment::new(Point::new(0.0, 0.0), Point::new(10.0, 5.0));
        assert!(!segment.is_horizontal());
        assert!(!segment.is_vertical());

        let horizontal = WireSegment::new(Point::new(0.0, 5.0), Point::new(10.0, 5.0));
        assert!(horizontal.is_horizontal());

        let vertical = WireSegment::new(Point::new(5.0, 0.0), Point::new(5.0, 10.0));
        assert!(vertical.is_vertical());
    }

    #[test]
    fn test_wire_bbox() {
        let segments = vec![
            WireSegment::new(Point::new(0.0, 0.0), Point::new(10.0, 5.0)),
            WireSegment::new(Point::new(5.0, 0.0), Point::new(15.0, 10.0)),
        ];

        let painter = WirePainter::new(segments, Color::black());
        let bbox = painter.bbox();

        assert_eq!(bbox.min_x(), 0.0);
        assert_eq!(bbox.min_y(), 0.0);
        assert_eq!(bbox.max_x(), 15.0);
        assert_eq!(bbox.max_y(), 10.0);
    }
}
