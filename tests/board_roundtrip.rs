//! Integration tests for Board IR round-trip:
//! .kicad_pcb → Board IR → JSON5 → Board IR
//! .kicad_pcb → Board IR → .kicad_pcb → Board IR

use std::path::PathBuf;
use kicad_json5::*;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Real-world boards from the HQ EDA online viewer demo set.
/// Not redistributable — see tests/fixtures/README.md. Tests using this
/// directory are `#[ignore]`d and run opt-in via `--ignored`.
fn hq_demo_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("KICAD_JSON5_HQ_DEMO_DIR") {
        return PathBuf::from(dir);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("example_sch")
}

fn load_from(dir: &PathBuf, name: &str) -> String {
    let path = dir.join(name);
    assert!(path.exists(), "Fixture not found: {} (see tests/fixtures/README.md)", path.display());
    std::fs::read_to_string(&path).unwrap()
}

fn load_fixture(name: &str) -> String {
    load_from(&fixtures_dir(), name)
}

fn parse_pcb(name: &str) -> ir::Board {
    let source = load_fixture(name);
    parse_board(&source)
        .unwrap_or_else(|e| panic!("Failed to parse {}: {}", name, e))
}

#[test]
fn test_parse_ch340g() {
    let board = parse_pcb("ch340g-usb-uart.kicad_pcb");
    assert!(!board.footprints.is_empty(), "should have footprints");
    assert!(!board.segments.is_empty(), "should have segments");
    assert!(!board.nets.is_empty(), "should have nets");
    assert!(!board.graphics.is_empty(), "should have graphics");
}

#[test]
fn test_parse_battery_power_board() {
    let board = parse_pcb("battery-power-board.kicad_pcb");
    assert!(!board.footprints.is_empty(), "should have footprints");
    assert!(!board.zones.is_empty(), "should have zones");
    assert!(!board.graphics.is_empty(), "should have graphics");
}

// Regression for zone filled_polygons lost through the JSON5 hop
// (generate_board_json5 historically omitted them; tiny-scarab's filled
// zones silently came back empty). Locked down with an inline board so it
// runs everywhere without external fixtures.
#[test]
fn test_zone_filled_polygons_json5_roundtrip() {
    let src = r#"(kicad_pcb (version "20240108") (generator "test")
	(zone (net 1) (net_name "GND") (layer "B.Cu") (hatch edge 0.508)
		(connect_pads (clearance 0.508))
		(min_thickness 0.254) (fill yes (thermal_gap 0.508) (thermal_bridge_width 0.508))
		(polygon (pts (xy 0 0) (xy 50 0) (xy 50 30) (xy 0 30)))
		(filled_polygon
			(layer "B.Cu")
			(pts (xy 1 1) (xy 49 1) (xy 49 29) (xy 1 29))
		)
	)
)"#;
    let board = parse_board(src).unwrap();
    assert_eq!(board.zones.len(), 1);
    assert_eq!(board.zones[0].filled_polygons.len(), 1, "parse: filled_polygon must be read");

    let json5 = generate_board_json5(&board).unwrap();
    assert!(json5.contains("filled_polygons"), "export: zone must carry filled_polygons");
    let re = parse_board_json5(&json5).unwrap();
    assert_eq!(re.zones[0].filled_polygons.len(), 1, "roundtrip: filled_polygons lost via json5");
    assert_eq!(re.zones[0].filled_polygons[0].layer, "B.Cu");
    assert_eq!(re.zones[0].filled_polygons[0].points, vec![(1.0, 1.0), (49.0, 1.0), (49.0, 29.0), (1.0, 29.0)]);
}

#[test]
fn test_board_json5_roundtrip() {
    let board = parse_pcb("battery-power-board.kicad_pcb");

    // Board IR → JSON5
    let json5 = generate_board_json5(&board)
        .expect("generate_board_json5 failed");

    // JSON5 → Board IR
    let board2 = parse_board_json5(&json5)
        .expect("parse_board_json5 failed");

    // Verify structural equality
    assert_eq!(board.footprints.len(), board2.footprints.len(), "footprint count");
    assert_eq!(board.segments.len(), board2.segments.len(), "segment count");
    assert_eq!(board.vias.len(), board2.vias.len(), "via count");
    assert_eq!(board.zones.len(), board2.zones.len(), "zone count");
    assert_eq!(board.graphics.len(), board2.graphics.len(), "graphics count");
    assert_eq!(board.nets.len(), board2.nets.len(), "net count");

    // Spot-check first footprint
    let fp1 = &board.footprints[0];
    let fp2 = &board2.footprints[0];
    assert_eq!(fp1.reference, fp2.reference);
    assert_eq!(fp1.lib_id, fp2.lib_id);
    assert_eq!(fp1.pads.len(), fp2.pads.len());

    // Verify zones with filled_polygons
    for (i, (z1, z2)) in board.zones.iter().zip(board2.zones.iter()).enumerate() {
        assert_eq!(z1.net, z2.net, "zone[{}] net", i);
        assert_eq!(z1.filled_polygons.len(), z2.filled_polygons.len(),
            "zone[{}] filled_polygons count", i);
    }
}

