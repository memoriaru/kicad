use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

use crate::ComponentDb;

// ---------------------------------------------------------------------------
// Composition spec (workflow definition)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CompositionSpec {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub goals: Vec<String>,
    pub required_inputs: Vec<String>,
    pub stages: Vec<StageDef>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StageDef {
    pub name: String,
    #[serde(flatten)]
    pub stage_type: StageType,
    #[serde(default)]
    pub outputs: Vec<String>,
    pub condition: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StageType {
    Suggest {
        #[serde(default)]
        isolated: bool,
    },
    Pipeline {
        name: String,
    },
    Recommend {
        rule: String,
        #[serde(default)]
        limit: Option<usize>,
    },
    Check {
        rule: String,
    },
}

// ---------------------------------------------------------------------------
// Composition result
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct CompositionResult {
    pub spec_name: String,
    pub inputs: HashMap<String, f64>,
    pub stages: Vec<StageResult>,
    pub context: HashMap<String, f64>,
    pub scores: HashMap<String, f64>,
    pub recommendations: Vec<StageRecommendation>,
    pub summary: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct StageResult {
    pub name: String,
    pub stage_type: String,
    pub skipped: bool,
    pub skip_reason: Option<String>,
    pub detail: Value,
}

#[derive(Debug, Serialize)]
pub struct StageRecommendation {
    pub stage: String,
    pub role: String,
    pub components: Vec<crate::Component>,
}

// ---------------------------------------------------------------------------
// Built-in compositions
// ---------------------------------------------------------------------------

pub fn builtin_compositions() -> Vec<CompositionSpec> {
    vec![
        CompositionSpec {
            name: "power_tree".into(),
            description: "Power tree design: suggest topology → run pipeline → recommend components → thermal check".into(),
            goals: vec!["power_supply_design".into()],
            required_inputs: vec!["vin".into(), "vout".into(), "iout".into()],
            stages: vec![
                StageDef {
                    name: "suggest".into(),
                    stage_type: StageType::Suggest { isolated: false },
                    outputs: vec!["topology".into(), "estimated_efficiency".into()],
                    condition: None,
                },
                StageDef {
                    name: "pipeline".into(),
                    stage_type: StageType::Pipeline { name: "buck".into() },
                    outputs: vec![],
                    condition: None,
                },
                StageDef {
                    name: "recommend_inductor".into(),
                    stage_type: StageType::Recommend {
                        rule: "buck_inductor_selection".into(),
                        limit: Some(5),
                    },
                    outputs: vec![],
                    condition: None,
                },
                StageDef {
                    name: "thermal_check".into(),
                    stage_type: StageType::Check {
                        rule: "thermal_dissipation".into(),
                    },
                    outputs: vec![],
                    condition: None,
                },
            ],
        },
        CompositionSpec {
            name: "ldo_design".into(),
            description: "LDO regulator design: pipeline → efficiency check → component recommendation".into(),
            goals: vec!["ldo_power_supply".into()],
            required_inputs: vec!["vin".into(), "vout".into(), "iout".into()],
            stages: vec![
                StageDef {
                    name: "suggest".into(),
                    stage_type: StageType::Suggest { isolated: false },
                    outputs: vec![],
                    condition: None,
                },
                StageDef {
                    name: "ldo_pipeline".into(),
                    stage_type: StageType::Pipeline { name: "ldo".into() },
                    outputs: vec![],
                    condition: None,
                },
                StageDef {
                    name: "thermal_check".into(),
                    stage_type: StageType::Check {
                        rule: "thermal_dissipation".into(),
                    },
                    outputs: vec![],
                    condition: None,
                },
            ],
        },
    ]
}

// ---------------------------------------------------------------------------
// Run composition
// ---------------------------------------------------------------------------

pub fn run_composition(
    db: &ComponentDb,
    spec: &CompositionSpec,
    user_inputs: &HashMap<String, f64>,
) -> Result<CompositionResult> {
    let mut context: HashMap<String, f64> = user_inputs.clone();
    let mut stages = Vec::new();
    let mut scores: HashMap<String, f64> = HashMap::new();
    let mut recommendations = Vec::new();

    // Fill defaults for common missing params
    context.entry("fsw".into()).or_insert(500000.0);
    context.entry("ripple_ratio".into()).or_insert(0.3);
    context.entry("ripple_v".into()).or_insert(0.05);
    context.entry("vdropout_max".into()).or_insert(0.5);
    context.entry("p_max".into()).or_insert(1.0);
    context.entry("eff_min".into()).or_insert(0.5);
    let iout_default = *context.get("iout").unwrap_or(&0.02);
    context.entry("i_led".into()).or_insert(iout_default);
    context.entry("vf".into()).or_insert(2.0);
    context.entry("r_power".into()).or_insert(0.25);
    context.entry("derating_factor".into()).or_insert(1.5);

    for stage_def in &spec.stages {
        // Evaluate condition gate
        if let Some(cond) = &stage_def.condition {
            let mut eval = crate::rules::EvalContext::new();
            for (name, &val) in &context {
                eval.set(name, val);
            }
            match eval.eval_check(cond) {
                Ok(true) => {}
                Ok(false) => {
                    stages.push(StageResult {
                        name: stage_def.name.clone(),
                        stage_type: stage_type_label(&stage_def.stage_type),
                        skipped: true,
                        skip_reason: Some(format!("Condition '{}' not met", cond)),
                        detail: Value::Null,
                    });
                    continue;
                }
                Err(e) => {
                    stages.push(StageResult {
                        name: stage_def.name.clone(),
                        stage_type: stage_type_label(&stage_def.stage_type),
                        skipped: true,
                        skip_reason: Some(format!("Condition eval error: {}", e)),
                        detail: Value::Null,
                    });
                    continue;
                }
            }
        }

        let result = execute_stage(
            db,
            stage_def,
            &mut context,
            &mut scores,
            &mut recommendations,
        );
        stages.push(result);
    }

    let summary = generate_summary(&stages, &scores);

    Ok(CompositionResult {
        spec_name: spec.name.clone(),
        inputs: user_inputs.clone(),
        stages,
        context,
        scores,
        recommendations,
        summary,
    })
}

fn stage_type_label(st: &StageType) -> String {
    match st {
        StageType::Suggest { .. } => "suggest".into(),
        StageType::Pipeline { .. } => "pipeline".into(),
        StageType::Recommend { .. } => "recommend".into(),
        StageType::Check { .. } => "check".into(),
    }
}

fn execute_stage(
    db: &ComponentDb,
    stage_def: &StageDef,
    context: &mut HashMap<String, f64>,
    scores: &mut HashMap<String, f64>,
    recommendations: &mut Vec<StageRecommendation>,
) -> StageResult {
    match &stage_def.stage_type {
        StageType::Suggest { isolated } => {
            let vin = context.get("vin").copied().unwrap_or(0.0);
            let vout = context.get("vout").copied().unwrap_or(0.0);
            let iout = context.get("iout").copied().unwrap_or(0.0);

            let recs = crate::skills::suggest_topologies(vin, vout, iout, *isolated);

            if let Some(best) = recs.first() {
                context.insert("topology".into(), 0.0); // marker
                context.insert("estimated_efficiency".into(), best.estimated_efficiency);
                scores.insert("topology_score".into(), best.score);

                // Auto-select pipeline name from best topology
                let pl_name = match best.topology.as_str() {
                    "buck" | "boost" | "ldo" | "led" => best.topology.clone(),
                    _ => String::new(),
                };
                if !pl_name.is_empty() {
                    context.insert("_pipeline_name".into(), 0.0); // we use string key below
                }
            }

            StageResult {
                name: stage_def.name.clone(),
                stage_type: "suggest".into(),
                skipped: false,
                skip_reason: None,
                detail: serde_json::to_value(&recs).unwrap_or(Value::Null),
            }
        }

        StageType::Pipeline { name } => {
            // Resolve pipeline name — support dynamic lookup from suggest stage
            let pl_name = if name == "auto" || name.is_empty() {
                // Find topology from suggest results
                context
                    .get("topology")
                    .map(|_| {
                        // Try to find the best matching pipeline
                        let vin = context.get("vin").copied().unwrap_or(0.0);
                        let vout = context.get("vout").copied().unwrap_or(0.0);
                        if vin > vout {
                            "buck"
                        } else {
                            "boost"
                        }
                    })
                    .unwrap_or("buck")
            } else {
                name.as_str()
            };

            match crate::pipeline::get_builtin_pipeline(pl_name) {
                Some(pipeline) => {
                    match crate::pipeline::run_pipeline(db, &pipeline, context) {
                        Ok(log) => {
                            // Merge pipeline outputs into context
                            for step in &log.steps {
                                for (k, &v) in &step.outputs {
                                    context.insert(k.clone(), v);
                                }
                            }
                            scores.insert("pipeline_passed".into(), log.passed as f64);
                            scores.insert("pipeline_failed".into(), log.failed as f64);
                            scores.insert("pipeline_skipped".into(), log.skipped as f64);

                            StageResult {
                                name: stage_def.name.clone(),
                                stage_type: "pipeline".into(),
                                skipped: false,
                                skip_reason: None,
                                detail: serde_json::to_value(&log).unwrap_or(Value::Null),
                            }
                        }
                        Err(e) => StageResult {
                            name: stage_def.name.clone(),
                            stage_type: "pipeline".into(),
                            skipped: true,
                            skip_reason: Some(format!("Pipeline failed: {}", e)),
                            detail: Value::Null,
                        },
                    }
                }
                None => StageResult {
                    name: stage_def.name.clone(),
                    stage_type: "pipeline".into(),
                    skipped: true,
                    skip_reason: Some(format!("Pipeline '{}' not found", pl_name)),
                    detail: Value::Null,
                },
            }
        }

        StageType::Recommend { rule, limit } => {
            let params_str = context
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect::<Vec<_>>()
                .join(",");

            match crate::service::recommend_components(db, rule, &params_str, None, *limit) {
                Ok((_rule, result, components)) => {
                    // Merge rule outputs
                    for (k, &v) in &result.outputs {
                        context.insert(k.clone(), v);
                    }

                    recommendations.push(StageRecommendation {
                        stage: stage_def.name.clone(),
                        role: rule.clone(),
                        components: components.clone(),
                    });

                    StageResult {
                        name: stage_def.name.clone(),
                        stage_type: "recommend".into(),
                        skipped: false,
                        skip_reason: None,
                        detail: serde_json::json!({
                            "rule": rule,
                            "outputs": result.outputs,
                            "component_count": components.len(),
                            "components": components,
                        }),
                    }
                }
                Err(e) => StageResult {
                    name: stage_def.name.clone(),
                    stage_type: "recommend".into(),
                    skipped: true,
                    skip_reason: Some(format!("Recommendation failed: {}", e)),
                    detail: Value::Null,
                },
            }
        }

        StageType::Check { rule } => {
            let params_str = context
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect::<Vec<_>>()
                .join(",");

            match crate::service::apply_rule_with_str_params(db, rule, &params_str, None) {
                Ok((rule_def, result)) => {
                    for (k, &v) in &result.outputs {
                        context.insert(k.clone(), v);
                    }
                    scores.insert(
                        format!("check_{}", rule),
                        if result.pass { 1.0 } else { 0.0 },
                    );

                    StageResult {
                        name: stage_def.name.clone(),
                        stage_type: "check".into(),
                        skipped: false,
                        skip_reason: None,
                        detail: serde_json::json!({
                            "rule": rule,
                            "description": rule_def.description,
                            "outputs": result.outputs,
                            "check": result.check_expression,
                            "pass": result.pass,
                        }),
                    }
                }
                Err(e) => StageResult {
                    name: stage_def.name.clone(),
                    stage_type: "check".into(),
                    skipped: true,
                    skip_reason: Some(format!("Check failed: {}", e)),
                    detail: Value::Null,
                },
            }
        }
    }
}

fn generate_summary(stages: &[StageResult], scores: &HashMap<String, f64>) -> Option<String> {
    let total = stages.len();
    let skipped = stages.iter().filter(|s| s.skipped).count();
    let executed = total - skipped;

    let check_passes: Vec<&str> = scores
        .iter()
        .filter(|(k, &v)| k.starts_with("check_") && v > 0.5)
        .map(|(k, _)| k.as_str())
        .collect();
    let check_fails: Vec<&str> = scores
        .iter()
        .filter(|(k, &v)| k.starts_with("check_") && v <= 0.5)
        .map(|(k, _)| k.as_str())
        .collect();

    let mut summary = format!("{}/{} stages executed", executed, total);
    if !check_passes.is_empty() {
        summary.push_str(&format!(", {} checks passed", check_passes.len()));
    }
    if !check_fails.is_empty() {
        summary.push_str(&format!(
            ", {} checks FAILED ({})",
            check_fails.len(),
            check_fails.join(", ")
        ));
    }
    Some(summary)
}
