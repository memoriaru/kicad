//! Symbol Painter - renders schematic symbols to graphics primitives
//!
//! Based on KiCanvas JS `SymbolPainter` class

use kicad_json5::ir::{self, GraphicElement};

use crate::render_core::{Point, Color, BoundingBox};
use crate::render_core::graphics::{Circle as RcCircle, Arc as RcArc, Polyline as RcPolyline,
    Polygon as RcPolygon, Stroke, Fill};
use crate::layer::{LayerSet, LayerId, LayerElement, LayerElementType};
use crate::bridge;
use crate::constants;
use super::Painter;
use super::pin_painter::{PinPainter, PinGraphic};

/// Symbol instance in a schematic
#[derive(Debug, Clone)]
pub struct SymbolInstance {
    /// Symbol library ID
    pub library_id: String,
    /// Position
    pub position: Point,
    /// Rotation angle in degrees
    pub rotation: i32,
    /// Mirror mode
    pub mirror: Mirror,
    /// Unit number (for multi-unit symbols)
    pub unit: i32,
    /// Pins
    pub pins: Vec<PinGraphic>,
    /// Reference designator
    pub reference: String,
    /// Value/part number
    pub value: String,
    /// Reference text position (x, y) in KiCad schematic coordinates, None = auto
    pub reference_position: Option<(f64, f64)>,
    /// Reference text rotation in degrees
    pub reference_rotation: f64,
    /// Reference text horizontal alignment ("start", "middle", "end")
    pub reference_h_align: String,
    /// Reference text vertical alignment
    pub reference_v_align: String,
    /// Value text position (x, y) in KiCad schematic coordinates, None = auto
    pub value_position: Option<(f64, f64)>,
    /// Value text rotation in degrees
    pub value_rotation: f64,
    /// Value text horizontal alignment
    pub value_h_align: String,
    /// Value text vertical alignment
    pub value_v_align: String,
    /// Whether the reference property is hidden
    pub reference_hidden: bool,
    /// Whether the value property is hidden
    pub value_hidden: bool,
}

/// Mirror mode for symbols
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mirror {
    None,
    X,
    Y,
}

/// Symbol Painter - renders symbols to graphics
pub struct SymbolPainter {
    /// Symbol instance data
    pub symbol: SymbolInstance,
    /// Graphics from library symbol (body shapes)
    pub body_graphics: Vec<GraphicElement>,
    /// Wire color
    pub wire_color: Color,
    /// Component outline color
    pub outline_color: Color,
    /// Pin color
    pub pin_color: Color,
    /// Reference color (cyan in KiCad)
    pub reference_color: Color,
    /// Value color (cyan in KiCad)
    pub value_color: Color,
}

impl SymbolPainter {
    /// Create a new symbol painter
    pub fn new(symbol: SymbolInstance) -> Self {
        Self {
            symbol,
            body_graphics: Vec::new(),
            wire_color: constants::wire_color(),
            outline_color: constants::component_outline_color(),
            pin_color: constants::pin_color(),
            reference_color: constants::reference_color(),
            value_color: constants::value_color(),
        }
    }

    /// Create a symbol painter with library graphics
    pub fn with_graphics(symbol: SymbolInstance, graphics: Vec<GraphicElement>) -> Self {
        Self {
            symbol,
            body_graphics: graphics,
            wire_color: constants::wire_color(),
            outline_color: constants::component_outline_color(),
            pin_color: constants::pin_color(),
            reference_color: constants::reference_color(),
            value_color: constants::value_color(),
        }
    }

    /// Get the symbol transformation matrix
    ///
    /// Matches JS `get_symbol_transform()`: includes Y-flip (library Y-UP →
    /// schematic Y-DOWN) baked into the rotation matrix, then translation to
    /// the symbol's schematic position.
    pub fn transform(&self) -> crate::render_core::Matrix {
        let rot_mirror = get_symbol_transform(self.symbol.rotation, &self.symbol.mirror);
        let translation = crate::render_core::Matrix::translation(
            self.symbol.position.x,
            self.symbol.position.y,
        );
        translation.multiply(&rot_mirror)
    }

