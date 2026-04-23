use component_db::*;
use pretty_assertions::assert_eq;
use std::io::Write;
use tempfile::NamedTempFile;

fn setup_db() -> ComponentDb {
    let db = ComponentDb::open_in_memory().unwrap();
    db.insert_category(&Category {
        id: None, name: "MCU".to_string(), parent_id: None, description: None,
    }).unwrap();
    db.insert_category(&Category {
        id: None, name: "Capacitor".to_string(), parent_id: None, description: None,
    }).unwrap();
    db.insert_category(&Category {
        id: None, name: "DCDC_IC".to_string(), parent_id: None, description: None,
    }).unwrap();
    db
}

#[test]
fn test_import_from_json() {
    let db = setup_db();
    let json = r#"{
        "mpn": "STM32F103C8T6",
        "manufacturer": "STMicroelectronics",
        "category": "MCU",
        "package": "LQFP-48",
        "description": "ARM Cortex-M3 72MHz 64KB Flash 20KB SRAM",
        "kicad_symbol": "MCU_ST_STM32F1xx:STM32F103C8Tx",
        "pins": [
            {"number": "1", "name": "VBAT", "electrical_type": "power_in"},
            {"number": "2", "name": "PC13", "alt_functions": ["RTC_TAMPER", "WKUP2"]},
            {"number": "44", "name": "PA0", "electrical_type": "bidirectional",
             "alt_functions": ["ADC0", "TIM2_CH1"]}
        ],
        "parameters": [
            {"name": "vcc_min", "value": 2.0, "unit": "V"},
            {"name": "vcc_max", "value": 3.6, "unit": "V"},
            {"name": "clock_max", "value": 72000000, "unit": "Hz"},
            {"name": "interfaces", "value_text": "SPI, I2C, USART, USB, CAN"}
        ],
        "supply_info": [
            {"supplier": "LCSC", "sku": "C8304", "stock": 50000, "price_breaks": [[100, 0.52]]}
        ]
    }"#;

    let id = db.import_from_json(json).unwrap();
    assert!(id > 0);

    let comp = db.get_component(id).unwrap().unwrap();
    assert_eq!(comp.mpn, "STM32F103C8T6");
    assert_eq!(comp.package, Some("LQFP-48".to_string()));
    assert_eq!(comp.kicad_symbol, Some("MCU_ST_STM32F1xx:STM32F103C8Tx".to_string()));

    let pins = db.get_pins(id).unwrap();
    assert_eq!(pins.len(), 3);
    let pa0 = pins.iter().find(|p| p.pin_number == "44").unwrap();
    assert_eq!(pa0.pin_name, "PA0");
    assert_eq!(pa0.alt_functions, Some(vec!["ADC0".to_string(), "TIM2_CH1".to_string()]));

    let params = db.get_parameters(id).unwrap();
    assert_eq!(params.len(), 4);
    let vcc = params.iter().find(|p| p.name == "vcc_min").unwrap();
    assert_eq!(vcc.value_numeric, Some(2.0));

    let intf = params.iter().find(|p| p.name == "interfaces").unwrap();
    assert_eq!(intf.value_text, Some("SPI, I2C, USART, USB, CAN".to_string()));

    let supply = db.get_supply_info(id).unwrap();
    assert_eq!(supply.len(), 1);
    assert_eq!(supply[0].supplier, "LCSC");
}

