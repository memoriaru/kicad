//! Routing-quality audits (P1-2): junction angles, degenerate overlaps,
//! corner mitering. Logic ported from the battery-board hand scripts.

use kicad_json5::ir::board::{Board, Segment};
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct AngleIssue {
    pub angle_deg: f64,
    pub net: String,
    pub layer: String,
    pub at: (f64, f64),
    pub kind: &'static str,
}

#[derive(Debug, Default)]
pub struct AngleReport {
    pub acute: Vec<AngleIssue>,
    pub overlaps: Vec<AngleIssue>,
    pub right_angles: usize,
    pub miter_candidates: usize,
}

fn dist2(a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)
}

fn vertex_angle(oa: (f64, f64), p: (f64, f64), ob: (f64, f64)) -> f64 {
    let v1 = (oa.0 - p.0, oa.1 - p.1);
    let v2 = (ob.0 - p.0, ob.1 - p.1);
    let d = (v1.0 * v2.0 + v1.1 * v2.1) / (v1.0.hypot(v1.1) * v2.0.hypot(v2.1) + 1e-9);
    d.clamp(-1.0, 1.0).acos().to_degrees()
}

fn seg_dist_pt(px: f64, py: f64, s: (f64, f64), e: (f64, f64)) -> f64 {
    let (dx, dy) = (e.0 - s.0, e.1 - s.1);
    let l2 = dx * dx + dy * dy;
    if l2 == 0.0 {
        return (px - s.0).hypot(py - s.1);
    }
    let t = (((px - s.0) * dx + (py - s.1) * dy) / l2).clamp(0.0, 1.0);
    (px - (s.0 + t * dx)).hypot(py - (s.1 + t * dy))
}

fn proj_t(p: (f64, f64), s: (f64, f64), e: (f64, f64)) -> f64 {
    let (dx, dy) = (e.0 - s.0, e.1 - s.1);
    let l2 = dx * dx + dy * dy;
    if l2 == 0.0 {
        return 0.0;
    }
    ((p.0 - s.0) * dx + (p.1 - s.1) * dy) / l2
}

fn collect(board: &Board) -> Vec<(usize, u32, String, String, (f64, f64), (f64, f64), f64)> {
    let names: BTreeMap<u32, String> = board.nets.iter().map(|n| (n.id, n.name.clone())).collect();
    let mut out = Vec::new();
    for (idx, s) in board.segments.iter().enumerate() {
        if (s.start.0 - s.end.0).abs() < 1e-9 && (s.start.1 - s.end.1).abs() < 1e-9 {
            continue;
        }
        out.push((
            idx,
            s.net,
            names.get(&s.net).cloned().unwrap_or_default(),
            s.layer.clone(),
            s.start,
            s.end,
            s.width,
        ));
    }
    out
}

fn pad_world_points(board: &Board) -> Vec<(f64, f64)> {
    let mut pts = Vec::new();
    for fp in &board.footprints {
        for pad in &fp.pads {
            if pad.net.unwrap_or(0) == 0 {
                continue;
            }
            let (ox, oy) = fp.pad_rotated_offset(pad);
            pts.push((fp.position.0 + ox, fp.position.1 + oy));
        }
    }
    pts
}

/// Scan junction angles and degenerate collinear-overlap segments.
pub fn audit_angles(board: &Board) -> AngleReport {
    let segs = collect(board);
    let names: BTreeMap<u32, String> = board.nets.iter().map(|n| (n.id, n.name.clone())).collect();
    let mut report = AngleReport::default();
    let key = |p: (f64, f64)| ((p.0 * 1000.0).round() as i64, (p.1 * 1000.0).round() as i64);
    let mut ep: BTreeMap<(i64, i64), Vec<usize>> = BTreeMap::new();
    for (i, (_, _, _, _, p1, p2, _)) in segs.iter().enumerate() {
        ep.entry(key(*p1)).or_default().push(i);
        ep.entry(key(*p2)).or_default().push(i);
    }
    let mut seen: std::collections::HashSet<(usize, usize)> = std::collections::HashSet::new();
    for idxs in ep.values() {
        for x in 0..idxs.len() {
            for y in x + 1..idxs.len() {
                let (ia, ib) = (idxs[x], idxs[y]);
                if !seen.insert((ia.min(ib), ia.max(ib))) {
                    continue;
                }
                let (ra, rb) = (&segs[ia], &segs[ib]);
                let anet = ra.1;
                let alayer = ra.3.clone();
                if ra.1 != rb.1 || ra.3 != rb.3 {
                    continue;
                }
                let (ap1, ap2, bp1, bp2) = (ra.4, ra.5, rb.4, rb.5);
                let shared = if dist2(ap1, bp1) < 4e-4 || dist2(ap1, bp2) < 4e-4 {
                    ap1
                } else {
                    ap2
                };
                let oa = if dist2(ap1, shared) < 4e-4 { ap2 } else { ap1 };
                let ob = if dist2(bp1, shared) < 4e-4 { bp2 } else { bp1 };
                let v = vertex_angle(oa, shared, ob);
                if v < 1.0 {
                    if collinear_overlap(oa, shared, ap1, ap2, bp1, bp2) {
                        report.overlaps.push(AngleIssue {
                            angle_deg: v,
                            net: names.get(&anet).cloned().unwrap_or_default(),
                            layer: alayer,
                            at: shared,
                            kind: "degenerate_overlap",
                        });
                    }
                } else if v < 50.0 {
                    report.acute.push(AngleIssue {
                        angle_deg: v,
                        net: names.get(&anet).cloned().unwrap_or_default(),
                        layer: alayer,
                        at: shared,
                        kind: "acute",
                    });
                } else if (89.0..91.0).contains(&v) {
                    report.right_angles += 1;
                }
            }
        }
    }
    report.miter_candidates = miter_candidates(board).len();
    report
}

