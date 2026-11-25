//! GND/PWR 连通性诊断器。
//!
//! 复用 `router::export_wavefront_grid_dilated` 的 dilation 网格（障碍口径与
//! 布线器一致：pad 物理缓冲 + trace 中心线 0.2mm CLR [+ 可选膨胀轮]），对目标
//! 铺铜网（AGND/DGND/3V3 这类）做：
//!   1. zone filled_polygon 栅格化 → 目标网铜格（zone ∪ pad ∪ trace ∪ via）
//!   2. 铜连通分量（层内 4 邻域；跨层只认 Via 格与 ThruHole Pad 格同位桥）
//!   3. 不含 pad/via 锚的分量 = 孤岛；分量边界接触的异网铜 = 归因
//!
//! 只读板子，不改板。打通方案（GPGraph/MST 让路重布）是后续阶段，不在此模块。

use std::collections::{BTreeMap, VecDeque};

use kicad_json5::ir::board::Board;

/// 单个铜连通分量（孤岛或主域）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct GroundRegion {
    pub net_name: String,
    pub layer: String,
    /// 无任何 pad/via 锚（悬空铜）。
    pub is_island: bool,
    pub cells: usize,
    pub area_mm2: f64,
    /// (min_x, min_y, max_x, max_y) 世界 mm。
    pub bbox_mm: (f64, f64, f64, f64),
    /// 归因：分量边界 8 邻域接触的异网铜 → 接触格数（降序）。
    pub contact_nets: Vec<(String, usize)>,
    /// 成分：zone fill 格（grid 无身份、靠栅格化入组的铜）
    pub zone_cells: usize,
    /// 成分：走线身份格（Trace(net)）
    pub trace_cells: usize,
    /// 成分：pad 身份格（Pad(net)，含膨胀缓冲）
    pub pad_cells: usize,
    /// 成分：via 身份格（Via(net)）
    pub via_cells: usize,
    /// 岛中心落入的模块路径（kdesign 侧后处理标注，空串=未命中）
    #[serde(default)]
    pub module: String,
}

#[derive(Debug, Default, serde::Serialize)]
pub struct GroundNetReport {
    pub net_name: String,
    pub anchor_pads: usize,
    pub anchor_vias: usize,
    /// 目标网铜格总数（zone+pad+trace+via）。
    pub copper_cells: usize,
    /// 铜连通分量数（含主域）。
    pub region_count: usize,
    /// 分量按面积降序；`islands` = 其中 is_island 的。
    pub regions: Vec<GroundRegion>,
}

#[derive(Debug, Default, serde::Serialize)]
pub struct GroundReport {
    pub grid_res_mm: f64,
    pub layers: Vec<String>,
    pub nets: Vec<GroundNetReport>,
}

impl GroundReport {
    pub fn total_islands(&self) -> usize {
        self.nets
            .iter()
            .map(|n| n.regions.iter().filter(|r| r.is_island).count())
            .sum()
    }
}

/// 目标网集合：不传则自动取板上所有 zone 的 net_name（铺铜诊断的主场景）。
pub fn resolve_target_nets(board: &Board, explicit: &[String]) -> anyhow::Result<Vec<String>> {
    if !explicit.is_empty() {
        return Ok(explicit.to_vec());
    }
    let mut names: Vec<String> = Vec::new();
    for z in &board.zones {
        if !z.net_name.is_empty() && !names.contains(&z.net_name) {
            names.push(z.net_name.clone());
        }
    }
    if names.is_empty() {
        anyhow::bail!("no --net given and board has no zones");
    }
    Ok(names)
}

