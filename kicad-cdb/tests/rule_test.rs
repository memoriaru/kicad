use component_db::*;
use pretty_assertions::assert_eq;

fn seed_rules_db() -> ComponentDb {
    let db = ComponentDb::open_in_memory().unwrap();

    let cat_dcdc = db.insert_category(&Category {
        id: None, name: "DCDC_IC".to_string(), parent_id: None, description: None,
    }).unwrap();
    let cat_ldo = db.insert_category(&Category {
        id: None, name: "LDO".to_string(), parent_id: None, description: None,
    }).unwrap();
    let cat_cap = db.insert_category(&Category {
        id: None, name: "Capacitor".to_string(), parent_id: None, description: None,
    }).unwrap();
    let cat_ind = db.insert_category(&Category {
        id: None, name: "Inductor".to_string(), parent_id: None, description: None,
    }).unwrap();

    // Buck inductor selection rule
    db.insert_design_rule(&DesignRule {
        id: None,
        name: "buck_inductor_selection".to_string(),
        category_id: Some(cat_dcdc),
        description: Some("Calculate minimum inductance for buck converter".to_string()),
        condition_expr: None,
        formula_expr: Some("l_min = (vout * (1 - vout / vin)) / (fsw * ripple_ratio * iout)".to_string()),
        check_expr: Some("L_value >= l_min * 0.8".to_string()),
        parameters: Some(r#"["vin", "vout", "iout", "fsw", "ripple_ratio"]"#.to_string()),
        output_params: Some(r#"["l_min"]"#.to_string()),
        source: Some("TI Application Note SLVA477".to_string()),
    }).unwrap();

    // LDO dropout check
    db.insert_design_rule(&DesignRule {
        id: None,
        name: "ldo_dropout_check".to_string(),
        category_id: Some(cat_ldo),
        description: Some("Verify LDO dropout voltage is within spec".to_string()),
        condition_expr: None,
        formula_expr: Some("dropout = vin - vout".to_string()),
        check_expr: Some("dropout >= vdropout_max".to_string()),
        parameters: Some(r#"["vin", "vout", "vdropout_max"]"#.to_string()),
        output_params: Some(r#"["dropout"]"#.to_string()),
        source: None,
    }).unwrap();

    // Capacitor voltage derating
    db.insert_design_rule(&DesignRule {
        id: None,
        name: "cap_voltage_derating".to_string(),
        category_id: Some(cat_cap),
        description: Some("Capacitor voltage rating should be >= 2x operating voltage".to_string()),
        condition_expr: None,
        formula_expr: Some("min_rating = voperating * derating_factor".to_string()),
        check_expr: Some("C_voltage_rating >= min_rating".to_string()),
        parameters: Some(r#"["voperating", "derating_factor"]"#.to_string()),
        output_params: Some(r#"["min_rating"]"#.to_string()),
        source: None,
    }).unwrap();

    db
}

#[test]
fn test_load_rule() {
    let db = seed_rules_db();
    let rules = db.get_design_rules_by_category(
        db.get_category_by_name("DCDC_IC").unwrap().unwrap().id.unwrap()
    ).unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].name, "buck_inductor_selection");
    assert!(rules[0].formula_expr.as_ref().unwrap().contains("l_min"));
}

#[test]
fn test_evaluate_simple_formula() {
    let db = seed_rules_db();
    let mut ctx = component_db::rules::EvalContext::new();
    ctx.set("vin", 12.0);
    ctx.set("vout", 5.0);
    ctx.set("fsw", 500e3);
    ctx.set("ripple_ratio", 0.3);
    ctx.set("iout", 2.0);

    // l_min = (vout * (1 - vout / vin)) / (fsw * ripple_ratio * iout)
    let result = ctx.eval("(vout * (1 - vout / vin)) / (fsw * ripple_ratio * iout)").unwrap();
    let expected = (5.0 * (1.0 - 5.0 / 12.0)) / (500e3 * 0.3 * 2.0);
    assert!((result - expected).abs() < 1e-12, "got {} expected {}", result, expected);
}

#[test]
fn test_evaluate_assignment() {
    let mut ctx = component_db::rules::EvalContext::new();
    ctx.set("vin", 12.0);
    ctx.set("vout", 5.0);

    // Evaluate formula with assignment: "dropout = vin - vout"
    let outputs = ctx.eval_formula("dropout = vin - vout").unwrap();
    assert_eq!(outputs.len(), 1);
    assert_eq!(outputs[0].0, "dropout");
    assert!((outputs[0].1 - 7.0).abs() < 1e-12);
}

#[test]
fn test_check_constraint_pass() {
    let mut ctx = component_db::rules::EvalContext::new();
    ctx.set("dropout", 7.0);
    ctx.set("vdropout_max", 0.5);

    // dropout >= vdropout_max → 7.0 >= 0.5 → PASS
    let result = ctx.eval_check("dropout >= vdropout_max").unwrap();
    assert!(result);
}

#[test]
fn test_check_constraint_fail() {
    let mut ctx = component_db::rules::EvalContext::new();
    ctx.set("dropout", 0.3);
    ctx.set("vdropout_max", 0.5);

    // dropout >= vdropout_max → 0.3 >= 0.5 → FAIL
    let result = ctx.eval_check("dropout >= vdropout_max").unwrap();
    assert!(!result);
}

#[test]
fn test_buck_inductor_rule_e2e() {
    let db = seed_rules_db();
    let rule = db.get_design_rules_by_category(
        db.get_category_by_name("DCDC_IC").unwrap().unwrap().id.unwrap()
    ).unwrap().into_iter().next().unwrap();

    // Design parameters: 12V→5V, 2A, 500kHz, 30% ripple
    let mut ctx = component_db::rules::EvalContext::new();
    ctx.set("vin", 12.0);
    ctx.set("vout", 5.0);
    ctx.set("iout", 2.0);
    ctx.set("fsw", 500e3);
    ctx.set("ripple_ratio", 0.3);

    // Step 1: Calculate l_min
    let outputs = ctx.eval_formula(rule.formula_expr.as_ref().unwrap()).unwrap();
    let l_min = outputs.iter().find(|(k, _)| k == "l_min").unwrap().1;

    // l_min = (5 * (1 - 5/12)) / (500000 * 0.3 * 2) ≈ 9.72e-6 = 9.72µH
    assert!(l_min > 9.0e-6 && l_min < 10.0e-6, "l_min = {} H, expected ~9.72µH", l_min);

    // Step 2: Check with a candidate inductor (10µH)
    ctx.set("L_value", 10e-6);
    let pass = ctx.eval_check(rule.check_expr.as_ref().unwrap()).unwrap();
    assert!(pass, "10µH inductor should pass (>= l_min * 0.8)");

    // Step 3: Check with a too-small inductor (5µH)
    ctx.set("L_value", 5e-6);
    let fail = ctx.eval_check(rule.check_expr.as_ref().unwrap()).unwrap();
    assert!(!fail, "5µH inductor should fail (< l_min * 0.8)");
}

#[test]
fn test_ldo_dropout_rule_e2e() {
    let db = seed_rules_db();
    let rule = db.get_design_rules_by_category(
        db.get_category_by_name("LDO").unwrap().unwrap().id.unwrap()
    ).unwrap().into_iter().next().unwrap();

    // 3.3V LDO from 5V input, max dropout 0.5V
    let mut ctx = component_db::rules::EvalContext::new();
    ctx.set("vin", 5.0);
    ctx.set("vout", 3.3);
    ctx.set("vdropout_max", 0.5);

    let outputs = ctx.eval_formula(rule.formula_expr.as_ref().unwrap()).unwrap();
    let dropout = outputs[0].1;
    assert!((dropout - 1.7).abs() < 1e-12);

    // 1.7V dropout >= 0.5V required → PASS
    let pass = ctx.eval_check(rule.check_expr.as_ref().unwrap()).unwrap();
    assert!(pass);
}

#[test]
fn test_cap_derating_rule_e2e() {
    let db = seed_rules_db();
    let rule = db.get_design_rules_by_category(
        db.get_category_by_name("Capacitor").unwrap().unwrap().id.unwrap()
    ).unwrap().into_iter().next().unwrap();

    // 3.3V operating, 2x derating → need >= 6.6V rating
    let mut ctx = component_db::rules::EvalContext::new();
    ctx.set("voperating", 3.3);
    ctx.set("derating_factor", 2.0);

    let outputs = ctx.eval_formula(rule.formula_expr.as_ref().unwrap()).unwrap();
    let min_rating = outputs[0].1;
    assert!((min_rating - 6.6).abs() < 1e-12);

    // Check with 10V cap → PASS
    ctx.set("C_voltage_rating", 10.0);
    assert!(ctx.eval_check(rule.check_expr.as_ref().unwrap()).unwrap());

    // Check with 6.3V cap → PASS (barely)
    ctx.set("C_voltage_rating", 6.3);
    assert!(!ctx.eval_check(rule.check_expr.as_ref().unwrap()).unwrap());
}

#[test]
fn test_apply_rule_with_db() {
    let db = seed_rules_db();
    let rule = db.get_design_rules_by_category(
        db.get_category_by_name("DCDC_IC").unwrap().unwrap().id.unwrap()
    ).unwrap().into_iter().next().unwrap();

    let inputs = serde_json::json!({
        "vin": 12.0, "vout": 5.0, "iout": 2.0, "fsw": 500e3, "ripple_ratio": 0.3
    });

    let result = db.apply_rule(&rule, &inputs, Some("L_value"), Some(10e-6)).unwrap();
    assert!(result.pass);
    assert!(result.outputs.contains_key("l_min"));
    let l_min = result.outputs["l_min"];
    assert!(l_min > 9.0e-6 && l_min < 10.0e-6);
}

#[test]
fn test_apply_rule_fail() {
    let db = seed_rules_db();
    let rule = db.get_design_rules_by_category(
        db.get_category_by_name("LDO").unwrap().unwrap().id.unwrap()
    ).unwrap().into_iter().next().unwrap();

    // Edge case: vin = vout, dropout = 0
    let inputs = serde_json::json!({
        "vin": 3.3, "vout": 3.3, "vdropout_max": 0.5
    });

    let result = db.apply_rule(&rule, &inputs, None, None).unwrap();
    assert!(!result.pass, "dropout=0 should fail vdropout_max=0.5 check");
}
