//! Pin Painter - renders schematic pins to graphics primitives
//!
//! Based on KiCanvas JS `PinPainter` class

use crate::render_core::{Point, Color, Matrix, BoundingBox};
use crate::render_core::graphics::{Circle, Polyline, Stroke};
use crate::layer::{LayerSet, LayerId, LayerElement, LayerElementType};
use crate::constants;
use super::Painter;

/// Pin orientation (matches JS angle_to_orientation)
///
/// In KiCad library Y-UP coordinate system:
/// - 0° = Right, 90° = Up, 180° = Left, 270° = Down
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinOrientation {
    Right,
    Up,
    Left,
    Down,
}

impl PinOrientation {
    /// Convert from KiCad rotation angle
    pub fn from_angle(angle: i32) -> Self {
        match angle % 360 {
            0 => PinOrientation::Right,
            90 => PinOrientation::Up,
            180 => PinOrientation::Left,
            270 => PinOrientation::Down,
            _ => PinOrientation::Right,
        }
    }

    /// Get the angle in radians
    pub fn to_radians(&self) -> f64 {
        match self {
            PinOrientation::Right => 0.0,
            PinOrientation::Up => std::f64::consts::FRAC_PI_2,
            PinOrientation::Left => std::f64::consts::PI,
            PinOrientation::Down => std::f64::consts::PI * 1.5,
        }
    }

    /// Get unit vector pointing in the pin's direction (library Y-UP coords).
    ///
    /// Matches JS `stem()` dir vectors. The symbol transform (with Y-flip)
    /// converts these to schematic Y-DOWN coordinates.
    pub fn direction(&self) -> Point {
        match self {
            PinOrientation::Right => Point::new(1.0, 0.0),
            PinOrientation::Up => Point::new(0.0, 1.0),
            PinOrientation::Left => Point::new(-1.0, 0.0),
            PinOrientation::Down => Point::new(0.0, -1.0),
        }
    }
}

/// Pin shape types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinShape {
    Line,
    InvertedClock,
    ClockLow,
    ClockHigh,
    ClockFallingEdge,
    ClockRisingEdge,
    NonLogic,
    Dot,
}

/// Pin graphic data from KiCad
#[derive(Debug, Clone)]
pub struct PinGraphic {
    /// Pin position (KiCad coordinates)
    pub position: Point,
    /// Rotation angle in degrees
    pub rotation: i32,
    /// Pin length in KiCad units (1 unit = 0.0254mm = 0.1 inches)
    pub length: f64,
    /// Pin name
    pub name: String,
    /// Pin number
    pub number: String,
    /// Pin shape
    pub shape: PinShape,
    /// Pin type (input/output/bidirectional/etc.)
    pub pin_type: PinType,
    /// Whether the pin name is visible
    pub name_visible: bool,
    /// Whether the pin number is visible
    pub number_visible: bool,
    /// Whether the entire pin is hidden (KiCad per-pin `hide` flag)
    pub hidden: bool,
    /// Pin name offset from body edge (from symbol's pin_names offset)
    pub pin_name_offset: f64,
    /// Unit name for the symbol
    pub unit_name: Option<String>,
}

/// Pin electrical type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinType {
    Input,
    Output,
    Bidirectional,
    TriState,
    Passive,
    Unspecified,
    PowerIn,
    PowerOut,
    OpenCollector,
    OpenEmitter,
    NotConnected,
}

impl PinGraphic {
    /// Create a new pin graphic
    pub fn new(position: Point, rotation: i32, length: f64) -> Self {
        Self {
            position,
            rotation,
            length,
            name: String::new(),
            number: String::new(),
            shape: PinShape::Line,
            pin_type: PinType::Passive,
            name_visible: true,
            number_visible: true,
            hidden: false,
            pin_name_offset: 0.508,
            unit_name: None,
        }
    }

    /// Get the pin orientation
    pub fn orientation(&self) -> PinOrientation {
        PinOrientation::from_angle(self.rotation)
    }

    /// Get the pin end position (where the wire connects)
    pub fn end_position(&self) -> Point {
        let dir = self.orientation().direction();
        Point::new(
            self.position.x + dir.x * self.length,
            self.position.y + dir.y * self.length,
        )
    }
}

/// Pin Painter - renders pins to graphics
pub struct PinPainter {
    /// Pin graphic data
    pub pin: PinGraphic,
    /// Transform matrix (symbol position + rotation + mirror)
    pub transform: Matrix,
    /// Wire color
    pub wire_color: Color,
    /// Pin color
    pub pin_color: Color,
}

