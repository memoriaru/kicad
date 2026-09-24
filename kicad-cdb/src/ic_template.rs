use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct IcCoreTemplate {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub ic: IcDef,
    #[serde(default)]
    pub peripherals: Vec<Peripheral>,
    #[serde(default)]
    pub params: HashMap<String, ParamDef>,
    #[serde(default)]
    pub interface: HashMap<String, InterfacePort>,
    #[serde(default)]
    pub constraints: HashMap<String, String>,
    #[serde(default)]
    pub layout: HashMap<String, LayoutPos>,
}

#[derive(Debug, Deserialize)]
pub struct IcDef {
    #[serde(default)]
    pub mpn: String,
    #[serde(default)]
    pub manufacturer: String,
    #[serde(default)]
    pub package: String,
    #[serde(default)]
    pub footprint: String,
    pub pins: Vec<IcPin>,
}

#[derive(Debug, Deserialize)]
pub struct IcPin {
    pub number: String,
    pub name: String,
    #[serde(default, rename = "type")]
    pub pin_type: String,
}

#[derive(Debug, Deserialize)]
pub struct Peripheral {
    pub role: String,
    #[serde(default)]
    pub lib: String,
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub footprint: String,
    pub pins: HashMap<String, String>,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Deserialize)]
pub struct ParamDef {
    #[serde(default)]
    pub value: Option<f64>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub formula: Option<String>,
    #[serde(default)]
    pub depends: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct InterfacePort {
    #[serde(default)]
    pub direction: String,
    #[serde(default)]
    #[serde(rename = "type")]
    pub port_type: String,
}

#[derive(Debug, Deserialize)]
pub struct LayoutPos {
    pub x: f64,
    pub y: f64,
}

/// Load IC core template from a JSON file
pub fn load_template(path: &Path) -> Result<IcCoreTemplate> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read IC template: {}", path.display()))?;
    let template: IcCoreTemplate = serde_json::from_str(&text)
        .with_context(|| format!("Failed to parse IC template: {}", path.display()))?;
    Ok(template)
}

