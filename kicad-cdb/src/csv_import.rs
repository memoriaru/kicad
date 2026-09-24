use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;

use crate::models::{Category, Component};
use crate::ComponentDb;

/// Import result summary
#[derive(Debug, serde::Serialize)]
pub struct CsvImportResult {
    pub total_rows: usize,
    pub imported: usize,
    pub skipped: usize,
    pub errors: Vec<String>,
}

/// Import components from a CSV file.
///
/// Expected CSV columns (header row required):
/// - `mpn` (required) — Manufacturer Part Number
/// - `manufacturer` — defaults to "Unknown"
/// - `category` — category name (looked up or created)
/// - `description`
/// - `package`
/// - `lifecycle` — defaults to "active"
/// - `datasheet_url`
/// - `kicad_symbol`
/// - `kicad_footprint`
/// - `model_3d_path`
/// - Any additional columns are treated as parameters (numeric values only)
pub fn import_csv(db: &ComponentDb, path: &Path) -> Result<CsvImportResult> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Cannot read CSV file: {}", path.display()))?;

    let mut reader = csv::Reader::from_reader(content.as_bytes());
    let headers = reader.headers()?.clone();

    let mpn_idx = headers
        .iter()
        .position(|h| h.trim().eq_ignore_ascii_case("mpn"))
        .ok_or_else(|| anyhow::anyhow!("CSV must have an 'mpn' column"))?;

    let category_idx = headers
        .iter()
        .position(|h| h.trim().eq_ignore_ascii_case("category"));

    // Identify parameter columns (not standard fields)
    let standard_cols = [
        "mpn",
        "manufacturer",
        "category",
        "description",
        "package",
        "lifecycle",
        "datasheet_url",
        "kicad_symbol",
        "kicad_footprint",
        "model_3d_path",
    ];
    let mut param_indices: Vec<(usize, String)> = Vec::new();
    for (i, h) in headers.iter().enumerate() {
        let name = h.trim().to_string();
        if !standard_cols.contains(&name.to_lowercase().as_str()) && i != mpn_idx {
            param_indices.push((i, name));
        }
    }

    let mut result = CsvImportResult {
        total_rows: 0,
        imported: 0,
        skipped: 0,
        errors: Vec::new(),
    };

    // Cache category IDs
    let mut cat_cache: HashMap<String, i64> = HashMap::new();

    for record in reader.records() {
        let record = match record {
            Ok(r) => r,
            Err(e) => {
                result.errors.push(format!("Parse error: {}", e));
                result.total_rows += 1;
                continue;
            }
        };

        result.total_rows += 1;

        let mpn = record.get(mpn_idx).unwrap_or("").trim().to_string();
        if mpn.is_empty() {
            result.skipped += 1;
            continue;
        }

        // Check for duplicate
        if db.get_component_by_mpn_any(&mpn).unwrap_or(None).is_some() {
            result.skipped += 1;
            continue;
        }

        // Resolve category
        let category_id = if let Some(ci) = category_idx {
            let cat_name = record.get(ci).unwrap_or("").trim().to_string();
            if cat_name.is_empty() {
                get_or_create_category(db, &mut cat_cache, "Uncategorized")?
            } else {
                get_or_create_category(db, &mut cat_cache, &cat_name)?
            }
        } else {
            get_or_create_category(db, &mut cat_cache, "Uncategorized")?
        };

        let get_col = |name: &str| -> String {
            headers
                .iter()
                .position(|h| h.trim().eq_ignore_ascii_case(name))
                .and_then(|i| record.get(i))
                .unwrap_or("")
                .trim()
                .to_string()
        };

        let comp = Component {
            id: None,
            mpn: mpn.clone(),
            manufacturer: if get_col("manufacturer").is_empty() {
                "Unknown".into()
            } else {
                get_col("manufacturer")
            },
            category_id,
            description: if get_col("description").is_empty() {
                None
            } else {
                Some(get_col("description"))
            },
            package: if get_col("package").is_empty() {
                None
            } else {
                Some(get_col("package"))
            },
            lifecycle: if get_col("lifecycle").is_empty() {
                "active".into()
            } else {
                get_col("lifecycle")
            },
            datasheet_url: if get_col("datasheet_url").is_empty() {
                None
            } else {
                Some(get_col("datasheet_url"))
            },
            kicad_symbol: if get_col("kicad_symbol").is_empty() {
                None
            } else {
                Some(get_col("kicad_symbol"))
            },
            kicad_footprint: if get_col("kicad_footprint").is_empty() {
                None
            } else {
                Some(get_col("kicad_footprint"))
            },
            symbol_lib_path: None,
            footprint_lib_path: None,
            model_3d_path: if get_col("model_3d_path").is_empty() {
                None
            } else {
                Some(get_col("model_3d_path"))
            },
        };

        match db.insert_component(&comp) {
            Ok(comp_id) => {
                // Import parameter columns
                for (idx, ref param_name) in &param_indices {
                    if let Some(val_str) = record.get(*idx) {
                        let val_str = val_str.trim();
                        if !val_str.is_empty() {
                            if let Ok(val) = val_str.parse::<f64>() {
                                let unit = extract_unit(val_str);
                                if let Err(e) = db.insert_parameter(&crate::models::Parameter {
                                    id: None,
                                    component_id: comp_id,
                                    name: param_name.clone(),
                                    value_numeric: Some(val),
                                    value_text: None,
                                    unit,
                                    typical: false,
                                    condition: None,
                                    source_page: None,
                                }) {
                                    result.errors.push(format!(
                                        "{} param '{}' import failed: {}",
                                        mpn, param_name, e
                                    ));
                                }
                            }
                        }
                    }
                }
                result.imported += 1;
            }
            Err(e) => {
                result.errors.push(format!("{}: {}", mpn, e));
                result.skipped += 1;
            }
        }
    }

    Ok(result)
}

