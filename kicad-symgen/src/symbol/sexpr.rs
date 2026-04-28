use crate::model::*;
use crate::symbol::layout::{self, LayoutPin};

/// Generate a complete .kicad_sym file content for one or more symbols
pub fn generate_symbol_lib(specs: &[SymbolSpec], version: KicadVersion) -> String {
    let mut s = String::new();
    s.push_str(&format!("(kicad_symbol_lib\n"));
    s.push_str(&format!("  (version \"{}\")\n", version.sym_lib_version()));
    s.push_str("  (generator \"kicad-symgen\")\n");

    for spec in specs {
        s.push_str(&generate_symbol(spec, version));
    }

    s.push_str(")\n");
    s
}

fn generate_symbol(spec: &SymbolSpec, version: KicadVersion) -> String {
    let name = spec.mpn.replace('.', "_");
    let layout = layout::compute_layout(spec);
    let mut s = String::new();

    // Symbol header (strict field order per kicad-sym-design-notes.md)
    s.push_str(&format!("  (symbol \"{}\"\n", name));

    // pin_numbers
    if spec.pins.len() <= 2 && spec.pins.iter().all(|p| p.electrical_type == ElectricalType::Passive) {
        s.push_str("    (pin_numbers hide)\n");
    }

    // pin_names
    s.push_str("    (pin_names (offset 0.254))\n");

    // exclude_from_sim
    if version >= KicadVersion::V9 {
        s.push_str("    (exclude_from_sim no)\n");
    }

    // in_bom, on_board
    s.push_str("    (in_bom yes)\n");
    s.push_str("    (on_board yes)\n");

    if version >= KicadVersion::V10 {
        s.push_str("    (in_pos_files yes)\n");
        s.push_str("    (duplicate_pin_numbers_are_jumpers no)\n");
    }

    // Properties (Reference, Value, Footprint, Datasheet, Description)
    let ref_prefix = spec.reference_prefix();
    s.push_str(&format!(
        "    (property \"Reference\" \"{}?\" (at 0 2.54 0)\n", ref_prefix
    ));
    s.push_str("      (effects (font (size 1.27 1.27))))\n");

    s.push_str(&format!(
        "    (property \"Value\" \"{}\" (at 0 -2.54 0)\n", spec.mpn
    ));
    s.push_str("      (effects (font (size 1.27 1.27))))\n");

    s.push_str(&format!(
        "    (property \"Footprint\" \"{}\" (at 0 0 0)\n",
        spec.footprint.as_deref().unwrap_or("")
    ));
    if version >= KicadVersion::V9 {
        s.push_str("      (effects (font (size 1.27 1.27)) (hide yes))\n");
    } else {
        s.push_str("      (effects (font (size 1.27 1.27)) hide)\n");
    }
    s.push_str("    )\n");

    if let Some(ref url) = spec.datasheet_url {
        s.push_str(&format!(
            "    (property \"Datasheet\" \"{}\" (at 0 0 0)\n", url
        ));
        if version >= KicadVersion::V9 {
            s.push_str("      (effects (font (size 1.27 1.27)) (hide yes))\n");
        } else {
            s.push_str("      (effects (font (size 1.27 1.27)) hide)\n");
        }
        s.push_str("    )\n");
    }

    if let Some(ref desc) = spec.description {
        s.push_str(&format!(
            "    (property \"Description\" \"{}\" (at 0 0 0)\n", desc
        ));
        if version >= KicadVersion::V9 {
            s.push_str("      (effects (font (size 1.27 1.27)) (hide yes))\n");
        } else {
            s.push_str("      (effects (font (size 1.27 1.27)) hide)\n");
        }
        s.push_str("    )\n");
    }

    // Unit 0_1: body graphics
    s.push_str(&format!("    (symbol \"{}_0_1\"\n", name));
    s.push_str(&format!(
        "      (rectangle (start {} {}) (end {} {})\n",
        -BODY_HALF_WIDTH_F,
        layout.body_height / 2.0,
        BODY_HALF_WIDTH_F,
        -(layout.body_height / 2.0)
    ));
    s.push_str("        (stroke (width 0.254) (type default))\n");
    s.push_str("        (fill (type background)))\n");
    s.push_str("    )\n");

    // Unit 1_1: pins
    if !layout.pins.is_empty() {
        s.push_str(&format!("    (symbol \"{}_1_1\"\n", name));
        for lp in &layout.pins {
            let pin = &spec.pins[lp.index];
            s.push_str(&gen_pin_sexp(pin, lp, version));
        }
        s.push_str("    )\n");
    }

    s.push_str("  )\n");
    s
}

