use super::*;
use crate::ir::{Sheet, SheetPin, SheetProperty};

impl SexprGenerator {
    pub(super) fn generate_symbol_instance(&mut self, output: &mut String, component: &SymbolInstance) {
        self.write_line(output, "(symbol");
        self.indent_level += 1;

        // lib_id
        self.write_line(output, &format!("(lib_id \"{}\")", Self::normalize_lib_id(&component.lib_id)));

        // at
        self.write_line(
            output,
            &Self::format_at(component.position.0, component.position.1, component.position.2),
        );

        // mirror
        match component.mirror {
            crate::ir::Mirror::X => self.write_line(output, "(mirror x)"),
            crate::ir::Mirror::Y => self.write_line(output, "(mirror y)"),
            crate::ir::Mirror::None => {}
        }

        // unit (always output)
        self.write_line(output, &format!("(unit {})", component.unit));

        // body_style (DeMorgan style, default 1)
        let is_v9_plus = matches!(self.effective_version, KicadVersion::V9 | KicadVersion::V10);
        if is_v9_plus {
            self.write_line(output, "(body_style 1)");
        }

        // exclude_from_sim, in_bom, on_board, dnp
        self.write_line(output, &format!("(exclude_from_sim {})", Self::format_bool(component.exclude_from_sim)));
        self.write_line(output, &format!("(in_bom {})", Self::format_bool(component.in_bom)));
        self.write_line(output, &format!("(on_board {})", Self::format_bool(component.on_board)));
        self.write_line(output, &format!("(dnp {})", Self::format_bool(component.dnp)));

        // fields_autoplaced
        if component.fields_autoplaced {
            self.write_line(output, &format!("(fields_autoplaced yes)"));
        }

        // UUID
        if self.config.include_uuids {
            let uuid = component.uuid.clone().unwrap_or_else(Self::new_uuid);
            self.write_line(output, &format!("(uuid \"{}\")", uuid));
        }

        // Properties
        for prop in &component.properties_ext {
            self.generate_property(output, prop);
        }

        // Pins
        for pin in &component.pins {
            self.generate_pin_instance(output, pin);
        }

        // instances
        self.generate_instances(output, &component.instances);

        self.indent_level -= 1;
        self.write_line(output, ")");
    }

    pub(super) fn generate_property(&mut self, output: &mut String, prop: &crate::ir::Property) {
        self.write_line(output, &format!("(property \"{}\"", prop.name));
        self.indent_level += 1;

        self.write_line(output, &format!("\"{}\"", Self::escape_string(&prop.value)));
        self.write_line(
            output,
            &Self::format_at(prop.position.0, prop.position.1, prop.position.2),
        );

        if prop.hide {
            self.write_line(output, &format!("(hide yes)"));
        }

        // v9+: show_name and do_not_autoplace
        let is_v9_plus = matches!(self.effective_version, KicadVersion::V9 | KicadVersion::V10);
        if is_v9_plus {
            self.write_line(output, &format!("(show_name {})", Self::format_bool(prop.show_name)));
            self.write_line(output, &format!("(do_not_autoplace {})", Self::format_bool(prop.do_not_autoplace)));
        }

        self.generate_effects(output, &prop.effects);

        self.indent_level -= 1;
        self.write_line(output, ")");
    }

    pub(super) fn generate_instances(&mut self, output: &mut String, instances: &Instances) {
        if instances.projects.is_empty() {
            return;
        }

        self.write_line(output, "(instances");
        self.indent_level += 1;

        for project in &instances.projects {
            self.write_line(output, &format!("(project \"{}\"", Self::escape_string(&project.name)));
            self.indent_level += 1;

            for path in &project.paths {
                self.write_line(output, &format!("(path \"{}\"", path.path));
                self.indent_level += 1;
                self.write_line(output, &format!("(reference \"{}\")", Self::escape_string(&path.reference)));
                self.write_line(output, &format!("(unit {})", path.unit));
                self.indent_level -= 1;
                self.write_line(output, ")");
            }

            self.indent_level -= 1;
            self.write_line(output, ")");
        }

        self.indent_level -= 1;
        self.write_line(output, ")");
    }

    pub(super) fn generate_pin_instance(
        &mut self,
        output: &mut String,
        pin: &crate::ir::PinInstance,
    ) {
        self.write_line(output, &format!("(pin \"{}\"", pin.number));
        self.indent_level += 1;

        if self.config.include_uuids {
            self.write_line(output, &format!("(uuid \"{}\")", Self::new_uuid()));
        }

        self.indent_level -= 1;
        self.write_line(output, ")");
    }

    pub(super) fn generate_wire(&mut self, output: &mut String, wire: &Wire) {
        self.write_line(output, "(wire");
        self.indent_level += 1;

        self.write_line(output, &format!(
            "(pts {} {})",
            Self::format_xy(wire.start.0, wire.start.1),
            Self::format_xy(wire.end.0, wire.end.1)
        ));

        self.generate_stroke(output, &wire.stroke);

        self.indent_level -= 1;
        self.write_line(output, ")");
    }

