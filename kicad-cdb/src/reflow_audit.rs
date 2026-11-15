//! 回流路径连续性审计 + 立体包地完整性
//!
//! 判据来源：回流设计经验——
//!   - 回流沿阻抗最小路径=高频沿信号线正下方；破坏参考平面=回流绕圈=面积暴增
//!   - 归因三类：挖槽、**一排齐孔**（为好看斜打一排=人为割断平面）、平面分割
//!   - 孤岛地铜皮高频有害→挖空删除
//!   - 包地判据（布线经验）："立体包地或别包"——同层两侧+纵向相邻层都是地
//!     且**必须打孔**；不打孔的包地线高频自身呈电感=天线（地弹）
//!
//! 三层分职：本模块=确定性判据（几何采样+连通性）；高速网语义复用 stitching 的
//! 口径（net 名模式，可覆盖）。KiCad DRC 不查这些——它只看间距与连接。

use kicad_json5::ir::board::Board;

use crate::stitching::{copper_axis, is_high_speed, net_display, point_in_poly, StitchConfig};

/// 投影带断点。
#[derive(Debug, Clone)]
pub struct ReflowBreak {
    pub net: String,
    pub at: (f64, f64),
    pub layer: String,      // 信号所在层
    pub ref_layer: String,  // 参考层
    pub cause: BreakCause,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BreakCause {
    /// 参考层完全无铜（平面分割/未铺）。
    NoPlane,
    /// zone outline 覆盖但填充缺失（挖槽/孤岛删除后残留）。
    SlotOrIsland,
    /// 断点 1mm 内 ≥3 过孔 = 齐排孔割断（口径"一排齐孔"）。
    ViaFence { nearby: usize },
}

/// 包地线检查结果。
#[derive(Debug, Clone)]
pub struct GuardTrace {
    pub net: String,         // 被包的信号网
    pub layer: String,
    /// guard 线段范围（近似中点+长度）。
    pub at: (f64, f64),
    pub length_mm: f64,
    /// guard 线上缝合 via 数。
    pub stitch_vias: usize,
    /// 纵向邻层（铜层轴上/下层）是否为地。
    pub vertical_ground: bool,
    pub verdict: GuardVerdict,
}

#[derive(Debug, Clone, PartialEq)]
pub enum GuardVerdict {
    /// 立体包地：打孔足 + 纵向是地。
    Ok3D { via_rate_mm: f64 },
    /// 同层包了但没打孔 = 天线（口径"只要求包地不要求打孔的主管=水货"）。
    AntennaNoVias,
    /// 打了孔但纵向邻层非地——半立体。
    NoVerticalGround,
}

fn seg_len(s: &kicad_json5::ir::board::Segment) -> f64 {
    (s.end.0 - s.start.0).hypot(s.end.1 - s.start.1)
}

/// 信号层在给定点的参考层（铜层轴邻层中有铜者；多候选取首个）。
fn ref_layer_at(board: &Board, axis: &[String], layer: &str, at: (f64, f64)) -> Option<(String, bool)> {
    // 返回 (参考层, outline 覆盖但填充缺失)
    let idx = axis.iter().position(|l| l == layer)?;
    for cand in [idx.checked_sub(1), Some(idx + 1).filter(|i| *i < axis.len())] {
        let Some(ci) = cand else { continue };
        let cl = &axis[ci];
        let mut outline_hit = false;
        for z in &board.zones {
            if z.layer != *cl {
                continue;
            }
            if z.filled_polygons.iter().any(|fp| fp.layer == *cl && point_in_poly(at, &fp.points)) {
                return Some((cl.clone(), false));
            }
            if !outline_hit && point_in_poly(at, &z.outline) {
                outline_hit = true;
            }
        }
        if outline_hit {
            return Some((cl.clone(), true)); // outline 有但填充缺 = 挖槽/孤岛
        }
    }
    None
}

/// 主审计：逐高速网投影带连续性（采样步 = 线宽/2，带宽容忍 3W 口径取中心线）。
pub fn audit_reflow(board: &Board, cfg: &StitchConfig) -> Vec<ReflowBreak> {
    let axis = copper_axis(board);
    let mut breaks = Vec::new();
    for seg in &board.segments {
        let net = net_display(board, seg.net);
        if net.is_empty() || !is_high_speed(&net, cfg) {
            continue;
        }
        let step = (seg.width / 2.0).max(0.2);
        let len = seg_len(seg);
        let n = (len / step).ceil() as usize;
        for k in 0..=n {
            let t = if n == 0 { 0.0 } else { k as f64 / n as f64 };
            let at = (
                seg.start.0 + t * (seg.end.0 - seg.start.0),
                seg.start.1 + t * (seg.end.1 - seg.start.1),
            );
            match ref_layer_at(board, &axis, &seg.layer, at) {
                Some((_, false)) => {} // 参考铜连续
                Some((rl, true)) => {
                    // outline 有填充缺：挖槽或孤岛——再按 via 密度归因
                    let nearby = board
                        .vias
                        .iter()
                        .filter(|v| (v.at.0 - at.0).hypot(v.at.1 - at.1) <= 1.0)
                        .count();
                    breaks.push(ReflowBreak {
                        net: net.clone(),
                        at,
                        layer: seg.layer.clone(),
                        ref_layer: rl,
                        cause: if nearby >= 3 {
                            BreakCause::ViaFence { nearby }
                        } else {
                            BreakCause::SlotOrIsland
                        },
                    });
                }
                None => {
                    // 该点邻层全无铜；若邻层连 zone 都没有则 NoPlane
                    breaks.push(ReflowBreak {
                        net: net.clone(),
                        at,
                        layer: seg.layer.clone(),
                        ref_layer: axis
                            .iter()
                            .position(|l| *l == seg.layer)
                            .and_then(|i| {
                                [i.checked_sub(1), Some(i + 1).filter(|j| *j < axis.len())]
                                    .into_iter()
                                    .flatten()
                                    .next()
                                    .map(|j| axis[j].clone())
                            })
                            .unwrap_or_default(),
                        cause: BreakCause::NoPlane,
                    });
                }
            }
        }
    }
    // 相邻断点去重聚类（同网同因、距离 <0.5mm 合并）
    let mut merged: Vec<ReflowBreak> = Vec::new();
    for b in breaks {
        if let Some(last) = merged.last_mut() {
            if last.net == b.net
                && last.layer == b.layer
                && last.cause == b.cause
                && (last.at.0 - b.at.0).hypot(last.at.1 - b.at.1) < 0.5
            {
                last.at = b.at;
                continue;
            }
        }
        merged.push(b);
    }
    merged
}

/// 包地线检测（同层 GND 走线与高速网平行走线 → 打孔率 + 纵向层判定）。
/// 口径：guard = GND 网线段，其 2mm 内有平行（方向夹角 <15°）高速网线段，
/// 平行重叠长度 ≥5mm。
pub fn audit_guards(board: &Board, cfg: &StitchConfig) -> Vec<GuardTrace> {
    let axis = copper_axis(board);
    let mut out = Vec::new();
    let gnd_segs: Vec<_> = board
        .segments
        .iter()
        .filter(|s| {
            let n = net_display(board, s.net);
            n == "GND" || n.ends_with("GND") || n.starts_with("GND")
        })
        .collect();
    for seg in &board.segments {
        let net = net_display(board, seg.net);
        if net.is_empty()
            || crate::current_carry::is_ground(&net)
            || crate::current_carry::net_voltage(&net).is_some()
            || !is_high_speed(&net, cfg)
        {
            continue; // 包地审计的"被包方"必须是信号网(GND 自身在 --all 下会自我匹配, 测试实锤)
        }
        let d1 = (seg.end.0 - seg.start.0, seg.end.1 - seg.start.1);
        for g in &gnd_segs {
            if g.layer != seg.layer {
                continue;
            }
            let d2 = (g.end.0 - g.start.0, g.end.1 - g.start.1);
            let dot = d1.0 * d2.0 + d1.1 * d2.1;
            let cross = (d1.0 * d2.1 - d1.1 * d2.0).abs();
            // 夹角 <15° ≈ cross/dot < 0.27
            if dot <= 0.0 || cross / dot > 0.27 {
                continue;
            }
            // 中点距离 <2mm 且长度足够 → 视作包地
            let gm = ((g.start.0 + g.end.0) / 2.0, (g.start.1 + g.end.1) / 2.0);
            let sm = ((seg.start.0 + seg.end.0) / 2.0, (seg.start.1 + seg.end.1) / 2.0);
            let dist = (gm.0 - sm.0).hypot(gm.1 - sm.1);
            let glen = seg_len(g);
            if dist > 2.0 || glen < 5.0 {
                continue;
            }
            // 打孔统计：guard 线路径 0.4mm 范围内的 GND via
            let stitch = board
                .vias
                .iter()
                .filter(|v| {
                    let n = net_display(board, v.net);
                    (n == "GND" || n.ends_with("GND") || n.starts_with("GND"))
                        && dist_pt_seg((**g).clone(), v.at) <= 0.4
                })
                .count();
            // 纵向邻层是否地
            let idx = axis.iter().position(|l| *l == seg.layer);
            let v_ground = idx
                .and_then(|i| {
                    [i.checked_sub(1), Some(i + 1).filter(|j| *j < axis.len())]
                        .into_iter()
                        .flatten()
                        .next()
                })
                .map(|ci| {
                    board.zones.iter().any(|z| {
                        z.layer == axis[ci]
                            && (z.net_name == "GND" || z.net_name.ends_with("GND"))
                    })
                })
                .unwrap_or(false);
            let verdict = if stitch == 0 {
                GuardVerdict::AntennaNoVias
            } else if !v_ground {
                GuardVerdict::NoVerticalGround
            } else {
                GuardVerdict::Ok3D { via_rate_mm: glen / stitch.max(1) as f64 }
            };
            out.push(GuardTrace {
                net: net.clone(),
                layer: seg.layer.clone(),
                at: gm,
                length_mm: glen,
                stitch_vias: stitch,
                vertical_ground: v_ground,
                verdict,
            });
        }
    }
    out
}

fn dist_pt_seg(s: kicad_json5::ir::board::Segment, p: (f64, f64)) -> f64 {
    let dx = s.end.0 - s.start.0;
    let dy = s.end.1 - s.start.1;
    let l2 = dx * dx + dy * dy;
    let t = if l2 == 0.0 {
        0.0
    } else {
        (((p.0 - s.start.0) * dx + (p.1 - s.start.1) * dy) / l2).clamp(0.0, 1.0)
    };
    (s.start.0 + t * dx - p.0).hypot(s.start.1 + t * dy - p.1)
}

/// 孤岛地检测（轻量版）：zone 填充多边形中面积小于阈值且与其它填充不相邻者。
/// 口径"孤岛地铜皮高频有害→挖空删除"。
pub fn find_ground_islands(board: &Board, min_area_mm2: f64) -> Vec<(String, (f64, f64), f64)> {
    let mut out = Vec::new();
    for z in &board.zones {
        if !(z.net_name == "GND" || z.net_name.ends_with("GND") || z.net_name.starts_with("GND")) {
            continue;
        }
        for fp in &z.filled_polygons {
            let area = poly_area(&fp.points);
            if area < min_area_mm2 && area > 0.0 {
                let c = centroid(&fp.points);
                out.push((z.layer.clone(), c, area));
            }
        }
    }
    out
}

fn poly_area(pts: &[(f64, f64)]) -> f64 {
    let n = pts.len();
    if n < 3 {
        return 0.0;
    }
    let mut a = 0.0;
    for i in 0..n {
        let j = (i + 1) % n;
        a += pts[i].0 * pts[j].1 - pts[j].0 * pts[i].1;
    }
    (a / 2.0).abs()
}

fn centroid(pts: &[(f64, f64)]) -> (f64, f64) {
    if pts.is_empty() {
        return (0.0, 0.0);
    }
    let (sx, sy) = pts.iter().fold((0.0, 0.0), |acc, p| (acc.0 + p.0, acc.1 + p.1));
    (sx / pts.len() as f64, sy / pts.len() as f64)
}

/// 人读报告。
pub fn format_report(breaks: &[ReflowBreak], guards: &[GuardTrace], islands: &[(String, (f64, f64), f64)]) -> String {
    let mut s = String::from("== 回流路径连续性审计 ==\n");
    if breaks.is_empty() {
        s.push_str("  ✅ 高速网投影带参考铜连续，无割断\n");
    }
    for b in breaks {
        let cause = match &b.cause {
            BreakCause::NoPlane => "无参考平面(分割/未铺)".to_string(),
            BreakCause::SlotOrIsland => "挖槽/孤岛填充缺失".to_string(),
            BreakCause::ViaFence { nearby } => format!("齐排过孔割断({nearby} via/1mm)"),
        };
        s.push_str(&format!(
            "  ⚠️ [{}] ({:.2},{:.2}) {} → 参考 {} : {}\n",
            b.net, b.at.0, b.at.1, b.layer, b.ref_layer, cause
        ));
    }
    s.push_str("\n== 立体包地完整性（立体包地或别包） ==\n");
    if guards.is_empty() {
        s.push_str("  （未检测到包地走线）\n");
    }
    for g in guards {
        let v = match &g.verdict {
            GuardVerdict::Ok3D { via_rate_mm } => format!("✅ 立体包地(每 {via_rate_mm:.1}mm 一孔)"),
            GuardVerdict::AntennaNoVias => "⚠️ 包地未打孔=天线(建议补孔或删除)".to_string(),
            GuardVerdict::NoVerticalGround => "⚠️ 纵向邻层非地——半立体(建议删或补)".to_string(),
        };
        s.push_str(&format!(
            "  [{}] guard@({:.2},{:.2}) {} 长 {:.1}mm 孔={} 纵向地={} ⇒ {}\n",
            g.net, g.at.0, g.at.1, g.layer, g.length_mm, g.stitch_vias, g.vertical_ground, v
        ));
    }
    s.push_str("\n== 孤岛地铜（高频有害→建议挖空） ==\n");
    if islands.is_empty() {
        s.push_str("  ✅ 无小面积孤岛\n");
    }
    for (layer, c, a) in islands {
        s.push_str(&format!("  ⚠️ {} @({:.2},{:.2}) 面积 {a:.2}mm²\n", layer, c.0, c.1));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use kicad_json5::ir::board::{NetDef, Segment, Zone};

    fn zone_gnd(layer: &str, x0: f64, x1: f64) -> Zone {
        Zone {
            net: 0,
            net_name: "GND".into(),
            layer: layer.into(),
            hatch_style: String::new(),
            hatch_pitch: 0.0,
            pad_connect: String::new(),
            connect_pads_clearance: 0.0,
            min_thickness: 0.0,
            fill: true,
            thermal_gap: 0.0,
            thermal_bridge_width: 0.0,
            island_removal_mode: 0,
            island_area: 0.0,
            outline: vec![(x0, 0.0), (x1, 0.0), (x1, 40.0), (x0, 40.0)],
            filled_polygons: vec![kicad_json5::ir::board::FilledPolygon {
                layer: layer.into(),
                points: vec![(x0, 0.0), (x1, 0.0), (x1, 40.0), (x0, 40.0)],
            }],
            keepout: None,
        }
    }

    /// 三归因：无平面 / 挖槽(outline 有填充缺) / 齐排孔。
    #[test]
    fn test_reflow_break_causes() {
        let cfg = StitchConfig { all_nets: true, ..Default::default() };
        // B.Cu 全 GND 填充 x∈[0,20]；x∈[20,40] 只有 outline 无填充（挖槽区）
        let mut b = Board::default();
        b.nets = vec![NetDef { id: 0, name: "GND".into() }, NetDef { id: 1, name: "CLK_X".into() }];
        b.zones.push(zone_gnd("B.Cu", 0.0, 20.0));
        b.zones.push(Zone {
            outline: vec![(20.0, 0.0), (40.0, 0.0), (40.0, 40.0), (20.0, 40.0)],
            filled_polygons: vec![],
            ..zone_gnd("B.Cu", 20.0, 40.0)
        });
        // 高速线 F.Cu 0→40 横穿
        b.segments.push(Segment {
            start: (0.0, 20.0), end: (40.0, 20.0), width: 0.2, layer: "F.Cu".into(), net: 1,
        });
        let br = audit_reflow(&b, &cfg);
        // 应有: x>20 区域断(挖槽) 或 NoPlane——挖槽区 outline 命中 ⇒ SlotOrIsland
        assert!(!br.is_empty(), "挖槽区必须报断");
        assert!(br.iter().all(|x| x.net == "CLK_X"));
        assert!(
            br.iter().any(|x| matches!(x.cause, BreakCause::SlotOrIsland | BreakCause::NoPlane)),
            "归因应为挖槽或无平面: {:?}",
            br.iter().map(|x| &x.cause).collect::<Vec<_>>()
        );
        // 左半连续区不报
        assert!(br.iter().all(|x| x.at.0 >= 19.5), "左半不应报断: {:?}", br);

        // 齐排孔归因: 挖槽区放 4 个 via → ViaFence
        for k in 0..4 {
            b.vias.push(kicad_json5::ir::board::Via {
                at: (25.0 + k as f64 * 0.3, 20.0),
                size: 0.6, drill: 0.3,
                layers: vec!["F.Cu".into(), "B.Cu".into()],
                net: 0, via_type: String::new(),
            });
        }
        let br2 = audit_reflow(&b, &cfg);
        assert!(
            br2.iter().any(|x| matches!(x.cause, BreakCause::ViaFence { .. })),
            "via 密集区应归因齐排孔: {:?}",
            br2.iter().map(|x| &x.cause).collect::<Vec<_>>()
        );
    }

    /// 三态：不打孔=天线 / 打孔+纵向地=立体。
    #[test]
    fn test_guard_verdicts() {
        let cfg = StitchConfig { all_nets: true, ..Default::default() };
        let mut b = Board::default();
        b.nets = vec![NetDef { id: 0, name: "GND".into() }, NetDef { id: 1, name: "CLK_X".into() }];
        // 高速线 + 平行 GND guard 距 1mm，长 10mm
        b.segments.push(Segment {
            start: (0.0, 10.0), end: (10.0, 10.0), width: 0.2, layer: "F.Cu".into(), net: 1,
        });
        b.segments.push(Segment {
            start: (0.0, 11.0), end: (10.0, 11.0), width: 0.2, layer: "F.Cu".into(), net: 0,
        });
        let g = audit_guards(&b, &cfg);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].verdict, GuardVerdict::AntennaNoVias, "无孔无纵向地应判天线");

        // 补孔 + B.Cu GND → 立体
        b.vias.push(kicad_json5::ir::board::Via {
            at: (5.0, 11.0), size: 0.6, drill: 0.3,
            layers: vec!["F.Cu".into(), "B.Cu".into()], net: 0, via_type: String::new(),
        });
        b.zones.push(zone_gnd("B.Cu", 0.0, 40.0));
        let g2 = audit_guards(&b, &cfg);
        assert!(matches!(g2[0].verdict, GuardVerdict::Ok3D { .. }), "打孔+纵向地=立体: {:?}", g2[0].verdict);
    }

    #[test]
    fn test_ground_islands() {
        let mut b = Board::default();
        b.nets = vec![NetDef { id: 0, name: "GND".into() }];
        let mut z = zone_gnd("B.Cu", 0.0, 40.0);
        z.filled_polygons.push(kicad_json5::ir::board::FilledPolygon {
            layer: "B.Cu".into(),
            points: vec![(5.0, 5.0), (6.0, 5.0), (6.0, 6.0), (5.0, 6.0)], // 1mm² 孤岛
        });
        b.zones.push(z);
        let isles = find_ground_islands(&b, 2.0);
        assert_eq!(isles.len(), 1);
        assert!((isles[0].2 - 1.0).abs() < 0.01);
    }
}
