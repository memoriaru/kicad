use anyhow::Result;
use serde::Serialize;
use std::collections::HashMap;

use crate::ComponentDb;

/// Single step execution record
#[derive(Debug, Serialize)]
pub struct DesignStep {
    pub seq: usize,
    pub rule_name: String,
    pub description: String,
    pub inputs: HashMap<String, f64>,
    pub formula: String,
    pub outputs: HashMap<String, f64>,
    pub check_expr: String,
    pub passed: bool,
    pub skipped: bool,
    pub skip_reason: Option<String>,
}

/// Complete design decision log
#[derive(Debug, Serialize)]
pub struct DesignLog {
    pub pipeline_name: String,
    pub user_inputs: HashMap<String, f64>,
    pub steps: Vec<DesignStep>,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
}

/// A single step in a pipeline definition
pub struct PipelineStep {
    pub rule_name: String,
    /// Optional condition expression (e.g. "iout > 2"). Step is skipped if false.
    pub condition: Option<String>,
}

/// A pipeline definition
pub struct Pipeline {
    pub name: String,
    pub description: String,
    pub user_inputs: Vec<String>,
    pub steps: Vec<PipelineStep>,
}

/// Get all built-in pipelines
pub fn builtin_pipelines() -> Vec<Pipeline> {
    vec![
        Pipeline {
            name: "buck".into(),
            description: "Buck Converter Design".into(),
            user_inputs: vec![
                "vin".into(), "vout".into(), "iout".into(),
                "fsw".into(), "ripple_ratio".into(), "ripple_v".into(),
            ],
            steps: vec![
                PipelineStep { rule_name: "buck_duty_cycle".into(), condition: None },
                PipelineStep { rule_name: "buck_inductor_selection".into(), condition: None },
                PipelineStep { rule_name: "buck_inductor_ripple".into(), condition: None },
                PipelineStep { rule_name: "buck_output_capacitor".into(), condition: None },
                PipelineStep { rule_name: "buck_input_capacitor".into(), condition: None },
                PipelineStep { rule_name: "buck_catch_diode".into(), condition: None },
                PipelineStep { rule_name: "thermal_dissipation".into(), condition: None },
            ],
        },
        Pipeline {
            name: "boost".into(),
            description: "Boost Converter Design".into(),
            user_inputs: vec![
                "vin".into(), "vout".into(), "iout".into(),
                "fsw".into(), "ripple_ratio".into(), "ripple_v".into(),
            ],
            steps: vec![
                PipelineStep { rule_name: "boost_duty_cycle".into(), condition: None },
                PipelineStep { rule_name: "boost_inductor_selection".into(), condition: None },
                PipelineStep { rule_name: "boost_inductor_ripple".into(), condition: None },
                PipelineStep { rule_name: "boost_output_capacitor".into(), condition: None },
                PipelineStep { rule_name: "boost_switch_voltage".into(), condition: None },
                PipelineStep { rule_name: "boost_diode_voltage".into(), condition: None },
                PipelineStep { rule_name: "thermal_dissipation".into(), condition: None },
            ],
        },
        Pipeline {
            name: "ldo".into(),
            description: "LDO Regulator Design".into(),
            user_inputs: vec![
                "vin".into(), "vout".into(), "iout".into(),
                "vdropout_max".into(), "p_max".into(), "eff_min".into(), "ripple_v".into(),
            ],
            steps: vec![
                PipelineStep { rule_name: "ldo_dropout_check".into(), condition: None },
                PipelineStep { rule_name: "ldo_power_dissipation".into(), condition: None },
                PipelineStep { rule_name: "ldo_efficiency".into(), condition: None },
                PipelineStep { rule_name: "ldo_output_cap".into(), condition: None },
            ],
        },
        Pipeline {
            name: "led".into(),
            description: "LED Current Limiting Design".into(),
            user_inputs: vec!["vin".into(), "vf".into(), "i_led".into(), "r_power".into()],
            steps: vec![
                PipelineStep { rule_name: "led_current_resistor".into(), condition: None },
            ],
        },
    ]
}

/// Get a built-in pipeline by name
pub fn get_builtin_pipeline(name: &str) -> Option<Pipeline> {
    builtin_pipelines().into_iter().find(|p| p.name == name)
}

