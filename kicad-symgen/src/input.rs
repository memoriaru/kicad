use anyhow::{Context, Result};
use kicad_cdb::ComponentDb;

use crate::model::*;

/// Load a SymbolSpec from a JSON5 file
pub fn from_json5_file(path: &str) -> Result<SymbolSpec> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read input file: {}", path))?;
    let spec: SymbolSpec = json5::from_str(&content)
        .with_context(|| format!("Failed to parse JSON5 from: {}", path))?;
    Ok(spec)
}

/// Load a SymbolSpec from the component database by MPN
pub fn from_database(db: &ComponentDb, mpn: &str) -> Result<SymbolSpec> {
    let comp = db.get_component_by_mpn(mpn, "")
        .with_context(|| format!("Component '{}' not found in database", mpn))?;

    let comp = match comp {
        Some(c) => c,
        None => {
            // Try without manufacturer
            return Err(anyhow::anyhow!("Component '{}' not found", mpn));
        }
    };

    let id = comp.id.context("Component has no ID")?;

    let db_pins = db.get_pins(id)?;

    let pins: Vec<SymbolPin> = db_pins.into_iter().map(|p| {
        SymbolPin {
            number: p.pin_number,
            name: p.pin_name,
            electrical_type: ElectricalType::from_str_lossy(
                p.electrical_type.as_deref().unwrap_or("passive")
            ),
            pin_group: p.pin_group,
            alt_functions: p.alt_functions,
            position: None,
        }
    }).collect();

    let ref_prefix = infer_reference_prefix(&comp.category_id, &comp.kicad_symbol, &pins);

    Ok(SymbolSpec {
        mpn: comp.mpn,
        lib_name: comp.kicad_symbol.as_ref()
            .and_then(|s| s.split(':').next())
            .unwrap_or("custom")
            .to_string(),
        reference_prefix: Some(ref_prefix),
        description: comp.description,
        datasheet_url: comp.datasheet_url,
        footprint: comp.kicad_footprint,
        manufacturer: Some(comp.manufacturer),
        package: comp.package,
        pins,
    })
}

/// Load multiple SymbolSpecs from the database by category
pub fn from_database_category(db: &ComponentDb, category: &str) -> Result<Vec<SymbolSpec>> {
    let components = db.query_components_by_category(category)?;
    let mut specs = Vec::new();
    for comp in components {
        let id = match comp.id {
            Some(id) => id,
            None => continue,
        };
        let db_pins = db.get_pins(id)?;
        let pins: Vec<SymbolPin> = db_pins.into_iter().map(|p| {
            SymbolPin {
                number: p.pin_number,
                name: p.pin_name,
                electrical_type: ElectricalType::from_str_lossy(
                    p.electrical_type.as_deref().unwrap_or("passive")
                ),
                pin_group: p.pin_group,
                alt_functions: p.alt_functions,
                position: None,
            }
        }).collect();

        let ref_prefix = infer_reference_prefix(&comp.category_id, &comp.kicad_symbol, &pins);

        specs.push(SymbolSpec {
            mpn: comp.mpn,
            lib_name: comp.kicad_symbol.as_ref()
                .and_then(|s| s.split(':').next())
                .unwrap_or("custom")
                .to_string(),
            reference_prefix: Some(ref_prefix),
            description: comp.description,
            datasheet_url: comp.datasheet_url,
            footprint: comp.kicad_footprint,
            manufacturer: Some(comp.manufacturer),
            package: comp.package,
            pins,
        });
    }
    Ok(specs)
}

fn infer_reference_prefix(
    _category_id: &i64,
    kicad_symbol: &Option<String>,
    pins: &[SymbolPin],
) -> String {
    if let Some(sym) = kicad_symbol {
        let short = sym.split(':').last().unwrap_or(sym);
        let upper = short.to_uppercase();
        // Standard KiCad library naming conventions
        if upper.starts_with("R") && !upper.starts_with("REG") && !upper.contains("RELAY") {
            return "R".to_string();
        }
        if upper.starts_with("C") && !upper.starts_with("CONN") && !upper.starts_with("CRYSTAL") {
            return "C".to_string();
        }
        if upper.starts_with("L") && !upper.starts_with("LED") && !upper.starts_with("LCD") {
            return "L".to_string();
        }
        if upper.starts_with("LED") {
            return "D".to_string();
        }
        if upper.starts_with("D") && !upper.starts_with("DIP") {
            return "D".to_string();
        }
        if upper.starts_with("CONN") || upper.starts_with("J") {
            return "J".to_string();
        }
        if upper.starts_with("SW") {
            return "SW".to_string();
        }
        if upper.starts_with("CRYSTAL") || upper.starts_with("XTAL") {
            return "Y".to_string();
        }
    }

    // Fallback: 2-pin passives get "R"/"C"/"L", others get "U"
    if pins.len() <= 2 {
        "U".to_string()
    } else {
        "U".to_string()
    }
}
