//! 制造 / 工艺规则审计：过孔排布割平面、20H 平面内缩、AC 耦合电容焊盘挖空建议。
//!
//! 判据来源：
//! - BGA 扇出格点 = 引脚间距的一半；打孔太密 = 割断电源 / 地平面（"动手术切一刀缝不起来 —— 隔开打"），
//!   "一排齐孔"的归因口径同 `reflow_audit`。
//! - 20H 规则：电源层比地层内缩 20 倍层间距，电源辐射全被地吸收。
//! - 差分对的隔直（AC 耦合）电容焊盘下方铜层要**挖空** —— 焊盘铜 + 回流路径突变造成阻抗不连续。

use kicad_json5::ir::board::Board;

use crate::current_carry::{is_ground, net_voltage};
use crate::stitching::{copper_axis, is_high_speed, StitchConfig};

// ---------------------------------------------------------------------------
// 同排齐孔检测（割平面）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ViaFence {
    /// 排方向："x"（同 x 竖排，跨 y 展开）| "y"（同 y 横排）。
    pub dir: &'static str,
    pub line: f64,
    pub from: f64,
    pub to: f64,
    pub count: usize,
    /// 排跨越的层（via span 并集，全通孔则 F/B）。
    pub layers: Vec<String>,
}

/// 检测同排齐孔：≥min_count 个 via 共线（同 x 或同 y，容差 tol_mm）即报。
/// "为好看斜着打一排 = 人为割断平面"，"地孔要错开打"。
pub fn detect_via_fences(board: &Board, min_count: usize, tol_mm: f64) -> Vec<ViaFence> {
    let mut out = Vec::new();
    let mut vias: Vec<&kicad_json5::ir::board::Via> = board.vias.iter().collect();
    // 按 x 聚类（同 x 竖排）
    vias.sort_by(|a, b| a.at.0.partial_cmp(&b.at.0).unwrap());
    let mut i = 0;
    while i < vias.len() {
        let mut j = i;
        while j + 1 < vias.len() && (vias[j + 1].at.0 - vias[i].at.0).abs() <= tol_mm {
            j += 1;
        }
        if j - i + 1 >= min_count {
            let mut run: Vec<_> = vias[i..=j].to_vec();
            run.sort_by(|a, b| a.at.1.partial_cmp(&b.at.1).unwrap());
            out.push(ViaFence {
                dir: "x",
                line: run.iter().map(|v| v.at.0).sum::<f64>() / run.len() as f64,
                from: run.first().unwrap().at.1,
                to: run.last().unwrap().at.1,
                count: run.len(),
                layers: fence_layers(&run),
            });
        }
        i = j + 1;
    }
    // 按 y 聚类（同 y 横排）
    vias.sort_by(|a, b| a.at.1.partial_cmp(&b.at.1).unwrap());
    let mut i = 0;
    while i < vias.len() {
        let mut j = i;
        while j + 1 < vias.len() && (vias[j + 1].at.1 - vias[i].at.1).abs() <= tol_mm {
            j += 1;
        }
        if j - i + 1 >= min_count {
            let mut run: Vec<_> = vias[i..=j].to_vec();
            run.sort_by(|a, b| a.at.0.partial_cmp(&b.at.0).unwrap());
            out.push(ViaFence {
                dir: "y",
                line: run.iter().map(|v| v.at.1).sum::<f64>() / run.len() as f64,
                from: run.first().unwrap().at.0,
                to: run.last().unwrap().at.0,
                count: run.len(),
                layers: fence_layers(&run),
            });
        }
        i = j + 1;
    }
    out
}

fn fence_layers(run: &[&kicad_json5::ir::board::Via]) -> Vec<String> {
    let axis = crate::stitching::SIX_LAYER_ORDER;
    let mut lo = usize::MAX;
    let mut hi = 0usize;
    for v in run {
        let (a, b) = match (v.layers.first(), v.layers.last()) {
            (Some(a), Some(b)) => (a.clone(), b.clone()),
            _ => ("F.Cu".into(), "B.Cu".into()),
        };
        let ia = axis.iter().position(|l| *l == a).unwrap_or(0);
        let ib = axis.iter().position(|l| *l == b).unwrap_or(5);
        lo = lo.min(ia.min(ib));
        hi = hi.max(ia.max(ib));
    }
    if run.is_empty() {
        vec![]
    } else {
        axis[lo..=hi].iter().map(|s| s.to_string()).collect()
    }
}

