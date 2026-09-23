//! PCB round-trip element-count tests.
//!
//! The generator output must contain exactly one serialized element per IR
//! element — any drift (silent loss or duplication) fails here.

use std::fs;
use std::path::PathBuf;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn hq_demo_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("KICAD_JSON5_HQ_DEMO_DIR") {
        return PathBuf::from(dir);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("example_sch")
}

fn load_from(dir: &PathBuf, path: &str) -> String {
    let p = dir.join(path);
    assert!(p.exists(), "Fixture not found: {} (see tests/fixtures/README.md)", p.display());
    fs::read_to_string(&p).unwrap()
}

fn roundtrip_pcb(source: &str) {
    let board = kicad_json5::parse_board(source).unwrap();

    // Count parsed elements
    let fp_line_count = board.footprints.iter().map(|f| f.fp_lines.len()).sum::<usize>();
    let fp_arc_count = board.footprints.iter().map(|f| f.fp_arcs.len()).sum::<usize>();
    let fp_circle_count = board.footprints.iter().map(|f| f.fp_circles.len()).sum::<usize>();
    let fp_rect_count = board.footprints.iter().map(|f| f.fp_rects.len()).sum::<usize>();
    let fp_poly_count = board.footprints.iter().map(|f| f.fp_polys.len()).sum::<usize>();
    let pad_count = board.footprints.iter().map(|f| f.pads.len()).sum::<usize>();
    let filled_poly_count = board.zones.iter().map(|z| z.filled_polygons.len()).sum::<usize>();

    // Generate back
    let config = kicad_json5::BoardSexprConfig::default();
    let mut gen = kicad_json5::BoardSexprGenerator::with_config(config);
    let output = gen.generate(&board).unwrap();

    // Every IR element must be serialized exactly once. The generator emits
    // each element opener as its own line ("(segment\n" etc.), so anchor on
    // the trailing newline to avoid matching longer tags like (zone_connect).
    assert_eq!(output.matches("(footprint \"").count(), board.footprints.len(), "footprints");
    assert_eq!(output.matches("(segment\n").count(), board.segments.len(), "segments");
    assert_eq!(output.matches("(via\n").count(), board.vias.len(), "vias");
    assert_eq!(output.matches("(zone\n").count(), board.zones.len(), "zones");
    // nets: no strict count — the generator inlines net names on pads/segments
    // (HQ fork dialect) instead of a top-level table, so "(net " also matches
    // every pad reference.
    assert_eq!(output.matches("(pad \"").count(), pad_count, "pads");
    assert_eq!(output.matches("(fp_line\n").count(), fp_line_count, "fp_lines");
    assert_eq!(output.matches("(fp_arc\n").count(), fp_arc_count, "fp_arcs");
    assert_eq!(output.matches("(fp_circle\n").count(), fp_circle_count, "fp_circles");
    assert_eq!(output.matches("(fp_rect\n").count(), fp_rect_count, "fp_rects");
    assert_eq!(output.matches("(fp_poly\n").count(), fp_poly_count, "fp_polys");
    assert_eq!(output.matches("(filled_polygon\n").count(), filled_poly_count, "filled_polygons");

    // Line count sanity: regeneration should stay within 3x of the source
    let orig_lines = source.lines().count();
    let new_lines = output.lines().count();
    assert!(
        new_lines <= orig_lines * 3,
        "regenerated file exploded: {} -> {} lines",
        orig_lines, new_lines
    );
}

#[test]
fn test_fixture_boards_roundtrip() {
    for name in ["ch340g-usb-uart.kicad_pcb", "battery-power-board.kicad_pcb"] {
        roundtrip_pcb(&load_from(&fixtures_dir(), name));
    }
}

#[test]
#[ignore = "needs HQ EDA demo fixture tiny-scarab.kicad_pcb (see tests/fixtures/README.md)"]
fn test_tiny_scarab_roundtrip() {
    roundtrip_pcb(&load_from(&hq_demo_dir(), "tiny-scarab.kicad_pcb"));
}

// Parser semantics: board-level gr_poly must not appear in board.graphics —
// the gr_poly entries in real files live inside pad primitives.
#[test]
#[ignore = "needs HQ EDA demo fixture tiny-scarab.kicad_pcb (see tests/fixtures/README.md)"]
fn test_tiny_scarab_graphics_detail() {
    let source = load_from(&hq_demo_dir(), "tiny-scarab.kicad_pcb");
    let board = kicad_json5::parse_board(&source).unwrap();

    let poly_count = board.graphics.iter().filter(|g| {
        matches!(&g.kind, kicad_json5::ir::board::BoardGraphicKind::Poly { .. })
    }).count();

    assert!(board.graphics.len() > 100, "tiny-scarab is graphics-rich");
    assert_eq!(poly_count, 0, "Board-level gr_poly should be 0 (the ones in file are inside pad primitives)");
}
