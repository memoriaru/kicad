use super::*;

impl SexprGenerator {
    // ============== Graphic Elements Generation ==============

    pub(super) fn generate_graphic_element(&mut self, output: &mut String, element: &GraphicElement) {
        match element {
            GraphicElement::Polyline(pl) => self.generate_polyline(output, pl),
            GraphicElement::Rectangle(rect) => self.generate_rectangle(output, rect),
            GraphicElement::Circle(circle) => self.generate_circle(output, circle),
            GraphicElement::Arc(arc) => self.generate_arc(output, arc),
            GraphicElement::Text(text) => self.generate_text(output, text),
            GraphicElement::Pin(pin) => self.generate_pin_graphic(output, pin),
        }
    }

    fn generate_polyline(&mut self, output: &mut String, polyline: &Polyline) {
        if polyline.points.len() < 2 {
            return;
        }

        self.write_line(output, "(polyline");
        self.indent_level += 1;

        // Points
        self.write_line(output, "(pts");
        self.indent_level += 1;
        for (x, y) in &polyline.points {
            self.write_line(output, &Self::format_xy(*x, *y));
        }
        self.indent_level -= 1;
        self.write_line(output, ")");

        // Stroke
        self.generate_stroke(output, &polyline.stroke);

        // Fill
        self.generate_fill(output, &polyline.fill);

        self.indent_level -= 1;
        self.write_line(output, ")");
    }

    fn generate_rectangle(&mut self, output: &mut String, rect: &Rectangle) {
        self.write_line(output, "(rectangle");
        self.indent_level += 1;

        self.write_line(output, &format!("(start {} {})", Self::format_number(rect.start.0), Self::format_number(rect.start.1)));
        self.write_line(output, &format!("(end {} {})", Self::format_number(rect.end.0), Self::format_number(rect.end.1)));

        self.generate_stroke(output, &rect.stroke);
        self.generate_fill(output, &rect.fill);

        self.indent_level -= 1;
        self.write_line(output, ")");
    }

    fn generate_circle(&mut self, output: &mut String, circle: &Circle) {
        self.write_line(output, "(circle");
        self.indent_level += 1;

        self.write_line(output, &format!("(center {} {})", Self::format_number(circle.center.0), Self::format_number(circle.center.1)));
        self.write_line(output, &format!("(radius {})", Self::format_number(circle.radius)));

        self.generate_stroke(output, &circle.stroke);
        self.generate_fill(output, &circle.fill);

        self.indent_level -= 1;
        self.write_line(output, ")");
    }

    fn generate_arc(&mut self, output: &mut String, arc: &Arc) {
        self.write_line(output, "(arc");
        self.indent_level += 1;

        self.write_line(output, &format!("(start {} {})", Self::format_number(arc.start.0), Self::format_number(arc.start.1)));
        self.write_line(output, &format!("(mid {} {})", Self::format_number(arc.mid.0), Self::format_number(arc.mid.1)));
        self.write_line(output, &format!("(end {} {})", Self::format_number(arc.end.0), Self::format_number(arc.end.1)));

        self.generate_stroke(output, &arc.stroke);
        self.generate_fill(output, &arc.fill);

        self.indent_level -= 1;
        self.write_line(output, ")");
    }

    fn generate_text(&mut self, output: &mut String, text: &Text) {
        self.write_line(output, "(text");
        self.indent_level += 1;

        self.write_line(output, &format!("\"{}\"", Self::escape_string(&text.text)));
        self.write_line(output, &Self::format_at(text.position.0, text.position.1, text.position.2));

        self.generate_effects(output, &text.effects);

        self.indent_level -= 1;
        self.write_line(output, ")");
    }

