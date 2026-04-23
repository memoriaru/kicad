use component_db::*;
use pretty_assertions::assert_eq;

fn setup_db() -> ComponentDb {
    let db = ComponentDb::open_in_memory().unwrap();
    // Insert required categories
    db.insert_category(&Category {
        id: None, name: "MCU".to_string(), parent_id: None, description: None,
    }).unwrap();
    db.insert_category(&Category {
        id: None, name: "DCDC_IC".to_string(), parent_id: None, description: None,
    }).unwrap();
    db.insert_category(&Category {
        id: None, name: "Capacitor".to_string(), parent_id: None, description: None,
    }).unwrap();
    db
}

fn make_component(category_id: i64) -> Component {
    Component {
        id: None,
        mpn: "STM32F103C8T6".to_string(),
        manufacturer: "STMicroelectronics".to_string(),
        category_id,
        description: Some("ARM Cortex-M3 MCU 72MHz 64KB Flash".to_string()),
        package: Some("LQFP-48".to_string()),
        lifecycle: "active".to_string(),
        datasheet_url: None,
        kicad_symbol: Some("MCU_ST_STM32F1xx:STM32F103C8Tx".to_string()),
        kicad_footprint: None,
    }
}

#[test]
fn test_insert_component() {
    let db = setup_db();
    let comp = make_component(1);
    let id = db.insert_component(&comp).unwrap();
    assert!(id > 0);

    let loaded = db.get_component(id).unwrap().unwrap();
    assert_eq!(loaded.mpn, "STM32F103C8T6");
    assert_eq!(loaded.manufacturer, "STMicroelectronics");
    assert_eq!(loaded.category_id, 1);
    assert_eq!(loaded.package, Some("LQFP-48".to_string()));
    assert_eq!(loaded.lifecycle, "active");
}

#[test]
fn test_insert_component_unique_constraint() {
    let db = setup_db();
    let comp = make_component(1);
    db.insert_component(&comp).unwrap();

    // Same mpn + manufacturer should fail
    let result = db.insert_component(&comp);
    assert!(result.is_err(), "Should fail on unique constraint violation");
}

#[test]
fn test_insert_pins() {
    let db = setup_db();
    let comp_id = db.insert_component(&make_component(1)).unwrap();

    let pins = vec![
        Pin {
            id: None, component_id: comp_id,
            pin_number: "1".to_string(), pin_name: "VBAT".to_string(),
            pin_group: Some("Power".to_string()),
            electrical_type: Some("power_in".to_string()),
            alt_functions: None, description: None,
        },
        Pin {
            id: None, component_id: comp_id,
            pin_number: "44".to_string(), pin_name: "PA0".to_string(),
            pin_group: Some("PortA".to_string()),
            electrical_type: Some("bidirectional".to_string()),
            alt_functions: Some(vec!["ADC0".to_string(), "TIM2_CH1".to_string()]),
            description: Some("Wake-up pin".to_string()),
        },
    ];

    let ids = db.insert_pins(&pins).unwrap();
    assert_eq!(ids.len(), 2);

    let loaded = db.get_pins(comp_id).unwrap();
    assert_eq!(loaded.len(), 2);
    assert_eq!(loaded[0].pin_name, "VBAT");
    assert_eq!(loaded[1].pin_name, "PA0");
    assert_eq!(loaded[1].alt_functions, Some(vec!["ADC0".to_string(), "TIM2_CH1".to_string()]));
}

#[test]
fn test_insert_parameters() {
    let db = setup_db();
    let comp_id = db.insert_component(&make_component(1)).unwrap();

    let params = vec![
        Parameter {
            id: None, component_id: comp_id,
            name: "vcc_min".to_string(),
            value_numeric: Some(2.0), value_text: None,
            unit: Some("V".to_string()), typical: false,
            condition: None, source_page: Some("p.12".to_string()),
        },
        Parameter {
            id: None, component_id: comp_id,
            name: "vcc_max".to_string(),
            value_numeric: Some(3.6), value_text: None,
            unit: Some("V".to_string()), typical: false,
            condition: None, source_page: None,
        },
        Parameter {
            id: None, component_id: comp_id,
            name: "interfaces".to_string(),
            value_numeric: None,
            value_text: Some("SPI, I2C, USART, USB, CAN".to_string()),
            unit: None, typical: false,
            condition: None, source_page: None,
        },
    ];

    let ids = db.insert_parameters(&params).unwrap();
    assert_eq!(ids.len(), 3);

    let loaded = db.get_parameters(comp_id).unwrap();
    assert_eq!(loaded.len(), 3);

    let vcc_min = loaded.iter().find(|p| p.name == "vcc_min").unwrap();
    assert_eq!(vcc_min.value_numeric, Some(2.0));
    assert_eq!(vcc_min.unit, Some("V".to_string()));

    let interfaces = loaded.iter().find(|p| p.name == "interfaces").unwrap();
    assert_eq!(interfaces.value_text, Some("SPI, I2C, USART, USB, CAN".to_string()));
}