fn collinear_overlap(
    oa: (f64, f64),
    shared: (f64, f64),
    _ap1: (f64, f64),
    _ap2: (f64, f64),
    bp1: (f64, f64),
    bp2: (f64, f64),
) -> bool {
    let ux = (oa.0 - shared.0, oa.1 - shared.1);
    let ln = ux.0.hypot(ux.1);
    if ln < 1e-9 {
        return false;
    }
    let u = (ux.0 / ln, ux.1 / ln);
    let proj = |p: (f64, f64)| (p.0 - shared.0) * u.0 + (p.1 - shared.1) * u.1;
    let perp = |p: (f64, f64)| -(p.0 - shared.0) * u.1 + (p.1 - shared.1) * u.0;
    if perp(bp1).abs() > 0.05 || perp(bp2).abs() > 0.05 {
        return false;
    }
    let alo = proj(oa).min(0.0);
    let ahi = proj(oa).max(0.0);
    let (blo, bhi) = (proj(bp1).min(proj(bp2)), proj(bp1).max(proj(bp2)));
    blo < ahi - 0.05 && bhi > alo + 0.05
}

struct MiterOp {
    ia: usize,
    ib: usize,
    net: u32,
    layer: String,
    width: f64,
    pa: (f64, f64),
    pb: (f64, f64),
    shared: (f64, f64),
}

/// Find the next miterable corner (scan only, no mutation).
fn find_next_miter(
    segments: &[Segment],
    pads: &[(f64, f64)],
    vias: &[(f64, f64)],
    dcut_base: f64,
) -> Option<MiterOp> {
    let key = |p: (f64, f64)| ((p.0 * 1000.0).round() as i64, (p.1 * 1000.0).round() as i64);
    let mut segs: Vec<(usize, u32, String, (f64, f64), (f64, f64), f64)> = Vec::new();
    for (i, s) in segments.iter().enumerate() {
        if (s.start.0 - s.end.0).abs() < 1e-9 && (s.start.1 - s.end.1).abs() < 1e-9 {
            continue;
        }
        segs.push((i, s.net, s.layer.clone(), s.start, s.end, s.width));
    }
    let mut ep: BTreeMap<(i64, i64), Vec<usize>> = BTreeMap::new();
    for (i, (_, _, _, p1, p2, _)) in segs.iter().enumerate() {
        ep.entry(key(*p1)).or_default().push(i);
        ep.entry(key(*p2)).or_default().push(i);
    }
    for idxs in ep.values() {
        if idxs.len() != 2 {
            continue;
        }
        let (ia, ib) = (idxs[0], idxs[1]);
        let sa = &segs[ia];
        let sb = &segs[ib];
        let (_, anet, alayer, ap1, ap2, aw) = (sa.0, sa.1, sa.2.clone(), sa.3, sa.4, sa.5);
        let (_, bnet, blayer, bp1, bp2, bw) = (sb.0, sb.1, sb.2.clone(), sb.3, sb.4, sb.5);
        if anet != bnet || alayer != blayer {
            continue;
        }
        let shared = if key(ap1) == key(bp1) || key(ap1) == key(bp2) {
            ap1
        } else {
            ap2
        };
        let oa = if dist2(ap1, shared) < 4e-4 { ap2 } else { ap1 };
        let ob = if dist2(bp1, shared) < 4e-4 { bp2 } else { bp1 };
        let axis = |p: (f64, f64), q: (f64, f64)| {
            if (p.0 - q.0).abs() < 1e-9 {
                'V'
            } else if (p.1 - q.1).abs() < 1e-9 {
                'H'
            } else {
                '?'
            }
        };
        let (aa, ab) = (axis(shared, oa), axis(shared, ob));
        if aa == '?' || ab == '?' || aa == ab {
            continue;
        }
        if pads.iter().any(|p| dist2(*p, shared) < 0.45f64.powi(2)) {
            continue;
        }
        if vias.iter().any(|p| dist2(*p, shared) < 0.5f64.powi(2)) {
            continue;
        }
        let mut through = false;
        // third same-net same-layer segment passing through the corner point
        'chk: for (si, snet, slayer, sp1, sp2, _) in &segs {
            if *si == ia || *si == ib || *snet != anet || *slayer != alayer {
                continue;
            }
            if seg_dist_pt(shared.0, shared.1, *sp1, *sp2) < 0.05 {
                let t = proj_t(shared, *sp1, *sp2);
                if t > 0.02 && t < 0.98 {
                    through = true;
                    break 'chk;
                }
            }
        }
        if through {
            continue;
        }
        let la = (oa.0 - shared.0).hypot(oa.1 - shared.1);
        let lb = (ob.0 - shared.0).hypot(ob.1 - shared.1);
        let dcut = dcut_base.max(aw.max(bw) * 0.9);
        if la < dcut * 2.0 + 0.1 || lb < dcut * 2.0 + 0.1 {
            continue;
        }
        let ua = ((oa.0 - shared.0) / la, (oa.1 - shared.1) / la);
        let ub = ((ob.0 - shared.0) / lb, (ob.1 - shared.1) / lb);
        return Some(MiterOp {
            ia,
            ib,
            net: anet,
            layer: alayer,
            width: aw.max(bw),
            pa: (shared.0 + ua.0 * dcut, shared.1 + ua.1 * dcut),
            pb: (shared.0 + ub.0 * dcut, shared.1 + ub.1 * dcut),
            shared,
        });
    }
    None
}