    fn generate_pin_graphic(&mut self, output: &mut String, pin: &PinGraphic) {
        let shape_str = match pin.shape {
            PinShape::Line => "line",
            PinShape::Inverted => "inverted",
            PinShape::Clock => "clock",
            PinShape::InvertedClock => "inverted_clock",
            PinShape::InputLow => "input_low",
            PinShape::ClockLow => "clock_low",
            PinShape::OutputLow => "output_low",
            PinShape::EdgeClockHigh => "edge_clock_high",
            PinShape::NonLogic => "non_logic",
            PinShape::Triangle => "triangle",
        };

        let type_str = match pin.pin_type {
            PinType::Input => "input",
            PinType::Output => "output",
            PinType::Bidirectional => "bidirectional",
            PinType::TriState => "tri_state",
            PinType::Passive => "passive",
            PinType::Free => "free",
            PinType::Unspecified => "unspecified",
            PinType::PowerIn => "power_in",
            PinType::PowerOut => "power_out",
            PinType::OpenCollector => "open_collector",
            PinType::OpenEmitter => "open_emitter",
            PinType::NoConnect => "no_connect",
        };

        // Format position without extra "(at ...)" wrapper
        let at_str = format!(
            "(at {} {} {})",
            Self::format_number(pin.position.0),
            Self::format_number(pin.position.1),
            Self::format_number(pin.position.2)
        );

        self.write_line(
            output,
            &format!(
                "(pin {} {} {} (length {})",
                type_str,
                shape_str,
                at_str,
                Self::format_number(pin.length)
            ),
        );
        self.indent_level += 1;

        self.write_line(output, &format!(
            "(name \"{}\" (effects (font (size 1.27 1.27))))",
            Self::escape_string(&pin.name)
        ));
        self.write_line(output, &format!(
            "(number \"{}\" (effects (font (size 1.27 1.27))))",
            pin.number
        ));

        self.indent_level -= 1;
        self.write_line(output, ")");
    }

    pub(super) fn generate_stroke(&mut self, output: &mut String, stroke: &Stroke) {
        let type_str = match stroke.stroke_type {
            StrokeType::Default => "default",
            StrokeType::Dash => "dash",
            StrokeType::Dot => "dot",
            StrokeType::DashDot => "dash_dot",
            StrokeType::DashDotDot => "dash_dot_dot",
            StrokeType::Solid => "solid",
        };

        self.write_line(
            output,
            &format!(
                "(stroke (width {}) (type {}))",
                Self::format_number(stroke.width),
                type_str
            ),
        );
    }

    pub(super) fn generate_fill(&mut self, output: &mut String, fill: &Fill) {
        let type_str = match fill.fill_type {
            FillType::None => "none",
            FillType::Outline => "outline",
            FillType::Background => "background",
            FillType::Color => "color",
        };

        self.write_line(output, &format!("(fill (type {}))", type_str));
    }

    pub(super) fn generate_effects(&mut self, output: &mut String, effects: &TextEffects) {
        self.write_line(output, "(effects");
        self.indent_level += 1;

        self.write_line(
            output,
            &format!(
                "(font (size {} {}))",
                Self::format_number(effects.font.size.0),
                Self::format_number(effects.font.size.1)
            ),
        );

        if effects.font.bold {
            self.write_line(output, "(bold yes)");
        }
        if effects.font.italic {
            self.write_line(output, "(italic yes)");
        }

        // Justify -- KiCad only accepts: left, right, top, bottom, mirror
        // "center" is implicit (no keyword). Omit direction if Center.
        let h = match effects.justify.horizontal {
            HorizontalAlign::Left => Some("left"),
            HorizontalAlign::Center => None,
            HorizontalAlign::Right => Some("right"),
        };
        let v = match effects.justify.vertical {
            VerticalAlign::Top => Some("top"),
            VerticalAlign::Center => None,
            VerticalAlign::Bottom => Some("bottom"),
        };

        if h.is_some() || v.is_some() || effects.justify.mirror {
            let mut parts = Vec::new();
            if let Some(hv) = h { parts.push(hv); }
            if let Some(vv) = v { parts.push(vv); }
            if effects.justify.mirror { parts.push("mirror"); }
            self.write_line(output, &format!("(justify {})", parts.join(" ")));
        }

        if effects.hide {
            self.write_line(output, "(hide yes)");
        }

        self.indent_level -= 1;
        self.write_line(output, ")");
    }
}
