use anyhow::Result;

use crate::ComponentDb;

const PIN_LENGTH: f64 = 2.54;
const PIN_SPACING: f64 = 2.54;

// ---------------------------------------------------------------------------
// Pin classification
// ---------------------------------------------------------------------------

fn is_ground_pin(name: &str) -> bool {
    let upper = name.to_uppercase();
    upper == "GND"
        || upper == "VSS"
        || upper == "DGND"
        || upper == "AGND"
        || upper == "PGND"
        || upper == "SGND"
        || upper.starts_with("GND")
        || upper.contains("GROUND")
}

fn is_supply_pin(name: &str) -> bool {
    let upper = name.to_uppercase();
    upper == "VCC"
        || upper == "VDD"
        || upper == "VCCD"
        || upper == "VCCA"
        || upper == "VIN"
        || upper == "VOUT"
        || upper == "AVDD"
        || upper == "DVDD"
        || upper == "VCCIO"
        || upper == "VREF"
        || upper == "VBAT"
        || upper == "VDDA"
        || upper == "VDDD"
        || upper == "VREG"
        || upper.starts_with("VDD")
        || upper.starts_with("VCC")
        || upper == "VBUS"
        || upper == "VUSB"
}

fn classify_pin(name: &str, etype: Option<&str>) -> PinClass {
    let upper = name.to_uppercase();

    if is_ground_pin(&upper) {
        return PinClass::PowerBottom;
    }
    if is_supply_pin(&upper) {
        return PinClass::PowerTop;
    }

    match etype.unwrap_or("passive") {
        "output" | "open_collector" | "open_emitter" | "power_out" => PinClass::Right,
        "input" | "clock" => PinClass::Left,
        _ => {
            if upper.starts_with("OUT")
                || upper.starts_with("SW")
                || upper.starts_with("TX")
                || upper.starts_with("DRV")
            {
                PinClass::Right
            } else if upper.starts_with("IN")
                || upper.starts_with("EN")
                || upper.starts_with("CLK")
                || upper.starts_with("CS")
                || upper.starts_with("RX")
                || upper.starts_with("FB")
            {
                PinClass::Left
            } else {
                PinClass::Left
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Body graphic inference — delegated to kicad-symgen
// ---------------------------------------------------------------------------

use kicad_symgen::symbol::graphics::{infer_body_graphic_from_names, BodyGraphic};

fn infer_body_graphic(pins: &[crate::Pin]) -> BodyGraphic {
    let pin_names: Vec<String> = pins.iter().map(|p| p.pin_name.to_uppercase()).collect();
    infer_body_graphic_from_names(&pin_names)
}

struct LayoutResult {
    body_width: f64,
    body_height: f64,
    pins: Vec<LayoutPin>,
}

#[derive(Debug)]
struct LayoutPin {
    index: usize,
    x: f64,
    y: f64,
    rotation: f64,
}

#[derive(Clone, Copy)]
enum PinClass {
    PowerTop,
    PowerBottom,
    Left,
    Right,
}

// ---------------------------------------------------------------------------
// Layout computation
// ---------------------------------------------------------------------------

fn compute_layout(pins: &[crate::Pin]) -> LayoutResult {
    let mut top_pins = Vec::new();
    let mut bottom_pins = Vec::new();
    let mut left_pins = Vec::new();
    let mut right_pins = Vec::new();

    for (i, pin) in pins.iter().enumerate() {
        let class = classify_pin(&pin.pin_name, pin.electrical_type.as_deref());
        match class {
            PinClass::PowerTop => top_pins.push(i),
            PinClass::PowerBottom => bottom_pins.push(i),
            PinClass::Left => left_pins.push(i),
            PinClass::Right => right_pins.push(i),
        }
    }

    // If all pins are on one side, split evenly
    if left_pins.is_empty() && right_pins.is_empty() {
        let half = pins.len() / 2;
        for (i, _pin) in pins.iter().enumerate() {
            if i < half {
                left_pins.push(i);
            } else {
                right_pins.push(i);
            }
        }
        top_pins.clear();
        bottom_pins.clear();
    } else if right_pins.is_empty() && top_pins.is_empty() && bottom_pins.is_empty() {
        let half = left_pins.len() / 2;
        let right: Vec<usize> = left_pins.split_off(half);
        right_pins = right;
    }

    let max_side = left_pins.len().max(right_pins.len()).max(1);
    let body_height = max_side as f64 * PIN_SPACING;
    let body_width = 5.08;

    let mut layout_pins = Vec::new();

    // Place left pins top to bottom
    let left_start = body_height / 2.0;
    for (j, &idx) in left_pins.iter().enumerate() {
        layout_pins.push(LayoutPin {
            index: idx,
            x: -body_width / 2.0 - PIN_LENGTH,
            y: left_start - j as f64 * PIN_SPACING,
            rotation: 0.0,
        });
    }

    // Place right pins top to bottom
    let right_start = body_height / 2.0;
    for (j, &idx) in right_pins.iter().enumerate() {
        layout_pins.push(LayoutPin {
            index: idx,
            x: body_width / 2.0 + PIN_LENGTH,
            y: right_start - j as f64 * PIN_SPACING,
            rotation: 180.0,
        });
    }

    // Place top power pins left to right
    let top_start = -(top_pins.len() as f64 - 1.0) * PIN_SPACING / 2.0;
    for (j, &idx) in top_pins.iter().enumerate() {
        layout_pins.push(LayoutPin {
            index: idx,
            x: top_start + j as f64 * PIN_SPACING,
            y: body_height / 2.0 + PIN_LENGTH,
            rotation: 90.0,
        });
    }

    // Place bottom ground pins left to right
    let bot_start = -(bottom_pins.len() as f64 - 1.0) * PIN_SPACING / 2.0;
    for (j, &idx) in bottom_pins.iter().enumerate() {
        layout_pins.push(LayoutPin {
            index: idx,
            x: bot_start + j as f64 * PIN_SPACING,
            y: -body_height / 2.0 - PIN_LENGTH,
            rotation: 270.0,
        });
    }

    LayoutResult {
        body_width,
        body_height,
        pins: layout_pins,
    }
}

// ---------------------------------------------------------------------------
// S-expression generation
// ---------------------------------------------------------------------------

pub fn generate_rich_symbol_lib(
    components: &[crate::Component],
    db: &ComponentDb,
) -> Result<String> {
    let mut out = String::new();
    out.push_str("(kicad_symbol_lib\n");
    out.push_str("  (version \"20231120\")\n");
    out.push_str("  (generator \"component-db\")\n");

    for comp in components {
        let id = comp.id.unwrap();
        let pins = db.get_pins(id)?;
        let name = comp.mpn.replace('.', "_");
        let is_passive_2pin = pins.len() == 2
            && comp.package.as_deref().is_some_and(|p| {
                let p = p.to_uppercase();
                p.contains("0402")
                    || p.contains("0603")
                    || p.contains("0805")
                    || p.contains("1206")
                    || p.contains("RESISTOR")
                    || p.contains("CAPACITOR")
            });

        out.push_str(&format!("  (symbol \"{}\"\n", name));
        out.push_str("    (in_bom yes)\n");
        out.push_str("    (on_board yes)\n");
        if is_passive_2pin {
            out.push_str("    (pin_numbers hide)\n");
        }
        out.push_str("    (pin_names (offset 0.254))\n");

        // Properties
        out.push_str(&format!(
            "    (property \"Reference\" \"{}\" (at 0 {} 0)\n",
            infer_ref_prefix(&comp.mpn),
            (pins.len().max(2) as f64 / 2.0 * 2.54 + 2.54)
        ));
        out.push_str("      (effects (font (size 1.27 1.27))))\n");
        out.push_str(&format!(
            "    (property \"Value\" \"{}\" (at 0 -{} 0)\n",
            comp.mpn,
            (pins.len().max(2) as f64 / 2.0 * 2.54 + 2.54)
        ));
        out.push_str("      (effects (font (size 1.27 1.27))))\n");
        if let Some(ref fp) = comp.kicad_footprint {
            out.push_str(&format!(
                "    (property \"Footprint\" \"{}\" (at 0 0 0)\n",
                fp
            ));
            out.push_str("      (effects (font (size 1.27 1.27)) hide))\n");
        }
        if let Some(ref url) = comp.datasheet_url {
            out.push_str(&format!(
                "    (property \"Datasheet\" \"{}\" (at 0 0 0)\n",
                url
            ));
            out.push_str("      (effects (font (size 1.27 1.27)) hide))\n");
        }
        out.push_str(&format!(
            "    (property \"Manufacturer\" \"{}\" (at 0 0 0)\n",
            comp.manufacturer
        ));
        out.push_str("      (effects (font (size 1.27 1.27)) hide))\n");
        out.push_str(&format!(
            "    (property \"MPN\" \"{}\" (at 0 0 0)\n",
            comp.mpn
        ));
        out.push_str("      (effects (font (size 1.27 1.27)) hide))\n");
        if comp.lifecycle != "active" {
            out.push_str(&format!(
                "    (property \"Lifecycle\" \"{}\" (at 0 0 0)\n",
                comp.lifecycle
            ));
            out.push_str("      (effects (font (size 1.27 1.27)) hide))\n");
        }
        if let Some(ref desc) = comp.description {
            out.push_str(&format!(
                "    (property \"Description\" \"{}\" (at 0 0 0)\n",
                desc.replace('"', "\\\"")
            ));
            out.push_str("      (effects (font (size 1.27 1.27)) hide))\n");
        }

        // Body graphics (unit 0_1)
        let mut layout = compute_layout(&pins);
        let body_graphic = infer_body_graphic(&pins);
        let hw = layout.body_width / 2.0;
        let hh = layout.body_height / 2.0 + 0.254;

        out.push_str(&format!("    (symbol \"{}_0_1\"\n", name));
        match body_graphic {
            BodyGraphic::Rectangle => {
                out.push_str(&format!(
                    "      (rectangle (start {} {}) (end {} {})\n",
                    -hw, hh, hw, -hh
                ));
                out.push_str("        (stroke (width 0.254) (type default))\n");
                out.push_str("        (fill (type background)))\n");
            }
            BodyGraphic::Triangle => {
                out.push_str("      (polyline\n");
                out.push_str("        (pts\n");
                out.push_str(&format!(
                    "          (xy {} {})\n          (xy {} {})\n          (xy {} {})\n          (xy {} {})\n",
                    -hw, hh, hw, 0.0, -hw, -hh, -hw, hh,
                ));
                out.push_str("        )\n");
                out.push_str("        (stroke (width 0.254) (type default))\n");
                out.push_str("        (fill (type background)))\n");
            }
            BodyGraphic::FunctionBlock { ref sections } => {
                out.push_str(&format!(
                    "      (rectangle (start {} {}) (end {} {})\n",
                    -hw, hh, hw, -hh
                ));
                out.push_str("        (stroke (width 0.254) (type default))\n");
                out.push_str("        (fill (type background)))\n");

                let section_count = sections.len().max(1);
                let section_width = (hw * 2.0) / section_count as f64;
                let label_y = hh - 1.27;

                for (i, label) in sections.iter().enumerate() {
                    if i > 0 {
                        let x = -hw + i as f64 * section_width;
                        out.push_str(&format!(
                            "      (line (start {} {}) (end {} {})\n",
                            x, hh, x, -hh
                        ));
                        out.push_str("        (stroke (width 0.127) (type default))\n");
                        out.push_str("        (fill (type none)))\n");
                    }
                    let label_x = -hw + (i as f64 + 0.5) * section_width;
                    out.push_str(&format!(
                        "      (text \"{}\" (at {} {}) (length 2.54)\n",
                        label, label_x, label_y
                    ));
                    out.push_str("        (effects (font (size 0.762 0.762))))\n");
                }
            }
        }
        out.push_str("    )\n");

        // Pins (unit 1_1)
        if !layout.pins.is_empty() {
            out.push_str(&format!("    (symbol \"{}_1_1\"\n", name));
            layout.pins.sort_by_key(|p| p.index);
            for lp in &layout.pins {
                let pin = &pins[lp.index];
                let etype = pin.electrical_type.as_deref().unwrap_or("passive");
                out.push_str(&format!(
                    "      (pin {} line (at {:.3} {:.3} {:.0}) (length {})\n",
                    etype, lp.x, lp.y, lp.rotation, PIN_LENGTH
                ));
                out.push_str(&format!(
                    "        (name \"{}\" (effects (font (size 1.27 1.27))))\n",
                    pin.pin_name
                ));
                out.push_str(&format!(
                    "        (number \"{}\" (effects (font (size 1.27 1.27))))\n",
                    pin.pin_number
                ));
                out.push_str("      )\n");
            }
            out.push_str("    )\n");
        }

        out.push_str("  )\n");
    }

    out.push_str(")\n");
    Ok(out)
}

fn infer_ref_prefix(mpn: &str) -> String {
    let upper = mpn.to_uppercase();
    if upper.starts_with("R") && !upper.starts_with("RT") && !upper.starts_with("REG") {
        return "R".into();
    }
    if upper.starts_with("C") && !upper.starts_with("CON") && !upper.starts_with("74") {
        return "C".into();
    }
    if upper.starts_with("L") && !upper.starts_with("LED") && !upper.starts_with("LM") {
        return "L".into();
    }
    if upper.starts_with("LED") {
        return "D".into();
    }
    if upper.starts_with("D") && !upper.starts_with("DIP") {
        return "D".into();
    }
    "U".into()
}

/// Generate a sym-lib-table entry line for a library file.
/// `lib_name` is the logical library name (e.g. "custom").
/// `lib_path` is the URI to the .kicad_sym file (relative or absolute).
pub fn generate_lib_table_entry(lib_name: &str, lib_path: &str) -> String {
    format!(
        "(lib (name \"{lib_name}\")(type \"KiCad\")(uri \"{lib_path}\")(options \"\")(descr \"\"))\n"
    )
}

/// Generate a complete sym-lib-table file from a list of (name, path) pairs.
pub fn generate_sym_lib_table(libs: &[(&str, &str)]) -> String {
    let mut out = String::from("(sym_lib_table\n");
    for (name, path) in libs {
        out.push_str(&generate_lib_table_entry(name, path));
    }
    out.push_str(")\n");
    out
}

/// Generate a .kicad_sym symbol library file for a single component by MPN.
/// Queries the component database, builds a kicad-symgen SymbolSpec, and generates the output.
pub fn generate_symbol_from_mpn(
    db_path: &str,
    mpn: &str,
    output_path: &str,
    lib_name: Option<&str>,
) -> Result<String> {
    let db = ComponentDb::open(db_path)?;

    let comp = db
        .get_component_by_mpn_any(mpn)?
        .ok_or_else(|| anyhow::anyhow!("Component '{}' not found in database", mpn))?;

    let comp_id = comp
        .id
        .ok_or_else(|| anyhow::anyhow!("Component '{}' has no database ID", mpn))?;

    let pins = db.get_pins(comp_id)?;

    // Convert DB pins to kicad-symgen SymbolPins
    let sym_pins: Vec<kicad_symgen::model::SymbolPin> = pins
        .iter()
        .map(|pin| {
            let etype = match pin.electrical_type.as_deref() {
                Some(t) => kicad_symgen::model::ElectricalType::from_str_lossy(t),
                None => infer_electrical_type_from_name(&pin.pin_name),
            };
            kicad_symgen::model::SymbolPin {
                number: pin.pin_number.clone(),
                name: pin.pin_name.clone(),
                electrical_type: etype,
                pin_group: pin.pin_group.clone(),
                alt_functions: pin
                    .alt_functions
                    .as_ref()
                    .map(|afs| afs.iter().map(|af| af.function.clone()).collect()),
            }
        })
        .collect();

    let lib = lib_name.unwrap_or("custom");
    let spec = kicad_symgen::model::SymbolSpec {
        mpn: comp.mpn.clone(),
        lib_name: lib.to_string(),
        reference_prefix: infer_ref_prefix(&comp.mpn).parse().ok(),
        description: comp.description.clone(),
        datasheet_url: comp.datasheet_url.clone(),
        footprint: comp.kicad_footprint.clone().or(comp.package.clone()),
        manufacturer: Some(comp.manufacturer.clone()),
        package: comp.package.clone(),
        pins: sym_pins,
    };

    let content = kicad_symgen::symbol::sexpr::generate_symbol_lib(
        std::slice::from_ref(&spec),
        kicad_symgen::model::KicadVersion::from_u8(8),
    );

    std::fs::write(output_path, &content)?;

    Ok(format!(
        "Generated symbol {} ({} pins) → {}",
        spec.mpn,
        spec.pins.len(),
        output_path
    ))
}

/// Infer electrical type from pin name.
fn infer_electrical_type_from_name(pin_name: &str) -> kicad_symgen::model::ElectricalType {
    use kicad_symgen::model::ElectricalType;
    let upper = pin_name.to_uppercase();
    if upper.starts_with("VCC")
        || upper.starts_with("VDD")
        || upper.starts_with("VIN")
        || upper == "V+"
        || upper == "AVCC"
        || upper == "DVCC"
        || upper == "VREF"
        || upper.starts_with("3V")
        || upper.starts_with("5V")
        || upper.starts_with("1V")
    {
        ElectricalType::PowerIn
    } else if upper == "GND"
        || upper == "VSS"
        || upper == "AGND"
        || upper == "DGND"
        || upper == "PGND"
        || upper == "SGND"
        || upper == "EP"
        || upper == "PAD"
    {
        ElectricalType::PowerIn
    } else if upper.starts_with("TX")
        || upper.starts_with("MOSI")
        || upper.starts_with("SDO")
        || upper.starts_with("SCL")
    {
        ElectricalType::Output
    } else if upper.starts_with("RX") || upper.starts_with("MISO") || upper.starts_with("SDI") {
        ElectricalType::Input
    } else if upper.starts_with("GPIO") || upper.starts_with("IO") || upper.starts_with("SDA") {
        ElectricalType::Bidirectional
    } else if upper == "NC" || upper == "N/C" || upper == "DNC" {
        ElectricalType::NoConnect
    } else {
        ElectricalType::Passive
    }
}
