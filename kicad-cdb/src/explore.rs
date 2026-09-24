use anyhow::Result;
use serde::Serialize;
use std::collections::HashMap;

use crate::ComponentDb;

#[derive(Debug, Serialize)]
pub struct ExplorationResult {
    pub requirements: DesignRequirements,
    pub candidates: Vec<DesignCandidate>,
    pub ranking_criteria: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct DesignRequirements {
    pub vin: f64,
    pub vout: f64,
    pub iout: f64,
    pub isolated: bool,
}

#[derive(Debug, Serialize)]
pub struct DesignCandidate {
    pub topology: String,
    pub pipeline_name: Option<String>,
    pub viable: bool,
    pub fail_reason: Option<String>,
    pub scores: DesignScores,
    pub key_params: HashMap<String, f64>,
    pub design_log: Option<crate::pipeline::DesignLog>,
}

#[derive(Debug, Serialize, Default)]
pub struct DesignScores {
    pub efficiency: f64,
    pub complexity: f64,
    pub thermal: f64,
    pub cost_estimate: f64,
    pub overall: f64,
}

#[derive(Debug, Serialize)]
pub struct ScoringWeights {
    pub efficiency: f64,
    pub complexity: f64,
    pub thermal: f64,
    pub cost: f64,
}

impl Default for ScoringWeights {
    fn default() -> Self {
        ScoringWeights {
            efficiency: 0.4,
            complexity: 0.2,
            thermal: 0.2,
            cost: 0.2,
        }
    }
}

fn topology_to_pipeline(topology: &str) -> Option<&'static str> {
    match topology {
        "buck" => Some("buck"),
        "boost" => Some("boost"),
        "ldo" => Some("ldo"),
        "led" => Some("led"),
        _ => None,
    }
}

fn complexity_score(topology: &str) -> f64 {
    match topology {
        "ldo" => 0.1, // simplest
        "led" => 0.1,
        "chargepump" => 0.2,
        "buck" => 0.3,
        "boost" => 0.35,
        "buckboost" => 0.45,
        "sepic" => 0.5,
        "inverting" => 0.5,
        "flyback" => 0.7, // most complex
        _ => 0.4,
    }
}

fn cost_score(topology: &str) -> f64 {
    match topology {
        "ldo" => 0.1,
        "led" => 0.1,
        "chargepump" => 0.15,
        "buck" => 0.25,
        "boost" => 0.25,
        "buckboost" => 0.35,
        "sepic" => 0.4,
        "inverting" => 0.35,
        "flyback" => 0.5,
        _ => 0.3,
    }
}

fn score_candidate(
    topology: &str,
    log: Option<&crate::pipeline::DesignLog>,
    vin: f64,
    vout: f64,
    _iout: f64,
    est_efficiency: f64,
    weights: &ScoringWeights,
) -> DesignScores {
    let efficiency = est_efficiency;

    let complexity = complexity_score(topology);

    let thermal = log
        .map(|_l| {
            let p_diss = (vin - vout) * (1.0 - efficiency) / efficiency.max(0.01);
            (p_diss / 10.0).min(1.0)
        })
        .unwrap_or(complexity * 0.8);

    let cost = cost_score(topology);

    let overall = efficiency * weights.efficiency
        + (1.0 - complexity) * weights.complexity
        + (1.0 - thermal) * weights.thermal
        + (1.0 - cost) * weights.cost;

    DesignScores {
        efficiency,
        complexity,
        thermal,
        cost_estimate: cost,
        overall,
    }
}

/// Explore design space: try all applicable topologies, run pipelines, score, rank.
pub fn explore(
    db: &ComponentDb,
    vin: f64,
    vout: f64,
    iout: f64,
    isolated: bool,
    weights: Option<ScoringWeights>,
) -> Result<ExplorationResult> {
    let weights = weights.unwrap_or_default();
    let suggestions = crate::skills::suggest_topologies(vin, vout, iout, isolated);

    let mut candidates = Vec::new();

    for sugg in &suggestions {
        let pipeline_name = topology_to_pipeline(&sugg.topology);

        let (viable, fail_reason, design_log, key_params) = if let Some(pl_name) = pipeline_name {
            if let Some(pipeline) = crate::pipeline::get_builtin_pipeline(pl_name) {
                let mut params = HashMap::new();
                params.insert("vin".into(), vin);
                params.insert("vout".into(), vout);
                params.insert("iout".into(), iout);

                // Add defaults for missing required params
                for input_name in &pipeline.user_inputs {
                    if !params.contains_key(input_name) {
                        match input_name.as_str() {
                            "fsw" => {
                                params.insert("fsw".into(), 500000.0);
                            }
                            "ripple_ratio" => {
                                params.insert("ripple_ratio".into(), 0.3);
                            }
                            "ripple_v" => {
                                params.insert("ripple_v".into(), 0.05);
                            }
                            "vdropout_max" => {
                                params.insert("vdropout_max".into(), 0.5);
                            }
                            "p_max" => {
                                params.insert("p_max".into(), 1.0);
                            }
                            "eff_min" => {
                                params.insert("eff_min".into(), 0.5);
                            }
                            "vf" => {
                                params.insert("vf".into(), 2.0);
                            }
                            "r_power" => {
                                params.insert("r_power".into(), 0.25);
                            }
                            _ => {}
                        }
                    }
                }

                match crate::pipeline::run_pipeline(db, &pipeline, &params) {
                    Ok(log) => {
                        let mut kp = HashMap::new();
                        for step in &log.steps {
                            for (name, val) in &step.outputs {
                                kp.insert(name.clone(), *val);
                            }
                        }
                        (true, None, Some(log), kp)
                    }
                    Err(e) => (false, Some(e.to_string()), None, HashMap::new()),
                }
            } else {
                (true, None, None, HashMap::new())
            }
        } else {
            (true, None, None, HashMap::new())
        };

        let scores = score_candidate(
            &sugg.topology,
            design_log.as_ref(),
            vin,
            vout,
            iout,
            sugg.estimated_efficiency,
            &weights,
        );

        candidates.push(DesignCandidate {
            topology: sugg.topology.clone(),
            pipeline_name: pipeline_name.map(|s| s.to_string()),
            viable,
            fail_reason,
            scores,
            key_params,
            design_log,
        });
    }

    // Sort by overall score descending
    candidates.sort_by(|a, b| {
        b.scores
            .overall
            .partial_cmp(&a.scores.overall)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    Ok(ExplorationResult {
        requirements: DesignRequirements {
            vin,
            vout,
            iout,
            isolated,
        },
        candidates,
        ranking_criteria: vec![
            "efficiency".into(),
            "complexity".into(),
            "thermal".into(),
            "cost".into(),
        ],
    })
}
