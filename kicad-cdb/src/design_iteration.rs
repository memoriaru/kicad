use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::composition::{Composition, ModuleInstance, TopologyInputs};
use crate::power_tree::{self, TreeNode};
use crate::requirement::DesignSpec;
use crate::schema;

// ── Data Structures ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesignSnapshot {
    pub id: String,
    pub name: String,
    pub version: u32,
    pub created_at: String,
    pub spec: DesignSpec,
    pub composition: Composition,
    #[serde(default)]
    pub power_tree_nodes: Vec<TreeNode>,
    #[serde(default)]
    pub module_pipeline_results: HashMap<String, HashMap<String, f64>>,
    #[serde(default)]
    pub schematic: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DesignDiff {
    pub changed_params: Vec<ParamChange>,
    pub affected_modules: Vec<String>,
    pub unaffected_modules: Vec<String>,
    pub modules_recomputed: usize,
    pub modules_cached: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ParamChange {
    pub name: String,
    pub old_value: f64,
    pub new_value: f64,
}

// ── Snapshot Persistence ─────────────────────────────────────────

fn get_connection() -> Result<rusqlite::Connection> {
    let db_path = std::env::var("CDB_PATH").unwrap_or_else(|_| "components.db".to_string());
    let conn = rusqlite::Connection::open(&db_path)
        .with_context(|| format!("Failed to open DB: {}", db_path))?;
    schema::run_migrations(&conn)?;
    Ok(conn)
}

pub fn save_snapshot(snapshot: &DesignSnapshot) -> Result<()> {
    let conn = get_connection()?;
    let spec_json = serde_json::to_string(&snapshot.spec)?;
    let comp_json = serde_json::to_string(&snapshot.composition)?;
    let tree_json = if snapshot.power_tree_nodes.is_empty() {
        None
    } else {
        Some(serde_json::to_string(&snapshot.power_tree_nodes)?)
    };
    let cache_json = if snapshot.module_pipeline_results.is_empty() {
        None
    } else {
        Some(serde_json::to_string(&snapshot.module_pipeline_results)?)
    };
    let sch_json = snapshot.schematic.clone();

    conn.execute(
        "INSERT OR IGNORE INTO design_snapshots (id, name, version, spec_json, composition_json, power_tree_json, pipeline_cache_json, schematic_json, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, datetime('now'))",
        rusqlite::params![
            snapshot.id,
            snapshot.name,
            snapshot.version,
            spec_json,
            comp_json,
            tree_json,
            cache_json,
            sch_json,
            snapshot.created_at,
        ],
    )?;
    Ok(())
}

pub fn load_snapshot(id: &str) -> Result<DesignSnapshot> {
    // H1: composite PK (id, version) — load the latest version for this id.
    let conn = get_connection()?;
    let result = conn.query_row(
        "SELECT id, name, version, spec_json, composition_json, power_tree_json, pipeline_cache_json, schematic_json, created_at FROM design_snapshots WHERE id = ?1 ORDER BY version DESC LIMIT 1",
        rusqlite::params![id],
        |row| {
            let id: String = row.get(0)?;
            let name: String = row.get(1)?;
            let version: u32 = row.get(2)?;
            let spec_json: String = row.get(3)?;
            let comp_json: String = row.get(4)?;
            let tree_json: Option<String> = row.get(5)?;
            let cache_json: Option<String> = row.get(6)?;
            let sch_json: Option<String> = row.get(7)?;
            let created_at: String = row.get(8)?;
            Ok((id, name, version, spec_json, comp_json, tree_json, cache_json, sch_json, created_at))
        },
    ).with_context(|| format!("Design snapshot '{}' not found", id))?;

    let (id, name, version, spec_json, comp_json, tree_json, cache_json, sch_json, created_at) =
        result;
    let spec: DesignSpec = serde_json::from_str(&spec_json)?;
    let composition: Composition = serde_json::from_str(&comp_json)?;
    let power_tree_nodes: Vec<TreeNode> = tree_json
        .map(|s| serde_json::from_str(&s))
        .transpose()?
        .unwrap_or_default();
    let module_pipeline_results: HashMap<String, HashMap<String, f64>> = cache_json
        .map(|s| serde_json::from_str(&s))
        .transpose()?
        .unwrap_or_default();

    Ok(DesignSnapshot {
        id,
        name,
        version,
        created_at,
        spec,
        composition,
        power_tree_nodes,
        module_pipeline_results,
        schematic: sch_json,
    })
}

pub fn list_snapshots() -> Result<Vec<(String, String, u32, String)>> {
    // H1: one row per (id, version). Deduplicate to latest version per id so
    // the top-level listing stays a design list, not a version list.
    let conn = get_connection()?;
    let mut stmt = conn.prepare(
        "SELECT id, name, version, updated_at FROM design_snapshots
         WHERE version = (SELECT MAX(version) FROM design_snapshots s2 WHERE s2.id = design_snapshots.id)
         ORDER BY updated_at DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
    })?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

/// H1: List all versions of a design, newest first.
/// Returns (version, created_at, updated_at) tuples.
pub fn list_snapshot_versions(id: &str) -> Result<Vec<(u32, String, String)>> {
    let conn = get_connection()?;
    let mut stmt = conn.prepare(
        "SELECT version, created_at, updated_at FROM design_snapshots WHERE id = ?1 ORDER BY version DESC",
    )?;
    let rows = stmt.query_map(rusqlite::params![id], |row| {
        Ok((row.get::<_, i64>(0)? as u32, row.get(1)?, row.get(2)?))
    })?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

/// H1: Load a specific version of a design (for rollback).
pub fn load_snapshot_version(id: &str, version: u32) -> Result<DesignSnapshot> {
    let conn = get_connection()?;
    let result = conn.query_row(
        "SELECT id, name, version, spec_json, composition_json, power_tree_json, pipeline_cache_json, schematic_json, created_at FROM design_snapshots WHERE id = ?1 AND version = ?2",
        rusqlite::params![id, version],
        |row| {
            let id: String = row.get(0)?;
            let name: String = row.get(1)?;
            let version: u32 = row.get(2)?;
            let spec_json: String = row.get(3)?;
            let comp_json: String = row.get(4)?;
            let tree_json: Option<String> = row.get(5)?;
            let cache_json: Option<String> = row.get(6)?;
            let sch_json: Option<String> = row.get(7)?;
            let created_at: String = row.get(8)?;
            Ok((id, name, version, spec_json, comp_json, tree_json, cache_json, sch_json, created_at))
        },
    ).with_context(|| format!("Design snapshot '{} v{}' not found", id, version))?;

    let (id, name, version, spec_json, comp_json, tree_json, cache_json, sch_json, created_at) =
        result;
    let spec: DesignSpec = serde_json::from_str(&spec_json)?;
    let composition: Composition = serde_json::from_str(&comp_json)?;
    let power_tree_nodes: Vec<TreeNode> = tree_json
        .map(|s| serde_json::from_str(&s))
        .transpose()?
        .unwrap_or_default();
    let module_pipeline_results: HashMap<String, HashMap<String, f64>> = cache_json
        .map(|s| serde_json::from_str(&s))
        .transpose()?
        .unwrap_or_default();

    Ok(DesignSnapshot {
        id,
        name,
        version,
        created_at,
        spec,
        composition,
        power_tree_nodes,
        module_pipeline_results,
        schematic: sch_json,
    })
}

// ── Change Detection ─────────────────────────────────────────────

fn apply_param_changes(spec: &mut DesignSpec, changes: &HashMap<String, f64>) -> Vec<ParamChange> {
    let mut param_changes = Vec::new();

    match spec {
        DesignSpec::PowerBoard(ref mut p) => {
            if let Some(&new_vin) = changes.get("vin") {
                if (p.vin - new_vin).abs() > 1e-6 {
                    param_changes.push(ParamChange {
                        name: "vin".into(),
                        old_value: p.vin,
                        new_value: new_vin,
                    });
                    p.vin = new_vin;
                }
            }
            for (i, out) in p.outputs.iter_mut().enumerate() {
                if let Some(&new_vout) = changes.get(&format!("output.{}.vout", i)) {
                    if (out.vout - new_vout).abs() > 1e-6 {
                        param_changes.push(ParamChange {
                            name: format!("output.{}.vout", i),
                            old_value: out.vout,
                            new_value: new_vout,
                        });
                        out.vout = new_vout;
                    }
                }
                if let Some(&new_iout) = changes.get(&format!("output.{}.iout", i)) {
                    if (out.iout - new_iout).abs() > 1e-6 {
                        param_changes.push(ParamChange {
                            name: format!("output.{}.iout", i),
                            old_value: out.iout,
                            new_value: new_iout,
                        });
                        out.iout = new_iout;
                    }
                }
            }
        }
        DesignSpec::IcBoard(ref mut ic) => {
            if let Some(&new_vin) = changes.get("power_input_voltage") {
                if (ic.power_input_voltage - new_vin).abs() > 1e-6 {
                    param_changes.push(ParamChange {
                        name: "power_input_voltage".into(),
                        old_value: ic.power_input_voltage,
                        new_value: new_vin,
                    });
                    ic.power_input_voltage = new_vin;
                }
            }
            for (i, ic_req) in ic.core_ics.iter_mut().enumerate() {
                for (key, &new_val) in changes {
                    if key.starts_with(&format!("ic.{}.", i)) {
                        let param_name = &key[format!("ic.{}.", i).len()..];
                        let old_val = ic_req.parameters.get(param_name).copied().unwrap_or(0.0);
                        if (old_val - new_val).abs() > 1e-6 {
                            param_changes.push(ParamChange {
                                name: key.clone(),
                                old_value: old_val,
                                new_value: new_val,
                            });
                            ic_req.parameters.insert(param_name.to_string(), new_val);
                        }
                    }
                }
            }
        }
        DesignSpec::MixedBoard(ref mut m) => {
            // Apply to both sub-specs
            let ic_changes: HashMap<String, f64> = changes
                .iter()
                .filter(|(k, _)| k.starts_with("power_input_voltage") || k.starts_with("ic."))
                .map(|(k, &v)| (k.clone(), v))
                .collect();
            let power_changes: HashMap<String, f64> = changes
                .iter()
                .filter(|(k, _)| k.starts_with("vin") || k.starts_with("output."))
                .map(|(k, &v)| (k.clone(), v))
                .collect();
            let mut ic_spec = DesignSpec::IcBoard(m.ic_board.clone());
            let mut power_spec = DesignSpec::PowerBoard(m.power.clone());
            param_changes.extend(apply_param_changes(&mut ic_spec, &ic_changes));
            param_changes.extend(apply_param_changes(&mut power_spec, &power_changes));
            if let DesignSpec::IcBoard(updated) = ic_spec {
                m.ic_board = updated;
            }
            if let DesignSpec::PowerBoard(updated) = power_spec {
                m.power = updated;
            }
        }
    }
    param_changes
}

fn detect_affected_modules(_spec: &DesignSpec, param_changes: &[ParamChange]) -> Vec<String> {
    let mut affected = Vec::new();

    // Input voltage changes affect all power modules
    let vin_changed = param_changes
        .iter()
        .any(|p| p.name == "vin" || p.name == "power_input_voltage");
    if vin_changed {
        affected.push("power.*".into());
    }

    // Output changes affect corresponding power modules
    for change in param_changes {
        if change.name.starts_with("output.") {
            if let Some(idx_str) = change.name.split('.').nth(1) {
                affected.push(format!("power_{}", idx_str));
            }
        }
        if change.name.starts_with("ic.") {
            if let Some(idx_str) = change.name.split('.').nth(1) {
                affected.push(format!("ic_{}", idx_str));
                affected.push("power_*".to_string()); // IC param change may affect power
            }
        }
    }

    affected.sort();
    affected.dedup();
    affected
}

// ── Core Update Logic ────────────────────────────────────────────

pub fn update_design(
    db: &crate::ComponentDb,
    snapshot: &DesignSnapshot,
    changes: &HashMap<String, f64>,
) -> Result<(DesignSnapshot, DesignDiff)> {
    let mut new_spec = snapshot.spec.clone();
    let param_changes = apply_param_changes(&mut new_spec, changes);

    if param_changes.is_empty() {
        return Ok((
            snapshot.clone(),
            DesignDiff {
                changed_params: vec![],
                affected_modules: vec![],
                unaffected_modules: snapshot
                    .composition
                    .modules
                    .iter()
                    .map(|m| m.id.clone())
                    .collect(),
                modules_recomputed: 0,
                modules_cached: snapshot.composition.modules.len(),
            },
        ));
    }

    let affected = detect_affected_modules(&new_spec, &param_changes);

    // Re-run spec_to_composition for updated spec
    let knowledge = crate::requirement::IcKnowledge::new();
    let assembly = crate::requirement::spec_to_composition(&new_spec, &knowledge)?;

    // Re-run power tree if power params changed
    let new_power_tree = if let Some(ref power_req) = assembly.power_request {
        match power_tree::run_power_tree(db, power_req) {
            Ok(result) => Some(result.tree),
            Err(_) => None,
        }
    } else {
        None
    };

    // Build updated composition
    let mut updated_composition = assembly.composition;

    // Merge power tree modules if available
    if let Some(ref tree_nodes) = new_power_tree {
        for (i, node) in tree_nodes.iter().enumerate() {
            let mut nets = HashMap::new();
            nets.insert("input".into(), node.input_net.clone());
            nets.insert("output".into(), node.output_net.clone());

            updated_composition.modules.push(ModuleInstance {
                id: format!("power_{}", i),
                template: node.template_name.clone(),
                template_type: "topology".into(),
                params: node.pipeline_outputs.clone(),
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
    }

    // Count affected/unaffected
    let all_module_ids: Vec<String> = updated_composition
        .modules
        .iter()
        .map(|m| m.id.clone())
        .collect();
    let unaffected: Vec<String> = all_module_ids
        .iter()
        .filter(|id| {
            let id_str: &str = id.as_str();
            !affected.iter().any(|a| {
                let a_str: &str = a.as_str();
                a_str == id_str
                    || (a_str.ends_with('*') && id_str.starts_with(&a_str[..a_str.len() - 1]))
            })
        })
        .cloned()
        .collect();
    let modules_recomputed = all_module_ids.len() - unaffected.len();

    let new_snapshot = DesignSnapshot {
        id: snapshot.id.clone(),
        name: snapshot.name.clone(),
        version: snapshot.version + 1,
        created_at: snapshot.created_at.clone(),
        spec: new_spec,
        composition: updated_composition,
        power_tree_nodes: new_power_tree.unwrap_or_default(),
        module_pipeline_results: snapshot.module_pipeline_results.clone(),
        schematic: None, // Will be generated on demand
    };

    let diff = DesignDiff {
        changed_params: param_changes,
        affected_modules: affected,
        unaffected_modules: unaffected,
        modules_recomputed,
        modules_cached: snapshot.composition.modules.len(),
    };

    Ok((new_snapshot, diff))
}

pub fn generate_new_uuid() -> String {
    format!(
        "{:x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    )
}

// ── Tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::power_tree::RailSpec;
    use crate::requirement::{IcBoardSpec, IcRequest, PowerBoardSpec};

    #[test]
    fn test_apply_param_changes_power() {
        let mut spec = DesignSpec::PowerBoard(PowerBoardSpec {
            vin: 12.0,
            outputs: vec![
                RailSpec {
                    vout: 5.0,
                    iout: 2.0,
                    name: Some("5V".into()),
                },
                RailSpec {
                    vout: 3.3,
                    iout: 1.0,
                    name: Some("3V3".into()),
                },
            ],
            isolated: false,
            efficiency_min: None,
        });

        let changes = HashMap::from([("vin".into(), 24.0), ("output.0.vout".into(), 3.3)]);

        let param_changes = apply_param_changes(&mut spec, &changes);
        assert_eq!(param_changes.len(), 2);

        if let DesignSpec::PowerBoard(p) = &spec {
            assert_eq!(p.vin, 24.0);
            assert_eq!(p.outputs[0].vout, 3.3);
            assert_eq!(p.outputs[1].vout, 3.3); // unchanged
        }
    }

    #[test]
    fn test_apply_param_changes_ic() {
        let mut spec = DesignSpec::IcBoard(IcBoardSpec {
            name: "test".into(),
            core_ics: vec![IcRequest {
                ic_type: crate::requirement::IcType::Mcu,
                mpn: Some("STM32F103".into()),
                parameters: HashMap::from([("frequency".into(), 72.0), ("vdd".into(), 3.3)]),
            }],
            interfaces: vec![],
            power_input_voltage: 5.0,
            constraints: vec![],
        });

        let changes = HashMap::from([
            ("ic.0.frequency".into(), 48.0),
            ("power_input_voltage".into(), 12.0),
        ]);

        let param_changes = apply_param_changes(&mut spec, &changes);
        assert_eq!(param_changes.len(), 2);

        if let DesignSpec::IcBoard(ic) = &spec {
            assert_eq!(ic.power_input_voltage, 12.0);
            assert_eq!(ic.core_ics[0].parameters["frequency"], 48.0);
        }
    }

    #[test]
    fn test_detect_affected_modules() {
        let spec = DesignSpec::PowerBoard(PowerBoardSpec {
            vin: 24.0,
            outputs: vec![RailSpec {
                vout: 3.3,
                iout: 2.0,
                name: None,
            }],
            isolated: false,
            efficiency_min: None,
        });

        let changes = vec![ParamChange {
            name: "output.0.vout".into(),
            old_value: 5.0,
            new_value: 3.3,
        }];

        let affected = detect_affected_modules(&spec, &changes);
        assert!(affected.contains(&"power_0".to_string()));
    }

    #[test]
    fn test_no_changes_detected() {
        let mut spec = DesignSpec::PowerBoard(PowerBoardSpec {
            vin: 12.0,
            outputs: vec![RailSpec {
                vout: 5.0,
                iout: 2.0,
                name: None,
            }],
            isolated: false,
            efficiency_min: None,
        });

        let changes = HashMap::from([("vin".into(), 12.0)]); // same value
        let param_changes = apply_param_changes(&mut spec, &changes);
        assert!(param_changes.is_empty());
    }

    #[test]
    fn test_snapshot_serialization() {
        let snapshot = DesignSnapshot {
            id: "test_123".into(),
            name: "test board".into(),
            version: 1,
            created_at: "2026-05-09".into(),
            spec: DesignSpec::PowerBoard(PowerBoardSpec {
                vin: 12.0,
                outputs: vec![RailSpec {
                    vout: 5.0,
                    iout: 2.0,
                    name: Some("5V".into()),
                }],
                isolated: false,
                efficiency_min: None,
            }),
            composition: Composition {
                name: "test".into(),
                description: "test".into(),
                modules: vec![],
                global_nets: vec![],
            },
            power_tree_nodes: vec![],
            module_pipeline_results: HashMap::new(),
            schematic: None,
        };

        let json = serde_json::to_string(&snapshot).unwrap();
        let deserialized: DesignSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, "test_123");
        assert_eq!(deserialized.version, 1);
    }

    /// H1: verify version history is retained and specific versions loadable.
    /// Before the composite-PK fix, INSERT OR REPLACE on single `id` PK
    /// silently discarded v1 when v2 was saved. Combined into one test to
    /// avoid CDB_PATH env-var races between parallel test threads.
    #[test]
    fn test_version_history_and_load() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::env::set_var("CDB_PATH", tmp.path());
        let conn = get_connection().unwrap();
        drop(conn);

        let mk_snap = |id: &str, version: u32, vin: f64| DesignSnapshot {
            id: id.into(),
            name: "H1 Test".into(),
            version,
            created_at: "0".into(),
            spec: DesignSpec::PowerBoard(PowerBoardSpec {
                vin,
                outputs: vec![],
                isolated: false,
                efficiency_min: None,
            }),
            composition: Composition {
                name: "test".into(),
                description: String::new(),
                modules: vec![],
                global_nets: vec![],
            },
            power_tree_nodes: vec![],
            module_pipeline_results: HashMap::new(),
            schematic: None,
        };

        // --- Part 1: version history retained ---
        save_snapshot(&mk_snap("h1_hist", 1, 12.0)).unwrap();
        save_snapshot(&mk_snap("h1_hist", 2, 24.0)).unwrap();
        let versions = list_snapshot_versions("h1_hist").unwrap();
        assert_eq!(versions.len(), 2, "both versions must be retained");
        assert_eq!(versions[0].0, 2, "newest version first");
        assert_eq!(versions[1].0, 1, "old version preserved");

        // --- Part 2: load specific version (not just latest) ---
        save_snapshot(&mk_snap("h1_load", 1, 5.0)).unwrap();
        save_snapshot(&mk_snap("h1_load", 2, 12.0)).unwrap();

        let latest = load_snapshot("h1_load").unwrap();
        assert_eq!(latest.version, 2, "load_snapshot returns latest");

        let v1 = load_snapshot_version("h1_load", 1).unwrap();
        assert_eq!(
            v1.version, 1,
            "load_snapshot_version returns the requested version"
        );
        if let DesignSpec::PowerBoard(p) = &v1.spec {
            assert_eq!(p.vin, 5.0, "v1 should have the original vin");
        } else {
            panic!("expected PowerBoard");
        }

        std::env::remove_var("CDB_PATH");
    }
}
