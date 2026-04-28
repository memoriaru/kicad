//! Label Painter - renders schematic labels to graphics primitives
//!
//! Handles local labels, global labels, hierarchical labels, etc.
//! Label shape geometry matches KiCanvas JS `GlobalLabelPainter.create_shape()`
//! and `HierarchicalLabelPainter.create_shape()` exactly.

use crate::render_core::{Point, Color, BoundingBox};
use crate::render_core::graphics::{Polygon, Stroke};
use crate::layer::{LayerSet, LayerId, LayerElement, LayerElementType};
use crate::constants;
use super::Painter;

/// Label type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelType {
    /// Local label (within schematic)
    Local,
    /// Global label (across schematics)
    Global,
    /// Hierarchical label (between sheets)
    Hierarchical,
}

/// Label shape (for global/hierarchical labels)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelShape {
    Input,
    Output,
    Bidirectional,
    TriState,
    Passive,
}

/// Label in a schematic
#[derive(Debug, Clone)]
pub struct Label {
    pub label_type: LabelType,
    pub position: Point,
    pub rotation: i32,
    pub text: String,
    pub shape: LabelShape,
    pub font_size: f64,
}

/// Rotate a point by angle in degrees (counter-clockwise in standard math coords).
fn rotate_point(x: f64, y: f64, angle_deg: f64) -> (f64, f64) {
    let rad = angle_deg.to_radians();
    let cos_a = rad.cos();
    let sin_a = rad.sin();
    (x * cos_a - y * sin_a, x * sin_a + y * cos_a)
}

/// Label Painter - renders labels to graphics
pub struct LabelPainter {
    pub label: Label,
    pub color: Color,
}

impl LabelPainter {
    pub fn new(label: Label, color: Color) -> Self {
        Self { label, color }
    }

    /// Paint the label text.
    /// For global/hierarchical labels, offset text past the shape so it doesn't overlap.
    /// KiCad places text to the right of the shape at the connection point.
    fn paint_label_text(&self, layers: &mut LayerSet) {
        let layer = layers.get_layer_mut(LayerId::Labels).unwrap();

        let text_width = self.label.text.len() as f64 * self.label.font_size * constants::CHAR_WIDTH_RATIO;

        // For global/hierarchical labels, offset text past the shape.
        // KiCad places text after the shape: offset ≈ font_size + margin.
        let shape_offset = match self.label.label_type {
            LabelType::Local => 0.0,
            LabelType::Global | LabelType::Hierarchical => {
                // Hierarchical shapes extend font_size from connection point.
                // Add text margin gap after the shape tip.
                self.label.font_size + constants::TEXT_MARGIN
            }
        };

        let pos = match self.label.rotation {
            0 => Point::new(self.label.position.x + shape_offset, self.label.position.y),
            180 => Point::new(self.label.position.x - text_width - shape_offset, self.label.position.y),
            90 => Point::new(self.label.position.x, self.label.position.y + shape_offset),
            270 => Point::new(self.label.position.x, self.label.position.y - text_width - shape_offset),
            _ => self.label.position,
        };

        layer.add_element(LayerElement::new(LayerElementType::Text {
            position: pos,
            text: self.label.text.clone(),
            font_size: self.label.font_size,
            color: self.color,
            bold: false,
            rotation: 0.0,
            text_anchor: "",
            dominant_baseline: "",
        }));
    }

    /// Paint global/hierarchical label shape.
    /// Uses completely different geometry for Global vs Hierarchical labels,
    /// matching JS `create_shape()` implementations exactly.
    fn paint_label_shape(&self, layers: &mut LayerSet) {
        if self.label.label_type == LabelType::Local {
            return;
        }

        let layer = layers.get_layer_mut(LayerId::Labels).unwrap();
        let pos = self.label.position;
        let stroke = Stroke::new(constants::LINE_WIDTH, self.color);

        match self.label.label_type {
            LabelType::Global => self.paint_global_shape(layer, pos, stroke),
            LabelType::Hierarchical => self.paint_hierarchical_shape(layer, pos, stroke),
            LabelType::Local => {}
        }
    }

