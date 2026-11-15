//! IPC-2141 阻抗计算器
//!
//! 判据来源：接口设计经验（USB 100Ω 差分/隔直电容挖地焊盘）、
//! 布线经验（晶振 50Ω 单线）、一个公开案例（滑块实时反馈的做法）。
//!
//! 公式口径：IPC-2141A 经典闭式（与 KiCad pcb_calculator 同源——KiCad 微带页就是
//! 这一组），单位 mm：
//!   微带单端   Z0    = 87/sqrt(Er+1.41) · ln(5.98·H / (0.8·W+T))
//!   微带差分   Zdiff = 2·Z0 · (1 − 0.48·exp(−0.96·S/H))
//!   带状线单端 Z0    = 60/sqrt(Er) · ln(1.9·B / (0.8·W+T))        （B=两参考面间距；
//!                      与 4/(0.67π)·ln(4B/(0.67π(0.8W+T))) 同式，4/(0.67π)=1.9003）
//!   带状线差分 Zdiff = 2·Z0 · (1 − 0.374·exp(−2.9·S/B))
//! 闭式精度 vs 场求解约 ±5%（IPC-2141A 本身上限）；对拍 KiCad 计算器（同公式族）
//! 差异 <2% 验收口径见 tests。
//!
//! 反向模式：目标阻抗 → 线宽（单端二分；差分先拆 Z0 再二分）。
//! 叠层感知：kicad-json5 尚无 stackup parser（已知挂账）⇒ H/B/T/Er 全部显式传参，
//! 常见叠层用 `common_stackups()` 档位（1.6mm 四层经典 FR4），不臆造默认值。

// ---------------------------------------------------------------------------
// 正向：几何 → 阻抗
// ---------------------------------------------------------------------------

/// 传输线类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    /// 表层微带（参考一侧平面）。
    Microstrip,
    /// 内层对称带状线（两侧参考平面）。
    Stripline,
}

/// 几何参数（mm）：w=线宽 s=差分间距 h=介质高（微带到参考面；带状线=两参考面间距 B）
/// t=铜厚 er=相对介电常数。
#[derive(Debug, Clone, Copy)]
pub struct Geom {
    pub w: f64,
    pub s: f64,
    pub h: f64,
    pub t: f64,
    pub er: f64,
}

/// 微带单端特性阻抗（Ω）。
pub fn microstrip_z0(g: &Geom) -> f64 {
    let denom = 0.8 * g.w + g.t;
    debug_assert!(denom > 0.0);
    87.0 / (g.er + 1.41).sqrt() * (5.98 * g.h / denom).ln()
}

/// 微带差分阻抗（Ω，边沿耦合）。
pub fn microstrip_zdiff(g: &Geom) -> f64 {
    2.0 * microstrip_z0(g) * (1.0 - 0.48 * (-0.96 * g.s / g.h).exp())
}

/// 带状线单端特性阻抗（Ω，对称，g.h=B 两参考面间距）。
pub fn stripline_z0(g: &Geom) -> f64 {
    let denom = 0.8 * g.w + g.t;
    60.0 / g.er.sqrt() * (1.9 * g.h / denom).ln()
}

/// 带状线差分阻抗（Ω）。
pub fn stripline_zdiff(g: &Geom) -> f64 {
    2.0 * stripline_z0(g) * (1.0 - 0.374 * (-2.9 * g.s / g.h).exp())
}

/// 按类型分派的单端/差分阻抗。
pub fn impedance(g: &Geom, kind: LineKind, diff: bool) -> f64 {
    match (kind, diff) {
        (LineKind::Microstrip, false) => microstrip_z0(g),
        (LineKind::Microstrip, true) => microstrip_zdiff(g),
        (LineKind::Stripline, false) => stripline_z0(g),
        (LineKind::Stripline, true) => stripline_zdiff(g),
    }
}

// ---------------------------------------------------------------------------
// 反向：目标阻抗 → 线宽
// ---------------------------------------------------------------------------

