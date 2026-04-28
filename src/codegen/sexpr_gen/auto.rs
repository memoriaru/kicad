use super::*;
use crate::ir::RenderHint;
use std::collections::HashMap;

/// Computed world position of a pin with its net context
struct PinInfo {
    x: f64,
    y: f64,
    rotation: f64,
    #[allow(dead_code)]
    net_id: u32,
}

impl SexprGenerator {
    // ============== Auto Connection Generation ==============

    /// Compute local pin positions for a symbol based on its template type.
    /// Returns HashMap<pin_number, (local_x, local_y)>.
    pub(super) fn compute_pin_positions(symbol: &Symbol) -> HashMap<String, (f64, f64)> {
        // Standard Device symbols (Device:R, Device:C, etc.) have fixed pin
        // positions defined in the embedded standard symbol data — use those.
        if let Some(positions) = Self::standard_pin_positions(symbol) {
            return positions;
        }

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
            DefaultSymbolKind::Resistor | DefaultSymbolKind::TwoPin => {
                let pin1 = Self::get_pin_or_default(symbol, 0, "1");
                positions.insert(pin1.number.clone(), (0.0, 2.54));
                let pin2 = Self::get_pin_or_default(symbol, 1, "2");
                positions.insert(pin2.number.clone(), (0.0, -2.54));
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

    /// Return pin positions for standard Device library symbols.
    /// These have fixed pin layouts defined in the embedded standard symbol data.
    fn standard_pin_positions(symbol: &Symbol) -> Option<HashMap<String, (f64, f64)>> {
        if crate::codegen::standard_symbols::get_standard_symbol(&symbol.lib_id).is_none() {
            return None;
        }
        let kind = Self::detect_default_kind(symbol);
        let mut positions = HashMap::new();
        match kind {
            DefaultSymbolKind::Resistor | DefaultSymbolKind::TwoPin
            | DefaultSymbolKind::Capacitor | DefaultSymbolKind::Inductor => {
                // Standard R/C/L/NTC: pins at (0, ±3.81)
                let pin1 = Self::get_pin_or_default(symbol, 0, "1");
                positions.insert(pin1.number.clone(), (0.0, 3.81));
                let pin2 = Self::get_pin_or_default(symbol, 1, "2");
                positions.insert(pin2.number.clone(), (0.0, -3.81));
            }
            DefaultSymbolKind::Diode | DefaultSymbolKind::Led => {
                // Standard D/LED: pins at (±3.81, 0)
                let pin1 = Self::get_pin_or_default(symbol, 0, "1");
                positions.insert(pin1.number.clone(), (-3.81, 0.0));
                let pin2 = Self::get_pin_or_default(symbol, 1, "2");
                positions.insert(pin2.number.clone(), (3.81, 0.0));
            }
            _ => return None, // ICs, connectors — not standard symbols
        }
        Some(positions)
    }

    pub(super) fn normalize_lib_id(lib_id: &str) -> String {
        if lib_id.contains(':') {
            lib_id.to_string()
        } else {
            format!("custom:{}", lib_id)
        }
    }

    pub(super) fn rotate_point(lx: f64, ly: f64, rotation_deg: f64) -> (f64, f64) {
        match rotation_deg as i32 {
            0 | 360 => (lx, ly),
            90 | -270 => (ly, -lx),
            180 | -180 => (-lx, -ly),
            270 | -90 => (-ly, lx),
            _ => {
                let rad = rotation_deg.to_radians();
                let c = rad.cos();
                let s = rad.sin();
                (lx * c + ly * s, -lx * s + ly * c)
            }
        }
    }

    pub(super) fn compute_label_rotation(lx: f64, ly: f64, crot: f64) -> f64 {
        let local_rot = if lx.abs() > ly.abs() {
            if lx < 0.0 { 0.0 } else { 180.0 }
        } else {
            if ly > 0.0 { 90.0 } else { 270.0 }
        };
        (local_rot + crot) % 360.0
    }

    /// Collect all pin world positions grouped by net_id.
    fn collect_pin_world_positions(
        schematic: &crate::ir::Schematic,
        symbol_pins: &HashMap<String, HashMap<String, (f64, f64)>>,
    ) -> (HashMap<u32, Vec<PinInfo>>, Vec<(f64, f64, f64, u32)>) {
        let mut pins_by_net: HashMap<u32, Vec<PinInfo>> = HashMap::new();
        let mut no_connect_positions: Vec<(f64, f64, f64, u32)> = Vec::new();

        for comp in &schematic.components {
            let comp_lib_id = Self::normalize_lib_id(&comp.lib_id);
            let pin_positions = match symbol_pins.get(&comp_lib_id) {
                Some(p) => p,
                None => continue,
            };
            let (cx, cy, crot) = comp.position;

            for pin in &comp.pins {
                if let Some(&(lx, ly)) = pin_positions.get(&pin.number) {
                    let (rx, ry) = Self::rotate_point(lx, -ly, crot);
                    let wx = cx + rx;
                    let wy = cy + ry;

                    if pin.nc {
                        let rot = Self::compute_label_rotation(lx, -ly, crot);
                        no_connect_positions.push((wx, wy, rot, 0));
                    } else if let Some(net_id) = pin.net_id {
                        let rot = Self::compute_label_rotation(lx, -ly, crot);
                        pins_by_net.entry(net_id).or_default().push(PinInfo {
                            x: wx, y: wy, rotation: rot, net_id,
                        });
                    }
                }
            }
        }

        (pins_by_net, no_connect_positions)
    }

    /// Auto-generate connections from net connectivity.
    /// Dispatches per-net based on `render` hint: wire, label, or power.
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

        // 2. Collect pin world positions grouped by net
        let (pins_by_net, _no_connects) = Self::collect_pin_world_positions(schematic, &symbol_pins);

        // 3. Build net lookups
        let net_names: HashMap<u32, &str> = schematic.nets.iter()
            .map(|n| (n.id, n.name.as_str()))
            .collect();
        let net_renders: HashMap<u32, RenderHint> = schematic.nets.iter()
            .map(|n| (n.id, n.render))
            .collect();

        // 4. Collect all label positions (for power_flags backward compat)
        let mut all_label_positions: Vec<(f64, f64, f64, &str)> = Vec::new();

        // 5. Dispatch per-net based on render hint
        let mut sorted_net_ids: Vec<u32> = pins_by_net.keys().copied().collect();
        sorted_net_ids.sort();

        for net_id in &sorted_net_ids {
            let pins = pins_by_net.get(net_id).unwrap();
            let render = net_renders.get(net_id).copied().unwrap_or_default();
            let net_name = net_names.get(net_id).copied().unwrap_or("?");

            match render {
                RenderHint::Wire => self.render_net_wires(output, *net_id, net_name, pins, &mut all_label_positions),
                RenderHint::Label => self.render_net_labels(output, net_name, pins, &mut all_label_positions),
                RenderHint::Power => self.render_net_power(output, *net_id, net_name, pins, &mut all_label_positions, schematic),
            }
        }

        // 6. Generate no_connect symbols
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

        // 7. Generate PWR_FLAG symbols if configured
        if self.config.insert_power_flags {
            self.generate_power_flags(output, schematic, &all_label_positions);
        }
    }

    // ============== Render: Label (existing behavior) ==============

    fn render_net_labels<'a>(
        &mut self,
        output: &mut String,
        net_name: &'a str,
        pins: &[PinInfo],
        all_label_positions: &mut Vec<(f64, f64, f64, &'a str)>,
    ) {
        let default_effects = TextEffects::default();

        for pin in pins {
            all_label_positions.push((pin.x, pin.y, pin.rotation, net_name));
            self.write_line(output, "(global_label");
            self.indent_level += 1;
            self.write_line(output, &format!("\"{}\"", Self::escape_string(net_name)));
            self.write_line(output, &Self::format_at(pin.x, pin.y, pin.rotation));
            self.write_line(output, "(shape passive)");
            self.generate_effects(output, &default_effects);
            if self.config.include_uuids {
                self.write_line(output, &format!("(uuid \"{}\")", Self::new_uuid()));
            }
            self.indent_level -= 1;
            self.write_line(output, ")");
        }
    }

    // ============== Render: Wire ==============

    fn render_net_wires<'a>(
        &mut self,
        output: &mut String,
        _net_id: u32,
        net_name: &'a str,
        pins: &[PinInfo],
        all_label_positions: &mut Vec<(f64, f64, f64, &'a str)>,
    ) {
        if pins.is_empty() {
            return;
        }

        if pins.len() == 1 {
            self.render_net_labels(output, net_name, pins, all_label_positions);
            return;
        }

        // All pins get a global_label first — net connectivity is established
        // by labels, wires are visual aids only.
        let default_effects = TextEffects::default();
        for pin in pins {
            all_label_positions.push((pin.x, pin.y, pin.rotation, net_name));
            self.write_line(output, "(global_label");
            self.indent_level += 1;
            self.write_line(output, &format!("\"{}\"", Self::escape_string(net_name)));
            self.write_line(output, &Self::format_at(pin.x, pin.y, pin.rotation));
            self.write_line(output, "(shape passive)");
            self.generate_effects(output, &default_effects);
            if self.config.include_uuids {
                self.write_line(output, &format!("(uuid \"{}\")", Self::new_uuid()));
            }
            self.indent_level -= 1;
            self.write_line(output, ")");
        }

        // Draw straight wires between aligned same-net pins for visual clarity.
        // Only horizontal or vertical wires — no L-shapes to avoid crossing
        // pins of other nets. All net connectivity is already via global_labels.
        const ALIGN_TOL: f64 = 0.5;
        const MAX_WIRE_LEN: f64 = 20.0;
        let default_stroke = Stroke::default();

        for i in 0..pins.len() {
            for j in (i+1)..pins.len() {
                let (x1, y1) = (pins[i].x, pins[i].y);
                let (x2, y2) = (pins[j].x, pins[j].y);

                if (y1 - y2).abs() < ALIGN_TOL && (x1 - x2).abs() <= MAX_WIRE_LEN {
                    // Same row: horizontal wire
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
                } else if (x1 - x2).abs() < ALIGN_TOL && (y1 - y2).abs() <= MAX_WIRE_LEN {
                    // Same column: vertical wire
                    self.write_line(output, "(wire");
                    self.indent_level += 1;
                    self.write_line(output, &format!(
                        "(pts {} {})",
                        Self::format_xy(x1, y1),
                        Self::format_xy(x1, y2)
                    ));
                    self.generate_stroke(output, &default_stroke);
                    self.indent_level -= 1;
                    self.write_line(output, ")");
                }
            }
        }
    }

    // ============== Render: Power ==============

    fn render_net_power<'a>(
        &mut self,
        output: &mut String,
        net_id: u32,
        net_name: &'a str,
        pins: &[PinInfo],
        all_label_positions: &mut Vec<(f64, f64, f64, &'a str)>,
        schematic: &crate::ir::Schematic,
    ) {
        if pins.is_empty() {
            return;
        }

        let lib_id = match Self::resolve_power_symbol(net_name) {
            Some(id) => id,
            None => {
                // Cannot resolve to a power symbol — fall back to label
                self.render_net_labels(output, net_name, pins, all_label_positions);
                return;
            }
        };

        // Check if the net already has a power_out pin on any connected component.
        // If so, skip the power symbol to avoid power_out-to-power_out conflict.
        let has_power_out = Self::net_has_power_out_pin(schematic, net_id);
        if has_power_out {
            self.render_net_labels(output, net_name, pins, all_label_positions);
            return;
        }

        let is_v9_plus = matches!(self.effective_version, KicadVersion::V9 | KicadVersion::V10);
        let first_pin = &pins[0];

        // Generate power symbol instance at the first pin
        self.write_line(output, "(symbol");
        self.indent_level += 1;
        self.write_line(output, &format!("(lib_id \"{}\")", lib_id));
        self.write_line(output, &Self::format_at(first_pin.x, first_pin.y, 0.0));
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
            "(property \"Reference\" \"#PWR??\" {} (effects (font (size 1.27 1.27)) (hide yes)))",
            Self::format_at(first_pin.x, first_pin.y, 0.0)
        ));
        self.write_line(output, &format!(
            "(property \"Value\" \"{}\" {} (effects (font (size 1.27 1.27))))",
            Self::escape_string(net_name),
            Self::format_at(first_pin.x, first_pin.y, 0.0)
        ));
        if self.config.include_uuids {
            self.write_line(output, &format!(
                "(pin \"1\" (uuid \"{}\"))",
                Self::new_uuid()
            ));
        }
        self.indent_level -= 1;
        self.write_line(output, ")");

