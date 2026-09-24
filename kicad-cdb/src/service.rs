use anyhow::{Context, Result};
use serde::Serialize;
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
            let val: f64 = kv[1]
                .parse()
                .context(format!("Invalid number: {}", kv[1]))?;
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

    // 修复: manufacturer/package 作为唯一选择器时, 空起步集会恒返回 0——
    // 此时以全库为起点再做交集保留。
    if results.is_empty()
        && search.is_none()
        && category.is_none()
        && !in_stock
        && (manufacturer.is_some() || package.is_some())
    {
        results = db.search("")?;
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
            let ids: std::collections::HashSet<i64> =
                filtered.iter().filter_map(|c| c.id).collect();
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
    let rule = db
        .get_rule_by_name(rule_name)?
        .ok_or_else(|| anyhow::anyhow!("Rule '{}' not found", rule_name))?;

    let mut inputs = serde_json::Map::new();
    for pair in params_str.split(',') {
        let kv: Vec<&str> = pair.splitn(2, '=').collect();
        if kv.len() == 2 {
            let val: f64 = kv[1]
                .parse()
                .context(format!("Invalid number: {}", kv[1]))?;
            inputs.insert(kv[0].trim().to_string(), serde_json::Value::from(val));
        }
    }

    let (cand_name, cand_val) = match candidate_str {
        Some(s) => parse_candidate(s)?,
        None => (None, None),
    };

    let result = db.apply_rule(
        &rule,
        &serde_json::Value::Object(inputs),
        cand_name.as_deref(),
        cand_val,
    )?;
    Ok((rule, result))
}

/// Apply a design rule and then search for components whose parameters
/// satisfy the computed output constraints (with ±20% tolerance).
pub fn recommend_components(
    db: &ComponentDb,
    rule_name: &str,
    params_str: &str,
    candidate_str: Option<&str>,
    limit: Option<usize>,
) -> Result<(DesignRule, RuleResult, Vec<Component>)> {
    let (rule, result) = apply_rule_with_str_params(db, rule_name, params_str, candidate_str)?;

    let mut recommendations = Vec::new();

    // For each output value, search for components with matching parameter
    for (param_name, computed_value) in &result.outputs {
        // Apply ±20% tolerance band
        let tolerance = 0.20;
        let min = Some(computed_value * (1.0 - tolerance));
        let max = Some(computed_value * (1.0 + tolerance));

        let matches = query_filtered(
            db,
            None,
            None,
            None,
            None,
            Some((param_name, min, max)),
            false,
            limit,
        )?;

        if !matches.is_empty() {
            recommendations = matches;
            break; // Use first successful parameter match
        }
    }

    Ok((rule, result, recommendations))
}

/// Compare multiple candidate values against a design rule.
/// Each candidate is tested independently and scored by margin.
pub fn compare_candidates(
    db: &ComponentDb,
    rule_name: &str,
    params_str: &str,
    candidates: &[(&str, f64)],
) -> Result<CompareResult> {
    let (_rule, baseline) = apply_rule_with_str_params(db, rule_name, params_str, None)?;

    let mut scores = Vec::new();

    for (name, value) in candidates {
        let rule_def = db.get_rule_by_name(rule_name)?.unwrap();
        let mut json_inputs = serde_json::Map::new();
        for pair in params_str.split(',') {
            let kv: Vec<&str> = pair.splitn(2, '=').collect();
            if kv.len() == 2 {
                if let Ok(val) = kv[1].parse::<f64>() {
                    json_inputs.insert(kv[0].trim().to_string(), serde_json::Value::from(val));
                }
            }
        }

        // Inject candidate value — use first output param name if check_expr references it
        let cand_var = rule_def
            .check_expr
            .as_ref()
            .and_then(|c| c.split(&['>', '<', '=', '!'][..]).next())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| name.to_string());
        json_inputs.insert(cand_var, serde_json::Value::from(*value));

        let result = db.apply_rule(
            &rule_def,
            &serde_json::Value::Object(json_inputs),
            None,
            None,
        )?;

        let margin = baseline.outputs.values().next().map_or(0.0, |computed| {
            if *computed != 0.0 {
                (value - computed) / computed.abs() * 100.0
            } else {
                0.0
            }
        });

        scores.push(CandidateScore {
            name: name.to_string(),
            value: *value,
            pass: result.pass,
            margin_pct: margin,
            rank: 0,
        });
    }

    // Sort by: passing first, then by margin descending
    scores.sort_by(|a, b| match (a.pass, b.pass) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => b
            .margin_pct
            .partial_cmp(&a.margin_pct)
            .unwrap_or(std::cmp::Ordering::Equal),
    });

    for (i, s) in scores.iter_mut().enumerate() {
        s.rank = i + 1;
    }

    Ok(CompareResult {
        rule_name: rule_name.to_string(),
        outputs: baseline.outputs,
        candidates: scores,
    })
}