#[allow(dead_code)] // 预留守卫钩子
fn si_placeholder_guard() -> usize {
    usize::MAX
}

/// Count eligible corners without mutating.
pub fn miter_candidates(board: &Board) -> Vec<(u32, (f64, f64))> {
    let pads = pad_world_points(board);
    let vias: Vec<(f64, f64)> = board.vias.iter().map(|v| v.at).collect();
    let mut probe = board.clone();
    let mut out = Vec::new();
    for _ in 0..500 {
        match find_next_miter(&probe.segments, &pads, &vias, 0.4) {
            Some(op) => {
                out.push((op.net, op.shared));
                apply_miter(&mut probe, &op);
            }
            None => break,
        }
    }
    out
}

fn apply_miter(board: &mut Board, op: &MiterOp) {
    let sa = &mut board.segments[op.ia];
    if dist2(sa.start, op.shared) < 4e-4 {
        sa.start = op.pa;
    } else {
        sa.end = op.pa;
    }
    let sb = &mut board.segments[op.ib];
    if dist2(sb.start, op.shared) < 4e-4 {
        sb.start = op.pb;
    } else {
        sb.end = op.pb;
    }
    board.segments.push(Segment {
        start: op.pa,
        end: op.pb,
        width: op.width,
        layer: op.layer.clone(),
        net: op.net,
    });
}

/// Miter all eligible corners; returns applied count.
pub fn miter_corners(board: &mut Board, dcut_base: f64) -> usize {
    let pads = pad_world_points(board);
    let vias: Vec<(f64, f64)> = board.vias.iter().map(|v| v.at).collect();
    let mut n = 0;
    for _ in 0..500 {
        match find_next_miter(&board.segments, &pads, &vias, dcut_base) {
            Some(op) => {
                apply_miter(board, &op);
                n += 1;
            }
            None => break,
        }
    }
    n
}