#[test]
fn test_insert_simulation_model() {
    let db = setup_db();
    let comp_id = db.insert_component(&make_component(1)).unwrap();

    let spice_model = r#"
* STM32F103 Simple Behavioral Model
.SUBCKT STM32F103 VDD VSS PA0 PA1 NRST
R_pd NRST VSS 100k
C_io PA0 VSS 5p
C_io PA1 VSS 5p
.ENDS STM32F103
"#;

    let id = db.insert_simulation_model(&SimulationModel {
        id: None, component_id: comp_id,
        model_type: "spice".to_string(),
        model_subcategory: Some("behavioral".to_string()),
        model_text: spice_model.to_string(),
        format: Some("spice3".to_string()),
        port_mapping: Some(r#"{"1": "VDD", "44": "PA0"}"#.to_string()),
        verified: false, source: Some("manufacturer".to_string()), notes: None,
    }).unwrap();
    assert!(id > 0);

    let models = db.get_simulation_models(comp_id).unwrap();
    assert_eq!(models.len(), 1);
    assert!(models[0].model_text.contains("STM32F103"));
    assert_eq!(models[0].model_type, "spice");
}

#[test]
fn test_insert_supply_info() {
    let db = setup_db();
    let comp_id = db.insert_component(&make_component(1)).unwrap();

    let id = db.insert_supply_info(&SupplyInfo {
        id: None, component_id: comp_id,
        supplier: "LCSC".to_string(),
        sku: Some("C8304".to_string()),
        price_breaks: Some("[[100, 0.52], [1000, 0.45]]".to_string()),
        stock: Some(50000), lead_time_days: Some(7), moq: Some(1),
    }).unwrap();
    assert!(id > 0);

    let infos = db.get_supply_info(comp_id).unwrap();
    assert_eq!(infos.len(), 1);
    assert_eq!(infos[0].supplier, "LCSC");
    assert_eq!(infos[0].stock, Some(50000));
}

#[test]
fn test_insert_design_rule() {
    let db = setup_db();
    // Use DCDC_IC category (id=2)
    let rule_id = db.insert_design_rule(&DesignRule {
        id: None,
        name: "buck_inductor_selection".to_string(),
        category_id: Some(2),
        description: Some("Buck converter inductor selection rule".to_string()),
        condition_expr: None,
        formula_expr: Some("l_min = (vout * (1 - vout / vin)) / (fsw * 0.3 * iout)".to_string()),
        check_expr: Some("Inductor.value >= l_min * 0.8".to_string()),
        parameters: Some(r#"["vin", "vout", "iout", "fsw"]"#.to_string()),
        output_params: Some(r#"["l_min"]"#.to_string()),
        source: None,
    }).unwrap();
    assert!(rule_id > 0);

    let rules = db.get_design_rules_by_category(2).unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].name, "buck_inductor_selection");
    assert!(rules[0].formula_expr.as_ref().unwrap().contains("l_min"));
}

#[test]
fn test_delete_cascade() {
    let db = setup_db();
    let comp_id = db.insert_component(&make_component(1)).unwrap();

    // Insert related records
    db.insert_pin(&Pin {
        id: None, component_id: comp_id,
        pin_number: "1".to_string(), pin_name: "VCC".to_string(),
        pin_group: None, electrical_type: Some("power_in".to_string()),
        alt_functions: None, description: None,
    }).unwrap();

    db.insert_parameter(&Parameter {
        id: None, component_id: comp_id,
        name: "vcc".to_string(), value_numeric: Some(3.3),
        value_text: None, unit: Some("V".to_string()),
        typical: true, condition: None, source_page: None,
    }).unwrap();

    db.insert_simulation_model(&SimulationModel {
        id: None, component_id: comp_id,
        model_type: "spice".to_string(), model_subcategory: None,
        model_text: "* model".to_string(), format: None,
        port_mapping: None, verified: false, source: None, notes: None,
    }).unwrap();

    db.insert_supply_info(&SupplyInfo {
        id: None, component_id: comp_id,
        supplier: "LCSC".to_string(), sku: None,
        price_breaks: None, stock: None, lead_time_days: None, moq: None,
    }).unwrap();

    // Delete component — should cascade
    assert!(db.delete_component(comp_id).unwrap());

    // Verify all related records are gone
    assert!(db.get_pins(comp_id).unwrap().is_empty());
    assert!(db.get_parameters(comp_id).unwrap().is_empty());
    assert!(db.get_simulation_models(comp_id).unwrap().is_empty());
    assert!(db.get_supply_info(comp_id).unwrap().is_empty());
}

#[test]
fn test_update_component() {
    let db = setup_db();
    let comp_id = db.insert_component(&make_component(1)).unwrap();

    let mut comp = db.get_component(comp_id).unwrap().unwrap();
    comp.description = Some("Updated description".to_string());
    comp.package = Some("QFP-48".to_string());

    assert!(db.update_component(&comp).unwrap());

    let updated = db.get_component(comp_id).unwrap().unwrap();
    assert_eq!(updated.description, Some("Updated description".to_string()));
    assert_eq!(updated.package, Some("QFP-48".to_string()));
}
