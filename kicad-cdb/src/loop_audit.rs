//! P2-1 立项候选 → audit-loop：热回路 / 回流收敛 / 敷铜切割 定量审计。
//!
//! 方法来自 battery 板手算验证（见知识库《PCB 回路与过孔定量分析方法》）：
//! 1. 识别 buck 拓扑（SW+BST 双特征的 IC）→ 输入电容/电感/输出电容
//! 2. 沿铜 BFS 求热回路各段路径长度（含 T 型 junction 连通）
//! 3. 每个 SMD GND pad → 最近 GND 过孔收敛距离（THT 直落 B.Cu pour 豁免）
//! 4. 热回路包络区内 B.Cu 异网穿越统计（切割回流镜面的嫌疑走线）

use kicad_json5::ir::board::{Board, Footprint, Pad, PadType};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

#[derive(Debug, Clone, serde::Serialize)]
pub struct HotLoop {
    pub ic_ref: String,
    pub ic_lib: String,
    pub vin_net: String,
    pub sw_net: String,
    pub out_net: String,
    /// Cin+ → VIN pin along copper (mm); ≤2 optimal, ≤4 acceptable
    pub cin_len_mm: Option<f64>,
    /// SW pin → L input (mm)
    pub sw_len_mm: Option<f64>,
    /// L output → Cout+ (mm)
    pub out_len_mm: Option<f64>,
    pub cin_ref: Option<String>,
    pub l_ref: Option<String>,
    pub cout_ref: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ReturnPath {
    pub pad: String,
    pub x_mm: f64,
    pub y_mm: f64,
    /// distance to nearest GND via (mm); ≤1 good, ≤2 advisory, >2 action
    pub via_mm: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PlaneCut {
    pub net: String,
    pub segs: usize,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct LoopAuditReport {
    pub hot_loop: Option<HotLoop>,
    pub return_paths: Vec<ReturnPath>,
    pub plane_cuts: Vec<PlaneCut>,
    /// return paths worse than 2mm
    pub return_path_issues: usize,
    /// hot loop segments over the acceptable threshold
    pub hot_loop_issues: usize,
}

fn is_gnd_net(name: &str) -> bool {
    let u = name.to_uppercase();
    u == "GND"
        || u.ends_with("_GND")
        || u.starts_with("GND_")
        || u == "AGND"
        || u == "DGND"
        || u == "PGND"
        || u == "SGND"
}

fn pad_world(fp: &Footprint, pad: &Pad) -> (f64, f64) {
    let (fx, fy, fr) = fp.position;
    let (lx, ly, lrot) = pad.position;
    let total = (fr + lrot).to_radians();
    let (s, c) = (total.sin(), total.cos());
    (fx + lx * c + ly * s, fy - lx * s + ly * c)
}

struct CompPad {
    ref_: String,
    num: String,
    #[allow(dead_code)] // 审计记录保留字段
    lib: String,
    x: f64,
    y: f64,
    net: u32,
    smd: bool,
}

fn collect_pads(board: &Board, id2name: &HashMap<u32, String>) -> Vec<CompPad> {
    let mut out = Vec::new();
    for fp in &board.footprints {
        for pad in &fp.pads {
            let (x, y) = pad_world(fp, pad);
            out.push(CompPad {
                ref_: fp.reference.clone(),
                num: pad.number.clone(),
                lib: fp.lib_id.clone(),
                x,
                y,
                net: pad.net.unwrap_or(0),
                smd: pad.pad_type == PadType::Smd,
            });
        }
    }
    let _ = id2name;
    out
}

/// Identify the switching converter: an IC whose nets include both a SW/PH
/// (switch/phase) and a BST/BOOT (bootstrap) net.
fn find_buck<'a>(board: &'a Board, id2name: &HashMap<u32, String>) -> Option<&'a Footprint> {
    let sw_bst = |fp: &Footprint| -> Option<(String, String)> {
        let mut sw = None;
        let mut bst = None;
        for pad in &fp.pads {
            if let Some(name) = pad.net.and_then(|id| id2name.get(&id)) {
                let u = name.to_uppercase();
                if (u.contains("SW") && !u.contains("Switch")) || u.contains("PH") {
                    sw = Some(name.clone());
                }
                if u.contains("BST") || u.contains("BOOT") {
                    bst = Some(name.clone());
                }
            }
        }
        match (sw, bst) {
            (Some(s), Some(b)) if s != b => Some((s, b)),
            _ => None,
        }
    };
    board.footprints.iter().find(|fp| sw_bst(fp).is_some())
}

/// Copper-graph BFS with T-junction awareness: a segment endpoint landing on
/// the MIDDLE of another segment is a connection (battery C3.1 case).
struct CopperGraph {
    adj: HashMap<(i64, i64), Vec<((i64, i64), f64)>>,
    nodes: Vec<(f64, f64)>,
}

impl CopperGraph {
    fn build(board: &Board, net: u32, layer: &str) -> Self {
        Self::build_with_pads(board, net, layer, true)
    }

