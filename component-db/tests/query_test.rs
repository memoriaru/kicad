use component_db::*;
use pretty_assertions::assert_eq;

/// Helper: populate a database with test data for querying
fn seed_db() -> ComponentDb {
    let db = ComponentDb::open_in_memory().unwrap();

    // Categories
    let cat_cap = db.insert_category(&Category {
        id: None, name: "Capacitor".to_string(), parent_id: None, description: None,
    }).unwrap();
    let cat_res = db.insert_category(&Category {
        id: None, name: "Resistor".to_string(), parent_id: None, description: None,
    }).unwrap();
    let cat_mcu = db.insert_category(&Category {
        id: None, name: "MCU".to_string(), parent_id: None, description: None,
    }).unwrap();
    let cat_dcdc = db.insert_category(&Category {
        id: None, name: "DCDC_IC".to_string(), parent_id: None, description: None,
    }).unwrap();

    // Capacitors
    for (mpn, value_nf, voltage, pkg) in [
        ("GRM155R71C104KA88", 100e-9, 16.0, "0402"),
        ("GRM188R71H104KA93", 100e-9, 50.0, "0603"),
        ("GRM21BR71C105KA01", 1e-6, 16.0, "0805"),
        ("CL05B103KB5NNNC", 10e-9, 50.0, "0402"),
    ] {
        let id = db.insert_component(&Component {
            id: None, mpn: mpn.to_string(), manufacturer: "Murata".to_string(),
            category_id: cat_cap, description: Some(format!("{} {}V {}", value_nf, voltage, pkg)),
            package: Some(pkg.to_string()), lifecycle: "active".to_string(),
            datasheet_url: None, kicad_symbol: None, kicad_footprint: None,
        }).unwrap();
        db.insert_parameter(&Parameter {
            id: None, component_id: id, name: "capacitance".to_string(),
            value_numeric: Some(value_nf), value_text: None, unit: Some("F".to_string()),
            typical: true, condition: None, source_page: None,
        }).unwrap();
        db.insert_parameter(&Parameter {
            id: None, component_id: id, name: "voltage_rating".to_string(),
            value_numeric: Some(voltage), value_text: None, unit: Some("V".to_string()),
            typical: false, condition: None, source_page: None,
        }).unwrap();
    }

    // MCU
    let mcu_id = db.insert_component(&Component {
        id: None, mpn: "STM32F103C8T6".to_string(), manufacturer: "ST".to_string(),
        category_id: cat_mcu, description: Some("ARM Cortex-M3 72MHz 64KB Flash".to_string()),
        package: Some("LQFP-48".to_string()), lifecycle: "active".to_string(),
        datasheet_url: None, kicad_symbol: None, kicad_footprint: None,
    }).unwrap();
    db.insert_parameter(&Parameter {
        id: None, component_id: mcu_id, name: "clock_max".to_string(),
        value_numeric: Some(72e6), value_text: None, unit: Some("Hz".to_string()),
        typical: false, condition: None, source_page: None,
    }).unwrap();
    db.insert_parameter(&Parameter {
        id: None, component_id: mcu_id, name: "interfaces".to_string(),
        value_numeric: None, value_text: Some("SPI, I2C, USART, USB, CAN".to_string()),
        unit: None, typical: false, condition: None, source_page: None,
    }).unwrap();

    // Supply info for first cap
    db.insert_supply_info(&SupplyInfo {
        id: None, component_id: 1, supplier: "LCSC".to_string(),
        sku: Some("C15195".to_string()), price_breaks: Some("[[100, 0.02]]".to_string()),
        stock: Some(50000), lead_time_days: Some(3), moq: Some(1),
    }).unwrap();

    db
}

#[test]
fn test_query_by_category() {
    let db = seed_db();
    let caps = db.query_components_by_category("Capacitor").unwrap();
    assert_eq!(caps.len(), 4);
    assert!(caps.iter().all(|c| c.mpn.starts_with("GRM") || c.mpn.starts_with("CL")));
}

#[test]
fn test_query_by_parameter_range() {
    let db = seed_db();
    // Find components with capacitance >= 100nF (100nF caps + 1uF cap)
    let results = db.query_by_parameter_range("capacitance", Some(100e-9), None).unwrap();
    assert!(results.len() >= 3, "Should find caps >= 100nF, got {}", results.len());

    // Cross-filter: of those, which have voltage >= 50V
    let high_v: Vec<_> = results.iter().filter(|c| {
        if let Ok(params) = db.get_parameters(c.id.unwrap()) {
            params.iter().any(|p| p.name == "voltage_rating" && p.value_numeric >= Some(50.0))
        } else { false }
    }).collect();
    assert_eq!(high_v.len(), 1, "Only GRM188 (100nF, 50V) matches both filters");
}

#[test]
fn test_query_by_parameter_exact() {
    let db = seed_db();
    let results = db.query_by_parameter_exact("Capacitor", "voltage_rating", 16.0).unwrap();
    assert_eq!(results.len(), 2); // GRM155 (16V) + GRM21B (16V)
}

#[test]
fn test_query_by_text_param() {
    let db = seed_db();
    let results = db.query_by_text_parameter("interfaces", "I2C").unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].mpn, "STM32F103C8T6");
}

#[test]
fn test_query_by_supply_stock() {
    let db = seed_db();
    let results = db.query_in_stock().unwrap();
    assert!(results.len() >= 1);
    assert!(results.iter().any(|c| c.mpn == "GRM155R71C104KA88"));
}

#[test]
fn test_query_component_with_pins() {
    let db = ComponentDb::open_in_memory().unwrap();
    let cat = db.insert_category(&Category {
        id: None, name: "MCU".to_string(), parent_id: None, description: None,
    }).unwrap();
    let id = db.insert_component(&Component {
        id: None, mpn: "TEST".to_string(), manufacturer: "M".to_string(),
        category_id: cat, description: None, package: None, lifecycle: "active".to_string(),
        datasheet_url: None, kicad_symbol: None, kicad_footprint: None,
    }).unwrap();
    db.insert_pin(&Pin {
        id: None, component_id: id, pin_number: "1".to_string(), pin_name: "VCC".to_string(),
        pin_group: None, electrical_type: Some("power_in".to_string()),
        alt_functions: None, description: None,
    }).unwrap();

    let (comp, pins) = db.get_component_with_pins(id).unwrap().unwrap();
    assert_eq!(comp.mpn, "TEST");
    assert_eq!(pins.len(), 1);
    assert_eq!(pins[0].pin_name, "VCC");
}

#[test]
fn test_query_component_with_params() {
    let db = seed_db();
    let (comp, params) = db.get_component_with_params(5).unwrap().unwrap(); // MCU id=5
    assert_eq!(comp.mpn, "STM32F103C8T6");
    assert!(params.len() >= 2);
    assert!(params.iter().any(|p| p.name == "clock_max"));
}

#[test]
fn test_full_text_search() {
    let db = seed_db();
    let results = db.search("Cortex").unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].mpn, "STM32F103C8T6");

    let results2 = db.search("0402").unwrap();
    assert!(results2.len() >= 2);
}

#[test]
fn test_query_by_multiple_params() {
    let db = seed_db();
    // Find caps: capacitance >= 100nF AND voltage_rating >= 50V
    let results = db.query_by_multiple_params(&[
        ("capacitance", Some(100e-9), None),
        ("voltage_rating", Some(50.0), None),
    ]).unwrap();
    assert_eq!(results.len(), 1); // Only GRM188R71H104KA93 (100nF, 50V)
    assert_eq!(results[0].mpn, "GRM188R71H104KA93");
}
