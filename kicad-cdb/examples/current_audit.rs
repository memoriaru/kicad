//! 载流审计：读落板 → 从板上抽电源树 → 逐电源网核对"最窄走线 / 过孔数扛不扛得住电流需求"。
//!
//!   cargo run -p kicad-cdb --example current_audit -- <board.kicad_pcb> [--oz 1] [--dt 30] [--net NAME=A,...] [--source NAME=A,...]
//!
//! 电流需求缺省走 VRM 铭牌表; 真实负载用 `--net 网名=安培` 覆盖, 板级输入的电流用 `--source`。
#[path = "shared/mod.rs"]
mod shared;

use std::collections::HashMap;

use kicad_cdb::current_carry::{
    audit_current_carry, extract_power_tree, format_report, CopperParams, CurrentOverrides,
};
use shared::Args;

fn main() {
    let args = Args::parse(&[]);
    let path = args.board_path_or_exit(
        "current_audit <board.kicad_pcb> [--oz 1] [--dt 30] [--net NAME=A,...] [--source NAME=A,...]",
    );
    let board = shared::load_board(&path);
    let currents = |opt: &str| -> HashMap<String, f64> {
        args.pairs(opt)
            .into_iter()
            .map(|(name, v)| {
                let amps = v.parse().unwrap_or_else(|_| {
                    eprintln!("{opt}: {name}={v:?} 里的电流不是数字");
                    std::process::exit(2)
                });
                (name, amps)
            })
            .collect()
    };
    let ov = CurrentOverrides {
        net_current: currents("--net"),
        source_current: currents("--source"),
        copper: CopperParams {
            oz: args.f64_or("--oz", 1.0),
            dt_c: args.f64_or("--dt", 30.0),
            internal: false,
        },
        ..Default::default()
    };
    let vrms = extract_power_tree(&board, &ov);
    let audits = audit_current_carry(&board, &vrms, &ov);
    print!("{}", format_report(&audits, &vrms, &ov));
}
