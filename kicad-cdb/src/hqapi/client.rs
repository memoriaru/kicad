use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::blocking::Client;
use serde_json::Value;

use super::{ProductDetail, SearchResult};

/// 默认的接口地址. 环境变量 `HQAPI_BASE_URL` 可以覆盖 (测试里把它指向一个没人监听的本地端口,
/// 这样"接口不可达时要优雅报错"的检查不必依赖真实网络).
const DEFAULT_BASE_URL: &str = "https://kiapi.eda.cn";
/// 建立连接的上限.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// 整个请求的上限. 调用方 (MCP 的 `serve`) 是同步循环, 一次网络调用不设上限地挂着,
/// 整个服务就不应答了.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

pub struct HqClient {
    client: Client,
    base_url: String,
}

impl HqClient {
    pub fn new() -> Result<Self> {
        let client = Client::builder()
            .user_agent("kicad-cdb/0.5")
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .context("Failed to create HTTP client")?;
        let base_url = std::env::var("HQAPI_BASE_URL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
        let base_url = base_url.trim_end_matches('/').to_string();
        Ok(Self { client, base_url })
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
            anyhow::bail!(
                "API error: {}",
                resp["message"].as_str().unwrap_or("unknown")
            );
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
            anyhow::bail!(
                "API error: {}",
                resp["message"].as_str().unwrap_or("unknown")
            );
        }

        let detail: ProductDetail =
            serde_json::from_value(resp["result"].clone()).with_context(|| {
                format!(
                    "Failed to parse product detail for {} (mfg_id={})",
                    mpn, manufacturer_id
                )
            })?;

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
        let url = format!("{}{}", self.base_url, path);

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
