use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Component {
    pub id: Option<i64>,
    pub mpn: String,
    pub manufacturer: String,
    pub category_id: i64,
    pub description: Option<String>,
    pub package: Option<String>,
    pub lifecycle: String,
    pub datasheet_url: Option<String>,
    pub kicad_symbol: Option<String>,
    pub kicad_footprint: Option<String>,
    pub symbol_lib_path: Option<String>,
    pub footprint_lib_path: Option<String>,
    pub model_3d_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Category {
    pub id: Option<i64>,
    pub name: String,
    pub parent_id: Option<i64>,
    pub description: Option<String>,
}

/// Structured alternate function for MCU pin multiplexing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PinAltFunction {
    pub function: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peripheral: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub af_num: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signal_type: Option<String>,
}

impl From<String> for PinAltFunction {
    fn from(s: String) -> Self {
        Self {
            function: s,
            peripheral: None,
            af_num: None,
            signal_type: None,
        }
    }
}

/// Lifecycle status constants
pub mod lifecycle {
    pub const ACTIVE: &str = "active";
    pub const NRND: &str = "nrnd";
    pub const OBSOLETE: &str = "obsolete";
    pub const EOL: &str = "eol";

    pub fn is_valid(status: &str) -> bool {
        matches!(status, ACTIVE | NRND | OBSOLETE | EOL)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pin {
    pub id: Option<i64>,
    pub component_id: i64,
    pub pin_number: String,
    pub pin_name: String,
    pub pin_group: Option<String>,
    pub electrical_type: Option<String>,
    pub alt_functions: Option<Vec<PinAltFunction>>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Parameter {
    pub id: Option<i64>,
    pub component_id: i64,
    pub name: String,
    pub value_numeric: Option<f64>,
    pub value_text: Option<String>,
    pub unit: Option<String>,
    pub typical: bool,
    pub condition: Option<String>,
    pub source_page: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationModel {
    pub id: Option<i64>,
    pub component_id: i64,
    pub model_type: String,
    pub model_subcategory: Option<String>,
    pub model_text: String,
    pub format: Option<String>,
    pub port_mapping: Option<String>,
    pub verified: bool,
    pub source: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesignRule {
    pub id: Option<i64>,
    pub name: String,
    pub category_id: Option<i64>,
    pub description: Option<String>,
    pub condition_expr: Option<String>,
    pub formula_expr: Option<String>,
    pub check_expr: Option<String>,
    pub parameters: Option<String>,
    pub output_params: Option<String>,
    pub source: Option<String>,
    pub domain: Option<String>,
    pub tags: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupplyInfo {
    pub id: Option<i64>,
    pub component_id: i64,
    pub supplier: String,
    pub sku: Option<String>,
    pub price_breaks: Option<String>,
    pub stock: Option<i64>,
    pub lead_time_days: Option<i64>,
    pub moq: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferenceCircuit {
    pub id: Option<i64>,
    pub component_id: i64,
    pub name: String,
    pub description: Option<String>,
    pub topology: Option<String>,
    pub circuit_json: Option<String>,
    pub notes: Option<String>,
}

/// Side-by-side parameter comparison result
#[derive(Debug, Serialize)]
pub struct DiffResult {
    pub components: Vec<Component>,
    pub parameters: Vec<DiffParameter>,
}

#[derive(Debug, Serialize)]
pub struct DiffParameter {
    pub name: String,
    pub unit: Option<String>,
    pub values: Vec<Option<f64>>,
}

/// Multi-candidate comparison result
#[derive(Debug, Serialize)]
pub struct CompareResult {
    pub rule_name: String,
    pub outputs: std::collections::HashMap<String, f64>,
    pub candidates: Vec<CandidateScore>,
}

#[derive(Debug, Serialize)]
pub struct CandidateScore {
    pub name: String,
    pub value: f64,
    pub pass: bool,
    pub margin_pct: f64,
    pub rank: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferenceDesign {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub tags: String,
    pub topology: String,
    pub requirements: String,
    pub schematic: String,
    pub parameters: String,
    pub verified: bool,
    pub created_at: String,
    pub updated_at: String,
}
