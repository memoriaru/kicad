//! 制造规则审计：齐排孔、20H (电源铜皮相对地铜皮内缩)、AC 耦合电容下方的挖空建议。
//!
//!   cargo run -p kicad-cdb --example mfg_audit -- <board.kicad_pcb> [--gap 2.0] [--fence-min 5]
#[path = "shared/mod.rs"]
mod shared;

use kicad_cdb::mfg_rules::{check_20h, detect_via_fences, find_ac_coupling_caps, format_report};
use kicad_cdb::stitching::StitchConfig;
use shared::Args;

fn main() {
    let args = Args::parse(&[]);
    let path = args.board_path_or_exit("mfg_audit <board.kicad_pcb> [--gap 2.0] [--fence-min 5]");
    let board = shared::load_board(&path);
    let fences = detect_via_fences(&board, args.usize_or("--fence-min", 5), 0.1);
    let shrink = check_20h(&board, args.f64_or("--gap", 2.0));
    let voids = find_ac_coupling_caps(&board, &StitchConfig::default());
    print!("{}", format_report(&fences, &shrink, &voids));
}
