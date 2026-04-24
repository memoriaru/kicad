//! Bridge module — converts kicad-json5 IR types to kicad-render painter data types.
//!
//! This is the single point of translation between the parsed schematic data
//! and the rendering pipeline. All IR types live in `kicad_json5::ir::*`,
//! all painter types live in `crate::painter::*` and `crate::render_core::*`.

use kicad_json5::ir::{
    self, Arc as IrArc, Circle as IrCircle, FillType, GraphicElement, Label as IrLabel,
    HorizontalAlign, PinGraphic as IrPinGraphic, PinShape as IrPinShape, PinType as IrPinType,
    Polyline as IrPolyline, Rectangle as IrRect, StrokeType, Symbol, SymbolInstance as IrSymbolInstance,
    VerticalAlign, Wire as IrWire,
};

use crate::painter::{
    Junction, Label, LabelShape, LabelType, Mirror, PinGraphic,
    PinShape, PinType, SymbolInstance, WirePainter, WireSegment,
};
use crate::render_core::graphics::{
    Arc, Circle, Fill, Polygon, Polyline, Stroke, StrokeStyle,
};
use crate::render_core::{Color, Point};
use crate::constants;

// ── Wire ──────────────────────────────────────────────────

/// Convert an IR Wire to a painter WireSegment (no Y-flip; done by SchematicRenderer).
pub fn convert_wire(wire: &IrWire) -> WireSegment {
    WireSegment::new(
        Point::new(wire.start.0, wire.start.1),
        Point::new(wire.end.0, wire.end.1),
    )
}

/// Build a WirePainter from all schematic wires.
pub fn convert_wires(wires: &[IrWire]) -> WirePainter {
    let segments: Vec<WireSegment> = wires.iter().map(convert_wire).collect();
    WirePainter::with_width(segments, constants::wire_color(), constants::WIRE_WIDTH)
}

// ── Junction ──────────────────────────────────────────────

/// Convert IR Junction to painter Junction.
pub fn convert_junction(pos: (f64, f64), diameter: f64) -> Junction {
    let d = if diameter > 0.0 {
        diameter
    } else {
        constants::JUNCTION_DIAMETER
    };
    Junction {
        position: Point::new(pos.0, pos.1),
        diameter: d,
    }
}

// ── Label ─────────────────────────────────────────────────

/// Convert IR Label to painter Label.
pub fn convert_label(label: &IrLabel) -> Label {
    let label_type = match label.label_type.as_str() {
        "global_label" => LabelType::Global,
        "hierarchical_label" => LabelType::Hierarchical,
        _ => LabelType::Local,
    };
    let shape = match label.shape.as_str() {
        "input" => LabelShape::Input,
        "output" => LabelShape::Output,
        "bidirectional" => LabelShape::Bidirectional,
        "tri_state" => LabelShape::TriState,
        "passive" => LabelShape::Passive,
        _ => LabelShape::Passive,
    };
    let font_size = label.effects.font.size.1.max(label.effects.font.size.0);
    Label {
        label_type,
        position: Point::new(label.position.0, label.position.1),
        rotation: label.position.2 as i32,
        text: label.text.clone(),
        shape,
        font_size: if font_size > 0.0 { font_size } else { constants::TEXT_SIZE },
    }
}

// ── Pin ───────────────────────────────────────────────────

/// Convert IR PinGraphic to painter PinGraphic.
pub fn convert_pin(pin: &IrPinGraphic, pin_names_hidden: bool, pin_numbers_hidden: bool, pin_name_offset: f64) -> PinGraphic {
    PinGraphic {
        position: Point::new(pin.position.0, pin.position.1),
        rotation: pin.position.2 as i32,
        length: pin.length,
        name: if pin.name == "~" { String::new() } else { pin.name.clone() },
        number: pin.number.clone(),
        shape: convert_pin_shape(&pin.shape),
        pin_type: convert_pin_type(&pin.pin_type),
        name_visible: !pin_names_hidden && !pin.name_effects.hide,
        number_visible: !pin_numbers_hidden && !pin.number_effects.hide,
        hidden: pin.hidden,
        pin_name_offset,
        unit_name: None,
    }
}

fn convert_pin_shape(shape: &IrPinShape) -> PinShape {
    match shape {
        IrPinShape::Line => PinShape::Line,
        IrPinShape::Inverted => PinShape::Dot,
        IrPinShape::Clock => PinShape::ClockHigh,
        IrPinShape::InvertedClock => PinShape::InvertedClock,
        IrPinShape::InputLow => PinShape::ClockLow,
        IrPinShape::ClockLow => PinShape::ClockLow,
        IrPinShape::OutputLow => PinShape::NonLogic,
        IrPinShape::EdgeClockHigh => PinShape::ClockRisingEdge,
        IrPinShape::NonLogic => PinShape::NonLogic,
        IrPinShape::Triangle => PinShape::Line,
    }
}