/// Fix pass: remove fully-contained degenerate overlaps + miter corners.
pub fn fix_angles(board: &mut Board) -> (usize, usize) {
    let mut removed = 0usize;
    loop {
        let segs = collect(board);
        let mut victim: Option<usize> = None;
        'outer: for i in 0..segs.len() {
            for j in 0..segs.len() {
                if i == j {
                    continue;
                }
                let (_ai, anet, ap1, ap2) = (&segs[i].0, segs[i].1, segs[i].4, segs[i].5);
                let (bi, bnet, bp1, bp2) = (&segs[j].0, segs[j].1, segs[j].4, segs[j].5);
                if anet != bnet || segs[i].3 != segs[j].3 {
                    continue;
                }
                let ux = (ap2.0 - ap1.0, ap2.1 - ap1.1);
                let ln = ux.0.hypot(ux.1);
                if ln < 1e-9 {
                    continue;
                }
                let u = (ux.0 / ln, ux.1 / ln);
                let perp = |p: (f64, f64)| -(p.0 - ap1.0) * u.1 + (p.1 - ap1.1) * u.0;
                if perp(bp1).abs() > 0.05 || perp(bp2).abs() > 0.05 {
                    continue;
                }
                let proj = |p: (f64, f64)| (p.0 - ap1.0) * u.0 + (p.1 - ap1.1) * u.1;
                let (alo, ahi) = (proj(ap1), proj(ap2));
                let (lo, hi) = (alo.min(ahi), alo.max(ahi));
                let (blo, bhi) = (proj(bp1).min(proj(bp2)), proj(bp1).max(proj(bp2)));
                if blo >= lo - 0.05 && bhi <= hi + 0.05 && (bhi - blo) < (hi - lo) {
                    victim = Some(*bi);
                    break 'outer;
                }
            }
        }
        match victim {
            Some(i) => {
                board.segments.remove(i);
                removed += 1;
            }
            None => break,
        }
    }
    let mitered = miter_corners(board, 0.4);
    (removed, mitered)
}

// ---------------------------------------------------------------------------
// P1-3: via audit
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ViaInfo {
    pub net: String,
    pub at: (f64, f64),
    pub size: f64,
    pub class: &'static str,
    pub f_segs: usize,
    pub b_segs: usize,
    pub f_pads: usize,
    pub in_zone: bool,
}

fn point_in_polys(p: (f64, f64), polys: &[Vec<(f64, f64)>]) -> bool {
    for pts in polys {
        let n = pts.len();
        if n < 3 {
            continue;
        }
        let mut inside = false;
        let mut j = n - 1;
        for i in 0..n {
            let (xi, yi) = pts[i];
            let (xj, yj) = pts[j];
            if (yi > p.1) != (yj > p.1) && p.0 < (xj - xi) * (p.1 - yi) / (yj - yi) + xi {
                inside = !inside;
            }
            j = i;
        }
        if inside {
            return true;
        }
    }
    false
}

/// Classify every via by its layer-side connectivity:
/// - `gnd_tie`: ties an SMD pad to the plane (B side is the pour) — required
/// - `signal`:  copper on both sides — a real layer transition
/// - `stitch`:  connects to the plane only — optional stitching
/// - `floating`: touches nothing — deletable
pub fn audit_vias(board: &Board) -> Vec<ViaInfo> {
    let names: BTreeMap<u32, String> = board.nets.iter().map(|n| (n.id, n.name.clone())).collect();
    let zone_polys: Vec<Vec<(f64, f64)>> = board
        .zones
        .iter()
        .flat_map(|z| {
            z.filled_polygons
                .iter()
                .map(|f| f.points.clone())
                .collect::<Vec<_>>()
        })
        .collect();
    let mut out = Vec::new();
    for (vi, v) in board.vias.iter().enumerate() {
        let _ = vi;
        let net = names.get(&v.net).cloned().unwrap_or_default();
        let mut f_segs = 0usize;
        let mut b_segs = 0usize;
        for s in &board.segments {
            if s.net != v.net {
                continue;
            }
            let d = seg_dist_pt(v.at.0, v.at.1, s.start, s.end);
            if d <= s.width / 2.0 + v.size / 2.0 + 0.01 {
                if s.layer.starts_with('F') {
                    f_segs += 1;
                } else {
                    b_segs += 1;
                }
            }
        }
        let mut f_pads = 0usize;
        for fp in &board.footprints {
            for pad in &fp.pads {
                if pad.net != Some(v.net) {
                    continue;
                }
                let th = pad.layers.iter().any(|l| l.contains("B.Cu"));
                if th {
                    continue; // TH pads exist on both layers, no tie needed
                }
                let (ox, oy) = fp.pad_rotated_offset(pad);
                let (px, py) = (fp.position.0 + ox, fp.position.1 + oy);
                if (px - v.at.0).hypot(py - v.at.1)
                    <= pad.size.0.max(pad.size.1) / 2.0 + v.size / 2.0 + 0.01
                {
                    f_pads += 1;
                }
            }
        }
        let in_zone = point_in_polys(v.at, &zone_polys);
        let class = if f_segs > 0 || f_pads > 0 {
            if b_segs > 0 {
                "signal"
            } else if in_zone {
                if f_pads > 0 {
                    "gnd_tie"
                } else {
                    "stitch"
                }
            } else {
                "floating"
            }
        } else if in_zone || b_segs > 0 {
            "stitch"
        } else {
            "floating"
        };
        out.push(ViaInfo {
            net,
            at: v.at,
            size: v.size,
            class,
            f_segs,
            b_segs,
            f_pads,
            in_zone,
        });
    }
    out
}
