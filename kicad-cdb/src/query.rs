use anyhow::Result;
use rusqlite::params;

use crate::db::ComponentDb;
use crate::models::*;

/// Parameter range filter: (param_name, min_value, max_value)
pub struct ParamFilter<'a> {
    pub name: &'a str,
    pub min: Option<f64>,
    pub max: Option<f64>,
}

impl ComponentDb {
    /// Query components by category name
    pub fn query_components_by_category(&self, category_name: &str) -> Result<Vec<Component>> {
        let mut stmt = self.conn.prepare(
            "SELECT c.id, c.mpn, c.manufacturer, c.category_id, c.description, c.package,
                    c.lifecycle, c.datasheet_url, c.kicad_symbol, c.kicad_footprint
             FROM components c
             JOIN categories cat ON c.category_id = cat.id
             WHERE cat.name = ?1
             ORDER BY c.mpn",
        )?;
        let comps = stmt.query_map(params![category_name], |row| {
            Ok(Component {
                id: Some(row.get(0)?), mpn: row.get(1)?, manufacturer: row.get(2)?,
                category_id: row.get(3)?, description: row.get(4)?, package: row.get(5)?,
                lifecycle: row.get(6)?, datasheet_url: row.get(7)?,
                kicad_symbol: row.get(8)?, kicad_footprint: row.get(9)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(comps)
    }

    /// Query components by a single parameter range
    pub fn query_by_parameter_range(
        &self,
        param_name: &str,
        min: Option<f64>,
        max: Option<f64>,
    ) -> Result<Vec<Component>> {
        let sql = match (min, max) {
            (Some(_lo), Some(_hi)) =>
                "SELECT c.* FROM components c JOIN parameters p ON c.id = p.component_id \
                 WHERE p.name = ?1 AND p.value_numeric >= ?2 AND p.value_numeric <= ?3",
            (Some(_lo), None) =>
                "SELECT c.* FROM components c JOIN parameters p ON c.id = p.component_id \
                 WHERE p.name = ?1 AND p.value_numeric >= ?2",
            (None, Some(_hi)) =>
                "SELECT c.* FROM components c JOIN parameters p ON c.id = p.component_id \
                 WHERE p.name = ?1 AND p.value_numeric <= ?2",
            (None, None) =>
                "SELECT c.* FROM components c JOIN parameters p ON c.id = p.component_id \
                 WHERE p.name = ?1",
        };

        let mut stmt = self.conn.prepare(sql)?;
        let comps = match (min, max) {
            (Some(lo), Some(hi)) => stmt.query_map(params![param_name, lo, hi], self.map_component())?,
            (Some(lo), None) => stmt.query_map(params![param_name, lo], self.map_component())?,
            (None, Some(hi)) => stmt.query_map(params![param_name, hi], self.map_component())?,
            (None, None) => stmt.query_map(params![param_name], self.map_component())?,
        }.collect::<Result<Vec<_>, _>>()?;
        Ok(comps)
    }

    /// Query by parameter exact value within a category
    pub fn query_by_parameter_exact(
        &self,
        category_name: &str,
        param_name: &str,
        value: f64,
    ) -> Result<Vec<Component>> {
        let mut stmt = self.conn.prepare(
            "SELECT c.* FROM components c
             JOIN categories cat ON c.category_id = cat.id
             JOIN parameters p ON c.id = p.component_id
             WHERE cat.name = ?1 AND p.name = ?2 AND ABS(p.value_numeric - ?3) < 0.0001 * ?3",
        )?;
        let comps = stmt.query_map(params![category_name, param_name, value], self.map_component())?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(comps)
    }

    /// Query by text parameter content (LIKE match)
    pub fn query_by_text_parameter(&self, param_name: &str, text_contains: &str) -> Result<Vec<Component>> {
        let pattern = format!("%{}%", text_contains);
        let mut stmt = self.conn.prepare(
            "SELECT c.* FROM components c
             JOIN parameters p ON c.id = p.component_id
             WHERE p.name = ?1 AND p.value_text LIKE ?2",
        )?;
        let comps = stmt.query_map(params![param_name, pattern], self.map_component())?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(comps)
    }

    /// Query components that have stock > 0
    pub fn query_in_stock(&self) -> Result<Vec<Component>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT c.* FROM components c
             JOIN supply_info s ON c.id = s.component_id
             WHERE s.stock > 0",
        )?;
        let comps = stmt.query_map([], self.map_component())?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(comps)
    }

    /// Query by multiple parameter ranges (intersection)
    pub fn query_by_multiple_params(&self, filters: &[(&str, Option<f64>, Option<f64>)]) -> Result<Vec<Component>> {
        if filters.is_empty() {
            return Ok(vec![]);
        }

        // Build dynamic SQL with JOINs for each filter
        let mut sql = String::from("SELECT DISTINCT c.* FROM components c ");
        let mut param_idx = 1;
        for (i, _) in filters.iter().enumerate() {
            sql.push_str(&format!(
                " JOIN parameters p{} ON c.id = p{}.component_id ", i, i
            ));
        }
        sql.push_str(" WHERE ");
        let mut conditions = Vec::new();
        for (i, (_name, min, max)) in filters.iter().enumerate() {
            match (min, max) {
                (Some(_), Some(_)) => {
                    conditions.push(format!(
                        "p{}.name = ?{} AND p{}.value_numeric >= ?{} AND p{}.value_numeric <= ?{}",
                        i, param_idx, i, param_idx + 1, i, param_idx + 2
                    ));
                    param_idx += 3;
                }
                (Some(_), None) => {
                    conditions.push(format!(
                        "p{}.name = ?{} AND p{}.value_numeric >= ?{}",
                        i, param_idx, i, param_idx + 1
                    ));
                    param_idx += 2;
                }
                (None, Some(_)) => {
                    conditions.push(format!(
                        "p{}.name = ?{} AND p{}.value_numeric <= ?{}",
                        i, param_idx, i, param_idx + 1
                    ));
                    param_idx += 2;
                }
                (None, None) => {
                    conditions.push(format!("p{}.name = ?{}", i, param_idx));
                    param_idx += 1;
                }
            }
        }
        sql.push_str(&conditions.join(" AND "));

        let mut stmt = self.conn.prepare(&sql)?;

        // Collect params
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        for (name, min, max) in filters {
            param_values.push(Box::new(name.to_string()));
            if let Some(lo) = min {
                param_values.push(Box::new(lo));
            }
            if let Some(hi) = max {
                param_values.push(Box::new(hi));
            }
        }

        let param_refs: Vec<&dyn rusqlite::types::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();
        let comps = stmt.query_map(param_refs.as_slice(), self.map_component())?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(comps)
    }

    /// Get component with all its pins
    pub fn get_component_with_pins(&self, id: i64) -> Result<Option<(Component, Vec<Pin>)>> {
        match self.get_component(id)? {
            Some(comp) => {
                let pins = self.get_pins(id)?;
                Ok(Some((comp, pins)))
            }
            None => Ok(None),
        }
    }

    /// Get component with all its parameters
    pub fn get_component_with_params(&self, id: i64) -> Result<Option<(Component, Vec<Parameter>)>> {
        match self.get_component(id)? {
            Some(comp) => {
                let params = self.get_parameters(id)?;
                Ok(Some((comp, params)))
            }
            None => Ok(None),
        }
    }

    /// Full-text search across mpn, description, package
    pub fn search(&self, query: &str) -> Result<Vec<Component>> {
        let pattern = format!("%{}%", query);
        let mut stmt = self.conn.prepare(
            "SELECT id, mpn, manufacturer, category_id, description, package, lifecycle,
                    datasheet_url, kicad_symbol, kicad_footprint
             FROM components
             WHERE mpn LIKE ?1 OR description LIKE ?1 OR package LIKE ?1",
        )?;
        let comps = stmt.query_map(params![pattern], self.map_component())?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(comps)
    }

    /// Row mapper helper for Component
    fn map_component(&self) -> impl Fn(&rusqlite::Row<'_>) -> rusqlite::Result<Component> {
        |row: &rusqlite::Row<'_>| {
            Ok(Component {
                id: Some(row.get(0)?), mpn: row.get(1)?, manufacturer: row.get(2)?,
                category_id: row.get(3)?, description: row.get(4)?, package: row.get(5)?,
                lifecycle: row.get(6)?, datasheet_url: row.get(7)?,
                kicad_symbol: row.get(8)?, kicad_footprint: row.get(9)?,
            })
        }
    }

    /// List distinct parameter names, optionally filtered by category
    pub fn list_parameter_names(&self, category: Option<&str>) -> Result<Vec<String>> {
        let sql = match category {
            Some(_) =>
                "SELECT DISTINCT p.name FROM parameters p \
                 JOIN components c ON p.component_id = c.id \
                 JOIN categories cat ON c.category_id = cat.id \
                 WHERE cat.name = ?1 ORDER BY p.name",
            None =>
                "SELECT DISTINCT name FROM parameters ORDER BY name",
        };
        let mut stmt = self.conn.prepare(sql)?;
        let names: Vec<String> = match category {
            Some(cat) => {
                let rows = stmt.query_map(params![cat], |row| row.get::<_, String>(0))?;
                rows.filter_map(|r| r.ok()).collect()
            }
            None => {
                let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
                rows.filter_map(|r| r.ok()).collect()
            }
        };
        Ok(names)
    }
}
