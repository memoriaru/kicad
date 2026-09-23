use super::*;
use crate::ir::RenderHint;
use std::collections::HashMap;

/// Computed world position of a pin with its net context
struct PinInfo {
    x: f64,
    y: f64,
    rotation: f64,
}

/// Axis-aligned bounding box of a component body (for collision detection)
#[derive(Clone)]
struct BodyRect {
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
}

/// Maximum Manhattan distance for wire routing (50mm ≈ 2 inches)
const MAX_WIRE_MANHATTAN: f64 = 50.0;

/// Tolerance for considering two coordinates equal
const COORD_TOL: f64 = 0.5;

/// Manhattan distance between two points
fn manhattan_distance(p1: (f64, f64), p2: (f64, f64)) -> f64 {
    (p1.0 - p2.0).abs() + (p1.1 - p2.1).abs()
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
                if pin_count == 0 {
                    return positions;
                }
                let short = symbol
                    .lib_id
                    .split(':')
                    .next_back()
                    .unwrap_or(&symbol.lib_id);
                let is_dual_row = short.contains("02x") || short.contains("_02x");
                let spacing = 5.08;

                if is_dual_row {
                    let rows = pin_count.div_ceil(2);
                    let body_hw = (5.08_f64.max(rows as f64 * 1.016) / 1.27).round() * 1.27;
                    let pin_length = 3.81;
                    for (i, pin) in symbol.pins.iter().enumerate() {
                        let row = i / 2;
                        let y = ((rows - 1) as f64 / 2.0 - row as f64) * spacing;
                        if i % 2 == 0 {
                            positions.insert(pin.number.clone(), (-(body_hw + pin_length), y));
                        } else {
                            positions.insert(pin.number.clone(), (body_hw + pin_length, y));
                        }
                    }
                } else {
                    let body_hw = 3.81;
                    let pin_length = 3.81;
                    for (i, pin) in symbol.pins.iter().enumerate() {
                        let y = ((pin_count - 1) as f64 / 2.0 - i as f64) * spacing;
                        positions.insert(pin.number.clone(), (body_hw + pin_length, y));
                    }
                }
            }
            DefaultSymbolKind::Ic => {
                let pin_count = symbol.pins.len();
                // >32 pins: two columns would tower >80mm and push pins off the
                // sheet (off-sheet pins are silently dropped from the netlist) —
                // distribute over four sides instead, like a QFP symbol.
                if pin_count > 32 && pin_count.is_multiple_of(4) {
                    let per_side = pin_count / 4;
                    let spacing = 5.08;
                    let half_span = (per_side as f64 - 1.0) / 2.0 * spacing;
                    let body_half = half_span + 2.54;
                    let pin_length = 2.54;
                    for (i, pin) in symbol.pins.iter().enumerate() {
                        let (lx, ly) = if i < per_side {
                            // left, top→bottom
                            (-(body_half + pin_length), half_span - i as f64 * spacing)
                        } else if i < 2 * per_side {
                            // bottom, left→right
                            let j = i - per_side;
                            (-half_span + j as f64 * spacing, -(body_half + pin_length))
                        } else if i < 3 * per_side {
                            // right, bottom→top
                            let j = i - 2 * per_side;
                            (body_half + pin_length, -half_span + j as f64 * spacing)
                        } else {
                            // top, right→left
                            let j = i - 3 * per_side;
                            (half_span - j as f64 * spacing, body_half + pin_length)
                        };
                        positions.insert(pin.number.clone(), (lx, ly));
                    }
                    return positions;
                }
                let left_count = pin_count.div_ceil(2);
                let right_count = pin_count - left_count;
                let max_per_side = left_count.max(right_count).max(1);
                let spacing = 5.08;
                let body_hw = (5.08_f64.max(max_per_side as f64 * 1.016) / 1.27).round() * 1.27;
                let pin_length = 2.54;

                for (i, pin) in symbol.pins.iter().enumerate() {
                    if i < left_count {
                        let y = ((left_count - 1) as f64 / 2.0 - i as f64) * spacing;
                        positions.insert(pin.number.clone(), (-body_hw - pin_length, y));
                    } else {
                        let ri = i - left_count;
                        let y = ((right_count - 1) as f64 / 2.0 - ri as f64) * spacing;
                        positions.insert(pin.number.clone(), (body_hw + pin_length, y));
                    }
                }
            }
            DefaultSymbolKind::Resistor => {
                // Pin at (0, ±5.81) = actual tip in .sexpr
                let pin1 = Self::get_pin_or_default(symbol, 0, "1");
                positions.insert(pin1.number.clone(), (0.0, 5.81));
                let pin2 = Self::get_pin_or_default(symbol, 1, "2");
                positions.insert(pin2.number.clone(), (0.0, -5.81));
            }
            DefaultSymbolKind::TwoPin => {
                let pin1 = Self::get_pin_or_default(symbol, 0, "1");
                positions.insert(pin1.number.clone(), (0.0, 2.54));
                let pin2 = Self::get_pin_or_default(symbol, 1, "2");
                positions.insert(pin2.number.clone(), (0.0, -2.54));
            }
            DefaultSymbolKind::Capacitor => {
                // Pin at (0, ±3.81) = actual tip in .sexpr
                let pin1 = Self::get_pin_or_default(symbol, 0, "1");
                positions.insert(pin1.number.clone(), (0.0, 3.81));
                let pin2 = Self::get_pin_or_default(symbol, 1, "2");
                positions.insert(pin2.number.clone(), (0.0, -3.81));
            }
            DefaultSymbolKind::Inductor => {
                // Pin at (0, ±5.81) = actual tip in .sexpr
                let pin1 = Self::get_pin_or_default(symbol, 0, "1");
                positions.insert(pin1.number.clone(), (0.0, 5.81));
                let pin2 = Self::get_pin_or_default(symbol, 1, "2");
                positions.insert(pin2.number.clone(), (0.0, -5.81));
            }
            DefaultSymbolKind::Diode | DefaultSymbolKind::Led => {
                // gen_diode_unit() generates vertical pins at (0, ±3.81)
                let pin1 = Self::get_pin_or_default(symbol, 0, "1");
                positions.insert(pin1.number.clone(), (0.0, 3.81));
                let pin2 = Self::get_pin_or_default(symbol, 1, "2");
                positions.insert(pin2.number.clone(), (0.0, -3.81));
            }
        }

        positions
    }

    /// Return pin TIP positions for standard Device library symbols.
    /// Must match the positions generated by gen_kicad_pin in symbol.rs,
    /// with pin_length added in the rotation direction.
    fn standard_pin_positions(symbol: &Symbol) -> Option<HashMap<String, (f64, f64)>> {
        crate::codegen::standard_symbols::get_standard_symbol(&symbol.lib_id)?;
        let short = symbol
            .lib_id
            .split(':')
            .next_back()
            .unwrap_or(&symbol.lib_id)
            .to_uppercase();
        let mut positions = HashMap::new();

        // NTC/Thermistor_NTC has different pin positions from resistors: (0, ±3.81) not (0, ±5.81)
        if short == "NTC" || short == "THERMISTOR_NTC" {
            let pin1 = Self::get_pin_or_default(symbol, 0, "1");
            positions.insert(pin1.number.clone(), (0.0, 3.81));
            let pin2 = Self::get_pin_or_default(symbol, 1, "2");
            positions.insert(pin2.number.clone(), (0.0, -3.81));
            return Some(positions);
        }

        let kind = Self::detect_default_kind(symbol);
        match kind {
            DefaultSymbolKind::Resistor => {
                let pin1 = Self::get_pin_or_default(symbol, 0, "1");
                positions.insert(pin1.number.clone(), (0.0, 5.81));
                let pin2 = Self::get_pin_or_default(symbol, 1, "2");
                positions.insert(pin2.number.clone(), (0.0, -5.81));
            }
            DefaultSymbolKind::TwoPin => {
                return None;
            }
            DefaultSymbolKind::Capacitor => {
                let pin1 = Self::get_pin_or_default(symbol, 0, "1");
                positions.insert(pin1.number.clone(), (0.0, 3.81));
                let pin2 = Self::get_pin_or_default(symbol, 1, "2");
                positions.insert(pin2.number.clone(), (0.0, -3.81));
            }
            DefaultSymbolKind::Inductor => {
                let pin1 = Self::get_pin_or_default(symbol, 0, "1");
                positions.insert(pin1.number.clone(), (0.0, 5.81));
                let pin2 = Self::get_pin_or_default(symbol, 1, "2");
                positions.insert(pin2.number.clone(), (0.0, -5.81));
            }
            DefaultSymbolKind::Diode | DefaultSymbolKind::Led => {
                let pin1 = Self::get_pin_or_default(symbol, 0, "1");
                positions.insert(pin1.number.clone(), (-3.81, 0.0));
                let pin2 = Self::get_pin_or_default(symbol, 1, "2");
                positions.insert(pin2.number.clone(), (3.81, 0.0));
            }
            _ => return None,
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

    pub(super) fn compute_label_rotation(lx: f64, _ly: f64) -> f64 {
        if lx.abs() > 0.1 {
            // Side pin (left/right): flag extends away from body
            if lx < 0.0 {
                180.0
            } else {
                0.0
            }
        } else {
            // Top/bottom pin: prefer horizontal label for readability
            // Default to RIGHT (0°) — matches KiCad convention
            0.0
        }
    }

    /// Collect all pin world positions grouped by net_id.
    #[allow(clippy::type_complexity)] // (net → pins, label anchor points)
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
                    let (rx, ry) = Self::rotate_point(lx, ly, crot);
                    let wx = cx + rx;
                    // KiCad lib_symbols pin y is in symbol-local coords (y-up convention):
                    // schematic_y = component_y - rotated_pin_y
                    let wy = cy - ry;

                    if pin.nc {
                        let rot = Self::compute_label_rotation(lx, ly);
                        no_connect_positions.push((wx, wy, rot, 0));
                    } else if let Some(net_id) = pin.net_id {
                        let rot = Self::compute_label_rotation(lx, ly);
                        pins_by_net.entry(net_id).or_default().push(PinInfo {
                            x: wx,
                            y: wy,
                            rotation: rot,
                        });
                    }
                }
            }
        }

        (pins_by_net, no_connect_positions)
    }

    /// Auto-generate connections from net connectivity.
    /// Two-phase: Phase 1 renders wires/labels/power symbols,
    /// Phase 2 generates junctions at wire-pin intersections.
    pub fn generate_auto_labels(&mut self, output: &mut String, schematic: &crate::ir::Schematic) {
        // 1. Build pin position map for each lib_symbol
        let mut symbol_pins: HashMap<String, HashMap<String, (f64, f64)>> = HashMap::new();
        for symbol in &schematic.lib_symbols {
            let positions = Self::compute_pin_positions(symbol);
            symbol_pins.insert(Self::normalize_lib_id(&symbol.lib_id), positions);
        }

        // 2. Collect pin world positions grouped by net
        let (mut pins_by_net, no_connect_positions) =
            Self::collect_pin_world_positions(schematic, &symbol_pins);

        // 3. Build net lookups
        let net_names: HashMap<u32, &str> = schematic
            .nets
            .iter()
            .map(|n| (n.id, n.name.as_str()))
            .collect();
        let net_renders: HashMap<u32, RenderHint> =
            schematic.nets.iter().map(|n| (n.id, n.render)).collect();

        // 3b. Inject hierarchical label positions as virtual pins so wires reach them
        let net_name_to_id: HashMap<&str, u32> = schematic
            .nets
            .iter()
            .map(|n| (n.name.as_str(), n.id))
            .collect();
        for label in &schematic.labels {
            if label.label_type != "hierarchical_label" {
                continue;
            }
            let net_id = match net_name_to_id.get(label.text.as_str()) {
                Some(&id) => id,
                None => continue,
            };
            let render = net_renders.get(&net_id).copied().unwrap_or_default();
            if !matches!(render, RenderHint::Wire | RenderHint::Label) {
                continue;
            }
            pins_by_net.entry(net_id).or_default().push(PinInfo {
                x: label.position.0,
                y: label.position.1,
                rotation: label.position.2,
            });
        }

        // 4. Collect all label positions (for power_flags backward compat)
        let mut all_label_positions: Vec<(f64, f64, f64, &str)> = Vec::new();

        // 5. Pre-collect ALL pin positions (with net_id) for conflict detection
        let mut all_pin_positions: Vec<(f64, f64)> = Vec::new();
        // (x, y, net_id) — used to check L-wire corner conflicts
        let mut all_pins_with_net: Vec<(f64, f64, u32)> = Vec::new();
        for (net_id, pins) in &pins_by_net {
            for pin in pins {
                all_pin_positions.push((pin.x, pin.y));
                all_pins_with_net.push((pin.x, pin.y, *net_id));
            }
        }
        // Add NC pin positions as forbidden (net_id=0 won't match any real net)
        for (x, y, _rot, _nid) in &no_connect_positions {
            all_pins_with_net.push((*x, *y, 0));
        }

        // 5b. Collect component body bounding boxes for wire collision detection
        let component_bodies = Self::collect_component_bodies(schematic);

        // 6. Phase 1: Dispatch per-net based on render hint, collecting wire segments
        let mut wire_segments: Vec<(f64, f64, f64, f64, u32)> = Vec::new();

        let mut sorted_net_ids: Vec<u32> = pins_by_net.keys().copied().collect();
        sorted_net_ids.sort();

        for net_id in &sorted_net_ids {
            let pins = pins_by_net
                .get(net_id)
                .expect("net_id from keys() must exist in map");
            let render = net_renders.get(net_id).copied().unwrap_or_default();
            let net_name = net_names.get(net_id).copied().unwrap_or("?");

            match render {
                RenderHint::Wire => self.render_net_wires(
                    output,
                    *net_id,
                    net_name,
                    pins,
                    &mut all_label_positions,
                    &mut wire_segments,
                    &all_pins_with_net,
                    &component_bodies,
                ),
                RenderHint::Label => self.render_net_labels(
                    output,
                    *net_id,
                    net_name,
                    pins,
                    &mut all_label_positions,
                    &mut wire_segments,
                    &all_pins_with_net,
                    &component_bodies,
                ),
                RenderHint::Power => self.render_net_power(
                    output,
                    *net_id,
                    net_name,
                    pins,
                    &mut all_label_positions,
                    schematic,
                ),
                RenderHint::Global => {
                    self.render_net_global(output, net_name, pins, &mut all_label_positions)
                }
            }
        }

        // 6. Phase 2: Generate junctions at wire-pin intersections
        self.generate_auto_junctions(output, &wire_segments, &all_pin_positions);

        // 7. Generate no_connect symbols
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
                        let (rx, ry) = Self::rotate_point(lx, ly, crot);
                        let wx = cx + rx;
                        let wy = cy - ry;
                        self.write_line(output, "(no_connect");
                        self.indent_level += 1;
                        self.write_line(
                            output,
                            &format!(
                                "(at {} {})",
                                Self::format_number(wx),
                                Self::format_number(wy)
                            ),
                        );
                        if self.config.include_uuids {
                            self.write_line(output, &format!("(uuid \"{}\")", Self::new_uuid()));
                        }
                        self.indent_level -= 1;
                        self.write_line(output, ")");
                    }
                }
            }
        }

        // 8. Generate PWR_FLAG symbols if configured.
        // For hierarchical designs (sheets present), PWR_FLAG is problematic:
        // placing one per sub-sheet on the same global net causes pin_to_pin
        // conflicts. Only generate PWR_FLAG for flat (non-hierarchical) schematics.
        let is_hierarchical = !schematic.sheets.is_empty();
        if self.config.insert_power_flags && !is_hierarchical {
            self.generate_power_flags(output, schematic, &all_label_positions);
        }
    }

    // ============== Render: Label (hybrid: wire + local label) ==============

    #[allow(clippy::too_many_arguments)]
    fn render_net_labels<'a>(
        &mut self,
        output: &mut String,
        net_id: u32,
        net_name: &'a str,
        pins: &[PinInfo],
        all_label_positions: &mut Vec<(f64, f64, f64, &'a str)>,
        wire_segments: &mut Vec<(f64, f64, f64, f64, u32)>,
        all_pins_with_net: &[(f64, f64, u32)],
        component_bodies: &[BodyRect],
    ) {
        if pins.is_empty() {
            return;
        }

        // For 2+ pins, try wire routing first
        if pins.len() >= 2 {
            let mut sorted_indices: Vec<usize> = (0..pins.len()).collect();
            sorted_indices.sort_by(|&a, &b| {
                pins[a]
                    .y
                    .partial_cmp(&pins[b].y)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });

            let mut visited = vec![false; pins.len()];
            visited[sorted_indices[0]] = true;
            let mut current_idx = sorted_indices[0];

            for _ in 1..sorted_indices.len() {
                let current = &pins[current_idx];
                let mut candidates: Vec<(usize, f64)> = sorted_indices
                    .iter()
                    .filter(|&&j| !visited[j])
                    .map(|&j| {
                        (
                            j,
                            manhattan_distance((current.x, current.y), (pins[j].x, pins[j].y)),
                        )
                    })
                    .filter(|&(_, d)| d <= MAX_WIRE_MANHATTAN)
                    .collect();
                candidates
                    .sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

                let mut connected = false;
                for (cand_idx, _) in &candidates {
                    let p1 = (current.x, current.y);
                    let p2 = (pins[*cand_idx].x, pins[*cand_idx].y);
                    if self.emit_l_wire(
                        output,
                        p1,
                        p2,
                        net_id,
                        wire_segments,
                        all_pins_with_net,
                        component_bodies,
                    ) {
                        visited[*cand_idx] = true;
                        current_idx = *cand_idx;
                        connected = true;
                        break;
                    }
                }
                if !connected {
                    break;
                }
            }

            // Anchor label at first visited pin
            let anchor = &pins[sorted_indices[0]];
            all_label_positions.push((anchor.x, anchor.y, anchor.rotation, net_name));
            self.emit_local_label(output, net_name, anchor);

            // Unvisited pins get local labels
            for (i, pin) in pins.iter().enumerate() {
                if !visited[i] {
                    all_label_positions.push((pin.x, pin.y, pin.rotation, net_name));
                    self.emit_local_label(output, net_name, pin);
                }
            }
        } else {
            // Single pin: just a local label
            for pin in pins {
                all_label_positions.push((pin.x, pin.y, pin.rotation, net_name));
                self.emit_local_label(output, net_name, pin);
            }
        }
    }

    // ============== Render: Global (global_label) ==============

    fn render_net_global<'a>(
        &mut self,
        output: &mut String,
        net_name: &'a str,
        pins: &[PinInfo],
        all_label_positions: &mut Vec<(f64, f64, f64, &'a str)>,
    ) {
        for pin in pins {
            // Skip if another net already has a label at this position (avoid shorting nets)
            let occupied = all_label_positions.iter().any(|&(x, y, _, other_net)| {
                (x - pin.x).abs() < 0.01 && (y - pin.y).abs() < 0.01 && other_net != net_name
            });
            if occupied {
                continue;
            }
            all_label_positions.push((pin.x, pin.y, pin.rotation, net_name));
            self.emit_global_label(output, net_name, pin);
        }
    }

    // ============== Render: Wire (L-shaped routing) ==============

    #[allow(clippy::too_many_arguments)]
    fn render_net_wires<'a>(
        &mut self,
        output: &mut String,
        net_id: u32,
        net_name: &'a str,
        pins: &[PinInfo],
        all_label_positions: &mut Vec<(f64, f64, f64, &'a str)>,
        wire_segments: &mut Vec<(f64, f64, f64, f64, u32)>,
        all_pins_with_net: &[(f64, f64, u32)],
        component_bodies: &[BodyRect],
    ) {
        if pins.is_empty() {
            return;
        }

        // Single pin: local label fallback
        if pins.len() == 1 {
            let pin = &pins[0];
            all_label_positions.push((pin.x, pin.y, pin.rotation, net_name));
            self.emit_local_label(output, net_name, pin);
            return;
        }

        // Sort pins by Y position (topmost first for consistent starting point)
        let mut sorted_indices: Vec<usize> = (0..pins.len()).collect();
        sorted_indices.sort_by(|&a, &b| {
            pins[a]
                .y
                .partial_cmp(&pins[b].y)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Nearest-neighbor chain: connect pins greedily
        let mut visited = vec![false; pins.len()];
        visited[sorted_indices[0]] = true;
        let mut current_idx = sorted_indices[0];

        for _ in 1..sorted_indices.len() {
            let current = &pins[current_idx];

            // Find nearest unvisited pin, trying candidates in order of distance
            let mut candidates: Vec<(usize, f64)> = sorted_indices
                .iter()
                .filter(|&&j| !visited[j])
                .map(|&j| {
                    (
                        j,
                        manhattan_distance((current.x, current.y), (pins[j].x, pins[j].y)),
                    )
                })
                .filter(|&(_, d)| d <= MAX_WIRE_MANHATTAN)
                .collect();
            candidates.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

            let mut connected = false;
            for (cand_idx, _) in &candidates {
                let p1 = (current.x, current.y);
                let p2 = (pins[*cand_idx].x, pins[*cand_idx].y);
                if self.emit_l_wire(
                    output,
                    p1,
                    p2,
                    net_id,
                    wire_segments,
                    all_pins_with_net,
                    component_bodies,
                ) {
                    visited[*cand_idx] = true;
                    current_idx = *cand_idx;
                    connected = true;
                    break;
                }
            }
            if !connected {
                break; // No reachable unvisited pin → remaining get labels
            }
        }

        // Always place local labels at every pin position for wire nets.
        // This ensures electrical connectivity via labels even when KiCad
        // doesn't recognize physical wire-to-pin connections (e.g. off-grid).
        for pin in pins {
            all_label_positions.push((pin.x, pin.y, pin.rotation, net_name));
            self.emit_local_label(output, net_name, pin);
        }
    }

    /// Emit a local label at a pin position (fallback for unreachable pins)
    fn emit_local_label(&mut self, output: &mut String, net_name: &str, pin: &PinInfo) {
        let mut effects = TextEffects::default();
        effects.justify.vertical = VerticalAlign::Center;
        match pin.rotation as i32 % 360 {
            0 => effects.justify.horizontal = HorizontalAlign::Left,
            180 => effects.justify.horizontal = HorizontalAlign::Right,
            90 | 270 => effects.justify.horizontal = HorizontalAlign::Left,
            _ => {}
        }
        self.write_line(output, "(label");
        self.indent_level += 1;
        self.write_line(output, &format!("\"{}\"", Self::escape_string(net_name)));
        self.write_line(output, &Self::format_at(pin.x, pin.y, pin.rotation));
        self.generate_effects(output, &effects);
        if self.config.include_uuids {
            self.write_line(output, &format!("(uuid \"{}\")", Self::new_uuid()));
        }
        self.indent_level -= 1;
        self.write_line(output, ")");
    }

    /// Generate an L-shaped wire from p1 to p2, avoiding pin conflicts
    /// and wire-wire overlaps. Falls back to Z-shape (3-segment) routing
    /// when both L-shape options conflict.
    /// Returns true if wire was emitted, false if all options conflict.
    #[allow(clippy::too_many_arguments)] // wire/symbol emitters carry full context per call
    fn emit_l_wire(
        &mut self,
        output: &mut String,
        p1: (f64, f64),
        p2: (f64, f64),
        net_id: u32,
        wire_segments: &mut Vec<(f64, f64, f64, f64, u32)>,
        all_pins_with_net: &[(f64, f64, u32)],
        component_bodies: &[BodyRect],
    ) -> bool {
        let (x1, y1) = p1;
        let (x2, y2) = p2;

        if (y1 - y2).abs() < COORD_TOL {
            // Same row: horizontal wire
            if Self::segment_conflicts((x1, y1), (x2, y1), net_id, all_pins_with_net)
                || Self::wire_segment_overlaps((x1, y1), (x2, y1), net_id, wire_segments)
                || Self::segment_crosses_bodies((x1, y1), (x2, y1), component_bodies)
            {
                return false;
            }
            self.emit_wire_segment(output, x1, y1, x2, y1, net_id, wire_segments);
            true
        } else if (x1 - x2).abs() < COORD_TOL {
            // Same column: vertical wire
            if Self::segment_conflicts((x1, y1), (x1, y2), net_id, all_pins_with_net)
                || Self::wire_segment_overlaps((x1, y1), (x1, y2), net_id, wire_segments)
                || Self::segment_crosses_bodies((x1, y1), (x1, y2), component_bodies)
            {
                return false;
            }
            self.emit_wire_segment(output, x1, y1, x1, y2, net_id, wire_segments);
            true
        } else {
            // L-shape: try both orthogonal directions
            let corner_a = (x2, y1);
            let conflict_a = Self::corner_conflicts(corner_a, net_id, all_pins_with_net)
                || Self::segment_conflicts((x1, y1), corner_a, net_id, all_pins_with_net)
                || Self::segment_conflicts(corner_a, (x2, y2), net_id, all_pins_with_net)
                || Self::wire_segment_overlaps((x1, y1), corner_a, net_id, wire_segments)
                || Self::wire_segment_overlaps(corner_a, (x2, y2), net_id, wire_segments)
                || Self::segment_crosses_bodies((x1, y1), corner_a, component_bodies)
                || Self::segment_crosses_bodies(corner_a, (x2, y2), component_bodies);

            let corner_b = (x1, y2);
            let conflict_b = Self::corner_conflicts(corner_b, net_id, all_pins_with_net)
                || Self::segment_conflicts((x1, y1), corner_b, net_id, all_pins_with_net)
                || Self::segment_conflicts(corner_b, (x2, y2), net_id, all_pins_with_net)
                || Self::wire_segment_overlaps((x1, y1), corner_b, net_id, wire_segments)
                || Self::wire_segment_overlaps(corner_b, (x2, y2), net_id, wire_segments)
                || Self::segment_crosses_bodies((x1, y1), corner_b, component_bodies)
                || Self::segment_crosses_bodies(corner_b, (x2, y2), component_bodies);

            if !conflict_a {
                self.emit_wire_segment(
                    output,
                    x1,
                    y1,
                    corner_a.0,
                    corner_a.1,
                    net_id,
                    wire_segments,
                );
                self.emit_wire_segment(
                    output,
                    corner_a.0,
                    corner_a.1,
                    x2,
                    y2,
                    net_id,
                    wire_segments,
                );
                return true;
            } else if !conflict_b {
                self.emit_wire_segment(
                    output,
                    x1,
                    y1,
                    corner_b.0,
                    corner_b.1,
                    net_id,
                    wire_segments,
                );
                self.emit_wire_segment(
                    output,
                    corner_b.0,
                    corner_b.1,
                    x2,
                    y2,
                    net_id,
                    wire_segments,
                );
                return true;
            }

            // Both L-shapes conflict — try Z-shape (3-segment) routing
            if self.emit_z_wire(
                output,
                p1,
                p2,
                net_id,
                wire_segments,
                all_pins_with_net,
                component_bodies,
            ) {
                return true;
            }
            false
        }
    }

    /// Collect component body bounding boxes from schematic components.
    /// Estimates body size based on lib_id (Device:R/C/L are small, ICs are larger).
    fn collect_component_bodies(schematic: &crate::ir::Schematic) -> Vec<BodyRect> {
        let mut bodies = Vec::new();
        for comp in &schematic.components {
            let lib_id = comp.lib_id.to_lowercase();
            let (hw, hh) = if lib_id.starts_with("device:r")
                || lib_id.starts_with("device:c")
                || lib_id.starts_with("device:l")
            {
                // Small passives: ~2.54 x 1.27 mm body
                (1.27, 0.635)
            } else if lib_id.starts_with("device:d") || lib_id.starts_with("device:led") {
                (1.27, 0.635)
            } else if lib_id.contains("connector") || lib_id.contains("conn") {
                // Connectors vary but typical 2-4mm wide
                (2.54, 2.54)
            } else {
                // ICs and other: default ~5x5mm
                (2.54, 2.54)
            };
            // Body rect centered on component position with clearance
            let clearance = 1.0;
            bodies.push(BodyRect {
                x1: comp.position.0 - hw - clearance,
                y1: comp.position.1 - hh - clearance,
                x2: comp.position.0 + hw + clearance,
                y2: comp.position.1 + hh + clearance,
            });
        }
        bodies
    }

    /// Check if a Manhattan (axis-aligned) wire segment crosses through any component body.
    /// Excludes segments that are very short (pin stubs) from false positives.
    fn segment_crosses_bodies(start: (f64, f64), end: (f64, f64), bodies: &[BodyRect]) -> bool {
        let (sx, sy) = start;
        let (ex, ey) = end;
        let seg_len = (sx - ex).abs() + (sy - ey).abs();
        if seg_len < 2.0 {
            // Very short segments (pin stubs) don't need body collision check
            return false;
        }

        for body in bodies {
            if Self::manhattan_segment_intersects_rect(sx, sy, ex, ey, body) {
                return true;
            }
        }
        false
    }

    /// Test if an axis-aligned segment (horizontal or vertical) intersects a rectangle.
    /// Uses proper line-rect intersection for Manhattan geometry.
    fn manhattan_segment_intersects_rect(
        sx: f64,
        sy: f64,
        ex: f64,
        ey: f64,
        rect: &BodyRect,
    ) -> bool {
        if (sy - ey).abs() < COORD_TOL {
            // Horizontal segment
            let min_x = sx.min(ex);
            let max_x = sx.max(ex);
            // Check Y overlap with rect
            if sy >= rect.y1 && sy <= rect.y2 {
                // Check X overlap
                if min_x < rect.x2 && max_x > rect.x1 {
                    return true;
                }
            }
        } else if (sx - ex).abs() < COORD_TOL {
            // Vertical segment
            let min_y = sy.min(ey);
            let max_y = sy.max(ey);
            // Check X overlap with rect
            if sx >= rect.x1 && sx <= rect.x2 {
                // Check Y overlap
                if min_y < rect.y2 && max_y > rect.y1 {
                    return true;
                }
            }
        }
        false
    }

    /// Check if a wire segment passes through a pin of a different net.
    /// A pin at the endpoints is OK (it's the source/destination), but a pin
    /// as a midpoint would create an unintended connection.
    fn segment_conflicts(
        seg_start: (f64, f64),
        seg_end: (f64, f64),
        net_id: u32,
        all_pins: &[(f64, f64, u32)],
    ) -> bool {
        let (sx, sy) = seg_start;
        let (ex, ey) = seg_end;
        for &(px, py, pnid) in all_pins {
            if pnid == net_id {
                continue;
            }
            // Horizontal segment: check if pin lies on it as a midpoint
            if (sy - ey).abs() < COORD_TOL && (py - sy).abs() < COORD_TOL {
                let min_x = sx.min(ex);
                let max_x = sx.max(ex);
                if px > min_x + COORD_TOL && px < max_x - COORD_TOL {
                    return true;
                }
            }
            // Vertical segment: check if pin lies on it as a midpoint
            if (sx - ex).abs() < COORD_TOL && (px - sx).abs() < COORD_TOL {
                let min_y = sy.min(ey);
                let max_y = sy.max(ey);
                if py > min_y + COORD_TOL && py < max_y - COORD_TOL {
                    return true;
                }
            }
        }
        false
    }

    /// Check if a corner point coincides with a pin of a different net.
    fn corner_conflicts(corner: (f64, f64), net_id: u32, all_pins: &[(f64, f64, u32)]) -> bool {
        for &(px, py, pnid) in all_pins {
            if pnid != net_id
                && (px - corner.0).abs() < COORD_TOL
                && (py - corner.1).abs() < COORD_TOL
            {
                return true;
            }
        }
        false
    }

    /// Check if a new wire segment would overlap with any existing wire from a different net.
    fn wire_segment_overlaps(
        start: (f64, f64),
        end: (f64, f64),
        net_id: u32,
        wire_segments: &[(f64, f64, f64, f64, u32)],
    ) -> bool {
        let (sx, sy) = start;
        let (ex, ey) = end;

        for &(wx1, wy1, wx2, wy2, wnet) in wire_segments {
            if wnet == net_id {
                continue;
            }

            // Horizontal overlap: both segments horizontal on same Y
            if (sy - ey).abs() < COORD_TOL
                && (wy1 - wy2).abs() < COORD_TOL
                && (sy - wy1).abs() < COORD_TOL
            {
                let min_x = sx.min(ex);
                let max_x = sx.max(ex);
                let wmin_x = wx1.min(wx2);
                let wmax_x = wx1.max(wx2);
                if min_x < wmax_x - COORD_TOL && max_x > wmin_x + COORD_TOL {
                    return true;
                }
            }

            // Vertical overlap: both segments vertical on same X
            if (sx - ex).abs() < COORD_TOL
                && (wx1 - wx2).abs() < COORD_TOL
                && (sx - wx1).abs() < COORD_TOL
            {
                let min_y = sy.min(ey);
                let max_y = sy.max(ey);
                let wmin_y = wy1.min(wy2);
                let wmax_y = wy1.max(wy2);
                if min_y < wmax_y - COORD_TOL && max_y > wmin_y + COORD_TOL {
                    return true;
                }
            }
        }
        false
    }

    /// Try Z-shape (3-segment) routing from p1 to p2.
    /// H-V-H: horizontal → vertical → horizontal (varying x_mid)
    /// V-H-V: vertical → horizontal → vertical (varying y_mid)
    #[allow(clippy::too_many_arguments)] // wire/symbol emitters carry full context per call
    fn emit_z_wire(
        &mut self,
        output: &mut String,
        p1: (f64, f64),
        p2: (f64, f64),
        net_id: u32,
        wire_segments: &mut Vec<(f64, f64, f64, f64, u32)>,
        all_pins_with_net: &[(f64, f64, u32)],
        component_bodies: &[BodyRect],
    ) -> bool {
        let (x1, y1) = p1;
        let (x2, y2) = p2;
        let grid = 2.54;

        // H-V-H candidates: vary x_mid
        let x_dir = if x1 < x2 { 1.0 } else { -1.0 };
        let x_mids = [
            (x1 + x2) / 2.0,
            x1 + x_dir * grid,
            x2 - x_dir * grid,
            x1 + x_dir * 2.0 * grid,
            x2 - x_dir * 2.0 * grid,
        ];

        for x_mid in &x_mids {
            let seg1 = ((x1, y1), (*x_mid, y1));
            let seg2 = ((*x_mid, y1), (*x_mid, y2));
            let seg3 = ((*x_mid, y2), (x2, y2));

            if Self::check_z_conflicts(
                seg1,
                seg2,
                seg3,
                net_id,
                all_pins_with_net,
                wire_segments,
                component_bodies,
            ) {
                continue;
            }

            self.emit_wire_segment(
                output,
                seg1.0 .0,
                seg1.0 .1,
                seg1.1 .0,
                seg1.1 .1,
                net_id,
                wire_segments,
            );
            self.emit_wire_segment(
                output,
                seg2.0 .0,
                seg2.0 .1,
                seg2.1 .0,
                seg2.1 .1,
                net_id,
                wire_segments,
            );
            self.emit_wire_segment(
                output,
                seg3.0 .0,
                seg3.0 .1,
                seg3.1 .0,
                seg3.1 .1,
                net_id,
                wire_segments,
            );
            return true;
        }

        // V-H-V candidates: vary y_mid
        let y_dir = if y1 < y2 { 1.0 } else { -1.0 };
        let y_mids = [
            (y1 + y2) / 2.0,
            y1 + y_dir * grid,
            y2 - y_dir * grid,
            y1 + y_dir * 2.0 * grid,
            y2 - y_dir * 2.0 * grid,
        ];

        for y_mid in &y_mids {
            let seg1 = ((x1, y1), (x1, *y_mid));
            let seg2 = ((x1, *y_mid), (x2, *y_mid));
            let seg3 = ((x2, *y_mid), (x2, y2));

            if Self::check_z_conflicts(
                seg1,
                seg2,
                seg3,
                net_id,
                all_pins_with_net,
                wire_segments,
                component_bodies,
            ) {
                continue;
            }

            self.emit_wire_segment(
                output,
                seg1.0 .0,
                seg1.0 .1,
                seg1.1 .0,
                seg1.1 .1,
                net_id,
                wire_segments,
            );
            self.emit_wire_segment(
                output,
                seg2.0 .0,
                seg2.0 .1,
                seg2.1 .0,
                seg2.1 .1,
                net_id,
                wire_segments,
            );
            self.emit_wire_segment(
                output,
                seg3.0 .0,
                seg3.0 .1,
                seg3.1 .0,
                seg3.1 .1,
                net_id,
                wire_segments,
            );
            return true;
        }

        false
    }

    /// Check all three segments of a Z-shape for pin conflicts and wire overlaps.
    fn check_z_conflicts(
        seg1: ((f64, f64), (f64, f64)),
        seg2: ((f64, f64), (f64, f64)),
        seg3: ((f64, f64), (f64, f64)),
        net_id: u32,
        all_pins: &[(f64, f64, u32)],
        wire_segments: &[(f64, f64, f64, f64, u32)],
        component_bodies: &[BodyRect],
    ) -> bool {
        for (seg_start, seg_end) in &[seg1, seg2, seg3] {
            if Self::segment_conflicts(*seg_start, *seg_end, net_id, all_pins)
                || Self::corner_conflicts(*seg_start, net_id, all_pins)
                || Self::corner_conflicts(*seg_end, net_id, all_pins)
                || Self::wire_segment_overlaps(*seg_start, *seg_end, net_id, wire_segments)
                || Self::segment_crosses_bodies(*seg_start, *seg_end, component_bodies)
            {
                return true;
            }
        }
        false
    }

    /// Emit a single wire segment and record it for junction detection
    #[allow(clippy::too_many_arguments)] // wire/symbol emitters carry full context per call
    fn emit_wire_segment(
        &mut self,
        output: &mut String,
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        net_id: u32,
        wire_segments: &mut Vec<(f64, f64, f64, f64, u32)>,
    ) {
        wire_segments.push((x1, y1, x2, y2, net_id));
        let default_stroke = Stroke::default();
        self.write_line(output, "(wire");
        self.indent_level += 1;
        self.write_line(
            output,
            &format!(
                "(pts {} {})",
                Self::format_xy(x1, y1),
                Self::format_xy(x2, y2)
            ),
        );
        self.generate_stroke(output, &default_stroke);
        self.indent_level -= 1;
        self.write_line(output, ")");
    }

    /// Generate junction dots at positions where wires pass through pin locations.
    fn generate_auto_junctions(
        &mut self,
        output: &mut String,
        wire_segments: &[(f64, f64, f64, f64, u32)],
        pin_positions: &[(f64, f64)],
    ) {
        if wire_segments.is_empty() {
            return;
        }

        let mut junction_set: Vec<(f64, f64)> = Vec::new();

        for &(px, py) in pin_positions {
            let mut need_junction = false;

            for &(x1, y1, x2, y2, _net_id) in wire_segments {
                // Check if pin lies on a horizontal wire segment (midpoint)
                if (y1 - y2).abs() < COORD_TOL && (py - y1).abs() < COORD_TOL {
                    let min_x = x1.min(x2);
                    let max_x = x1.max(x2);
                    if px > min_x + COORD_TOL && px < max_x - COORD_TOL {
                        need_junction = true;
                        break;
                    }
                }
                // Check if pin lies on a vertical wire segment (midpoint)
                if (x1 - x2).abs() < COORD_TOL && (px - x1).abs() < COORD_TOL {
                    let min_y = y1.min(y2);
                    let max_y = y1.max(y2);
                    if py > min_y + COORD_TOL && py < max_y - COORD_TOL {
                        need_junction = true;
                        break;
                    }
                }
            }

            if need_junction
                && !junction_set
                    .iter()
                    .any(|&(jx, jy)| (jx - px).abs() < COORD_TOL && (jy - py).abs() < COORD_TOL)
            {
                junction_set.push((px, py));
            }
        }

        for (x, y) in junction_set {
            self.write_line(output, "(junction");
            self.indent_level += 1;
            self.write_line(
                output,
                &format!("(at {} {})", Self::format_number(x), Self::format_number(y)),
            );
            if self.config.include_uuids {
                self.write_line(output, &format!("(uuid \"{}\")", Self::new_uuid()));
            }
            self.indent_level -= 1;
            self.write_line(output, ")");
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
                // No matching power symbol — use global_labels
                self.render_net_global(output, net_name, pins, all_label_positions);
                return;
            }
        };

        let has_power_out = Self::net_has_power_out_pin(schematic, net_id);
        // Always use global_labels for power nets to avoid pin_to_pin conflicts
        // across hierarchical sheets. Power symbols create power_out pins that
        // conflict with IC VOUT pins on the same global net in other sheets.
        // The IC power_out pins provide the ERC driving source.
        // `|| true` is intentional (see comment above); the power-symbol path
        // below is retained as reference and never taken.
        #[allow(clippy::overly_complex_bool_expr)]
        if has_power_out || true {
            self.render_net_global(output, net_name, pins, all_label_positions);
            return;
        }

        // Normal power net: emit power symbol + global_labels for all pins
        let is_v9_plus = matches!(self.effective_version, KicadVersion::V9 | KicadVersion::V10);
        let first_pin = &pins[0];

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
        self.write_line(
            output,
            &format!(
            "(property \"Reference\" \"#PWR??\" {} (effects (font (size 1.27 1.27)) (hide yes)))",
            Self::format_at(first_pin.x, first_pin.y, 0.0)
        ),
        );
        self.write_line(
            output,
            &format!(
                "(property \"Value\" \"{}\" {} (effects (font (size 1.27 1.27))))",
                Self::escape_string(net_name),
                Self::format_at(first_pin.x, first_pin.y, 0.0)
            ),
        );
        if self.config.include_uuids {
            self.write_line(
                output,
                &format!("(pin \"1\" (uuid \"{}\"))", Self::new_uuid()),
            );
        }
        self.indent_level -= 1;
        self.write_line(output, ")");

        // All pins get global_labels (wire routing skipped for power nets to avoid power_out-to-power_out conflicts)
        for pin in pins {
            all_label_positions.push((pin.x, pin.y, pin.rotation, net_name));
            self.emit_global_label(output, net_name, pin);
        }
    }

    fn emit_global_label(&mut self, output: &mut String, net_name: &str, pin: &PinInfo) {
        let mut effects = TextEffects::default();
        effects.justify.vertical = VerticalAlign::Center;
        // KiCad convention (verified from reference .kicad_sch files):
        //   rotation 0°   → justify left   (flag RIGHT, text anchored left)
        //   rotation 180° → justify right   (flag LEFT, text anchored right)
        //   rotation 90°  → justify left    (flag UP, text anchored left)
        //   rotation 270° → justify right   (flag DOWN, text anchored right)
        match pin.rotation as i32 % 360 {
            0 => effects.justify.horizontal = HorizontalAlign::Left,
            180 => effects.justify.horizontal = HorizontalAlign::Right,
            90 => effects.justify.horizontal = HorizontalAlign::Left,
            270 => effects.justify.horizontal = HorizontalAlign::Right,
            _ => {}
        }
        self.write_line(output, "(global_label");
        self.indent_level += 1;
        self.write_line(output, &format!("\"{}\"", Self::escape_string(net_name)));
        self.write_line(output, &Self::format_at(pin.x, pin.y, pin.rotation));
        self.write_line(output, "(shape passive)");
        self.generate_effects(output, &effects);
        if self.config.include_uuids {
            self.write_line(output, &format!("(uuid \"{}\")", Self::new_uuid()));
        }
        self.indent_level -= 1;
        self.write_line(output, ")");
    }

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

    pub(super) fn resolve_power_symbol(net_name: &str) -> Option<String> {
        let upper = net_name.to_uppercase();
        if upper.starts_with('+') || upper.starts_with('-') {
            return Some(format!("power:{}", net_name));
        }
        if upper == "GND" || upper == "VCC" || upper == "GNDA" || upper == "GNDD" {
            return Some(format!("power:{}", net_name));
        }
        None
    }

    // ============== Power Flags (existing) ==============

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

        let net_names: HashMap<u32, &str> = schematic
            .nets
            .iter()
            .map(|n| (n.id, n.name.as_str()))
            .collect();

        let mut power_flag_nets: Vec<&str> = Vec::new();

        // Standard: nets with power_in pins but no power_out driver.
        // Skip nets that also have output/bidirectional pins — those are signal nets,
        // and inserting PWR_FLAG there causes pin_to_pin (output vs power_out) errors.
        for (net_id, types) in &net_pin_types {
            let has_power_in = types.contains(&"power_in");
            let has_power_out = types.contains(&"power_out");
            let has_signal_output = types
                .iter()
                .any(|t| matches!(*t, "output" | "bidirectional"));
            if has_power_in && !has_power_out && !has_signal_output {
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

        // Fallback: use position of first power_in pin for nets without labels
        let mut net_pin_positions: HashMap<u32, (f64, f64)> = HashMap::new();
        for comp in &schematic.components {
            let comp_lib_id = Self::normalize_lib_id(&comp.lib_id);
            let pin_type_map = match lib_pin_types.get(&comp_lib_id) {
                Some(m) => m,
                None => continue,
            };
            for pin in &comp.pins {
                if let Some(net_id) = pin.net_id {
                    let ptype = pin_type_map.get(pin.number.as_str()).unwrap_or(&"passive");
                    if *ptype == "power_in" && !net_pin_positions.contains_key(&net_id) {
                        // Use component position as approximate pin position
                        let (cx, cy, _cr) = comp.position;
                        net_pin_positions.insert(net_id, (cx, cy));
                    }
                }
            }
        }
        for (net_id, pos) in &net_pin_positions {
            if let Some(name) = net_names.get(net_id) {
                if !net_positions.contains_key(name) {
                    net_positions.insert(name, *pos);
                }
            }
        }

        let mut flag_index: u32 = 1;
        for net_name in &power_flag_nets {
            if let Some(&(x, y)) = net_positions.get(net_name) {
                let ref_name = format!("#FLG{:02}", flag_index);
                flag_index += 1;
                // Offset PWR_FLAG so it doesn't overlap with global_label at the same position
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
                self.write_line(
                    output,
                    &format!(
                    "(property \"Value\" \"{}\" {} (effects (font (size 1.27 1.27)) (hide yes)))",
                    Self::escape_string(net_name),
                    Self::format_at(x, y, 0.0)
                ),
                );
                if self.config.include_uuids {
                    self.write_line(
                        output,
                        &format!("(pin \"1\" (uuid \"{}\"))", Self::new_uuid()),
                    );
                }
                self.indent_level -= 1;
                self.write_line(output, ")");
            }
        }
    }
}