#[test]
fn test_board_sexpr_roundtrip() {
    let board = parse_pcb("battery-power-board.kicad_pcb");

    // Board IR → S-expression
    let mut gen = codegen::BoardSexprGenerator::new();
    let sexpr = gen.generate(&board)
        .expect("board sexpr generation failed");

    // S-expression → Board IR
    let board2 = parse_board(&sexpr)
        .expect("re-parse board from generated sexpr failed");

    // Verify structural equality
    assert_eq!(board.footprints.len(), board2.footprints.len(), "footprint count");
    assert_eq!(board.segments.len(), board2.segments.len(), "segment count");
    assert_eq!(board.vias.len(), board2.vias.len(), "via count");
    assert_eq!(board.zones.len(), board2.zones.len(), "zone count");
    assert_eq!(board.graphics.len(), board2.graphics.len(), "graphics count");
    // The generator inlines net names on elements (HQ fork dialect) and does
    // not emit a top-level net table, so nets never referenced by any element
    // (the unnamed net-0 placeholder and dangling nets) do not survive.
    // What must survive is every net an element references — i.e. the
    // electrical connectivity.
    let refs = |b: &ir::Board| {
        let mut s = std::collections::BTreeSet::new();
        for fp in &b.footprints {
            for p in &fp.pads {
                if let Some(n) = p.net { s.insert(n); }
            }
        }
        for seg in &b.segments { s.insert(seg.net); }
        for via in &b.vias { s.insert(via.net); }
        for z in &b.zones { s.insert(z.net); }
        s
    };
    assert_eq!(refs(&board), refs(&board2), "referenced net id set (electrical connectivity)");
    let named = |b: &ir::Board| b.nets.iter().filter(|n| !n.name.is_empty()).count();
    assert_eq!(named(&board), named(&board2) + 1, "exactly the one dangling net (GATE_Q3) may vanish");
}

#[test]
fn test_full_pipeline() {
    // .kicad_pcb → Board IR → JSON5 → Board IR → .kicad_pcb → Board IR
    let board1 = parse_pcb("ch340g-usb-uart.kicad_pcb");

    // → JSON5 → back
    let json5 = generate_board_json5(&board1).unwrap();
    let board2 = parse_board_json5(&json5).unwrap();

    // → S-expression → back
    let mut gen = codegen::BoardSexprGenerator::new();
    let sexpr = gen.generate(&board2).unwrap();
    let board3 = parse_board(&sexpr).unwrap();

    // All three should have the same structural counts
    assert_eq!(board1.footprints.len(), board2.footprints.len());
    assert_eq!(board2.footprints.len(), board3.footprints.len());
    assert_eq!(board1.segments.len(), board3.segments.len());
    assert_eq!(board1.zones.len(), board3.zones.len());
}

// Graphics-rich board must survive pcb → json5 → pcb without graphic loss.
#[test]
fn test_graphics_roundtrip_via_json5() {
    let src = load_fixture("battery-power-board.kicad_pcb");
    let board = parse_board(&src).unwrap();

    let fp_lines = board.footprints.iter().map(|f| f.fp_lines.len()).sum::<usize>();
    let fp_texts = board.footprints.iter().map(|f| f.fp_texts.len()).sum::<usize>();
    let gr = board.graphics.len();
    assert!(gr > 0, "fixture must have board graphics");

    let json5 = generate_board_json5(&board).unwrap();
    let re = parse_board_json5(&json5).unwrap();
    assert_eq!(re.graphics.len(), gr, "board graphics lost via json5 (Edge.Cuts!)");
    assert_eq!(re.footprints.iter().map(|f| f.fp_lines.len()).sum::<usize>(), fp_lines);
    assert_eq!(re.footprints.iter().map(|f| f.fp_texts.len()).sum::<usize>(), fp_texts);

    // second hop: json5 → .kicad_pcb
    let mut gen = codegen::BoardSexprGenerator::new();
    let sexpr = gen.generate(&re).unwrap();
    let final_board = parse_board(&sexpr).unwrap();
    assert_eq!(final_board.graphics.len(), gr, "board graphics lost via sexpr hop");
}

