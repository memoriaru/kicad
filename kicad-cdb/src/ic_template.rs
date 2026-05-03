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

/// Load built-in IC template by name
pub fn load_builtin_template(name: &str) -> Result<IcCoreTemplate> {
    let json_content = match name {
        "RT9193-ADJ" => include_str!("../ic-templates/RT9193-ADJ.json"),
        "EL7156" => include_str!("../ic-templates/EL7156.json"),
        "AO3408-low-side-switch" => include_str!("../ic-templates/AO3408-low-side-switch.json"),
        "NTC-divider" => include_str!("../ic-templates/NTC-divider.json"),
        _ => anyhow::bail!(
            "Unknown IC template: {}. Available: RT9193-ADJ, EL7156, AO3408-low-side-switch, NTC-divider",
            name
        ),
    };
    let template: IcCoreTemplate = serde_json::from_str(json_content)
        .with_context(|| format!("Failed to parse built-in IC template: {}", name))?;
    Ok(template)
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
    var_names.sort_by(|a, b| b.len().cmp(&a.len()));
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
        while *pos < chars.len() && (chars[*pos].is_ascii_digit() || chars[*pos] == '.' || chars[*pos] == 'e' || chars[*pos] == 'E' || (*pos > start && (chars[*pos] == '+' || chars[*pos] == '-'))) {
            *pos += 1;
        }
        let num_str: String = chars[start..*pos].iter().collect();
        num_str.parse::<f64>()
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
}
