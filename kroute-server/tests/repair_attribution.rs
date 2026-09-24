//! Task1 修订版：NO-PATH 封锁归因（CPU 全空间扩展）测试。
//! 板级链路：kicad-cdb mini 板 → export_wavefront_grid_repair → attribute_blockades，
//! 网身份来自编码里带 net id 的铜皮格（Pad/Trace/Via）。

use kroute_server::wavefront::{attribute_blockades, cell_passable, GridSpec};

#[test]
fn test_attribution_reports_blockade_cluster_net_ids() {
    // 7x7 单层：围墙（Blocked=1）围住笼内起点 (2,2)，顶墙中间嵌一格异网 Trace(2)
    let (cols, rows) = (7usize, 7usize);
    let mut grid = vec![0u32; cols * rows];
    let wall = [
        (1usize, 1usize),
        (2, 1),
        (3, 1),
        (4, 1),
        (5, 1),
        (1, 2),
        (1, 3),
        (1, 4),
        (1, 5),
        (5, 2),
        (5, 3),
        (5, 4),
        (5, 5),
        (2, 5),
        (3, 5),
        (4, 5),
    ];
    for &(c, r) in &wall {
        grid[r * cols + c] = 1;
    }
    grid[cols + 3] = 1_000_002; // 顶墙中间的异网铜皮 → 归因应报 net 2

    let spec = GridSpec {
        cols,
        rows,
        layers: 1,
        via_cost: 3.0,
    };
    // 笼内 (2,2) 与笼外 (0,6) 的格都应可通行（墙把它们隔开，而非格子本身占死）
    let pass = |i: usize| cell_passable(grid[i], 9, true);
    assert!(pass(2 * cols + 2) && pass(0), "笼内外端点应为 Free");

    let clusters = attribute_blockades(&grid, spec, (0, 2, 2), (0, 6, 0), 9, true, 5);
    assert!(!clusters.is_empty(), "四面封死 → 归因簇非空");
    let top = &clusters[0];
    assert_eq!(top.net_ids, vec![2], "簇网身份 = 围墙上的异网铜皮");
    // 簇 bbox 覆盖围墙
    assert_eq!(
        (top.min_row, top.min_col, top.max_row, top.max_col),
        (1, 1, 5, 5)
    );
}

#[test]
fn test_attribution_empty_when_routable() {
    // 无墙：归因为空
    let (cols, rows) = (5usize, 5usize);
    let grid = vec![0u32; cols * rows];
    let spec = GridSpec {
        cols,
        rows,
        layers: 1,
        via_cost: 3.0,
    };
    let clusters = attribute_blockades(&grid, spec, (0, 0, 0), (0, 4, 4), 1, true, 5);
    assert!(clusters.is_empty(), "可达时无归因簇");
}

