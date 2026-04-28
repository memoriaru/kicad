use super::*;
use std::collections::HashMap;

impl SexprGenerator {
    // ============== Auto Wire Generation ==============

    /// Compute local pin positions for a symbol based on its template type.
    /// Returns HashMap<pin_number, (local_x, local_y)>.
    /// Uses the same pin numbering as gen_*_unit (get_pin_or_default) to avoid mismatches.
    pub(super) fn compute_pin_positions(symbol: &Symbol) -> HashMap<String, (f64, f64)> {
        let kind = Self::detect_default_kind(symbol);
        let mut positions = HashMap::new();

        match kind {
            DefaultSymbolKind::Connector => {
                let pin_count = symbol.pins.len();
                if pin_count == 0 { return positions; }
                let short = symbol.lib_id.split(':').last().unwrap_or(&symbol.lib_id);
                let is_dual_row = short.contains("02x") || short.contains("_02x");
                let spacing = 2.54;

                if is_dual_row {
                    let rows = (pin_count + 1) / 2;
                    for (i, pin) in symbol.pins.iter().enumerate() {
                        let row = i / 2;
                        let y = ((rows - 1) - row) as f64 * spacing;
                        if i % 2 == 0 {
                            positions.insert(pin.number.clone(), (-5.08, y));
                        } else {
                            positions.insert(pin.number.clone(), (7.62, y));
                        }
                    }
                } else {
                    for (i, pin) in symbol.pins.iter().enumerate() {
                        let y = ((pin_count - 1) - i) as f64 * spacing;
                        positions.insert(pin.number.clone(), (-5.08, y));
                    }
                }
            }
            DefaultSymbolKind::Ic => {
                let pin_count = symbol.pins.len();
                let left_count = (pin_count + 1) / 2;
                let right_count = pin_count - left_count;
                let spacing = 2.54;
                let body_hw = 5.08;
                let pin_length = 2.54;

                for (i, pin) in symbol.pins.iter().enumerate() {
                    if i < left_count {
                        let y = (left_count - 1 - i) as f64 * spacing / 2.0;
                        positions.insert(pin.number.clone(), (-body_hw - pin_length, y));
                    } else {
                        let ri = i - left_count;
                        let y = (right_count - 1 - ri) as f64 * spacing / 2.0;
                        positions.insert(pin.number.clone(), (body_hw + pin_length, y));
                    }
                }
            }
            // 2-pin passives: use pin at positions from KiCad standard symbols.
            // These are the (at x y angle) values in the lib_symbol definition,
            // which match what SCH_PIN::GetPosition() returns after Y-negation.
            DefaultSymbolKind::Resistor | DefaultSymbolKind::TwoPin => {
                let pin1 = Self::get_pin_or_default(symbol, 0, "1");
                positions.insert(pin1.number.clone(), (0.0, 3.81));
                let pin2 = Self::get_pin_or_default(symbol, 1, "2");
                positions.insert(pin2.number.clone(), (0.0, -3.81));
            }
            DefaultSymbolKind::Capacitor | DefaultSymbolKind::Inductor => {
                let pin1 = Self::get_pin_or_default(symbol, 0, "1");
                positions.insert(pin1.number.clone(), (0.0, 3.81));
                let pin2 = Self::get_pin_or_default(symbol, 1, "2");
                positions.insert(pin2.number.clone(), (0.0, -3.81));
            }
            DefaultSymbolKind::Diode | DefaultSymbolKind::Led => {
                let pin1 = Self::get_pin_or_default(symbol, 0, "1");
                positions.insert(pin1.number.clone(), (-3.81, 0.0));
                let pin2 = Self::get_pin_or_default(symbol, 1, "2");
                positions.insert(pin2.number.clone(), (3.81, 0.0));
            }
        }

        positions
    }

    /// Normalize lib_id: ensure it has a library prefix (LibraryName:SymbolName).
    /// "Device:R" -> unchanged; "FP6277" -> "custom:FP6277".
    pub(super) fn normalize_lib_id(lib_id: &str) -> String {
        if lib_id.contains(':') {
            lib_id.to_string()
        } else {
            format!("custom:{}", lib_id)
        }
    }

