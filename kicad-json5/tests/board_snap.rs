//! `Board::snap_trace_junctions` 回归测试: freerouting 回导在走线交汇点留下准零长度碎片
//! (0.0001–0.01 mm); A* 栅格布线器留下"两端点差一点却不相等"的接缝. 断言: 吸附后 (1) 没有 < tol 的段、
//! 没有距离 < tol 的同网端点; (2) 交汇处端点精确重合 (锚点: 过孔圆心 > 焊盘中心 > 其余);
//! (3) 连通关系不变 —— 同网照旧相连, 异网绝不合并; (4) 原本在 45° 网格上的线不被弄歪;
//! (5) 幂等; (6) 没有接缝的板一个坐标都不动.
//! SES 解析在 pcbnew (C++) 那一侧, 这里构造等价的 IR 中间结构; 坐标取自实板.

use kicad_json5::ir::board::{Board, Footprint, Pad, Segment, Via};

fn seg(sx: f64, sy: f64, ex: f64, ey: f64, net: u32) -> Segment {
    Segment {
        start: (sx, sy),
        end: (ex, ey),
        width: 0.2,
        layer: "F.Cu".into(),
        net,
    }
}

fn via(x: f64, y: f64, net: u32) -> Via {
    Via {
        at: (x, y),
        size: 0.6,
        drill: 0.4,
        layers: vec!["F.Cu".into(), "B.Cu".into()],
        net,
        via_type: String::new(),
    }
}

fn fp_with_pad(lib: &str, reference: &str, fp_pos: (f64, f64, f64), pad: Pad) -> Footprint {
    Footprint {
        lib_id: lib.into(),
        reference: reference.into(),
        position: fp_pos,
        layer: "F.Cu".into(),
        pads: vec![pad],
        ..Default::default()
    }
}

fn pad(number: &str, position: (f64, f64, f64), size: (f64, f64), net: u32) -> Pad {
    Pad {
        number: number.into(),
        position,
        size,
        layers: vec!["F.Cu".into()],
        net: Some(net),
        ..Default::default()
    }
}

