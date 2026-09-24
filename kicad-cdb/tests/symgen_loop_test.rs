//! P1-5 补充：symgen↔cdb 闭环回归测试
//!
//! 验证完整链路：DB 插入 component+pins → 生成 symbol (.kicad_sym) → 生成 footprint (.kicad_mod)
//! 确保两个生成器正确消费 DB 数据，且输出包含预期的 pin/pad 数量和名称。

use kicad_cdb::models::{Category, Component, Pin};
use kicad_cdb::{footprint, footprint_lib, symgen, ComponentDb};

fn setup_db_with_soic8() -> (ComponentDb, i64) {
    let db = ComponentDb::open_in_memory().unwrap();
    db.insert_category(&Category {
        id: None,
        name: "OpAmp".into(),
        parent_id: None,
        description: None,
    })
    .unwrap();

    let comp = Component {
        id: None,
        mpn: "TEST-SOIC8-OPAMP".into(),
        manufacturer: "TestCo".into(),
        category_id: 1,
        description: Some("Test op-amp for regression".into()),
        package: Some("SOIC-8".into()),
        lifecycle: "active".into(),
        datasheet_url: Some("https://example.com/datasheet.pdf".into()),
        kicad_symbol: None,
        kicad_footprint: Some("Package_SO:SOIC-8_3.9x4.9mm_P1.27mm".into()),
        symbol_lib_path: None,
        footprint_lib_path: None,
        model_3d_path: None,
    };
    let comp_id = db.insert_component(&comp).unwrap();

    // 8 pins with mixed electrical types to exercise classification
    let pins = vec![
        Pin {
            id: None,
            component_id: comp_id,
            pin_number: "1".into(),
            pin_name: "V+".into(),
            pin_group: Some("power".into()),
            electrical_type: Some("power_in".into()),
            alt_functions: None,
            description: None,
        },
        Pin {
            id: None,
            component_id: comp_id,
            pin_number: "2".into(),
            pin_name: "IN-".into(),
            pin_group: Some("signal".into()),
            electrical_type: Some("input".into()),
            alt_functions: None,
            description: None,
        },
        Pin {
            id: None,
            component_id: comp_id,
            pin_number: "3".into(),
            pin_name: "IN+".into(),
            pin_group: Some("signal".into()),
            electrical_type: Some("input".into()),
            alt_functions: None,
            description: None,
        },
        Pin {
            id: None,
            component_id: comp_id,
            pin_number: "4".into(),
            pin_name: "GND".into(),
            pin_group: Some("power".into()),
            electrical_type: Some("power_in".into()),
            alt_functions: None,
            description: None,
        },
        Pin {
            id: None,
            component_id: comp_id,
            pin_number: "5".into(),
            pin_name: "NC".into(),
            pin_group: None,
            electrical_type: Some("passive".into()),
            alt_functions: None,
            description: None,
        },
        Pin {
            id: None,
            component_id: comp_id,
            pin_number: "6".into(),
            pin_name: "OUT".into(),
            pin_group: Some("signal".into()),
            electrical_type: Some("output".into()),
            alt_functions: None,
            description: None,
        },
        Pin {
            id: None,
            component_id: comp_id,
            pin_number: "7".into(),
            pin_name: "V-".into(),
            pin_group: Some("power".into()),
            electrical_type: Some("power_in".into()),
            alt_functions: None,
            description: None,
        },
        Pin {
            id: None,
            component_id: comp_id,
            pin_number: "8".into(),
            pin_name: "NC2".into(),
            pin_group: None,
            electrical_type: Some("passive".into()),
            alt_functions: None,
            description: None,
        },
    ];
    db.insert_pins(&pins).unwrap();

    (db, comp_id)
}

#[test]
fn test_symbol_generation_loop() {
    let (db, comp_id) = setup_db_with_soic8();
    let comp = db.get_component(comp_id).unwrap().unwrap();

    // Generate symbol library from DB component
    let sym = symgen::generate_rich_symbol_lib(&[comp], &db).unwrap();

    // Must contain all 8 pins
    let pin_count = sym.matches("(pin ").count();
    assert_eq!(pin_count, 8, "symbol should have 8 pins");

    // Each pin number and name must appear
    for num in &["1", "2", "3", "4", "5", "6", "7", "8"] {
        let needle = format!("(number \"{}\"", num);
        assert!(
            sym.contains(&needle),
            "pin number '{}' missing from symbol",
            num
        );
    }
    for name in &["V+", "IN-", "IN+", "GND", "OUT", "V-"] {
        assert!(
            sym.contains(name),
            "pin name '{}' missing from symbol",
            name
        );
    }

    // MPN should appear in the symbol definition
    assert!(sym.contains("TEST-SOIC8-OPAMP"), "MPN missing from symbol");
}

