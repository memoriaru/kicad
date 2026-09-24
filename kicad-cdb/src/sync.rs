use anyhow::{Context, Result};
use serde::Serialize;

use crate::db::ComponentDb;

/// Result of a sync operation
#[derive(Debug, Serialize)]
pub struct SyncResult {
    pub new_count: usize,
    pub updated_count: usize,
    pub unchanged_count: usize,
    pub errors: Vec<String>,
    pub new_mpns: Vec<String>,
    pub updated_mpns: Vec<String>,
}

/// Compute a content hash from the raw import JSON to detect changes.
/// Uses a simple deterministic string hash (FNV-1a 64-bit).
pub fn compute_content_hash(json: &str) -> String {
    let hash = fnv1a_64(json.trim().as_bytes());
    format!("{:016x}", hash)
}

/// FNV-1a 64-bit hash
fn fnv1a_64(data: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &byte in data {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

impl ComponentDb {
    /// Sync components from a JSON array — detects changes via content hash.
    /// - New components: inserted
    /// - Existing with changed hash: updated (pins/params deleted and re-inserted)
    /// - Existing with same hash: skipped
    pub fn sync_from_json(&self, json: &str) -> Result<SyncResult> {
        let items: Vec<serde_json::Value> = serde_json::from_str(json)
            .context("Failed to parse sync JSON array")?;

        let mut result = SyncResult {
            new_count: 0,
            updated_count: 0,
            unchanged_count: 0,
            errors: Vec::new(),
            new_mpns: Vec::new(),
            updated_mpns: Vec::new(),
        };

        for item in &items {
            let item_json = serde_json::to_string(item)?;
            let mpn = item.get("mpn").and_then(|v| v.as_str()).unwrap_or("");
            let manufacturer = item.get("manufacturer").and_then(|v| v.as_str()).unwrap_or("");

            if mpn.is_empty() {
                result.errors.push(format!("Missing mpn in item: {}", item_json.len()));
                continue;
            }

            let new_hash = compute_content_hash(&item_json);

            match self.get_component_by_mpn(mpn, manufacturer)? {
                Some(existing) => {
                    let existing_hash = existing.content_hash.as_deref().unwrap_or("");
                    if existing_hash == new_hash {
                        result.unchanged_count += 1;
                    } else {
                        // Delete child records and re-import
                        self.conn.execute(
                            "DELETE FROM pins WHERE component_id = ?1",
                            rusqlite::params![existing.id],
                        )?;
                        self.conn.execute(
                            "DELETE FROM parameters WHERE component_id = ?1",
                            rusqlite::params![existing.id],
                        )?;
                        self.conn.execute(
                            "DELETE FROM supply_info WHERE component_id = ?1",
                            rusqlite::params![existing.id],
                        )?;

                        // Update component with new hash
                        let mut updated = existing.clone();
                        updated.content_hash = Some(new_hash);
                        // Update fields from JSON
                        if let Some(desc) = item.get("description").and_then(|v| v.as_str()) {
                            updated.description = Some(desc.to_string());
                        }
                        if let Some(pkg) = item.get("package").and_then(|v| v.as_str()) {
                            updated.package = Some(pkg.to_string());
                        }
                        self.update_component(&updated)?;

                        // Re-insert child records
                        self.import_children(existing.id.expect("existing should have id"), item)?;

                        result.updated_count += 1;
                        result.updated_mpns.push(mpn.to_string());
                    }
                }
                None => {
                    // Import as new with content hash
                    match self.import_from_json(&item_json) {
                        Ok(comp_id) => {
                            // Set content hash on the newly imported component
                            self.conn.execute(
                                "UPDATE components SET content_hash = ?1 WHERE id = ?2",
                                rusqlite::params![new_hash, comp_id],
                            )?;
                            result.new_count += 1;
                            result.new_mpns.push(mpn.to_string());
                        }
                        Err(e) => {
                            result.errors.push(format!("{}: {}", mpn, e));
                        }
                    }
                }
            }
        }

        Ok(result)
    }

    fn import_children(&self, comp_id: i64, item: &serde_json::Value) -> Result<()> {
        if let Some(pins) = item.get("pins").and_then(|v| v.as_array()) {
            for p in pins {
                let alt_json = p.get("alt_functions").map(|v| serde_json::to_string(v).unwrap_or_default());
                self.conn.execute(
                    "INSERT INTO pins (component_id, pin_number, pin_name, pin_group, electrical_type, alt_functions, description)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    rusqlite::params![
                        comp_id,
                        p.get("number").and_then(|v| v.as_str()).unwrap_or(""),
                        p.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                        p.get("pin_group").and_then(|v| v.as_str()),
                        p.get("electrical_type").and_then(|v| v.as_str()),
                        alt_json,
                        p.get("description").and_then(|v| v.as_str()),
                    ],
                )?;
            }
        }

        if let Some(params) = item.get("parameters").and_then(|v| v.as_array()) {
            for p in params {
                self.conn.execute(
                    "INSERT INTO parameters (component_id, name, value_numeric, value_text, unit, typical, condition, source_page)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    rusqlite::params![
                        comp_id,
                        p.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                        p.get("value").and_then(|v| v.as_f64()),
                        p.get("value_text").and_then(|v| v.as_str()),
                        p.get("unit").and_then(|v| v.as_str()),
                        p.get("typical").and_then(|v| v.as_bool()).unwrap_or(false) as i32,
                        p.get("condition").and_then(|v| v.as_str()),
                        p.get("source_page").and_then(|v| v.as_str()),
                    ],
                )?;
            }
        }

        Ok(())
    }

    /// Recompute content hashes for all components missing one
    pub fn backfill_content_hashes(&self) -> Result<usize> {
        let mut stmt = self.conn.prepare(
            "SELECT id, mpn, manufacturer FROM components WHERE content_hash IS NULL"
        )?;
        let rows: Vec<(i64, String, String)> = stmt.query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?.collect::<Result<Vec<_>, _>>()?;

        let count = rows.len();
        for (id, _mpn, _mfg) in &rows {
            // Use a deterministic hash from component data
            let hash_input = format!("{}:{}", id, _mpn);
            let hash = compute_content_hash(&hash_input);
            self.conn.execute(
                "UPDATE components SET content_hash = ?1 WHERE id = ?2",
                rusqlite::params![hash, id],
            )?;
        }
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_content_hash_deterministic() {
        let json = r#"{"mpn":"TEST","pins":[]}"#;
        let h1 = compute_content_hash(json);
        let h2 = compute_content_hash(json);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 16);
    }

    #[test]
    fn test_content_hash_differs_for_different_input() {
        let h1 = compute_content_hash(r#"{"mpn":"A"}"#);
        let h2 = compute_content_hash(r#"{"mpn":"B"}"#);
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_sync_new_component() {
        let db = ComponentDb::open_in_memory().unwrap();
        db.insert_category(&crate::models::Category {
            id: None, name: "Test".to_string(), parent_id: None, description: None,
        }).unwrap();

        let json = r#"[{
            "mpn": "SYNC_TEST",
            "manufacturer": "TestMfg",
            "category": "Test",
            "auto_create_category": false,
            "description": "Sync test component",
            "pins": [
                {"number": "1", "name": "IN", "electrical_type": "input"},
                {"number": "2", "name": "OUT", "electrical_type": "output"}
            ]
        }]"#;

        let result = db.sync_from_json(json).unwrap();
        assert_eq!(result.new_count, 1);
        assert_eq!(result.updated_count, 0);
        assert_eq!(result.unchanged_count, 0);
        assert!(result.new_mpns.contains(&"SYNC_TEST".to_string()));
    }

    #[test]
    fn test_sync_unchanged() {
        let db = ComponentDb::open_in_memory().unwrap();
        db.insert_category(&crate::models::Category {
            id: None, name: "Test".to_string(), parent_id: None, description: None,
        }).unwrap();

        let json = r#"[{
            "mpn": "SYNC_TEST",
            "manufacturer": "TestMfg",
            "category": "Test",
            "description": "Test"
        }]"#;

        // First sync: new
        let r1 = db.sync_from_json(json).unwrap();
        assert_eq!(r1.new_count, 1);

        // Second sync with same data: unchanged
        let r2 = db.sync_from_json(json).unwrap();
        assert_eq!(r2.unchanged_count, 1);
    }

    #[test]
    fn test_sync_detects_change() {
        let db = ComponentDb::open_in_memory().unwrap();
        db.insert_category(&crate::models::Category {
            id: None, name: "Test".to_string(), parent_id: None, description: None,
        }).unwrap();

        let json_v1 = r#"[{
            "mpn": "SYNC_TEST",
            "manufacturer": "TestMfg",
            "category": "Test",
            "description": "Version 1"
        }]"#;

        let json_v2 = r#"[{
            "mpn": "SYNC_TEST",
            "manufacturer": "TestMfg",
            "category": "Test",
            "description": "Version 2"
        }]"#;

        let r1 = db.sync_from_json(json_v1).unwrap();
        assert_eq!(r1.new_count, 1);

        let r2 = db.sync_from_json(json_v2).unwrap();
        assert_eq!(r2.updated_count, 1);
        assert_eq!(r2.updated_mpns, vec!["SYNC_TEST"]);
    }

    #[test]
    fn test_backfill_hashes() {
        let db = ComponentDb::open_in_memory().unwrap();
        let cat_id = db.insert_category(&crate::models::Category {
            id: None, name: "Test".to_string(), parent_id: None, description: None,
        }).unwrap();

        db.insert_component(&crate::models::Component {
            id: None, mpn: "A".to_string(), manufacturer: "M".to_string(),
            category_id: cat_id, lifecycle: "active".to_string(),
            content_hash: None, ..Default::default()
        }).unwrap();

        let count = db.backfill_content_hashes().unwrap();
        assert_eq!(count, 1);
    }
}