/// 主入口。`res` 建议默认 0.25（与布线主流程一致），`dilate` 额外膨胀轮数
/// （1 轮 ≈ 0.25mm，补偿 trace 中心线口径不含半线宽）。
pub fn audit_ground(
    board: &Board,
    targets: &[String],
    layer_config: crate::layer_config::BoardLayerConfig,
    res: f64,
    dilate: usize,
) -> anyhow::Result<GroundReport> {
    let export = crate::router::export_wavefront_grid_dilated(board, layer_config, res, dilate)?;
    let cols = export.cols;
    let rows = export.rows;
    let per_layer = cols * rows;
    let n_layers = export.layers.len();
    let (ox, oy) = export.origin_mm;

    let net_id_of: BTreeMap<&str, u32> =
        board.nets.iter().map(|n| (n.name.as_str(), n.id)).collect();
    let name_of: BTreeMap<u32, String> =
        board.nets.iter().map(|n| (n.id, n.name.clone())).collect();
    let mut target_ids: BTreeMap<u32, String> = BTreeMap::new();
    for t in targets {
        let id = *net_id_of
            .get(t.as_str())
            .ok_or_else(|| anyhow::anyhow!("net '{}' not found on board", t))?;
        target_ids.insert(id, t.clone());
    }

    // ---- 目标网铜格掩码（0=无铜，否则目标网 id；两网铜格不混算）----
    let mut copper = vec![0u32; n_layers * per_layer];
    let mut anchor_pads: BTreeMap<u32, usize> = BTreeMap::new();
    let mut anchor_vias: BTreeMap<u32, usize> = BTreeMap::new();
    for (i, &v) in export.grid_u32.iter().enumerate() {
        if v < 2 {
            continue; // Free / Blocked
        }
        let (byte, nid) = if v < 1_000_000 {
            (2u8, v - 2)
        } else if v < 2_000_000 {
            (3u8, v - 1_000_000)
        } else {
            (4u8, v - 2_000_000)
        };
        if target_ids.contains_key(&nid) {
            copper[i] = nid;
            match byte {
                2 => *anchor_pads.entry(nid).or_default() += 1,
                4 => *anchor_vias.entry(nid).or_default() += 1,
                _ => {}
            }
        }
    }

    // ---- zone filled_polygon 栅格化（扫描线，格中心采样）----
    let layer_index: BTreeMap<&str, usize> = export
        .layers
        .iter()
        .enumerate()
        .map(|(i, s)| (s.as_str(), i))
        .collect();
    for z in &board.zones {
        if std::env::var("GROUND_AUDIT_DBG").is_ok() {
            eprintln!(
                "[dbg] zone net='{}' filled={:?}",
                z.net_name,
                z.filled_polygons
                    .iter()
                    .map(|f| (f.layer.clone(), f.points.len()))
                    .collect::<Vec<_>>()
            );
        }
        let in_targets = net_id_of
            .get(z.net_name.as_str())
            .map(|id| target_ids.contains_key(id))
            .unwrap_or(false);
        if !in_targets {
            continue;
        }
        let dbg = std::env::var("GROUND_AUDIT_DBG").is_ok();
        let mut filled_cnt = 0usize;
        for fp in &z.filled_polygons {
            if let Some(&fli) = layer_index.get(fp.layer.as_str()) {
                rasterize_polygon(&fp.points, cols, rows, ox, oy, res, |r, c| {
                    copper[fli * per_layer + r * cols + c] =
                        *net_id_of.get(z.net_name.as_str()).unwrap_or(&0);
                    filled_cnt += 1;
                });
            } else if dbg {
                eprintln!(
                    "[dbg] zone '{}' polygon layer '{}' NOT in signal layers",
                    z.net_name, fp.layer
                );
            }
        }
        if dbg {
            eprintln!(
                "[dbg] zone '{}' rasterized {} cells",
                z.net_name, filled_cnt
            );
        }
    }

    // ---- 铜连通分量（跨层桥 = 同 (row,col) 的 Via 格）----
    // grid 的 mark_existing_traces 只标 segments 不标 vias，缝合孔/换层 via
    // 在此补栅格化（目标网 via：copper 身份 + is_via 桥标记）。
    let mut is_via = vec![false; n_layers * per_layer];
    for (i, &v) in export.grid_u32.iter().enumerate() {
        if v >= 2_000_000 {
            let nid = v - 2_000_000;
            if target_ids.contains_key(&nid) {
                is_via[i] = true;
            }
        }
    }
    for via in &board.vias {
        if via.net == 0 || !target_ids.contains_key(&via.net) {
            continue;
        }
        let covered: Vec<usize> = via
            .layers
            .iter()
            .filter_map(|l| layer_index.get(l.as_str()).copied())
            .collect();
        if covered.is_empty() {
            continue;
        }
        let gc = (((via.at.0 - ox) / res).round() as i64).clamp(0, cols as i64 - 1) as usize;
        let gr = (((via.at.1 - oy) / res).round() as i64).clamp(0, rows as i64 - 1) as usize;
        let rad = ((via.size / 2.0) / res).ceil() as i64;
        for dr in -rad..=rad {
            for dc in -rad..=rad {
                if dr * dr + dc * dc > rad * rad {
                    continue; // 圆内
                }
                let (rr, cc) = (gr as i64 + dr, gc as i64 + dc);
                if rr < 0 || cc < 0 || rr >= rows as i64 || cc >= cols as i64 {
                    continue;
                }
                for &li in &covered {
                    let idx = li * per_layer + rr as usize * cols + cc as usize;
                    copper[idx] = via.net;
                    is_via[idx] = true;
                }
            }
        }
        *anchor_vias.entry(via.net).or_default() += 1;
    }

    let mut seen = vec![false; n_layers * per_layer];
    // 一次性扫描全板铜分量，按起点格的网归属归组（分量天然单网：
    // 异网铜有 CLR 隔离，跨层桥也带同网条件）。
    let mut by_net: BTreeMap<u32, Vec<GroundRegion>> = BTreeMap::new();
    let mut cells_by_net: BTreeMap<u32, usize> = BTreeMap::new();

    for start in 0..n_layers * per_layer {
        if copper[start] == 0 || seen[start] {
            continue;
        }
        let cur_net = copper[start];
        let mut comp: Vec<usize> = Vec::new();
        let mut q = VecDeque::new();
        seen[start] = true;
        q.push_back(start);
        while let Some(i) = q.pop_front() {
            comp.push(i);
            let l = i / per_layer;
            let rc = i % per_layer;
            let r = rc / cols;
            let c = rc % cols;
            let v = export.grid_u32[i];
            // 层内 4 邻域
            for &(dr, dc) in &[(0i64, 1i64), (0, -1), (1, 0), (-1, 0)] {
                let (rr, cc) = (r as i64 + dr as i64, c as i64 + dc as i64);
                if rr < 0 || cc < 0 || rr >= rows as i64 || cc >= cols as i64 {
                    continue;
                }
                let j = l * per_layer + rr as usize * cols + cc as usize;
                if copper[j] == cur_net && !seen[j] {
                    seen[j] = true;
                    q.push_back(j);
                }
            }
            // 跨层桥：同 (r,c) 的 via 格（任意层，同网）
            if is_via[i] {
                for l2 in 0..n_layers {
                    let j = l2 * per_layer + rc;
                    if j != i && copper[j] == cur_net && !seen[j] {
                        seen[j] = true;
                        q.push_back(j);
                    }
                }
            }
            // 跨层桥：ThruHole pad——同 (r,c) 多层皆为 Pad 身份即贯穿（同网）
            // （SMD pad 只标声明层，不会误连独立层走线）
            if (2..1_000_000).contains(&v) {
                for l2 in 0..n_layers {
                    let j = l2 * per_layer + rc;
                    if j != i
                        && copper[j] == cur_net
                        && !seen[j]
                        && (2..1_000_000).contains(&export.grid_u32[j])
                    {
                        seen[j] = true;
                        q.push_back(j);
                    }
                }
            }
        }
        let (min_r, min_c, max_r, max_c) = comp.iter().fold(
            (usize::MAX, usize::MAX, 0usize, 0usize),
            |(a, b, c, d), &i| {
                let rc = i % per_layer;
                let (r, cc) = (rc / cols, rc % cols);
                (a.min(r), b.min(cc), c.max(r), d.max(cc))
            },
        );
        let mut contacts: BTreeMap<u32, usize> = BTreeMap::new();
        for &i in &comp {
            let l = i / per_layer;
            let rc = i % per_layer;
            let r = rc / cols;
            let c = rc % cols;
            for &(dr, dc) in &[
                (0i64, 1i64),
                (0, -1),
                (1, 0),
                (-1, 0),
                (1, 1),
                (1, -1),
                (-1, 1),
                (-1, -1),
            ] {
                let (rr, cc) = (r as i64 + dr, c as i64 + dc);
                if rr < 0 || cc < 0 || rr >= rows as i64 || cc >= cols as i64 {
                    continue;
                }
                let j = l * per_layer + rr as usize * cols + cc as usize;
                let v = export.grid_u32[j];
                if v < 2 {
                    continue; // Free / Blocked：无净身份，不归因
                }
                let nid = if v >= 2_000_000 {
                    v - 2_000_000 // Via
                } else if v >= 1_000_000 {
                    v - 1_000_000 // Trace
                } else {
                    v - 2 // Pad
                };
                if nid != cur_net {
                    *contacts.entry(nid).or_default() += 1;
                }
            }
        }
        let mut contact_nets: Vec<(String, usize)> = contacts
            .into_iter()
            .map(|(id, n)| {
                (
                    name_of.get(&id).cloned().unwrap_or_else(|| id.to_string()),
                    n,
                )
            })
            .collect();
        contact_nets.sort_by(|a, b| b.1.cmp(&a.1));
        let has_pad_anchor = comp
            .iter()
            .any(|&i| (2..1_000_000).contains(&export.grid_u32[i]));
        let has_via_anchor = comp.iter().any(|&i| export.grid_u32[i] >= 2_000_000);
        // 走线连接：分量含同网 Trace 格即连网（uc=0 口径下 trace 必接 pad）
        let has_trace_connect = comp
            .iter()
            .any(|&i| (1_000_000..2_000_000).contains(&export.grid_u32[i]));
        let (mut zone_cells, mut trace_cells, mut pad_cells, mut via_cells) =
            (0usize, 0usize, 0usize, 0usize);
        for &i in &comp {
            let v = export.grid_u32[i];
            match v {
                0 => zone_cells += 1, // grid 无身份、由 zone 栅格化入组
                1 => {}               // Blocked（元件体），zone 铜叠其上
                2..1_000_000 => pad_cells += 1,
                1_000_000..2_000_000 => trace_cells += 1,
                _ => via_cells += 1,
            }
        }
        let region = GroundRegion {
            net_name: name_of.get(&cur_net).cloned().unwrap_or_default(),
            layer: export.layers[comp[0] / per_layer].clone(),
            is_island: !(has_pad_anchor || has_via_anchor || has_trace_connect),
            cells: comp.len(),
            area_mm2: comp.len() as f64 * res * res,
            bbox_mm: (
                ox + min_c as f64 * res,
                oy + min_r as f64 * res,
                ox + (max_c + 1) as f64 * res,
                oy + (max_r + 1) as f64 * res,
            ),
            contact_nets: contact_nets.into_iter().take(5).collect(),
            zone_cells,
            trace_cells,
            pad_cells,
            via_cells,
            module: String::new(),
        };
        *cells_by_net.entry(cur_net).or_default() += comp.len();
        by_net.entry(cur_net).or_default().push(region);
    }

    // ---- 按目标网组装报告 ----
    let mut report = GroundReport {
        grid_res_mm: export.grid_res_mm,
        layers: export.layers.clone(),
        nets: Vec::new(),
    };
    for (&nid, tname) in &target_ids {
        let mut regions = by_net.remove(&nid).unwrap_or_default();
        regions.sort_by(|a, b| b.area_mm2.total_cmp(&a.area_mm2));
        let rep = GroundNetReport {
            net_name: tname.clone(),
            anchor_pads: *anchor_pads.get(&nid).unwrap_or(&0),
            anchor_vias: *anchor_vias.get(&nid).unwrap_or(&0),
            copper_cells: *cells_by_net.get(&nid).unwrap_or(&0),
            region_count: regions.len(),
            regions,
        };
        report.nets.push(rep);
    }

    Ok(report)
}

