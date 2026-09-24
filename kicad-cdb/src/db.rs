use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};

use crate::models::*;

pub struct ComponentDb {
    pub conn: Connection,
}

impl ComponentDb {
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let db = Self { conn };
        db.initialize()?;
        Ok(db)
    }

    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        let db = Self { conn };
        db.initialize()?;
        Ok(db)
    }

    fn initialize(&self) -> Result<()> {
        let version = crate::schema::run_migrations(&self.conn)?;
        if version > 0 {
            eprintln!("Schema version: {}", version);
        }
        Ok(())
    }

    // --- Category CRUD ---

    pub fn insert_category(&self, cat: &Category) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO categories (name, parent_id, description) VALUES (?1, ?2, ?3)",
            params![cat.name, cat.parent_id, cat.description],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_category(&self, id: i64) -> Result<Option<Category>> {
        self.conn
            .query_row(
                "SELECT id, name, parent_id, description FROM categories WHERE id = ?1",
                params![id],
                |row| {
                    Ok(Category {
                        id: Some(row.get(0)?),
                        name: row.get(1)?,
                        parent_id: row.get(2)?,
                        description: row.get(3)?,
                    })
                },
            )
            .optional()
            .map_err(|e| e.into())
    }

    pub fn get_category_by_name(&self, name: &str) -> Result<Option<Category>> {
        self.conn
            .query_row(
                "SELECT id, name, parent_id, description FROM categories WHERE name = ?1",
                params![name],
                |row| {
                    Ok(Category {
                        id: Some(row.get(0)?),
                        name: row.get(1)?,
                        parent_id: row.get(2)?,
                        description: row.get(3)?,
                    })
                },
            )
            .optional()
            .map_err(|e| e.into())
    }

    pub fn get_child_categories(&self, parent_id: i64) -> Result<Vec<Category>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, parent_id, description FROM categories WHERE parent_id = ?1",
        )?;
        let cats = stmt
            .query_map(params![parent_id], |row| {
                Ok(Category {
                    id: Some(row.get(0)?),
                    name: row.get(1)?,
                    parent_id: row.get(2)?,
                    description: row.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(cats)
    }

    pub fn delete_category(&self, id: i64) -> Result<bool> {
        let affected = self
            .conn
            .execute("DELETE FROM categories WHERE id = ?1", params![id])?;
        Ok(affected > 0)
    }

    // --- Component CRUD ---

    pub fn insert_component(&self, comp: &Component) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO components (mpn, manufacturer, category_id, description, package, lifecycle, datasheet_url, kicad_symbol, kicad_footprint, symbol_lib_path, footprint_lib_path, model_3d_path)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                comp.mpn, comp.manufacturer, comp.category_id,
                comp.description, comp.package, comp.lifecycle,
                comp.datasheet_url, comp.kicad_symbol, comp.kicad_footprint,
                comp.symbol_lib_path, comp.footprint_lib_path, comp.model_3d_path
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_component(&self, id: i64) -> Result<Option<Component>> {
        self.conn.query_row(
            "SELECT id, mpn, manufacturer, category_id, description, package, lifecycle, datasheet_url, kicad_symbol, kicad_footprint, symbol_lib_path, footprint_lib_path, model_3d_path
             FROM components WHERE id = ?1",
            params![id],
            |row| Ok(Component {
                id: Some(row.get(0)?),
                mpn: row.get(1)?,
                manufacturer: row.get(2)?,
                category_id: row.get(3)?,
                description: row.get(4)?,
                package: row.get(5)?,
                lifecycle: row.get(6)?,
                datasheet_url: row.get(7)?,
                kicad_symbol: row.get(8)?,
                kicad_footprint: row.get(9)?,
                symbol_lib_path: row.get(10)?,
                footprint_lib_path: row.get(11)?,
                model_3d_path: row.get(12)?,
            }),
        ).optional().map_err(|e| e.into())
    }

    pub fn get_component_by_mpn(&self, mpn: &str, manufacturer: &str) -> Result<Option<Component>> {
        self.conn.query_row(
            "SELECT id, mpn, manufacturer, category_id, description, package, lifecycle, datasheet_url, kicad_symbol, kicad_footprint, symbol_lib_path, footprint_lib_path, model_3d_path
             FROM components WHERE mpn = ?1 AND manufacturer = ?2",
            params![mpn, manufacturer],
            |row| Ok(Component {
                id: Some(row.get(0)?),
                mpn: row.get(1)?,
                manufacturer: row.get(2)?,
                category_id: row.get(3)?,
                description: row.get(4)?,
                package: row.get(5)?,
                lifecycle: row.get(6)?,
                datasheet_url: row.get(7)?,
                kicad_symbol: row.get(8)?,
                kicad_footprint: row.get(9)?,
                symbol_lib_path: row.get(10)?,
                footprint_lib_path: row.get(11)?,
                model_3d_path: row.get(12)?,
            }),
        ).optional().map_err(|e| e.into())
    }

    pub fn get_component_by_mpn_any(&self, mpn: &str) -> Result<Option<Component>> {
        self.conn.query_row(
            "SELECT id, mpn, manufacturer, category_id, description, package, lifecycle, datasheet_url, kicad_symbol, kicad_footprint, symbol_lib_path, footprint_lib_path, model_3d_path
             FROM components WHERE mpn = ?1 LIMIT 1",
            params![mpn],
            |row| Ok(Component {
                id: Some(row.get(0)?),
                mpn: row.get(1)?,
                manufacturer: row.get(2)?,
                category_id: row.get(3)?,
                description: row.get(4)?,
                package: row.get(5)?,
                lifecycle: row.get(6)?,
                datasheet_url: row.get(7)?,
                kicad_symbol: row.get(8)?,
                kicad_footprint: row.get(9)?,
                symbol_lib_path: row.get(10)?,
                footprint_lib_path: row.get(11)?,
                model_3d_path: row.get(12)?,
            }),
        ).optional().map_err(|e| e.into())
    }

    pub fn update_component(&self, comp: &Component) -> Result<bool> {
        let id = comp.id.context("Component id required for update")?;
        let affected = self.conn.execute(
            "UPDATE components SET mpn=?1, manufacturer=?2, category_id=?3, description=?4,
             package=?5, lifecycle=?6, datasheet_url=?7, kicad_symbol=?8, kicad_footprint=?9,
             symbol_lib_path=?10, footprint_lib_path=?11, model_3d_path=?12,
             updated_at=datetime('now') WHERE id=?13",
            params![
                comp.mpn,
                comp.manufacturer,
                comp.category_id,
                comp.description,
                comp.package,
                comp.lifecycle,
                comp.datasheet_url,
                comp.kicad_symbol,
                comp.kicad_footprint,
                comp.symbol_lib_path,
                comp.footprint_lib_path,
                comp.model_3d_path,
                id
            ],
        )?;
        Ok(affected > 0)
    }

    /// Update only lib path fields for a component
    pub fn update_lib_paths(
        &self,
        id: i64,
        symbol_path: Option<&str>,
        footprint_path: Option<&str>,
        model_3d_path: Option<&str>,
    ) -> Result<bool> {
        let affected = self.conn.execute(
            "UPDATE components SET symbol_lib_path=?1, footprint_lib_path=?2, model_3d_path=?3, updated_at=datetime('now') WHERE id=?4",
            params![symbol_path, footprint_path, model_3d_path, id],
        )?;
        Ok(affected > 0)
    }

    pub fn delete_component(&self, id: i64) -> Result<bool> {
        let affected = self
            .conn
            .execute("DELETE FROM components WHERE id = ?1", params![id])?;
        Ok(affected > 0)
    }

    // --- Pin CRUD ---

    pub fn insert_pin(&self, pin: &Pin) -> Result<i64> {
        let alt_json: Option<String> = match &pin.alt_functions {
            Some(v) => {
                Some(serde_json::to_string(v).context("Failed to serialize pin alt_functions")?)
            }
            None => None,
        };
        self.conn.execute(
            "INSERT INTO pins (component_id, pin_number, pin_name, pin_group, electrical_type, alt_functions, description)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![pin.component_id, pin.pin_number, pin.pin_name, pin.pin_group,
                    pin.electrical_type, alt_json, pin.description],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn insert_pins(&self, pins: &[Pin]) -> Result<Vec<i64>> {
        let mut ids = Vec::with_capacity(pins.len());
        for pin in pins {
            ids.push(self.insert_pin(pin)?);
        }
        Ok(ids)
    }

    pub fn get_pins(&self, component_id: i64) -> Result<Vec<Pin>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, component_id, pin_number, pin_name, pin_group, electrical_type, alt_functions, description
             FROM pins WHERE component_id = ?1 ORDER BY pin_number",
        )?;
        let pins = stmt
            .query_map(params![component_id], |row| {
                let alt_json: Option<String> = row.get(6)?;
                let alt_functions = alt_json.map(|s| parse_alt_functions(&s));
                Ok(Pin {
                    id: Some(row.get(0)?),
                    component_id: row.get(1)?,
                    pin_number: row.get(2)?,
                    pin_name: row.get(3)?,
                    pin_group: row.get(4)?,
                    electrical_type: row.get(5)?,
                    alt_functions,
                    description: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(pins)
    }

    // --- Parameter CRUD ---

    pub fn insert_parameter(&self, param: &Parameter) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO parameters (component_id, name, value_numeric, value_text, unit, typical, condition, source_page)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                param.component_id, param.name, param.value_numeric, param.value_text,
                param.unit, param.typical as i32, param.condition, param.source_page
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn insert_parameters(&self, params: &[Parameter]) -> Result<Vec<i64>> {
        let mut ids = Vec::with_capacity(params.len());
        for p in params {
            ids.push(self.insert_parameter(p)?);
        }
        Ok(ids)
    }

    pub fn get_parameters(&self, component_id: i64) -> Result<Vec<Parameter>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, component_id, name, value_numeric, value_text, unit, typical, condition, source_page
             FROM parameters WHERE component_id = ?1 ORDER BY name",
        )?;
        let params = stmt
            .query_map(params![component_id], |row| {
                Ok(Parameter {
                    id: Some(row.get(0)?),
                    component_id: row.get(1)?,
                    name: row.get(2)?,
                    value_numeric: row.get(3)?,
                    value_text: row.get(4)?,
                    unit: row.get(5)?,
                    typical: row.get::<_, bool>(6)?,
                    condition: row.get(7)?,
                    source_page: row.get(8)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(params)
    }

    // --- Simulation Model CRUD ---

    pub fn insert_simulation_model(&self, model: &SimulationModel) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO simulation_models (component_id, model_type, model_subcategory, model_text, format, port_mapping, verified, source, notes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                model.component_id, model.model_type, model.model_subcategory,
                model.model_text, model.format, model.port_mapping,
                model.verified as i32, model.source, model.notes
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_simulation_models(&self, component_id: i64) -> Result<Vec<SimulationModel>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, component_id, model_type, model_subcategory, model_text, format, port_mapping, verified, source, notes
             FROM simulation_models WHERE component_id = ?1",
        )?;
        let models = stmt
            .query_map(params![component_id], |row| {
                Ok(SimulationModel {
                    id: Some(row.get(0)?),
                    component_id: row.get(1)?,
                    model_type: row.get(2)?,
                    model_subcategory: row.get(3)?,
                    model_text: row.get(4)?,
                    format: row.get(5)?,
                    port_mapping: row.get(6)?,
                    verified: row.get::<_, bool>(7)?,
                    source: row.get(8)?,
                    notes: row.get(9)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(models)
    }

    // --- Supply Info CRUD ---

    pub fn insert_supply_info(&self, info: &SupplyInfo) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO supply_info (component_id, supplier, sku, price_breaks, stock, lead_time_days, moq)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                info.component_id, info.supplier, info.sku, info.price_breaks,
                info.stock, info.lead_time_days, info.moq
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_supply_info(&self, component_id: i64) -> Result<Vec<SupplyInfo>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, component_id, supplier, sku, price_breaks, stock, lead_time_days, moq
             FROM supply_info WHERE component_id = ?1",
        )?;
        let infos = stmt
            .query_map(params![component_id], |row| {
                Ok(SupplyInfo {
                    id: Some(row.get(0)?),
                    component_id: row.get(1)?,
                    supplier: row.get(2)?,
                    sku: row.get(3)?,
                    price_breaks: row.get(4)?,
                    stock: row.get(5)?,
                    lead_time_days: row.get(6)?,
                    moq: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(infos)
    }

    // --- Design Rule CRUD ---

    pub fn insert_design_rule(&self, rule: &DesignRule) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO design_rules (name, category_id, description, condition_expr, formula_expr, check_expr, parameters, output_params, source, domain, tags)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                rule.name, rule.category_id, rule.description,
                rule.condition_expr, rule.formula_expr, rule.check_expr,
                rule.parameters, rule.output_params, rule.source,
                rule.domain, rule.tags
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_design_rules_by_category(&self, category_id: i64) -> Result<Vec<DesignRule>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, category_id, description, condition_expr, formula_expr, check_expr, parameters, output_params, source, domain, tags
             FROM design_rules WHERE category_id = ?1",
        )?;
        let rules = stmt
            .query_map(params![category_id], |row| {
                Ok(DesignRule {
                    id: Some(row.get(0)?),
                    name: row.get(1)?,
                    category_id: row.get(2)?,
                    description: row.get(3)?,
                    condition_expr: row.get(4)?,
                    formula_expr: row.get(5)?,
                    check_expr: row.get(6)?,
                    parameters: row.get(7)?,
                    output_params: row.get(8)?,
                    source: row.get(9)?,
                    domain: row.get(10)?,
                    tags: row.get(11)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rules)
    }

    pub fn get_rule_by_name(&self, name: &str) -> Result<Option<DesignRule>> {
        self.conn.query_row(
            "SELECT id, name, category_id, description, condition_expr, formula_expr, check_expr, parameters, output_params, source, domain, tags
             FROM design_rules WHERE name = ?1",
            params![name],
            |row| Ok(DesignRule {
                id: Some(row.get(0)?),
                name: row.get(1)?,
                category_id: row.get(2)?,
                description: row.get(3)?,
                condition_expr: row.get(4)?,
                formula_expr: row.get(5)?,
                check_expr: row.get(6)?,
                parameters: row.get(7)?,
                output_params: row.get(8)?,
                source: row.get(9)?,
                domain: row.get(10)?,
                tags: row.get(11)?,
            }),
        ).optional().map_err(|e| e.into())
    }

    // --- Simulation Model Management ---

    pub fn list_all_simulation_models(
        &self,
        model_type: Option<&str>,
        unverified_only: bool,
    ) -> Result<Vec<(SimulationModel, String)>> {
        let mut sql = String::from(
            "SELECT sm.id, sm.component_id, sm.model_type, sm.model_subcategory, sm.model_text, sm.format, sm.port_mapping, sm.verified, sm.source, sm.notes, c.mpn \
             FROM simulation_models sm JOIN components c ON sm.component_id = c.id WHERE 1=1"
        );
        if model_type.is_some() {
            sql.push_str(" AND sm.model_type = ?");
        }
        if unverified_only {
            sql.push_str(" AND sm.verified = 0");
        }
        sql.push_str(" ORDER BY c.mpn, sm.model_type");

        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = match model_type {
            Some(mt) => stmt.query(params![mt])?,
            None => stmt.query(params![])?,
        };

        let mut results = Vec::new();
        while let Some(row) = rows.next()? {
            let model = SimulationModel {
                id: Some(row.get(0)?),
                component_id: row.get(1)?,
                model_type: row.get(2)?,
                model_subcategory: row.get(3)?,
                model_text: row.get(4)?,
                format: row.get(5)?,
                port_mapping: row.get(6)?,
                verified: row.get::<_, bool>(7)?,
                source: row.get(8)?,
                notes: row.get(9)?,
            };
            let mpn: String = row.get(10)?;
            results.push((model, mpn));
        }
        Ok(results)
    }

    pub fn verify_simulation_model(&self, model_id: i64) -> Result<bool> {
        let rows = self.conn.execute(
            "UPDATE simulation_models SET verified = 1 WHERE id = ?1",
            params![model_id],
        )?;
        Ok(rows > 0)
    }

    // --- Lifecycle Management ---

    pub fn update_lifecycle(&self, component_id: i64, status: &str) -> Result<bool> {
        let rows = self.conn.execute(
            "UPDATE components SET lifecycle = ?1, updated_at = datetime('now') WHERE id = ?2",
            params![status, component_id],
        )?;
        Ok(rows > 0)
    }

    pub fn query_by_lifecycle(&self, status: &str) -> Result<Vec<Component>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, mpn, manufacturer, category_id, description, package, lifecycle, datasheet_url, kicad_symbol, kicad_footprint, symbol_lib_path, footprint_lib_path, model_3d_path \
             FROM components WHERE lifecycle = ?1 ORDER BY mpn",
        )?;
        let comps = stmt
            .query_map(params![status], |row| {
                Ok(Component {
                    id: Some(row.get(0)?),
                    mpn: row.get(1)?,
                    manufacturer: row.get(2)?,
                    category_id: row.get(3)?,
                    description: row.get(4)?,
                    package: row.get(5)?,
                    lifecycle: row.get(6)?,
                    datasheet_url: row.get(7)?,
                    kicad_symbol: row.get(8)?,
                    kicad_footprint: row.get(9)?,
                    symbol_lib_path: row.get(10)?,
                    footprint_lib_path: row.get(11)?,
                    model_3d_path: row.get(12)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(comps)
    }

    /// Compare parameters of multiple components side-by-side.
    pub fn compare_components(&self, mpns: &[&str]) -> Result<DiffResult> {
        let mut components = Vec::new();
        let mut param_sets: Vec<Vec<Parameter>> = Vec::new();

        for mpn in mpns {
            let comp = self
                .get_component_by_mpn_any(mpn)?
                .ok_or_else(|| anyhow::anyhow!("Component '{}' not found", mpn))?;
            let params = self.get_parameters(comp.id.context("Component missing id")?)?;
            components.push(comp);
            param_sets.push(params);
        }

        let mut seen = std::collections::HashSet::new();
        let mut all_names = Vec::new();
        for params in &param_sets {
            for p in params {
                if seen.insert(p.name.clone()) {
                    all_names.push(p.name.clone());
                }
            }
        }

        let mut parameters = Vec::new();
        for name in &all_names {
            let mut unit = None;
            let mut values = Vec::new();
            for params in &param_sets {
                if let Some(p) = params.iter().find(|p| p.name == *name) {
                    if unit.is_none() {
                        unit = p.unit.clone();
                    }
                    values.push(p.value_numeric);
                } else {
                    values.push(None);
                }
            }
            parameters.push(DiffParameter {
                name: name.clone(),
                unit,
                values,
            });
        }

        Ok(DiffResult {
            components,
            parameters,
        })
    }

    // --- Reference designs ---

    pub fn save_reference_design(
        &self,
        name: &str,
        description: &str,
        tags: &str,
        topology: &str,
        requirements: &str,
        schematic: &str,
        parameters: &str,
        verified: bool,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT OR REPLACE INTO reference_designs (name, description, tags, topology, requirements, schematic, parameters, verified, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,datetime('now'))",
            params![name, description, tags, topology, requirements, schematic, parameters, verified as i32],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn list_reference_designs(
        &self,
        tag_filter: Option<&str>,
    ) -> Result<Vec<(i64, String, String, String, bool)>> {
        let mut sql =
            "SELECT id, name, description, tags, verified FROM reference_designs".to_string();
        if tag_filter.is_some() {
            sql.push_str(" WHERE tags LIKE '%' || ?1 || '%'");
        }
        sql.push_str(" ORDER BY updated_at DESC");

        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = if let Some(tag) = tag_filter {
            stmt.query(params![tag])?
        } else {
            stmt.query(params![])?
        };

        let mut result = Vec::new();
        while let Some(row) = rows.next()? {
            result.push((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get::<_, i32>(4)? != 0,
            ));
        }
        Ok(result)
    }

    pub fn get_reference_design(&self, name: &str) -> Result<Option<ReferenceDesign>> {
        self.conn.query_row(
            "SELECT id, name, description, tags, topology, requirements, schematic, parameters, verified, created_at, updated_at FROM reference_designs WHERE name = ?1",
            params![name],
            |row| Ok(ReferenceDesign {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                tags: row.get(3)?,
                topology: row.get(4)?,
                requirements: row.get(5)?,
                schematic: row.get(6)?,
                parameters: row.get(7)?,
                verified: row.get::<_, i32>(8)? != 0,
                created_at: row.get(9)?,
                updated_at: row.get(10)?,
            }),
        ).optional().map_err(Into::into)
    }

    pub fn delete_reference_design(&self, name: &str) -> Result<bool> {
        let count = self.conn.execute(
            "DELETE FROM reference_designs WHERE name = ?1",
            params![name],
        )?;
        Ok(count > 0)
    }
}

/// Parse alt_functions JSON supporting both old (Vec<String>) and new (Vec<PinAltFunction>) formats.
fn parse_alt_functions(text: &str) -> Vec<PinAltFunction> {
    if let Ok(v) = serde_json::from_str::<Vec<PinAltFunction>>(text) {
        return v;
    }
    serde_json::from_str::<Vec<String>>(text)
        .unwrap_or_default()
        .into_iter()
        .map(|s| PinAltFunction {
            function: s,
            peripheral: None,
            af_num: None,
            signal_type: None,
        })
        .collect()
}
