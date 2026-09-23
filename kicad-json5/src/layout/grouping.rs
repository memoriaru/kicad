//! IC peripheral grouping and auto-constraint generation.

use std::collections::{HashMap, HashSet};

use super::types::PlacementEntry;

/// A group of components centered around an IC.
#[derive(Debug)]
pub struct ComponentGroup {
    pub ic_index: usize,
    pub peripheral_indices: Vec<usize>,
    pub anchor_pos: (f64, f64),
}

/// Classify a component as an IC (not a transistor, diode, fuse, or connector).
pub fn is_ic_component(lib_id: &str, pin_count: usize) -> bool {
    if lib_id.starts_with("Device:") || lib_id.starts_with("Conn") || lib_id.contains("PPTC") {
        return false;
    }
    // Exclude common discrete semiconductors (3-pin transistors, diodes)
    if pin_count <= 4 {
        let lu = lib_id.to_uppercase();
        // MOSFET/BJT part numbers
        if lu.contains("AO34") || lu.contains("2N700") || lu.contains("BSS") || lu.contains("SI23")
        {
            return false;
        }
        // Generic transistor/diode patterns
        if lu.starts_with("Q_") || lu.starts_with("D_") || lu.starts_with("LED") {
            return false;
        }
    }
    pin_count >= 3
}

/// Classify a component as a connector.
pub fn is_connector_component(lib_id: &str) -> bool {
    lib_id.starts_with("Conn")
}

/// Group components around ICs based on net connectivity.
///
/// Uses best-match assignment: each 2-pin non-connector component is assigned
/// to the IC with which it shares the most *unique* nets (excluding nets shared
/// by all ICs, e.g. GND). This avoids a greedy first-IC-wins problem.
pub fn group_by_ic(
    comp_lib_ids: &[&str],
    comp_net_ids: &[Vec<Option<u32>>],
    anchor_start: (f64, f64),
    anchor_spacing: f64,
) -> Vec<ComponentGroup> {
    let n = comp_lib_ids.len();

    let ic_indices: Vec<usize> = (0..n)
        .filter(|&i| is_ic_component(comp_lib_ids[i], comp_net_ids[i].len()))
        .collect();

    let mut assigned: HashSet<usize> = ic_indices.iter().copied().collect();

    // Collect each IC's nets
    let ic_nets: Vec<HashSet<u32>> = ic_indices
        .iter()
        .map(|&ic_idx| comp_net_ids[ic_idx].iter().copied().flatten().collect())
        .collect();

    // Find nets shared by ALL ICs (e.g. GND) — exclude from scoring
    let universal_nets: HashSet<u32> = if ic_nets.len() > 1 {
        ic_nets.iter().skip(1).fold(ic_nets[0].clone(), |acc, s| {
            acc.intersection(s).copied().collect()
        })
    } else {
        HashSet::new()
    };

    // For each IC, compute unique nets (not shared with all other ICs)
    let ic_unique_nets: Vec<HashSet<u32>> = ic_nets
        .iter()
        .map(|s| s.difference(&universal_nets).copied().collect())
        .collect();

    // Collect candidate peripherals: 2-pin, non-IC, non-connector
    let candidates: Vec<usize> = (0..n)
        .filter(|&i| {
            !assigned.contains(&i)
                && !is_connector_component(comp_lib_ids[i])
                && comp_net_ids[i].len() == 2
        })
        .collect();

    // For each candidate, find the IC with the highest unique-net overlap
    for &cidx in &candidates {
        let c_nets: HashSet<u32> = comp_net_ids[cidx].iter().copied().flatten().collect();
        let best_ic = ic_indices
            .iter()
            .enumerate()
            .max_by_key(|&(gi, _)| c_nets.intersection(&ic_unique_nets[gi]).count())
            .map(|(gi, _)| gi);

        if let Some(gi) = best_ic {
            // Only assign if there's at least one unique net overlap
            let overlap = c_nets.intersection(&ic_unique_nets[gi]).count();
            if overlap > 0 {
                assigned.insert(cidx);
                // We'll add to periph list below
            }
        }
    }

    // Build groups: collect peripherals for each IC
    // Re-do assignment to track which IC each peripheral goes to
    let mut periph_map: HashMap<usize, Vec<usize>> = HashMap::new();
    for &ic_idx in &ic_indices {
        periph_map.insert(ic_idx, Vec::new());
    }

    // Re-evaluate candidates for assignment
    let mut final_assigned: HashSet<usize> = ic_indices.iter().copied().collect();
    for &cidx in &candidates {
        let c_nets: HashSet<u32> = comp_net_ids[cidx].iter().copied().flatten().collect();
        let mut best_gi = 0usize;
        let mut best_overlap = 0;
        for (gi, _) in ic_indices.iter().enumerate() {
            let overlap = c_nets.intersection(&ic_unique_nets[gi]).count();
            if overlap > best_overlap {
                best_overlap = overlap;
                best_gi = gi;
            }
        }
        if best_overlap > 0 {
            let ic_idx = ic_indices[best_gi];
            periph_map.get_mut(&ic_idx).unwrap().push(cidx);
            final_assigned.insert(cidx);
        }
    }

    let mut groups = Vec::new();
    for (group_idx, &ic_idx) in ic_indices.iter().enumerate() {
        let x = anchor_start.0 + group_idx as f64 * anchor_spacing;
        let y = anchor_start.1;
        groups.push(ComponentGroup {
            ic_index: ic_idx,
            peripheral_indices: periph_map[&ic_idx].clone(),
            anchor_pos: (x, y),
        });
    }

    groups
}

