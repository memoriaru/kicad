//! P2 制造规则审计 CLI：齐排孔 / 20H / AC 挖空建议。
//!   mfg_audit <board.kicad_pcb> [--gap 2.0] [--fence-min 5]
use kicad_cdb::mfg_rules::{check_20h, detect_via_fences, find_ac_coupling_caps, format_report};
use kicad_cdb::stitching::StitchConfig;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = match args.iter().find(|a| !a.starts_with("--")) {
        Some(p) => p.clone(),
        None => { eprintln!("usage: mfg_audit <board.kicad_pcb> [--gap 2.0] [--fence-min 5]"); std::process::exit(2); }
    };
    let mut gap = 2.0f64;
    let mut fence_min = 5usize;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--gap" => { gap = args[i + 1].parse().unwrap(); i += 2; }
            "--fence-min" => { fence_min = args[i + 1].parse().unwrap(); i += 2; }
            _ => i += 1,
        }
    }
    let board = kicad_json5::parse_board(&std::fs::read_to_string(&path).expect("read")).expect("parse");
    let fences = detect_via_fences(&board, fence_min, 0.1);
    let shrink = check_20h(&board, gap);
    let voids = find_ac_coupling_caps(&board, &StitchConfig::default());
    print!("{}", format_report(&fences, &shrink, &voids));
}
