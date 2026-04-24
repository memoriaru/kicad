//! Junction Painter - renders schematic junction dots to graphics primitives
//!
//! A junction is a filled circle that indicates wires/pins are connected

use crate::render_core::{Point, Color, BoundingBox};
use crate::render_core::graphics::{Circle, Fill};
use crate::layer::{LayerSet, LayerId, LayerElement, LayerElementType};
use crate::constants;
use super::Painter;

/// Junction - a connection point between wires
#[derive(Debug, Clone)]
pub struct Junction {
    /// Position of the junction
    pub position: Point,
    /// Diameter of the junction dot in mm (default: 0.016mm = 40 mils)
    pub diameter: f64,
}

impl Junction {
    /// Create a new junction at the given position
    pub fn new(position: Point) -> Self {
        Self {
            position,
            diameter: constants::JUNCTION_DIAMETER,
        }
    }

    /// Create a junction with custom diameter
    pub fn with_diameter(position: Point, diameter: f64) -> Self {
        Self { position, diameter }
    }

    /// Get the radius
    pub fn radius(&self) -> f64 {
        self.diameter / 2.0
    }
}

/// Junction Painter - renders junction dots
pub struct JunctionPainter {
    /// Junctions to paint
    pub junctions: Vec<Junction>,
    /// Color of the junction (usually wire color)
    pub color: Color,
}

impl JunctionPainter {
    /// Create a new junction painter
    pub fn new(junctions: Vec<Junction>, color: Color) -> Self {
        Self { junctions, color }
    }

    /// Add a junction
    pub fn add_junction(&mut self, junction: Junction) {
        self.junctions.push(junction);
    }

    /// Paint a single junction
    fn paint_junction(&self, layers: &mut LayerSet, junction: &Junction) {
        let layer = layers.get_layer_mut(&LayerId::junctions()).unwrap();

        let circle = Circle::new(junction.position, junction.radius())
            .with_fill(Fill::solid(self.color));

        layer.add_element(LayerElement::new(LayerElementType::Circle(circle)));
    }
}

impl Painter for JunctionPainter {
    fn layers(&self) -> Vec<LayerId> {
        vec![LayerId::junctions()]
    }

    fn bbox(&self) -> BoundingBox {
        if self.junctions.is_empty() {
            return BoundingBox::empty();
        }

        let mut bbox = BoundingBox::empty();
        for junction in &self.junctions {
            let r = junction.radius();
            bbox.expand_point(junction.position.x - r, junction.position.y - r);
            bbox.expand_point(junction.position.x + r, junction.position.y + r);
        }
        bbox
    }

    fn paint(&self, layers: &mut LayerSet) {
        for junction in &self.junctions {
            self.paint_junction(layers, junction);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_junction_creation() {
        let junction = Junction::new(Point::new(10.0, 20.0));
        assert_eq!(junction.position.x, 10.0);
        assert_eq!(junction.position.y, 20.0);
        assert_eq!(junction.radius(), constants::JUNCTION_DIAMETER / 2.0);
    }

    #[test]
    fn test_junction_bbox() {
        let junctions = vec![
            Junction::new(Point::new(10.0, 20.0)),
            Junction::new(Point::new(30.0, 40.0)),
        ];

        let painter = JunctionPainter::new(junctions, Color::black());
        let bbox = painter.bbox();

        let r = constants::JUNCTION_DIAMETER / 2.0;
        assert!(bbox.min_x() <= 10.0 - r);
        assert!(bbox.max_x() >= 30.0 + r);
    }
}
