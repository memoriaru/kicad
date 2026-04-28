//! Symbol Painter - renders schematic symbols to graphics primitives
//!
//! Based on KiCanvas JS `SymbolPainter` class

use kicad_json5::ir::GraphicElement;

use crate::render_core::{Point, Color, BoundingBox, Matrix};
use crate::render_core::graphics::{Circle as RcCircle, Arc as RcArc, Stroke, Fill};
use crate::layer::{LayerSet, LayerId, LayerElement, LayerElementType};
use crate::bridge;
use crate::constants;
use super::Painter;
use super::pin_painter::{PinPainter, PinGraphic};

/// Symbol instance in a schematic
#[derive(Debug, Clone)]
pub struct SymbolInstance {
    pub library_id: String,
    pub position: Point,
    pub rotation: i32,
    pub mirror: Mirror,
    pub unit: i32,
    pub pins: Vec<PinGraphic>,
    pub reference: String,
    pub value: String,
    pub reference_position: Option<(f64, f64)>,
    pub reference_rotation: f64,
    pub reference_h_align: &'static str,
    pub reference_v_align: &'static str,
    pub value_position: Option<(f64, f64)>,
    pub value_rotation: f64,
    pub value_h_align: &'static str,
    pub value_v_align: &'static str,
    pub reference_hidden: bool,
    pub value_hidden: bool,
    /// Do Not Populate — draw an X cross over the symbol
    pub dnp: bool,
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
    pub symbol: SymbolInstance,
    pub body_graphics: Vec<GraphicElement>,
    pub wire_color: Color,
    pub outline_color: Color,
    pub pin_color: Color,
    pub reference_color: Color,
    pub value_color: Color,
}

/// Transform a slice of points through a matrix, returning Vec<Point>.
fn transform_points(pts: &[Point], transform: &Matrix) -> Vec<Point> {
    pts.iter().map(|p| transform.transform(p)).collect()
}

impl SymbolPainter {
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

    /// Get the symbol transformation matrix.
    /// Includes Y-flip (library Y-UP → schematic Y-DOWN) baked into the rotation matrix,
    /// then translation to the symbol's schematic position.
    pub fn transform(&self) -> Matrix {
        let rot_mirror = get_symbol_transform(self.symbol.rotation, &self.symbol.mirror);
        let translation = Matrix::translation(self.symbol.position.x, self.symbol.position.y);
        translation.multiply(&rot_mirror)
    }