fn convert_pin_type(pt: &IrPinType) -> PinType {
    match pt {
        IrPinType::Input => PinType::Input,
        IrPinType::Output => PinType::Output,
        IrPinType::Bidirectional => PinType::Bidirectional,
        IrPinType::TriState => PinType::TriState,
        IrPinType::Passive => PinType::Passive,
        IrPinType::Free => PinType::Unspecified,
        IrPinType::Unspecified => PinType::Unspecified,
        IrPinType::PowerIn => PinType::PowerIn,
        IrPinType::PowerOut => PinType::PowerOut,
        IrPinType::OpenCollector => PinType::OpenCollector,
        IrPinType::OpenEmitter => PinType::OpenEmitter,
        IrPinType::NoConnect => PinType::NotConnected,
    }
}

// ── Symbol ────────────────────────────────────────────────

/// Convert IR SymbolInstance + lib Symbol to painter SymbolInstance.
pub fn convert_symbol(
    component: &IrSymbolInstance,
    lib_symbol: Option<&Symbol>,
) -> SymbolInstance {
    let mirror = match component.mirror {
        ir::Mirror::X => Mirror::X,
        ir::Mirror::Y => Mirror::Y,
        ir::Mirror::None => Mirror::None,
    };

    let pins = if let Some(lib) = lib_symbol {
        collect_pins(lib)
    } else {
        Vec::new()
    };

    // Extract Reference and Value positions, rotation, justify, and hide status from properties_ext
    // KiCad stores property positions as ABSOLUTE schematic coordinates
    let default_justify = kicad_json5::ir::Justify::default();

    let ref_prop = component.properties_ext.iter().find(|p| p.name == "Reference");
    let val_prop = component.properties_ext.iter().find(|p| p.name == "Value");

    let (reference_position, reference_rotation, reference_hidden, ref_justify) = ref_prop
        .map(|p| (Some((p.position.0, p.position.1)), p.position.2, p.hide || p.effects.hide, &p.effects.justify))
        .unwrap_or((None, 0.0, false, &default_justify));
    let (value_position, value_rotation, value_hidden, val_justify) = val_prop
        .map(|p| (Some((p.position.0, p.position.1)), p.position.2, p.hide || p.effects.hide, &p.effects.justify))
        .unwrap_or((None, 0.0, false, &default_justify));

    SymbolInstance {
        library_id: component.lib_id.clone(),
        position: Point::new(component.position.0, component.position.1),
        rotation: component.position.2 as i32,
        mirror,
        unit: component.unit as i32,
        pins,
        reference: component.reference.clone(),
        value: component.value.clone(),
        reference_position,
        reference_rotation,
        reference_h_align: h_align_to_svg(ref_justify.horizontal),
        reference_v_align: v_align_to_svg(ref_justify.vertical),
        value_position,
        value_rotation,
        value_h_align: h_align_to_svg(val_justify.horizontal),
        value_v_align: v_align_to_svg(val_justify.vertical),
        reference_hidden,
        value_hidden,
    }
}

fn h_align_to_svg(align: HorizontalAlign) -> String {
    match align {
        HorizontalAlign::Left => "start".to_string(),
        HorizontalAlign::Center => "middle".to_string(),
        HorizontalAlign::Right => "end".to_string(),
    }
}

fn v_align_to_svg(align: VerticalAlign) -> String {
    match align {
        VerticalAlign::Top => "hanging".to_string(),
        VerticalAlign::Center => "central".to_string(),
        VerticalAlign::Bottom => "auto".to_string(),
    }
}

/// Collect all pins from a library symbol definition.
fn collect_pins(lib: &Symbol) -> Vec<PinGraphic> {
    let mut pins = Vec::new();
    let hidden_names = lib.pin_names_hidden;
    let hidden_nums = lib.pin_numbers_hidden;

    let offset = lib.pin_name_offset;

    for g in &lib.graphics {
        if let GraphicElement::Pin(p) = g {
            pins.push(convert_pin(p, hidden_names, hidden_nums, offset));
        }
    }
    for unit in &lib.units {
        for g in &unit.graphics {
            if let GraphicElement::Pin(p) = g {
                pins.push(convert_pin(p, hidden_names, hidden_nums, offset));
            }
        }
    }
    pins
}

// ── Graphic elements (symbol body shapes) ─────────────────

/// IR stroke → render_core Stroke. If width is 0, uses default LINE_WIDTH.
/// `color` should come from the theme (e.g. outline_color for symbol body).
pub fn convert_stroke_with_color(s: &ir::Stroke, color: Color) -> Stroke {
    let width = if s.width > 0.0 { s.width } else { constants::LINE_WIDTH };
    let style = match s.stroke_type {
        StrokeType::Dash => StrokeStyle::Dash,
        StrokeType::Dot => StrokeStyle::Dot,
        StrokeType::DashDot => StrokeStyle::DashDot,
        StrokeType::DashDotDot => StrokeStyle::DashDotDot,
        _ => StrokeStyle::Solid,
    };
    Stroke { width, color, style }
}

/// IR stroke → render_core Stroke, forcing solid style.
/// KiCanvas JS ignores dash type for ALL schematic rendering — every line is solid.
pub fn convert_stroke_solid(s: &ir::Stroke, color: Color) -> Stroke {
    let width = if s.width > 0.0 { s.width } else { constants::LINE_WIDTH };
    Stroke { width, color, style: StrokeStyle::Solid }
}