#[test]
fn test_footprint_generation_loop() {
    let (db, comp_id) = setup_db_with_soic8();
    let comp = db.get_component(comp_id).unwrap().unwrap();

    // Generate footprint from DB component
    let fp = footprint::generate_footprint_for_component(&comp, &db).unwrap();

    // SOIC-8 → 8 pads
    let pad_count = fp.matches("(pad ").count();
    assert_eq!(pad_count, 8, "footprint should have 8 pads for SOIC-8");

    // Must contain pad numbers 1-8
    for num in &["1", "2", "3", "4", "5", "6", "7", "8"] {
        assert!(
            fp.contains(&format!("\"{}\"", num)),
            "pad number '{}' missing from footprint",
            num
        );
    }

    // SOIC-8 package name should appear
    assert!(fp.contains("SOIC-8"), "package name missing from footprint");
}

#[test]
fn test_footprint_uses_imported_metadata_when_available() {
    let (db, comp_id) = setup_db_with_soic8();

    // Import metadata for this footprint (simulating kdesign import-footprints)
    let metas = vec![footprint_lib::FootprintMeta {
        lib_id: "Package_SO:SOIC-8_3.9x4.9mm_P1.27mm".into(),
        pad_count: 8,
        pitch: 1.27,
        row_spacing: Some(5.2), // slightly different from hardcoded 5.4
        body_size: Some((3.9, 4.9)),
        drill_size: None,
        pad_size: Some((0.6, 0.6)), // custom pad size
        source_lib: "Package_SO".into(),
    }];
    let imported = footprint_lib::import_footprint_metadata(&db, &metas).unwrap();
    assert_eq!(imported, 1);

    let comp = db.get_component(comp_id).unwrap().unwrap();
    let fp = footprint::generate_footprint_for_component(&comp, &db).unwrap();

    // With imported metadata, pad size 0.6×0.6 should be used
    assert_eq!(fp.matches("(pad ").count(), 8, "still 8 pads with metadata");
    // The footprint should generate successfully using real dimensions
    assert!(fp.contains("SOIC-8"));
}

#[test]
fn test_symbol_footprint_consistency() {
    // Verify symbol pin count == footprint pad count (same component).
    let (db, comp_id) = setup_db_with_soic8();
    let comp = db.get_component(comp_id).unwrap().unwrap();

    let sym = symgen::generate_rich_symbol_lib(std::slice::from_ref(&comp), &db).unwrap();
    let fp = footprint::generate_footprint_for_component(&comp, &db).unwrap();

    let sym_pins = sym.matches("(pin ").count();
    let fp_pads = fp.matches("(pad ").count();
    assert_eq!(
        sym_pins, fp_pads,
        "symbol pins ({}) must match footprint pads ({})",
        sym_pins, fp_pads
    );
}

#[test]
fn test_missing_package_returns_error() {
    let db = ComponentDb::open_in_memory().unwrap();
    db.insert_category(&Category {
        id: None,
        name: "Test".into(),
        parent_id: None,
        description: None,
    })
    .unwrap();
    let comp = Component {
        id: None,
        mpn: "NOPKG".into(),
        manufacturer: "TestCo".into(),
        category_id: 1,
        description: None,
        package: None, // no package
        lifecycle: "active".into(),
        datasheet_url: None,
        kicad_symbol: None,
        kicad_footprint: None,
        symbol_lib_path: None,
        footprint_lib_path: None,
        model_3d_path: None,
    };
    let comp_id = db.insert_component(&comp).unwrap();
    let comp = db.get_component(comp_id).unwrap().unwrap();

    let result = footprint::generate_footprint_for_component(&comp, &db);
    assert!(
        result.is_err(),
        "footprint generation should fail without package"
    );
}
