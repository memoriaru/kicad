use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::db::ComponentDb;
use crate::models::*;

#[derive(Debug, Deserialize)]
struct ImportComponent {
    mpn: String,
    manufacturer: String,
    category: String,
    #[allow(dead_code)]
    auto_create_category: Option<bool>,
    description: Option<String>,
    package: Option<String>,
    datasheet_url: Option<String>,
    kicad_symbol: Option<String>,
    kicad_footprint: Option<String>,
    pins: Option<Vec<ImportPin>>,
    parameters: Option<Vec<ImportParameter>>,
    supply_info: Option<Vec<ImportSupply>>,
    simulation_models: Option<Vec<ImportSimModel>>,
}

#[derive(Debug, Deserialize, Serialize)]
struct ImportPin {
    number: String,
    name: String,
    pin_group: Option<String>,
    electrical_type: Option<String>,
    alt_functions: Option<AltFunctionValue>,
    description: Option<String>,
}

/// Supports both old (Vec<String>) and new (Vec<PinAltFunction>) import formats.
#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
enum AltFunctionValue {
    Simple(Vec<String>),
    Structured(Vec<PinAltFunction>),
}

impl AltFunctionValue {
    fn into_structured(self) -> Vec<PinAltFunction> {
        match self {
            AltFunctionValue::Simple(v) => v
                .into_iter()
                .map(|s| PinAltFunction {
                    function: s,
                    peripheral: None,
                    af_num: None,
                    signal_type: None,
                })
                .collect(),
            AltFunctionValue::Structured(v) => v,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct ImportParameter {
    name: String,
    value: Option<f64>,
    value_text: Option<String>,
    unit: Option<String>,
    typical: Option<bool>,
    condition: Option<String>,
    source_page: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct ImportSupply {
    supplier: String,
    sku: Option<String>,
    price_breaks: Option<serde_json::Value>,
    stock: Option<i64>,
    lead_time_days: Option<i64>,
    moq: Option<i64>,
}

#[derive(Debug, Deserialize, Serialize)]
struct ImportSimModel {
    model_type: String,
    model_subcategory: Option<String>,
    model_text: String,
    format: Option<String>,
    port_mapping: Option<String>,
    source: Option<String>,
    notes: Option<String>,
}

impl ComponentDb {
    /// Import a single component from JSON string.
    /// If the component (mpn+manufacturer) already exists, updates it (upsert).
    pub fn import_from_json(&self, json: &str) -> Result<i64> {
        let imp: ImportComponent =
            serde_json::from_str(json).context("Failed to parse import JSON")?;

        // Resolve category
        let category_id = match self.get_category_by_name(&imp.category)? {
            Some(cat) => cat.id.expect("Category from DB should have an ID"),
            None => {
                if imp.auto_create_category == Some(true) {
                    self.insert_category(&Category {
                        id: None,
                        name: imp.category.clone(),
                        parent_id: None,
                        description: None,
                    })?
                } else {
                    bail!(
                        "Category '{}' not found. Set auto_create_category=true to create it.",
                        imp.category
                    );
                }
            }
        };

        // Upsert component
        let comp_id = match self.get_component_by_mpn(&imp.mpn, &imp.manufacturer)? {
            Some(mut existing) => {
                existing.category_id = category_id;
                existing.description = imp.description.or(existing.description);
                existing.package = imp.package.or(existing.package);
                existing.datasheet_url = imp.datasheet_url.or(existing.datasheet_url);
                existing.kicad_symbol = imp.kicad_symbol.or(existing.kicad_symbol);
                existing.kicad_footprint = imp.kicad_footprint.or(existing.kicad_footprint);
                self.update_component(&existing)?;
                existing.id.expect("Existing component should have an ID")
            }
            None => self.insert_component(&Component {
                id: None,
                mpn: imp.mpn,
                manufacturer: imp.manufacturer,
                category_id,
                description: imp.description,
                package: imp.package,
                lifecycle: "active".to_string(),
                datasheet_url: imp.datasheet_url,
                kicad_symbol: imp.kicad_symbol,
                kicad_footprint: imp.kicad_footprint,
                symbol_lib_path: None,
                footprint_lib_path: None,
                model_3d_path: None,
            })?,
        };

        // Insert pins
        if let Some(pins) = imp.pins {
            for p in pins {
                self.insert_pin(&Pin {
                    id: None,
                    component_id: comp_id,
                    pin_number: p.number,
                    pin_name: p.name,
                    pin_group: p.pin_group,
                    electrical_type: p.electrical_type,
                    alt_functions: p.alt_functions.map(|v| v.into_structured()),
                    description: p.description,
                })?;
            }
        }

        // Insert parameters
        if let Some(params) = imp.parameters {
            for p in params {
                self.insert_parameter(&Parameter {
                    id: None,
                    component_id: comp_id,
                    name: p.name,
                    value_numeric: p.value,
                    value_text: p.value_text,
                    unit: p.unit,
                    typical: p.typical.unwrap_or(false),
                    condition: p.condition,
                    source_page: p.source_page,
                })?;
            }
        }

        // Insert supply info
        if let Some(supplies) = imp.supply_info {
            for s in supplies {
                let price_breaks: Option<String> = match &s.price_breaks {
                    Some(v) => {
                        Some(serde_json::to_string(v).context("Failed to serialize price_breaks")?)
                    }
                    None => None,
                };
                self.insert_supply_info(&SupplyInfo {
                    id: None,
                    component_id: comp_id,
                    supplier: s.supplier,
                    sku: s.sku,
                    price_breaks,
                    stock: s.stock,
                    lead_time_days: s.lead_time_days,
                    moq: s.moq,
                })?;
            }
        }

        // Insert simulation models
        if let Some(models) = imp.simulation_models {
            for m in models {
                self.insert_simulation_model(&SimulationModel {
                    id: None,
                    component_id: comp_id,
                    model_type: m.model_type,
                    model_subcategory: m.model_subcategory,
                    model_text: m.model_text,
                    format: m.format,
                    port_mapping: m.port_mapping,
                    verified: false,
                    source: m.source,
                    notes: m.notes,
                })?;
            }
        }

        Ok(comp_id)
    }

    /// Import multiple components from a JSON array string.
    pub fn import_batch_from_json(&self, json: &str) -> Result<Vec<i64>> {
        let imps: Vec<ImportComponent> =
            serde_json::from_str(json).context("Failed to parse batch import JSON")?;

        let mut ids = Vec::with_capacity(imps.len());
        for imp in imps {
            let single_json = serde_json::to_string(&serde_json::json!({
                "mpn": imp.mpn,
                "manufacturer": imp.manufacturer,
                "category": imp.category,
                "auto_create_category": imp.auto_create_category,
                "description": imp.description,
                "package": imp.package,
                "datasheet_url": imp.datasheet_url,
                "kicad_symbol": imp.kicad_symbol,
                "kicad_footprint": imp.kicad_footprint,
                "parameters": imp.parameters,
                "supply_info": imp.supply_info,
                "simulation_models": imp.simulation_models,
            }))?;
            ids.push(self.import_from_json(&single_json)?);
        }
        Ok(ids)
    }

    /// Import a simulation model for an existing component.
    /// Performs basic format validation for SPICE models.
    pub fn import_simulation_model(
        &self,
        component_id: i64,
        model_type: &str,
        model_subcategory: Option<&str>,
        model_text: &str,
        format: &str,
    ) -> Result<i64> {
        let mut notes: Option<String> = None;

        if model_type.eq_ignore_ascii_case("spice") {
            let validation = validate_spice(model_text);
            if !validation.is_empty() {
                notes = Some(validation);
            }
        }

        self.insert_simulation_model(&SimulationModel {
            id: None,
            component_id,
            model_type: model_type.to_string(),
            model_subcategory: model_subcategory.map(|s| s.to_string()),
            model_text: model_text.to_string(),
            format: Some(format.to_string()),
            port_mapping: None,
            verified: false,
            source: None,
            notes,
        })
    }
}

/// Basic SPICE format validation. Returns warning notes (empty string if clean).
fn validate_spice(text: &str) -> String {
    let mut warnings = Vec::new();
    let upper = text.to_uppercase();

    let has_subckt = upper.contains(".SUBCKT");
    let has_ends = upper.contains(".ENDS");

    if has_subckt && !has_ends {
        warnings.push("missing .ENDS for .SUBCKT".to_string());
    }
    if has_ends && !has_subckt {
        warnings.push(".ENDS without matching .SUBCKT".to_string());
    }

    if has_subckt {
        if let Some(line) = upper.lines().find(|l| l.trim().starts_with(".SUBCKT")) {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 3 {
                warnings.push(format!(
                    ".SUBCKT has {} tokens, expected >= 3 (name + ports)",
                    parts.len()
                ));
            }
        }
    }

    if !has_subckt && !upper.contains(".MODEL") && !upper.contains(".END") {
        warnings.push("no .SUBCKT, .MODEL, or .END directive found".to_string());
    }

    warnings.join("; ")
}