    /// Paint symbol body graphics. Two-pass: background-filled shapes first, then all others.
    fn paint_body(&self, layers: &mut LayerSet) {
        let transform = self.transform();
        let bg_layer = layers.get_layer_mut(LayerId::SymbolBackground);

        if let Some(layer) = bg_layer {
            // Pass 1: background-filled rectangles
            for ge in &self.body_graphics {
                if let GraphicElement::Rectangle(ir_rect) = ge {
                    if ir_rect.fill.fill_type == kicad_json5::ir::FillType::Background {
                        let mut rc_poly = bridge::convert_rectangle_with_fill(ir_rect, self.outline_color, self.outline_color);
                        rc_poly.points = transform_points(&rc_poly.points, &transform);
                        layer.add_element(LayerElement::new(LayerElementType::Polygon(rc_poly)));
                    }
                }
            }

            // Pass 2: all other graphics
            for ge in &self.body_graphics {
                match ge {
                    GraphicElement::Polyline(ir_poly) => {
                        let mut rc_poly = bridge::convert_polyline_with_color(ir_poly, self.outline_color);
                        rc_poly.points = transform_points(&rc_poly.points, &transform);
                        layer.add_element(LayerElement::new(LayerElementType::Polyline(rc_poly)));
                    }
                    GraphicElement::Rectangle(ir_rect) => {
                        if ir_rect.fill.fill_type == kicad_json5::ir::FillType::Background {
                            continue;
                        }
                        let mut rc_poly = bridge::convert_rectangle_with_fill(ir_rect, self.outline_color, self.outline_color);
                        rc_poly.points = transform_points(&rc_poly.points, &transform);
                        layer.add_element(LayerElement::new(LayerElementType::Polygon(rc_poly)));
                    }
                    GraphicElement::Circle(ir_circle) => {
                        let rc_circle = bridge::convert_circle_with_fill(ir_circle, self.outline_color, self.outline_color);
                        let center = transform.transform(&rc_circle.center);
                        let mut transformed = RcCircle::new(center, rc_circle.radius)
                            .with_fill(rc_circle.fill);
                        if let Some(s) = rc_circle.stroke {
                            transformed = transformed.with_stroke(s);
                        }
                        layer.add_element(LayerElement::new(LayerElementType::Circle(transformed)));
                    }
                    GraphicElement::Arc(ir_arc) => {
                        let start_p = transform.transform(&Point::new(ir_arc.start.0, ir_arc.start.1));
                        let mid_p = transform.transform(&Point::new(ir_arc.mid.0, ir_arc.mid.1));
                        let end_p = transform.transform(&Point::new(ir_arc.end.0, ir_arc.end.1));

                        let fill = bridge::convert_fill_with_outline(&ir_arc.fill, self.outline_color);
                        if let Some(rc_arc) = compute_arc_from_points(
                            start_p, mid_p, end_p,
                            bridge::convert_stroke_solid(&ir_arc.stroke, self.outline_color),
                            fill,
                        ) {
                            layer.add_element(LayerElement::new(LayerElementType::Arc(rc_arc)));
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
                                text_anchor: "",
                                dominant_baseline: "",
                            }));
                        }
                    }
                    GraphicElement::Pin(_) => {}
                }
            }
        }
    }

    fn paint_pins(&self, layers: &mut LayerSet) {
        let transform = self.transform();

        for pin in &self.symbol.pins {
            let painter = PinPainter::new(
                pin.clone(),
                transform.clone(),
                self.pin_color,
                self.pin_color,
            );
            painter.paint(layers);
        }
    }

    /// Paint a DNP (Do Not Populate) cross over the symbol body.
    /// KiCad draws two diagonal lines forming an X across the body bounding box (pins excluded).
    fn paint_dnp_cross(&self, layers: &mut LayerSet) {
        let layer = layers.get_layer_mut(LayerId::SymbolForeground).unwrap();
        let body_bbox = self.body_bbox();
        if body_bbox.is_empty() {
            return;
        }
        let stroke = Stroke::new(constants::LINE_WIDTH, constants::component_outline_color());

        let top_left = (body_bbox.min_x(), body_bbox.min_y());
        let bottom_right = (body_bbox.max_x(), body_bbox.max_y());
        let top_right = (body_bbox.max_x(), body_bbox.min_y());
        let bottom_left = (body_bbox.min_x(), body_bbox.max_y());

        let line1 = crate::render_core::graphics::Polyline::from_points(&[top_left, bottom_right], stroke.clone());
        layer.add_element(LayerElement::new(LayerElementType::Polyline(line1)));

        let line2 = crate::render_core::graphics::Polyline::from_points(&[top_right, bottom_left], stroke);
        layer.add_element(LayerElement::new(LayerElementType::Polyline(line2)));
    }

    /// Bounding box of the symbol body graphics only (no pins, no padding).
    fn body_bbox(&self) -> BoundingBox {
        let mut bbox = BoundingBox::empty();
        let transform = self.transform();

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
                GraphicElement::Arc(ir_arc) => {
                    let s = transform.transform(&Point::new(ir_arc.start.0, ir_arc.start.1));
                    let m = transform.transform(&Point::new(ir_arc.mid.0, ir_arc.mid.1));
                    let e = transform.transform(&Point::new(ir_arc.end.0, ir_arc.end.1));
                    for p in [s, m, e] {
                        bbox.expand_point(p.x, p.y);
                    }
                }
                _ => {}
            }
        }

        bbox
    }

    /// Compute the draw position for a property text.
    /// Paint a property (reference or value) text.
    fn paint_property(
        &self,
        layers: &mut LayerSet,
        text: &str,
        pos: Option<(f64, f64)>,
        fallback_offset_y: f64,
        prop_rotation: f64,
        h_align: &str,
        v_align: &str,
        hidden: bool,
        skip_if_hash: bool,
        color: Color,
    ) {
        if text.is_empty() || hidden || (skip_if_hash && text.starts_with('#')) {
            return;
        }

        let layer = layers.get_layer_mut(LayerId::SymbolForeground).unwrap();

        let anchor_pos = if let Some((x, y)) = pos {
            Point::new(x, y)
        } else {
            Point::new(self.symbol.position.x, self.symbol.position.y + fallback_offset_y)
        };

        // Property angle from the sch file needs draw_rotation adjustment:
        // when the symbol is rotated 90/270, prop angles 0/180 render at 90°
        // and prop angles 90/270 render at 0° (to keep text readable).
        let orient = draw_rotation(self.symbol.rotation, prop_rotation);

        let center = anchor_pos;

        layer.add_element(LayerElement::new(LayerElementType::Text {
            position: center,
            text: text.to_string(),
            font_size: constants::TEXT_SIZE,
            color,
            bold: false,
            rotation: orient,
            text_anchor: "middle",
            dominant_baseline: "central",
        }));
    }
}

