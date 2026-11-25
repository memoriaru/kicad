//! 分组等长报告：约束声明 / 模板 → 解析到板上的网 → 布线后的等长偏差报告。
//!
//!   cargo run -p kicad-cdb --example length_match -- <board.kicad_pcb> --template ddr3-1600
//!   cargo run -p kicad-cdb --example length_match -- <board.kicad_pcb> --constraints constraints.json5
//!   cargo run -p kicad-cdb --example length_match -- <board.kicad_pcb> --pair DIFF_P,DIFF_N
#[path = "shared/mod.rs"]
mod shared;

use kicad_cdb::constraint::{
    eval_length_matching, format_length_report, parse_constraints, resolve, template,
};
use shared::Args;

const USAGE: &str =
    "length_match <board.kicad_pcb> --template NAME | --constraints FILE.json5 | --pair A,B";

fn main() {
    let args = Args::parse(&[]);
    let path = args.board_path_or_exit(USAGE);
    // 三种来源互斥, 取最后给出的那个
    let (cs, hint) = match args.last_of(&["--template", "--constraints", "--pair"]) {
        Some(("--template", name)) => {
            let cs = template(name).unwrap_or_else(|| {
                eprintln!("未知模板 {name}（可选 usb2/eth-100m/sgmii/rgmii/ddr3-1600）");
                std::process::exit(2)
            });
            (cs, format!("模板 {name}"))
        }
        Some(("--constraints", file)) => {
            let text = shared::read_text("约束文件", file);
            let cs = parse_constraints(&text).unwrap_or_else(|e| {
                eprintln!("约束文件 {file:?} 解析不了: {e}");
                std::process::exit(2)
            });
            (cs, file.to_string())
        }
        Some(("--pair", pair)) => {
            let Some((a, b)) = pair.split_once(',') else {
                eprintln!("usage: {USAGE}\n--pair 要 A,B");
                std::process::exit(2);
            };
            // 对内等长 <5mil
            let text = format!(
                "{{ groups: [ {{ name: \"PAIR_{}\", nets: [\"{a}\",\"{b}\"], tolerance_mm: 0.127, reference: \"longest\" }} ] }}",
                a.to_uppercase().replace('_', "")
            );
            let cs = parse_constraints(&text).unwrap_or_else(|e| {
                eprintln!("--pair {pair:?} 生成的约束解析不了: {e}");
                std::process::exit(2)
            });
            (cs, format!("对内等长 {a}/{b} (±5mil)"))
        }
        _ => {
            eprintln!("usage: {USAGE}");
            std::process::exit(2)
        }
    };
    let board = shared::load_board(&path);
    let groups = resolve(&board, &cs);
    for g in &groups {
        if !g.missing.is_empty() {
            eprintln!("⚠️ [{}] 声明了但板上不存在: {:?}", g.name, g.missing);
        }
    }
    let reports = eval_length_matching(&board, &groups);
    print!("{}", format_length_report(&reports, &hint));
}