    /// Paint symbol body graphics (rectangles, circles, arcs, polylines).
    /// Two-pass rendering: background-filled shapes first (as base layer),
    /// then all other graphics on top. This prevents the yellow body fill
    /// from covering small pin-adjacent rectangles.
    fn paint_body(&self, layers: &mut LayerSet) {
        let transform = self.transform();
        let bg_layer = layers.get_layer_mut(&LayerId::symbol_background());

        if let Some(layer) = bg_layer {
            // Pass 1: background-filled rectangles (symbol body fill)
            for ge in &self.body_graphics {
                if let GraphicElement::Rectangle(ir_rect) = ge {
                    if ir_rect.fill.fill_type == kicad_json5::ir::FillType::Background {
                        let mut rc_poly = bridge::convert_rectangle_with_fill(ir_rect, self.outline_color, self.outline_color);
                        let transformed_points: Vec<(f64, f64)> = rc_poly.points
                            .iter()
                            .map(|p| {
                                let tp = transform.transform(p);
                                (tp.x, tp.y)
                            })
                            .collect();
                        rc_poly.points = transformed_points.into_iter()
                            .map(|(x, y)| Point::new(x, y))
                            .collect();
                        layer.add_element(LayerElement::new(
                            LayerElementType::Polygon(rc_poly),
                        ));
                    }
                }
            }

            // Pass 2: all other graphics
            for ge in &self.body_graphics {
                match ge {
                    GraphicElement::Polyline(ir_poly) => {
                        let mut rc_poly = bridge::convert_polyline_with_color(ir_poly, self.outline_color);
                        // Transform all points
                        let transformed_points: Vec<(f64, f64)> = rc_poly.points
                            .iter()
                            .map(|p| {
                                let tp = transform.transform(p);
                                (tp.x, tp.y)
                            })
                            .collect();
                        rc_poly.points = transformed_points.into_iter()
                            .map(|(x, y)| Point::new(x, y))
                            .collect();
                        layer.add_element(LayerElement::new(
                            LayerElementType::Polyline(rc_poly),
                        ));
                    }
                    GraphicElement::Rectangle(ir_rect) => {
                        // Skip background rects already rendered in pass 1
                        if ir_rect.fill.fill_type == kicad_json5::ir::FillType::Background {
                            continue;
                        }
                        let mut rc_poly = bridge::convert_rectangle_with_fill(ir_rect, self.outline_color, self.outline_color);
                        // Transform all points
                        let transformed_points: Vec<(f64, f64)> = rc_poly.points
                            .iter()
                            .map(|p| {
                                let tp = transform.transform(p);
                                (tp.x, tp.y)
                            })
                            .collect();
                        rc_poly.points = transformed_points.into_iter()
                            .map(|(x, y)| Point::new(x, y))
                            .collect();
                        layer.add_element(LayerElement::new(
                            LayerElementType::Polygon(rc_poly),
                        ));
                    }
                    GraphicElement::Circle(ir_circle) => {
                        let rc_circle = bridge::convert_circle_with_fill(ir_circle, self.outline_color, self.outline_color);
                        let center = transform.transform(&rc_circle.center);
                        let mut transformed = RcCircle::new(center, rc_circle.radius)
                            .with_fill(rc_circle.fill);
                        if let Some(s) = rc_circle.stroke {
                            transformed = transformed.with_stroke(s);
                        }
                        layer.add_element(LayerElement::new(
                            LayerElementType::Circle(transformed),
                        ));
                    }
                    GraphicElement::Arc(ir_arc) => {
                        // Transform all 3 defining points through the symbol matrix,
                        // then recalculate center/radius/angles from transformed points.
                        // Only transforming the center would leave angles wrong for rotated symbols.
                        let start_p = transform.transform(&Point::new(ir_arc.start.0, ir_arc.start.1));
                        let mid_p = transform.transform(&Point::new(ir_arc.mid.0, ir_arc.mid.1));
                        let end_p = transform.transform(&Point::new(ir_arc.end.0, ir_arc.end.1));

                        let fill = bridge::convert_fill_with_outline(&ir_arc.fill, self.outline_color);
                        if let Some(rc_arc) = compute_arc_from_points(
                            start_p, mid_p, end_p,
                            bridge::convert_stroke_solid(&ir_arc.stroke, self.outline_color),
                            fill,
                        ) {
                            layer.add_element(LayerElement::new(
                                LayerElementType::Arc(rc_arc),
                            ));
                        }
                    }
                    GraphicElement::Text(ir_text) => {
                        let pos = transform.transform(&Point::new(ir_text.position.0, ir_text.position.1));
                        let font_size = ir_text.effects.font.size.1.max(ir_text.effects.font.size.0);
                        if !ir_text.effects.hide {
                            layer.add_element(LayerElement::new(LayerElementType::Text {
                                position: pos,
                                text: ir_text.text.clone(),
                                font_size: if font_size > 0.0 { font_size } else { constants::TEXT_SIZE },
                                color: self.outline_color,
                                bold: false,
                                rotation: 0.0,
                                text_anchor: String::new(),
                                dominant_baseline: String::new(),
                            }));
                        }
                    }
                    GraphicElement::Pin(_) => {
                        // Pins are handled separately by paint_pins
                    }
                }
            }
        }
    }

