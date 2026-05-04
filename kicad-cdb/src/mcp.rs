use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io::{self, BufRead, Write};

use crate::ComponentDb;

// ---------------------------------------------------------------------------
// JSON-RPC types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct Request {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Serialize)]
struct Response {
    jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcError>,
}

#[derive(Serialize)]
struct RpcError {
    code: i32,
    message: String,
}

// ---------------------------------------------------------------------------
// MCP tool definitions
// ---------------------------------------------------------------------------

fn tool_list() -> Vec<Value> {
    vec![
        json!({
            "name": "query_components",
            "description": "Query components from the local database with filters",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "search": { "type": "string", "description": "Full-text search (MPN, description, package)" },
                    "category": { "type": "string", "description": "Category name filter" },
                    "params": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "name": { "type": "string" },
                                "min": { "type": "number" },
                                "max": { "type": "number" }
                            },
                            "required": ["name"]
                        },
                        "description": "Parameter range filters"
                    },
                    "manufacturer": { "type": "string" },
                    "package": { "type": "string" },
                    "in_stock": { "type": "boolean" },
                    "limit": { "type": "integer" }
                }
            }
        }),
        json!({
            "name": "show_component",
            "description": "Show full details for a component by MPN",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "mpn": { "type": "string", "description": "Manufacturer Part Number" }
                },
                "required": ["mpn"]
            }
        }),
        json!({
            "name": "list_categories",
            "description": "List all component categories in the database",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "list_parameters",
            "description": "List distinct parameter names available for filtering",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "category": { "type": "string", "description": "Filter by category" }
                }
            }
        }),
        json!({
            "name": "suggest_topology",
            "description": "Suggest power topology based on voltage/current requirements",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "vin": { "type": "number", "description": "Input voltage (V)" },
                    "vout": { "type": "number", "description": "Output voltage (V)" },
                    "iout": { "type": "number", "description": "Output current (A)" },
                    "isolated": { "type": "boolean", "description": "Require galvanic isolation" }
                },
                "required": ["vin", "vout", "iout"]
            }
        }),
        json!({
            "name": "run_pipeline",
            "description": "Run a design pipeline (buck, boost, ldo, led)",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Pipeline name" },
                    "params": {
                        "type": "object",
                        "description": "Key-value parameters (e.g. {\"vin\":12, \"vout\":3.3})"
                    }
                },
                "required": ["name"]
            }
        }),
        json!({
            "name": "check_rule",
            "description": "Apply a design rule with given parameters",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "rule": { "type": "string", "description": "Rule name" },
                    "params": {
                        "type": "object",
                        "description": "Key-value numeric parameters"
                    },
                    "candidate": { "type": "string", "description": "Candidate value as name=value" }
                },
                "required": ["rule", "params"]
            }
        }),
        json!({
            "name": "search_online",
            "description": "Search HuaQiu online component library",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "keyword": { "type": "string" },
                    "limit": { "type": "integer" }
                },
                "required": ["keyword"]
            }
        }),
        json!({
            "name": "fetch_component",
            "description": "Fetch component from HuaQiu and import into local database",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "mpn": { "type": "string" },
                    "mfg_id": { "type": "string" }
                },
                "required": ["mpn"]
            }
        }),
        json!({
            "name": "recommend_components",
            "description": "Apply a design rule and search for components matching computed constraints",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "rule": { "type": "string", "description": "Rule name" },
                    "params": {
                        "type": "object",
                        "description": "Key-value numeric parameters"
                    },
                    "candidate": { "type": "string", "description": "Candidate value as name=value" },
                    "limit": { "type": "integer" }
                },
                "required": ["rule", "params"]
            }
        }),
    ]
}

// ---------------------------------------------------------------------------
// Tool dispatch
// ---------------------------------------------------------------------------

fn dispatch(db: &ComponentDb, method: &str, params: Value) -> Result<Value> {
    match method {
        "tools/list" => Ok(json!({ "tools": tool_list() })),
        "tools/call" => {
            let name = params["name"].as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing tool name"))?;
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            call_tool(db, name, args)
        }
        "initialize" => Ok(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "cdb", "version": env!("CARGO_PKG_VERSION") }
        })),
        "notifications/initialized" | "ping" => Ok(Value::Null),
        _ => Err(anyhow::anyhow!("Unknown method: {}", method)),
    }
}

