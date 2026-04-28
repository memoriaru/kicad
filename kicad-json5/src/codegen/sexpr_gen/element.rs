use super::*;

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

    pub(super) fn generate_net(&mut self, output: &mut String, net: &Net) {
        self.write_line(output, "(net");
        self.indent_level += 1;

        self.write_line(output, &format!("{} \"{}\"", net.id, Self::escape_string(&net.name)));

        if let Some(net_type) = &net.net_type {
            self.write_line(output, &format!("(type \"{}\")", net_type));
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
}