/// Auto-generate a placement constraint for a peripheral component.
///
/// Returns `(constraint_entry, new_fb_role)`. When the component is the first
/// FB resistor in a chain, `new_fb_role` is set to `Some(current_role)` so the
/// caller can track it for subsequent OffsetFrom constraints.
#[allow(clippy::too_many_arguments)]
pub fn build_peripheral_constraint(
    periph_lib_id: &str,
    periph_net_ids: &[Option<u32>],
    ic_power_nets: &HashSet<u32>,
    ic_output_nets: &HashSet<u32>,
    ic_special_pins: &HashMap<String, u32>,
    power_pin_name: Option<&str>,
    output_pin_name: Option<&str>,
    prev_fb_role: Option<&str>,
    current_role: &str,
) -> (PlacementEntry, Option<String>) {
    let p_nets: HashSet<u32> = periph_net_ids.iter().copied().flatten().collect();
    let lib_u = periph_lib_id.to_uppercase();
    let connects_power = p_nets.intersection(ic_power_nets).count() > 0;
    let connects_output = p_nets.intersection(ic_output_nets).count() > 0;
    let connects_special: Vec<(&str, &u32)> = ic_special_pins
        .iter()
        .filter(|(_, nid)| p_nets.contains(nid))
        .map(|(n, nid)| (n.as_str(), nid))
        .collect();

    let is_fb_resistor =
        lib_u.contains(":R") && connects_special.iter().any(|(n, _)| n.contains("FB"));

    let entry = if (lib_u.contains(":C") || lib_u.contains(":CP")) && connects_power {
        let pin = power_pin_name.unwrap_or("VIN");
        PlacementEntry::NearPin {
            near_pin: format!("ic.{}", pin),
            max_distance_mil: 400.0,
            side: Some("left".into()),
            clearance_mil: Some(150.0),
        }
    } else if (lib_u.contains(":C") || lib_u.contains(":CP")) && connects_output {
        let pin = output_pin_name.unwrap_or("VOUT");
        PlacementEntry::NearPin {
            near_pin: format!("ic.{}", pin),
            max_distance_mil: 400.0,
            side: Some("right".into()),
            clearance_mil: Some(200.0),
        }
    } else if lib_u.contains(":L") && connects_output {
        let pin = output_pin_name.unwrap_or("SW");
        PlacementEntry::NearPin {
            near_pin: format!("ic.{}", pin),
            max_distance_mil: 400.0,
            side: Some("right".into()),
            clearance_mil: Some(150.0),
        }
    } else if lib_u.contains(":C") && connects_special.iter().any(|(n, _)| n.contains("BST")) {
        let bst = connects_special
            .iter()
            .find(|(n, _)| n.contains("BST"))
            .map(|(n, _)| *n)
            .unwrap_or("BST");
        PlacementEntry::NearPin {
            near_pin: format!("ic.{}", bst),
            max_distance_mil: 300.0,
            side: Some("right".into()),
            clearance_mil: Some(100.0),
        }
    } else if is_fb_resistor {
        if let Some(prev) = prev_fb_role {
            PlacementEntry::OffsetFrom {
                offset_from: prev.to_string(),
                dx_mil: 0.0,
                dy_mil: 300.0,
            }
        } else {
            let fb = connects_special
                .iter()
                .find(|(n, _)| n.contains("FB"))
                .map(|(n, _)| *n)
                .unwrap_or("FB");
            let out_pin = output_pin_name.unwrap_or("VOUT");
            PlacementEntry::BetweenPins {
                pin_a: format!("ic.{}", out_pin),
                pin_b: format!("ic.{}", fb),
                offset_mil: Some(300.0),
                side: Some("right".into()),
            }
        }
    } else {
        PlacementEntry::NearPin {
            near_pin: "ic.GND".into(),
            max_distance_mil: 500.0,
            side: None,
            clearance_mil: Some(150.0),
        }
    };

    let new_fb_role = if is_fb_resistor && prev_fb_role.is_none() {
        Some(current_role.to_string())
    } else {
        None
    };

    (entry, new_fb_role)
}
