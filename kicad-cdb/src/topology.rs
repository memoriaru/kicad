use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

use crate::template_store::TemplateStore;

#[derive(Debug, Deserialize)]
pub struct TopologyTemplate {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub inputs: Vec<String>,
    #[serde(default)]
    pub components: Vec<ComponentSlot>,
    #[serde(default)]
    pub connections: Vec<Connection>,
    #[serde(default)]
    pub layout: HashMap<String, LayoutPos>,
}

#[derive(Debug, Deserialize)]
pub struct ComponentSlot {
    pub role: String,
    #[serde(default)]
    pub lib: String,
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub rules: Vec<String>,
    #[serde(default)]
    pub params: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct Connection {
    pub net: String,
    pub pins: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct LayoutPos {
    pub x: f64,
    pub y: f64,
}

/// Load a topology template from a JSON file
pub fn load_template(path: &Path) -> Result<TopologyTemplate> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read template: {}", path.display()))?;
    let template: TopologyTemplate = serde_json::from_str(&text)
        .with_context(|| format!("Failed to parse template: {}", path.display()))?;
    Ok(template)
}

/// 本模块的模板存储: 数据库表 `topology_templates` + 目录 `templates/`.
static STORE: TemplateStore = TemplateStore::new("topology_templates", "templates");

/// Load topology template by name. Tries DB first, then filesystem fallback.
pub fn load_builtin_template(name: &str) -> Result<TopologyTemplate> {
    let json = STORE.load_json(name)?.with_context(|| {
        format!(
            "Unknown topology: '{}'. Use 'kdesign import-templates' to load, or ensure templates/{}.json exists",
            name, name
        )
    })?;
    serde_json::from_str(&json).with_context(|| format!("Failed to parse topology '{}'", name))
}

/// List all available topology template names (from DB + filesystem)
pub fn list_builtin_topologies() -> Vec<String> {
    STORE.list_names()
}

/// Import all topology templates from a directory into the DB
pub fn import_topologies_from_dir(dir: &Path, conn: &rusqlite::Connection) -> Result<usize> {
    STORE.import_dir(dir, conn)
}

/// Resolve template params — substitute "$var" references with actual values
pub fn resolve_params(
    params: &HashMap<String, serde_json::Value>,
    inputs: &HashMap<String, f64>,
) -> HashMap<String, f64> {
    let mut resolved = HashMap::new();
    for (key, val) in params {
        match val {
            serde_json::Value::String(s) if s.starts_with('$') => {
                let var_name = &s[1..];
                if let Some(v) = inputs.get(var_name) {
                    resolved.insert(key.clone(), *v);
                }
            }
            serde_json::Value::Number(n) => {
                if let Some(f) = n.as_f64() {
                    resolved.insert(key.clone(), f);
                }
            }
            _ => {}
        }
    }
    resolved
}