/// Orient a pin label offset based on pin orientation.
///
/// Matches JS `PinLabelInternals.orient_label()`.
/// Returns (offset_x, offset_y, text_rotation_degrees, text_anchor, dominant_baseline).
fn orient_pin_label(ox: f64, oy: f64, orientation: PinOrientation, h_align: &'static str, v_align: &'static str) -> (f64, f64, f64, &'static str, &'static str) {
    let (dx, dy, rot, anchor): (f64, f64, f64, &'static str) = match orientation {
        PinOrientation::Right => (ox, oy, 0.0, h_align),
        PinOrientation::Left => (-ox, oy, 0.0, if h_align == "left" { "end" } else { h_align }),
        PinOrientation::Up => (oy, -ox, 90.0, h_align),
        PinOrientation::Down => (oy, ox, 90.0, if h_align == "left" { "end" } else { h_align }),
    };

    let baseline: &str = match v_align {
        "center" => "central",
        "top" => "hanging",
        _ => "",
    };

    let anchor_str: &str = match anchor {
        "left" => "start",
        "center" => "middle",
        "right" => "end",
        other => other,
    };

    (dx, dy, rot, anchor_str, baseline)
}

impl PinPainter {
    /// Create a new pin painter
    pub fn new(pin: PinGraphic, transform: Matrix, wire_color: Color, pin_color: Color) -> Self {
        Self {
            pin,
            transform,
            wire_color,
            pin_color,
        }
    }

    /// Compute the effective screen orientation by transforming the library
    /// pin direction through the symbol transform matrix.
    ///
    /// The JS code works in library coords inside an SVG group with Y-flip,
    /// so label offsets naturally correct themselves. Since we apply the
    /// transform to individual coordinates, we must use the screen orientation
    /// for label placement so offsets point in the correct visual direction.
    fn effective_orientation(&self) -> PinOrientation {
        let lib_dir = self.pin.orientation().direction();
        let e = &self.transform.elements;
        // Apply only the rotation+scale part (ignore translation e[4], e[5])
        // | a  c |   |dx|   |a*dx + c*dy|
        // | b  d | * |dy| = |b*dx + d*dy|
        let screen_dx = e[0] * lib_dir.x + e[2] * lib_dir.y;
        let screen_dy = e[1] * lib_dir.x + e[3] * lib_dir.y;

        if screen_dx.abs() > screen_dy.abs() {
            if screen_dx > 0.0 { PinOrientation::Right } else { PinOrientation::Left }
        } else {
            if screen_dy > 0.0 { PinOrientation::Down } else { PinOrientation::Up }
        }
    }

    /// Check if the pin is vertically oriented on screen after transform.
    fn is_screen_vertical(&self) -> bool {
        let lib_dir = self.pin.orientation().direction();
        let e = &self.transform.elements;
        let screen_dx = e[0] * lib_dir.x + e[2] * lib_dir.y;
        let screen_dy = e[1] * lib_dir.x + e[3] * lib_dir.y;
        screen_dy.abs() > screen_dx.abs()
    }

    /// Correct Y-flip for vertical pin labels: flip offset_y and swap text-anchor
    /// so the text extends INTO the body instead of away from it.
    fn correct_vertical_anchor(offset_y: &mut f64, text_anchor: &mut &'static str, is_vertical: bool) {
        if is_vertical {
            *offset_y = -*offset_y;
            *text_anchor = match *text_anchor {
                "start" => "end",
                "end" => "start",
                other => other,
            };
        }
    }

