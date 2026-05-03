use anyhow::{Context, Result};
use reqwest::blocking::Client;
use serde_json::Value;

use super::{ProductDetail, SearchResult};

const BASE_URL: &str = "https://kiapi.eda.cn";

pub struct HqClient {
    client: Client,
}

#[derive(Debug)]
pub struct ApiError(String);

impl HqClient {
    pub fn new() -> Result<Self> {
        let client = Client::builder()
            .user_agent("kicad-cdb/0.5")
            .build()
            .context("Failed to create HTTP client")?;
        Ok(Self { client })
    }

    /// Search components by keyword
    pub fn search(&self, keyword: &str, page_size: usize) -> Result<Vec<SearchResult>> {
        let body = serde_json::json!({
            "pageNum": 1,
            "pageSize": page_size,
            "haveEdaModel": 1,
            "desc": keyword
        });

        let resp: Value = self.post("/api/chiplet/products/kicad/queryPage", &body)?;

        let code = resp["code"].as_i64().unwrap_or(0);
        if code != 200000 {
            anyhow::bail!("API error: {}", resp["message"].as_str().unwrap_or("unknown"));
        }

        let results = resp["result"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| serde_json::from_value(v.clone()).ok())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Ok(results)
    }

    /// Get product detail by manufacturer_id and mpn
    pub fn product_detail(&self, manufacturer_id: &str, mpn: &str) -> Result<ProductDetail> {
        let body = serde_json::json!({
            "manufacturer_id": manufacturer_id,
            "mpn": mpn
        });

        let resp: Value = self.post("/api/chiplet/kicad/productDetail", &body)?;

        let code = resp["code"].as_i64().unwrap_or(0);
        if code != 200000 {
            anyhow::bail!("API error: {}", resp["message"].as_str().unwrap_or("unknown"));
        }

        let detail: ProductDetail = serde_json::from_value(resp["result"].clone())
            .with_context(|| format!("Failed to parse product detail for {} (mfg_id={})", mpn, manufacturer_id))?;

        Ok(detail)
    }

    /// Search supply chain info
    pub fn supply_chain(&self, parts: &[(&str, &str)]) -> Result<Value> {
        let body: Vec<String> = parts
            .iter()
            .map(|(mfg_id, mpn)| format!("{}-{}", mfg_id, mpn))
            .collect();

        let resp: Value = self.post("/api/chiplet/kicad/searchSupplyChain", &body)?;
        Ok(resp)
    }

    /// Download text content from URL (e.g. .kicad_sym file)
    pub fn download_text(&self, url: &str) -> Result<String> {
        let url = if url.starts_with("//") {
            format!("https:{}", url)
        } else {
            url.to_string()
        };

        let text = self
            .client
            .get(&url)
            .send()
            .context(format!("Failed to download {}", url))?
            .text()
            .context("Failed to read response body")?;

        Ok(text)
    }

    fn post(&self, path: &str, body: &impl serde::Serialize) -> Result<Value> {
        let url = format!("{}{}", BASE_URL, path);

        let resp = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("X-Language", "zh")
            .json(body)
            .send()
            .context(format!("Failed to POST {}", path))?
            .json::<Value>()
            .context("Failed to parse JSON response")?;

        Ok(resp)
    }
}