    /// Rotate a local point by the component's rotation angle (degrees).
    /// KiCad uses CLOCKWISE rotation: 0=(1,0,0,1), 90=(0,1,-1,0), 180=(-1,0,0,-1), 270=(0,-1,1,0)
    pub(super) fn rotate_point(lx: f64, ly: f64, rotation_deg: f64) -> (f64, f64) {
        match rotation_deg as i32 {
            0 | 360 => (lx, ly),
            90 | -270 => (ly, -lx),
            180 | -180 => (-lx, -ly),
            270 | -90 => (-ly, lx),
            _ => {
                // Generic CW rotation
                let rad = rotation_deg.to_radians();
                let c = rad.cos();
                let s = rad.sin();
                (lx * c + ly * s, -lx * s + ly * c)
            }
        }
    }

    /// Transform a local file-coordinate offset by component rotation.
    /// Input (lx, ly) is in file coordinates (Y-DOWN) relative to symbol origin.
    /// Output is the file-coordinate offset to add to the component's (at) position.
    ///
    /// KiCad internally uses Y-UP; parseXY(true) negates Y when loading.
    /// The net effect on file coords is:
    ///   0 deg -> (lx, ly),  90 deg -> (-ly, lx),  180 deg -> (-lx, -ly),  270 deg -> (ly, -lx)
    pub(super) fn transform_file_offset(lx: f64, ly: f64, rotation_deg: f64) -> (f64, f64) {
        match rotation_deg as i32 {
            0 | 360 => (lx, ly),
            90 | -270 => (-ly, lx),
            180 | -180 => (-lx, -ly),
            270 | -90 => (ly, -lx),
            _ => {
                // Generic: same as KiCad's internal transform applied to file coords
                let (ix, iy) = (lx, -ly); // file -> internal
                let rad = rotation_deg.to_radians();
                let c = rad.cos();
                let s = rad.sin();
                let rx = ix * c + iy * s;
                let ry = -ix * s + iy * c;
                (rx, -ry) // internal -> file
            }
        }
    }

    /// Compute label rotation from pin local position and component rotation.
    /// The label should face the same direction as the pin's wire connection point.
    pub(super) fn compute_label_rotation(lx: f64, ly: f64, crot: f64) -> f64 {
        // Determine pin direction in file coordinates (before component rotation).
        // In gen_ic_unit/gen_passive_unit:
        //   Left pins:   x < 0, pin points RIGHT  -> label rotation 0
        //   Right pins:  x > 0, pin points LEFT   -> label rotation 180
        //   Top pins:    y > 0 (file coords), pin points DOWN -> label rotation 90
        //   Bottom pins: y < 0, pin points UP   -> label rotation 270
        let local_rot = if lx.abs() > ly.abs() {
            if lx < 0.0 { 0.0 } else { 180.0 }
        } else {
            if ly > 0.0 { 90.0 } else { 270.0 }
        };
        // Apply component rotation
        (local_rot + crot) % 360.0
    }

