//! 载流审计示例：读落板 .kicad_pcb → 抽电源树 → 逐网载流审计。
//!
//! 用法（实板验收）：
//!   cargo run -p kicad-cdb --example current_audit -- <board.kicad_pcb> [--oz 1] [--dt 30]
//! 电流需求缺省走 VRM 铭牌表；真实负载用 net:current 覆盖（逗号分隔 NET=安培）。
use std::collections::HashMap;

use kicad_cdb::current_carry::{
    audit_current_carry, extract_power_tree, format_report, CopperParams, CurrentOverrides,
};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = match args.iter().find(|a| !a.starts_with("--")) {
        Some(p) => p.clone(),
        None => {
            eprintln!("usage: current_audit <board.kicad_pcb> [--oz 1] [--dt 30] [--net NAME=A,...]");
            std::process::exit(2);
        }
    };
    let mut oz = 1.0f64;
    let mut dt = 30.0f64;
    let mut net_cur: HashMap<String, f64> = HashMap::new();
    let mut src_cur: HashMap<String, f64> = HashMap::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--oz" => {
                oz = args[i + 1].parse().unwrap();
                i += 2;
            }
            "--dt" => {
                dt = args[i + 1].parse().unwrap();
                i += 2;
            }
            "--net" => {
                for kv in args[i + 1].split(',') {
                    let (n, a) = kv.split_once('=').expect("NAME=A");
                    net_cur.insert(n.trim().to_string(), a.trim().parse().expect("A"));
                }
                i += 2;
            }
            "--source" => {
                for kv in args[i + 1].split(',') {
                    let (n, a) = kv.split_once('=').expect("NAME=A");
                    src_cur.insert(n.trim().to_string(), a.trim().parse().expect("A"));
                }
                i += 2;
            }
            _ => i += 1,
        }
    }

    let content = std::fs::read_to_string(&path).expect("read pcb");
    let board = kicad_json5::parse_board(&content).expect("parse board");
    let ov = CurrentOverrides {
        net_current: net_cur,
        source_current: src_cur,
        copper: CopperParams { oz, dt_c: dt, internal: false },
        ..Default::default()
    };
    let vrms = extract_power_tree(&board, &ov);
    let audits = audit_current_carry(&board, &vrms, &ov);
    print!("{}", format_report(&audits, &vrms, &ov));
}