// ===== HQ EDA demo boards (opt-in, not redistributable) =====

#[test]
#[ignore = "needs HQ EDA demo fixture tiny-scarab.kicad_pcb (see tests/fixtures/README.md)"]
fn test_parse_tiny_scarab() {
    let src = load_from(&hq_demo_dir(), "tiny-scarab.kicad_pcb");
    let board = parse_board(&src).unwrap();
    assert!(!board.footprints.is_empty(), "should have footprints");
    assert!(!board.segments.is_empty(), "should have segments");
    assert!(!board.vias.is_empty(), "should have vias");
    assert!(!board.zones.is_empty(), "should have zones");
    assert!(!board.graphics.is_empty(), "should have graphics");
    assert!(!board.nets.is_empty(), "should have nets");
}

#[test]
#[ignore = "needs HQ EDA demo fixture tiny-scarab.kicad_pcb (see tests/fixtures/README.md)"]
fn test_tiny_scarab_json5_roundtrip() {
    let src = load_from(&hq_demo_dir(), "tiny-scarab.kicad_pcb");
    let board = parse_board(&src).unwrap();

    let json5 = generate_board_json5(&board).unwrap();
    let board2 = parse_board_json5(&json5).unwrap();

    assert_eq!(board.footprints.len(), board2.footprints.len(), "footprint count");
    assert_eq!(board.zones.len(), board2.zones.len(), "zone count");
    for (i, (z1, z2)) in board.zones.iter().zip(board2.zones.iter()).enumerate() {
        assert_eq!(z1.filled_polygons.len(), z2.filled_polygons.len(),
            "zone[{}] filled_polygons count", i);
    }
    assert_eq!(board.graphics.len(), board2.graphics.len(), "graphics count");
}

// ===== Task 4 Phase 2: footprint (model ...) 往返保真 =====

// offset 漏读 bug 回归: KiCad 实际写法把数值嵌在内层 `(xyz ...)` 列表里,
// 旧实现直接把 `(offset` 的第 1 个参数当数字读 → 恒 None → 静默丢成默认值 0。
#[test]
fn test_model_offset_xyz_not_swallowed() {
    let src = r#"(kicad_pcb (version "20240108") (generator "test")
	(footprint "Test:P" (layer "F.Cu") (at 0 0)
		(model "${KIPRJMOD}/3d/REF.wrl"
			(offset (xyz 0 0 1.25))
			(scale (xyz 1 1 1))
			(rotate (xyz 0 0 0))
		)
	)
)"#;
    let board = parse_board(src).unwrap();
    let m = &board.footprints[0].models[0];
    assert_eq!(m.offset, (0.0, 0.0, 1.25),
        "offset z 漏读(回归): 内层 (xyz) 列表未展开, 数值被默认值吞掉");
    assert_eq!(m.path, "${KIPRJMOD}/3d/REF.wrl");
}