const BODY_HALF_WIDTH_F: f64 = 5.08;

fn gen_pin_sexp(pin: &SymbolPin, lp: &LayoutPin, _version: KicadVersion) -> String {
    let mut s = String::new();
    let etype = pin.electrical_type.to_kicad_keyword();

    s.push_str(&format!(
        "      (pin {} line (at {} {}) (length 2.54)\n",
        etype, fmt_f(lp.x), fmt_f(lp.y)
    ));
    s.push_str(&format!(
        "        (name \"{}\" (effects (font (size 1.27 1.27))))\n",
        pin.name
    ));
    s.push_str(&format!(
        "        (number \"{}\" (effects (font (size 1.27 1.27))))\n",
        pin.number
    ));
    s.push_str("      )\n");
    s
}

/// Format a float without unnecessary trailing zeros
fn fmt_f(v: f64) -> String {
    if v == v.trunc() {
        format!("{}", v as i64)
    } else {
        format!("{}", v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_pin(number: &str, name: &str, etype: ElectricalType) -> SymbolPin {
        SymbolPin {
            number: number.to_string(),
            name: name.to_string(),
            electrical_type: etype,
            pin_group: None,
            alt_functions: None,
            position: None,
        }
    }

    #[test]
    fn test_generate_basic_symbol() {
        let spec = SymbolSpec {
            mpn: "FP6277".to_string(),
            lib_name: "custom".to_string(),
            reference_prefix: Some("U".to_string()),
            description: Some("2A synchronous boost converter".to_string()),
            datasheet_url: None,
            footprint: Some("Package_TO_SOT_SMD:SOT-23-6".to_string()),
            manufacturer: None,
            package: None,
            pins: vec![
                make_pin("1", "LX", ElectricalType::PowerOut),
                make_pin("2", "GND", ElectricalType::PowerIn),
                make_pin("3", "EN", ElectricalType::Input),
                make_pin("4", "FB", ElectricalType::Input),
                make_pin("5", "VCC", ElectricalType::PowerIn),
                make_pin("6", "SW", ElectricalType::PowerOut),
            ],
        };

        let output = generate_symbol_lib(&[spec], KicadVersion::V8);

        assert!(output.starts_with("(kicad_symbol_lib"));
        assert!(output.contains("(version \"20231120\")"));
        assert!(output.contains("(generator \"kicad-symgen\")"));
        assert!(output.contains("(symbol \"FP6277\""));
        assert!(output.contains("(property \"Reference\" \"U?\""));
        assert!(output.contains("(property \"Value\" \"FP6277\""));
        assert!(output.contains("(property \"Footprint\" \"Package_TO_SOT_SMD:SOT-23-6\""));
        assert!(output.contains("pin power_in line"));
        assert!(output.contains("pin input line"));
        assert!(output.contains("pin power_out line"));
        assert!(output.contains("(name \"GND\""));
        assert!(output.contains("(name \"VCC\""));
        assert!(output.contains("(name \"EN\""));
        assert!(output.contains("(number \"1\""));
        assert!(output.contains("FP6277_0_1"));
        assert!(output.contains("FP6277_1_1"));
    }
}
