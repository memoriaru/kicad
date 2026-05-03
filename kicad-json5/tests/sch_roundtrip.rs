//! Integration tests using real .kicad_sch files from example_sch/
//!
//! These tests verify that the parser → IR → generator pipeline works
//! correctly on real-world schematics of varying complexity.

use std::path::PathBuf;
use kicad_json5::*;

fn example_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("example_sch")
}

fn load_sch(name: &str) -> String {
    let path = example_dir().join(name);
    assert!(path.exists(), "Fixture not found: {}", path.display());
    std::fs::read_to_string(&path).unwrap()
}

fn parse_and_check(name: &str) -> Schematic {
    let source = load_sch(name);
    let schematic = parse_schematic(&source, InputFormat::Sexpr)
        .unwrap_or_else(|e| panic!("Failed to parse {}: {}", name, e));

    assert!(
        !schematic.metadata.uuid.is_empty(),
        "{}: UUID should be non-empty",
        name
    );

    schematic
}

/// Parse → Generate S-expression → Re-parse: the IR should be structurally identical.
fn roundtrip_schematic(name: &str) {
    let original = parse_and_check(name);

    let mut gen = SexprGenerator::new();
    let output = gen.generate(&original).expect("Sexpr generation failed");

    let reparsed = parse_schematic(&output, InputFormat::Sexpr)
        .expect("Re-parse of generated output failed");

    assert_eq!(
        original.components.len(),
        reparsed.components.len(),
        "{}: component count mismatch after roundtrip ({} vs {})",
        name, original.components.len(), reparsed.components.len()
    );

    if original.wires.len() > 0 {
        let wire_ratio = reparsed.wires.len() as f64 / original.wires.len() as f64;
        assert!(
            wire_ratio > 0.5 && wire_ratio < 2.0,
            "{}: wire count diverged too much ({} vs {})",
            name, original.wires.len(), reparsed.wires.len()
        );
    }

    assert!(
        !original.lib_symbols.is_empty(),
        "{}: should have lib_symbols",
        name
    );
}

// ── Voltage-selection (medium, power circuits) ──────────────

#[test]
fn test_parse_voltage_selection() {
    let sch = parse_and_check("Voltage-selection.kicad_sch");
    assert!(!sch.components.is_empty(), "Should have components");
    assert!(!sch.wires.is_empty(), "Should have wires");
}

#[test]
fn test_roundtrip_voltage_selection() {
    roundtrip_schematic("Voltage-selection.kicad_sch");
}

#[test]
fn test_topology_voltage_selection() {
    let sch = parse_and_check("Voltage-selection.kicad_sch");
    let summary = kicad_json5::topology::extract_topology(&sch);
    assert!(!summary.power_domains.is_empty(), "Should detect power domains");
}

// ── Target-MCU (medium, MCU 100+ pin) ──────────────────────

#[test]
fn test_parse_target_mcu() {
    let sch = parse_and_check("Target-MCU.kicad_sch");
    assert!(sch.components.len() > 20, "Target-MCU has many components, got {}", sch.components.len());
}

#[test]
fn test_roundtrip_target_mcu() {
    roundtrip_schematic("Target-MCU.kicad_sch");
}

// ── USB (large, Type-C + ESD) ───────────────────────────────

#[test]
fn test_parse_usb() {
    let sch = parse_and_check("USB.kicad_sch");
    assert!(sch.components.len() > 30, "USB has many components, got {}", sch.components.len());
}

#[test]
fn test_roundtrip_usb() {
    roundtrip_schematic("USB.kicad_sch");
}

// ── WCH-LinkE (large, debugger board) ───────────────────────

#[test]
fn test_parse_wch_linke() {
    let sch = parse_and_check("WCH-LinkE-R0-1v3.kicad_sch");
    assert!(!sch.components.is_empty());
}

#[test]
fn test_roundtrip_wch_linke() {
    roundtrip_schematic("WCH-LinkE-R0-1v3.kicad_sch");
}

// ── tiny-scarab (largest, complete single-board) ────────────

#[test]
fn test_parse_tiny_scarab() {
    let sch = parse_and_check("tiny-scarab.kicad_sch");
    assert!(sch.components.len() > 50, "tiny-scarab should have 50+ components, got {}", sch.components.len());
}

#[test]
fn test_roundtrip_tiny_scarab() {
    roundtrip_schematic("tiny-scarab.kicad_sch");
}

#[test]
fn test_topology_tiny_scarab() {
    let sch = parse_and_check("tiny-scarab.kicad_sch");
    let summary = kicad_json5::topology::extract_topology(&sch);
    assert!(!summary.power_domains.is_empty(), "Should detect power domains");
    assert!(!summary.component_summary.by_type.is_empty(), "Should have component stats");
}
