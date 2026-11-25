//! 换层缝合孔 / 缝合电容：审计 + 自动补孔。
//!
//! 判据来源 —— 参考平面"被迫破坏时的搭桥"：
//!   - 换层到另一 GND 层 → **缝合地孔 stitching via**（通孔连两地，回流沿地孔走）
//!   - 换层跨到 Power 层 → **缝合电容 stitching cap**（GND-PWR 搭桥；理论根基 =
//!     高频下 Power 与 GND 等效短路，去耦电容串平面）
//!   - 换层孔 100mil 范围内必打回流地孔（= 2.54mm）
//!   - 无缝合条件（周边无铜）→ 告警清单（挖槽 / 分割区，回流绕圈面积暴增）
//!
//! 三层分职：本模块=确定性判据与几何（找空位/碰撞检查）；"哪根是高速线"是语义
//! 层——默认 net 名模式匹配，可 --nets/--all 覆盖。电容插入是器件级改动（BOM/
//! 位号），首版只出**建议清单**；缝合 via 是纯铜改动，自动插入。
//!
//! 参考平面口径（诚实记账）：用 zone 的 **filled_polygons**（实际填铜）判定换层
//! 点相邻层的参考类型，zone 无填充数据时回退 outline（此时结论偏乐观，报告标注）。

use kicad_json5::ir::board::{Board, Via};

use crate::current_carry::{is_ground, net_voltage};

/// 换层点（信号网 via）。
#[derive(Debug, Clone)]
pub struct LayerTransition {
    pub net: String,
    pub at: (f64, f64),
    /// via 的层跨度端点（span 首末层）。
    pub from_layer: String,
    pub to_layer: String,
}

/// 相邻层参考平面类型。
#[derive(Debug, Clone, PartialEq)]
pub enum RefPlane {
    Ground(String),
    Power(String),
    None,
}

/// 审计发现（三态 + 已满足）。
#[derive(Debug, Clone)]
pub struct StitchFinding {
    pub net: String,
    pub at: (f64, f64),
    pub from_ref: RefPlane,
    pub to_ref: RefPlane,
    pub verdict: Verdict,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    /// GND→GND 且附近已有地孔（含本 via 邻域）——回流路径已通。
    Ok,
    /// GND→GND 缺就近地孔 → 补缝合 via（apply_stitching_vias 可自动落位）。
    NeedVia,
    /// GND↔PWR 缺搭桥电容 → 建议清单（器件级改动不自动插）。
    NeedCap { pwr_net: String },
    /// 一侧无参考铜（跨分割/挖槽）→ 告警：回流绕圈。
    NoReference,
}

#[derive(Debug, Clone)]
pub struct StitchConfig {
    /// 就近判定半径（mm），默认 2.54 = 100mil。
    pub max_dist: f64,
    /// 高速网名模式（正则不走，简单 contains/后缀匹配已够）。
    pub net_patterns: Vec<String>,
    /// 命中任何网（不过滤）。
    pub all_nets: bool,
    /// 插孔时的净空（mm）。
    pub clearance: f64,
    /// 插孔 size/drill。
    pub via_size: f64,
    pub via_drill: f64,
}

