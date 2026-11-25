//! 阻抗计算：正向 (几何 → 阻抗) 与反向 (目标阻抗 → 线宽), 微带 / 带状线 × 单端 / 差分。
//!
//!   cargo run -p kicad-cdb --example impedance_calc -- --w 0.2 --s 0.2 --h 0.21 --t 0.035 --er 4.4
//!   cargo run -p kicad-cdb --example impedance_calc -- --z 100 --diff --s 0.2 --h 0.21 --t 0.035 --er 4.4
#[path = "shared/mod.rs"]
mod shared;

use kicad_cdb::impedance::{
    impedance, microstrip_z0, microstrip_zdiff, solve_width, stripline_z0, stripline_zdiff, Geom,
    LineKind,
};
use shared::Args;

fn main() {
    let args = Args::parse(&["--diff"]);
    let g = Geom {
        w: args.f64_or("--w", 0.2),
        s: args.f64_or("--s", 0.2),
        h: args.f64_or("--h", 0.21),
        t: args.f64_or("--t", 0.035),
        er: args.f64_or("--er", 4.4),
    };
    let diff = args.has("--diff");
    let Some(z) = args.f64_opt("--z") else {
        println!(
            "微带   单端 Z0={:.2}Ω   差分(s={:.2}) Zdiff={:.2}Ω",
            microstrip_z0(&g),
            g.s,
            microstrip_zdiff(&g)
        );
        println!(
            "带状线 单端 Z0={:.2}Ω   差分(s={:.2}) Zdiff={:.2}Ω   (h=B 两参考面间距)",
            stripline_z0(&g),
            g.s,
            stripline_zdiff(&g)
        );
        return;
    };
    let w = solve_width(z, g.s, g.h, g.t, g.er, LineKind::Microstrip, diff);
    let g2 = Geom { w, ..g };
    println!(
        "微带目标 {}{}Ω ⇒ 线宽 w={:.3}mm (h={:.2} t={:.3} er={:.2} s={:.2}; 回算 {:.2}Ω)",
        if diff { "差分" } else { "单端" },
        z,
        w,
        g.h,
        g.t,
        g.er,
        g.s,
        impedance(&g2, LineKind::Microstrip, diff)
    );
}