/// IR stroke → render_core Stroke with default black color.
pub fn convert_stroke(s: &ir::Stroke) -> Stroke {
    convert_stroke_with_color(s, Color::black())
}

/// IR fill → render_core Fill. Outline type gets no fill.
pub fn convert_fill(f: &ir::Fill) -> Fill {
    convert_fill_with_outline(f, Color::black())
}

/// IR fill → render_core Fill with outline color for "outline" fill type.
/// In JS/KiCad: `fill outline` means fill with the stroke/outline color.
pub fn convert_fill_with_outline(f: &ir::Fill, outline_color: Color) -> Fill {
    match f.fill_type {
        FillType::None => Fill::none(),
        FillType::Outline => Fill::solid(outline_color),
        FillType::Background => Fill::solid(Color::from_rgb(255, 255, 194)), // #FFFFC2
        FillType::Color => {
            if let Some((r, g, b, _a)) = f.color {
                Fill::solid(Color::from_rgb(r, g, b))
            } else {
                Fill::none()
            }
        }
    }
}

/// Convert IR Polyline to render_core Polyline.
pub fn convert_polyline(ir: &IrPolyline) -> Polyline {
    convert_polyline_with_color(ir, Color::black())
}

/// Convert IR Polyline with specified stroke color.
pub fn convert_polyline_with_color(ir: &IrPolyline, color: Color) -> Polyline {
    let points: Vec<(f64, f64)> = ir.points.iter().map(|(x, y)| (*x, *y)).collect();
    Polyline::from_points(&points, convert_stroke_with_color(&ir.stroke, color))
}

/// Convert IR Polyline with specified stroke color, forcing solid stroke style.
/// JS PolylinePainter ignores dash type entirely — all schematic polylines render as solid.
pub fn convert_polyline_solid(ir: &IrPolyline, color: Color) -> Polyline {
    let points: Vec<(f64, f64)> = ir.points.iter().map(|(x, y)| (*x, *y)).collect();
    Polyline::from_points(&points, convert_stroke_solid(&ir.stroke, color))
}

/// Convert IR Polyline with stroke and fill outline color.
pub fn convert_polyline_with_fill(ir: &IrPolyline, stroke_color: Color, outline_color: Color) -> Polyline {
    // Polylines don't have fill, but include for API consistency
    let points: Vec<(f64, f64)> = ir.points.iter().map(|(x, y)| (*x, *y)).collect();
    Polyline::from_points(&points, convert_stroke_solid(&ir.stroke, stroke_color))
}

/// Convert IR Rectangle to render_core Polygon (4 corners).
pub fn convert_rectangle(ir: &IrRect) -> Polygon {
    convert_rectangle_with_color(ir, Color::black())
}

/// Convert IR Rectangle with specified stroke color.
pub fn convert_rectangle_with_color(ir: &IrRect, color: Color) -> Polygon {
    convert_rectangle_with_fill(ir, color, color)
}

/// Convert IR Rectangle with separate stroke and fill outline colors.
pub fn convert_rectangle_with_fill(ir: &IrRect, stroke_color: Color, outline_color: Color) -> Polygon {
    let (x1, y1) = ir.start;
    let (x2, y2) = ir.end;
    let fill = convert_fill_with_outline(&ir.fill, outline_color);
    let stroke = convert_stroke_solid(&ir.stroke, stroke_color);
    Polygon::from_points(&[(x1, y1), (x2, y1), (x2, y2), (x1, y2), (x1, y1)])
        .with_stroke(stroke)
        .with_fill(fill)
}

/// Convert IR Circle to render_core Circle.
pub fn convert_circle(ir: &IrCircle) -> Circle {
    convert_circle_with_color(ir, Color::black())
}

/// Convert IR Circle with specified stroke color.
pub fn convert_circle_with_color(ir: &IrCircle, color: Color) -> Circle {
    convert_circle_with_fill(ir, color, color)
}

/// Convert IR Circle with separate stroke and fill outline colors.
pub fn convert_circle_with_fill(ir: &IrCircle, stroke_color: Color, outline_color: Color) -> Circle {
    Circle::new(Point::new(ir.center.0, ir.center.1), ir.radius)
        .with_fill(convert_fill_with_outline(&ir.fill, outline_color))
        .with_stroke(convert_stroke_solid(&ir.stroke, stroke_color))
}

/// Convert IR Arc to render_core Arc.
/// The IR stores (start, mid, end) points; we compute center/radius/angles.
pub fn convert_arc(ir: &IrArc) -> Option<Arc> {
    convert_arc_with_color(ir, Color::black())
}

/// Convert IR Arc with specified stroke color.
pub fn convert_arc_with_color(ir: &IrArc, color: Color) -> Option<Arc> {
    let (cx, cy, radius, start_angle, end_angle) = ir.calculate_arc_params()?;
    let fill = convert_fill_with_outline(&ir.fill, color);
    Some(Arc::new(
        Point::new(cx, cy),
        radius,
        start_angle,
        end_angle,
        convert_stroke_solid(&ir.stroke, color),
    ).with_fill(fill))
}
