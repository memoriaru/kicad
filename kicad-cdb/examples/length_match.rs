//! 分组等长 CLI：注入约束声明/模板 → resolve → 布线后等长报告。
//!
//!   length_match <board.kicad_pcb> --template ddr3-1600
//!   length_match <board.kicad_pcb> --constraints constraints.json5
//!   length_match <board.kicad_pcb> --pair DIFF_P,DIFF_N   # 快捷差分对
use kicad_cdb::constraint::{
    eval_length_matching, format_length_report, parse_constraints, resolve, template,
};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = match args.iter().find(|a| !a.starts_with("--")) {
        Some(p) => p.clone(),
        None => {
            eprintln!("usage: length_match <board.kicad_pcb> --template ddr3-1600 | --constraints f.json5 | --pair A,B");
            std::process::exit(2);
        }
    };
    let mut cs = None;
    let mut hint = String::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--template" => {
                let name = &args[i + 1];
                cs = template(name).or_else(|| {
                    eprintln!("未知模板 {name}（可选 usb2/eth-100m/sgmii/rgmii/ddr3-1600）");
                    None
                });
                hint = format!("模板 {name}");
                i += 2;
            }
            "--constraints" => {
                let text = std::fs::read_to_string(&args[i + 1]).expect("读约束文件");
                cs = parse_constraints(&text).ok();
                hint = args[i + 1].clone();
                i += 2;
            }
            "--pair" => {
                let (a, b) = args[i + 1].split_once(',').expect("A,B");
                let text = format!(
                    "{{ groups: [ {{ name: \"PAIR_{}\", nets: [\"{a}\",\"{b}\"], tolerance_mm: 0.127, reference: \"longest\" }} ] }}",
                    a.to_uppercase().replace('_', "")
                );
                cs = parse_constraints(&text).ok();
                hint = format!("对内等长 {a}/{b} (±5mil)");
                i += 2;
            }
            _ => i += 1,
        }
    }
    let cs = match cs {
        Some(c) => c,
        None => std::process::exit(2),
    };
    let content = std::fs::read_to_string(&path).expect("read pcb");
    let board = kicad_json5::parse_board(&content).expect("parse board");
    let groups = resolve(&board, &cs);
    for g in &groups {
        if !g.missing.is_empty() {
            eprintln!("⚠️ [{}] 声明了但板上不存在: {:?}", g.name, g.missing);
        }
    }
    let reports = eval_length_matching(&board, &groups);
    print!("{}", format_length_report(&reports, &hint));
}