#[test]
fn test_attribution_board_level_miniboard() {
    // 板级链路：一圈 OTHER 网走线围住 from → repair 导出 → 归因 net_ids = [OTHER 的 net id]
    use kicad_cdb::layer_config::BoardLayerConfig;
    use kicad_json5::ir::board::{
        Board, BoardGraphic, BoardGraphicKind, Footprint, NetDef, Pad, Segment, Via,
    };

    let mut b = Board::new();
    b.nets = vec![
        NetDef {
            id: 0,
            name: String::new(),
        },
        NetDef {
            id: 1,
            name: "SIG".into(),
        },
        NetDef {
            id: 2,
            name: "OTHER".into(),
        },
    ];
    b.graphics.push(BoardGraphic {
        kind: BoardGraphicKind::Rect {
            start: (0.0, 0.0),
            end: (30.0, 30.0),
        },
        layer: "Edge.Cuts".into(),
        stroke_width: 0.05,
        fill: false,
    });
    let mk_pad = |at: (f64, f64), net: Option<u32>| Pad {
        number: "1".into(),
        pad_type: kicad_json5::ir::board::PadType::Smd,
        shape: kicad_json5::ir::board::PadShape::Rect,
        position: (at.0, at.1, 0.0),
        size: (0.5, 0.5),
        layers: vec!["F.Cu".into()],
        drill: None,
        net,
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
    };
    // SIG 目标网：from pad（笼内）。大 offset 让 pad 脱离 footprint body
    // （fallback body 4x4mm 以 fp 为中心 [8.9,12.9]，pad 世界坐标 = (10.9-2.9, 8) = (8,8)）
    let mut fp_sig = Footprint::new("Test:PAD", "TP1", "X");
    fp_sig.position = (10.9, 8.0, 0.0);
    fp_sig.pads.push(mk_pad((-2.9, 0.0), Some(1)));
    b.footprints.push(fp_sig);
    // OTHER 围墙：矩形走线圈住 (8,8)
    let walls = [
        ((5.0, 5.0), (11.0, 5.0)),
        ((5.0, 11.0), (11.0, 11.0)),
        ((5.0, 5.0), (5.0, 11.0)),
        ((11.0, 5.0), (11.0, 11.0)),
    ];
    for (s, e) in walls {
        b.segments.push(Segment {
            start: s,
            end: e,
            width: 0.3,
            layer: "F.Cu".into(),
            net: 2,
        });
    }
    // OTHER 的 via 贴在围墙上（带网身份的孔）
    b.vias.push(Via {
        at: (8.0, 5.0),
        size: 0.6,
        drill: 0.3,
        layers: vec!["F.Cu".into(), "B.Cu".into()],
        net: 2,
    });

    let e = kicad_cdb::router::export_wavefront_grid_repair(
        &b,
        BoardLayerConfig::two_layer(),
        0.25,
        0,
        "SIG",
        (8.0, 8.0),
        (25.0, 25.0),
        0.0,
    )
    .expect("repair export");
    assert_eq!(e.nets.len(), 1);

    let spec = GridSpec {
        cols: e.cols,
        rows: e.rows,
        layers: 1,
        via_cost: 3.0,
    };
    // layers=1：单层修复场景（F.Cu 死锁）——两层时 goal 可经 B.Cu 绕行（那是合法修复路径，非封锁）
    let clusters = attribute_blockades(
        &e.grid_u32,
        spec,
        (0, e.nets[0].start.1, e.nets[0].start.0),
        (0, e.nets[0].goal.1, e.nets[0].goal.0),
        1,
        true,
        5,
    );
    assert!(!clusters.is_empty(), "围墙封死 → 板级归因簇非空");
    assert!(
        clusters.iter().any(|c| c.net_ids.contains(&2)),
        "归因网身份应包含围墙网 OTHER(2)，实际 {:?}",
        clusters
            .iter()
            .map(|c| c.net_ids.clone())
            .collect::<Vec<_>>()
    );
}

#[test]
fn test_attribution_hole_wall_cluster_identity_via_pad() {
    // 孔墙本身编码为 Blocked（无身份），但孔墙包着带身份的铜（PTH pad = Pad(net)）；
    // 归因簇应把铜身份报出来。一堵贯穿全行的墙 + 墙心 Pad(4)：
    let (cols, rows) = (9usize, 9usize);
    let mut grid = vec![0u32; cols * rows];
    for c in 0..cols {
        grid[4 * cols + c] = 1;
    }
    grid[4 * cols + 4] = 2 + 4; // 墙心的异网 PTH pad 铜盘 Pad(4)
    let spec = GridSpec {
        cols,
        rows,
        layers: 1,
        via_cost: 3.0,
    };
    // 上半区 → 下半区：必穿墙
    let clusters = attribute_blockades(&grid, spec, (0, 1, 1), (0, 7, 1), 1, true, 5);
    assert!(!clusters.is_empty());
    assert!(
        clusters[0].net_ids.contains(&4),
        "孔墙簇应携带孔主人（pad 铜）身份"
    );
}
