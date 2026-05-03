use anyhow::Result;

use super::sym_parser::ExtractedPin;
use super::ProductDetail;

/// Convert HuaQiu product detail + extracted pins into import JSON
/// compatible with ComponentDb::import_from_json()
pub fn detail_to_import_json(detail: &ProductDetail, pins: &[ExtractedPin]) -> Result<String> {
    // Build category path from cateList (use the most specific one)
    let category = detail
        .categories
        .iter()
        .max_by_key(|c| c.level)
        .map(|c| {
            // Find parent to build "parent/child" path
            let parent = detail
                .categories
                .iter()
                .find(|p| p.id == c.parent_id)
                .map(|p| p.display_name.as_str())
                .unwrap_or("uncategorized");
            if c.level <= 1 {
                c.display_name.to_lowercase().replace(' ', "_")
            } else {
                format!(
                    "{}/{}",
                    parent.to_lowercase().replace(' ', "_"),
                    c.display_name.to_lowercase().replace(' ', "_")
                )
            }
        })
        .unwrap_or_else(|| "uncategorized".to_string());

    // Build parameters from attr groups (deduplicate by name)
    let mut parameters = Vec::new();
    let mut seen_names = std::collections::HashSet::new();
    for group in &detail.attr_groups {
        for attr in &group.attrs {
            if attr.short_name.is_empty() || attr.value.is_empty() {
                continue;
            }

            // Skip duplicate parameter names
            if !seen_names.insert(attr.short_name.clone()) {
                continue;
            }

            let (value_numeric, value_text, unit) = parse_param_value(&attr.value);

            parameters.push(serde_json::json!({
                "name": attr.short_name,
                "value": value_numeric,
                "value_text": value_text,
                "unit": unit,
            }));
        }
    }

    // Build pins
    let pins_json: Vec<serde_json::Value> = pins
        .iter()
        .map(|p| {
            serde_json::json!({
                "number": p.number,
                "name": p.name,
                "electrical_type": p.pin_type,
            })
        })
        .collect();

    // Build supply info with huaqiu_pn as SKU
    let supply = if !detail.huaqiu_pn.is_empty() {
        vec![serde_json::json!({
            "supplier": "HuaQiu",
            "sku": detail.huaqiu_pn,
        })]
    } else {
        vec![]
    };

    let import = serde_json::json!({
        "mpn": detail.mpn,
        "manufacturer": detail.mfg,
        "category": category,
        "auto_create_category": true,
        "description": detail.description,
        "package": detail.package,
        "datasheet_url": normalize_url(&detail.datasheet),
        "kicad_footprint": detail.package,
        "pins": pins_json,
        "parameters": parameters,
        "supply_info": supply,
    });

    Ok(serde_json::to_string(&import)?)
}

/// Parse parameter value like "1.4 V, 6 V" or "22 mA" or "4" into
/// (numeric_value, text_value, unit)
fn parse_param_value(raw: &str) -> (Option<f64>, Option<String>, Option<String>) {
    let raw = raw.trim();

    // Try to parse as plain number
    if let Ok(n) = raw.parse::<f64>() {
        return (Some(n), None, None);
    }

    // Try to extract number + unit (e.g. "22 mA", "1.4 V", "110 dB")
    // Handle comma-separated ranges like "1.4 V, 6 V" -> keep as text
    if raw.contains(',') {
        return (None, Some(raw.to_string()), None);
    }

    // Try "number unit" pattern
    let parts: Vec<&str> = raw.splitn(2, ' ').collect();
    if parts.len() == 2 {
        if let Ok(n) = parts[0].parse::<f64>() {
            return (Some(n), None, Some(parts[1].to_string()));
        }
    }

    // Fallback: store as text
    if raw.is_empty() {
        (None, None, None)
    } else {
        (None, Some(raw.to_string()), None)
    }
}

fn normalize_url(url: &str) -> String {
    if url.starts_with("//") {
        format!("https:{}", url)
    } else {
        url.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_param_numeric() {
        let (val, text, unit) = parse_param_value("4");
        assert_eq!(val, Some(4.0));
        assert_eq!(text, None);
        assert_eq!(unit, None);
    }

    #[test]
    fn test_parse_param_with_unit() {
        let (val, text, unit) = parse_param_value("22 mA");
        assert_eq!(val, Some(22.0));
        assert_eq!(text, None);
        assert_eq!(unit, Some("mA".to_string()));
    }

    #[test]
    fn test_parse_param_range() {
        let (val, text, unit) = parse_param_value("1.4 V, 6 V");
        assert_eq!(val, None);
        assert_eq!(text, Some("1.4 V, 6 V".to_string()));
        assert_eq!(unit, None);
    }

    #[test]
    fn test_parse_param_db() {
        let (val, text, unit) = parse_param_value("110 dB");
        assert_eq!(val, Some(110.0));
        assert_eq!(unit, Some("dB".to_string()));
    }
}
