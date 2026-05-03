use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

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

/// Load built-in template by name (ldo, buck, led)
pub fn load_builtin_template(name: &str) -> Result<TopologyTemplate> {
    let json_content = match name {
        "ldo" => include_str!("../templates/ldo.json"),
        "buck" => include_str!("../templates/buck.json"),
        "led" => include_str!("../templates/led.json"),
        "boost" => include_str!("../templates/boost.json"),
        "buckboost" => include_str!("../templates/buckboost.json"),
        "inverting" => include_str!("../templates/inverting.json"),
        "sepic" => include_str!("../templates/sepic.json"),
        "chargepump" => include_str!("../templates/chargepump.json"),
        "flyback" => include_str!("../templates/flyback.json"),
        _ => anyhow::bail!(
            "Unknown template: {}. Available: ldo, buck, led, boost, buckboost, inverting, sepic, chargepump, flyback",
            name
        ),
    };
    let template: TopologyTemplate = serde_json::from_str(json_content)
        .with_context(|| format!("Failed to parse built-in template: {}", name))?;
    Ok(template)
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
