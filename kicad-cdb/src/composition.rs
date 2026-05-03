use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

/// A composition defines how multiple modules are assembled into one schematic.
#[derive(Debug, Deserialize)]
pub struct Composition {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub modules: Vec<ModuleInstance>,
    #[serde(default)]
    pub global_nets: Vec<NetDef>,
}

#[derive(Debug, Deserialize)]
pub struct ModuleInstance {
    /// Unique instance ID (used for internal net namespacing)
    pub id: String,
    /// Template name (IC core template or topology template)
    pub template: String,
    /// Template type: "ic-core" or "topology"
    #[serde(default = "default_template_type")]
    pub template_type: String,
    /// Parameters for this instance (e.g. vout=3.3)
    #[serde(default)]
    pub params: HashMap<String, f64>,
    /// Net name overrides: interface_port → actual net name
    #[serde(default)]
    pub nets: HashMap<String, String>,
    /// Y-offset for placement (mm), auto-calculated if not set
    #[serde(default)]
    pub y_offset: Option<f64>,
    /// Additional inputs for topology templates
    #[serde(default)]
    pub topology_inputs: Option<TopologyInputs>,
}

#[derive(Debug, Deserialize)]
pub struct TopologyInputs {
    pub vin: f64,
    pub vout: f64,
    pub iout: f64,
}

#[derive(Debug, Deserialize)]
pub struct NetDef {
    pub name: String,
    #[serde(default, rename = "type")]
    pub net_type: Option<String>,
}

fn default_template_type() -> String {
    "ic-core".to_string()
}

/// Load a composition definition from a JSON file
pub fn load_composition(path: &Path) -> Result<Composition> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read composition: {}", path.display()))?;
    let comp: Composition = serde_json::from_str(&text)
        .with_context(|| format!("Failed to parse composition: {}", path.display()))?;
    Ok(comp)
}