#[test]
fn test_import_spice_model() {
    let db = setup_db();
    // First create a component
    let comp_id = db.insert_component(&Component {
        id: None, mpn: "TL431AIDBZR".to_string(), manufacturer: "TI".to_string(),
        category_id: 3, description: None, package: None, lifecycle: "active".to_string(),
        datasheet_url: None, kicad_symbol: None, kicad_footprint: None,
    }).unwrap();

    let spice = r#"* TL431 Shunt Regulator
.SUBCKT TL431 REF CAT ANO
R1 REF ANO 10k
Q1 CAT REF ANO QNPN
.MODEL QNPN NPN(IS=1E-14 BF=200)
.ENDS TL431
"#;

    let model_id = db.import_simulation_model(comp_id, "spice", Some("behavioral"), spice, "spice3").unwrap();
    assert!(model_id > 0);

    let models = db.get_simulation_models(comp_id).unwrap();
    assert_eq!(models.len(), 1);
    assert!(models[0].model_text.contains("TL431"));
    assert_eq!(models[0].model_type, "spice");
    assert_eq!(models[0].format, Some("spice3".to_string()));
}

#[test]
fn test_import_upsert() {
    let db = setup_db();

    let json1 = r#"{
        "mpn": "GRM155R71C104KA88",
        "manufacturer": "Murata",
        "category": "Capacitor",
        "package": "0402",
        "parameters": [{"name": "capacitance", "value": 1e-7, "unit": "F"}]
    }"#;
    let id1 = db.import_from_json(json1).unwrap();

    // Import again — should update, not duplicate
    let json2 = r#"{
        "mpn": "GRM155R71C104KA88",
        "manufacturer": "Murata",
        "category": "Capacitor",
        "package": "0402",
        "description": "Updated: 100nF 16V 0402"
    }"#;
    let id2 = db.import_from_json(json2).unwrap();
    assert_eq!(id1, id2, "Upsert should return same component ID");

    let comp = db.get_component(id1).unwrap().unwrap();
    assert_eq!(comp.description, Some("Updated: 100nF 16V 0402".to_string()));
}

#[test]
fn test_import_batch() {
    let db = setup_db();

    let batch = r#"[
        {"mpn": "C0402_100nF", "manufacturer": "Murata", "category": "Capacitor",
         "parameters": [{"name": "capacitance", "value": 1e-7, "unit": "F"}]},
        {"mpn": "C0402_10nF", "manufacturer": "Murata", "category": "Capacitor",
         "parameters": [{"name": "capacitance", "value": 1e-8, "unit": "F"}]},
        {"mpn": "C0402_1uF", "manufacturer": "Samsung", "category": "Capacitor",
         "parameters": [{"name": "capacitance", "value": 1e-6, "unit": "F"}]}
    ]"#;

    let ids = db.import_batch_from_json(batch).unwrap();
    assert_eq!(ids.len(), 3);
    assert!(ids.iter().all(|&id| id > 0));

    // Verify all 3 caps are in DB
    let caps = db.query_components_by_category("Capacitor").unwrap();
    assert_eq!(caps.len(), 3);
}

#[test]
fn test_import_validation_missing_mpn() {
    let db = setup_db();
    let json = r#"{"manufacturer": "TI", "category": "MCU"}"#;
    let result = db.import_from_json(json);
    assert!(result.is_err(), "Should fail on missing mpn");
}

#[test]
fn test_import_validation_missing_category() {
    let db = setup_db();
    let json = r#"{"mpn": "TEST", "manufacturer": "TI", "category": "NonExistent"}"#;
    let result = db.import_from_json(json);
    assert!(result.is_err(), "Should fail on unknown category");
}

#[test]
fn test_import_auto_create_category() {
    let db = ComponentDb::open_in_memory().unwrap();
    // Pre-create only parent
    db.insert_category(&Category {
        id: None, name: "IC".to_string(), parent_id: None, description: None,
    }).unwrap();

    let json = r#"{
        "mpn": "ESP32-S3",
        "manufacturer": "Espressif",
        "category": "WiFiMCU",
        "auto_create_category": true,
        "description": "ESP32-S3 WiFi+BLE MCU"
    }"#;

    let id = db.import_from_json(json).unwrap();
    assert!(id > 0);

    let cat = db.get_category_by_name("WiFiMCU").unwrap();
    assert!(cat.is_some(), "Category should be auto-created");
    assert_eq!(cat.unwrap().name, "WiFiMCU");
}
