use anyhow::{Context, Result};
use std::collections::HashMap;

use crate::models::*;
use crate::rules::RuleResult;
use crate::ComponentDb;

/// Parse comma-separated key=value pairs into HashMap (e.g. "vin=12,vout=3.3")
pub fn parse_kv_f64(s: &str) -> Result<HashMap<String, f64>> {
    let mut map = HashMap::new();
    for pair in s.split(',') {
        let kv: Vec<&str> = pair.splitn(2, '=').collect();
        if kv.len() == 2 {
            let val: f64 = kv[1].parse().context(format!("Invalid number: {}", kv[1]))?;
            map.insert(kv[0].trim().to_string(), val);
        }
    }
    Ok(map)
}

/// Parse candidate value string "name=value" into optional (name, value) pair
pub fn parse_candidate(s: &str) -> Result<(Option<String>, Option<f64>)> {
    let kv: Vec<&str> = s.splitn(2, '=').collect();
    if kv.len() == 2 {
        Ok((Some(kv[0].trim().to_string()), Some(kv[1].parse()?)))
    } else {
        Ok((None, None))
    }
}

/// Unified component query with all filter dimensions.
/// Eliminates duplication between CLI cmd_query and MCP tool_query.
pub fn query_filtered(
    db: &ComponentDb,
    search: Option<&str>,
    category: Option<&str>,
    manufacturer: Option<&str>,
    package: Option<&str>,
    param: Option<(&str, Option<f64>, Option<f64>)>,
    in_stock: bool,
    limit: Option<usize>,
) -> Result<Vec<Component>> {
    let mut results = Vec::new();

    if let Some(cat) = category {
        results = db.query_components_by_category(cat)?;
    }

    if let Some(query) = search {
        results = db.search(query)?;
    }

    if in_stock {
        let stocked = db.query_in_stock()?;
        if results.is_empty() {
            results = stocked;
        } else {
            let ids: std::collections::HashSet<i64> = stocked.iter().filter_map(|c| c.id).collect();
            results.retain(|c| c.id.map(|id| ids.contains(&id)).unwrap_or(false));
        }
    }

    if let Some(mfg) = manufacturer {
        let mfg_lower = mfg.to_lowercase();
        results.retain(|c| c.manufacturer.to_lowercase().contains(&mfg_lower));
    }

    if let Some(pkg) = package {
        results.retain(|c| c.package.as_deref() == Some(pkg));
    }

    if let Some((name, min, max)) = param {
        let filtered = db.query_by_parameter_range(name, min, max)?;
        if results.is_empty() {
            results = filtered;
        } else {
            let ids: std::collections::HashSet<i64> = filtered.iter().filter_map(|c| c.id).collect();
            results.retain(|c| c.id.map(|id| ids.contains(&id)).unwrap_or(false));
        }
    }

    if let Some(n) = limit {
        results.truncate(n);
    }

    Ok(results)
}

/// Look up a rule by name, parse string params, and apply it.
/// Eliminates duplication between cmd_check, cmd_rules --apply, and tool_check.
pub fn apply_rule_with_str_params(
    db: &ComponentDb,
    rule_name: &str,
    params_str: &str,
    candidate_str: Option<&str>,
) -> Result<(DesignRule, RuleResult)> {
    let rule = db.get_rule_by_name(rule_name)?
        .ok_or_else(|| anyhow::anyhow!("Rule '{}' not found", rule_name))?;

    let mut inputs = serde_json::Map::new();
    for pair in params_str.split(',') {
        let kv: Vec<&str> = pair.splitn(2, '=').collect();
        if kv.len() == 2 {
            let val: f64 = kv[1].parse().context(format!("Invalid number: {}", kv[1]))?;
            inputs.insert(kv[0].trim().to_string(), serde_json::Value::from(val));
        }
    }

    let (cand_name, cand_val) = match candidate_str {
        Some(s) => parse_candidate(s)?,
        None => (None, None),
    };

    let result = db.apply_rule(&rule, &serde_json::Value::Object(inputs), cand_name.as_deref(), cand_val)?;
    Ok((rule, result))
}