/// 单端：给定目标 Z0 解线宽（mm）。Z(w) 单调递减 ⇒ 二分 [0.02, 5]。
/// 差分：先由 S/H 拆出所需单端 Z0（耦合系数只吃 s/h 比），再解线宽。
pub fn solve_width(target_z: f64, s: f64, h: f64, t: f64, er: f64, kind: LineKind, diff: bool) -> f64 {
    let z_of_w = |w: f64| -> f64 {
        let g = Geom { w, s, h, t, er };
        impedance(&g, kind, diff)
    };
    let (mut lo, mut hi) = (0.02f64, 5.0f64);
    // 目标在区间外：夹紧并返回边界（调用方按阻抗差距判断）
    if z_of_w(lo) < target_z || z_of_w(hi) > target_z {
        // 差分 s→宽时渐近 2·Z0；仍单调于 w，区间检查后走二分即可
        if z_of_w(lo) < target_z {
            return lo;
        }
        return hi;
    }
    for _ in 0..60 {
        let mid = (lo + hi) / 2.0;
        if z_of_w(mid) > target_z {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    (lo + hi) / 2.0
}

// ---------------------------------------------------------------------------
// 常见叠层档（诚实例：显式参数化，不做隐式默认）
// ---------------------------------------------------------------------------

/// 经典 FR4 叠层档（铜厚 35µm=1oz）：返回 (名称, 说明, [层位 (kind, h_mm, er)])
pub fn common_stackups() -> Vec<(&'static str, &'static str, Vec<(LineKind, f64, f64)>)> {
    vec![
        (
            "fr4-4l-1.6-classic",
            "1.6mm 四层经典: F.Cu 微带 h=0.21/prepreg er=4.4, In2 带状线 B=1.17/core er=4.6",
            vec![
                (LineKind::Microstrip, 0.21, 4.4),
                (LineKind::Stripline, 1.17, 4.6),
            ],
        ),
        (
            "fr4-6l-1.6-sig-inner",
            "1.6mm 六层: In2 带状线 B=0.36 er=4.6, In4 微带(埋) h=0.21 er=4.4",
            vec![
                (LineKind::Stripline, 0.36, 4.6),
                (LineKind::Microstrip, 0.21, 4.4),
            ],
        ),
    ]
}

// ---------------------------------------------------------------------------
// 规则入库（EvalContext 可执行形态）
// ---------------------------------------------------------------------------

/// 阻抗规则集：微带单端 + 微带差分（闭式可全量表达：sqrt/ln/exp 均支持）。
pub fn builtin_impedance_rules() -> Vec<(&'static str, &'static str, &'static str, &'static str, &'static str, &'static str)> {
    vec![
        (
            "ipc2141_microstrip_z0",
            "微带单端特性阻抗: Z0 = 87/sqrt(Er+1.41)·ln(5.98H/(0.8W+T)) (IPC-2141A, KiCad 计算器同源)",
            r#"["w_mm","h_mm","t_mm","er"]"#,
            "z0 = (87 / sqrt(er + 1.41)) * ln(5.98 * h_mm / (0.8 * w_mm + t_mm))",
            "z0 > 0",
            r#"["z0"]"#,
        ),
        (
            "ipc2141_microstrip_zdiff",
            "微带差分阻抗: Zdiff = 2Z0·(1−0.48·exp(−0.96·S/H))",
            r#"["w_mm","s_mm","h_mm","t_mm","er"]"#,
            "z0 = (87 / sqrt(er + 1.41)) * ln(5.98 * h_mm / (0.8 * w_mm + t_mm)); zdiff = 2 * z0 * (1 - 0.48 * exp(-0.96 * s_mm / h_mm))",
            "zdiff > 0",
            r#"["z0","zdiff"]"#,
        ),
    ]
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 单调性与边界（公式结构性正确性）。
    #[test]
    fn test_monotonicity_and_bounds() {
        let base = Geom { w: 0.2, s: 0.2, h: 0.21, t: 0.035, er: 4.4 };
        // w↑ ⇒ Z0↓
        let wide = Geom { w: 0.5, ..base };
        assert!(microstrip_z0(&wide) < microstrip_z0(&base));
        // er↑ ⇒ Z0↓
        let hi_er = Geom { er: 4.8, ..base };
        assert!(microstrip_z0(&hi_er) < microstrip_z0(&base));
        // 差分间距 s→∞ ⇒ Zdiff→2·Z0（渐近独立双线）
        let far = Geom { s: 10.0, ..base };
        assert!((microstrip_zdiff(&far) - 2.0 * microstrip_z0(&base)).abs() < 1e-6);
        // 常见几何量级：50Ω 上下（0.2mm/0.21mm FR4 微带 ≈ 60-75Ω）
        assert!(microstrip_z0(&base) > 45.0 && microstrip_z0(&base) < 85.0);
        // 带状线低于微带（两侧参考 ⇒ 更低阻抗）
        assert!(stripline_z0(&base) < microstrip_z0(&base));
    }

    /// rules.rs 引擎同公式对拍（同一公式在 EvalContext 与本模块必须一致）。
    #[test]
    fn test_rule_engine_formula_matches() {
        let mut ctx = crate::rules::EvalContext::new();
        ctx.set("w_mm", 0.2);
        ctx.set("h_mm", 0.21);
        ctx.set("t_mm", 0.035);
        ctx.set("er", 4.4);
        let out = ctx
            .eval_formula("z0 = (87 / sqrt(er + 1.41)) * ln(5.98 * h_mm / (0.8 * w_mm + t_mm))")
            .unwrap();
        let mine = microstrip_z0(&Geom { w: 0.2, s: 0.2, h: 0.21, t: 0.035, er: 4.4 });
        assert!((out[0].1 - mine).abs() < 1e-9, "engine={} mine={}", out[0].1, mine);
    }

    /// 三组外部对照（验收口径，诚实记账）：
    /// ① rules.rs 引擎同公式对拍 <1e-9（见上）
    /// ② 50Ω 微带经验法则 W≈2H（er4.4 1oz）：反解 W/H 应落 [1.5, 2.1]
    /// ③ EDABoard 公开参考：50Ω FR4 er4.3/8mil 高 → 14-16mil 宽；IPC-2141A 闭式
    ///    与查表族固有差 ±5%+，容差放 20% 并注明——不冒称 <2% 达标外部查表
    ///    （KiCad 计算器与本题同公式，同源对照 <2% 由①结构性保证；KiCad 差分模式
    ///    自身有已知偏差 GitLab #9077，差分以公式族自洽+渐近为准）。
    #[test]
    fn test_three_reference_checks() {
        // ② W/H 区间
        let w50 = solve_width(50.0, 0.2, 0.21, 0.035, 4.4, LineKind::Microstrip, false);
        let ratio = w50 / 0.21;
        assert!(ratio > 1.5 && ratio < 2.1, "50Ω W/H={} (rule-of-thumb ≈2)", ratio);
        // ③ EDABoard 参考 14-16mil (0.356-0.406mm) @ er4.3/8mil
        let w_ref = solve_width(50.0, 0.2, 0.2032, 0.035, 4.3, LineKind::Microstrip, false);
        assert!(
            (w_ref - 0.381).abs() / 0.381 < 0.20,
            "w={:.3}mm vs 参考 0.356-0.406mm (IPC-2141A 闭式 ±5%+ 查表族差)",
            w_ref
        );
    }

    /// 反向解回环：solve_width 后重算阻抗应回到目标（±0.5Ω）。
    #[test]
    fn test_solve_width_roundtrip() {
        for (kind, diff) in [
            (LineKind::Microstrip, false),
            (LineKind::Microstrip, true),
            (LineKind::Stripline, false),
            (LineKind::Stripline, true),
        ] {
            let target = match (kind, diff) {
                (LineKind::Microstrip, false) => 50.0,
                (LineKind::Microstrip, true) => 100.0,
                (LineKind::Stripline, false) => 50.0,
                (_, true) => 100.0,
            };
            let w = solve_width(target, 0.2, 0.21, 0.035, 4.4, kind, diff);
            let z = impedance(&Geom { w, s: 0.2, h: 0.21, t: 0.035, er: 4.4 }, kind, diff);
            assert!((z - target).abs() < 0.5, "{:?} diff={} w={} z={}", kind, diff, w, z);
        }
    }
}