fn call_tool(db: &ComponentDb, name: &str, args: Value) -> Result<Value> {
    match name {
        "query_components" => tool_query(db, &args),
        "show_component" => tool_show(db, &args),
        "list_categories" => tool_categories(db),
        "list_parameters" => tool_list_params(db, &args),
        "suggest_topology" => tool_suggest(&args),
        "run_pipeline" => tool_pipeline(db, &args),
        "check_rule" => tool_check(db, &args),
        "search_online" => tool_search_online(&args),
        "fetch_component" => tool_fetch(db, &args),
        "recommend_components" => tool_recommend(db, &args),
        _ => Err(anyhow::anyhow!("Unknown tool: {}", name)),
    }
}

// ---------------------------------------------------------------------------
// Tool implementations — use service layer, no inline SQL
// ---------------------------------------------------------------------------

fn tool_query(db: &ComponentDb, args: &Value) -> Result<Value> {
    // Extract param filters from JSON array
    let param_filter = args["params"].as_array().and_then(|arr| {
        // Use the first param filter (service::query_filtered takes a single range)
        arr.first().map(|pf| {
            let name = pf["name"].as_str().unwrap_or("");
            (name, pf["min"].as_f64(), pf["max"].as_f64())
        })
    });

    let results = crate::service::query_filtered(
        db,
        args["search"].as_str(),
        args["category"].as_str(),
        args["manufacturer"].as_str(),
        args["package"].as_str(),
        param_filter,
        args["in_stock"].as_bool().unwrap_or(false),
        args["limit"].as_u64().map(|n| n as usize),
    )?;

    Ok(json!({ "count": results.len(), "components": results }))
}

fn tool_show(db: &ComponentDb, args: &Value) -> Result<Value> {
    let mpn = args["mpn"].as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing mpn"))?;
    let comp = db.get_component_by_mpn_any(mpn)?
        .ok_or_else(|| anyhow::anyhow!("Component '{}' not found", mpn))?;

    let id = comp.id.unwrap();
    let pins = db.get_pins(id)?;
    let params = db.get_parameters(id)?;
    let models = db.get_simulation_models(id)?;
    let supply = db.get_supply_info(id)?;

    Ok(json!({
        "component": comp,
        "pins": pins,
        "parameters": params,
        "models": models,
        "supply": supply
    }))
}

fn tool_categories(db: &ComponentDb) -> Result<Value> {
    let cats: Vec<crate::Category> = db.conn
        .prepare("SELECT id, name, parent_id, description FROM categories ORDER BY name")?
        .query_map([], |row| Ok(crate::Category {
            id: Some(row.get(0)?), name: row.get(1)?, parent_id: row.get(2)?, description: row.get(3)?,
        }))?.filter_map(|r| r.ok()).collect();
    Ok(json!({ "categories": cats }))
}

fn tool_list_params(db: &ComponentDb, args: &Value) -> Result<Value> {
    let cat = args["category"].as_str();
    let names = db.list_parameter_names(cat)?;
    Ok(json!({ "parameters": names }))
}

fn tool_suggest(args: &Value) -> Result<Value> {
    let vin = args["vin"].as_f64().ok_or_else(|| anyhow::anyhow!("Missing vin"))?;
    let vout = args["vout"].as_f64().ok_or_else(|| anyhow::anyhow!("Missing vout"))?;
    let iout = args["iout"].as_f64().ok_or_else(|| anyhow::anyhow!("Missing iout"))?;
    let isolated = args["isolated"].as_bool().unwrap_or(false);
    let recs = crate::skills::suggest_topologies(vin, vout, iout, isolated);
    Ok(json!({
        "requirements": { "vin": vin, "vout": vout, "iout": iout, "isolated": isolated },
        "recommendations": recs
    }))
}