    /// Paint the pin body and shape decorations.
    ///
    /// Matches JS `PinShapeInternals.draw()` exactly.
    /// JS convention: `position` = wire connection point (outer end),
    /// `p0` = body-proximal end (inner end).
    /// Rust `end_position()` = position + direction * length = p0 (body end).
    fn paint_pin_body_and_shape(&self, layers: &mut LayerSet) {
        let layer = layers.get_layer_mut(LayerId::SymbolPin)
            .expect("SymbolPin layer missing from LayerSet");
        let position = self.pin.position;
        let p0 = self.pin.end_position();
        let dir = self.pin.orientation().direction();

        let radius = constants::PIN_SYMBOL_SIZE; // 0.635mm
        let diam = radius * 2.0; // 1.27mm

        let stroke = Stroke::new(constants::LINE_WIDTH, self.wire_color);

        // Helper: transform a library-space point to schematic space
        let tx = |p: Point| -> Point { self.transform.transform(&p) };

        // Helper: add polyline in schematic space
        let add_polyline = |layer: &mut crate::layer::Layer, pts: &[Point], stroke: Stroke| {
            let pts_xy: Vec<(f64, f64)> = pts.iter().map(|p| (p.x, p.y)).collect();
            layer.add_element(LayerElement::new(LayerElementType::Polyline(
                Polyline::from_points(&pts_xy, stroke),
            )));
        };

        // Helper: clock notch (V-shape polyline at p0, perpendicular to pin direction)
        // JS: if !dir.y (horizontal pin) → vertical notch; else (vertical pin) → horizontal notch
        let clock_notch = |layer: &mut crate::layer::Layer, p0: Point, dir: Point, stroke: Stroke| {
            if dir.y == 0.0 {
                // Horizontal pin: notch is vertical at p0
                let a = Point::new(p0.x, p0.y + radius);
                let b = Point::new(p0.x - dir.x * radius, p0.y);
                let c = Point::new(p0.x, p0.y - radius);
                add_polyline(layer, &[tx(a), tx(b), tx(c)], stroke);
            } else {
                // Vertical pin: notch is horizontal at p0
                let a = Point::new(p0.x + radius, p0.y);
                let b = Point::new(p0.x, p0.y - dir.y * radius);
                let c = Point::new(p0.x - radius, p0.y);
                add_polyline(layer, &[tx(a), tx(b), tx(c)], stroke);
            }
        };

        // Helper: low-active triangle at p0
        let low_tri = |layer: &mut crate::layer::Layer, p0: Point, dir: Point, stroke: Stroke| {
            if dir.y == 0.0 {
                let a = Point::new(p0.x + dir.x * diam, p0.y);
                let b = Point::new(p0.x + dir.x * diam, p0.y - diam);
                add_polyline(layer, &[tx(a), tx(b), tx(p0)], stroke);
            } else {
                let a = Point::new(p0.x, p0.y + dir.y * diam);
                let b = Point::new(p0.x - diam, p0.y + dir.y * diam);
                add_polyline(layer, &[tx(a), tx(b), tx(p0)], stroke);
            }
        };

        match self.pin.shape {
            PinShape::Line => {
                // Just the line from p0 to position
                add_polyline(layer, &[tx(p0), tx(position)], stroke);
            }
            PinShape::Dot => {
                // Inverted: circle at p0 + dir*radius, then line from p0+dir*diam to position
                let circle_center = Point::new(p0.x + dir.x * radius, p0.y + dir.y * radius);
                let circle = Circle::new(tx(circle_center), radius)
                    .with_stroke(Stroke::new(constants::LINE_WIDTH, self.wire_color));
                layer.add_element(LayerElement::new(LayerElementType::Circle(circle)));

                let line_start = Point::new(p0.x + dir.x * diam, p0.y + dir.y * diam);
                add_polyline(layer, &[tx(line_start), tx(position)], stroke);
            }
            PinShape::InvertedClock => {
                // Circle + line + clock notch
                let circle_center = Point::new(p0.x + dir.x * radius, p0.y + dir.y * radius);
                let circle = Circle::new(tx(circle_center), radius)
                    .with_stroke(Stroke::new(constants::LINE_WIDTH, self.wire_color));
                layer.add_element(LayerElement::new(LayerElementType::Circle(circle)));

                let line_start = Point::new(p0.x + dir.x * diam, p0.y + dir.y * diam);
                add_polyline(layer, &[tx(line_start), tx(position)], stroke.clone());
                clock_notch(layer, p0, dir, stroke);
            }
            PinShape::ClockHigh => {
                // Line + clock notch
                add_polyline(layer, &[tx(p0), tx(position)], stroke.clone());
                clock_notch(layer, p0, dir, stroke);
            }
            PinShape::ClockLow | PinShape::ClockRisingEdge => {
                // Line + clock notch + low triangle
                add_polyline(layer, &[tx(p0), tx(position)], stroke.clone());
                clock_notch(layer, p0, dir, stroke.clone());
                low_tri(layer, p0, dir, stroke);
            }
            PinShape::ClockFallingEdge => {
                // input_low: Line + low triangle only
                add_polyline(layer, &[tx(p0), tx(position)], stroke.clone());
                low_tri(layer, p0, dir, stroke);
            }
            PinShape::NonLogic => {
                // Line + X cross at p0
                add_polyline(layer, &[tx(p0), tx(position)], stroke.clone());
                // First diagonal
                let d1 = Point::new(dir.x + dir.y, dir.y - dir.x);
                let d2 = Point::new(dir.x - dir.y, dir.y + dir.x);
                let x1a = Point::new(p0.x - d1.x * radius, p0.y - d1.y * radius);
                let x1b = Point::new(p0.x + d1.x * radius, p0.y + d1.y * radius);
                add_polyline(layer, &[tx(x1a), tx(x1b)], stroke.clone());
                // Second diagonal
                let x2a = Point::new(p0.x - d2.x * radius, p0.y - d2.y * radius);
                let x2b = Point::new(p0.x + d2.x * radius, p0.y + d2.y * radius);
                add_polyline(layer, &[tx(x2a), tx(x2b)], stroke);
            }
        }
    }

