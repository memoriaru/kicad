//! 换层缝合审计：读落板 → 换层点三态审计 (缺缝合孔 / 缺搭桥电容 / 跨分割) → `--apply` 自动补 GND 缝合 via 并输出新板。
//!
//!   cargo run -p kicad-cdb --example stitch_audit -- <board.kicad_pcb> [--all] [--nets CLK,USB] [--apply out.kicad_pcb]
#[path = "shared/mod.rs"]
mod shared;

use kicad_cdb::stitching::{apply_stitching_vias, audit_stitching, format_report};
use shared::Args;

fn main() {
    let args = Args::parse(&["--all"]);
    let path = args.board_path_or_exit(
        "stitch_audit <board.kicad_pcb> [--all] [--nets A,B] [--apply out.kicad_pcb]",
    );
    let cfg = shared::stitch_config(&args);
    let mut board = shared::load_board(&path);
    let findings = audit_stitching(&board, &cfg);
    let (inserted, failed) = match args.value("--apply") {
        Some(out) => {
            let r = apply_stitching_vias(&mut board, &findings, &cfg);
            let mut gen = kicad_json5::BoardSexprGenerator::new();
            std::fs::write(out, gen.generate(&board).expect("generate")).expect("write");
            r
        }
        None => (vec![], vec![]),
    };
    print!("{}", format_report(&findings, &inserted, &failed));
}