    /// Paint all pins
    fn paint_pins(&self, layers: &mut LayerSet) {
        let transform = self.transform();

        for pin in &self.symbol.pins {
            let painter = PinPainter::new(
                pin.clone(),
                transform.clone(),
                self.pin_color,  // JS: pin line/shape uses theme.pin (red)
                self.pin_color,
            );
            painter.paint(layers);
        }
    }

    /// Paint reference text
    fn paint_reference(&self, layers: &mut LayerSet) {
        if self.symbol.reference.is_empty() || self.symbol.reference_hidden || self.symbol.reference.starts_with('#') {
            return;
        }

        let layer = layers.get_layer_mut(&LayerId::symbol_foreground()).unwrap();

        let pos = if let Some((rx, ry)) = self.symbol.reference_position {
            Point::new(rx, ry)
        } else {
            Point::new(self.symbol.position.x, self.symbol.position.y - 2.54)
        };

        layer.add_element(LayerElement::new(LayerElementType::Text {
            position: pos,
            text: self.symbol.reference.clone(),
            font_size: constants::TEXT_SIZE,
            color: self.reference_color,
            bold: false,
            rotation: draw_rotation(self.symbol.rotation, self.symbol.reference_rotation),
            text_anchor: self.symbol.reference_h_align.clone(),
            dominant_baseline: self.symbol.reference_v_align.clone(),
        }));
    }

    /// Paint value text
    fn paint_value(&self, layers: &mut LayerSet) {
        if self.symbol.value.is_empty() || self.symbol.value_hidden {
            return;
        }

        let layer = layers.get_layer_mut(&LayerId::symbol_foreground()).unwrap();

        let pos = if let Some((vx, vy)) = self.symbol.value_position {
            // Use actual KiCad property position (already in schematic coords)
            Point::new(vx, vy)
        } else {
            // Fallback: below symbol
            Point::new(self.symbol.position.x, self.symbol.position.y + 2.54)
        };

        layer.add_element(LayerElement::new(LayerElementType::Text {
            position: pos,
            text: self.symbol.value.clone(),
            font_size: constants::TEXT_SIZE,
            color: self.value_color,
            bold: false,
            rotation: draw_rotation(self.symbol.rotation, self.symbol.value_rotation),
            text_anchor: self.symbol.value_h_align.clone(),
            dominant_baseline: self.symbol.value_v_align.clone(),
        }));
    }
}

impl Painter for SymbolPainter {
    fn layers(&self) -> Vec<LayerId> {
        vec![
            LayerId::symbol_background(),
            LayerId::symbol_pin(),
            LayerId::symbol_foreground(),
        ]
    }

    fn bbox(&self) -> BoundingBox {
        let mut bbox = BoundingBox::empty();

        // Include all pins
        let transform = self.transform();
        for pin in &self.symbol.pins {
            let start = transform.transform(&pin.position);
            let end = transform.transform(&pin.end_position());
            bbox.expand_point(start.x, start.y);
            bbox.expand_point(end.x, end.y);
        }

        // Include body graphics
        for ge in &self.body_graphics {
            match ge {
                GraphicElement::Polyline(ir_poly) => {
                    for (x, y) in &ir_poly.points {
                        let p = transform.transform(&Point::new(*x, *y));
                        bbox.expand_point(p.x, p.y);
                    }
                }
                GraphicElement::Rectangle(ir_rect) => {
                    for &(x, y) in &[ir_rect.start, ir_rect.end] {
                        let p = transform.transform(&Point::new(x, y));
                        bbox.expand_point(p.x, p.y);
                    }
                }
                GraphicElement::Circle(ir_circle) => {
                    let c = transform.transform(&Point::new(ir_circle.center.0, ir_circle.center.1));
                    bbox.expand_point(c.x - ir_circle.radius, c.y - ir_circle.radius);
                    bbox.expand_point(c.x + ir_circle.radius, c.y + ir_circle.radius);
                }
                _ => {}
            }
        }

        // Add padding for reference/value text
        bbox = bbox.with_padding(2.54);

        bbox
    }

