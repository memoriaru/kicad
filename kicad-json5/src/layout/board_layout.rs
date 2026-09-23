//! Board-level auto-layout engine for schematics.
//!
//! Takes a Schematic IR without positions and generates coordinate assignments
//! following power-flow direction (left→right) with functional section grouping.

use std::collections::{HashMap, HashSet};

use crate::ir::Schematic;
use crate::topology::{classify_component, classify_net, ComponentKind, NetKind};

// ── Constants ──────────────────────────────────────────────────────

const GRID: f64 = 2.54;
const SECTION_GAP: f64 = 19.05;
const COMPONENT_GAP: f64 = 19.05;
const ROW_GAP: f64 = 25.4;
const START_X: f64 = 25.4;
const START_Y: f64 = 25.4;
const A4_W: f64 = 250.0; // usable width inside A4 border

// ── Data structures ────────────────────────────────────────────────

/// A functional section of the board (e.g., input protection, buck converter)
#[derive(Debug, Clone)]
pub struct Section {
    pub anchor_index: usize,
    pub anchor_kind: AnchorKind,
    pub member_indices: Vec<usize>,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorKind {
    Ic,
    Connector,
    Protection,
    Transistor,
}

/// A dashed divider line between functional sections
#[derive(Debug, Clone)]
pub struct DividerLine {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
}

/// A functional label for a section, placed below the section block
#[derive(Debug, Clone)]
pub struct SectionLabel {
    pub text: String,
    pub x: f64,
    pub y: f64,
}

/// Board layout result
#[derive(Debug, Clone)]
pub struct BoardLayout {
    pub sections: Vec<Section>,
    pub positions: HashMap<usize, (f64, f64, f64)>,
    pub dividers: Vec<DividerLine>,
    pub section_labels: Vec<SectionLabel>,
}

// ── Main entry point ───────────────────────────────────────────────

/// Auto-layout a schematic: assign (x, y, rotation) to all components.
pub fn auto_layout(schematic: &Schematic) -> BoardLayout {
    let n = schematic.components.len();
    if n == 0 {
        return BoardLayout {
            sections: Vec::new(),
            positions: HashMap::new(),
            dividers: Vec::new(),
            section_labels: Vec::new(),
        };
    }

    // Step 1: Classify all components
    let comp_kinds: Vec<ComponentKind> = schematic
        .components
        .iter()
        .map(|c| classify_component(&c.lib_id))
        .collect();

    // Step 2: Build net lookup tables
    let net_id_to_name: HashMap<u32, String> = schematic
        .nets
        .iter()
        .map(|n| (n.id, n.name.clone()))
        .collect();
    let net_name_to_id: HashMap<String, u32> = schematic
        .nets
        .iter()
        .map(|n| (n.name.clone(), n.id))
        .collect();
    let net_kinds: HashMap<u32, NetKind> = schematic
        .nets
        .iter()
        .map(|n| (n.id, classify_net(&n.name)))
        .collect();

    // Step 3: Build component → connected net IDs mapping
    let comp_nets: Vec<HashSet<u32>> = schematic
        .components
        .iter()
        .map(|comp| comp.pins.iter().filter_map(|p| p.net_id).collect())
        .collect();

    // Step 4: Identify anchors (ICs, connectors, protection devices)
    let anchors = identify_anchors(schematic, &comp_kinds);

    // Step 5: Power flow analysis → order power nets
    let power_order = analyze_power_flow(schematic, &net_kinds);

    // Step 6: Sort anchors by power flow rank
    let sorted_anchors =
        sort_anchors_by_power(&anchors, &comp_nets, &power_order, &net_kinds, schematic);

    // Step 7: Group peripherals around anchors
    let mut sections = group_into_sections(&sorted_anchors, &comp_nets, &comp_kinds, n);

    // Step 8: Place components relative to (0,0) per section, compute bounding boxes
    let mut positions = HashMap::new();
    for section in &mut sections {
        place_section_components(
            section,
            schematic,
            &comp_kinds,
            &comp_nets,
            &net_id_to_name,
            &net_name_to_id,
            &net_kinds,
            &mut positions,
        );
        // Compute bounding box from placed positions
        compute_section_bbox(section, &positions);
    }

    // Step 9: Arrange sections by bounding boxes (auto-wrap within A4)
    let dividers = arrange_sections_by_bboxes(&mut sections);

    // Step 10: Offset all positions to absolute section coordinates
    for section in &sections {
        let (sx, sy) = (section.x, section.y);
        for &mi in &section.member_indices {
            if let Some(pos) = positions.get_mut(&mi) {
                pos.0 += sx;
                pos.1 += sy;
            }
        }
        if let Some(pos) = positions.get_mut(&section.anchor_index) {
            pos.0 += sx;
            pos.1 += sy;
        }
    }

    // Step 10b: Ensure no component's left-side labels exceed the frame border.
    // Global labels extend ~15mm left from the pin; inner border at ~12mm.
    // If a component's left pin x < 28mm, shift the component right.
    const MIN_LEFT_PIN_X: f64 = 28.0;
    let pin_offset: f64 = 7.62; // body_hw + pin_length for typical components
    for pos in positions.values_mut() {
        let left_pin_x = pos.0 - pin_offset;
        if left_pin_x < MIN_LEFT_PIN_X {
            let shift = snap_to_grid(MIN_LEFT_PIN_X - left_pin_x);
            pos.0 += shift;
        }
    }

    // Step 11: Place any unassigned components
    place_remaining(&sections, &comp_kinds, &mut positions, n);

    // Step 12: Generate section functional labels
    // Place each label centered below the section's actual component extents
    let label_gap = 2.54; // 1 grid unit below bottommost extent
    let mut section_labels = Vec::new();
    for section in &sections {
        let all_indices: Vec<usize> = std::iter::once(section.anchor_index)
            .chain(section.member_indices.iter().copied())
            .collect();

        let mut min_x = f64::MAX;
        let mut max_x = f64::MIN;
        let mut bottom_extent = f64::MIN; // lowest point including body

        for &idx in &all_indices {
            if let Some(&(x, y, _)) = positions.get(&idx) {
                let comp = &schematic.components[idx];
                let half_h = estimate_body_half_height(&comp.lib_id, comp.pins.len());
                min_x = min_x.min(x);
                max_x = max_x.max(x);
                bottom_extent = bottom_extent.max(y + half_h);
            }
        }

        if min_x == f64::MAX {
            continue;
        }

        let center_x = snap_to_grid((min_x + max_x) / 2.0);
        let label_y = snap_to_grid(bottom_extent + label_gap);

        section_labels.push(SectionLabel {
            text: generate_section_label(section, schematic),
            x: center_x,
            y: label_y,
        });
    }

    BoardLayout {
        sections,
        positions,
        dividers,
        section_labels,
    }
}

/// Apply auto-layout positions directly to a schematic.
/// Returns the full layout result including dividers and section labels.
pub fn apply_to(schematic: &mut Schematic) -> BoardLayout {
    let layout = auto_layout(schematic);
    for (idx, (x, y, rot)) in &layout.positions {
        if let Some(comp) = schematic.components.get_mut(*idx) {
            comp.position = (*x, *y, *rot);
        }
    }
    layout
}

// ── Anchor identification ──────────────────────────────────────────

#[derive(Debug, Clone)]
struct Anchor {
    index: usize,
    kind: AnchorKind,
}

fn identify_anchors(schematic: &Schematic, comp_kinds: &[ComponentKind]) -> Vec<Anchor> {
    let mut anchors = Vec::new();

    for (i, comp) in schematic.components.iter().enumerate() {
        // First try the standard classifier
        let kind = match comp_kinds[i] {
            ComponentKind::Ic | ComponentKind::Power => Some(AnchorKind::Ic),
            ComponentKind::Connector => Some(AnchorKind::Connector),
            ComponentKind::Transistor => {
                if comp.pins.len() >= 3 {
                    Some(AnchorKind::Transistor)
                } else {
                    None
                }
            }
            ComponentKind::Fuse => Some(AnchorKind::Protection),
            _ => None,
        };

        // Fallback: heuristic classification for lib_ids without library prefix
        let kind = kind.or_else(|| classify_by_heuristic(&comp.lib_id, comp.pins.len()));

        let Some(kind) = kind else { continue };

        // For IC: must have ≥3 pins (exclude simple discretes)
        if kind == AnchorKind::Ic && comp.pins.len() < 3 {
            continue;
        }

        // Exclude common discrete semiconductors that look like ICs
        if kind == AnchorKind::Ic {
            let lu = comp.lib_id.to_uppercase();
            if lu.contains("AO34")
                || lu.contains("2N700")
                || lu.contains("BSS")
                || lu.contains("SI23")
            {
                continue;
            }
        }

        anchors.push(Anchor { index: i, kind });
    }

    anchors
}

/// Heuristic classification for lib_ids that don't have a standard library prefix.
fn classify_by_heuristic(lib_id: &str, pin_count: usize) -> Option<AnchorKind> {
    let lu = lib_id.to_uppercase();

    // Connector patterns: Conn_*, header_*
    if lu.starts_with("CONN") || lu.contains("HEADER") || lu.contains("PINHEADER") {
        return Some(AnchorKind::Connector);
    }

    // PPTC / Fuse
    if lu.contains("PPTC") || lu.contains("FUSE") {
        return Some(AnchorKind::Protection);
    }

    // MOSFET / Transistor (3-pin SOT packages)
    if lu.contains("AO34") || lu.contains("2N700") || lu.contains("BSS") || lu.contains("SI23") {
        return Some(AnchorKind::Transistor);
    }

    // IC: ≥5 pins and not a connector/transistor — likely an IC
    if pin_count >= 5 {
        return Some(AnchorKind::Ic);
    }

    // Known IC part number patterns (MPN-based)
    let ic_patterns = [
        "SY81", "RT9", "FP62", "CH34", "SN65", "ADS", "STM32", "ESP32", "LM35", "LM393", "AMS1117",
        "TPS", "MP15", "XL15", "74HC", "74LS", "CD40",
    ];
    for pat in &ic_patterns {
        if lu.starts_with(pat) {
            return Some(AnchorKind::Ic);
        }
    }

    None
}

// ── Power flow analysis ────────────────────────────────────────────

fn analyze_power_flow(
    schematic: &Schematic,
    net_kinds: &HashMap<u32, NetKind>,
) -> HashMap<u32, usize> {
    let power_nets: Vec<(u32, String)> = schematic
        .nets
        .iter()
        .filter_map(|n| {
            if matches!(net_kinds.get(&n.id), Some(NetKind::Power)) {
                Some((n.id, n.name.clone()))
            } else {
                None
            }
        })
        .collect();

    // Build directed edges: IC converts input_power → output_power
    // by analyzing which power nets each IC connects
    let mut edges: Vec<(u32, u32)> = Vec::new(); // (input_net, output_net)

    // Build net_id → net_name map
    let net_id_to_name: HashMap<u32, &str> = schematic
        .nets
        .iter()
        .map(|n| (n.id, n.name.as_str()))
        .collect();

    for comp in &schematic.components {
        let comp_power_nets: Vec<(u32, &str)> = comp
            .pins
            .iter()
            .filter_map(|pin| {
                let net_id = pin.net_id?;
                let net_name = net_id_to_name.get(&net_id)?;
                if matches!(net_kinds.get(&net_id), Some(NetKind::Power)) {
                    Some((net_id, *net_name))
                } else {
                    None
                }
            })
            .collect();

        if comp_power_nets.len() >= 2 {
            // Sort by name heuristically to guess direction
            let mut sorted = comp_power_nets.clone();
            sorted.sort_by_key(|a| power_net_rank_heuristic(a.1));
            // First = input (lower rank), Last = output (higher rank)
            for i in 0..sorted.len() {
                for j in (i + 1)..sorted.len() {
                    edges.push((sorted[i].0, sorted[j].0));
                }
            }
        }
    }

    // Topological sort using the edges
    let mut rank_map = topological_sort_power_nets(&power_nets, &edges);

    // If topological sort didn't cover all, fall back to heuristic
    for (net_id, name) in &power_nets {
        if !rank_map.contains_key(net_id) {
            let h_rank = power_net_rank_heuristic(name);
            rank_map.insert(*net_id, h_rank);
        }
    }

    rank_map
}

/// Heuristic rank for a power net name (lower = upstream/input, higher = downstream/output)
fn power_net_rank_heuristic(name: &str) -> usize {
    let lu = name.to_uppercase();
    // Input patterns → low rank
    if lu.contains("BAT_IN") || lu == "VIN" || lu.contains("VBUS") {
        return 0;
    }
    if lu.contains("FUSED") || lu.contains("PROT") {
        return 1;
    }
    if lu.contains("7V4") || lu.contains("BAT") {
        return 2;
    }
    // Higher voltages → mid rank
    if lu.contains("12V") {
        return 3;
    }
    if lu.contains("5V") {
        return 4;
    }
    if lu.contains("3V3") || lu.contains("3.3") {
        return 5;
    }
    if lu.contains("1V8") || lu.contains("1.8") {
        return 6;
    }
    // Default: parse voltage if possible
    if let Some(v) = crate::topology::extract_voltage(name) {
        if let Ok(volts) = v.parse::<f64>() {
            return (volts * 10.0) as usize;
        }
    }
    // Alphabetical fallback
    100
}

fn topological_sort_power_nets(
    power_nets: &[(u32, String)],
    edges: &[(u32, u32)],
) -> HashMap<u32, usize> {
    let net_ids: HashSet<u32> = power_nets.iter().map(|(id, _)| *id).collect();
    let mut in_degree: HashMap<u32, usize> = HashMap::new();
    let mut adj: HashMap<u32, Vec<u32>> = HashMap::new();

    for &id in &net_ids {
        in_degree.insert(id, 0);
        adj.insert(id, Vec::new());
    }

    for &(from, to) in edges {
        if net_ids.contains(&from) && net_ids.contains(&to) && from != to {
            adj.entry(from).or_default().push(to);
            *in_degree.entry(to).or_insert(0) += 1;
        }
    }

    // Kahn's algorithm
    let mut queue: Vec<u32> = in_degree
        .iter()
        .filter(|(_, &deg)| deg == 0)
        .map(|(&id, _)| id)
        .collect();
    queue.sort();

    let mut result = HashMap::new();
    let mut rank = 0;

    while !queue.is_empty() {
        queue.sort();
        let next_queue: Vec<u32> = std::mem::take(&mut queue);
        for id in next_queue {
            result.insert(id, rank);
            rank += 1;
            if let Some(neighbors) = adj.get(&id) {
                for &n in neighbors {
                    if let Some(deg) = in_degree.get_mut(&n) {
                        *deg -= 1;
                        if *deg == 0 {
                            queue.push(n);
                        }
                    }
                }
            }
        }
    }

    result
}

// ── Anchor sorting ─────────────────────────────────────────────────

fn sort_anchors_by_power(
    anchors: &[Anchor],
    comp_nets: &[HashSet<u32>],
    power_order: &HashMap<u32, usize>,
    net_kinds: &HashMap<u32, NetKind>,
    schematic: &Schematic,
) -> Vec<Anchor> {
    let mut sorted: Vec<&Anchor> = anchors.iter().collect();

    sorted.sort_by_key(|a| {
        let _comp = &schematic.components[a.index];
        let nets = &comp_nets[a.index];

        let power_ranks: Vec<usize> = nets
            .iter()
            .filter_map(|nid| {
                if matches!(net_kinds.get(nid), Some(NetKind::Power)) {
                    power_order.get(nid).copied()
                } else {
                    None
                }
            })
            .collect();

        let effective_rank = match a.kind {
            AnchorKind::Connector => {
                // Connectors: use min rank (placed at edges based on supply position)
                // J1 connects VBAT_IN (rank 0) → leftmost
                // J2 connects VBAT_7V4/5V_BUCK/3V3_STBY → needs special handling
                // Use min rank: input connectors will naturally be leftmost
                // Output connectors will be mid-right
                power_ranks.iter().min().copied().unwrap_or(usize::MAX)
            }
            _ => power_ranks.iter().min().copied().unwrap_or(usize::MAX),
        };

        (effective_rank, a.index)
    });

    // Post-process: move last connector to the end (output connector → rightmost)
    // Find connectors and ensure the one with the highest index is last
    let connector_indices: Vec<usize> = sorted
        .iter()
        .enumerate()
        .filter(|(_, a)| a.kind == AnchorKind::Connector)
        .map(|(i, _)| i)
        .collect();

    if connector_indices.len() >= 2 {
        // The last connector (highest reference, e.g. J2) should be after all ICs
        let last_conn_pos = *connector_indices.last().unwrap();
        let last_conn = sorted.remove(last_conn_pos);
        sorted.push(last_conn);
    }

    sorted.into_iter().cloned().collect()
}

// ── Section grouping ───────────────────────────────────────────────

fn group_into_sections(
    anchors: &[Anchor],
    comp_nets: &[HashSet<u32>],
    comp_kinds: &[ComponentKind],
    total: usize,
) -> Vec<Section> {
    let mut assigned: HashSet<usize> = anchors.iter().map(|a| a.index).collect();
    let mut sections: Vec<Section> = anchors
        .iter()
        .map(|a| Section {
            anchor_index: a.index,
            anchor_kind: a.kind,
            member_indices: Vec::new(),
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        })
        .collect();

    // For each anchor, collect its unique nets (excluding GND)
    let _gnd_net_ids: HashSet<u32> = comp_nets
        .iter()
        .flatten()
        .filter(|&&_nid| {
            // We'll need the net kind, but we don't have it here
            // Just collect all nets for now
            false
        })
        .copied()
        .collect();

    // Anchor nets: sets of net IDs for each anchor
    let anchor_nets: Vec<HashSet<u32>> =
        anchors.iter().map(|a| comp_nets[a.index].clone()).collect();

    // Find universal nets (shared by all anchors) — exclude from scoring
    let universal: HashSet<u32> = if anchor_nets.len() > 1 {
        anchor_nets
            .iter()
            .skip(1)
            .fold(anchor_nets[0].clone(), |acc, s| {
                acc.intersection(s).copied().collect()
            })
    } else {
        HashSet::new()
    };

    // Assign 2-pin, non-anchor, non-connector components to best anchor
    for (i, kind) in comp_kinds.iter().enumerate() {
        if assigned.contains(&i) {
            continue;
        }
        if matches!(kind, ComponentKind::Connector) {
            continue;
        }

        let c_nets = &comp_nets[i];

        // Find best anchor by unique-net overlap
        let mut best_section = 0;
        let mut best_overlap = 0;
        for (si, a_nets) in anchor_nets.iter().enumerate() {
            let unique_a: HashSet<u32> = a_nets.difference(&universal).copied().collect();
            let overlap = c_nets.intersection(&unique_a).count();
            if overlap > best_overlap {
                best_overlap = overlap;
                best_section = si;
            }
        }

        if best_overlap > 0 {
            sections[best_section].member_indices.push(i);
            assigned.insert(i);
        }
    }

    // Unassigned components → assign by signal net overlap (not just power nets)
    // This catches MOSFETs like Q3 that connect via gate signal nets
    for i in 0..total {
        if assigned.contains(&i) {
            continue;
        }
        if matches!(comp_kinds[i], ComponentKind::Connector) {
            continue;
        }

        let c_nets = &comp_nets[i];

        // Try signal net overlap (any net, not just unique/power)
        let mut best_section = 0;
        let mut best_overlap = 0;
        for (si, a_nets) in anchor_nets.iter().enumerate() {
            let overlap = c_nets.intersection(a_nets).count();
            if overlap > best_overlap {
                best_overlap = overlap;
                best_section = si;
            }
        }

        if best_overlap > 0 {
            sections[best_section].member_indices.push(i);
            assigned.insert(i);
        } else if !sections.is_empty() {
            // Absolute fallback: first section
            sections[0].member_indices.push(i);
            assigned.insert(i);
        }
    }

    // Handle unassigned connectors → create separate sections
    for (i, kind) in comp_kinds.iter().enumerate() {
        if assigned.contains(&i) {
            continue;
        }
        if matches!(kind, ComponentKind::Connector) {
            sections.push(Section {
                anchor_index: i,
                anchor_kind: AnchorKind::Connector,
                member_indices: Vec::new(),
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            });
            assigned.insert(i);
        }
    }

    sections
}

// ── Section positioning ────────────────────────────────────────────

/// Compute section bounding box from placed positions (relative to 0,0).
fn compute_section_bbox(section: &mut Section, positions: &HashMap<usize, (f64, f64, f64)>) {
    let mut min_x = f64::MAX;
    let mut min_y = f64::MAX;
    let mut max_x = f64::MIN;
    let mut max_y = f64::MIN;

    let all_indices: Vec<usize> = std::iter::once(section.anchor_index)
        .chain(section.member_indices.iter().copied())
        .collect();

    for &idx in &all_indices {
        if let Some(&(x, y, _)) = positions.get(&idx) {
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
    }

    if min_x == f64::MAX {
        section.width = 25.4;
        section.height = 25.4;
        return;
    }

    // Padding: vertical padding + horizontal padding for global_label flag shapes
    // Global labels extend ~15mm from pin position on each side
    let v_pad = GRID * 2.0;
    let h_pad = 15.24; // ~6 grid units, covers global_label flag+text on each side
    section.x = snap_to_grid(min_x - h_pad);
    section.y = snap_to_grid(min_y - v_pad);
    section.width = snap_to_grid(max_x - min_x + h_pad * 2.0);
    section.height = snap_to_grid(max_y - min_y + v_pad * 2.0);
}

/// Arrange sections by bounding boxes, auto-wrapping to stay within A4.
/// Returns divider lines: horizontal between rows, vertical between sections in same row.
fn arrange_sections_by_bboxes(sections: &mut [Section]) -> Vec<DividerLine> {
    let mut dividers = Vec::new();

    // Track rows: each entry is (section_index, abs_x, abs_y, w, h)
    #[allow(clippy::type_complexity)]
    let mut rows: Vec<Vec<(usize, f64, f64, f64, f64)>> = Vec::new();
    let mut current_row: Vec<(usize, f64, f64, f64, f64)> = Vec::new();

    let mut x_cursor = START_X;
    let mut y_cursor = START_Y;
    let mut row_max_h = 0.0f64;

    let connector_count = sections
        .iter()
        .filter(|s| s.anchor_kind == AnchorKind::Connector)
        .count();
    let mut conn_placed = 0;

    for (si, section) in sections.iter_mut().enumerate() {
        let w = section.width;
        let h = section.height;
        let old_x = section.x;
        let old_y = section.y;

        let is_output_connector =
            section.anchor_kind == AnchorKind::Connector && conn_placed == connector_count - 1;

        if is_output_connector {
            section.x = snap_to_grid(x_cursor - old_x);
            section.y = snap_to_grid(y_cursor - old_y);
            current_row.push((si, x_cursor, y_cursor, w, h));
            x_cursor += w + SECTION_GAP;
            row_max_h = row_max_h.max(h);
            conn_placed += 1;
            continue;
        }

        if section.anchor_kind == AnchorKind::Connector {
            conn_placed += 1;
        }

        // Auto-wrap within A4 width
        if x_cursor + w > A4_W && x_cursor > START_X {
            rows.push(std::mem::take(&mut current_row));
            x_cursor = START_X;
            y_cursor += row_max_h + ROW_GAP;
            row_max_h = 0.0;
        }

        section.x = snap_to_grid(x_cursor - old_x);
        section.y = snap_to_grid(y_cursor - old_y);
        current_row.push((si, x_cursor, y_cursor, w, h));
        x_cursor += w + SECTION_GAP;
        row_max_h = row_max_h.max(h);
    }

    if !current_row.is_empty() {
        rows.push(current_row);
    }

    // Generate dividers from row information
    for (ri, row) in rows.iter().enumerate() {
        if row.is_empty() {
            continue;
        }

        let row_top = row
            .iter()
            .map(|(_, _, y, _, _)| *y)
            .fold(f64::MAX, f64::min);
        let row_bottom = row
            .iter()
            .map(|(_, _, y, _, h)| y + h)
            .fold(f64::MIN, f64::max);
        let row_left = row
            .iter()
            .map(|(_, x, _, _, _)| *x)
            .fold(f64::MAX, f64::min);
        let row_right = row
            .iter()
            .map(|(_, x, _, w, _)| x + w)
            .fold(f64::MIN, f64::max);

        // Vertical dividers between adjacent sections in the same row
        for i in 0..row.len().saturating_sub(1) {
            let (_, ax, _, aw, _) = row[i];
            let (_, bx, _, _, _) = row[i + 1];
            let gap_mid_x = snap_to_grid((ax + aw + bx) / 2.0);
            dividers.push(DividerLine {
                x1: gap_mid_x,
                y1: row_top,
                x2: gap_mid_x,
                y2: row_bottom,
            });
        }

        // Horizontal divider between this row and the next
        if ri < rows.len() - 1 {
            let next_row_top = rows[ri + 1]
                .iter()
                .map(|(_, _, y, _, _)| *y)
                .fold(f64::MAX, f64::min);
            let div_y = snap_to_grid((row_bottom + next_row_top) / 2.0);
            dividers.push(DividerLine {
                x1: row_left - 5.0,
                y1: div_y,
                x2: row_right + 5.0,
                y2: div_y,
            });
        }
    }

    dividers
}

// ── Component placement within section ─────────────────────────────

#[allow(clippy::too_many_arguments)]
fn place_section_components(
    section: &Section,
    schematic: &Schematic,
    comp_kinds: &[ComponentKind],
    comp_nets: &[HashSet<u32>],
    net_id_to_name: &HashMap<u32, String>,
    _net_name_to_id: &HashMap<String, u32>,
    net_kinds: &HashMap<u32, NetKind>,
    positions: &mut HashMap<usize, (f64, f64, f64)>,
) {
    let anchor = &schematic.components[section.anchor_index];

    // Place anchor at section origin
    let ax = snap_to_grid(section.x + 25.4);
    let ay = snap_to_grid(section.y + 25.4);
    positions.insert(section.anchor_index, (ax, ay, 0.0));

    if section.member_indices.is_empty() {
        return;
    }

    // Identify anchor's power input and power output nets
    let mut anchor_power_in_nets: HashSet<u32> = HashSet::new();
    let mut anchor_power_out_nets: HashSet<u32> = HashSet::new();
    let mut anchor_special_nets: HashMap<String, u32> = HashMap::new();

    for pin in &anchor.pins {
        let Some(net_id) = pin.net_id else { continue };
        if !matches!(net_kinds.get(&net_id), Some(NetKind::Power)) {
            continue;
        };
        let pt = pin.pin_type.to_lowercase();
        if pt.contains("power_in") || pt.contains("input") {
            anchor_power_in_nets.insert(net_id);
        } else if pt.contains("power_out") || pt.contains("output") {
            anchor_power_out_nets.insert(net_id);
        }
    }

    // Also track special nets (FB, SW, BST, EN) by name
    for pin in &anchor.pins {
        let Some(net_id) = pin.net_id else { continue };
        if let Some(name) = net_id_to_name.get(&net_id) {
            let lu = name.to_uppercase();
            if lu.contains("FB") || lu.contains("BST") || lu.contains("SW_") {
                anchor_special_nets.insert(lu, net_id);
            }
        }
    }

    // For non-IC anchors (connectors, MOSFETs, fuses), do simple chain layout
    if section.anchor_kind != AnchorKind::Ic {
        place_non_ic_section(
            section,
            schematic,
            comp_kinds,
            comp_nets,
            net_id_to_name,
            net_kinds,
            positions,
        );
        return;
    }

    // ── IC-centric placement ──
    let mut input_caps: Vec<usize> = Vec::new();
    let mut output_caps: Vec<usize> = Vec::new();
    let mut inductors: Vec<usize> = Vec::new();
    let mut fb_resistors: Vec<usize> = Vec::new();
    let mut bst_caps: Vec<usize> = Vec::new();
    let mut gate_resistors: Vec<usize> = Vec::new();
    let mut pull_resistors: Vec<usize> = Vec::new();
    let mut others: Vec<usize> = Vec::new();

    for &mi in &section.member_indices {
        let kind = comp_kinds[mi];
        let m_nets = &comp_nets[mi];

        let connects_in = m_nets.iter().any(|n| anchor_power_in_nets.contains(n));
        let connects_out = m_nets.iter().any(|n| anchor_power_out_nets.contains(n));
        let connects_gate = m_nets.iter().any(|&nid| {
            net_id_to_name
                .get(&nid)
                .map(|n| n.to_uppercase().contains("GATE"))
                .unwrap_or(false)
        });
        let connects_fb = m_nets.iter().any(|&nid| {
            net_id_to_name
                .get(&nid)
                .map(|n| n.to_uppercase().contains("FB"))
                .unwrap_or(false)
        });
        let connects_bst = m_nets.iter().any(|&nid| {
            net_id_to_name
                .get(&nid)
                .map(|n| n.to_uppercase().contains("BST"))
                .unwrap_or(false)
        });

        match kind {
            ComponentKind::Capacitor => {
                if connects_bst {
                    bst_caps.push(mi);
                } else if connects_in {
                    input_caps.push(mi);
                } else if connects_out {
                    output_caps.push(mi);
                } else {
                    others.push(mi);
                }
            }
            ComponentKind::Inductor => {
                inductors.push(mi);
            }
            ComponentKind::Resistor => {
                if connects_fb {
                    fb_resistors.push(mi);
                } else if connects_gate {
                    gate_resistors.push(mi);
                } else if connects_in || connects_out {
                    pull_resistors.push(mi);
                } else {
                    others.push(mi);
                }
            }
            _ => {
                others.push(mi);
            }
        }
    }

    let gap = COMPONENT_GAP;
    let grid2 = GRID * 2.0;

    // Left side: input caps stacked vertically
    for (i, &mi) in input_caps.iter().enumerate() {
        let x = snap_to_grid(ax - gap);
        let y = snap_to_grid(ay + i as f64 * grid2);
        positions.insert(mi, (x, y, 0.0));
    }

    // Right side column 1: output caps
    let right_x1 = snap_to_grid(ax + gap);
    for (i, &mi) in output_caps.iter().enumerate() {
        let x = right_x1;
        let y = snap_to_grid(ay + i as f64 * grid2);
        positions.insert(mi, (x, y, 0.0));
    }

    // Right side column 2: inductors + BST caps
    let right_x2 = snap_to_grid(ax + gap * 2.0);
    for (i, &mi) in inductors.iter().enumerate() {
        let x = right_x2;
        let y = snap_to_grid(ay + i as f64 * grid2);
        positions.insert(mi, (x, y, 0.0));
    }
    for (i, &mi) in bst_caps.iter().enumerate() {
        let x = right_x2;
        let y = snap_to_grid(ay - grid2 - i as f64 * grid2);
        positions.insert(mi, (x, y, 0.0));
    }

    // Right side column 3: feedback resistors (stacked down with gap)
    let right_x3 = snap_to_grid(ax + gap * 3.0);
    for (i, &mi) in fb_resistors.iter().enumerate() {
        let x = right_x3;
        let y = snap_to_grid(ay + i as f64 * COMPONENT_GAP);
        positions.insert(mi, (x, y, 0.0));
    }

    // Below IC: pull resistors (input/output related)
    let below_y = snap_to_grid(ay + gap);
    for (i, &mi) in pull_resistors.iter().enumerate() {
        let x = snap_to_grid(ax + (i as f64 - pull_resistors.len() as f64 / 2.0) * grid2);
        let y = below_y;
        positions.insert(mi, (x, y, 0.0));
    }

    // Gate resistors: below, left of anchor
    for (i, &mi) in gate_resistors.iter().enumerate() {
        let x = snap_to_grid(ax - gap + i as f64 * grid2);
        let y = snap_to_grid(ay + gap);
        positions.insert(mi, (x, y, 0.0));
    }

    // Others: place in a grid below-right
    let other_start_y = snap_to_grid(ay + gap + grid2);
    for (i, &mi) in others.iter().enumerate() {
        let col = i % 3;
        let row = i / 3;
        let x = snap_to_grid(ax - gap + col as f64 * gap);
        let y = snap_to_grid(other_start_y + row as f64 * grid2);
        positions.insert(mi, (x, y, 0.0));
    }
}

/// Place components for non-IC sections (connectors, MOSFETs, fuses).
/// Uses a compact chain layout.
#[allow(clippy::too_many_arguments)]
fn place_non_ic_section(
    section: &Section,
    schematic: &Schematic,
    _comp_kinds: &[ComponentKind],
    _comp_nets: &[HashSet<u32>],
    _net_id_to_name: &HashMap<u32, String>,
    _net_kinds: &HashMap<u32, NetKind>,
    positions: &mut HashMap<usize, (f64, f64, f64)>,
) {
    let _anchor = &schematic.components[section.anchor_index];
    let ax = snap_to_grid(section.x + 12.7);
    let ay = snap_to_grid(section.y + 25.4);
    positions.insert(section.anchor_index, (ax, ay, 0.0));

    let gap = COMPONENT_GAP;
    let grid2 = GRID * 2.0;

    // Place members to the right and below in a compact grid
    for (i, &mi) in section.member_indices.iter().enumerate() {
        let col = i % 2;
        let row = i / 2;
        let x = snap_to_grid(ax + (col as f64 + 1.0) * gap);
        let y = snap_to_grid(ay + row as f64 * grid2);
        positions.insert(mi, (x, y, 0.0));
    }
}

// ── Remaining components ───────────────────────────────────────────

fn place_remaining(
    sections: &[Section],
    _comp_kinds: &[ComponentKind],
    positions: &mut HashMap<usize, (f64, f64, f64)>,
    total: usize,
) {
    // Find max X from sections to place remaining on the right
    let max_x = sections
        .iter()
        .map(|s| s.x + s.width)
        .fold(0.0f64, f64::max);
    let mut x_cursor = snap_to_grid(max_x + SECTION_GAP);
    let mut y_cursor = START_Y;

    for i in 0..total {
        if positions.contains_key(&i) {
            continue;
        }

        // Place in a column on the right side
        positions.insert(i, (snap_to_grid(x_cursor), snap_to_grid(y_cursor), 0.0));
        y_cursor += COMPONENT_GAP;

        // Wrap to next column
        if y_cursor > 200.0 {
            y_cursor = START_Y;
            x_cursor += COMPONENT_GAP;
        }
    }
}

// ── Utilities ──────────────────────────────────────────────────────

fn snap_to_grid(v: f64) -> f64 {
    (v / GRID).round() * GRID
}

/// Estimate a component's half-height from center to its bottommost visual extent,
/// including body + pin tips + global label room.
fn estimate_body_half_height(lib_id: &str, pin_count: usize) -> f64 {
    let short = lib_id.split(':').next_back().unwrap_or(lib_id);
    let is_dual = short.contains("02x") || short.contains("_02x") || short.contains("Conn_02");
    let spacing = 5.08_f64;
    // Room for global label flag + text extending below pin tip
    let label_room = 5.08;

    if is_dual {
        let rows = pin_count.div_ceil(2);
        let half_span = if rows > 1 {
            ((rows - 1) as f64 / 2.0) * spacing
        } else {
            0.0
        };
        half_span + 2.54 + label_room
    } else if pin_count > 2 {
        let left = pin_count.div_ceil(2);
        let right = pin_count - left;
        let max_per_side = left.max(right).max(1);
        let half_span = if max_per_side > 1 {
            ((max_per_side - 1) as f64 / 2.0) * spacing
        } else {
            0.0
        };
        half_span + 2.54 + label_room
    } else {
        // 2-pin: pin tip distance + global label room
        let lu = lib_id.to_uppercase();
        let pin_tip = if lu.contains("CP") || lu.contains("ELECTRO") {
            5.08
        } else {
            5.81
        };
        pin_tip + label_room
    }
}

fn generate_section_label(section: &Section, schematic: &Schematic) -> String {
    let anchor = &schematic.components[section.anchor_index];
    let lib = anchor.lib_id.to_uppercase();
    match section.anchor_kind {
        AnchorKind::Connector => {
            // Heuristic: connector with lower reference number is input
            if anchor.reference.starts_with('J') {
                let num: u32 = anchor.reference[1..].parse().unwrap_or(0);
                if num <= 1 {
                    "Input".to_string()
                } else {
                    "Output".to_string()
                }
            } else {
                "Connector".to_string()
            }
        }
        AnchorKind::Ic => {
            if lib.contains("SY81") || lib.contains("MP15") || lib.contains("XL15") {
                "Buck Converter".to_string()
            } else if lib.contains("RT91") || lib.contains("AMS1117") || lib.contains("LD1117") {
                "LDO Regulator".to_string()
            } else if lib.contains("CH34") {
                "USB-UART Bridge".to_string()
            } else if lib.contains("SN65") || lib.contains("MAX48") {
                "Transceiver".to_string()
            } else if lib.contains("ADS") {
                "ADC".to_string()
            } else if lib.contains("STM32") || lib.contains("ESP32") {
                "MCU".to_string()
            } else {
                anchor.value.clone()
            }
        }
        AnchorKind::Protection => {
            if lib.contains("PPTC") {
                "Over-current Protection".to_string()
            } else {
                "Protection".to_string()
            }
        }
        AnchorKind::Transistor => {
            if lib.contains("AO34") || lib.contains("SI23") {
                "Load Switch".to_string()
            } else if lib.contains("2N700") || lib.contains("BSS") {
                "Switch Control".to_string()
            } else {
                "Transistor".to_string()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snap_to_grid() {
        assert_eq!(snap_to_grid(25.4), 25.4);
        assert_eq!(snap_to_grid(25.5), 25.4);
        assert_eq!(snap_to_grid(26.67), 27.94);
        assert_eq!(snap_to_grid(0.0), 0.0);
    }

    #[test]
    fn test_empty_schematic() {
        let sch = Schematic::default();
        let layout = auto_layout(&sch);
        assert!(layout.sections.is_empty());
        assert!(layout.positions.is_empty());
    }
}