    /// `with_pad_junctions`: endpoints landing inside a pad's copper connect
    /// through the pad — model pads as star junctions so pad-mediated
    /// connections (C2 stub → U1.1 pad → VIN rail) are graph-reachable.
    fn build_with_pads(board: &Board, net: u32, layer: &str, with_pad_junctions: bool) -> Self {
        let key = |p: (f64, f64)| ((p.0 * 1000.0).round() as i64, (p.1 * 1000.0).round() as i64);
        // collect segments of this net+layer
        let segs: Vec<((f64, f64), (f64, f64))> = board
            .segments
            .iter()
            .filter(|s| s.net == net && s.layer == layer)
            .map(|s| (s.start, s.end))
            .collect();

        // node set: all endpoints + interior T-junction points
        let mut node_list: Vec<(f64, f64)> = Vec::new();
        let mut node_idx: HashMap<(i64, i64), usize> = HashMap::new();
        let get_node = |p: (f64, f64),
                        node_list: &mut Vec<(f64, f64)>,
                        node_idx: &mut HashMap<(i64, i64), usize>|
         -> usize {
            let k = key(p);
            if let Some(&i) = node_idx.get(&k) {
                return i;
            }
            node_list.push(p);
            node_idx.insert(k, node_list.len() - 1);
            node_list.len() - 1
        };

        let mut edges: Vec<(usize, usize, f64)> = Vec::new();
        for (a, b) in &segs {
            let ia = get_node(*a, &mut node_list, &mut node_idx);
            let ib = get_node(*b, &mut node_list, &mut node_idx);
            edges.push((ia, ib, (a.0 - b.0).hypot(a.1 - b.1)));
        }
        // T-junctions: endpoint of one seg lying strictly inside another
        let eps = 0.01;
        for (a, b) in &segs {
            for p in node_list.clone() {
                let d = point_seg_dist(p, *a, *b);
                let ends = point_seg_dist(p, *a, *a).min(point_seg_dist(p, *b, *b));
                if d < eps && ends > eps * 10.0 {
                    // split: connect p to segment endpoints via sub-segment distances
                    let ip = get_node(p, &mut node_list, &mut node_idx);
                    let ia = get_node(*a, &mut node_list, &mut node_idx);
                    let ib = get_node(*b, &mut node_list, &mut node_idx);
                    if ip != ia {
                        edges.push((ip, ia, (p.0 - a.0).hypot(p.1 - a.1)));
                    }
                    if ip != ib {
                        edges.push((ip, ib, (p.0 - b.0).hypot(p.1 - b.1)));
                    }
                }
            }
        }

        let mut adj: HashMap<(i64, i64), Vec<((i64, i64), f64)>> = HashMap::new();
        for (a, b, w) in &edges {
            add_edge(&mut adj, &node_list, *a, *b, *w);
        }

        // pad junctions: endpoints within pad copper of the same net join
        // through the pad
        if with_pad_junctions {
            for fp in &board.footprints {
                for pad in &fp.pads {
                    if pad.net != Some(net) {
                        continue;
                    }
                    // layer check: SMD pads only on their copper; THT on all
                    let on_layer = pad.layers.iter().any(|l| l == layer || l == "*.Cu")
                        || (pad.pad_type == PadType::ThruHole && layer.ends_with(".Cu"));
                    if !on_layer {
                        continue;
                    }
                    let (pcx, pcy) = {
                        let (fx, fy, fr) = fp.position;
                        let (lx, ly, lrot) = pad.position;
                        let total = (fr + lrot).to_radians();
                        let (s, c) = (total.sin(), total.cos());
                        (fx + lx * c + ly * s, fy - lx * s + ly * c)
                    };
                    let r = pad.size.0.max(pad.size.1) / 2.0 + 0.05;
                    let inside: Vec<usize> = (0..node_list.len())
                        .filter(|&i| (node_list[i].0 - pcx).hypot(node_list[i].1 - pcy) <= r)
                        .collect();
                    if inside.len() >= 2 {
                        let ip = get_node((pcx, pcy), &mut node_list, &mut node_idx);
                        for i in inside {
                            let w = (node_list[i].0 - pcx).hypot(node_list[i].1 - pcy);
                            add_edge(&mut adj, &node_list, i, ip, w);
                        }
                    }
                }
            }
        }
        CopperGraph {
            adj,
            nodes: node_list,
        }
    }

