//! 板子速查 (调试用)：列网表和电源芯片 / 列某个位号每个焊盘挂在哪个网上 / 列挂在某个网上的器件。
//!
//!   cargo run -p kicad-cdb --example inspect -- nets <board.kicad_pcb>
//!   cargo run -p kicad-cdb --example inspect -- pads <board.kicad_pcb> <REF>
//!   cargo run -p kicad-cdb --example inspect -- net  <board.kicad_pcb> <NET>
#[path = "shared/mod.rs"]
mod shared;

use kicad_cdb::current_carry::{extract_power_tree, CurrentOverrides};
use shared::Args;

const USAGE: &str =
    "inspect nets <board.kicad_pcb> | pads <board.kicad_pcb> <REF> | net <board.kicad_pcb> <NET>";

fn main() {
    let args = Args::parse(&[]);
    let board = shared::load_board(&args.arg(1, "板文件", USAGE));
    match args.arg(0, "子命令", USAGE).as_str() {
        "nets" => {
            let mut names: Vec<&str> = board.nets.iter().map(|n| n.name.as_str()).collect();
            names.sort_unstable();
            println!("nets({}): {:?}", names.len(), names);
            println!(
                "footprints={} segments={} vias={}",
                board.footprints.len(),
                board.segments.len(),
                board.vias.len()
            );
            // 电源芯片按 current_carry 的器件目录认 (和 current_audit 同一套判据)
            for v in extract_power_tree(&board, &CurrentOverrides::default()) {
                let value = board
                    .footprints
                    .iter()
                    .find(|f| f.reference == v.refdes)
                    .map(|f| f.value.as_str())
                    .unwrap_or("");
                println!(
                    "  VRM {} {}  {}  {} -> {}",
                    v.refdes,
                    value,
                    v.role.as_str(),
                    v.vin_net,
                    v.vout_net
                );
            }
        }
        "pads" => {
            let refdes = args.arg(2, "位号", USAGE);
            for f in board.footprints.iter().filter(|f| f.reference == refdes) {
                println!("{} {} pads={}", f.reference, f.value, f.pads.len());
                for p in &f.pads {
                    println!(
                        "  pin {:>3} -> {:?}",
                        p.number,
                        p.net_name.clone().unwrap_or_default()
                    );
                }
            }
        }
        "net" => {
            let want = args.arg(2, "网名", USAGE).to_uppercase();
            for f in &board.footprints {
                let pins: Vec<&String> = f
                    .pads
                    .iter()
                    .filter_map(|p| p.net_name.as_ref())
                    .filter(|n| n.to_uppercase() == want)
                    .collect();
                if !pins.is_empty() {
                    println!(
                        "{:<6} {:<24} {} pin(s) on {}",
                        f.reference,
                        f.value,
                        f.lib_id.rsplit(':').next().unwrap_or(""),
                        pins.len()
                    );
                }
            }
        }
        other => {
            eprintln!("usage: {USAGE}\n不认识的子命令 {other:?}");
            std::process::exit(2)
        }
    }
}