    /// Paint the pin name text.
    ///
    /// Matches JS `PinPainter.draw_name_and_number()` → `PinLabelInternals.place_inside()`
    /// when pin_name_offset > 0, or `place_above()` when pin_name_offset <= 0.
    fn paint_pin_name(&self, layers: &mut LayerSet) {
        if !self.pin.name_visible || self.pin.name.is_empty() {
            return;
        }

        let layer = layers.get_layer_mut(LayerId::SymbolPin)
            .expect("SymbolPin layer missing from LayerSet");

        let font_size = constants::PINNAME_SIZE;
        let pin_length = self.pin.length;
        let orientation = self.effective_orientation();
        let pin_name_offset = self.pin.pin_name_offset;
        let text_margin = constants::TEXT_MARGIN * constants::TEXT_OFFSET_RATIO;
        let thickness = constants::LINE_WIDTH;

        let (offset_x, mut offset_y, rotation, mut text_anchor, dominant_baseline) = if pin_name_offset > 0.0 {
            let ox = pin_name_offset - thickness / 2.0 + pin_length;
            orient_pin_label(ox, 0.0, orientation, "left", "center")
        } else {
            let ox = pin_length / 2.0;
            let oy = -(text_margin + thickness);
            orient_pin_label(ox, oy, orientation, "center", "bottom")
        };

        Self::correct_vertical_anchor(&mut offset_y, &mut text_anchor, self.is_screen_vertical());

        let text_pos = self.transform.transform(&Point::new(
            self.pin.position.x + offset_x,
            self.pin.position.y + offset_y,
        ));

        layer.add_element(LayerElement::new(LayerElementType::Text {
            position: text_pos,
            text: self.pin.name.clone(),
            font_size,
            color: self.pin_color,
            bold: false,
            rotation,
            text_anchor,
            dominant_baseline,
        }));
    }

    /// Paint the pin number text.
    fn paint_pin_number(&self, layers: &mut LayerSet) {
        if !self.pin.number_visible || self.pin.number.is_empty() {
            return;
        }

        let layer = layers.get_layer_mut(LayerId::SymbolPin)
            .expect("SymbolPin layer missing from LayerSet");

        let font_size = constants::PINNUM_SIZE;
        let pin_length = self.pin.length;
        let orientation = self.effective_orientation();
        let pin_name_offset = self.pin.pin_name_offset;
        let text_margin = constants::TEXT_MARGIN * constants::TEXT_OFFSET_RATIO;
        let thickness = constants::LINE_WIDTH;

        let (offset_x, mut offset_y, rotation, mut text_anchor, dominant_baseline) = if pin_name_offset > 0.0 {
            let ox = pin_length / 2.0;
            let oy = -(text_margin + thickness);
            orient_pin_label(ox, oy, orientation, "center", "bottom")
        } else {
            let ox = pin_length / 2.0;
            let oy = text_margin + thickness;
            orient_pin_label(ox, oy, orientation, "center", "top")
        };

        Self::correct_vertical_anchor(&mut offset_y, &mut text_anchor, self.is_screen_vertical());

        let text_pos = self.transform.transform(&Point::new(
            self.pin.position.x + offset_x,
            self.pin.position.y + offset_y,
        ));

        layer.add_element(LayerElement::new(LayerElementType::Text {
            position: text_pos,
            text: self.pin.number.clone(),
            font_size,
            color: self.pin_color,
            bold: false,
            rotation,
            text_anchor,
            dominant_baseline,
        }));
    }
}

impl Painter for PinPainter {
    fn bbox(&self) -> BoundingBox {
        let start = self.transform.transform(&self.pin.position);
        let end = self.transform.transform(&self.pin.end_position());

        let mut bbox = BoundingBox::from_points(&start, &end);

        // Add padding for text
        bbox = bbox.with_padding(1.27);

        bbox
    }

    fn paint(&self, layers: &mut LayerSet) {
        if self.pin.hidden {
            return;
        }
        self.paint_pin_body_and_shape(layers);
        self.paint_pin_name(layers);
        self.paint_pin_number(layers);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pin_orientation() {
        assert_eq!(PinOrientation::from_angle(0), PinOrientation::Right);
        assert_eq!(PinOrientation::from_angle(90), PinOrientation::Up);
        assert_eq!(PinOrientation::from_angle(180), PinOrientation::Left);
        assert_eq!(PinOrientation::from_angle(270), PinOrientation::Down);
        assert_eq!(PinOrientation::from_angle(360), PinOrientation::Right);
    }

    #[test]
    fn test_pin_end_position() {
        let pin = PinGraphic::new(Point::new(0.0, 0.0), 0, 5.0);
        let end = pin.end_position();
        assert_eq!(end.x, 5.0);
        assert_eq!(end.y, 0.0);

        let pin = PinGraphic::new(Point::new(0.0, 0.0), 90, 5.0);
        let end = pin.end_position();
        assert_eq!(end.x, 0.0);
        // Y-UP: Up direction = (0, 1), so end = position + (0, 5) = (0, 5)
        assert_eq!(end.y, 5.0);
    }
}