impl Default for StitchConfig {
    fn default() -> Self {
        StitchConfig {
            max_dist: 2.54,
            net_patterns: [
                "CLK", "LVDS", "USB", "DP", "DM", "MIPI", "DIFF", "SDA", "SCL",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            all_nets: false,
            clearance: 0.2,
            via_size: 0.6,
            via_drill: 0.3,
        }
    }
}

pub fn net_display(board: &Board, id: u32) -> String {
    board
        .nets
        .iter()
        .find(|n| n.id == id)
        .map(|n| n.name.clone())
        .unwrap_or_default()
}

pub fn is_high_speed(net: &str, cfg: &StitchConfig) -> bool {
    if cfg.all_nets {
        return true;
    }
    let u = net.to_uppercase();
    u.ends_with("_P")
        || u.ends_with("_N")
        || u.ends_with("DP")
        || u.ends_with("DM")
        || cfg
            .net_patterns
            .iter()
            .any(|p| u.contains(&p.to_uppercase()))
}

/// 点在多边形内（射线法）。
pub fn point_in_poly(pt: (f64, f64), poly: &[(f64, f64)]) -> bool {
    let mut inside = false;
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = poly[i];
        let (xj, yj) = poly[j];
        if (yi > pt.1) != (yj > pt.1) {
            let x_int = (xj - xi) * (pt.1 - yi) / (yj - yi) + xi;
            if pt.0 < x_int {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

/// 标准六层序 (外到内到外), `copper_axis` 和制造规则审计共用.
pub(crate) const SIX_LAYER_ORDER: [&str; 6] =
    ["F.Cu", "In1.Cu", "In2.Cu", "In3.Cu", "In4.Cu", "B.Cu"];

/// 实际铜层轴：F/B 恒在 ∪ 板上出现过铜的层（zone/segment/via 层），按标准六层序。
/// ⚠ 不能用 board.layers——Board::default() 只有 F/B 两层声明（首版测试全挂的根因：
/// 邻层轴塌缩成 [F.Cu, B.Cu]，In1 的 GND zone 永远不被看见）。两层板轴=[F,B]
/// （F.Cu 邻=B.Cu ✓）；via 跨过的内层是活动铜层也入轴（信号的参考层查找必需）。
pub fn copper_axis(board: &Board) -> Vec<String> {
    let mut axis: Vec<String> = Vec::new();
    for l in SIX_LAYER_ORDER {
        if l == "F.Cu" || l == "B.Cu" {
            axis.push(l.to_string());
            continue;
        }
        let used = board.zones.iter().any(|z| z.layer == l)
            || board.segments.iter().any(|s| s.layer == l)
            || board.vias.iter().any(|v| v.layers.iter().any(|vl| vl == l));
        if used {
            axis.push(l.to_string());
        }
    }
    for d in &board.layers {
        // 非标准层名（方言板）追加尾部
        if d.name.ends_with(".Cu") && !axis.contains(&d.name) {
            let used = board.zones.iter().any(|z| z.layer == d.name)
                || board.segments.iter().any(|s| s.layer == d.name)
                || board
                    .vias
                    .iter()
                    .any(|v| v.layers.iter().any(|vl| vl == &d.name));
            if used {
                axis.push(d.name.clone());
            }
        }
    }
    axis
}

/// 相邻层（铜层轴中 layer 的上/下层）。
fn neighbor_layers(board: &Board, layer: &str) -> Vec<String> {
    let order = copper_axis(board);
    match order.iter().position(|l| l == layer) {
        Some(i) => {
            let mut v = Vec::new();
            if i > 0 {
                v.push(order[i - 1].clone());
            }
            if i + 1 < order.len() {
                v.push(order[i + 1].clone());
            }
            v
        }
        None => vec![],
    }
}

/// 某层在某点的参考平面类型：优先 filled_polygons（实际铜），回退 outline。
fn ref_plane_at(board: &Board, layer: &str, at: (f64, f64)) -> RefPlane {
    let mut fallback: Option<&kicad_json5::ir::board::Zone> = None;
    for z in &board.zones {
        if z.layer != layer {
            continue;
        }
        let hit_fill = z
            .filled_polygons
            .iter()
            .any(|fp| fp.layer == layer && point_in_poly(at, &fp.points));
        if hit_fill {
            return classify(z.net_name.as_str());
        }
        if fallback.is_none() && point_in_poly(at, &z.outline) {
            fallback = Some(z);
        }
    }
    match fallback {
        Some(z) => classify(&z.net_name),
        None => RefPlane::None,
    }
}

fn classify(net: &str) -> RefPlane {
    if is_ground(net) {
        RefPlane::Ground(net.to_string())
    } else if net_voltage(net).is_some() || net.is_empty() {
        // 空 net zone 不算参考；可解析电压网 = 电源平面
        if net.is_empty() {
            RefPlane::None
        } else {
            RefPlane::Power(net.to_string())
        }
    } else {
        RefPlane::None
    }
}

/// 换层点提取：高速信号网的 via（层跨度端点层）。
pub fn detect_transitions(board: &Board, cfg: &StitchConfig) -> Vec<LayerTransition> {
    let mut out = Vec::new();
    for via in &board.vias {
        let net = net_display(board, via.net);
        if net.is_empty() || is_ground(&net) || net_voltage(&net).is_some() {
            continue; // 地/电源网自身的 via 不算信号换层
        }
        if !is_high_speed(&net, cfg) {
            continue;
        }
        let (from, to) = match (via.layers.first(), via.layers.last()) {
            (Some(a), Some(b)) if a != b => (a.clone(), b.clone()),
            _ => ("F.Cu".to_string(), "B.Cu".to_string()), // 无 layers=通孔全跨
        };
        out.push(LayerTransition {
            net,
            at: via.at,
            from_layer: from,
            to_layer: to,
        });
    }
    out
}

/// 主审计：逐换层点判三态。
pub fn audit_stitching(board: &Board, cfg: &StitchConfig) -> Vec<StitchFinding> {
    let transitions = detect_transitions(board, cfg);
    let mut out = Vec::new();
    for tr in &transitions {
        // 参考层：信号在 from_layer 上时其相邻层的参考；进入 to_layer 同理。
        // 口径：换层点的"破坏"在于两层参考平面可能不同——各取信号侧最近参考层。
        let from_ref = neighbor_layers(board, &tr.from_layer)
            .into_iter()
            .map(|l| ref_plane_at(board, &l, tr.at))
            .find(|r| *r != RefPlane::None)
            .unwrap_or(RefPlane::None);
        let to_ref = neighbor_layers(board, &tr.to_layer)
            .into_iter()
            .map(|l| ref_plane_at(board, &l, tr.at))
            .find(|r| *r != RefPlane::None)
            .unwrap_or(RefPlane::None);

        let verdict = match (&from_ref, &to_ref) {
            (RefPlane::None, _) | (_, RefPlane::None) => Verdict::NoReference,
            (RefPlane::Ground(_), RefPlane::Ground(_)) => {
                let has_gnd_via = board.vias.iter().any(|v| {
                    let n = net_display(board, v.net);
                    n != tr.net
                        && is_ground(&n)
                        && (v.at.0 - tr.at.0).hypot(v.at.1 - tr.at.1) <= cfg.max_dist
                });
                if has_gnd_via {
                    Verdict::Ok
                } else {
                    Verdict::NeedVia
                }
            }
            (RefPlane::Ground(_), RefPlane::Power(p))
            | (RefPlane::Power(p), RefPlane::Ground(_)) => {
                // 附近已有搭桥电容（同时挂 GND 与该电源网的电容件）？
                let has_cap = board.footprints.iter().any(|fp| {
                    let nets: Vec<&str> = fp
                        .pads
                        .iter()
                        .filter_map(|p| p.net_name.as_deref())
                        .collect();
                    nets.iter().any(|n| is_ground(n))
                        && nets.contains(&p.as_str())
                        && (fp.position.0 - tr.at.0).hypot(fp.position.1 - tr.at.1)
                            <= cfg.max_dist * 2.0
                });
                if has_cap {
                    Verdict::Ok
                } else {
                    Verdict::NeedCap { pwr_net: p.clone() }
                }
            }
            (RefPlane::Power(_), RefPlane::Power(_)) => Verdict::Ok, // 同为电源参考
        };
        out.push(StitchFinding {
            net: tr.net.clone(),
            at: tr.at,
            from_ref,
            to_ref,
            verdict,
        });
    }
    out
}

/// 位置是否空（不撞既有 pad/via/seg，含净空带）。
fn spot_free(board: &Board, at: (f64, f64), r: f64, clr: f64) -> bool {
    // via
    for v in &board.vias {
        if (v.at.0 - at.0).hypot(v.at.1 - at.1) < v.size / 2.0 + r + clr {
            return false;
        }
    }
    // pads（footprint 局部坐标 + 器件平移；rot 忽略=保守近似，pad 世界角处理归 board_assert）
    for fp in &board.footprints {
        for p in &fp.pads {
            let px = fp.position.0 + p.position.0;
            let py = fp.position.1 + p.position.1;
            let (sw, sh) = p.size;
            if (px - at.0).abs() < sw / 2.0 + r + clr && (py - at.1).abs() < sh / 2.0 + r + clr {
                return false;
            }
        }
    }
    // segments（AABB 粗判 + 点线距）
    for s in &board.segments {
        let (x0, y0) = s.start;
        let (x1, y1) = s.end;
        if at.0 < x0.min(x1) - r - clr
            || at.0 > x0.max(x1) + r + clr
            || at.1 < y0.min(y1) - r - clr
            || at.1 > y0.max(y1) + r + clr
        {
            continue;
        }
        // 点到线段距离
        let dx = x1 - x0;
        let dy = y1 - y0;
        let l2 = dx * dx + dy * dy;
        let t = if l2 == 0.0 {
            0.0
        } else {
            (((at.0 - x0) * dx + (at.1 - y0) * dy) / l2).clamp(0.0, 1.0)
        };
        let cx = x0 + t * dx;
        let cy = y0 + t * dy;
        if (cx - at.0).hypot(cy - at.1) < s.width / 2.0 + r + clr {
            return false;
        }
    }
    true
}

/// 自动补缝合 via：对 NeedVia 的换层点，就近找空位落 GND 孔（同信号 via 层跨）。
/// 返回 (插入的 via 列表, 未找到空位的点)。
pub fn apply_stitching_vias(
    board: &mut Board,
    findings: &[StitchFinding],
    cfg: &StitchConfig,
) -> (Vec<Via>, Vec<(f64, f64)>) {
    let gnd_net_id = board
        .nets
        .iter()
        .find(|n| is_ground(&n.name))
        .map(|n| n.id)
        .or_else(|| board.nets.iter().find(|n| n.name == "GND").map(|n| n.id));
    let r = cfg.via_size / 2.0;
    let mut inserted = Vec::new();
    let mut failed = Vec::new();
    for f in findings {
        if f.verdict != Verdict::NeedVia {
            continue;
        }
        // 8 方向 × 距离梯（0.65~1.5mm）试探空位
        let mut placed: Option<Via> = None;
        'search: for dist in [0.65, 0.8, 1.0, 1.2, 1.5] {
            for k in 0..8 {
                let ang = k as f64 * std::f64::consts::PI / 4.0;
                let cand = (f.at.0 + dist * ang.cos(), f.at.1 + dist * ang.sin());
                if spot_free(board, cand, r, cfg.clearance) {
                    let v = Via {
                        at: cand,
                        size: cfg.via_size,
                        drill: cfg.via_drill,
                        layers: vec!["F.Cu".into(), "B.Cu".into()],
                        net: gnd_net_id.unwrap_or(0),
                        via_type: String::new(),
                    };
                    placed = Some(v);
                    break 'search;
                }
            }
        }
        match placed {
            Some(v) => {
                board.vias.push(Via {
                    at: v.at,
                    ..v.clone()
                });
                inserted.push(v);
            }
            None => failed.push(f.at),
        }
    }
    (inserted, failed)
}

/// 人读报告。
pub fn format_report(
    findings: &[StitchFinding],
    inserted: &[Via],
    failed: &[(f64, f64)],
) -> String {
    let mut s = String::from(
        "== 换层缝合审计（回流判据: GND→GND 补地孔 / GND↔PWR 补搭桥电容 / 无参考=告警） ==\n",
    );
    for f in findings {
        let v = match &f.verdict {
            Verdict::Ok => "✅ 已满足".to_string(),
            Verdict::NeedVia => "🔧 缺缝合地孔(自动插入候选)".to_string(),
            Verdict::NeedCap { pwr_net } => format!("🧣 缺搭桥电容 GND↔{pwr_net}(建议, 不自动插)"),
            Verdict::NoReference => "⚠️ 跨分割/无参考铜——回流绕圈".to_string(),
        };
        s.push_str(&format!(
            "  [{:<10}] ({:.2},{:.2}) {:?} → {:?}  {}\n",
            f.net, f.at.0, f.at.1, f.from_ref, f.to_ref, v
        ));
    }
    s.push_str(&format!(
        "\n汇总: {} 换层点 = {} ok / {} need-via / {} need-cap / {} no-ref\n",
        findings.len(),
        findings.iter().filter(|f| f.verdict == Verdict::Ok).count(),
        findings
            .iter()
            .filter(|f| f.verdict == Verdict::NeedVia)
            .count(),
        findings
            .iter()
            .filter(|f| matches!(f.verdict, Verdict::NeedCap { .. }))
            .count(),
        findings
            .iter()
            .filter(|f| f.verdict == Verdict::NoReference)
            .count(),
    ));
    if !inserted.is_empty() {
        s.push_str(&format!("已插入缝合 via {} 个:\n", inserted.len()));
        for v in inserted {
            s.push_str(&format!(
                "  ({:.2},{:.2}) size {}\n",
                v.at.0, v.at.1, v.size
            ));
        }
    }
    if !failed.is_empty() {
        s.push_str(&format!(
            "⚠️ 找不到空位的点 {} 个: {:?}\n",
            failed.len(),
            failed
        ));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mkboard() -> Board {
        let mut b = Board::default();
        b.nets.clear();
        b.nets.push(kicad_json5::ir::board::NetDef {
            id: 1,
            name: "GND".into(),
        });
        b.nets.push(kicad_json5::ir::board::NetDef {
            id: 2,
            name: "CLK_MAIN".into(),
        });
        b
    }

    fn zone(
        net_id: u32,
        net_name: &str,
        layer: &str,
        pts: Vec<(f64, f64)>,
    ) -> kicad_json5::ir::board::Zone {
        kicad_json5::ir::board::Zone {
            net: net_id,
            net_name: net_name.into(),
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
            outline: pts,
            filled_polygons: vec![],
            keepout: None,
        }
    }

    #[test]
    fn test_point_in_poly() {
        let sq = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
        assert!(point_in_poly((5.0, 5.0), &sq));
        assert!(!point_in_poly((15.0, 5.0), &sq));
    }

    /// 判据三态：GND→GND 缺孔 / GND→PWR 缺容 / 跨分割告警 / 已有地孔=ok。
    #[test]
    fn test_audit_three_verdicts() {
        let cfg = StitchConfig {
            all_nets: true,
            ..Default::default()
        };
        // 板：In1 全板 GND；In2 左半 GND 右半 5V；B 侧信号 via 在右半（In2 参考=5V）
        let mut b = mkboard();
        b.nets.push(kicad_json5::ir::board::NetDef {
            id: 3,
            name: "5V".into(),
        });
        b.zones.push(zone(
            1,
            "GND",
            "In1.Cu",
            vec![(0.0, 0.0), (80.0, 0.0), (80.0, 80.0), (0.0, 80.0)],
        ));
        b.zones.push(zone(
            1,
            "GND",
            "In2.Cu",
            vec![(0.0, 0.0), (40.0, 0.0), (40.0, 80.0), (0.0, 80.0)],
        ));
        b.zones.push(zone(
            3,
            "5V",
            "In2.Cu",
            vec![(40.0, 0.0), (80.0, 0.0), (80.0, 80.0), (40.0, 80.0)],
        ));
        // 信号 via @ (60,40): F.Cu→In2.Cu? 用 F→In3 跨度让 from 参考取 In1(GND) to 取 In2(5V)
        b.vias.push(Via {
            at: (60.0, 40.0),
            size: 0.6,
            drill: 0.3,
            layers: vec!["F.Cu".into(), "In3.Cu".into()],
            net: 2,
            via_type: String::new(),
        });
        let f = audit_stitching(&b, &cfg);
        assert_eq!(f.len(), 1);
        assert!(
            matches!(f[0].verdict, Verdict::NeedCap { .. }),
            "GND→5V 应判缺容: {:?}",
            f[0].verdict
        );

        // 同一点放一个 GND via 在 In2 也全 GND 的世界 → NeedVia; 已有近旁 GND via → Ok
        let mut b2 = mkboard();
        b2.zones.push(zone(
            1,
            "GND",
            "In1.Cu",
            vec![(0.0, 0.0), (80.0, 0.0), (80.0, 80.0), (0.0, 80.0)],
        ));
        b2.zones.push(zone(
            1,
            "GND",
            "In2.Cu",
            vec![(0.0, 0.0), (80.0, 0.0), (80.0, 80.0), (0.0, 80.0)],
        ));
        b2.vias.push(Via {
            at: (60.0, 40.0),
            size: 0.6,
            drill: 0.3,
            layers: vec!["F.Cu".into(), "In3.Cu".into()],
            net: 2,
            via_type: String::new(),
        });
        let f2 = audit_stitching(&b2, &cfg);
        assert_eq!(f2[0].verdict, Verdict::NeedVia);
        // 近旁补 GND via → Ok
        b2.vias.push(Via {
            at: (61.0, 40.0),
            size: 0.6,
            drill: 0.3,
            layers: vec!["F.Cu".into(), "B.Cu".into()],
            net: 1,
            via_type: String::new(),
        });
        let f3 = audit_stitching(&b2, &cfg);
        assert_eq!(f3[0].verdict, Verdict::Ok);

        // 跨分割: 无任何 zone → NoReference
        let mut b3 = mkboard();
        b3.vias.push(Via {
            at: (60.0, 40.0),
            size: 0.6,
            drill: 0.3,
            layers: vec!["F.Cu".into(), "In3.Cu".into()],
            net: 2,
            via_type: String::new(),
        });
        let f4 = audit_stitching(&b3, &cfg);
        assert_eq!(f4[0].verdict, Verdict::NoReference);
    }

    /// 插孔：NeedVia 点旁自动落位且不撞信号 via 本体。
    #[test]
    fn test_apply_vias() {
        let cfg = StitchConfig {
            all_nets: true,
            ..Default::default()
        };
        let mut b = mkboard();
        b.zones.push(zone(
            1,
            "GND",
            "In1.Cu",
            vec![(0.0, 0.0), (80.0, 0.0), (80.0, 80.0), (0.0, 80.0)],
        ));
        b.zones.push(zone(
            1,
            "GND",
            "In2.Cu",
            vec![(0.0, 0.0), (80.0, 0.0), (80.0, 80.0), (0.0, 80.0)],
        ));
        b.vias.push(Via {
            at: (60.0, 40.0),
            size: 0.6,
            drill: 0.3,
            layers: vec!["F.Cu".into(), "In3.Cu".into()],
            net: 2,
            via_type: String::new(),
        });
        let findings = audit_stitching(&b, &cfg);
        let (inserted, failed) = apply_stitching_vias(&mut b, &findings, &cfg);
        assert_eq!(inserted.len(), 1);
        assert!(failed.is_empty());
        let v = &inserted[0];
        assert_eq!(v.net, 1); // GND
        let d = (v.at.0 - 60.0).hypot(v.at.1 - 40.0);
        assert!(d >= 0.65 - 1e-9, "落位距离 {}", d);
    }
}