impl Painter for SymbolPainter {
    fn bbox(&self) -> BoundingBox {
        let mut bbox = BoundingBox::empty();
        let transform = self.transform();

        for pin in &self.symbol.pins {
            let start = transform.transform(&pin.position);
            let end = transform.transform(&pin.end_position());
            bbox.expand_point(start.x, start.y);
            bbox.expand_point(end.x, end.y);
        }

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

        bbox.with_padding(2.54)
    }

    fn paint(&self, layers: &mut LayerSet) {
        self.paint_body(layers);
        self.paint_pins(layers);
        if self.symbol.dnp {
            self.paint_dnp_cross(layers);
        }
        self.paint_property(
            layers,
            &self.symbol.reference,
            self.symbol.reference_position,
            -2.54,
            self.symbol.reference_rotation,
            self.symbol.reference_h_align,
            self.symbol.reference_v_align,
            self.symbol.reference_hidden,
            true,
            self.reference_color,
        );
        self.paint_property(
            layers,
            &self.symbol.value,
            self.symbol.value_position,
            2.54,
            self.symbol.value_rotation,
            self.symbol.value_h_align,
            self.symbol.value_v_align,
            self.symbol.value_hidden,
            false,
            self.value_color,
        );
    }
}

/// Helper function to get symbol transform matrix (matching JS get_symbol_transform)
pub fn get_symbol_transform(rotation: i32, mirror: &Mirror) -> Matrix {
    let (a, b, c, d) = match rotation % 360 {
        0 => (1.0, 0.0, 0.0, -1.0),
        90 => (0.0, -1.0, -1.0, 0.0),
        180 => (-1.0, 0.0, 0.0, 1.0),
        270 => (0.0, 1.0, 1.0, 0.0),
        _ => (1.0, 0.0, 0.0, -1.0),
    };

    match mirror {
        Mirror::X => Matrix::new([a, b, -c, -d, 0.0, 0.0]),
        Mirror::Y => Matrix::new([-a, -b, c, d, 0.0, 0.0]),
        Mirror::None => Matrix::new([a, b, c, d, 0.0, 0.0]),
    }
}

/// Compute the effective draw rotation for a property text.
/// When the symbol is at 90/270, text orientations swap to stay readable:
/// prop angles 0/180 → draw at 90°, prop angles 90/270 → draw at 0°.
fn draw_rotation(symbol_rotation: i32, property_angle_deg: f64) -> f64 {
    let is_90_or_270 = matches!(symbol_rotation % 360, 90 | 270);
    if is_90_or_270 {
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
