use crate::model::*;
use crate::fmt;
use crate::symbol::layout::{self, LayoutPin};

/// Generate a complete .kicad_sym file content for one or more symbols
pub fn generate_symbol_lib(specs: &[SymbolSpec], version: KicadVersion) -> String {
    let mut s = String::new();
    s.push_str("(kicad_symbol_lib\n");
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

    s.push_str(&format!("  (symbol \"{}\"\n", name));

    if spec.pins.len() <= 2 && spec.pins.iter().all(|p| p.electrical_type == ElectricalType::Passive) {
        s.push_str("    (pin_numbers hide)\n");
    }

    s.push_str("    (pin_names (offset 0.254))\n");

    if version >= KicadVersion::V9 {
        s.push_str("    (exclude_from_sim no)\n");
    }

    s.push_str("    (in_bom yes)\n");
    s.push_str("    (on_board yes)\n");

    if version >= KicadVersion::V10 {
        s.push_str("    (in_pos_files yes)\n");
        s.push_str("    (duplicate_pin_numbers_are_jumpers no)\n");
    }

    // Properties
    s.push_str(&format!(
        "    (property \"Reference\" \"{}?\" (at 0 2.54 0)\n", spec.reference_prefix()
    ));
    s.push_str("      (effects (font (size 1.27 1.27))))\n");

    s.push_str(&format!(
        "    (property \"Value\" \"{}\" (at 0 -2.54 0)\n", spec.mpn
    ));
    s.push_str("      (effects (font (size 1.27 1.27))))\n");

    let hide_effects = if version >= KicadVersion::V9 {
        "(effects (font (size 1.27 1.27)) (hide yes))"
    } else {
        "(effects (font (size 1.27 1.27)) hide)"
    };

    s.push_str(&format!(
        "    (property \"Footprint\" \"{}\" (at 0 0 0)\n      {})\n    )\n",
        spec.footprint.as_deref().unwrap_or(""), hide_effects
    ));

    if let Some(ref url) = spec.datasheet_url {
        s.push_str(&format!(
            "    (property \"Datasheet\" \"{}\" (at 0 0 0)\n      {})\n    )\n", url, hide_effects
        ));
    }

    if let Some(ref desc) = spec.description {
        s.push_str(&format!(
            "    (property \"Description\" \"{}\" (at 0 0 0)\n      {})\n    )\n", desc, hide_effects
        ));
    }

    // Unit 0_1: body graphics
    s.push_str(&format!("    (symbol \"{}_0_1\"\n", name));
    s.push_str(&format!(
        "      (rectangle (start {} {}) (end {} {})\n",
        fmt::fmt_f(-fmt::BODY_HALF_WIDTH),
        fmt::fmt_f(layout.body_height / 2.0),
        fmt::fmt_f(fmt::BODY_HALF_WIDTH),
        fmt::fmt_f(-(layout.body_height / 2.0))
    ));
    s.push_str("        (stroke (width 0.254) (type default))\n");
    s.push_str("        (fill (type background)))\n");
    s.push_str("    )\n");

    // Unit 1_1: pins
    if !layout.pins.is_empty() {
        s.push_str(&format!("    (symbol \"{}_1_1\"\n", name));
        for lp in &layout.pins {
            let pin = &spec.pins[lp.index];
            s.push_str(&gen_pin_sexp(pin, lp));
        }
        s.push_str("    )\n");
    }

    s.push_str("  )\n");
    s
}

fn gen_pin_sexp(pin: &SymbolPin, lp: &LayoutPin) -> String {
    format!(
        "      (pin {} line (at {} {}) (length 2.54)\n        (name \"{}\" (effects (font (size 1.27 1.27))))\n        (number \"{}\" (effects (font (size 1.27 1.27))))\n      )\n",
        pin.electrical_type.to_kicad_keyword(),
        fmt::fmt_f(lp.x), fmt::fmt_f(lp.y),
        pin.name, pin.number
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fmt::test_helpers::*;

    #[test]
    fn test_generate_basic_symbol() {
        let spec = SymbolSpec {
            mpn: "FP6277".to_string(),
            description: Some("2A synchronous boost converter".to_string()),
            footprint: Some("Package_TO_SOT_SMD:SOT-23-6".to_string()),
            pins: vec![
                make_pin("1", "LX", ElectricalType::PowerOut),
                make_pin("2", "GND", ElectricalType::PowerIn),
                make_pin("3", "EN", ElectricalType::Input),
                make_pin("4", "FB", ElectricalType::Input),
                make_pin("5", "VCC", ElectricalType::PowerIn),
                make_pin("6", "SW", ElectricalType::PowerOut),
            ],
            ..make_spec("FP6277", vec![])
        };
        // Override pins since make_spec takes pins
        let spec = SymbolSpec { pins: vec![
            make_pin("1", "LX", ElectricalType::PowerOut),
            make_pin("2", "GND", ElectricalType::PowerIn),
            make_pin("3", "EN", ElectricalType::Input),
            make_pin("4", "FB", ElectricalType::Input),
            make_pin("5", "VCC", ElectricalType::PowerIn),
            make_pin("6", "SW", ElectricalType::PowerOut),
        ], ..spec };

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
        assert!(output.contains("FP6277_0_1"));
        assert!(output.contains("FP6277_1_1"));
    }
}