    /// Global label shape — matches JS `GlobalLabelPainter.create_shape()`.
    ///
    /// Creates a rectangular flag extending from the connection point,
    /// with shape-specific modifications:
    /// - input: flag indented on left (connection point side)
    /// - output: flag indented on right (far side)
    /// - bidirectional/tri_state: indented on both sides
    /// - passive: plain rectangle
    fn paint_global_shape(
        &self,
        layer: &mut crate::layer::Layer,
        pos: Point,
        stroke: Stroke,
    ) {
        let text_height = self.label.font_size;
        let margin = text_height * constants::LABEL_SIZE_RATIO;
        let half_size = text_height / 2.0 + margin;
        // JS: symbol_length = schtext.get_text_box().w + 2 * margin
        // get_text_box().w ≈ text.len() * size.x (character cell width = font_size)
        let text_width = self.label.text.len() as f64 * self.label.font_size;
        let symbol_length = text_width + 2.0 * margin;
        let stroke_width = constants::LINE_WIDTH;
        let x = symbol_length + stroke_width;
        let y = half_size + stroke_width;

        // Base rectangle flag: connection point at origin, flag extends in -x direction
        // Points in local coords (before rotation):
        //   (0,0) → (0,-y) → (-x,-y) → (-x,0) → (-x,y) → (0,y) → (0,0)
        let mut pts = vec![
            (0.0, 0.0),
            (0.0, -y),
            (-x, -y),
            (-x, 0.0),
            (-x, y),
            (0.0, y),
            (0.0, 0.0),
        ];

        let mut offset_x = 0.0;

        match self.label.shape {
            LabelShape::Input => {
                offset_x = -half_size;
                pts[0].0 += half_size;
                pts[6].0 += half_size;
            }
            LabelShape::Output => {
                pts[3].0 -= half_size;
            }
            LabelShape::Bidirectional | LabelShape::TriState => {
                offset_x = -half_size;
                pts[0].0 += half_size;
                pts[6].0 += half_size;
                pts[3].0 -= half_size;
            }
            LabelShape::Passive => {}
        }

        // Apply offset and rotation, then translate to world position
        let angle_deg = (self.label.rotation as f64) + 180.0;
        let world_pts: Vec<(f64, f64)> = pts
            .iter()
            .map(|(px, py)| {
                let ox = px + offset_x;
                let oy = *py;
                let (rx, ry) = rotate_point(ox, oy, angle_deg);
                (rx + pos.x, ry + pos.y)
            })
            .collect();

        let polygon = Polygon::from_points(&world_pts)
            .with_stroke(stroke);

        layer.add_element(LayerElement::new(LayerElementType::Polygon(polygon)));
    }

    /// Hierarchical label shape — matches JS `HierarchicalLabelPainter.create_shape()`.
    ///
    /// Creates directional arrow/diamond shapes based on label shape type:
    /// - output: right-pointing arrow (pentagon)
    /// - input: left-pointing arrow (pentagon)
    /// - bidirectional/tri_state: diamond (4 points + close)
    /// - passive: simple rectangle
    fn paint_hierarchical_shape(
        &self,
        layer: &mut crate::layer::Layer,
        pos: Point,
        stroke: Stroke,
    ) {
        // JS: s = schtext.text_width = attributes.size.x = single character cell width.
        // In our mm units, this equals font_size. NOT the total text string width.
        let s = self.label.font_size;
        let s2 = s / 2.0;

        let pts: Vec<(f64, f64)> = match self.label.shape {
            LabelShape::Output => vec![
                (0.0, s2),
                (s2, s2),
                (s, 0.0),
                (s2, -s2),
                (0.0, -s2),
                (0.0, s2),
            ],
            LabelShape::Input => vec![
                (s, s2),
                (s2, s2),
                (0.0, 0.0),
                (s2, -s2),
                (s, -s2),
                (s, s2),
            ],
            LabelShape::Bidirectional | LabelShape::TriState => vec![
                (s2, s2),
                (s, 0.0),
                (s2, -s2),
                (0.0, 0.0),
                (s2, s2),
            ],
            LabelShape::Passive => vec![
                (0.0, s2),
                (s, s2),
                (s, -s2),
                (0.0, -s2),
                (0.0, s2),
            ],
        };

        let angle_deg = self.label.rotation as f64;
        let world_pts: Vec<(f64, f64)> = pts
            .iter()
            .map(|(px, py)| {
                let (rx, ry) = rotate_point(*px, *py, angle_deg);
                (rx + pos.x, ry + pos.y)
            })
            .collect();

        let polygon = Polygon::from_points(&world_pts)
            .with_stroke(stroke);

        layer.add_element(LayerElement::new(LayerElementType::Polygon(polygon)));
    }
}

impl Painter for LabelPainter {
    fn bbox(&self) -> BoundingBox {
        let text_width = self.label.text.len() as f64 * self.label.font_size * constants::CHAR_WIDTH_RATIO;
        let text_height = self.label.font_size;

        BoundingBox::from_min_max(
            self.label.position.x,
            self.label.position.y,
            self.label.position.x + text_width,
            self.label.position.y + text_height,
        )
    }

    fn paint(&self, layers: &mut LayerSet) {
        self.paint_label_text(layers);
        self.paint_label_shape(layers);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_label_bbox() {
        let label = Label {
            label_type: LabelType::Local,
            position: Point::new(10.0, 20.0),
            rotation: 0,
            text: "NET_NAME".to_string(),
            shape: LabelShape::Passive,
            font_size: 1.27,
        };

        let painter = LabelPainter::new(label, Color::black());
        let bbox = painter.bbox();

        assert!(bbox.min_x() <= 10.0);
        assert!(bbox.max_x() >= 10.0);
    }

    #[test]
    fn test_rotate_point() {
        // 90 degrees: (1,0) → (0,1)
        let (rx, ry) = rotate_point(1.0, 0.0, 90.0);
        assert!((rx - 0.0).abs() < 1e-6);
        assert!((ry - 1.0).abs() < 1e-6);

        // 180 degrees: (1,0) → (-1,0)
        let (rx, ry) = rotate_point(1.0, 0.0, 180.0);
        assert!((rx - (-1.0)).abs() < 1e-6);
        assert!((ry - 0.0).abs() < 1e-6);
    }
}
