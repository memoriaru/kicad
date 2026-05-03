pub mod client;
pub mod parser;
pub mod sym_parser;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::ComponentDb;

pub use client::HqClient;

/// Deserialize JSON null as default (empty string for String, 0 for numbers)
fn null_to_default<'de, D, T>(deserializer: D) -> std::result::Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Default + serde::Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

/// Search result item from queryPage API
#[derive(Debug, Serialize, Deserialize)]
pub struct SearchResult {
    pub mpn: String,
    pub manufacturer: String,
    #[serde(default)]
    pub category: String,
    #[serde(rename = "category_id", default)]
    pub category_id: String,
    #[serde(default)]
    pub package: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub datasheet: String,
    #[serde(default)]
    pub footprint_file_url: String,
    #[serde(default)]
    pub footprint_name: String,
    #[serde(default)]
    pub huaqiu_pn: String,
    #[serde(rename = "manufacturer_id", default)]
    pub manufacturer_id: String,
    #[serde(default)]
    pub attrs: Vec<SearchAttr>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchAttr {
    #[serde(rename = "name_display", default)]
    pub name_display: String,
    #[serde(rename = "value_display", default)]
    pub value_display: String,
}

/// Product detail from productDetail API
#[derive(Debug, Deserialize)]
pub struct ProductDetail {
    #[serde(deserialize_with = "null_to_default")]
    pub mpn: String,
    #[serde(default, deserialize_with = "null_to_default")]
    pub mfg: String,
    #[serde(rename = "mfgId", default, deserialize_with = "null_to_default")]
    pub mfg_id: i64,
    #[serde(default, deserialize_with = "null_to_default")]
    pub description: String,
    #[serde(rename = "mfPackageCode", default, deserialize_with = "null_to_default")]
    pub package: String,
    #[serde(default, deserialize_with = "null_to_default")]
    pub datasheet: String,
    #[serde(default, deserialize_with = "null_to_default")]
    pub image: String,
    #[serde(default, rename = "cateList", deserialize_with = "null_to_default")]
    pub categories: Vec<CategoryInfo>,
    #[serde(default, rename = "groupAttrInfoVOList", deserialize_with = "null_to_default")]
    pub attr_groups: Vec<AttrGroup>,
    #[serde(default, rename = "cadUrlList", deserialize_with = "null_to_default")]
    pub cad_urls: Vec<CadUrl>,
    #[serde(default, rename = "supplyChainVOList", deserialize_with = "null_to_default")]
    pub supply_chain: Vec<serde_json::Value>,
    #[serde(default, rename = "huaqiu_pn", deserialize_with = "null_to_default")]
    pub huaqiu_pn: String,
}

#[derive(Debug, Deserialize)]
pub struct CategoryInfo {
    #[serde(rename = "cateId", deserialize_with = "null_to_default")]
    pub id: i64,
    #[serde(rename = "cateDisplayName", default, deserialize_with = "null_to_default")]
    pub display_name: String,
    #[serde(rename = "parentId", default, deserialize_with = "null_to_default")]
    pub parent_id: i64,
    #[serde(default, deserialize_with = "null_to_default")]
    pub level: i32,
}

#[derive(Debug, Deserialize)]
pub struct AttrGroup {
    #[serde(rename = "attrGroupName", default, deserialize_with = "null_to_default")]
    pub name: String,
    #[serde(rename = "attrInfoVO", default)]
    pub attrs: Vec<AttrInfo>,
}

#[derive(Debug, Deserialize)]
pub struct AttrInfo {
    #[serde(rename = "attrShortName", default, deserialize_with = "null_to_default")]
    pub short_name: String,
    #[serde(rename = "attrValue", default, deserialize_with = "null_to_default")]
    pub value: String,
    #[serde(rename = "attrName", default, deserialize_with = "null_to_default")]
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct CadUrl {
    #[serde(rename = "type", deserialize_with = "null_to_default")]
    pub url_type: String,
    #[serde(rename = "fileUrl", deserialize_with = "null_to_default")]
    pub url: String,
    #[serde(rename = "fileName", deserialize_with = "null_to_default")]
    pub name: String,
}

/// Fetch a component from HuaQiu and import into database
pub fn fetch_and_import(db: &ComponentDb, mpn: &str, mfg_id: Option<&str>) -> Result<i64> {
    let client = HqClient::new()?;

    // Step 1: Search to find manufacturer_id if not provided
    let (resolved_mfg_id, resolved_mpn) = if let Some(mid) = mfg_id {
        (mid.to_string(), mpn.to_string())
    } else {
        let results = client.search(mpn, 5)?;
        if results.is_empty() {
            anyhow::bail!("No results found for '{}'", mpn);
        }
        let first = &results[0];
        println!("Found: {} by {} (mfg_id={})", first.mpn, first.manufacturer, first.manufacturer_id);
        (first.manufacturer_id.clone(), first.mpn.clone())
    };

    // Step 2: Get product detail
    let detail = client.product_detail(&resolved_mfg_id, &resolved_mpn)?;

    // Step 3: Try to download symbol and extract pins
    let pins = if let Some(sym_url) = detail.cad_urls.iter().find(|c| c.url_type == "symbol") {
        match client.download_text(&sym_url.url) {
            Ok(sym_text) => sym_parser::extract_pins(&sym_text),
            Err(e) => {
                eprintln!("Warning: could not download symbol: {}", e);
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };

    // Step 4: Convert to import JSON and store
    let import_json = parser::detail_to_import_json(&detail, &pins)?;
    let id = db.import_from_json(&import_json)?;

    Ok(id)
}
