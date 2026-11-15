//! 缝合审计：读落板 → 换层点三态审计 → (--apply) 自动补 GND 缝合 via 输出新板。
//!
//!   stitch_audit <board.kicad_pcb> [--all] [--nets CLK,USB] [--apply out.kicad_pcb]
use kicad_cdb::stitching::{apply_stitching_vias, audit_stitching, format_report, StitchConfig};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = match args.iter().find(|a| !a.starts_with("--")) {
        Some(p) => p.clone(),
        None => {
            eprintln!("usage: stitch_audit <board.kicad_pcb> [--all] [--nets A,B] [--apply out.kicad_pcb]");
            std::process::exit(2);
        }
    };
    let mut cfg = StitchConfig::default();
    let mut apply_out: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--all" => {
                cfg.all_nets = true;
                i += 1;
            }
            "--nets" => {
                cfg.net_patterns = args[i + 1].split(',').map(|s| s.to_string()).collect();
                i += 2;
            }
            "--apply" => {
                apply_out = Some(args[i + 1].clone());
                i += 2;
            }
            _ => i += 1,
        }
    }

    let content = std::fs::read_to_string(&path).expect("read pcb");
    let mut board = kicad_json5::parse_board(&content).expect("parse board");
    let findings = audit_stitching(&board, &cfg);
    let (inserted, failed) = if apply_out.is_some() {
        let r = apply_stitching_vias(&mut board, &findings, &cfg);
        let mut gen = kicad_json5::BoardSexprGenerator::new();
        let out = gen.generate(&board).expect("generate");
        std::fs::write(apply_out.as_ref().unwrap(), out).expect("write");
        r
    } else {
        (vec![], vec![])
    };
    print!("{}", format_report(&findings, &inserted, &failed));
}
