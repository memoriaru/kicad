//! 回流路径 + 包地审计：换层处参考平面连不连续、包地线有没有打孔、孤岛地铜皮。
//!
//!   cargo run -p kicad-cdb --example reflow_audit_cli -- <board.kicad_pcb> [--all] [--nets CLK,USB] [--island-mm2 2]
#[path = "shared/mod.rs"]
mod shared;

use kicad_cdb::reflow_audit::{audit_guards, audit_reflow, find_ground_islands, format_report};
use shared::Args;

fn main() {
    let args = Args::parse(&["--all"]);
    let path = args.board_path_or_exit(
        "reflow_audit_cli <board.kicad_pcb> [--all] [--nets A,B] [--island-mm2 2]",
    );
    let cfg = shared::stitch_config(&args);
    let board = shared::load_board(&path);
    let breaks = audit_reflow(&board, &cfg);
    let guards = audit_guards(&board, &cfg);
    let islands = find_ground_islands(&board, args.f64_or("--island-mm2", 2.0));
    print!("{}", format_report(&breaks, &guards, &islands));
}