/// Find alternative components for a given MPN.
/// Searches the same category with similar key parameters.
pub fn find_alternatives(
    db: &ComponentDb,
    mpn: &str,
    max_results: usize,
) -> Result<Vec<Component>> {
    // Find the original component
    let originals = db.search(mpn)?;
    let original = originals
        .iter()
        .find(|c| c.mpn == mpn)
        .ok_or_else(|| anyhow::anyhow!("Component '{}' not found", mpn))?;

    let cat_id = original.category_id;

    // Get key parameters of the original
    let orig_params = db.get_parameters(original.id.unwrap())?;
    let key_param_names: Vec<String> = orig_params
        .iter()
        .filter(|p| p.typical)
        .map(|p| p.name.clone())
        .collect();

    // Search same category for alternatives
    let candidates = db.query_components_by_category(
        &db.get_category(cat_id)?.map(|c| c.name).unwrap_or_default(),
    )?;

    // Filter: different MPN, same package preferred, active lifecycle
    let mut alternatives: Vec<Component> = candidates
        .into_iter()
        .filter(|c| c.mpn != mpn)
        .filter(|c| c.lifecycle == "active")
        .filter(|c| c.package == original.package || original.package.is_none())
        .collect();

    // Score by parameter similarity
    alternatives.sort_by(|a, b| {
        let score_a = compute_similarity(a, &orig_params, &key_param_names, db);
        let score_b = compute_similarity(b, &orig_params, &key_param_names, db);
        score_b
            .partial_cmp(&score_a)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    alternatives.truncate(max_results);
    Ok(alternatives)
}

fn compute_similarity(
    candidate: &Component,
    target_params: &[crate::Parameter],
    key_names: &[String],
    db: &ComponentDb,
) -> f64 {
    let cand_id = match candidate.id {
        Some(id) => id,
        None => return 0.0,
    };
    let cand_params = db.get_parameters(cand_id).unwrap_or_default();
    let mut score = 0.0;
    let mut checked = 0;

    for name in key_names {
        let target_val = target_params
            .iter()
            .find(|p| p.name == *name)
            .and_then(|p| p.value_numeric);
        let cand_val = cand_params
            .iter()
            .find(|p| p.name == *name)
            .and_then(|p| p.value_numeric);

        if let (Some(tv), Some(cv)) = (target_val, cand_val) {
            if tv != 0.0 {
                let ratio = 1.0 - ((cv - tv) / tv).abs().min(1.0);
                score += ratio;
            }
            checked += 1;
        }
    }

    if checked > 0 {
        score / checked as f64
    } else {
        0.5
    }
}

/// Estimate BOM cost for a list of MPNs at given quantities.
/// Returns per-item and total cost based on supply_info price_breaks.
pub fn estimate_bom_cost(db: &ComponentDb, items: &[(String, u32)]) -> Result<BomCostResult> {
    let mut line_items = Vec::new();
    let mut total_cost = 0.0;

    for (mpn, qty) in items {
        let components = db.search(mpn)?;
        let comp = components.iter().find(|c| c.mpn == *mpn);

        let (unit_price, supplier, available) = if let Some(c) = comp {
            if let Some(cid) = c.id {
                let supply = db.get_supply_info(cid)?;
                find_best_price(&supply, *qty)
            } else {
                (None, None, 0)
            }
        } else {
            (None, None, 0)
        };

        let price = unit_price.unwrap_or(0.0);
        let line_total = price * *qty as f64;
        total_cost += line_total;

        line_items.push(BomLineItem {
            mpn: mpn.clone(),
            quantity: *qty,
            unit_price: price,
            line_total,
            supplier: supplier.unwrap_or_else(|| "unknown".into()),
            available_stock: available,
        });
    }

    Ok(BomCostResult {
        items: line_items,
        total_cost,
    })
}

fn find_best_price(supply: &[SupplyInfo], qty: u32) -> (Option<f64>, Option<String>, i64) {
    let mut best_price: Option<f64> = None;
    let mut best_supplier: Option<String> = None;
    let mut total_stock: i64 = 0;

    for info in supply {
        if let Some(stock) = info.stock {
            total_stock += stock;
        }
        if let Some(ref breaks) = info.price_breaks {
            // price_breaks format: "[[1, 0.50], [10, 0.40], [100, 0.30]]"
            if let Ok(tiers) = serde_json::from_str::<Vec<Vec<f64>>>(breaks) {
                let mut tier_price: Option<f64> = None;
                for tier in &tiers {
                    if tier.len() >= 2 && tier[0] as u32 <= qty {
                        tier_price = Some(tier[1]);
                    }
                }
                if let Some(p) = tier_price {
                    if best_price.is_none_or(|bp| p < bp) {
                        best_price = Some(p);
                        best_supplier = Some(info.supplier.clone());
                    }
                }
            }
        }
    }

    (best_price, best_supplier, total_stock)
}

/// BOM cost estimation result
#[derive(Debug, Serialize)]
pub struct BomCostResult {
    pub items: Vec<BomLineItem>,
    pub total_cost: f64,
}

#[derive(Debug, Serialize)]
pub struct BomLineItem {
    pub mpn: String,
    pub quantity: u32,
    pub unit_price: f64,
    pub line_total: f64,
    pub supplier: String,
    pub available_stock: i64,
}