        // All pins (including the first) get a global_label for connectivity.
        // The power symbol provides visual representation; the label ensures
        // KiCad ERC/netlist recognizes the electrical connection.
        for pin in pins {
            all_label_positions.push((pin.x, pin.y, pin.rotation, net_name));
            let default_effects = TextEffects::default();
            self.write_line(output, "(global_label");
            self.indent_level += 1;
            self.write_line(output, &format!("\"{}\"", Self::escape_string(net_name)));
            self.write_line(output, &Self::format_at(pin.x, pin.y, pin.rotation));
            self.write_line(output, "(shape passive)");
            self.generate_effects(output, &default_effects);
            if self.config.include_uuids {
                self.write_line(output, &format!("(uuid \"{}\")", Self::new_uuid()));
            }
            self.indent_level -= 1;
            self.write_line(output, ")");
        }
    }

    /// Map a net name to a KiCad power symbol lib_id.
    /// Check if any component pin on the given net has type power_out.
    fn net_has_power_out_pin(schematic: &crate::ir::Schematic, net_id: u32) -> bool {
        let mut lib_pin_types: HashMap<&str, HashMap<&str, &str>> = HashMap::new();
        for symbol in &schematic.lib_symbols {
            let mut pin_map: HashMap<&str, &str> = HashMap::new();
            for pin in &symbol.pins {
                pin_map.insert(&pin.number, &pin.pin_type);
            }
            lib_pin_types.insert(&symbol.lib_id, pin_map);
        }
        for comp in &schematic.components {
            let comp_lib_id = &comp.lib_id;
            let pin_type_map = match lib_pin_types.get(comp_lib_id.as_str()) {
                Some(m) => m,
                None => continue,
            };
            for pin in &comp.pins {
                if let Some(nid) = pin.net_id {
                    if nid == net_id {
                        let ptype = pin_type_map.get(pin.number.as_str()).unwrap_or(&"passive");
                        if *ptype == "power_out" {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    /// Only matches names that correspond to standard KiCad power symbols
    /// (power:+N, power:-N, power:GND, power:VCC, etc.) to avoid Value/net
    /// name conflicts — a power symbol's Value determines its net, so the
    /// net name must exactly match the symbol name.
    pub(super) fn resolve_power_symbol(net_name: &str) -> Option<String> {
        let upper = net_name.to_uppercase();
        // Standard voltage rails: +3V3, +5V, +15V, -9V, -4V, etc.
        if upper.starts_with('+') || upper.starts_with('-') {
            return Some(format!("power:{}", net_name));
        }
        // Standard ground and VCC: exact match only (not PGND, AGND, etc.)
        if upper == "GND" || upper == "VCC" || upper == "GNDA" || upper == "GNDD" {
            return Some(format!("power:{}", net_name));
        }
        None
    }

    // ============== Power Flags (existing) ==============

    /// Place PWR_FLAG instances on nets with power_in pins but no power_out.
    pub(super) fn generate_power_flags(
        &mut self,
        output: &mut String,
        schematic: &crate::ir::Schematic,
        labels_to_place: &[(f64, f64, f64, &str)],
    ) {
        let is_v9_plus = matches!(self.effective_version, KicadVersion::V9 | KicadVersion::V10);

        let mut lib_pin_types: HashMap<String, HashMap<&str, &str>> = HashMap::new();
        for symbol in &schematic.lib_symbols {
            let mut pin_map: HashMap<&str, &str> = HashMap::new();
            for pin in &symbol.pins {
                pin_map.insert(&pin.number, &pin.pin_type);
            }
            lib_pin_types.insert(Self::normalize_lib_id(&symbol.lib_id), pin_map);
        }

        let mut net_pin_types: HashMap<u32, Vec<&str>> = HashMap::new();
        for comp in &schematic.components {
            let comp_lib_id = Self::normalize_lib_id(&comp.lib_id);
            let pin_type_map = match lib_pin_types.get(&comp_lib_id) {
                Some(m) => m,
                None => continue,
            };
            for pin in &comp.pins {
                if let Some(net_id) = pin.net_id {
                    let ptype = pin_type_map.get(pin.number.as_str()).unwrap_or(&"passive");
                    net_pin_types.entry(net_id).or_default().push(ptype);
                }
            }
        }

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

        let mut net_positions: HashMap<&str, (f64, f64)> = HashMap::new();
        for &(x, y, _, name) in labels_to_place {
            if power_flag_nets.contains(&name) && !net_positions.contains_key(&name) {
                net_positions.insert(name, (x, y));
            }
        }

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