    /// Shortest path length between two world points (nearest graph nodes).
    fn path_len(&self, from: (f64, f64), to: (f64, f64), snap: f64) -> Option<f64> {
        let key = |p: (f64, f64)| ((p.0 * 1000.0).round() as i64, (p.1 * 1000.0).round() as i64);
        let nearest = |p: (f64, f64)| -> Option<usize> {
            let mut best: Option<(f64, usize)> = None;
            for (i, n) in self.nodes.iter().enumerate() {
                let d = (p.0 - n.0).hypot(p.1 - n.1);
                if d <= snap && best.map(|(bd, _)| d < bd).unwrap_or(true) {
                    best = Some((d, i));
                }
            }
            best.map(|(_, i)| i)
        };
        let a = nearest(from)?;
        let b = nearest(to)?;
        let ka = key(self.nodes[a]);
        let kb = key(self.nodes[b]);
        if ka == kb {
            return Some(0.0);
        }
        let mut dist: HashMap<(i64, i64), f64> = HashMap::new();
        dist.insert(ka, 0.0);
        let mut pq: VecDeque<((i64, i64), f64)> = VecDeque::new();
        pq.push_back((ka, 0.0));
        while let Some((cur, d)) = pq.pop_front() {
            if d > *dist.get(&cur).unwrap_or(&f64::MAX) {
                continue;
            }
            if cur == kb {
                return Some(d);
            }
            if let Some(neigh) = self.adj.get(&cur) {
                for (next, w) in neigh {
                    let nd = d + w;
                    if nd < *dist.get(next).unwrap_or(&f64::MAX) - 1e-9 {
                        dist.insert(*next, nd);
                        pq.push_back((*next, nd));
                    }
                }
            }
        }
        None
    }
}

fn add_edge(
    adj: &mut HashMap<(i64, i64), Vec<((i64, i64), f64)>>,
    node_list: &[(f64, f64)],
    a: usize,
    b: usize,
    w: f64,
) {
    let ka = (
        (node_list[a].0 * 1000.0).round() as i64,
        (node_list[a].1 * 1000.0).round() as i64,
    );
    let kb = (
        (node_list[b].0 * 1000.0).round() as i64,
        (node_list[b].1 * 1000.0).round() as i64,
    );
    adj.entry(ka).or_default().push((kb, w));
    adj.entry(kb).or_default().push((ka, w));
}

fn point_seg_dist(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (vx, vy) = (b.0 - a.0, b.1 - a.1);
    let len2 = vx * vx + vy * vy;
    if len2 < 1e-12 {
        return (p.0 - a.0).hypot(p.1 - a.1);
    }
    let t = (((p.0 - a.0) * vx + (p.1 - a.1) * vy) / len2).clamp(0.0, 1.0);
    let (cx, cy) = (a.0 + t * vx, a.1 + t * vy);
    (p.0 - cx).hypot(p.1 - cy)
}

pub fn audit_loop(board: &Board) -> LoopAuditReport {
    let mut report = LoopAuditReport::default();
    let id2name: HashMap<u32, String> = board.nets.iter().map(|n| (n.id, n.name.clone())).collect();
    let net_id =
        |name: &str| -> Option<u32> { board.nets.iter().find(|n| n.name == name).map(|n| n.id) };

    // ── 1. hot loop ────────────────────────────────────────────────
    if let Some(ic) = find_buck(board, &id2name) {
        let nets_of = |fp: &Footprint| -> Vec<String> {
            fp.pads
                .iter()
                .filter_map(|p| p.net.and_then(|id| id2name.get(&id)).cloned())
                .collect()
        };
        let ic_nets = nets_of(ic);
        let sw_net = ic_nets
            .iter()
            .find(|n| {
                let u = n.to_uppercase();
                (u.contains("SW") && !u.contains("Switch")) || u.contains("PH")
            })
            .cloned();
        let vin_net = ic_nets
            .iter()
            .find(|n| {
                let u = n.to_uppercase();
                u.contains("VIN")
                    || u.contains("VBAT")
                    || u.contains("VBUS")
                    || (u.contains("VCC") && !u.contains("EN"))
            })
            .cloned();

        if let (Some(sw_net), Some(vin_net)) = (sw_net.clone(), vin_net.clone()) {
            let pads = collect_pads(board, &id2name);
            let is_cap = |lib: &str| {
                let l = lib.to_uppercase();
                l.contains("CAPACITOR") || l.contains("CP_ELEC") || l.contains("_C_")
            };
            let is_ind = |lib: &str| {
                let l = lib.to_uppercase();
                l.contains("INDUCTOR") || l.contains("_L_") || l.contains("COIL")
            };

            let sw_id = net_id(&sw_net);
            let vin_id = net_id(&vin_net);

            // inductor on SW net
            let l_fp = board
                .footprints
                .iter()
                .find(|fp| is_ind(&fp.lib_id) && fp.pads.iter().any(|p| p.net == sw_id));
            // output net = the inductor's other net
            let out_net = l_fp.and_then(|l| {
                l.pads
                    .iter()
                    .filter_map(|p| p.net.and_then(|id| id2name.get(&id)).cloned())
                    .find(|n| n != &sw_net)
            });

            // Cin: capacitor on VIN net nearest to the IC
            let ic_pos = (ic.position.0, ic.position.1);
            let cin = board
                .footprints
                .iter()
                .filter(|fp| is_cap(&fp.lib_id) && fp.pads.iter().any(|p| p.net == vin_id))
                .min_by_key(|fp| {
                    let (x, y) = (fp.position.0, fp.position.1);
                    ((x - ic_pos.0) * (x - ic_pos.0) + (y - ic_pos.1) * (y - ic_pos.1)) as i64
                });
            // Cout: capacitor on output net nearest to L
            let out_id = out_net.as_deref().and_then(net_id);
            let cout = match (&l_fp, out_id) {
                (Some(l), Some(oid)) => {
                    let lp = (l.position.0, l.position.1);
                    board
                        .footprints
                        .iter()
                        .filter(|fp| {
                            is_cap(&fp.lib_id) && fp.pads.iter().any(|p| p.net == Some(oid))
                        })
                        .min_by_key(|fp| {
                            let (x, y) = (fp.position.0, fp.position.1);
                            ((x - lp.0) * (x - lp.0) + (y - lp.1) * (y - lp.1)) as i64
                        })
                }
                _ => None,
            };

            // copper paths
            let g_vin = vin_id.map(|id| CopperGraph::build(board, id, "F.Cu"));
            let g_sw = CopperGraph::build(board, sw_id.unwrap_or(0), "F.Cu");
            let g_out = out_id.map(|id| CopperGraph::build(board, id, "F.Cu"));

            let _pad_at = |pred: &dyn Fn(&CompPad) -> bool| -> Option<(f64, f64)> {
                pads.iter().find(|p| pred(p)).map(|p| (p.x, p.y))
            };

            let cin_len = match (&cin, &g_vin) {
                (Some(c), Some(g)) => {
                    let cp = pads
                        .iter()
                        .find(|p| p.ref_ == c.reference && Some(p.net) == vin_id);
                    let vp = pads
                        .iter()
                        .find(|p| p.ref_ == ic.reference && Some(p.net) == vin_id);
                    match (cp, vp) {
                        (Some(cp), Some(vp)) => g.path_len((cp.x, cp.y), (vp.x, vp.y), 0.6),
                        _ => None,
                    }
                }
                _ => None,
            };
            let sw_len = {
                let sw_pad = pads
                    .iter()
                    .find(|p| p.ref_ == ic.reference && Some(p.net) == sw_id);
                let l_pad = l_fp.and_then(|l| {
                    l.pads
                        .iter()
                        .find(|p| p.net == sw_id)
                        .map(|p| pad_world(l, p))
                });
                match (sw_pad, l_pad) {
                    (Some(s), Some(lp)) => g_sw.path_len((s.x, s.y), lp, 0.6),
                    _ => None,
                }
            };
            let out_len = match (&l_fp, &cout, &g_out) {
                (Some(l), Some(c), Some(g)) => {
                    let l_out_pad = l
                        .pads
                        .iter()
                        .find(|p| p.net == out_id)
                        .map(|p| pad_world(l, p));
                    let c_pad = pads
                        .iter()
                        .find(|p| p.ref_ == c.reference && Some(p.net) == out_id);
                    match (l_out_pad, c_pad) {
                        (Some(lp), Some(cp)) => g.path_len(lp, (cp.x, cp.y), 0.6),
                        _ => None,
                    }
                }
                _ => None,
            };

            let judge = |v: Option<f64>, good: f64, ok: f64| -> usize {
                match v {
                    Some(v) if v <= good => 0,
                    Some(v) if v <= ok => 0,
                    Some(_) => 1,
                    None => 0,
                }
            };
            report.hot_loop_issues =
                judge(cin_len, 2.0, 4.0) + judge(sw_len, 5.0, 8.0) + judge(out_len, 3.0, 5.0);

            report.hot_loop = Some(HotLoop {
                ic_ref: ic.reference.clone(),
                ic_lib: ic.lib_id.clone(),
                vin_net,
                sw_net,
                out_net: out_net.clone().unwrap_or_default(),
                cin_len_mm: cin_len,
                sw_len_mm: sw_len,
                out_len_mm: out_len,
                cin_ref: cin.map(|c| c.reference.clone()),
                l_ref: l_fp.map(|l| l.reference.clone()),
                cout_ref: cout.map(|c| c.reference.clone()),
            });
        }
    }

    // ── 2. GND return convergence ─────────────────────────────────
    let gnd_ids: HashSet<u32> = board
        .nets
        .iter()
        .filter(|n| is_gnd_net(&n.name))
        .map(|n| n.id)
        .collect();
    let gnd_vias: Vec<(f64, f64)> = board
        .vias
        .iter()
        .filter(|v| gnd_ids.contains(&v.net))
        .map(|v| v.at)
        .collect();
    if !gnd_vias.is_empty() {
        let pads = collect_pads(board, &id2name);
        // pads with a GND segment endpoint inside their copper are trace-fed —
        // the via-distance criterion doesn't apply (battery TP2 case)
        let trace_connected = |p: &CompPad| -> bool {
            board.segments.iter().any(|s| {
                gnd_ids.contains(&s.net)
                    && s.layer == "F.Cu"
                    && (point_seg_dist(s.start, (p.x, p.y), (p.x, p.y)) <= 1.2
                        || point_seg_dist(s.end, (p.x, p.y), (p.x, p.y)) <= 1.2)
            })
        };
        for p in &pads {
            if !gnd_ids.contains(&p.net) || !p.smd {
                continue;
            }
            if let Some(via) = gnd_vias
                .iter()
                .map(|&(vx, vy)| (p.x - vx).hypot(p.y - vy))
                .fold(None::<f64>, |acc, d| match acc {
                    Some(a) if a <= d => acc,
                    _ => Some(d),
                })
            {
                let fed = trace_connected(p);
                let effective = if fed { 0.0 } else { via };
                if effective > 2.0 {
                    report.return_path_issues += 1;
                }
                report.return_paths.push(ReturnPath {
                    pad: format!("{}.{}", p.ref_, p.num),
                    x_mm: p.x,
                    y_mm: p.y,
                    via_mm: effective,
                });
            }
        }
        report
            .return_paths
            .sort_by(|a, b| b.via_mm.partial_cmp(&a.via_mm).unwrap());
    }

    // ── 3. plane cuts under the hot loop ──────────────────────────
    if let Some(hl) = &report.hot_loop {
        // envelope = pads of IC + Cin + L + Cout, expanded 2mm
        let members: Vec<String> = [
            Some(hl.ic_ref.clone()),
            hl.cin_ref.clone(),
            hl.l_ref.clone(),
            hl.cout_ref.clone(),
        ]
        .into_iter()
        .flatten()
        .collect();
        let mut bbox: Option<(f64, f64, f64, f64)> = None;
        for fp in &board.footprints {
            if !members.contains(&fp.reference) {
                continue;
            }
            for pad in &fp.pads {
                let (x, y) = pad_world(fp, pad);
                bbox = Some(match bbox {
                    None => (x, y, x, y),
                    Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
                });
            }
        }
        if let Some((mut x0, mut y0, mut x1, mut y1)) = bbox {
            x0 -= 2.0;
            y0 -= 2.0;
            x1 += 2.0;
            y1 += 2.0;
            let zone_layer = board
                .zones
                .iter()
                .find(|z| gnd_ids.contains(&z.net))
                .map(|z| z.layer.clone())
                .unwrap_or_else(|| "B.Cu".into());
            let mut cuts: BTreeMap<String, usize> = BTreeMap::new();
            for seg in &board.segments {
                if seg.layer != zone_layer || is_gnd_net(&seg.net.to_string()) {
                    continue;
                }
                let name = id2name.get(&seg.net).cloned().unwrap_or_default();
                if is_gnd_net(&name) {
                    continue;
                }
                let in_box = |p: (f64, f64)| p.0 >= x0 && p.0 <= x1 && p.1 >= y0 && p.1 <= y1;
                if in_box(seg.start) || in_box(seg.end) {
                    *cuts.entry(name).or_default() += 1;
                }
            }
            report.plane_cuts = cuts
                .into_iter()
                .map(|(net, segs)| PlaneCut { net, segs })
                .collect();
        }
    }

    report
}

/// Human-readable report; returns (text, advisory_count, action_count).
pub fn format_loop_report(report: &LoopAuditReport) -> (String, usize, usize) {
    use std::fmt::Write;
    let mut out = String::new();
    let mut advisory = 0usize;
    let mut action = 0usize;

    match &report.hot_loop {
        Some(hl) => {
            let _ = writeln!(out, "buck: {} ({})", hl.ic_ref, hl.ic_lib);
            let mut seg = |label: &str, v: Option<f64>, good: f64, ok: f64| -> String {
                match v {
                    Some(v) if v <= good => format!("{} {:.1}mm ✓", label, v),
                    Some(v) if v <= ok => {
                        advisory += 1;
                        format!("{} {:.1}mm ⚠ (>{} optimal)", label, v, good)
                    }
                    Some(v) => {
                        action += 1;
                        format!("{} {:.1}mm ✗ (>{})", label, v, ok)
                    }
                    None => format!("{} n/a", label),
                }
            };
            let _ = writeln!(
                out,
                "热回路: {} | {} | {}",
                seg(
                    &format!("{}+→VIN", hl.cin_ref.as_deref().unwrap_or("?")),
                    hl.cin_len_mm,
                    2.0,
                    4.0
                ),
                seg("SW→L", hl.sw_len_mm, 5.0, 8.0),
                seg(
                    &format!(
                        "{}→{}+",
                        hl.l_ref.as_deref().unwrap_or("?"),
                        hl.cout_ref.as_deref().unwrap_or("?")
                    ),
                    hl.out_len_mm,
                    3.0,
                    5.0
                )
            );
        }
        None => out.push_str("buck: 未检出（无 SW+BST 双特征 IC）\n"),
    }

    if !report.return_paths.is_empty() {
        let ok = report
            .return_paths
            .iter()
            .filter(|r| r.via_mm <= 1.0)
            .count();
        let fed = report
            .return_paths
            .iter()
            .filter(|r| r.via_mm <= 0.001)
            .count();
        let worst = report.return_paths.iter().find(|r| r.via_mm > 0.001);
        match worst {
            Some(w) => {
                let _ = writeln!(
                    out,
                    "回流收敛: {}/{} ≤1mm（{} 走线馈电/孔上 pad），最差孔距 {} {:.2}mm",
                    ok,
                    report.return_paths.len(),
                    fed,
                    w.pad,
                    w.via_mm
                );
            }
            None => {
                let _ = writeln!(
                    out,
                    "回流收敛: {}/{} 全部走线馈电或孔上 pad ✓",
                    ok,
                    report.return_paths.len()
                );
            }
        }
        for r in report.return_paths.iter().filter(|r| r.via_mm > 1.0) {
            if r.via_mm > 2.0 {
                action += 1;
                let _ = writeln!(
                    out,
                    "  ✗ {} @({:.1},{:.1}) 距最近 GND 孔 {:.2}mm —— 加回流孔",
                    r.pad, r.x_mm, r.y_mm, r.via_mm
                );
            } else {
                advisory += 1;
                let _ = writeln!(out, "  ⚠ {} {:.2}mm（advisory）", r.pad, r.via_mm);
            }
        }
    } else {
        out.push_str("回流收敛: 无 GND 过孔可参照（或无 SMD GND pad）\n");
    }

    if report.plane_cuts.is_empty() {
        out.push_str("敷铜切割: 热回路包络区无异网穿越 ✓");
    } else {
        // loop's own rails crossing under themselves are normal; foreign nets
        // slicing the return plane are the actionable ones
        let rails: Vec<String> = report
            .hot_loop
            .as_ref()
            .map(|h| vec![h.vin_net.clone(), h.sw_net.clone(), h.out_net.clone()])
            .unwrap_or_default();
        let foreign: Vec<&PlaneCut> = report
            .plane_cuts
            .iter()
            .filter(|c| !rails.contains(&c.net) && !is_gnd_net(&c.net))
            .collect();
        let rail_cuts: Vec<String> = report
            .plane_cuts
            .iter()
            .filter(|c| rails.contains(&c.net))
            .map(|c| format!("{}×{}", c.net, c.segs))
            .collect();
        let _ = write!(out, "敷铜切割:");
        if !rail_cuts.is_empty() {
            let _ = write!(out, " 自轨 {}（正常）", rail_cuts.join(" "));
        }
        if foreign.is_empty() {
            if rail_cuts.is_empty() {
                out.push_str(" 无");
            }
            out.push_str(" ✓");
        } else {
            advisory += foreign.len();
            let list: Vec<String> = foreign
                .iter()
                .map(|c| format!("{}×{}", c.net, c.segs))
                .collect();
            let _ = write!(
                out,
                " ⚠ 外轨穿越回流区（挪 F.Cu 或绕行）: {}",
                list.join(" ")
            );
        }
    }

    (out, advisory, action)
}

#[cfg(test)]
mod loop_audit_tests {
    use super::*;

