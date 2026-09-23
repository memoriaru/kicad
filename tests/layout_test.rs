//! Board-level auto-layout integration test

use std::path::PathBuf;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

#[test]
fn test_battery_power_board_auto_layout() {
    let json5_path = fixtures_dir().join("battery-power-board-auto-layout.json5");
    let json5 = std::fs::read_to_string(&json5_path)
        .expect("battery-power-board-auto-layout.json5 not found");

    let mut sch = kicad_json5::parse_json5(&json5).expect("Failed to parse JSON5");

    // Verify initial: all positions are zero or unset
    let all_zero = sch.components.iter().all(|c| c.position.0 == 0.0 && c.position.1 == 0.0);
    assert!(all_zero, "All components should start at (0,0)");

    // Apply auto-layout
    let layout = kicad_json5::layout::apply_to(&mut sch);

    // Verify all components got positions
    for comp in &sch.components {
        assert!(
            comp.position.0 != 0.0 || comp.position.1 != 0.0,
            "{} should have a non-zero position after layout, got ({}, {})",
            comp.reference, comp.position.0, comp.position.1
        );
    }

    // Verify power flow direction: J1.x < U1.x (input before processing)
    let j1 = sch.components.iter().find(|c| c.reference == "J1").expect("J1 not found");
    let u1 = sch.components.iter().find(|c| c.reference == "U1").expect("U1 not found");

    assert!(
        j1.position.0 < u1.position.0,
        "Input connector J1 ({}) should be left of IC U1 ({})",
        j1.position.0, u1.position.0
    );

    // Verify grid alignment
    for comp in &sch.components {
        let (x, y, _) = comp.position;
        let x_rem = (x % 2.54).abs();
        let y_rem = (y % 2.54).abs();
        assert!(
            x_rem < 0.01 || x_rem > 2.53,
            "{} x={} not grid-aligned (rem={})",
            comp.reference, x, x_rem
        );
        assert!(
            y_rem < 0.01 || y_rem > 2.53,
            "{} y={} not grid-aligned (rem={})",
            comp.reference, y, y_rem
        );
    }

    // Verify no overlaps (basic check: no two components at same position)
    for i in 0..sch.components.len() {
        for j in (i + 1)..sch.components.len() {
            let ci = &sch.components[i];
            let cj = &sch.components[j];
            let dist = ((ci.position.0 - cj.position.0).powi(2)
                + (ci.position.1 - cj.position.1).powi(2))
                .sqrt();
            assert!(
                dist > 2.0,
                "{} and {} overlap: dist={:.2} ({:.1},{:.1}) vs ({:.1},{:.1})",
                ci.reference, cj.reference, dist,
                ci.position.0, ci.position.1, cj.position.0, cj.position.1
            );
        }
    }

    // Generate S-expression and write .kicad_sch to the system temp dir for
    // visual inspection (never into a shared fixture directory)
    let mut gen = kicad_json5::SexprGenerator::new();
    let sexpr = gen.generate(&sch).expect("Failed to generate S-expression");
    let mut output = sexpr;
    let mut extra_text = String::new();
    // Divider lines
    for div in &layout.dividers {
        extra_text.push_str(&format!(
            "  (polyline (pts (xy {:.2} {:.2}) (xy {:.2} {:.2})) (stroke (width 0.254) (type dash)) (fill (type none)))\n",
            div.x1, div.y1, div.x2, div.y2
        ));
    }
    // Section functional labels
    for label in &layout.section_labels {
        extra_text.push_str(&format!(
            "  (text \"{}\" (at {:.2} {:.2} 0)\n    (effects (font (size 2.032 2.032) bold) (color 0 0 160 1)))\n",
            label.text, label.x, label.y
        ));
    }
    // Find the last ')' that closes the schematic and insert before it
    if let Some(pos) = output.rfind(')') {
        output.insert_str(pos, &format!("\n{}", extra_text));
    }
    let sch_path = std::env::temp_dir().join("battery-power-board-auto-test.kicad_sch");
    std::fs::write(&sch_path, &output).expect("Failed to write .kicad_sch");
}