/// Fetch pin list from HuaQiu EDA API by MPN.
/// Searches for the component, downloads its KiCad symbol, and extracts pin info.
pub fn fetch_pins_from_hqapi(mpn: &str) -> Result<Vec<IcPin>> {
    let client = crate::hqapi::HqClient::new()?;

    // Search to find manufacturer_id
    let results = client.search(mpn, 5)?;
    if results.is_empty() {
        anyhow::bail!("No results found for '{}'", mpn);
    }
    let first = &results[0];

    // Get product detail
    let detail = client.product_detail(&first.manufacturer_id, &first.mpn)?;

    // Download symbol and extract pins
    let extracted = if let Some(sym_url) = detail.cad_urls.iter().find(|c| c.url_type == "symbol") {
        match client.download_text(&sym_url.url) {
            Ok(sym_text) => crate::hqapi::sym_parser::extract_pins(&sym_text),
            Err(e) => {
                eprintln!("Warning: could not download symbol: {}", e);
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };

    // Convert ExtractedPin → IcPin, mapping KiCad type codes to human-readable names
    const TYPE_MAP: &[(&str, &str)] = &[
        ("I", "input"),
        ("O", "output"),
        ("B", "bidirectional"),
        ("W", "power_in"),
        ("w", "power_out"),
        ("P", "passive"),
        ("U", "unspecified"),
        ("C", "open_collector"),
        ("E", "open_emitter"),
        ("T", "tri_state"),
        ("N", "no_connect"),
    ];

    let pins: Vec<IcPin> = extracted
        .into_iter()
        .map(|p| {
            let pin_type = TYPE_MAP
                .iter()
                .find(|(code, _)| *code == p.pin_type)
                .map(|(_, name)| name.to_string())
                .unwrap_or_else(|| p.pin_type.clone());
            IcPin {
                number: p.number,
                name: p.name,
                pin_type,
            }
        })
        .collect();

    Ok(pins)
}

/// Load IC template by name. Tries DB first, then filesystem fallback.
pub fn load_builtin_template(name: &str) -> Result<IcCoreTemplate> {
    // Try DB first
    if let Some(db) = get_db_connection() {
        if let Ok(Some(json)) = load_from_db(&db, name) {
            return serde_json::from_str(&json)
                .with_context(|| format!("Failed to parse IC template '{}' from DB", name));
        }
    }

    // Filesystem fallback: look in ic-templates/ relative to cwd
    let path = Path::new("ic-templates").join(format!("{}.json", name));
    if path.exists() {
        return load_template(&path);
    }

    anyhow::bail!(
        "Unknown IC template: '{}'. Use 'cdb import-templates' to load templates into DB, or ensure ic-templates/{}.json exists",
        name, name
    )
}

/// List all available IC template names (from DB + filesystem)
pub fn list_builtin_templates() -> Vec<String> {
    let mut names = Vec::new();

    // From DB
    if let Some(db) = get_db_connection() {
        if let Ok(db_names) = list_from_db(&db) {
            names.extend(db_names);
        }
    }

    // From filesystem
    if let Ok(entries) = std::fs::read_dir("ic-templates") {
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

/// Import all .json templates from a directory into the DB
pub fn import_templates_from_dir(dir: &Path, conn: &rusqlite::Connection) -> Result<usize> {
    let entries = std::fs::read_dir(dir)
        .with_context(|| format!("Failed to read template directory: {}", dir.display()))?;
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
                "INSERT OR REPLACE INTO ic_templates (name, description, json_data, updated_at) VALUES (?1, ?2, ?3, datetime('now'))",
                rusqlite::params![name, desc, json],
            )?;
            count += 1;
        }
    }
    Ok(count)
}

// --- Internal DB helpers ---

fn get_db_connection() -> Option<rusqlite::Connection> {
    let db_path = std::env::var("CDB_PATH").unwrap_or_else(|_| "components.db".to_string());
    rusqlite::Connection::open(&db_path).ok()
}

fn load_from_db(conn: &rusqlite::Connection, name: &str) -> Result<Option<String>> {
    let result: Option<String> = conn
        .query_row(
            "SELECT json_data FROM ic_templates WHERE name = ?1",
            rusqlite::params![name],
            |row| row.get(0),
        )
        .ok();
    Ok(result)
}

fn list_from_db(conn: &rusqlite::Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT name FROM ic_templates ORDER BY name")?;
    let names: Vec<String> = stmt
        .query_map([], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(names)
}

/// Resolve all template parameters — evaluate formulas in dependency order
pub fn resolve_params(
    template: &IcCoreTemplate,
    user_inputs: &HashMap<String, f64>,
) -> Result<HashMap<String, f64>> {
    let mut resolved = HashMap::new();

    // Seed with user inputs
    for (k, v) in user_inputs {
        resolved.insert(k.clone(), *v);
    }

    // Seed with fixed values from template
    for (name, def) in &template.params {
        if let Some(v) = def.value {
            resolved.insert(name.clone(), v);
        }
    }

    // Iteratively resolve formulas (max 10 passes for dependency chains)
    for _ in 0..10 {
        let mut changed = false;
        for (name, def) in &template.params {
            if resolved.contains_key(name) {
                continue;
            }
            if let Some(formula) = &def.formula {
                // Check all dependencies are resolved
                let all_deps = def.depends.iter().all(|d| resolved.contains_key(d));
                if all_deps {
                    if let Ok(val) = eval_formula(formula, &resolved) {
                        resolved.insert(name.clone(), val);
                        changed = true;
                    }
                }
            }
        }
        if !changed {
            break;
        }
    }

    Ok(resolved)
}

/// Simple formula evaluator — supports basic arithmetic and variable substitution
fn eval_formula(formula: &str, vars: &HashMap<String, f64>) -> Result<f64> {
    let expr = formula.trim();
    // Expect "varname = expression"
    if let Some(eq_pos) = expr.find('=') {
        let _var_name = expr[..eq_pos].trim();
        let expression = expr[eq_pos + 1..].trim();
        return eval_arithmetic(expression, vars);
    }
    eval_arithmetic(expr, vars)
}

/// Evaluate a simple arithmetic expression with variable substitution
fn eval_arithmetic(expr: &str, vars: &HashMap<String, f64>) -> Result<f64> {
    let mut substituted = expr.to_string();
    // Sort variable names by length (longest first) to avoid partial replacement
    let mut var_names: Vec<String> = vars.keys().cloned().collect();
    var_names.sort_by_key(|b| std::cmp::Reverse(b.len()));
    for name in &var_names {
        let val = vars[name.as_str()];
        substituted = substituted.replace(name.as_str(), &format!("({})", val));
    }

    // Simple recursive descent parser for: +, -, *, /, (, )
    let chars: Vec<char> = substituted.chars().collect();
    let mut pos = 0;
    let result = parse_expr(&chars, &mut pos)?;

    Ok(result)
}

fn parse_expr(chars: &[char], pos: &mut usize) -> Result<f64> {
    let mut result = parse_term(chars, pos)?;
    skip_whitespace(chars, pos);
    while *pos < chars.len() {
        let c = chars[*pos];
        if c == '+' {
            *pos += 1;
            skip_whitespace(chars, pos);
            result += parse_term(chars, pos)?;
        } else if c == '-' {
            *pos += 1;
            skip_whitespace(chars, pos);
            result -= parse_term(chars, pos)?;
        } else {
            break;
        }
        skip_whitespace(chars, pos);
    }
    Ok(result)
}

fn parse_term(chars: &[char], pos: &mut usize) -> Result<f64> {
    let mut result = parse_factor(chars, pos)?;
    skip_whitespace(chars, pos);
    while *pos < chars.len() {
        let c = chars[*pos];
        if c == '*' {
            *pos += 1;
            skip_whitespace(chars, pos);
            result *= parse_factor(chars, pos)?;
        } else if c == '/' {
            *pos += 1;
            skip_whitespace(chars, pos);
            let divisor = parse_factor(chars, pos)?;
            if divisor == 0.0 {
                anyhow::bail!("Division by zero in formula");
            }
            result /= divisor;
        } else {
            break;
        }
        skip_whitespace(chars, pos);
    }
    Ok(result)
}

fn parse_factor(chars: &[char], pos: &mut usize) -> Result<f64> {
    skip_whitespace(chars, pos);
    if *pos >= chars.len() {
        anyhow::bail!("Unexpected end of expression");
    }

    let c = chars[*pos];
    if c == '(' {
        *pos += 1;
        let result = parse_expr(chars, pos)?;
        skip_whitespace(chars, pos);
        if *pos < chars.len() && chars[*pos] == ')' {
            *pos += 1;
        }
        Ok(result)
    } else if c == '-' {
        *pos += 1;
        Ok(-parse_factor(chars, pos)?)
    } else {
        // Parse number
        let start = *pos;
        while *pos < chars.len()
            && (chars[*pos].is_ascii_digit()
                || chars[*pos] == '.'
                || chars[*pos] == 'e'
                || chars[*pos] == 'E'
                || (*pos > start && (chars[*pos] == '+' || chars[*pos] == '-')))
        {
            *pos += 1;
        }
        let num_str: String = chars[start..*pos].iter().collect();
        num_str
            .parse::<f64>()
            .with_context(|| format!("Failed to parse number: '{}'", num_str))
    }
}

fn skip_whitespace(chars: &[char], pos: &mut usize) {
    while *pos < chars.len() && chars[*pos].is_whitespace() {
        *pos += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rt9193_load() {
        let t = load_builtin_template("RT9193-ADJ").unwrap();
        assert_eq!(t.ic.pins.len(), 5);
        assert_eq!(t.ic.pins[0].name, "VIN");
        assert_eq!(t.peripherals.len(), 4); // c_in, c_out, r_fb1, r_fb2
    }

    #[test]
    fn test_el7156_load() {
        let t = load_builtin_template("EL7156").unwrap();
        assert_eq!(t.ic.pins.len(), 8);
        assert_eq!(t.peripherals.len(), 3); // c_vplus, c_vminus, r_out
    }

    #[test]
    fn test_rt9193_param_resolution() {
        let t = load_builtin_template("RT9193-ADJ").unwrap();
        let mut inputs = HashMap::new();
        inputs.insert("vout".to_string(), 3.3);
        let resolved = resolve_params(&t, &inputs).unwrap();

        // vref = 0.8 (fixed)
        assert!((resolved["vref"] - 0.8).abs() < 0.001);
        // r_fb2 = 10000 (fixed)
        assert!((resolved["r_fb2"] - 10000.0).abs() < 1.0);
        // r_fb1 = r_fb2 * (vout / vref - 1) = 10000 * (3.3/0.8 - 1) = 10000 * 3.125 = 31250
        assert!((resolved["r_fb1"] - 31250.0).abs() < 1.0);
    }

    #[test]
    fn test_rt9193_verify_u5_13v() {
        let t = load_builtin_template("RT9193-ADJ").unwrap();
        let mut inputs = HashMap::new();
        inputs.insert("vout".to_string(), 13.0);
        let resolved = resolve_params(&t, &inputs).unwrap();
        // r_fb1 = 10000 * (13/0.8 - 1) = 10000 * 15.25 = 152500
        // Actual schematic uses R1=10k, R2=649 → effective R_fb1/R_fb2 = 10000/649
        // Vout = 0.8 * (1 + 10000/649) = 13.13V ≈ 13V ✓
        let ratio = resolved["r_fb1"] / resolved["r_fb2"];
        let vout_calc = 0.8 * (1.0 + ratio);
        assert!((vout_calc - 13.0).abs() < 0.5);
    }

    #[test]
    fn test_formula_eval() {
        let mut vars = HashMap::new();
        vars.insert("r_fb2".to_string(), 10000.0);
        vars.insert("vout".to_string(), 3.3);
        vars.insert("vref".to_string(), 0.8);

        let result = eval_formula("r_fb1 = r_fb2 * (vout / vref - 1)", &vars).unwrap();
        assert!((result - 31250.0).abs() < 0.1);
    }

    #[test]
    fn test_arithmetic() {
        let vars = HashMap::new();
        let r1 = eval_arithmetic("2 + 3", &vars);
        eprintln!("2+3 = {:?}", r1);
        assert!((r1.unwrap() - 5.0).abs() < 0.001);
        assert!((eval_arithmetic("2 * 3 + 1", &vars).unwrap() - 7.0).abs() < 0.001);
        assert!((eval_arithmetic("(2 + 3) * 4", &vars).unwrap() - 20.0).abs() < 0.001);
        assert!((eval_arithmetic("10 / 3", &vars).unwrap() - 3.333).abs() < 0.01);
    }

    #[test]
    fn test_fp6277_load() {
        let t = load_builtin_template("FP6277").unwrap();
        assert_eq!(t.ic.pins.len(), 6);
        assert_eq!(t.ic.pins[0].name, "EN");
        assert_eq!(t.ic.pins[2].name, "SW");
        assert_eq!(t.ic.pins[5].name, "VIN");
        assert_eq!(t.peripherals.len(), 3); // l_in, c_in, c_out
        assert_eq!(t.interface.len(), 4); // VIN, GND, VOUT, EN
    }

    #[test]
    fn test_sy7208_load() {
        let t = load_builtin_template("SY7208").unwrap();
        assert_eq!(t.ic.pins.len(), 6);
        assert_eq!(t.ic.mpn, "SY7208");
        assert_eq!(t.peripherals.len(), 3);
        // Verify fixed param
        let params = resolve_params(&t, &HashMap::new()).unwrap();
        assert!((params["l_value"] - 4.7e-6).abs() < 1e-10);
    }
}
