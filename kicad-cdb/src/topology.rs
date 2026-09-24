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

/// Load topology template by name. Tries DB first, then filesystem fallback.
pub fn load_builtin_template(name: &str) -> Result<TopologyTemplate> {
    // Try DB first
    if let Some(db) = get_db_connection() {
        if let Ok(Some(json)) = load_from_db(&db, name) {
            return serde_json::from_str(&json)
                .with_context(|| format!("Failed to parse topology '{}' from DB", name));
        }
    }

    // Filesystem fallback
    let path = Path::new("templates").join(format!("{}.json", name));
    if path.exists() {
        return load_template(&path);
    }

    anyhow::bail!(
        "Unknown topology: '{}'. Use 'cdb import-templates' to load, or ensure templates/{}.json exists",
        name, name
    )
}

/// List all available topology template names (from DB + filesystem)
pub fn list_builtin_topologies() -> Vec<String> {
    let mut names = Vec::new();

    if let Some(db) = get_db_connection() {
        if let Ok(db_names) = list_from_db(&db) {
            names.extend(db_names);
        }
    }

    if let Ok(entries) = std::fs::read_dir("templates") {
        for entry in entries.flatten() {
            if let Some(name) = entry.path().file_stem().and_then(|s| s.to_str()) {
                if !names.contains(&name.to_string()) {
                    names.push(name.to_string());
                }
            }
        }
    }

    names.sort();
    names.dedup();
    names
}

/// Import all topology templates from a directory into the DB
pub fn import_topologies_from_dir(dir: &Path, conn: &rusqlite::Connection) -> Result<usize> {
    let entries = std::fs::read_dir(dir)
        .with_context(|| format!("Failed to read topology directory: {}", dir.display()))?;
    let mut count = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("json") {
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            if name.is_empty() {
                continue;
            }
            let json = std::fs::read_to_string(&path)?;
            let desc: Option<String> = serde_json::from_str::<serde_json::Value>(&json)
                .ok()
                .and_then(|v| {
                    v.get("description")
                        .and_then(|d| d.as_str().map(|s| s.to_string()))
                });
            conn.execute(
                "INSERT OR REPLACE INTO topology_templates (name, description, json_data, updated_at) VALUES (?1, ?2, ?3, datetime('now'))",
                rusqlite::params![name, desc, json],
            )?;
            count += 1;
        }
    }
    Ok(count)
}

fn get_db_connection() -> Option<rusqlite::Connection> {
    let db_path = std::env::var("CDB_PATH").unwrap_or_else(|_| "components.db".to_string());
    rusqlite::Connection::open(&db_path).ok()
}

fn load_from_db(conn: &rusqlite::Connection, name: &str) -> Result<Option<String>> {
    let result: Option<String> = conn
        .query_row(
            "SELECT json_data FROM topology_templates WHERE name = ?1",
            rusqlite::params![name],
            |row| row.get(0),
        )
        .ok();
    Ok(result)
}

fn list_from_db(conn: &rusqlite::Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT name FROM topology_templates ORDER BY name")?;
    let names: Vec<String> = stmt
        .query_map([], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(names)
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
