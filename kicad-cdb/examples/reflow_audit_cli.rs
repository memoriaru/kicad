//! 回流+包地审计 CLI。
//!
//!   reflow_audit <board.kicad_pcb> [--all] [--nets CLK,USB] [--island-mm2 2]
use kicad_cdb::reflow_audit::{audit_guards, audit_reflow, find_ground_islands, format_report};
use kicad_cdb::stitching::StitchConfig;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = match args.iter().find(|a| !a.starts_with("--")) {
        Some(p) => p.clone(),
        None => {
            eprintln!("usage: reflow_audit <board.kicad_pcb> [--all] [--nets A,B] [--island-mm2 2]");
            std::process::exit(2);
        }
    };
    let mut cfg = StitchConfig::default();
    let mut island_min = 2.0f64;
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
            "--island-mm2" => {
                island_min = args[i + 1].parse().unwrap();
                i += 2;
            }
            _ => i += 1,
        }
    }
    let content = std::fs::read_to_string(&path).expect("read pcb");
    let board = kicad_json5::parse_board(&content).expect("parse board");
    let breaks = audit_reflow(&board, &cfg);
    let guards = audit_guards(&board, &cfg);
    let islands = find_ground_islands(&board, island_min);
    print!("{}", format_report(&breaks, &guards, &islands));
}