/// 扫描线多边形栅格化（格中心采样，偶奇填充）。
fn rasterize_polygon(
    points: &[(f64, f64)],
    cols: usize,
    rows: usize,
    ox: f64,
    oy: f64,
    res: f64,
    mut set: impl FnMut(usize, usize),
) {
    if points.len() < 3 {
        return;
    }
    let min_y = points.iter().map(|p| p.1).fold(f64::MAX, f64::min);
    let max_y = points.iter().map(|p| p.1).fold(f64::MIN, f64::max);
    let r0 = (((min_y - oy) / res).floor() as i64).max(0) as usize;
    let r1 = (((max_y - oy) / res).ceil() as i64).min(rows as i64) as usize;
    let n = points.len();
    for r in r0..r1.min(rows) {
        let yc = oy + (r as f64 + 0.5) * res; // 格中心 y
        let mut xs: Vec<f64> = Vec::new();
        for i in 0..n {
            let (x1, y1) = points[i];
            let (x2, y2) = points[(i + 1) % n]; // 闭合：尾点连回首点
            if (y1 <= yc && y2 > yc) || (y2 <= yc && y1 > yc) {
                let t = (yc - y1) / (y2 - y1);
                xs.push(x1 + t * (x2 - x1));
            }
        }
        xs.sort_by(|a, b| a.total_cmp(b));
        for pair in xs.chunks(2) {
            if pair.len() < 2 {
                continue;
            }
            let c0 = (((pair[0] - ox) / res).ceil() as i64).max(0) as usize;
            let c1 = (((pair[1] - ox) / res).floor() as i64).min(cols as i64 - 1) as usize;
            for c in c0..=c1.min(cols.saturating_sub(1)) {
                let xc = ox + (c as f64 + 0.5) * res;
                if xc >= pair[0] && xc <= pair[1] {
                    set(r, c);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kicad_json5::ir::board::{DrillDef, FilledPolygon, NetDef, Pad, PadShape, PadType, Zone};

    fn small_board() -> Board {
        let mut b = Board::default();
        b.nets.push(NetDef {
            id: 1,
            name: "AGND".into(),
        });
        b.nets.push(NetDef {
            id: 2,
            name: "SIG".into(),
        });
        b
    }

    #[test]
    fn island_split_by_trace_is_attributed() {
        let mut b = small_board();
        // 两块 AGND zone 铜（左/右），中间被 SIG trace 切开；左块带 GND pad（锚）。
        let z = Zone {
            net: 1,
            net_name: "AGND".into(),
            layer: "F.Cu".into(),
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
            outline: vec![],
            filled_polygons: vec![
                FilledPolygon {
                    layer: "F.Cu".into(),
                    points: vec![(0.0, 0.0), (4.0, 0.0), (4.0, 10.0), (0.0, 10.0)],
                },
                FilledPolygon {
                    layer: "F.Cu".into(),
                    points: vec![(5.2, 0.0), (7.0, 0.0), (7.0, 10.0), (5.2, 10.0)],
                },
            ],
            keepout: None,
        };
        b.zones.push(z);
        // SIG trace 竖切 x=4.8，把两块铜隔开（0.2 线宽+0.2 CLR 膨胀贴上右块 zone 左缘）
        for y in 0..40 {
            b.segments.push(kicad_json5::ir::board::Segment {
                layer: "F.Cu".into(),
                net: 2,
                width: 0.2,
                start: (4.8, y as f64 * 0.25),
                end: (4.8, (y as f64 + 1.0) * 0.25),
            });
        }
        // GND pad（ThruHole）在左块 (2,3)：mark_pads 会标成 Pad(1)（F/B 两层，跨层桥）
        let mut fp = kicad_json5::ir::board::Footprint::new("T:R", "R1", "R1");
        fp.position = (2.0, 3.0, 0.0);
        fp.pads.push(Pad {
            number: "1".into(),
            pad_type: PadType::ThruHole,
            shape: PadShape::Circle,
            position: (0.0, 0.0, 0.0),
            size: (1.0, 1.0),
            layers: vec!["*.Cu".into()],
            drill: Some(DrillDef {
                diameter: 0.6,
                width: None,
                offset: None,
            }),
            net: Some(1),
            net_name: Some("AGND".into()),
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
            primitives: Vec::new(),
        });
        b.footprints.push(fp);
        // 右块旁放一个 net=0 的 dummy footprint（撑大 grid bbox，覆盖两块 zone）
        let mut fp2 = kicad_json5::ir::board::Footprint::new("T:R", "R2", "R2");
        fp2.position = (9.0, 7.0, 0.0);
        fp2.pads.push(Pad {
            number: "1".into(),
            pad_type: PadType::ThruHole,
            shape: PadShape::Circle,
            position: (0.0, 0.0, 0.0),
            size: (1.0, 1.0),
            layers: vec!["*.Cu".into()],
            drill: Some(DrillDef {
                diameter: 0.6,
                width: None,
                offset: None,
            }),
            net: None,
            net_name: None,
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
            primitives: Vec::new(),
        });
        b.footprints.push(fp2);

        let report = audit_ground(
            &b,
            &["AGND".into()],
            crate::layer_config::BoardLayerConfig::two_layer(),
            0.25,
            0,
        )
        .unwrap();
        let agnd = report.nets.iter().find(|n| n.net_name == "AGND").unwrap();
        assert!(
            agnd.region_count >= 2,
            "expected >=2 regions, got {}",
            agnd.region_count
        );
        let islands: Vec<_> = agnd.regions.iter().filter(|r| r.is_island).collect();
        eprintln!(
            "REGIONS {:?}",
            agnd.regions
                .iter()
                .map(|r| (
                    r.cells,
                    r.area_mm2,
                    r.is_island,
                    r.bbox_mm,
                    r.contact_nets.clone()
                ))
                .collect::<Vec<_>>()
        );
        assert_eq!(islands.len(), 1, "right zone should be the single island");
        assert!(
            islands[0].contact_nets.iter().any(|(n, _)| n == "SIG"),
            "island should be attributed to SIG trace, got {:?}",
            islands[0].contact_nets
        );
    }
}