/// Run a pipeline and produce a design decision log
pub fn run_pipeline(
    db: &ComponentDb,
    pipeline: &Pipeline,
    user_inputs: &HashMap<String, f64>,
) -> Result<DesignLog> {
    let rules = db.get_all_design_rules()?;
    let mut ctx: HashMap<String, f64> = user_inputs.clone();

    let mut steps = Vec::new();
    let mut passed = 0;
    let mut failed = 0;
    let mut skipped = 0;

    for (i, step_def) in pipeline.steps.iter().enumerate() {
        // Check step-level condition gate
        if let Some(cond) = &step_def.condition {
            let mut eval = crate::rules::EvalContext::new();
            for (name, &val) in &ctx {
                eval.set(name, val);
            }
            match eval.eval_check(cond) {
                Ok(true) => {},
                Ok(false) => {
                    skipped += 1;
                    steps.push(DesignStep {
                        seq: i + 1,
                        rule_name: step_def.rule_name.clone(),
                        description: String::new(),
                        inputs: HashMap::new(),
                        formula: String::new(),
                        outputs: HashMap::new(),
                        check_expr: String::new(),
                        passed: false,
                        skipped: true,
                        skip_reason: Some(format!("Step condition '{}' not met", cond)),
                    });
                    continue;
                }
                Err(_) => {
                    skipped += 1;
                    steps.push(DesignStep {
                        seq: i + 1,
                        rule_name: step_def.rule_name.clone(),
                        description: String::new(),
                        inputs: HashMap::new(),
                        formula: String::new(),
                        outputs: HashMap::new(),
                        check_expr: String::new(),
                        passed: false,
                        skipped: true,
                        skip_reason: Some(format!("Step condition '{}' could not be evaluated", cond)),
                    });
                    continue;
                }
            }
        }

        let rule = match rules.iter().find(|r| r.name == step_def.rule_name) {
            Some(r) => r.clone(),
            None => {
                skipped += 1;
                steps.push(DesignStep {
                    seq: i + 1,
                    rule_name: step_def.rule_name.clone(),
                    description: String::new(),
                    inputs: HashMap::new(),
                    formula: String::new(),
                    outputs: HashMap::new(),
                    check_expr: String::new(),
                    passed: false,
                    skipped: true,
                    skip_reason: Some(format!("Rule '{}' not found in database", step_def.rule_name)),
                });
                continue;
            }
        };

        // Extract parameters declared by this rule from shared context
        let param_names: Vec<String> = rule.parameters
            .as_ref()
            .and_then(|p| serde_json::from_str::<Vec<String>>(p).ok())
            .unwrap_or_default();

        let mut step_inputs = serde_json::Map::new();
        for pname in &param_names {
            if let Some(&val) = ctx.get(pname.as_str()) {
                step_inputs.insert(pname.clone(), serde_json::Value::from(val));
            }
        }

        // apply_rule now validates input completeness and bails on missing params
        let result = db.apply_rule(
            &rule,
            &serde_json::Value::Object(step_inputs.clone()),
            None,
            None,
        )?;

        // Merge outputs into shared context
        for (name, val) in &result.outputs {
            ctx.insert(name.clone(), *val);
        }

        let is_skipped = result.check_expression.contains("(skipped:");

        if is_skipped {
            skipped += 1;
        } else if result.pass {
            passed += 1;
        } else {
            failed += 1;
        }

        steps.push(DesignStep {
            seq: i + 1,
            rule_name: rule.name.clone(),
            description: rule.description.clone().unwrap_or_default(),
            inputs: step_inputs.into_iter().map(|(k, v)| (k, v.as_f64().unwrap_or(0.0))).collect(),
            formula: rule.formula_expr.clone().unwrap_or_default(),
            outputs: result.outputs,
            check_expr: if is_skipped { String::new() } else { result.check_expression },
            passed: result.pass,
            skipped: is_skipped,
            skip_reason: if is_skipped { Some("condition not met".into()) } else { None },
        });
    }

    Ok(DesignLog {
        pipeline_name: pipeline.name.clone(),
        user_inputs: user_inputs.clone(),
        steps,
        passed,
        failed,
        skipped,
    })
}
