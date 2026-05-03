use anyhow::{Result, Context};
use rusqlite::{params, Connection, OptionalExtension};

use crate::models::*;
use crate::schema::SCHEMA_SQL;

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
        self.conn.execute_batch(SCHEMA_SQL)?;
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
        self.conn.query_row(
            "SELECT id, name, parent_id, description FROM categories WHERE id = ?1",
            params![id],
            |row| Ok(Category {
                id: Some(row.get(0)?),
                name: row.get(1)?,
                parent_id: row.get(2)?,
                description: row.get(3)?,
            }),
        ).optional().map_err(|e| e.into())
    }

    pub fn get_category_by_name(&self, name: &str) -> Result<Option<Category>> {
        self.conn.query_row(
            "SELECT id, name, parent_id, description FROM categories WHERE name = ?1",
            params![name],
            |row| Ok(Category {
                id: Some(row.get(0)?),
                name: row.get(1)?,
                parent_id: row.get(2)?,
                description: row.get(3)?,
            }),
        ).optional().map_err(|e| e.into())
    }

    pub fn get_child_categories(&self, parent_id: i64) -> Result<Vec<Category>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, parent_id, description FROM categories WHERE parent_id = ?1",
        )?;
        let cats = stmt.query_map(params![parent_id], |row| {
            Ok(Category {
                id: Some(row.get(0)?),
                name: row.get(1)?,
                parent_id: row.get(2)?,
                description: row.get(3)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(cats)
    }

    pub fn delete_category(&self, id: i64) -> Result<bool> {
        let affected = self.conn.execute("DELETE FROM categories WHERE id = ?1", params![id])?;
        Ok(affected > 0)
    }

    // --- Component CRUD ---

    pub fn insert_component(&self, comp: &Component) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO components (mpn, manufacturer, category_id, description, package, lifecycle, datasheet_url, kicad_symbol, kicad_footprint)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                comp.mpn, comp.manufacturer, comp.category_id,
                comp.description, comp.package, comp.lifecycle,
                comp.datasheet_url, comp.kicad_symbol, comp.kicad_footprint
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_component(&self, id: i64) -> Result<Option<Component>> {
        self.conn.query_row(
            "SELECT id, mpn, manufacturer, category_id, description, package, lifecycle, datasheet_url, kicad_symbol, kicad_footprint
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
            }),
        ).optional().map_err(|e| e.into())
    }

    pub fn get_component_by_mpn(&self, mpn: &str, manufacturer: &str) -> Result<Option<Component>> {
        self.conn.query_row(
            "SELECT id, mpn, manufacturer, category_id, description, package, lifecycle, datasheet_url, kicad_symbol, kicad_footprint
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
            }),
        ).optional().map_err(|e| e.into())
    }

    pub fn update_component(&self, comp: &Component) -> Result<bool> {
        let id = comp.id.context("Component id required for update")?;
        let affected = self.conn.execute(
            "UPDATE components SET mpn=?1, manufacturer=?2, category_id=?3, description=?4,
             package=?5, lifecycle=?6, datasheet_url=?7, kicad_symbol=?8, kicad_footprint=?9,
             updated_at=datetime('now') WHERE id=?10",
            params![
                comp.mpn, comp.manufacturer, comp.category_id, comp.description,
                comp.package, comp.lifecycle, comp.datasheet_url,
                comp.kicad_symbol, comp.kicad_footprint, id
            ],
        )?;
        Ok(affected > 0)
    }

    pub fn delete_component(&self, id: i64) -> Result<bool> {
        let affected = self.conn.execute("DELETE FROM components WHERE id = ?1", params![id])?;
        Ok(affected > 0)
    }

    // --- Pin CRUD ---

    pub fn insert_pin(&self, pin: &Pin) -> Result<i64> {
        let alt_json: Option<String> = match &pin.alt_functions {
            Some(v) => Some(serde_json::to_string(v).context("Failed to serialize pin alt_functions")?),
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
        let pins = stmt.query_map(params![component_id], |row| {
            let alt_json: Option<String> = row.get(6)?;
            let alt_functions = alt_json.map(|s| serde_json::from_str(&s).unwrap_or_default());
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
        })?.collect::<Result<Vec<_>, _>>()?;
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
        let params = stmt.query_map(params![component_id], |row| {
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
        })?.collect::<Result<Vec<_>, _>>()?;
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
        let models = stmt.query_map(params![component_id], |row| {
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
        })?.collect::<Result<Vec<_>, _>>()?;
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
        let infos = stmt.query_map(params![component_id], |row| {
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
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(infos)
    }

    // --- Design Rule CRUD ---

    pub fn insert_design_rule(&self, rule: &DesignRule) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO design_rules (name, category_id, description, condition_expr, formula_expr, check_expr, parameters, output_params, source)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                rule.name, rule.category_id, rule.description,
                rule.condition_expr, rule.formula_expr, rule.check_expr,
                rule.parameters, rule.output_params, rule.source
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_design_rules_by_category(&self, category_id: i64) -> Result<Vec<DesignRule>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, category_id, description, condition_expr, formula_expr, check_expr, parameters, output_params, source
             FROM design_rules WHERE category_id = ?1",
        )?;
        let rules = stmt.query_map(params![category_id], |row| {
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
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rules)
    }
}
