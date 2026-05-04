use anyhow::Result;
use rusqlite::params;
use serde::Serialize;

use crate::ComponentDb;

/// Supplier information for a BOM entry
#[derive(Debug, Serialize)]
pub struct SupplierInfo {
    pub supplier: String,
    pub sku: Option<String>,
    pub stock: Option<i64>,
    pub price_breaks: Option<String>,
    pub moq: Option<i64>,
}

/// Aggregated BOM entry
#[derive(Debug, Serialize)]
pub struct BomEntry {
    pub references: Vec<String>,
    pub mpn: String,
    pub manufacturer: Option<String>,
    pub value: Option<String>,
    pub footprint: Option<String>,
    pub category: String,
    pub description: Option<String>,
    pub quantity: usize,
    pub suppliers: Vec<SupplierInfo>,
}

/// Row from the BOM query
struct BomRow {
    mpn: String,
    manufacturer: Option<String>,
    value: Option<String>,
    footprint: Option<String>,
    category: String,
    description: Option<String>,
}

/// Generate BOM from all components in the database.
/// Aggregates by (category, mpn, footprint).
pub fn generate_bom(db: &ComponentDb) -> Result<Vec<BomEntry>> {
    let mut stmt = db.conn.prepare(
        "SELECT c.mpn, c.manufacturer, c.description, c.package,
                COALESCE(cat.name, 'uncategorized') as category
         FROM components c
         LEFT JOIN categories cat ON c.category_id = cat.id
         ORDER BY category, c.mpn",
    )?;

    let rows: Vec<BomRow> = stmt.query_map([], |row| {
        Ok(BomRow {
            mpn: row.get(0)?,
            manufacturer: row.get(1)?,
            value: None,
            footprint: row.get(3)?,
            category: row.get(4)?,
            description: row.get(2)?,
        })
    })?.filter_map(|r| r.ok()).collect();

    // Try to get a "value" parameter for each component
    let mut entries: Vec<BomEntry> = Vec::new();
    for row in &rows {
        // Look up supply info
        let comp_id: Option<i64> = db.conn.query_row(
            "SELECT id FROM components WHERE mpn = ?1 LIMIT 1",
            params![row.mpn],
            |r| r.get(0),
        ).ok();

        let suppliers = if let Some(cid) = comp_id {
            db.get_supply_info(cid)?
                .into_iter()
                .map(|s| SupplierInfo {
                    supplier: s.supplier,
                    sku: s.sku,
                    stock: s.stock,
                    price_breaks: s.price_breaks,
                    moq: s.moq,
                })
                .collect()
        } else {
            Vec::new()
        };

        entries.push(BomEntry {
            references: Vec::new(),
            mpn: row.mpn.clone(),
            manufacturer: row.manufacturer.clone(),
            value: row.value.clone(),
            footprint: row.footprint.clone(),
            category: row.category.clone(),
            description: row.description.clone(),
            quantity: 1,
            suppliers,
        });
    }

    Ok(entries)
}

/// Convert BOM entries to CSV string
pub fn bom_to_csv(entries: &[BomEntry]) -> Result<String> {
    let mut wtr = csv::Writer::from_writer(Vec::new());
    // Header
    wtr.write_record(&["Qty", "MPN", "Manufacturer", "Value", "Footprint", "Category", "Description", "Supplier", "SKU", "Stock"])?;

    for entry in entries {
        if entry.suppliers.is_empty() {
            wtr.write_record(&[
                entry.quantity.to_string().as_str(),
                &entry.mpn,
                entry.manufacturer.as_deref().unwrap_or(""),
                entry.value.as_deref().unwrap_or(""),
                entry.footprint.as_deref().unwrap_or(""),
                &entry.category,
                entry.description.as_deref().unwrap_or(""),
                "", "", "",
            ])?;
        } else {
            for sup in &entry.suppliers {
                wtr.write_record(&[
                    entry.quantity.to_string().as_str(),
                    &entry.mpn,
                    entry.manufacturer.as_deref().unwrap_or(""),
                    entry.value.as_deref().unwrap_or(""),
                    entry.footprint.as_deref().unwrap_or(""),
                    &entry.category,
                    entry.description.as_deref().unwrap_or(""),
                    &sup.supplier,
                    sup.sku.as_deref().unwrap_or(""),
                    sup.stock.map(|s| s.to_string()).unwrap_or_default().as_str(),
                ])?;
            }
        }
    }

    let bytes = wtr.into_inner()?;
    Ok(String::from_utf8(bytes)?)
}

/// Convert BOM entries to JSON string
pub fn bom_to_json(entries: &[BomEntry]) -> Result<String> {
    Ok(serde_json::to_string_pretty(&entries)?)
}