    fn paint(&self, layers: &mut LayerSet) {
        self.paint_body(layers);
        self.paint_pins(layers);
        self.paint_reference(layers);
        self.paint_value(layers);
    }
}

/// Helper function to get symbol transform matrix (matching JS get_symbol_transform)
pub fn get_symbol_transform(rotation: i32, mirror: &Mirror) -> crate::render_core::Matrix {
    let (a, b, c, d) = match rotation % 360 {
        0 => (1.0, 0.0, 0.0, -1.0),
        90 => (0.0, -1.0, -1.0, 0.0),
        180 => (-1.0, 0.0, 0.0, 1.0),
        270 => (0.0, 1.0, 1.0, 0.0),
        _ => (1.0, 0.0, 0.0, -1.0),
    };

    match mirror {
        Mirror::X => crate::render_core::Matrix::new([a, b, -c, -d, 0.0, 0.0]),
        Mirror::Y => crate::render_core::Matrix::new([-a, -b, c, d, 0.0, 0.0]),
        Mirror::None => crate::render_core::Matrix::new([a, b, c, d, 0.0, 0.0]),
    }
}

/// Compute the effective draw rotation for a property text, matching JS `SchField.draw_rotation`.
///
/// JS logic: check if the symbol transform's element[1] has |value| == 1,
/// which indicates 90° or 270° symbol rotation. If so, swap the text angle:
/// - 0°/180° → 90°
/// - 90°/270° → 0°
/// Otherwise, keep the property's own angle.
fn draw_rotation(symbol_rotation: i32, property_angle_deg: f64) -> f64 {
    let matrix = get_symbol_transform(symbol_rotation, &Mirror::None);
    // JS checks parent_transform.elements[1] which is matrix.elements[1]
    if matrix.elements[1].abs() == 1.0 {
        let deg = property_angle_deg % 360.0;
        if (deg - 0.0).abs() < 0.5 || (deg - 180.0).abs() < 0.5 {
            90.0
        } else {
            0.0
        }
    } else {
        property_angle_deg
    }
}

/// Compute an Arc from 3 points (start, mid, end) after they've been transformed.
fn compute_arc_from_points(
    start: Point, mid: Point, end: Point, stroke: Stroke, fill: Fill,
) -> Option<RcArc> {
    let (x1, y1) = (start.x, start.y);
    let (x2, y2) = (mid.x, mid.y);
    let (x3, y3) = (end.x, end.y);

    let ma = x2 - x1;
    let mb = y2 - y1;
    let mc = x3 - x2;
    let md = y3 - y2;
    let det = ma * md - mb * mc;
    if det.abs() < 1e-10 {
        return None;
    }

    let x1_sq = x1 * x1 + y1 * y1;
    let x2_sq = x2 * x2 + y2 * y2;
    let x3_sq = x3 * x3 + y3 * y3;

    let cx = (x1_sq * (y2 - y3) + x2_sq * (y3 - y1) + x3_sq * (y1 - y2)) / (2.0 * det);
    let cy = (x1_sq * (x3 - x2) + x2_sq * (x1 - x3) + x3_sq * (x2 - x1)) / (2.0 * det);
    let radius = ((cx - x1).powi(2) + (cy - y1).powi(2)).sqrt();
    let start_angle = (y1 - cy).atan2(x1 - cx);
    let end_angle = (y3 - cy).atan2(x3 - cx);

    Some(RcArc::new(
        Point::new(cx, cy),
        radius,
        start_angle,
        end_angle,
        stroke,
    ).with_fill(fill))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_symbol_transform() {
        let matrix = get_symbol_transform(0, &Mirror::None);
        let p = Point::new(1.0, 2.0);
        let transformed = matrix.transform(&p);
        assert!((transformed.x - 1.0).abs() < 1e-6);
        assert!((transformed.y - (-2.0)).abs() < 1e-6);

        let matrix = get_symbol_transform(0, &Mirror::X);
        let p = Point::new(1.0, 2.0);
        let transformed = matrix.transform(&p);
        assert!((transformed.x - 1.0).abs() < 1e-6);
        assert!((transformed.y - 2.0).abs() < 1e-6);

        let matrix = get_symbol_transform(0, &Mirror::Y);
        let p = Point::new(1.0, 2.0);
        let transformed = matrix.transform(&p);
        assert!((transformed.x - (-1.0)).abs() < 1e-6);
        assert!((transformed.y - (-2.0)).abs() < 1e-6);
    }
}