fn tool_pipeline(db: &ComponentDb, args: &Value) -> Result<Value> {
    let name = args["name"].as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing pipeline name"))?;
    let pipeline = crate::pipeline::get_builtin_pipeline(name)
        .ok_or_else(|| anyhow::anyhow!("Unknown pipeline: {}", name))?;

    let user_params: std::collections::HashMap<String, f64> = args["params"].as_object()
        .map(|obj| obj.iter()
            .filter_map(|(k, v)| v.as_f64().map(|n| (k.clone(), n)))
            .collect())
        .unwrap_or_default();

    let log = crate::pipeline::run_pipeline(db, &pipeline, &user_params)?;
    Ok(serde_json::to_value(&log)?)
}

fn tool_check(db: &ComponentDb, args: &Value) -> Result<Value> {
    let rule_name = args["rule"].as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing rule name"))?;

    // Convert JSON params to comma-separated string for service function
    let params_str = args["params"].as_object()
        .map(|obj| obj.iter()
            .filter_map(|(k, v)| v.as_f64().map(|n| format!("{}={}", k, n)))
            .collect::<Vec<_>>()
            .join(","))
        .unwrap_or_default();

    let candidate_str = args["candidate"].as_str();
    let (rule, result) = crate::service::apply_rule_with_str_params(db, rule_name, &params_str, candidate_str)?;

    Ok(json!({
        "rule": rule.name,
        "description": rule.description,
        "outputs": result.outputs,
        "check_expr": result.check_expression,
        "pass": result.pass
    }))
}

fn tool_search_online(args: &Value) -> Result<Value> {
    let keyword = args["keyword"].as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing keyword"))?;
    let limit = args["limit"].as_u64().unwrap_or(20) as usize;
    let client = crate::hqapi::HqClient::new()?;
    let results = client.search(keyword, limit)?;
    Ok(json!({ "count": results.len(), "results": results }))
}

fn tool_recommend(db: &ComponentDb, args: &Value) -> Result<Value> {
    let rule_name = args["rule"].as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing rule name"))?;

    let params_str = args["params"].as_object()
        .map(|obj| obj.iter()
            .filter_map(|(k, v)| v.as_f64().map(|n| format!("{}={}", k, n)))
            .collect::<Vec<_>>()
            .join(","))
        .unwrap_or_default();

    let candidate_str = args["candidate"].as_str();
    let limit = args["limit"].as_u64().map(|n| n as usize);

    let (rule, result, recommendations) = crate::service::recommend_components(
        db, rule_name, &params_str, candidate_str, limit,
    )?;

    Ok(json!({
        "rule": rule.name,
        "outputs": result.outputs,
        "pass": result.pass,
        "recommendation_count": recommendations.len(),
        "recommendations": recommendations,
    }))
}

fn tool_fetch(db: &ComponentDb, args: &Value) -> Result<Value> {
    let mpn = args["mpn"].as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing mpn"))?;
    let mfg_id = args["mfg_id"].as_str();
    let id = crate::hqapi::fetch_and_import(db, mpn, mfg_id)?;
    let comp = db.get_component(id)?.unwrap();
    Ok(json!({ "imported": comp, "id": id }))
}

// ---------------------------------------------------------------------------
// Server loop (stdio transport)
// ---------------------------------------------------------------------------

pub fn serve(db: &ComponentDb) -> Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() { continue; }

        let req: Request = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(e) => {
                let resp = Response {
                    jsonrpc: "2.0",
                    id: None,
                    result: None,
                    error: Some(RpcError { code: -32700, message: format!("Parse error: {}", e) }),
                };
                writeln!(stdout, "{}", serde_json::to_string(&resp)?)?;
                stdout.flush()?;
                continue;
            }
        };

        let id = req.id.clone();

        if req.method == "notifications/initialized" {
            continue;
        }

        let result = dispatch(db, &req.method, req.params);

        let resp = match result {
            Ok(val) => Response {
                jsonrpc: "2.0", id, result: Some(val), error: None,
            },
            Err(e) => Response {
                jsonrpc: "2.0", id, result: None,
                error: Some(RpcError { code: -32603, message: e.to_string() }),
            },
        };

        writeln!(stdout, "{}", serde_json::to_string(&resp)?)?;
        stdout.flush()?;
    }

    Ok(())
}