fn dist(a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

/// 长度在 (0, tol) 之间的段数.
fn frags(board: &Board, tol: f64) -> usize {
    board
        .segments
        .iter()
        .filter(|s| {
            let d = dist(s.start, s.end);
            d > 0.0 && d < tol
        })
        .count()
}

/// 同网、距离在 (0, tol) 之间的线端点对数.
fn seams(board: &Board, tol: f64) -> usize {
    let pts: Vec<(u32, (f64, f64))> = board
        .segments
        .iter()
        .flat_map(|s| [(s.net, s.start), (s.net, s.end)])
        .collect();
    let mut n = 0;
    for i in 0..pts.len() {
        for j in (i + 1)..pts.len() {
            if pts[i].0 == pts[j].0 {
                let d = dist(pts[i].1, pts[j].1);
                if d > 0.0 && d < tol {
                    n += 1;
                }
            }
        }
    }
    n
}

/// 只带走线的板.
fn board_with(segments: Vec<Segment>) -> Board {
    Board {
        segments,
        ..Default::default()
    }
}

/// 偏离 45° 网格多少度.
fn off_grid(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    if dx.hypot(dy) < 1e-9 {
        return 0.0;
    }
    let ang = dy.atan2(dx).to_degrees().rem_euclid(180.0);
    (ang - (ang / 45.0).round() * 45.0).abs()
}

#[test]
fn fragment_between_two_traces_is_snapped_and_removed() {
    // 实板样本 (146.3,108.438)->(146.3,108.4375): 两根竖直长线之间夹一段 0.0005 mm 碎片.
    let mut board = board_with(vec![
        seg(146.3, 108.4, 146.3, 108.438, 1),
        seg(146.3, 108.438, 146.3, 108.4375, 1), // 碎片
        seg(146.3, 108.4375, 146.3, 109.0, 1),
    ]);
    let rep = board.snap_trace_junctions(0.01);
    assert_eq!(rep.segments_removed, 1);
    assert_eq!(board.segments.len(), 2);
    assert_eq!(frags(&board, 0.01), 0);
    assert_eq!(seams(&board, 0.01), 0);
    // 连通不变: 两根长线的端点精确重合 (并到其中之一), 而且都还是竖线
    assert_eq!(board.segments[0].end, board.segments[1].start);
    assert_eq!(board.segments[0].end.0, 146.3);
    assert!(off_grid(board.segments[0].start, board.segments[0].end) < 1e-9);
    assert_eq!(
        off_grid(board.segments[1].start, board.segments[1].end),
        0.0
    );
}

#[test]
fn via_centre_wins_as_anchor() {
    let mut board = board_with(vec![
        seg(0.0, 0.0, 5.0, 0.0, 2),
        seg(5.0, 0.0, 5.0005, 0.0, 2), // 碎片
        seg(5.0005, 0.0, 9.0, 0.0, 2),
    ]);
    board.vias = vec![via(5.0002, 0.0, 2)];
    let rep = board.snap_trace_junctions(0.01);
    assert_eq!(rep.segments_removed, 1);
    // 过孔不动, 交汇处两端点都并到过孔圆心
    assert_eq!(board.vias[0].at, (5.0002, 0.0));
    assert_eq!(board.segments[0].end, (5.0002, 0.0));
    assert_eq!(board.segments[1].start, (5.0002, 0.0));
    assert_eq!(frags(&board, 0.01), 0);
}

#[test]
fn seam_without_fragment_is_closed_and_every_trace_stays_exact() {
    // 实板样本: 没有碎片段, 两根线的端点差 0.0025 (焊盘中心在 1/8 mm 栅格上 12.9375, 走线端点在
    // 0.01 mm 栅格上 12.94). 锚点取"挪完之后不精确的线偏差总和最小"的候选 (12.94, 23.95): 竖线原样,
    // 45° 线的近端并过去之后整段平移, 仍然精确 45°.
    let mut board = board_with(vec![
        seg(12.94, 24.5, 12.94, 23.95, 3),
        seg(12.9375, 23.95, 12.1597, 24.7278, 3),
    ]);
    board.footprints = vec![fp_with_pad(
        "Lib:X",
        "Q1",
        (0.0, 0.0, 0.0),
        pad("3", (12.9375, 23.95, 0.0), (0.6, 1.0), 3),
    )];
    let rep = board.snap_trace_junctions(0.01);
    assert_eq!(rep.clusters, 1);
    assert_eq!(rep.leftover, 0);
    assert_eq!(board.segments[0].end, board.segments[1].start);
    assert_eq!(seams(&board, 0.01), 0);
    for s in &board.segments {
        assert!(off_grid(s.start, s.end) < 1e-9, "{:?}", s); // 浮点算出来是 1e-14 量级, 不能拿 == 0.0 比
    }
    // 竖线没动; 45° 线平移了 0.00125 (方向恢复 45°, 长度变了千分之几 —— 端点吸附本来就会这样)
    assert_eq!(board.segments[0].start, (12.94, 24.5));
    assert_eq!(board.segments[0].end, (12.94, 23.95));
    assert!(
        (dist(board.segments[1].start, board.segments[1].end)
            - dist((12.9375, 23.95), (12.1597, 24.7278)))
        .abs()
            < 0.01
    );
    assert_eq!(rep.straightened, 1);
}

#[test]
fn anchor_on_via_centre_slides_the_neighbouring_traces() {
    // 接缝附近挂着过孔. 锚点在 [竖线下端 (142.558,133.628), 水平长线右端 (142.555,133.63), 过孔圆心 (142.56,133.63)]
    // 里选: 三个候选都会让一条线不精确 (条数相同), 偏差总和最小的是竖线下端 (水平长线偏 0.02°, 平移一下就回来)
    // —— 所以选它, 而不是过孔圆心. 结果每条线都仍然精确.
    let mut board = board_with(vec![
        seg(142.558, 131.0, 142.558, 133.201, 4),   // 竖线 (上)
        seg(142.558, 133.201, 142.558, 133.628, 4), // 竖线 (下), 下端 (142.558, 133.628)
        seg(142.558, 133.201, 143.758, 133.201, 4), // 拐点向右的水平线
        seg(136.961, 133.63, 142.555, 133.63, 4),   // 背面水平长线, 右端 (142.555, 133.63)
    ]);
    board.vias = vec![via(142.56, 133.63, 4)];
    let rep = board.snap_trace_junctions(0.01);
    assert_eq!(rep.clusters, 1);
    assert_eq!(rep.leftover, 0);
    assert_eq!(seams(&board, 0.01), 0);
    for s in &board.segments {
        assert!(off_grid(s.start, s.end) < 1e-9, "{:?}", s); // 浮点算出来是 1e-14 量级, 不能拿 == 0.0 比
    }
    // 锚点是竖线下端 (142.558, 133.628): 水平长线的右端并到它 (x +0.003, y −0.002), 然后按原始方向整条平移 ——
    // 左端挪到 (142.558 − 5.594, 133.628) = (136.964, 133.628), 仍然水平; 竖线、拐点都没动; 过孔没动
    assert_eq!(board.segments[1].end, (142.558, 133.628));
    assert_eq!(board.segments[0].end, board.segments[1].start);
    assert_eq!(board.segments[2].start, board.segments[1].start);
    assert_eq!(board.segments[3].end, (142.558, 133.628));
    assert!((board.segments[3].start.0 - 136.964).abs() < 1e-9);
    assert_eq!(board.segments[3].start.1, 133.628);
    assert_eq!(board.vias[0].at, (142.56, 133.63));
    assert_eq!(rep.straightened, 1);
}

#[test]
fn line_end_to_via_centre_alone_is_not_a_seam() {
    // 线端落在过孔铜盘里 (差 0.005) 连接本来就可靠: 只有一个线端坐标时不并、不挪.
    let mut board = board_with(vec![
        seg(136.961, 133.628, 142.555, 133.628, 4),
        seg(142.555, 133.628, 142.555, 133.201, 4),
    ]);
    board.vias = vec![via(142.56, 133.63, 4)];
    let before = board.segments.clone();
    let rep = board.snap_trace_junctions(0.01);
    assert_eq!(rep, Default::default());
    for (a, b) in board.segments.iter().zip(before.iter()) {
        assert_eq!((a.start, a.end), (b.start, b.end));
    }
}

#[test]
fn slide_never_moves_an_endpoint_out_of_its_pad() {
    // 竖线的上端落在一片很窄的焊盘里 (半宽 0.003): 平移会把它挪出焊盘 → 整链回退, 线带着偏差留下.
    let mut board = board_with(vec![
        seg(5.004, 2.0, 5.004, 1.0, 5), // 上端在焊盘里
        seg(5.0, 1.0, 9.0, 1.0, 5),     // 以 (5.0, 1.0) 为端点的三条线
        seg(5.0, 1.0, 4.0, 0.0, 5),
        seg(5.0, 0.5, 5.0, 1.0, 5),
    ]);
    board.footprints = vec![fp_with_pad(
        "Lib:R",
        "R1",
        (5.004, 2.0, 0.0),
        pad("1", (0.0, 0.0, 0.0), (0.006, 0.2), 5),
    )];
    let rep = board.snap_trace_junctions(0.01);
    assert_eq!(rep.clusters, 1);
    assert_eq!(rep.leftover, 1);
    // 接缝并上了 (锚点是度数高的 (5.0, 1.0)), 焊盘里的端点没动
    assert_eq!(seams(&board, 0.01), 0);
    assert_eq!(board.segments[0].start, (5.004, 2.0));
    assert!(board.footprints[0].pad_contains(&board.footprints[0].pads[0], board.segments[0].start));
    assert!(off_grid(board.segments[0].start, board.segments[0].end) > 0.0);
}

#[test]
fn different_nets_are_never_merged() {
    let mut board = board_with(vec![
        seg(0.0, 0.0, 5.0, 5.0, 1),
        seg(5.001, 5.0, 6.0, 6.0, 2),
    ]);
    let before = board.segments.clone();
    let rep = board.snap_trace_junctions(0.01);
    assert_eq!(rep, Default::default());
    assert_eq!(board.segments.len(), 2);
    for (a, b) in board.segments.iter().zip(before.iter()) {
        assert_eq!((a.start, a.end), (b.start, b.end));
    }
}

#[test]
fn rotated_footprint_pad_contains_follows_the_real_orientation() {
    // 封装转 90°: 局部 (0.5, 0) 的焊盘, 世界坐标在 (fp.x, fp.y − 0.5) (KiCad 的屏幕坐标 y 向下).
    // 焊盘 1.0 × 0.4 (局部 x 方向 1.0): 转了 90° 之后世界方向是竖的.
    let fp = fp_with_pad(
        "Lib:U",
        "U1",
        (10.0, 20.0, 90.0),
        pad("1", (0.5, 0.0, 0.0), (1.0, 0.4), 7),
    );
    assert_eq!(fp.pad_world_center(&fp.pads[0]), (10.0, 19.5));
    assert!(fp.pad_contains(&fp.pads[0], (10.0, 19.1))); // 竖方向 ±0.5 之内
    assert!(fp.pad_contains(&fp.pads[0], (10.0, 19.9)));
    assert!(!fp.pad_contains(&fp.pads[0], (10.35, 19.5))); // 横方向 ±0.2 之外
    assert!(!fp.pad_contains(&fp.pads[0], (10.0, 18.9)));
}

#[test]
fn snap_is_idempotent_and_leaves_a_clean_board_untouched() {
    let mut board = board_with(vec![
        seg(0.0, 0.0, 5.0, 0.0, 2),
        seg(5.0, 0.0, 5.0005, 0.0, 2),
        seg(5.0005, 0.0, 9.0, 0.0, 2),
    ]);
    board.snap_trace_junctions(0.01);
    let once = board.segments.clone();
    let rep = board.snap_trace_junctions(0.01);
    assert_eq!(rep, Default::default());
    assert_eq!(board.segments.len(), once.len());
    for (a, b) in board.segments.iter().zip(once.iter()) {
        assert_eq!((a.start, a.end), (b.start, b.end));
    }
}

#[test]
fn zero_tolerance_is_a_no_op() {
    let mut board = board_with(vec![seg(0.0, 0.0, 0.0005, 0.0, 1)]);
    let before = board.segments.clone();
    assert_eq!(board.snap_trace_junctions(0.0), Default::default());
    assert_eq!(board.snap_trace_junctions(-1.0), Default::default());
    assert_eq!(board.segments.len(), before.len());
}

#[test]
fn slide_that_eats_a_neighbours_clearance_is_rolled_back() {
    // 实板回归: 挪 0.001 之后两条相邻线的间距 0.125 → 0.124. 长竖线 A 的下端并到锚点 (5.008, 1.0)
    // 之后要整条平移 0.008; 右边 0.15 处有一条异网竖线 B → 平移后间距 0.142 < 0.15 → 整链回退.
    let lines = || {
        vec![
            seg(5.0, 30.0, 5.0, 1.0, 1), // A (长线: 并到锚点后偏差角最小, 所以锚点在它的另一侧)
            seg(5.008, 1.0, 9.0, 1.0, 1),
            seg(5.008, 1.0, 8.0, 3.992, 1),
            seg(5.35, 30.0, 5.35, 0.5, 2), // B: 与 A 的铜缘间距正好 0.15
        ]
    };
    let mut board = board_with(lines());
    let rep = board.snap_trace_junctions_with_rules(0.01, Some(0.15));
    assert_eq!(rep.clusters, 1);
    assert_eq!(rep.leftover, 1);
    assert_eq!(seams(&board, 0.01), 0);
    // A 的上端没有动 (整链回退), A 带着偏差留下
    assert_eq!(board.segments[0].start, (5.0, 30.0));
    assert_eq!(board.segments[0].end, (5.008, 1.0));
    assert!(off_grid(board.segments[0].start, board.segments[0].end) > 0.0);
    assert_eq!(board.segments[3].start, (5.35, 30.0));

    // 规则放宽到 0.14: 同样的平移就能过
    let mut board2 = board_with(lines());
    let rep2 = board2.snap_trace_junctions_with_rules(0.01, Some(0.14));
    assert_eq!(rep2.straightened, 1);
    assert_eq!(rep2.leftover, 0);
    assert_eq!(board2.segments[0].start, (5.008, 30.0));
    assert_eq!(board2.segments[0].end, (5.008, 1.0));
}
