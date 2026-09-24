//! Per-tool deep semantic tests: drive tools/call against a SEEDED database
//! and assert actual tool semantics (filtering, math, lifecycle), not just
//! protocol shape.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

use kicad_cdb::models::DesignRule;
use kicad_cdb::models::{Category, Component, Parameter, Pin, SupplyInfo};

// ── Harness ────────────────────────────────────────────────────────

struct Server {
    child: Child,
    stdin: std::process::ChildStdin,
    lines: Receiver<String>,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn spawn_server(db_path: &std::path::Path) -> Server {
    let mut child = Command::new(env!("CARGO_BIN_EXE_kdesign"))
        .arg("--db")
        .arg(db_path)
        .arg("serve")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn kdesign serve");
    let stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout)
            .lines()
            .map_while(std::result::Result::ok)
        {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    Server {
        child,
        stdin,
        lines: rx,
    }
}

fn call(server: &mut Server, tool: &str, args: serde_json::Value, id: u64) {
    let req = serde_json::json!({
        "jsonrpc": "2.0", "id": id,
        "method": "tools/call",
        "params": { "name": tool, "arguments": args }
    });
    writeln!(server.stdin, "{}", req).expect("write");
    server.stdin.flush().expect("flush");
}

fn result_of(resp: &serde_json::Value) -> serde_json::Value {
    assert!(resp["error"].is_null(), "tool error: {}", resp["error"]);
    resp["result"].clone()
}

fn seed_db(path: &std::path::Path) {
    let db = kicad_cdb::db::ComponentDb::open(path.to_str().unwrap()).expect("seed db open");

    let cat_cap = db
        .insert_category(&Category {
            id: None,
            name: "passive/capacitor".into(),
            parent_id: None,
            description: None,
        })
        .unwrap();
    let cat_res = db
        .insert_category(&Category {
            id: None,
            name: "passive/resistor".into(),
            parent_id: None,
            description: None,
        })
        .unwrap();
    let cat_ldo = db
        .insert_category(&Category {
            id: None,
            name: "power/ldo".into(),
            parent_id: None,
            description: None,
        })
        .unwrap();

    let comp = |mpn: &str, mfg: &str, cat: i64, pkg: &str, desc: &str| Component {
        id: None,
        mpn: mpn.into(),
        manufacturer: mfg.into(),
        category_id: cat,
        description: Some(desc.into()),
        package: Some(pkg.into()),
        lifecycle: "active".into(),
        datasheet_url: None,
        kicad_symbol: None,
        kicad_footprint: None,
        symbol_lib_path: None,
        footprint_lib_path: None,
        model_3d_path: None,
    };

    let c100n = db
        .insert_component(&comp(
            "CL10B104KB8NNNC",
            "Samsung",
            cat_cap,
            "0603",
            "100nF 25V X7R",
        ))
        .unwrap();
    let c10u = db
        .insert_component(&comp(
            "GRM188R61E106MA73D",
            "Murata",
            cat_cap,
            "0603",
            "10uF 25V X5R",
        ))
        .unwrap();
    let _r10k = db
        .insert_component(&comp(
            "RC0603FR-0710KL",
            "Yageo",
            cat_res,
            "0603",
            "10kΩ 1%",
        ))
        .unwrap();
    let ldo = db
        .insert_component(&comp(
            "RT9013-33GB",
            "Richtek",
            cat_ldo,
            "SOT-23-5",
            "300mA LDO 3.3V",
        ))
        .unwrap();

    let param = |cid: i64, name: &str, v: f64, unit: &str| Parameter {
        id: None,
        component_id: cid,
        name: name.into(),
        value_numeric: Some(v),
        value_text: None,
        unit: Some(unit.into()),
        typical: true,
        condition: None,
        source_page: None,
    };
    db.insert_parameter(&param(c100n, "capacitance", 1.0e-7, "F"))
        .unwrap();
    db.insert_parameter(&param(c10u, "capacitance", 1.0e-5, "F"))
        .unwrap();
    db.insert_parameter(&param(_r10k, "resistance", 1.0e4, "Ω"))
        .unwrap();
    db.insert_parameter(&param(ldo, "vdropout_max", 1.0, "V"))
        .unwrap();

    db.insert_pin(&Pin {
        id: None,
        component_id: ldo,
        pin_number: "1".into(),
        pin_name: "VIN".into(),
        pin_group: None,
        electrical_type: Some("power_in".into()),
        alt_functions: None,
        description: None,
    })
    .unwrap();
    db.insert_pin(&Pin {
        id: None,
        component_id: ldo,
        pin_number: "5".into(),
        pin_name: "VOUT".into(),
        pin_group: None,
        electrical_type: Some("power_out".into()),
        alt_functions: None,
        description: None,
    })
    .unwrap();

    db.insert_supply_info(&SupplyInfo {
        id: None,
        component_id: c100n,
        supplier: "LCSC".into(),
        sku: Some("C14663".into()),
        price_breaks: Some("[[1,0.10],[10,0.06],[100,0.04]]".into()),
        stock: Some(5000),
        lead_time_days: None,
        moq: None,
    })
    .unwrap();
    db.insert_supply_info(&SupplyInfo {
        id: None,
        component_id: ldo,
        supplier: "LCSC".into(),
        sku: Some("C134139".into()),
        price_breaks: Some("[[1,0.85],[10,0.72]]".into()),
        stock: Some(1200),
        lead_time_days: None,
        moq: None,
    })
    .unwrap();

    // Rule for check_rule / recommend semantics
    db.insert_design_rule(&DesignRule {
        id: None,
        name: "ldo_dropout_check".into(),
        category_id: Some(cat_ldo),
        description: Some("Verify LDO dropout voltage is within spec".into()),
        condition_expr: None,
        formula_expr: Some("dropout = vin - vout".into()),
        check_expr: Some("dropout >= vdropout_max".into()),
        parameters: Some(r#"["vin", "vout", "vdropout_max"]"#.into()),
        output_params: Some(r#"["dropout"]"#.into()),
        source: None,
        domain: None,
        tags: None,
    })
    .unwrap();
}

fn setup(tag: &str) -> (std::path::PathBuf, Server) {
    let dir = std::env::temp_dir().join(format!("kdesign-semantic-{}-{}", tag, std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("components.db");
    seed_db(&db);
    let srv = spawn_server(&db);
    (db, srv)
}

// ── A. DB 读类语义 ────────────────────────────────────────────────

#[test]
fn query_components_filters() {
    let (_db, mut srv) = setup("query");

    // category 过滤
    call(
        &mut srv,
        "query_components",
        serde_json::json!({ "category": "passive/capacitor" }),
        1,
    );
    let r = result_of(&read_last(&mut srv));
    assert_eq!(r["count"], 2, "two seeded capacitors");

    // 全文搜索
    call(
        &mut srv,
        "query_components",
        serde_json::json!({ "search": "RT9013" }),
        2,
    );
    let r = result_of(&read_last(&mut srv));
    assert_eq!(r["count"], 1);
    assert_eq!(r["components"][0]["mpn"], "RT9013-33GB");

    // manufacturer 唯一选择器(曾恒返 0 的缺陷, query_filtered 已修为全库起点)
    call(
        &mut srv,
        "query_components",
        serde_json::json!({ "manufacturer": "Samsung" }),
        3,
    );
    let r = result_of(&read_last(&mut srv));
    assert_eq!(r["count"], 1);

    // limit 截断
    call(
        &mut srv,
        "query_components",
        serde_json::json!({ "category": "passive/capacitor", "limit": 1 }),
        4,
    );
    let r = result_of(&read_last(&mut srv));
    assert_eq!(r["count"], 1);
}

#[test]
fn query_components_param_range() {
    let (_db, mut srv) = setup("range");
    // capacitance ∈ [1e-6, 1e-3] → 只有 10uF (1e-5), 排除 100nF (1e-7)
    call(
        &mut srv,
        "query_components",
        serde_json::json!({
            "category": "passive/capacitor",
            "params": [{ "name": "capacitance", "min": 1.0e-6, "max": 1.0e-3 }]
        }),
        1,
    );
    let r = result_of(&read_last(&mut srv));
    assert_eq!(r["count"], 1, "param range must select only 10uF: {}", r);
    assert_eq!(r["components"][0]["mpn"], "GRM188R61E106MA73D");
}

#[test]
fn show_component_full_payload() {
    let (_db, mut srv) = setup("show");
    call(
        &mut srv,
        "show_component",
        serde_json::json!({ "mpn": "RT9013-33GB" }),
        1,
    );
    let r = result_of(&read_last(&mut srv));
    assert_eq!(r["component"]["mpn"], "RT9013-33GB");
    assert_eq!(r["component"]["package"], "SOT-23-5");
    assert_eq!(r["pins"].as_array().unwrap().len(), 2, "seeded pins");
    assert_eq!(r["parameters"][0]["name"], "vdropout_max");
    assert!(!r["supply"].as_array().unwrap().is_empty());

    // 缺参 / 未知 mpn → 结构化错误
    call(&mut srv, "show_component", serde_json::json!({}), 2);
    let resp = read_last(&mut srv);
    assert_eq!(resp["error"]["code"], -32603);
    call(
        &mut srv,
        "show_component",
        serde_json::json!({ "mpn": "NOPE-123" }),
        3,
    );
    let resp = read_last(&mut srv);
    assert_eq!(resp["error"]["code"], -32603);
    assert!(resp["error"]["message"]
        .as_str()
        .unwrap()
        .contains("not found"));
}

#[test]
fn categories_and_parameter_names() {
    let (_db, mut srv) = setup("cats");
    call(&mut srv, "list_categories", serde_json::json!({}), 1);
    let r = result_of(&read_last(&mut srv));
    let names: Vec<&str> = r["categories"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"passive/capacitor"));
    assert!(names.contains(&"power/ldo"));

    call(
        &mut srv,
        "list_parameters",
        serde_json::json!({ "category": "passive/capacitor" }),
        2,
    );
    let r = result_of(&read_last(&mut srv));
    let s = r.to_string();
    assert!(
        s.contains("capacitance"),
        "parameter names include capacitance: {}",
        s
    );
}

// ── B. 纯参数类语义 ───────────────────────────────────────────────

#[test]
fn suggest_topology_semantics() {
    let (_db, mut srv) = setup("topo");
    call(
        &mut srv,
        "suggest_topology",
        serde_json::json!({ "vin": 12.0, "vout": 3.3, "iout": 1.0 }),
        1,
    );
    let r = result_of(&read_last(&mut srv));
    assert_eq!(r["requirements"]["vout"], 3.3);
    assert!(
        !r["recommendations"].as_array().unwrap().is_empty(),
        "buck-range input yields recommendations"
    );

    // 缺必填参数 → 结构化错误
    call(
        &mut srv,
        "suggest_topology",
        serde_json::json!({ "vout": 3.3 }),
        2,
    );
    let resp = read_last(&mut srv);
    assert_eq!(resp["error"]["code"], -32603);
}

#[test]
fn list_ic_types_populated() {
    let (_db, mut srv) = setup("ictypes");
    call(&mut srv, "list_ic_types", serde_json::json!({}), 1);
    let r = result_of(&read_last(&mut srv));
    assert!(
        r["total"].as_u64().unwrap() > 0,
        "IC knowledge base non-empty"
    );
}

// ── C. 规则引擎语义 ──────────────────────────────────────────────

#[test]
fn check_rule_dropout_math() {
    let (_db, mut srv) = setup("check");

    // dropout = vin - vout = 5 - 3.3 = 1.7 >= 1.0 → PASS
    call(
        &mut srv,
        "check_rule",
        serde_json::json!({
            "rule": "ldo_dropout_check",
            "params": { "vin": 5.0, "vout": 3.3, "vdropout_max": 1.0 }
        }),
        1,
    );
    let r = result_of(&read_last(&mut srv));
    assert_eq!(r["rule"], "ldo_dropout_check");
    assert_eq!(r["pass"], true);
    let dropout = r["outputs"]["dropout"].as_f64().expect("dropout output");
    assert!(
        (dropout - 1.7).abs() < 1e-9,
        "5 - 3.3 = 1.7 (f64 舍入容差), got {}",
        dropout
    );

    // dropout = 3.6 - 3.3 = 0.3 < 1.0 → FAIL
    call(
        &mut srv,
        "check_rule",
        serde_json::json!({
            "rule": "ldo_dropout_check",
            "params": { "vin": 3.6, "vout": 3.3, "vdropout_max": 1.0 }
        }),
        2,
    );
    let r = result_of(&read_last(&mut srv));
    assert_eq!(r["pass"], false);

    // 未知规则 → 结构化错误
    call(
        &mut srv,
        "check_rule",
        serde_json::json!({ "rule": "no_such_rule" }),
        3,
    );
    let resp = read_last(&mut srv);
    assert_eq!(resp["error"]["code"], -32603);
}

#[test]
fn recommend_components_responds_with_rule_verdict() {
    let (_db, mut srv) = setup("recommend");
    call(
        &mut srv,
        "recommend_components",
        serde_json::json!({
            "rule": "ldo_dropout_check",
            "params": { "vin": 5.0, "vout": 3.3, "vdropout_max": 1.0 },
            "candidate": "RT9013-33GB",
            "limit": 5
        }),
        1,
    );
    let r = result_of(&read_last(&mut srv));
    assert_eq!(r["rule"], "ldo_dropout_check");
    assert_eq!(r["pass"], true);
    assert!(r["recommendations"].is_array());
}

// ── D. 成本与替代料语义 ──────────────────────────────────────────

#[test]
fn estimate_bom_cost_applies_price_break_tiers() {
    let (_db, mut srv) = setup("bom");
    // CL10B104 price breaks: [[1,0.10],[10,0.06],[100,0.04]] — qty 10 → 单价 0.06
    call(
        &mut srv,
        "estimate_bom_cost",
        serde_json::json!({
            "items": [ { "mpn": "CL10B104KB8NNNC", "quantity": 10 } ]
        }),
        1,
    );
    let r = result_of(&read_last(&mut srv));
    let total = r["total_cost"].as_f64().expect("total_cost present");
    assert!(
        (total - 0.6).abs() < 1e-6,
        "10 × 0.06 = 0.60, got {}",
        total
    );

    // 未知 mpn → 行项目单价为 null, 不崩
    call(
        &mut srv,
        "estimate_bom_cost",
        serde_json::json!({
            "items": [ { "mpn": "UNKNOWN-MPN", "quantity": 5 } ]
        }),
        2,
    );
    let r = result_of(&read_last(&mut srv));
    assert!(r["total_cost"].is_f64());

    // 空 items → 结构化错误
    call(
        &mut srv,
        "estimate_bom_cost",
        serde_json::json!({ "items": [] }),
        3,
    );
    let resp = read_last(&mut srv);
    assert_eq!(resp["error"]["code"], -32603);
}

#[test]
fn find_alternatives_same_package_same_category() {
    let (_db, mut srv) = setup("alts");
    // 0603 电容的替代: 同 category + 同 package + active, 原件排除
    call(
        &mut srv,
        "find_alternatives",
        serde_json::json!({
            "mpn": "CL10B104KB8NNNC", "limit": 5
        }),
        1,
    );
    let r = result_of(&read_last(&mut srv));
    assert_eq!(r["original_mpn"], "CL10B104KB8NNNC");
    let mpns: Vec<&str> = r["alternatives"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|a| a["mpn"].as_str())
        .collect();
    assert!(
        !mpns.contains(&"CL10B104KB8NNNC"),
        "original must be excluded"
    );
    assert!(
        mpns.contains(&"GRM188R61E106MA73D"),
        "same-package cap should rank in: {:?}",
        mpns
    );
}

// ── E. 持久化生命周期 ────────────────────────────────────────────

#[test]
fn design_save_load_list_versions() {
    let (_db, mut srv) = setup("design");
    let spec = serde_json::json!({
        "type": "power_board",
        "vin": 12.0,
        "outputs": [ { "vout": 3.3, "iout": 0.5, "name": "3V3" } ]
    });

    call(
        &mut srv,
        "save_design",
        serde_json::json!({ "name": "e2e-design", "spec": spec }),
        1,
    );
    let r = result_of(&read_last(&mut srv));
    let design_id = r["design_id"].as_str().expect("design_id").to_string();
    assert_eq!(r["version"], 1);

    call(&mut srv, "list_designs", serde_json::json!({}), 2);
    let r = result_of(&read_last(&mut srv));
    let names: Vec<&str> = r["designs"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|d| d["name"].as_str())
        .collect();
    assert!(names.contains(&"e2e-design"));

    call(
        &mut srv,
        "load_design",
        serde_json::json!({ "design_id": design_id }),
        3,
    );
    let r = result_of(&read_last(&mut srv));
    assert_eq!(r["name"], "e2e-design");
    assert_eq!(r["spec"]["vin"], 12.0);

    call(
        &mut srv,
        "list_design_versions",
        serde_json::json!({ "design_id": design_id }),
        4,
    );
    let r = result_of(&read_last(&mut srv));
    assert!(r["total"].as_u64().unwrap() >= 1);

    call(
        &mut srv,
        "restore_design_version",
        serde_json::json!({ "design_id": design_id, "version": 1 }),
        5,
    );
    let r = result_of(&read_last(&mut srv));
    assert_eq!(r["version"], 1);
}

#[test]
fn reference_design_lifecycle() {
    let (_db, mut srv) = setup("refdes");
    call(
        &mut srv,
        "save_reference_design",
        serde_json::json!({
            "name": "ldo-3v3-ref",
            "description": "3.3V LDO reference",
            "tags": "ldo,power",
            "topology": "ldo",
            "verified": true
        }),
        1,
    );
    let r = result_of(&read_last(&mut srv));
    assert_eq!(r["saved"], true);

    call(
        &mut srv,
        "get_reference_design",
        serde_json::json!({ "name": "ldo-3v3-ref" }),
        2,
    );
    let r = result_of(&read_last(&mut srv));
    assert_eq!(r["name"], "ldo-3v3-ref");
    assert_eq!(r["verified"], true);

    call(
        &mut srv,
        "list_reference_designs",
        serde_json::json!({ "tag": "power" }),
        3,
    );
    let r = result_of(&read_last(&mut srv));
    assert_eq!(r["count"], 1, "tag filter matches");
}

// ── F. 外部依赖类: 优雅报错而非崩溃 ─────────────────────────────

#[test]
fn external_dependency_tools_fail_gracefully() {
    let (_db, mut srv) = setup("ext");
    // kicad-cli 不存在/输入文件缺失 → 结构化 RPC 错误
    call(
        &mut srv,
        "run_drc",
        serde_json::json!({ "input": "/nonexistent-board.kicad_pcb" }),
        1,
    );
    let resp = read_last(&mut srv);
    assert_eq!(resp["error"]["code"], -32603, "graceful DRC error");

    call(
        &mut srv,
        "run_erc",
        serde_json::json!({ "input": "/nonexistent.kicad_sch" }),
        2,
    );
    let resp = read_last(&mut srv);
    assert_eq!(resp["error"]["code"], -32603, "graceful ERC error");

    // 在线拉取(真实网络): 模糊搜索可能命中也可能拒绝——两者都是合法响应,
    // 关键断言是响应为合法 JSON-RPC 且服务器随后仍存活
    call(
        &mut srv,
        "fetch_component",
        serde_json::json!({ "mpn": "RT9013-33GB" }),
        3,
    );
    let resp = read_last(&mut srv);
    assert!(
        resp["result"].is_object() || resp["error"].is_object(),
        "fetch must yield a JSON-RPC response, got: {}",
        resp
    );

    // 之后服务器仍正常应答
    call(&mut srv, "list_categories", serde_json::json!({}), 4);
    let _ = result_of(&read_last(&mut srv));
}

// ── helper: 读取最近一次响应 ─────────────────────────────────────

fn read_last(server: &mut Server) -> serde_json::Value {
    let line = server
        .lines
        .recv_timeout(Duration::from_secs(30))
        .expect("timeout waiting for response");
    serde_json::from_str::<serde_json::Value>(&line)
        .unwrap_or_else(|e| panic!("non-JSON line on channel: {line:?} ({e})"))
}
