use anyhow::{Context, Result};
use std::collections::BTreeMap;

use kicad_json5::Schematic;

/// Generate a KiCad-format netlist from a parsed Schematic IR.
/// Output is in the (export (version "E") ...) S-expression format
/// compatible with KiCad 6/7/8.
pub fn generate_netlist(schematic: &Schematic) -> Result<String> {
    let mut out = String::new();

    // Header
    out.push_str("(export (version \"E\")\n");

    // Components section
    out.push_str("  (components\n");
    for comp in &schematic.components {
        out.push_str("    (comp\n");
        out.push_str(&format!("      (ref \"{}\")\n", comp.reference));
        out.push_str(&format!("      (value \"{}\")\n", escape_sch(&comp.value)));
        if let Some(fp) = &comp.footprint {
            out.push_str(&format!("      (footprint \"{}\")\n", fp));
        }
        if let Some(lib) = parse_lib_name(&comp.lib_id) {
            out.push_str(&format!("      (libsource (lib \"{}\") (part \"{}\"))\n",
                lib, parse_part_name(&comp.lib_id).unwrap_or(&comp.value)));
        }
        if let Some(uuid) = &comp.uuid {
            out.push_str(&format!("      (tstamp {})\n", uuid));
        }
        out.push_str("    )\n");
    }
    out.push_str("  )\n");

    // Nets section — collect pins by net
    let mut net_pins: BTreeMap<Option<u32>, Vec<(String, String)>> = BTreeMap::new();
    for comp in &schematic.components {
        for pin in &comp.pins {
            let key = pin.net_id;
            net_pins.entry(key)
                .or_default()
                .push((comp.reference.clone(), pin.number.clone()));
        }
    }

    out.push_str("  (nets\n");
    let mut code: u32 = 0;

    // Build net name lookup
    let net_names: BTreeMap<u32, String> = schematic.nets.iter()
        .map(|n| (n.id, n.name.clone()))
        .collect();

    // Pinned nets (have a net_id)
    let mut sorted_nets: Vec<(u32, &String)> = net_names.iter()
        .map(|(id, name)| (*id, name))
        .collect();
    sorted_nets.sort_by_key(|(id, _)| *id);

    for (net_id, net_name) in &sorted_nets {
        code += 1;
        out.push_str(&format!("    (net (code {}) (name \"{}\")\n", code, escape_sch(net_name)));
        if let Some(pins) = net_pins.get(&Some(*net_id)) {
            for (ref_des, pin_num) in pins {
                out.push_str(&format!("      (node (ref \"{}\") (pin \"{}\"))\n", ref_des, pin_num));
            }
        }
        out.push_str("    )\n");
    }

    out.push_str("  )\n");
    out.push_str(")\n");

    Ok(out)
}

/// Parse schematic file and generate netlist in one step
pub fn netlist_from_file(path: &str) -> Result<String> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read schematic file: {}", path))?;

    let format = if path.ends_with(".json5") || path.ends_with(".json") {
        kicad_json5::InputFormat::Json5
    } else {
        kicad_json5::InputFormat::Sexpr
    };

    let schematic = kicad_json5::parse_schematic(&content, format)
        .with_context(|| format!("Failed to parse schematic: {}", path))?;

    generate_netlist(&schematic)
}

fn parse_lib_name(lib_id: &str) -> Option<&str> {
    lib_id.split(':').next()
}

fn parse_part_name(lib_id: &str) -> Option<&str> {
    lib_id.split(':').nth(1)
}

fn escape_sch(s: &str) -> String {
    s.replace('"', "\\\"")
}