// ---------------------------------------------------------------------------
// BGA 扇出格点（格点=引脚间距/2）
// ---------------------------------------------------------------------------

/// 扇出孔格点 = pitch/2（0.8mm 间距 BGA → 0.4mm 格点；0.5mm → 0.25mm）。
/// 给布局/扇出放置的 snapping 锚点表（原点=器件中心，半格偏移）。
pub fn fanout_grid(pitch_mm: f64, half_extent_mm: f64) -> Vec<(f64, f64)> {
    let g = pitch_mm / 2.0;
    let n = (half_extent_mm / g).floor() as i32;
    let mut pts = Vec::new();
    for iy in -n..=n {
        for ix in -n..=n {
            pts.push((ix as f64 * g + g / 2.0, iy as f64 * g + g / 2.0));
        }
    }
    pts
}

// ---------------------------------------------------------------------------
// 20H 平面内缩检查
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ShrinkViolation {
    pub power_zone: String,
    pub ground_zone: String,
    /// 电源 bbox 超出“地内缩 20H 后 bbox”的量（mm，四向最大者）。
    pub overshoot_mm: f64,
    pub required_inset_mm: f64,
}

/// 20H 检查：每对（电源平面, 地平面）同板共存时，电源层 outline 的 bbox 必须
/// 落在地层 bbox 内缩 20×层间距 的范围内。层间距显式传参（stackup parser 是
/// 已知挂账，不臆造默认值）。
pub fn check_20h(board: &Board, layer_gap_mm: f64) -> Vec<ShrinkViolation> {
    let inset = 20.0 * layer_gap_mm;
    let bbox = |pts: &[(f64, f64)]| -> Option<(f64, f64, f64, f64)> {
        if pts.is_empty() {
            return None;
        }
        Some((
            pts.iter().map(|p| p.0).fold(f64::INFINITY, f64::min),
            pts.iter().map(|p| p.1).fold(f64::INFINITY, f64::min),
            pts.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max),
            pts.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max),
        ))
    };
    let mut out = Vec::new();
    for pz in &board.zones {
        let pv = net_voltage(&pz.net_name);
        if is_ground(&pz.net_name) || pv.is_none() {
            continue;
        }
        let Some(pbb) = bbox(&pz.outline) else {
            continue;
        };
        for gz in &board.zones {
            if !is_ground(&gz.net_name) || gz.layer == pz.layer {
                continue;
            }
            let Some(gbb) = bbox(&gz.outline) else {
                continue;
            };
            // 四向余量（各自 ≥0 才合法；20H=电源须落在地内缩 inset 后的界内）
            let slack = (pbb.0 - (gbb.0 + inset))
                .min(pbb.1 - (gbb.1 + inset))
                .min((gbb.2 - inset) - pbb.2)
                .min((gbb.3 - inset) - pbb.3);
            if slack < 0.0 {
                out.push(ShrinkViolation {
                    power_zone: format!("{}@{}", pz.net_name, pz.layer),
                    ground_zone: format!("{}@{}", gz.net_name, gz.layer),
                    overshoot_mm: -slack,
                    required_inset_mm: inset,
                });
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// AC 耦合电容焊盘挖空区生成
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct AcCapVoid {
    pub refdes: String,
    /// 差分对网（P/N）。
    pub pair: (String, String),
    /// 挖空多边形（pad 组 bbox 外扩 margin，参考层）。
    pub rect: (f64, f64, f64, f64), // x0 y0 x1 y1
    /// 建议挖空的参考层（cap 所在层的邻铜层）。
    pub ref_layer: String,
}

/// 识别 AC 耦合电容：2-pad 电容件，两 pad 分别挂在差分对（*_P/*_N 同名根）上。
pub fn find_ac_coupling_caps(board: &Board, cfg: &StitchConfig) -> Vec<AcCapVoid> {
    let axis = copper_axis(board);
    let margin = 0.5; // 焊盘外扩 0.5mm（经验口径，防阻抗突变带足够宽）
    let mut out = Vec::new();
    for fp in &board.footprints {
        if fp.pads.len() != 2 {
            continue;
        }
        let n1 = fp.pads[0].net_name.clone().unwrap_or_default();
        let n2 = fp.pads[1].net_name.clone().unwrap_or_default();
        if n1.is_empty() || n2.is_empty() || n1 == n2 {
            continue;
        }
        // 差分后缀两族: *_P/*_N 与 USB 风格 *DP/*DM（首版只认 _P/_N 漏掉 USB_DM 实锤）
        let root_of = |n: &str| -> Option<(String, &'static str)> {
            let u = n.to_uppercase();
            for suf in ["_DP", "_DM", "DP", "DM", "_P", "_N"] {
                if let Some(r) = u.strip_suffix(suf) {
                    let tag = if suf.ends_with("P") || suf.ends_with("DP") {
                        "P"
                    } else {
                        "N"
                    };
                    return Some((r.to_string(), tag));
                }
            }
            None
        };
        let (Some((r1, t1)), Some((r2, t2))) = (root_of(&n1), root_of(&n2)) else {
            continue;
        };
        if r1 != r2 || t1 == t2 || !is_high_speed(&n1, cfg) {
            continue; // 根不同或同极性 → 不是差分对
        }
        // pad 世界 bbox（局部 at + fp 位置；rot 忽略=保守，外扩已覆盖）
        let mut x0 = f64::INFINITY;
        let mut y0 = f64::INFINITY;
        let mut x1 = f64::NEG_INFINITY;
        let mut y1 = f64::NEG_INFINITY;
        for p in &fp.pads {
            let cx = fp.position.0 + p.position.0;
            let cy = fp.position.1 + p.position.1;
            x0 = x0.min(cx - p.size.0 / 2.0);
            x1 = x1.max(cx + p.size.0 / 2.0);
            y0 = y0.min(cy - p.size.1 / 2.0);
            y1 = y1.max(cy + p.size.1 / 2.0);
        }
        // 参考层：cap 所在层（footprint.layer）的邻铜层
        let ref_layer = axis
            .iter()
            .position(|l| *l == fp.layer)
            .and_then(|i| {
                [i.checked_sub(1), Some(i + 1).filter(|j| *j < axis.len())]
                    .into_iter()
                    .flatten()
                    .next()
            })
            .map(|ci| axis[ci].clone())
            .unwrap_or_else(|| "In1.Cu".into());
        out.push(AcCapVoid {
            refdes: fp.reference.clone(),
            pair: (n1.clone(), n2.clone()),
            rect: (x0 - margin, y0 - margin, x1 + margin, y1 + margin),
            ref_layer,
        });
    }
    out
}

/// 人读报告。
pub fn format_report(
    fences: &[ViaFence],
    shrink: &[ShrinkViolation],
    voids: &[AcCapVoid],
) -> String {
    let mut s = String::from("== 同排齐孔（割平面；'地孔要错开打'） ==\n");
    if fences.is_empty() {
        s.push_str("  ✅ 无同排齐孔\n");
    }
    for f in fences {
        s.push_str(&format!(
            "  ⚠️ {}={:.2} 方向排 {} 孔, 跨 {:.2}→{:.2} (层 {})\n",
            if f.dir == "x" { "x" } else { "y" },
            f.line,
            f.count,
            f.from,
            f.to,
            f.layers.join(",")
        ));
    }
    s.push_str("\n== 20H 平面内缩 ==\n");
    if shrink.is_empty() {
        s.push_str("  ✅ 电源平面全部满足 20H 内缩（或无同板电源/地平面对）\n");
    }
    for v in shrink {
        s.push_str(&format!(
            "  ⚠️ {} 超出 {} 的 20H 内缩边界 {:.2}mm（需内缩 {:.1}mm）\n",
            v.power_zone, v.ground_zone, v.overshoot_mm, v.required_inset_mm
        ));
    }
    s.push_str("\n== AC 耦合电容焊盘挖空建议 ==\n");
    if voids.is_empty() {
        s.push_str("  （未识别到 AC 耦合电容）\n");
    }
    for v in voids {
        s.push_str(&format!(
            "  🔧 {} ({}/{}) 参考层 {} 挖空区 ({:.2},{:.2})~({:.2},{:.2})\n",
            v.refdes, v.pair.0, v.pair.1, v.ref_layer, v.rect.0, v.rect.1, v.rect.2, v.rect.3
        ));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use kicad_json5::ir::board::{Footprint, NetDef, Pad, PadShape, PadType, Via, Zone};

    fn mkzone(net_name: &str, layer: &str, x0: f64, y0: f64, x1: f64, y1: f64) -> Zone {
        Zone {
            net: 0,
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
            outline: vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1)],
            filled_polygons: vec![],
            keepout: None,
        }
    }

    /// 验收口径：0.5mm BGA（格点=间距/2=0.25）+ 同排 ≥5 孔告警触发。
    #[test]
    fn test_via_fence_and_fanout_grid() {
        let grid = fanout_grid(0.5, 0.75);
        assert!(
            grid.contains(&(0.125, 0.125)),
            "格点=pitch/2=0.25, 首点半格偏移 0.125"
        );
        // 格点=pitch/2=0.25 且半格偏移(BGA"眼点"口径), 即 (x-0.125) 落 0.25 整数倍
        assert!(grid
            .iter()
            .all(|(x, _)| ((x - 0.125) / 0.25 - ((x - 0.125) / 0.25).round()).abs() < 1e-9));

        let mut b = Board {
            nets: vec![NetDef {
                id: 0,
                name: "GND".into(),
            }],
            ..Default::default()
        };
        // 同 y 横排 6 孔（pitch 0.5）——齐排
        for k in 0..6 {
            b.vias.push(Via {
                at: (10.0 + k as f64 * 0.5, 20.0),
                size: 0.3,
                drill: 0.15,
                layers: vec!["F.Cu".into(), "B.Cu".into()],
                net: 0,
                via_type: String::new(),
            });
        }
        // 错开的一组 4 孔（每孔 y 各异=真错开；首版数据 k%2 有一半恰好同 y 仍共线被自己抓包）
        for k in 0..4 {
            b.vias.push(Via {
                at: (30.0 + k as f64 * 0.5, 25.0 + k as f64 * 0.25),
                size: 0.3,
                drill: 0.15,
                layers: vec!["F.Cu".into(), "B.Cu".into()],
                net: 0,
                via_type: String::new(),
            });
        }
        let fences = detect_via_fences(&b, 5, 0.1);
        assert_eq!(fences.len(), 1, "只有齐排的 6 孔组触发: {:?}", fences);
        assert_eq!(fences[0].count, 6);
        assert_eq!(fences[0].dir, "y");
    }

    /// 验收口径：2.0mm 层距 ⇒ 内缩 40mm 边界检测正确。
    #[test]
    fn test_20h() {
        let mut b = Board {
            nets: vec![
                NetDef {
                    id: 0,
                    name: "GND".into(),
                },
                NetDef {
                    id: 1,
                    name: "5V".into(),
                },
            ],
            ..Default::default()
        };
        // 地 100×100 @In2, 电源 100×100 @In3（零内缩）⇒ 违规 over=40
        b.zones
            .push(mkzone("GND", "In2.Cu", 0.0, 0.0, 100.0, 100.0));
        b.zones.push(mkzone("5V", "In3.Cu", 0.0, 0.0, 100.0, 100.0));
        let v = check_20h(&b, 2.0);
        assert_eq!(v.len(), 1);
        assert!(
            (v[0].overshoot_mm - 40.0).abs() < 1e-9,
            "2mm 层距⇒20H=40mm, 零内缩超界恰 40"
        );
        assert!((v[0].required_inset_mm - 40.0).abs() < 1e-9);
        // 电源内缩 40 ⇒ 通过
        b.zones.pop();
        b.zones.push(mkzone("5V", "In3.Cu", 40.0, 40.0, 60.0, 60.0));
        assert!(check_20h(&b, 2.0).is_empty());
    }

    /// 差分串容识别 + 挖空区形状（USB 口用例）。
    #[test]
    fn test_ac_cap_void() {
        let cfg = StitchConfig::default();
        let mut b = Board {
            nets: vec![
                NetDef {
                    id: 0,
                    name: "GND".into(),
                },
                NetDef {
                    id: 1,
                    name: "USB_DP".into(),
                },
                NetDef {
                    id: 2,
                    name: "USB_DM".into(),
                },
                NetDef {
                    id: 3,
                    name: "SPI_CS".into(),
                },
            ],
            ..Default::default()
        };
        let mut cap = Footprint {
            lib_id: "Device:C".into(),
            reference: "C9".into(),
            value: "100nF".into(),
            position: (50.0, 50.0, 0.0),
            layer: "F.Cu".into(),
            uuid: None,
            locked: false,
            descr: None,
            tags: None,
            path: None,
            sheetname: None,
            sheetfile: None,
            attr: None,
            solder_mask_margin: None,
            solder_paste_margin: None,
            solder_paste_ratio: None,
            clearance: None,
            zone_connect: None,
            thermal_bridge_width: None,
            thermal_bridge_angle: None,
            pads: vec![
                Pad {
                    number: "1".into(),
                    pad_type: PadType::Smd,
                    shape: PadShape::Rect,
                    position: (-0.5, 0.0, 0.0),
                    size: (0.6, 0.8),
                    layers: vec!["F.Cu".into()],
                    drill: None,
                    net: Some(1),
                    net_name: Some("USB_DP".into()),
                    pin_function: None,
                    pin_type: None,
                    roundrect_rratio: None,
                    solder_mask_margin: None,
                    thermal_bridge_width: None,
                    thermal_bridge_angle: None,
                    thermal_gap: None,
                    clearance: None,
                    zone_connect: None,
                    remove_unused_layers: None,
                    options: None,
                    primitives: vec![],
                },
                Pad {
                    number: "2".into(),
                    pad_type: PadType::Smd,
                    shape: PadShape::Rect,
                    position: (0.5, 0.0, 0.0),
                    size: (0.6, 0.8),
                    layers: vec!["F.Cu".into()],
                    drill: None,
                    net: Some(2),
                    net_name: Some("USB_DM".into()),
                    pin_function: None,
                    pin_type: None,
                    roundrect_rratio: None,
                    solder_mask_margin: None,
                    thermal_bridge_width: None,
                    thermal_bridge_angle: None,
                    thermal_gap: None,
                    clearance: None,
                    zone_connect: None,
                    remove_unused_layers: None,
                    options: None,
                    primitives: vec![],
                },
            ],
            ..Footprint::default()
        };
        cap.pads[0].size = (0.6, 0.8);
        b.footprints.push(cap);
        let voids = find_ac_coupling_caps(&b, &cfg);
        assert_eq!(voids.len(), 1, "USB_DP/USB_DM 串容识别");
        let v = &voids[0];
        assert_eq!(v.refdes, "C9");
        // pad 组 bbox: x∈[49.2,50.8](±0.5中心±0.3) 外扩 0.5 ⇒ [48.7, 51.3]
        assert!((v.rect.0 - 48.7).abs() < 1e-9 && (v.rect.2 - 51.3).abs() < 1e-9);
        assert_eq!(
            v.ref_layer, "B.Cu",
            "两层轴 F.Cu 的邻参考=B.Cu(轴塌缩时 B 是唯一邻层)"
        );
        // 非差分对电容不识别: SPI_CS 串阻性电容
        let mut other = b.footprints[0].clone();
        other.reference = "C10".into();
        other.pads[0].net_name = Some("SPI_CS".into());
        other.pads[1].net_name = Some("GND".into());
        b.footprints.push(other);
        assert_eq!(find_ac_coupling_caps(&b, &cfg).len(), 1);
    }
}
