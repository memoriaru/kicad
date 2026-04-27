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
    alt_functions: Option<Vec<String>>,
    description: Option<String>,
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
        let imp: ImportComponent = serde_json::from_str(json)
            .context("Failed to parse import JSON")?;

        // Resolve category
        let category_id = match self.get_category_by_name(&imp.category)? {
            Some(cat) => cat.id.unwrap(),
            None => {
                if imp.auto_create_category == Some(true) {
                    self.insert_category(&Category {
                        id: None,
                        name: imp.category.clone(),
                        parent_id: None,
                        description: None,
                    })?
                } else {
                    bail!("Category '{}' not found. Set auto_create_category=true to create it.", imp.category);
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
                existing.id.unwrap()
            }
            None => {
                self.insert_component(&Component {
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
                })?
            }
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
                    alt_functions: p.alt_functions,
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
                let price_breaks = s.price_breaks.map(|v| serde_json::to_string(&v).unwrap());
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
        let imps: Vec<ImportComponent> = serde_json::from_str(json)
            .context("Failed to parse batch import JSON")?;

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
    pub fn import_simulation_model(
        &self,
        component_id: i64,
        model_type: &str,
        model_subcategory: Option<&str>,
        model_text: &str,
        format: &str,
    ) -> Result<i64> {
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
            notes: None,
        })
    }
}