    /// Auto-generate wires from net connectivity.
    pub(super) fn generate_auto_wires(
        &mut self,
        output: &mut String,
        schematic: &crate::ir::Schematic,
    ) {
        // 1. Build pin position map for each lib_symbol: lib_id -> { pin_number -> (lx, ly) }
        let mut symbol_pins: HashMap<String, HashMap<String, (f64, f64)>> = HashMap::new();
        for symbol in &schematic.lib_symbols {
            let positions = Self::compute_pin_positions(symbol);
            symbol_pins.insert(Self::normalize_lib_id(&symbol.lib_id), positions);
        }

        // 2. Build net -> list of absolute pin positions
        let mut net_endpoints: HashMap<u32, Vec<(f64, f64)>> = HashMap::new();
        for comp in &schematic.components {
            let comp_lib_id = Self::normalize_lib_id(&comp.lib_id);
            let pin_positions = match symbol_pins.get(&comp_lib_id) {
                Some(p) => p,
                None => continue,
            };
            let (cx, cy, crot) = comp.position;

            for pin in &comp.pins {
                if let (Some(net_id), Some(&(lx, ly))) = (pin.net_id, pin_positions.get(&pin.number)) {
                    let (rx, ry) = Self::rotate_point(lx, ly, crot);
                    net_endpoints
                        .entry(net_id)
                        .or_default()
                        .push((cx + rx, cy + ry));
                }
            }
        }

        // 3. Generate wires for each net using L-shaped connections
        let default_stroke = Stroke::default();

        let mut sorted_nets: Vec<_> = net_endpoints.into_iter().collect();
        sorted_nets.sort_by_key(|(id, _)| *id);

        for (_net_id, mut pts) in sorted_nets {
            if pts.len() < 2 {
                continue;
            }

            // Snap to 1.27mm (50mil) grid
            let snap = |v: f64| (v / 1.27).round() * 1.27;
            for p in &mut pts {
                p.0 = snap(p.0);
                p.1 = snap(p.1);
            }

            // Sort by x coordinate, then by y
            pts.sort_by(|a, b| {
                a.0.partial_cmp(&b.0)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            });

            // Connect consecutive pins with L-shaped wires
            for i in 0..pts.len() - 1 {
                let (x1, y1) = pts[i];
                let (x2, y2) = pts[i + 1];

                // Horizontal segment: (x1, y1) -> (x2, y1)
                if (x1 - x2).abs() > 0.01 {
                    self.write_line(output, "(wire");
                    self.indent_level += 1;
                    self.write_line(output, &format!(
                        "(pts {} {})",
                        Self::format_xy(x1, y1),
                        Self::format_xy(x2, y1)
                    ));
                    self.generate_stroke(output, &default_stroke);
                    self.indent_level -= 1;
                    self.write_line(output, ")");
                }

                // Vertical segment: (x2, y1) -> (x2, y2)
                if (y1 - y2).abs() > 0.01 {
                    self.write_line(output, "(wire");
                    self.indent_level += 1;
                    self.write_line(output, &format!(
                        "(pts {} {})",
                        Self::format_xy(x2, y1),
                        Self::format_xy(x2, y2)
                    ));
                    self.generate_stroke(output, &default_stroke);
                    self.indent_level -= 1;
                    self.write_line(output, ")");
                }
            }
        }
    }