    pub(super) fn generate_label(&mut self, output: &mut String, label: &Label) {
        // KiCad uses "netclass_flag" as the token for directive labels
        let label_type = label.label_type.as_str();
        let effective_type = if label_type == "directive_label" {
            "netclass_flag"
        } else {
            label_type
        };
        let is_global = effective_type == "global_label";
        let is_hierarchical = effective_type == "hierarchical_label";
        let is_directive = effective_type == "netclass_flag";

        self.write_line(output, &format!("({}", effective_type));
        self.indent_level += 1;

        self.write_line(output, &format!("\"{}\"", Self::escape_string(&label.text)));
        self.write_line(
            output,
            &Self::format_at(label.position.0, label.position.1, label.position.2),
        );

        if is_global || is_hierarchical || is_directive {
            self.write_line(output, &format!("(shape {})", label.shape));
        }

        self.generate_effects(output, &label.effects);

        if self.config.include_uuids {
            self.write_line(output, &format!("(uuid \"{}\")", Self::new_uuid()));
        }

        self.indent_level -= 1;
        self.write_line(output, ")");
    }

    pub(super) fn generate_junction(&mut self, output: &mut String, junction: &Junction) {
        self.write_line(output, "(junction");
        self.indent_level += 1;

        // Junction (at) only has x, y — no rotation
        self.write_line(output, &format!(
            "(at {} {})",
            Self::format_number(junction.position.0),
            Self::format_number(junction.position.1)
        ));

        if junction.diameter != 0.0 {
            self.write_line(output, &format!("(diameter {})", Self::format_number(junction.diameter)));
        }

        if self.config.include_uuids {
            self.write_line(output, &format!("(uuid \"{}\")", Self::new_uuid()));
        }

        self.indent_level -= 1;
        self.write_line(output, ")");
    }

    pub(super) fn generate_no_connect(&mut self, output: &mut String, nc: &NoConnect) {
        self.write_line(output, "(no_connect");
        self.indent_level += 1;

        // no_connect (at) only has x, y — no rotation
        self.write_line(output, &format!(
            "(at {} {})",
            Self::format_number(nc.position.0),
            Self::format_number(nc.position.1)
        ));

        if self.config.include_uuids {
            let uuid = nc.uuid.clone().unwrap_or_else(Self::new_uuid);
            self.write_line(output, &format!("(uuid \"{}\")", uuid));
        }

        self.indent_level -= 1;
        self.write_line(output, ")");
    }

    pub(super) fn generate_bus(&mut self, output: &mut String, bus: &Bus) {
        if bus.points.len() < 2 {
            return;
        }

        self.write_line(output, "(bus");
        self.indent_level += 1;

        self.write_line(output, "(pts");
        self.indent_level += 1;
        for (x, y) in &bus.points {
            self.write_line(output, &Self::format_xy(*x, *y));
        }
        self.indent_level -= 1;
        self.write_line(output, ")");

        self.generate_stroke(output, &bus.stroke);

        self.indent_level -= 1;
        self.write_line(output, ")");
    }

    pub(super) fn generate_bus_entry(&mut self, output: &mut String, entry: &BusEntry) {
        self.write_line(output, "(bus_entry");
        self.indent_level += 1;

        self.write_line(output, &Self::format_at(entry.position.0, entry.position.1, 0.0));
        self.write_line(
            output,
            &format!("(size {} {})", Self::format_number(entry.size.0), Self::format_number(entry.size.1)),
        );

        self.generate_stroke(output, &entry.stroke);

        self.indent_level -= 1;
        self.write_line(output, ")");
    }

    // ============== Hierarchical Sheet Generation ==============

    pub(super) fn generate_sheet(&mut self, output: &mut String, sheet: &Sheet) {
        self.write_line(output, "(sheet");
        self.indent_level += 1;

        if self.config.include_uuids {
            self.write_line(output, &format!("(uuid \"{}\")", Self::new_uuid()));
        }

        self.write_line(output, &format!(
            "(at {} {})",
            Self::format_number(sheet.position.0),
            Self::format_number(sheet.position.1)
        ));

        self.write_line(output, &format!(
            "(size {} {})",
            Self::format_number(sheet.size.0),
            Self::format_number(sheet.size.1)
        ));

        self.generate_stroke(output, &sheet.stroke);

        // Sheet fill uses (fill (color R G B A)) format with float alpha
        self.generate_sheet_fill(output, &sheet.fill);

        self.generate_sheet_property(output, "Sheetname", &sheet.sheet_name);
        self.generate_sheet_property(output, "Sheetfile", &sheet.sheet_file);

        let distributed = Self::distribute_sheet_pins(sheet);
        for pin in &distributed {
            self.generate_sheet_pin(output, pin);
        }

        self.indent_level -= 1;
        self.write_line(output, ")");
    }

