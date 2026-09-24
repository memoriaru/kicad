use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::composition::{Composition, ModuleInstance, NetDef, TopologyInputs};
use crate::{design, design_review, pipeline, skills, ComponentDb};

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PowerTreeRequest {
    pub vin: f64,
    pub outputs: Vec<RailSpec>,
    #[serde(default)]
    pub isolated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RailSpec {
    pub vout: f64,
    pub iout: f64,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeNode {
    pub id: String,
    pub topology: String,
    pub template_name: String,
    pub template_type: String,
    pub vin: f64,
    pub vout: f64,
    pub iout: f64,
    pub input_net: String,
    pub output_net: String,
    pub vin_source: VinSource,
    pub y_offset: f64,
    pub pipeline_passed: Option<usize>,
    pub pipeline_failed: Option<usize>,
    #[serde(default)]
    pub pipeline_outputs: HashMap<String, f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VinSource {
    GlobalInput,
    Cascade { source_id: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationIssue {
    pub severity: String,
    pub module_id: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationResult {
    pub valid: bool,
    pub issues: Vec<ValidationIssue>,
    pub load_summary: HashMap<String, LoadInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LoadInfo {
    pub own_iout: f64,
    pub downstream_iout: f64,
    pub total_demand: f64,
}

#[derive(Debug, Serialize)]
pub struct PowerTreeResult {
    pub request: PowerTreeRequest,
    pub tree: Vec<TreeNode>,
    pub validation: ValidationResult,
    pub review: design_review::DesignReviewResult,
    pub schematic: String,
    pub summary: String,
    pub incremental_info: Option<IncrementalInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PowerTreeCache {
    pub request: PowerTreeRequest,
    pub tree: Vec<TreeNode>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IncrementalInfo {
    pub modules_recomputed: usize,
    pub modules_cached: usize,
    pub changed_params: Vec<String>,
}

#[derive(Debug)]
pub struct ChangeSet {
    pub vin_changed: bool,
    pub output_count_changed: bool,
    pub changed_indices: Vec<usize>,
    pub affected_module_ids: Vec<String>,
}

// ---------------------------------------------------------------------------
// Net naming
// ---------------------------------------------------------------------------

pub fn voltage_to_net_name(voltage: f64) -> String {
    if voltage <= 0.0 {
        return "GND".to_string();
    }
    // Format: 5.0 → "5V", 3.3 → "3V3", 1.8 → "1V8", 12.0 → "12V"
    let is_integer = (voltage - voltage.round()).abs() < 1e-6;
    if is_integer {
        format!("{}V_RAIL", voltage as i64)
    } else {
        // For decimal voltages, multiply to find a clean representation
        let v_str = format!("{}", voltage);
        let clean = v_str.replace('.', "V");
        format!("{}_RAIL", clean)
    }
}

// ---------------------------------------------------------------------------
// Decomposition algorithm
// ---------------------------------------------------------------------------

pub fn decompose_power_tree(request: &PowerTreeRequest) -> Vec<TreeNode> {
    let mut tree = Vec::new();
    let mut intermediates: Vec<(f64, String)> = vec![(request.vin, "VIN".to_string())];

    let mut sorted_outputs: Vec<&RailSpec> = request.outputs.iter().collect();
    sorted_outputs.sort_by(|a, b| b.vout.partial_cmp(&a.vout).unwrap());

    for (idx, rail) in sorted_outputs.iter().enumerate() {
        let (vin_eff, input_net, vin_source) =
            find_best_input(request.vin, rail.vout, &intermediates);

        let candidates =
            skills::suggest_topologies(vin_eff, rail.vout, rail.iout, request.isolated);
        let topology = candidates
            .first()
            .map(|c| c.topology.clone())
            .unwrap_or_else(|| "buck".to_string());

        let out_net = rail
            .name
            .clone()
            .unwrap_or_else(|| voltage_to_net_name(rail.vout));

        let id = format!(
            "{}_{}",
            topology.to_lowercase(),
            out_net
                .to_lowercase()
                .replace("_rail", "")
                .replace(".", "v")
        );

        let node = TreeNode {
            id,
            topology: topology.clone(),
            template_name: topology.to_lowercase(),
            template_type: "topology".to_string(),
            vin: vin_eff,
            vout: rail.vout,
            iout: rail.iout,
            input_net: input_net.clone(),
            output_net: out_net.clone(),
            vin_source,
            y_offset: idx as f64 * 50.0,
            pipeline_passed: None,
            pipeline_failed: None,
            pipeline_outputs: HashMap::new(),
        };

        intermediates.push((rail.vout, out_net.clone()));
        tree.push(node);
    }

    tree
}

fn find_best_input(
    global_vin: f64,
    vout: f64,
    intermediates: &[(f64, String)],
) -> (f64, String, VinSource) {
    let mut best_vin = global_vin;
    let mut best_net = "VIN".to_string();
    let mut best_source = VinSource::GlobalInput;
    let mut best_diff = (global_vin - vout).abs();

    for (v, net_name) in intermediates {
        if *v > vout + 1.0 && *v <= global_vin {
            let diff = (*v - vout).abs();
            if diff < best_diff {
                best_diff = diff;
                best_vin = *v;
                best_net = net_name.clone();
                best_source = VinSource::Cascade {
                    source_id: net_name.to_lowercase().replace("_rail", ""),
                };
            }
        }
    }

    (best_vin, best_net, best_source)
}

// ---------------------------------------------------------------------------
// Interface validation + auto-fix
// ---------------------------------------------------------------------------

pub fn validate_tree(tree: &[TreeNode]) -> ValidationResult {
    let mut downstream_map: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, node) in tree.iter().enumerate() {
        if let VinSource::Cascade { .. } = &node.vin_source {
            downstream_map
                .entry(node.input_net.clone())
                .or_default()
                .push(i);
        }
    }

    let mut load_summary = HashMap::new();
    let mut issues = Vec::new();

    let total_demand_cache = compute_total_demands(tree, &downstream_map);

    for node in tree {
        let downstream_iout = total_demand_cache
            .get(&node.output_net)
            .copied()
            .unwrap_or(0.0);
        let total_demand = node.iout + downstream_iout;

        load_summary.insert(
            node.id.clone(),
            LoadInfo {
                own_iout: node.iout,
                downstream_iout,
                total_demand,
            },
        );

        if downstream_iout > 0.0 {
            issues.push(ValidationIssue {
                severity: "info".to_string(),
                module_id: node.id.clone(),
                message: format!(
                    "iout adjusted: {:.2}A → {:.2}A (own {:.2}A + downstream {:.2}A)",
                    node.iout, total_demand, node.iout, downstream_iout
                ),
            });
        }
    }

    let valid = true;
    ValidationResult {
        valid,
        issues,
        load_summary,
    }
}

pub fn fix_cascade_currents(tree: &mut [TreeNode]) {
    let mut downstream_map: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, node) in tree.iter().enumerate() {
        if let VinSource::Cascade { .. } = &node.vin_source {
            downstream_map
                .entry(node.input_net.clone())
                .or_default()
                .push(i);
        }
    }

    let mut cache: HashMap<String, f64> = HashMap::new();
    for node in tree.iter() {
        compute_demand_recursive(node, tree, &downstream_map, &mut cache);
    }

    // Update iout: add downstream demand to each module with dependents
    for node in tree.iter_mut() {
        if let Some(&downstream) = cache.get(&node.output_net) {
            if downstream > 0.0 {
                node.iout += downstream;
            }
        }
    }
}

fn compute_total_demands(
    tree: &[TreeNode],
    downstream_map: &HashMap<String, Vec<usize>>,
) -> HashMap<String, f64> {
    let mut cache: HashMap<String, f64> = HashMap::new();
    for node in tree {
        compute_demand_recursive(node, tree, downstream_map, &mut cache);
    }
    cache
}

fn compute_demand_recursive(
    node: &TreeNode,
    tree: &[TreeNode],
    downstream_map: &HashMap<String, Vec<usize>>,
    cache: &mut HashMap<String, f64>,
) -> f64 {
    if let Some(&total) = cache.get(&node.output_net) {
        return total;
    }

    let mut downstream_sum = 0.0;
    if let Some(dependents) = downstream_map.get(&node.output_net) {
        for &dep_idx in dependents {
            let dep_node = &tree[dep_idx];
            let dep_demand = compute_demand_recursive(dep_node, tree, downstream_map, cache);
            downstream_sum += dep_node.iout + dep_demand;
        }
    }

    cache.insert(node.output_net.clone(), downstream_sum);
    downstream_sum
}

// ---------------------------------------------------------------------------
// Build Composition from tree
// ---------------------------------------------------------------------------

pub fn build_composition(tree: &[TreeNode], request: &PowerTreeRequest) -> Composition {
    let mut modules = Vec::new();
    for node in tree {
        let mut params = HashMap::new();
        params.insert("vin".to_string(), node.vin);
        params.insert("vout".to_string(), node.vout);
        params.insert("iout".to_string(), node.iout);

        let mut nets = HashMap::new();
        nets.insert("VIN".to_string(), node.input_net.clone());
        nets.insert("VOUT".to_string(), node.output_net.clone());
        nets.insert("GND".to_string(), "GND".to_string());

        modules.push(ModuleInstance {
            id: node.id.clone(),
            template: node.template_name.clone(),
            template_type: node.template_type.clone(),
            params,
            nets,
            y_offset: Some(node.y_offset),
            topology_inputs: Some(TopologyInputs {
                vin: node.vin,
                vout: node.vout,
                iout: node.iout,
            }),
            computed_values: node.pipeline_outputs.clone(),
        });
    }

    let mut global_nets = vec![
        NetDef {
            name: "VIN".to_string(),
            net_type: Some("power".to_string()),
        },
        NetDef {
            name: "GND".to_string(),
            net_type: Some("power".to_string()),
        },
    ];
    for node in tree {
        if node.output_net != "VIN" && node.output_net != "GND" {
            global_nets.push(NetDef {
                name: node.output_net.clone(),
                net_type: Some("power".to_string()),
            });
        }
    }

    Composition {
        name: format!(
            "Power Tree: {}V → {} rails",
            request.vin,
            request.outputs.len()
        ),
        description: format!("Auto-composed power tree from {}V", request.vin),
        modules,
        global_nets,
    }
}

// ---------------------------------------------------------------------------
// Change detection + incremental update
// ---------------------------------------------------------------------------

pub fn detect_changes(
    old_request: &PowerTreeRequest,
    new_request: &PowerTreeRequest,
    old_tree: &[TreeNode],
) -> ChangeSet {
    let vin_changed = (old_request.vin - new_request.vin).abs() > 1e-6;
    let output_count_changed = old_request.outputs.len() != new_request.outputs.len();

    let mut changed_indices = Vec::new();
    let min_len = old_request.outputs.len().min(new_request.outputs.len());
    for i in 0..min_len {
        let old = &old_request.outputs[i];
        let new = &new_request.outputs[i];
        if (old.vout - new.vout).abs() > 1e-6 || (old.iout - new.iout).abs() > 1e-6 {
            changed_indices.push(i);
        }
    }
    // New outputs added are also "changed"
    for i in min_len..new_request.outputs.len() {
        changed_indices.push(i);
    }

    // Determine affected module IDs
    let mut affected = Vec::new();

    if vin_changed || output_count_changed {
        // Everything is affected
        affected.extend(old_tree.iter().map(|n| n.id.clone()));
    } else {
        // Find modules for changed output indices (by voltage match)
        for &idx in &changed_indices {
            let new_rail = &new_request.outputs[idx];
            if let Some(node) = old_tree
                .iter()
                .find(|n| (n.vout - new_rail.vout).abs() < 0.01)
            {
                affected.push(node.id.clone());
                // Also add upstream modules (cascade chain)
                add_upstream(old_tree, &node.input_net, &mut affected);
            }
        }
    }

    ChangeSet {
        vin_changed,
        output_count_changed,
        changed_indices,
        affected_module_ids: affected,
    }
}

fn add_upstream(tree: &[TreeNode], input_net: &str, affected: &mut Vec<String>) {
    if let Some(upstream) = tree.iter().find(|n| n.output_net == input_net) {
        if !affected.contains(&upstream.id) {
            affected.push(upstream.id.clone());
            add_upstream(tree, &upstream.input_net, affected);
        }
    }
}

fn requests_equal(a: &PowerTreeRequest, b: &PowerTreeRequest) -> bool {
    if (a.vin - b.vin).abs() > 1e-6
        || a.outputs.len() != b.outputs.len()
        || a.isolated != b.isolated
    {
        return false;
    }
    for (ao, bo) in a.outputs.iter().zip(b.outputs.iter()) {
        if (ao.vout - bo.vout).abs() > 1e-6 || (ao.iout - bo.iout).abs() > 1e-6 {
            return false;
        }
    }
    true
}

pub fn run_power_tree_with_cache(
    db: &ComponentDb,
    request: &PowerTreeRequest,
    cache: Option<&PowerTreeCache>,
) -> Result<PowerTreeResult> {
    if let Some(cached) = cache {
        if requests_equal(&cached.request, request) {
            // Exact match — reuse tree directly (already fixed)
            let composition = build_composition(&cached.tree, request);
            let review = design_review::review_composition(&composition, &cached.tree);
            let schematic = design::generate_composed_schematic(db, &composition)?;
            let validation = ValidationResult {
                valid: true,
                issues: vec![ValidationIssue {
                    severity: "info".to_string(),
                    module_id: "-".to_string(),
                    message: "Result loaded from cache (no recomputation needed)".to_string(),
                }],
                load_summary: HashMap::new(),
            };
            let summary = format_power_tree_summary(&cached.tree, request, &validation, &review);
            return Ok(PowerTreeResult {
                request: request.clone(),
                tree: cached.tree.clone(),
                validation,
                review,
                schematic,
                summary,
                incremental_info: Some(IncrementalInfo {
                    modules_recomputed: 0,
                    modules_cached: cached.tree.len(),
                    changed_params: vec![],
                }),
            });
        }

        // Partial match — incremental update
        let changes = detect_changes(&cached.request, request, &cached.tree);

        if changes.affected_module_ids.is_empty() {
            // No changes detected but requests differ (unlikely)
            return run_power_tree(db, request);
        }

        // Re-decompose fully (tree structure may change)
        let mut tree = decompose_power_tree(request);
        let validation = validate_tree(&tree);
        fix_cascade_currents(&mut tree);

        // Selective pipeline: only run for affected modules
        let mut modules_recomputed = 0;
        let mut modules_cached = 0;

        for node in &mut tree {
            if changes.affected_module_ids.contains(&node.id) {
                // Re-run pipeline for this module
                if let Some(pipeline) = pipeline::get_builtin_pipeline(&node.topology) {
                    let mut params = HashMap::new();
                    params.insert("vin".to_string(), node.vin);
                    params.insert("vout".to_string(), node.vout);
                    params.insert("iout".to_string(), node.iout);
                    params.entry("fsw".to_string()).or_insert(500000.0);
                    params.entry("ripple_ratio".to_string()).or_insert(0.3);
                    params.entry("ripple_v".to_string()).or_insert(0.05);

                    if let Ok(log) = pipeline::run_pipeline(db, &pipeline, &params) {
                        node.pipeline_passed = Some(log.passed);
                        node.pipeline_failed = Some(log.failed);
                    }
                }
                modules_recomputed += 1;
            } else {
                // Copy pipeline results from cached tree
                if let Some(cached_node) = cached.tree.iter().find(|cn| cn.id == node.id) {
                    node.pipeline_passed = cached_node.pipeline_passed;
                    node.pipeline_failed = cached_node.pipeline_failed;
                }
                modules_cached += 1;
            }
        }

        let composition = build_composition(&tree, request);
        let review = design_review::review_composition(&composition, &tree);
        let schematic = design::generate_composed_schematic(db, &composition)?;
        let summary = format_power_tree_summary(&tree, request, &validation, &review);

        let mut changed_params = Vec::new();
        if changes.vin_changed {
            changed_params.push("vin".to_string());
        }
        for &idx in &changes.changed_indices {
            changed_params.push(format!("outputs[{}]", idx));
        }

        return Ok(PowerTreeResult {
            request: request.clone(),
            tree,
            validation,
            review,
            schematic,
            summary,
            incremental_info: Some(IncrementalInfo {
                modules_recomputed,
                modules_cached,
                changed_params,
            }),
        });
    }

    // No cache — full generation
    let mut result = run_power_tree(db, request)?;
    result.incremental_info = Some(IncrementalInfo {
        modules_recomputed: result.tree.len(),
        modules_cached: 0,
        changed_params: vec!["all".to_string()],
    });
    Ok(result)
}

// ---------------------------------------------------------------------------
// Orchestrator (full generation)
// ---------------------------------------------------------------------------

pub fn run_power_tree(db: &ComponentDb, request: &PowerTreeRequest) -> Result<PowerTreeResult> {
    let mut tree = decompose_power_tree(request);

    let validation = validate_tree(&tree);

    // Auto-fix: update iout for modules with downstream dependents
    fix_cascade_currents(&mut tree);

    for node in &mut tree {
        if let Some(pipeline) = pipeline::get_builtin_pipeline(&node.topology) {
            let mut params = HashMap::new();
            params.insert("vin".to_string(), node.vin);
            params.insert("vout".to_string(), node.vout);
            params.insert("iout".to_string(), node.iout);
            params.entry("fsw".to_string()).or_insert(500000.0);
            params.entry("ripple_ratio".to_string()).or_insert(0.3);
            params.entry("ripple_v".to_string()).or_insert(0.05);
            // Feedback network defaults (typical buck/boost controller)
            params.entry("vref".to_string()).or_insert(0.8);
            params.entry("r_fb2".to_string()).or_insert(10000.0);

            if let Ok(log) = pipeline::run_pipeline(db, &pipeline, &params) {
                node.pipeline_passed = Some(log.passed);
                node.pipeline_failed = Some(log.failed);
                // Collect all pipeline step outputs for schematic generation
                for step in &log.steps {
                    for (name, val) in &step.outputs {
                        node.pipeline_outputs.insert(name.clone(), *val);
                    }
                }
            }
        }
    }

    let composition = build_composition(&tree, request);
    let review = design_review::review_composition(&composition, &tree);
    let schematic = design::generate_composed_schematic(db, &composition)?;
    let summary = format_power_tree_summary(&tree, request, &validation, &review);

    Ok(PowerTreeResult {
        request: request.clone(),
        tree,
        validation,
        review,
        schematic,
        summary,
        incremental_info: None,
    })
}

fn format_power_tree_summary(
    tree: &[TreeNode],
    request: &PowerTreeRequest,
    validation: &ValidationResult,
    review: &design_review::DesignReviewResult,
) -> String {
    let mut lines = Vec::new();
    lines.push(format!(
        "Power Tree: {}V → {} rails",
        request.vin,
        request.outputs.len()
    ));
    lines.push(String::new());

    for node in tree {
        let cascade = match &node.vin_source {
            VinSource::Cascade { source_id } => format!(" (from {})", source_id),
            VinSource::GlobalInput => String::new(),
        };
        let load_info = validation.load_summary.get(&node.id);
        let load_str = load_info
            .filter(|info| info.downstream_iout > 0.0)
            .map(|info| format!(" [total demand: {:.2}A]", info.total_demand))
            .unwrap_or_default();
        lines.push(format!(
            "  {} [{}] {}V → {}V @ {}A{}{}",
            node.id, node.topology, node.vin, node.vout, node.iout, cascade, load_str
        ));
        if let (Some(p), Some(f)) = (node.pipeline_passed, node.pipeline_failed) {
            lines.push(format!("    Pipeline: {} passed, {} failed", p, f));
        }
    }

    if !validation.issues.is_empty() {
        lines.push(String::new());
        lines.push(format!("Validation: {} issues", validation.issues.len()));
        for issue in &validation.issues {
            let tag = match issue.severity.as_str() {
                "error" => "ERROR",
                "warning" => "WARN",
                _ => "INFO",
            };
            lines.push(format!(
                "  [{}] {}: {}",
                tag, issue.module_id, issue.message
            ));
        }
    } else {
        lines.push(String::new());
        lines.push("Validation: passed".to_string());
    }

    // Design review summary
    lines.push(String::new());
    if review.issues.is_empty() {
        lines.push(format!(
            "Design Review: {} checks, all passed",
            review.checks_run
        ));
    } else {
        let errors = review
            .issues
            .iter()
            .filter(|i| i.severity == "error")
            .count();
        let warnings = review
            .issues
            .iter()
            .filter(|i| i.severity == "warning")
            .count();
        let infos = review
            .issues
            .iter()
            .filter(|i| i.severity == "info")
            .count();
        lines.push(format!(
            "Design Review: {} checks, {} errors, {} warnings, {} info",
            review.checks_run, errors, warnings, infos
        ));
        for issue in &review.issues {
            let tag = match issue.severity.as_str() {
                "error" => "ERROR",
                "warning" => "WARN",
                _ => "INFO",
            };
            let module_str = issue.module_id.as_deref().unwrap_or("-");
            lines.push(format!(
                "  [{}][{}] {}: {}",
                tag, issue.category, module_str, issue.message
            ));
        }
    }

    lines.join("\n")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_voltage_to_net_name() {
        assert_eq!(voltage_to_net_name(5.0), "5V_RAIL");
        assert_eq!(voltage_to_net_name(3.3), "3V3_RAIL");
        assert_eq!(voltage_to_net_name(1.8), "1V8_RAIL");
        assert_eq!(voltage_to_net_name(12.0), "12V_RAIL");
    }

    #[test]
    fn test_decompose_single_rail() {
        let request = PowerTreeRequest {
            vin: 12.0,
            outputs: vec![RailSpec {
                vout: 5.0,
                iout: 2.0,
                name: None,
            }],
            isolated: false,
        };
        let tree = decompose_power_tree(&request);
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].topology, "buck");
        assert_eq!(tree[0].input_net, "VIN");
    }

    #[test]
    fn test_decompose_cascade() {
        let request = PowerTreeRequest {
            vin: 12.0,
            outputs: vec![
                RailSpec {
                    vout: 5.0,
                    iout: 2.0,
                    name: None,
                },
                RailSpec {
                    vout: 3.3,
                    iout: 1.0,
                    name: None,
                },
            ],
            isolated: false,
        };
        let tree = decompose_power_tree(&request);
        assert_eq!(tree.len(), 2);

        // 5V module should come from VIN
        let buck = tree.iter().find(|n| (n.vout - 5.0).abs() < 0.01).unwrap();
        assert_eq!(buck.input_net, "VIN");

        // 3.3V module should cascade from 5V
        let ldo = tree.iter().find(|n| (n.vout - 3.3).abs() < 0.01).unwrap();
        assert!(matches!(ldo.vin_source, VinSource::Cascade { .. }));
        assert_eq!(ldo.input_net, "5V_RAIL");
    }

    #[test]
    fn test_build_composition() {
        let request = PowerTreeRequest {
            vin: 12.0,
            outputs: vec![
                RailSpec {
                    vout: 5.0,
                    iout: 2.0,
                    name: None,
                },
                RailSpec {
                    vout: 3.3,
                    iout: 1.0,
                    name: None,
                },
            ],
            isolated: false,
        };
        let tree = decompose_power_tree(&request);
        let comp = build_composition(&tree, &request);

        assert_eq!(comp.modules.len(), 2);
        assert_eq!(comp.global_nets.len(), 4); // VIN, GND, 5V_RAIL, 3V3_RAIL
    }

    #[test]
    fn test_validate_single_rail_ok() {
        let request = PowerTreeRequest {
            vin: 12.0,
            outputs: vec![RailSpec {
                vout: 5.0,
                iout: 2.0,
                name: None,
            }],
            isolated: false,
        };
        let tree = decompose_power_tree(&request);
        let result = validate_tree(&tree);
        assert!(result.valid);
        assert!(result.issues.is_empty());
    }

    #[test]
    fn test_validate_cascade_info_reported() {
        // 5V/2A feeds 3.3V/1A: cascade detected, info reported
        let request = PowerTreeRequest {
            vin: 12.0,
            outputs: vec![
                RailSpec {
                    vout: 5.0,
                    iout: 2.0,
                    name: None,
                },
                RailSpec {
                    vout: 3.3,
                    iout: 1.0,
                    name: None,
                },
            ],
            isolated: false,
        };
        let tree = decompose_power_tree(&request);
        let result = validate_tree(&tree);

        assert!(result.valid);
        let infos = result
            .issues
            .iter()
            .filter(|i| i.severity == "info")
            .count();
        assert_eq!(infos, 1); // 5V module has downstream

        let buck_5v = result
            .load_summary
            .values()
            .find(|info| info.downstream_iout > 0.0)
            .unwrap();
        assert!((buck_5v.downstream_iout - 1.0).abs() < 0.01);
        assert!((buck_5v.total_demand - 3.0).abs() < 0.01);
    }

    #[test]
    fn test_fix_cascade_currents() {
        let request = PowerTreeRequest {
            vin: 12.0,
            outputs: vec![
                RailSpec {
                    vout: 5.0,
                    iout: 2.0,
                    name: None,
                },
                RailSpec {
                    vout: 3.3,
                    iout: 1.0,
                    name: None,
                },
            ],
            isolated: false,
        };
        let mut tree = decompose_power_tree(&request);

        // Before fix: 5V module iout = 2A
        let buck_5v = tree.iter().find(|n| (n.vout - 5.0).abs() < 0.01).unwrap();
        assert!((buck_5v.iout - 2.0).abs() < 0.01);

        fix_cascade_currents(&mut tree);

        // After fix: 5V module iout = 2 + 1 (downstream) = 3A
        let buck_5v = tree.iter().find(|n| (n.vout - 5.0).abs() < 0.01).unwrap();
        assert!((buck_5v.iout - 3.0).abs() < 0.01);

        // 3.3V module unchanged
        let buck_3v3 = tree.iter().find(|n| (n.vout - 3.3).abs() < 0.01).unwrap();
        assert!((buck_3v3.iout - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_validate_multi_level_cascade() {
        // 12V → 5V/3A → 3.3V/1A → 1.8V/0.5A
        let request = PowerTreeRequest {
            vin: 12.0,
            outputs: vec![
                RailSpec {
                    vout: 5.0,
                    iout: 3.0,
                    name: None,
                },
                RailSpec {
                    vout: 3.3,
                    iout: 1.0,
                    name: None,
                },
                RailSpec {
                    vout: 1.8,
                    iout: 0.5,
                    name: None,
                },
            ],
            isolated: false,
        };
        let tree = decompose_power_tree(&request);
        let result = validate_tree(&tree);

        assert!(result.valid);
        // 3.3V has downstream 0.5A → info reported
        // 5V has downstream (1.0 + 0.5) = 1.5A → info reported
        let infos = result
            .issues
            .iter()
            .filter(|i| i.severity == "info")
            .count();
        assert!(infos >= 1);
    }

    #[test]
    fn test_fix_multi_level_cascade_currents() {
        let request = PowerTreeRequest {
            vin: 12.0,
            outputs: vec![
                RailSpec {
                    vout: 5.0,
                    iout: 3.0,
                    name: None,
                },
                RailSpec {
                    vout: 3.3,
                    iout: 1.0,
                    name: None,
                },
                RailSpec {
                    vout: 1.8,
                    iout: 0.5,
                    name: None,
                },
            ],
            isolated: false,
        };
        let mut tree = decompose_power_tree(&request);
        fix_cascade_currents(&mut tree);

        // 3.3V: own=1A + downstream(0.5A) = 1.5A
        let buck_3v3 = tree.iter().find(|n| (n.vout - 3.3).abs() < 0.01).unwrap();
        assert!((buck_3v3.iout - 1.5).abs() < 0.01);

        // 5V: own=3A + downstream(1.0+0.5=1.5A) = 4.5A
        let buck_5v = tree.iter().find(|n| (n.vout - 5.0).abs() < 0.01).unwrap();
        assert!((buck_5v.iout - 4.5).abs() < 0.01);
    }

    #[test]
    fn test_detect_changes_no_change() {
        let request = PowerTreeRequest {
            vin: 12.0,
            outputs: vec![
                RailSpec {
                    vout: 5.0,
                    iout: 2.0,
                    name: None,
                },
                RailSpec {
                    vout: 3.3,
                    iout: 1.0,
                    name: None,
                },
            ],
            isolated: false,
        };
        let tree = decompose_power_tree(&request);
        let changes = detect_changes(&request, &request, &tree);
        assert!(!changes.vin_changed);
        assert!(!changes.output_count_changed);
        assert!(changes.changed_indices.is_empty());
        assert!(changes.affected_module_ids.is_empty());
    }

    #[test]
    fn test_detect_changes_vin_changed() {
        let old = PowerTreeRequest {
            vin: 12.0,
            outputs: vec![RailSpec {
                vout: 5.0,
                iout: 2.0,
                name: None,
            }],
            isolated: false,
        };
        let new = PowerTreeRequest {
            vin: 24.0,
            outputs: vec![RailSpec {
                vout: 5.0,
                iout: 2.0,
                name: None,
            }],
            isolated: false,
        };
        let tree = decompose_power_tree(&old);
        let changes = detect_changes(&old, &new, &tree);
        assert!(changes.vin_changed);
        assert_eq!(changes.affected_module_ids.len(), tree.len());
    }

    #[test]
    fn test_detect_changes_single_output() {
        let old = PowerTreeRequest {
            vin: 12.0,
            outputs: vec![
                RailSpec {
                    vout: 5.0,
                    iout: 2.0,
                    name: None,
                },
                RailSpec {
                    vout: 3.3,
                    iout: 1.0,
                    name: None,
                },
            ],
            isolated: false,
        };
        let new = PowerTreeRequest {
            vin: 12.0,
            outputs: vec![
                RailSpec {
                    vout: 5.0,
                    iout: 2.0,
                    name: None,
                },
                RailSpec {
                    vout: 3.3,
                    iout: 2.0,
                    name: None,
                }, // iout changed
            ],
            isolated: false,
        };
        let tree = decompose_power_tree(&old);
        let changes = detect_changes(&old, &new, &tree);
        assert!(!changes.vin_changed);
        assert_eq!(changes.changed_indices.len(), 1);
        // 3.3V module and its upstream (5V) should be affected
        assert!(!changes.affected_module_ids.is_empty());
    }

    #[test]
    fn test_requests_equal() {
        let a = PowerTreeRequest {
            vin: 12.0,
            outputs: vec![RailSpec {
                vout: 5.0,
                iout: 2.0,
                name: None,
            }],
            isolated: false,
        };
        let b = PowerTreeRequest {
            vin: 12.0,
            outputs: vec![RailSpec {
                vout: 5.0,
                iout: 2.0,
                name: None,
            }],
            isolated: false,
        };
        assert!(requests_equal(&a, &b));

        let c = PowerTreeRequest {
            vin: 12.0,
            outputs: vec![RailSpec {
                vout: 5.0,
                iout: 3.0,
                name: None,
            }],
            isolated: false,
        };
        assert!(!requests_equal(&a, &c));
    }

    #[test]
    fn test_cache_roundtrip() {
        let request = PowerTreeRequest {
            vin: 12.0,
            outputs: vec![RailSpec {
                vout: 5.0,
                iout: 2.0,
                name: None,
            }],
            isolated: false,
        };
        let tree = decompose_power_tree(&request);
        let cache = PowerTreeCache {
            request: request.clone(),
            tree: tree.clone(),
        };

        let json = serde_json::to_string(&cache).unwrap();
        let restored: PowerTreeCache = serde_json::from_str(&json).unwrap();

        assert!((restored.request.vin - 12.0).abs() < 0.01);
        assert_eq!(restored.tree.len(), 1);
        assert_eq!(restored.tree[0].topology, tree[0].topology);
    }
}