// model 字段解析三形态: 全字段 / 仅 path / ${KIPRJMOD} 变量路径 (+ 多 model 共存)。
// 缺省子字段按 KiCad 默认: offset (0,0,0) / scale (1,1,1) / rotate (0,0,0)。
#[test]
fn test_parse_model_three_forms() {
    let src = r#"(kicad_pcb (version "20240108") (generator "test")
	(footprint "Test:Full" (layer "F.Cu") (at 0 0)
		(model "${KIPRJMOD}/3d/U1.wrl"
			(offset (xyz 0.5 -1 2.25))
			(scale (xyz 1 0.5 1))
			(rotate (xyz -90 0 45))
		)
	)
	(footprint "Test:PathOnly" (layer "F.Cu") (at 5 0)
		(model "Package_TO_SOT_SMD.3dshapes/SOT-23.wrl")
	)
	(footprint "Test:Var" (layer "F.Cu") (at 10 0)
		(model "${KIPRJMOD}/3d/带引号\"x.wrl"
			(offset (xyz 0 0 0.8))
		)
	)
	(footprint "Test:Multi" (layer "F.Cu") (at 15 0)
		(model "${KIPRJMOD}/3d/A.wrl" (offset (xyz 0 0 1)))
		(model "${KIPRJMOD}/3d/B.wrl" (offset (xyz 0 0 2)))
	)
)"#;
    let board = parse_board(src).unwrap();

    let full = &board.footprints.iter().find(|f| f.lib_id == "Test:Full").unwrap().models[0];
    assert_eq!(full.offset, (0.5, -1.0, 2.25));
    assert_eq!(full.scale, (1.0, 0.5, 1.0));
    assert_eq!(full.rotate, (-90.0, 0.0, 45.0));

    // 仅 path: 子字段缺省 → KiCad 默认值 (不丢 model 本身)
    let po = &board.footprints.iter().find(|f| f.lib_id == "Test:PathOnly").unwrap().models[0];
    assert_eq!(po.offset, (0.0, 0.0, 0.0));
    assert_eq!(po.scale, (1.0, 1.0, 1.0));
    assert_eq!(po.rotate, (0.0, 0.0, 0.0));

    // ${} 变量与引号转义在路径里原样保真
    let var = &board.footprints.iter().find(|f| f.lib_id == "Test:Var").unwrap().models[0];
    assert_eq!(var.path, "${KIPRJMOD}/3d/带引号\"x.wrl");
    assert_eq!(var.offset, (0.0, 0.0, 0.8));

    // 同一 footprint 多个 model
    let multi = &board.footprints.iter().find(|f| f.lib_id == "Test:Multi").unwrap().models;
    assert_eq!(multi.len(), 2);
    assert_eq!(multi[0].offset.2, 1.0);
    assert_eq!(multi[1].offset.2, 2.0);
}

// 往返: 含 model 的板 json5→sexpr→json5 二次 convert 必须位级幂等,
// model 行保留、路径与 xyz 数值不漂。
#[test]
fn test_model_roundtrip_double_hop_idempotent() {
    let src = r#"(kicad_pcb (version "20240108") (generator "test")
	(footprint "Test:R" (layer "F.Cu") (uuid "11111111-2222-3333-4444-555555555555") (at 1 2)
		(model "${KIPRJMOD}/3d/R1.wrl"
			(offset (xyz 0 0 1.235))
			(scale (xyz 1 1 1))
			(rotate (xyz 0 0 0))
		)
	)
	(footprint "Test:Bare" (layer "F.Cu") (uuid "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee") (at 3 4)
		(model "x.3dshapes/only-path.wrl")
	)
)"#;
    let board1 = parse_board(src).unwrap();

    // hop1: sexpr → json5 → sexpr
    let j5a = generate_board_json5(&board1).unwrap();
    assert!(j5a.contains("models: ["), "json5 导出必须带 models 数组");
    let b2 = parse_board_json5(&j5a).unwrap();
    let sexpr1 = codegen::BoardSexprGenerator::new().generate(&b2).unwrap();

    // hop2: sexpr → json5 → sexpr
    let board2 = parse_board(&sexpr1).unwrap();
    let j5b = generate_board_json5(&board2).unwrap();
    let b3 = parse_board_json5(&j5b).unwrap();
    let sexpr2 = codegen::BoardSexprGenerator::new().generate(&b3).unwrap();

    // 二次 convert 位级一致
    assert_eq!(sexpr1, sexpr2, "二次 convert 必须位级幂等");

    // model 行数不变 + 路径与 xyz 逐一相等
    let n_src = src.matches("(model ").count();
    assert_eq!(sexpr1.matches("(model ").count(), n_src, "model 行数在往返后必须不变");
    let expect = [
        ("${KIPRJMOD}/3d/R1.wrl", (0.0f64, 0.0f64, 1.235f64), (1.0, 1.0, 1.0), (0.0, 0.0, 0.0)),
        ("x.3dshapes/only-path.wrl", (0.0, 0.0, 0.0), (1.0, 1.0, 1.0), (0.0, 0.0, 0.0)),
    ];
    for (fp, e) in board2.footprints.iter().zip(expect.iter()) {
        assert_eq!(fp.models.len(), 1, "{} 应保留 1 个 model", fp.lib_id);
        let m = &fp.models[0];
        assert_eq!(m.path, e.0, "路径保真 ({}): {}", fp.lib_id, m.path);
        assert_eq!(m.offset, e.1, "{} offset 数值漂移", fp.lib_id);
        assert_eq!(m.scale, e.2);
        assert_eq!(m.rotate, e.3);
    }
}
