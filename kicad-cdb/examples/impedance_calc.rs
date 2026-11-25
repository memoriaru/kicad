//! 阻抗计算示例：正向（几何→阻抗）与反向（目标→线宽）。
//!
//! 用法：
//!   impedance_calc --w 0.2 --s 0.2 --h 0.21 --t 0.035 --er 4.4        # 正向：单端+差分 × 微带/带状线
//!   impedance_calc --z 100 --diff --s 0.2 --h 0.21 --t 0.035 --er 4.4 # 反向：目标阻抗→线宽
fn main() {
    let mut g = kicad_cdb::impedance::Geom {
        w: 0.2,
        s: 0.2,
        h: 0.21,
        t: 0.035,
        er: 4.4,
    };
    let mut target: Option<f64> = None;
    let mut diff = false;
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < a.len() {
        match a[i].as_str() {
            "--w" => g.w = a[i + 1].parse().unwrap(),
            "--s" => g.s = a[i + 1].parse().unwrap(),
            "--h" => g.h = a[i + 1].parse().unwrap(),
            "--t" => g.t = a[i + 1].parse().unwrap(),
            "--er" => g.er = a[i + 1].parse().unwrap(),
            "--z" => target = Some(a[i + 1].parse().unwrap()),
            "--diff" => diff = true,
            _ => {}
        }
        i += if a[i].starts_with("--") && a[i] != "--diff" {
            2
        } else {
            1
        };
    }
    match target {
        None => {
            println!(
                "微带   单端 Z0={:.2}Ω   差分(s={:.2}) Zdiff={:.2}Ω",
                kicad_cdb::impedance::microstrip_z0(&g),
                g.s,
                kicad_cdb::impedance::microstrip_zdiff(&g)
            );
            println!(
                "带状线 单端 Z0={:.2}Ω   差分(s={:.2}) Zdiff={:.2}Ω   (h=B 两参考面间距)",
                kicad_cdb::impedance::stripline_z0(&g),
                g.s,
                kicad_cdb::impedance::stripline_zdiff(&g)
            );
        }
        Some(z) => {
            let w = kicad_cdb::impedance::solve_width(
                z,
                g.s,
                g.h,
                g.t,
                g.er,
                kicad_cdb::impedance::LineKind::Microstrip,
                diff,
            );
            let g2 = kicad_cdb::impedance::Geom { w, ..g };
            println!(
                "微带目标 {}{}Ω ⇒ 线宽 w={:.3}mm (h={:.2} t={:.3} er={:.2} s={:.2}; 回算 {:.2}Ω)",
                if diff { "差分" } else { "单端" },
                z,
                w,
                g.h,
                g.t,
                g.er,
                g.s,
                kicad_cdb::impedance::impedance(
                    &g2,
                    kicad_cdb::impedance::LineKind::Microstrip,
                    diff
                )
            );
        }
    }
}