    #[test]
    fn test_copper_graph_pad_junction() {
        // stub ends inside a pad whose other side feeds a rail — the pad is
        // the junction (battery C2 case)
        let mut board = Board::new();
        let n = board.add_net("VIN");
        board.segments.push(kicad_json5::ir::board::Segment {
            start: (32.3, 28.6),
            end: (32.3, 26.9),
            width: 0.25,
            layer: "F.Cu".into(),
            net: n,
        });
        board.segments.push(kicad_json5::ir::board::Segment {
            start: (29.45, 26.75),
            end: (32.05, 26.75),
            width: 0.5,
            layer: "F.Cu".into(),
            net: n,
        });
        // IC pad bridging the stub end and the rail end
        let mut fp = Footprint::new("Test:IC", "U1", "X");
        fp.position = (32.05, 26.75, 0.0);
        fp.pads.push(Pad {
            number: "1".into(),
            pad_type: PadType::Smd,
            shape: kicad_json5::ir::board::PadShape::Rect,
            position: (0.0, 0.0, 0.0),
            size: (0.7, 0.6),
            layers: vec!["F.Cu".into()],
            drill: None,
            net: Some(n),
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
        board.footprints.push(fp);

        let g = CopperGraph::build(&board, n, "F.Cu");
        let len = g.path_len((32.3, 28.6), (32.05, 26.75), 0.6);
        assert!(len.is_some(), "pad junction must connect stub to rail");
        assert!((len.unwrap() - (1.7 + 0.29)).abs() < 0.15, "got {:?}", len);
    }
}