    /// Auto-generate labels from net connectivity.
    /// Instead of L-shaped wires, place a label at each pin's world coordinate.
    /// Pins sharing the same net_id get the same label text, so KiCad connects them implicitly.
    pub fn generate_auto_labels(
        &mut self,
        output: &mut String,
        schematic: &crate::ir::Schematic,
    ) {
        // 1. Build pin position map for each lib_symbol
        let mut symbol_pins: HashMap<String, HashMap<String, (f64, f64)>> = HashMap::new();
        for symbol in &schematic.lib_symbols {
            let positions = Self::compute_pin_positions(symbol);
            symbol_pins.insert(Self::normalize_lib_id(&symbol.lib_id), positions);
        }

        // 2. Build net_id -> net_name lookup
        let net_names: HashMap<u32, &str> = schematic.nets.iter()
            .map(|n| (n.id, n.name.as_str()))
            .collect();

        // 3. Collect all (x, y, rotation, net_name) for pins with net assignments
        let mut labels_to_place: Vec<(f64, f64, f64, &str)> = Vec::new();

        for comp in &schematic.components {
            let comp_lib_id = Self::normalize_lib_id(&comp.lib_id);
            let pin_positions = match symbol_pins.get(&comp_lib_id) {
                Some(p) => p,
                None => continue,
            };
            let (cx, cy, crot) = comp.position;

            for pin in &comp.pins {
                if let (Some(net_id), Some(&(lx, ly))) = (pin.net_id, pin_positions.get(&pin.number)) {
                    if let Some(net_name) = net_names.get(&net_id) {
                        // Pin world position in file coordinates:
                        // KiCad internally Y-negates pin at positions (parseXY(true)),
                        // so we negate Y here to match. Then apply component rotation.
                        let (rx, ry) = Self::rotate_point(lx, -ly, crot);
                        let wx = cx + rx;
                        let wy = cy + ry;

                        // Label rotation: based on pin direction in file coordinates.
                        // In the file, pin direction points AWAY from the symbol body
                        // (toward the wire connection end). The label should face the
                        // same direction as the pin so it reads naturally.
                        let label_rot = Self::compute_label_rotation(lx, ly, crot);

                        labels_to_place.push((wx, wy, label_rot, *net_name));
                    }
                }
            }
        }

        // 4. Sort by net_name then position for deterministic output
        labels_to_place.sort_by(|a, b| {
            a.3.cmp(b.3)
                .then(a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
                .then(a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        });

        // 5. Generate one label per pin at its world coordinate.
        // KiCad connects pins that share the same label text at the same location.
        // Do NOT snap to grid -- the label must be at the exact pin connection point.
        let default_effects = TextEffects::default();

        for (x, y, rot, net_name) in &labels_to_place {
            // Use local net labels (plain text) for most signals.
            // Local labels connect pins within the same sheet -- cleaner visually.
            self.write_line(output, "(label");
            self.indent_level += 1;
            self.write_line(output, &format!("\"{}\"", Self::escape_string(net_name)));
            self.write_line(output, &Self::format_at(*x, *y, *rot));
            self.generate_effects(output, &default_effects);
            if self.config.include_uuids {
                self.write_line(output, &format!("(uuid \"{}\")", Self::new_uuid()));
            }
            self.indent_level -= 1;
            self.write_line(output, ")");
        }

        // 6. Generate (no_connect ...) for pins marked nc=true in JSON5
        for comp in &schematic.components {
            let comp_lib_id = Self::normalize_lib_id(&comp.lib_id);
            let pin_positions = match symbol_pins.get(&comp_lib_id) {
                Some(p) => p,
                None => continue,
            };
            let (cx, cy, crot) = comp.position;
            for pin in &comp.pins {
                if pin.nc {
                    if let Some(&(lx, ly)) = pin_positions.get(&pin.number) {
                        let (rx, ry) = Self::rotate_point(lx, -ly, crot);
                        let wx = cx + rx;
                        let wy = cy + ry;
                        self.write_line(output, "(no_connect");
                        self.indent_level += 1;
                        self.write_line(output, &format!("(at {} {})", Self::format_number(wx), Self::format_number(wy)));
                        if self.config.include_uuids {
                            self.write_line(output, &format!("(uuid \"{}\")", Self::new_uuid()));
                        }
                        self.indent_level -= 1;
                        self.write_line(output, ")");
                    }
                }
            }
        }

        // 7. Generate PWR_FLAG symbols for nets that have power_in pins
        //    but no power_out pins, to satisfy KiCad's ERC power-pin-driven check.
        if self.config.insert_power_flags {
            self.generate_power_flags(output, schematic, &labels_to_place);
        }
    }

    /// Place PWR_FLAG instances on nets with power_in pins but no power_out.
    /// PWR_FLAG is a KiCad standard symbol with a power_out pin (length=0 at origin).
    /// Placing it at any label position for the target net marks that net as driven.
    pub(super) fn generate_power_flags(
        &mut self,
        output: &mut String,
        schematic: &crate::ir::Schematic,
        labels_to_place: &[(f64, f64, f64, &str)],
    ) {
        let is_v9_plus = matches!(self.effective_version, KicadVersion::V9 | KicadVersion::V10);

        // Build lib_id -> { pin_number -> pin_type } from lib_symbols
        let mut lib_pin_types: HashMap<String, HashMap<&str, &str>> = HashMap::new();
        for symbol in &schematic.lib_symbols {
            let mut pin_map: HashMap<&str, &str> = HashMap::new();
            for pin in &symbol.pins {
                pin_map.insert(&pin.number, &pin.pin_type);
            }
            lib_pin_types.insert(Self::normalize_lib_id(&symbol.lib_id), pin_map);
        }

        // Collect net_id -> set of pin types (from lib_symbol definitions)
        let mut net_pin_types: HashMap<u32, Vec<&str>> = HashMap::new();
        for comp in &schematic.components {
            let comp_lib_id = Self::normalize_lib_id(&comp.lib_id);
            let pin_type_map = match lib_pin_types.get(&comp_lib_id) {
                Some(m) => m,
                None => continue,
            };
            for pin in &comp.pins {
                if let Some(net_id) = pin.net_id {
                    // Use lib_symbol's pin type, not component's (component defaults to "passive")
                    let ptype = pin_type_map.get(pin.number.as_str()).unwrap_or(&"passive");
                    net_pin_types.entry(net_id).or_default().push(ptype);
                }
            }
        }

        // Find nets that have power_in but no power_out or output pins.
        // Also skip nets driven by any output-capable pin to avoid pin_to_pin conflicts.
        let net_names: HashMap<u32, &str> = schematic.nets.iter()
            .map(|n| (n.id, n.name.as_str()))
            .collect();

        let mut power_flag_nets: Vec<&str> = Vec::new();
        for (net_id, types) in &net_pin_types {
            let has_power_in = types.iter().any(|t| *t == "power_in");
            let has_driver = types.iter().any(|t| {
                matches!(*t, "power_out" | "output" | "bidirectional")
            });
            if has_power_in && !has_driver {
                if let Some(name) = net_names.get(net_id) {
                    power_flag_nets.push(*name);
                }
            }
        }
        power_flag_nets.sort();
        power_flag_nets.dedup();

        if power_flag_nets.is_empty() {
            return;
        }

        // Find a label position for each net (first occurrence)
        let mut net_positions: HashMap<&str, (f64, f64)> = HashMap::new();
        for &(x, y, _, name) in labels_to_place {
            if power_flag_nets.contains(&name) && !net_positions.contains_key(&name) {
                net_positions.insert(name, (x, y));
            }
        }

        // Generate PWR_FLAG symbol definition in lib_symbols (injected inline)
        // We add it directly before the first PWR_FLAG instance using a comment marker
        // Actually, we need to add it to lib_symbols. But we can't modify lib_symbols here.
        // Instead, we rely on the PWR_FLAG being referenced and KiCad resolving it
        // from its standard power library. For safety, we also emit the lib_symbol.

        // Emit PWR_FLAG lib_symbol definition (if not already present)
        // This gets written into the schematic's symbol instances section as a standalone
        // component that references "power:PWR_FLAG". KiCad will resolve it from the
        // standard power library at load time.

        let mut flag_index: u32 = 1;
        for net_name in &power_flag_nets {
            if let Some(&(x, y)) = net_positions.get(net_name) {
                let ref_name = format!("#FLG{:02}", flag_index);
                flag_index += 1;
                self.write_line(output, "(symbol");
                self.indent_level += 1;
                self.write_line(output, "(lib_id \"power:PWR_FLAG\")");
                self.write_line(output, &Self::format_at(x, y, 0.0));
                self.write_line(output, "(unit 1)");
                if is_v9_plus {
                    self.write_line(output, "(body_style 1)");
                    self.write_line(output, "(exclude_from_sim no)");
                }
                self.write_line(output, "(in_bom yes)");
                self.write_line(output, "(on_board yes)");
                if is_v9_plus {
                    self.write_line(output, "(dnp no)");
                }
                if self.config.include_uuids {
                    self.write_line(output, &format!("(uuid \"{}\")", Self::new_uuid()));
                }
                self.write_line(output, &format!(
                    "(property \"Reference\" \"{}\" {} (effects (font (size 1.27 1.27)) (hide yes)))",
                    ref_name,
                    Self::format_at(x, y, 0.0)
                ));
                self.write_line(output, &format!(
                    "(property \"Value\" \"{}\" {} (effects (font (size 1.27 1.27))))",
                    Self::escape_string(net_name),
                    Self::format_at(x, y, 0.0)
                ));
                if self.config.include_uuids {
                    self.write_line(output, &format!(
                        "(pin \"1\" (uuid \"{}\"))",
                        Self::new_uuid()
                    ));
                }
                self.indent_level -= 1;
                self.write_line(output, ")");
            }
        }
    }
}