fn get_or_create_category(
    db: &ComponentDb,
    cache: &mut HashMap<String, i64>,
    name: &str,
) -> Result<i64> {
    if let Some(&id) = cache.get(name) {
        return Ok(id);
    }

    // Try to find existing
    if let Ok(Some(cat)) = db.get_category_by_name(name) {
        let id = cat.id.context("Category missing id")?;
        cache.insert(name.to_string(), id);
        return Ok(id);
    }

    // Create new
    let id = db.insert_category(&Category {
        id: None,
        name: name.to_string(),
        parent_id: None,
        description: None,
    })?;
    cache.insert(name.to_string(), id);
    Ok(id)
}

fn extract_unit(s: &str) -> Option<String> {
    let s = s.trim();
    // Find where the numeric part ends, then take the rest as unit
    let num_end = s
        .char_indices()
        .take_while(|(_, c)| c.is_ascii_digit() || *c == '.' || *c == '-' || *c == 'e' || *c == 'E')
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0);
    let unit = s[num_end..].trim().to_string();
    if unit.is_empty() {
        None
    } else {
        Some(unit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_import_csv_basic() {
        let db = ComponentDb::open_in_memory().unwrap();

        let csv_content =
            "mpn,manufacturer,category,description,package,capacitance,voltage_rating\n\
            GRM188R71C104KA01,Murata,Capacitor,100nF 0603 MLCC,0603,1e-7,16\n\
            CL05B104KO5NNNC,Samsung,Capacitor,100nF 0402,0402,1e-7,16\n\
            GRM21BR71A106KA73,Murata,Capacitor,10uF 0805,0805,1e-5,10";

        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), csv_content).unwrap();

        let result = import_csv(&db, tmp.path()).unwrap();
        assert_eq!(result.total_rows, 3);
        assert_eq!(result.imported, 3);
        assert_eq!(result.skipped, 0);
        assert!(result.errors.is_empty());

        // Verify components exist
        let c1 = db
            .get_component_by_mpn_any("GRM188R71C104KA01")
            .unwrap()
            .unwrap();
        assert_eq!(c1.manufacturer, "Murata");
        assert_eq!(c1.package.as_deref(), Some("0603"));
    }

    #[test]
    fn test_import_csv_skip_duplicate() {
        let db = ComponentDb::open_in_memory().unwrap();

        let csv_content = "mpn,manufacturer,category\nR001,Yageo,Resistor\nR001,Yageo,Resistor";
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), csv_content).unwrap();

        let result = import_csv(&db, tmp.path()).unwrap();
        assert_eq!(result.imported, 1);
        assert_eq!(result.skipped, 1);
    }

    #[test]
    fn test_extract_unit() {
        assert_eq!(extract_unit("100nF"), Some("nF".into()));
        assert_eq!(extract_unit("16V"), Some("V".into()));
        assert_eq!(extract_unit("3.3"), None);
    }
}