    fn generate_sheet_fill(&mut self, output: &mut String, fill: &crate::ir::Fill) {
        if let Some((r, g, b, a)) = &fill.color {
            // Sheet fill uses float alpha (0.0-1.0), not integer
            let a_float = *a as f64 / 255.0;
            self.write_line(output, &format!(
                "(fill (color {} {} {} {}))",
                r, g, b, Self::format_number(a_float)
            ));
        } else {
            self.write_line(output, "(fill (type none))");
        }
    }

    fn generate_sheet_property(&mut self, output: &mut String, prop_name: &str, prop: &SheetProperty) {
        self.write_line(output, &format!("(property \"{}\"", prop_name));
        self.indent_level += 1;

        self.write_line(output, &format!("\"{}\"", Self::escape_string(&prop.value)));
        self.write_line(
            output,
            &Self::format_at(prop.position.0, prop.position.1, prop.position.2),
        );

        self.generate_effects(output, &prop.effects);

        self.indent_level -= 1;
        self.write_line(output, ")");
    }

    fn generate_sheet_pin(&mut self, output: &mut String, pin: &SheetPin) {
        let pin_type_str = pin.pin_type.to_str();
        self.write_line(output, &format!("(pin \"{}\" {}", Self::escape_string(&pin.name), pin_type_str));
        self.indent_level += 1;

        self.write_line(
            output,
            &Self::format_at(pin.position.0, pin.position.1, pin.position.2),
        );

        self.generate_effects(output, &pin.effects);

        if self.config.include_uuids {
            self.write_line(output, &format!("(uuid \"{}\")", Self::new_uuid()));
        }

        self.indent_level -= 1;
        self.write_line(output, ")");
    }

    /// Compute distributed pin positions for a sheet.
    /// If all pins are at (0,0), auto-distributes them along sheet edges:
    /// Input pins → left edge, Output pins → right edge, others → bottom edge.
    /// Otherwise returns the original pins.
    pub(super) fn distribute_sheet_pins(sheet: &Sheet) -> Vec<SheetPin> {
        let all_at_origin = sheet.pins.iter().all(|p| p.position.0 == 0.0 && p.position.1 == 0.0);
        if !all_at_origin || sheet.pins.is_empty() {
            return sheet.pins.clone();
        }

        // KiCad uses ABSOLUTE coordinates for sheet pin (at), not relative.
        let (sx, sy) = sheet.position;
        let (w, h) = sheet.size;
        let left_pins: Vec<&SheetPin> = sheet.pins.iter()
            .filter(|p| matches!(p.pin_type, crate::ir::PinType::Input)).collect();
        let right_pins: Vec<&SheetPin> = sheet.pins.iter()
            .filter(|p| matches!(p.pin_type, crate::ir::PinType::Output)).collect();
        let other_pins: Vec<&SheetPin> = sheet.pins.iter()
            .filter(|p| !matches!(p.pin_type, crate::ir::PinType::Input | crate::ir::PinType::Output))
            .collect();

        // Snap value to 1.27mm grid for KiCad connection compatibility
        let snap = |v: f64| -> f64 { (v / 1.27).round() * 1.27 };

        let mut result = Vec::with_capacity(sheet.pins.len());

        if !left_pins.is_empty() {
            let step = h / (left_pins.len() as f64 + 1.0);
            for (i, pin) in left_pins.iter().enumerate() {
                let y = snap(sy + step * (i as f64 + 1.0));
                result.push(SheetPin {
                    name: pin.name.clone(),
                    pin_type: pin.pin_type,
                    position: (sx, y, 180.0),
                    effects: pin.effects.clone(),
                });
            }
        }

        if !right_pins.is_empty() {
            let step = h / (right_pins.len() as f64 + 1.0);
            for (i, pin) in right_pins.iter().enumerate() {
                let y = snap(sy + step * (i as f64 + 1.0));
                result.push(SheetPin {
                    name: pin.name.clone(),
                    pin_type: pin.pin_type,
                    position: (sx + w, y, 0.0),
                    effects: pin.effects.clone(),
                });
            }
        }

        if !other_pins.is_empty() {
            let step = w / (other_pins.len() as f64 + 1.0);
            for (i, pin) in other_pins.iter().enumerate() {
                let x = snap(sx + step * (i as f64 + 1.0));
                result.push(SheetPin {
                    name: pin.name.clone(),
                    pin_type: pin.pin_type,
                    position: (x, sy + h, 90.0),
                    effects: pin.effects.clone(),
                });
            }
        }

        let name_order: Vec<&str> = sheet.pins.iter().map(|p| p.name.as_str()).collect();
        result.sort_by(|a, b| {
            let ai = name_order.iter().position(|n| *n == a.name).unwrap_or(999);
            let bi = name_order.iter().position(|n| *n == b.name).unwrap_or(999);
            ai.cmp(&bi)
        });

        result
    }
}
