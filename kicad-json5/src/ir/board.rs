//! PCB Board Intermediate Representation
//!
//! Data structures for representing a printed circuit board, used for
//! generating `.kicad_pcb` files and PCB SVG rendering.

use std::collections::HashMap;

use crate::error::{Error, Result};
use crate::parser::ast::{Atom, SExpr};

// ---------------------------------------------------------------------------
// S-expression parsing helpers
// ---------------------------------------------------------------------------

/// Find the first child list whose head matches `tag`.
fn find_child<'a>(list: &'a [SExpr], tag: &str) -> Option<&'a [SExpr]> {
    list.iter().find_map(|item| {
        let children = item.as_list()?;
        children.first()?.as_ident().filter(|id| *id == tag)?;
        Some(children)
    })
}

/// Find all child lists whose head matches `tag`.
fn find_children<'a>(list: &'a [SExpr], tag: &str) -> Vec<&'a [SExpr]> {
    list.iter()
        .filter_map(|item| {
            let children = item.as_list()?;
            children.first()?.as_ident().filter(|id| *id == tag)?;
            Some(children)
        })
        .collect()
}

/// Parse `(at x y [angle])` into `(x, y, angle)`.
fn parse_at(items: &[SExpr]) -> (f64, f64, f64) {
    if items.len() >= 4 {
        (
            items[1].as_number().unwrap_or(0.0),
            items[2].as_number().unwrap_or(0.0),
            items[3].as_number().unwrap_or(0.0),
        )
    } else if items.len() >= 3 {
        (
            items[1].as_number().unwrap_or(0.0),
            items[2].as_number().unwrap_or(0.0),
            0.0,
        )
    } else {
        (0.0, 0.0, 0.0)
    }
}

/// Parse `(xy x y)` pairs from a `(pts ...)` list.
fn parse_xy_points(items: &[SExpr]) -> Vec<(f64, f64)> {
    items
        .iter()
        .skip(1)
        .filter_map(|item| {
            let pair = item.as_list()?;
            pair.first()?.as_ident().filter(|id| *id == "xy")?;
            Some((
                pair.get(1)?.as_number().unwrap_or(0.0),
                pair.get(2)?.as_number().unwrap_or(0.0),
            ))
        })
        .collect()
}

/// Parse `(start x y)` or `(end x y)` from items.
fn parse_point(items: &[SExpr], tag: &str) -> Option<(f64, f64)> {
    let child = find_child(items, tag)?;
    Some((
        child.get(1)?.as_number().unwrap_or(0.0),
        child.get(2)?.as_number().unwrap_or(0.0),
    ))
}

/// Parse stroke width from `(stroke (width W))`.
fn parse_stroke_width(items: &[SExpr]) -> f64 {
    find_child(items, "stroke")
        .and_then(|s| find_child(&s[1..], "width"))
        .and_then(|w| w.get(1)?.as_number())
        .unwrap_or(0.12)
}

/// Parse layer from `(layer "F.Cu")`.
fn parse_layer(items: &[SExpr]) -> String {
    find_child(items, "layer")
        .and_then(|l| l.get(1)?.as_string())
        .unwrap_or("F.Cu")
        .to_string()
}

/// Parse fill from `(fill solid)` or `(fill none)`. Returns true for solid/yes.
fn parse_fill(items: &[SExpr]) -> bool {
    find_child(items, "fill")
        .and_then(|f| f.get(1)?.as_string())
        .map(|s| s == "solid" || s == "yes")
        .unwrap_or(false)
}

/// 解析 model 子字段 `(tag (xyz X Y Z))` → (x, y, z)。
/// 数值嵌在内层 xyz 列表里 —— 旧实现直接把 tag 后第 1 个参数当数字读,
/// 恒为 None → 静默丢成默认值 (offset 漏读 bug)。容忍把数值平铺在
/// tag 下的旧口径 `(tag X Y Z)`; 内层 xyz 缺项按 0 补齐。
fn parse_xyz_child(items: &[SExpr], tag: &str) -> Option<(f64, f64, f64)> {
    let child = find_child(items, tag)?;
    if let Some(xyz) = child.get(1).and_then(|v| v.as_list()) {
        if xyz.first()?.as_ident()? == "xyz" {
            let n = |i: usize| xyz.get(i).and_then(|v| v.as_number()).unwrap_or(0.0);
            return Some((n(1), n(2), n(3)));
        }
        return None;
    }
    // 平铺数值口径: (tag X Y Z)
    let n = |i: usize| child.get(i).and_then(|v| v.as_number());
    Some((n(1)?, n(2)?, n(3)?))
}

// ---------------------------------------------------------------------------
// Top-level Board
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Board {
    pub version: String,
    pub generator: String,
    pub general: BoardGeneral,
    pub paper: String,
    pub title_block: Option<BoardTitleBlock>,
    pub layers: Vec<LayerDef>,
    pub setup: BoardSetup,
    pub nets: Vec<NetDef>,
    /// `(net_class ...)` declarations — the per-net rule plane (clearance, default
    /// widths, via params). Empty means "no explicit rules": kicad-cli then judges
    /// everything against its built-in defaults (0.2000 clearance / 0.2000 width),
    /// which is what all of our boards silently did until 2026-11-16.
    pub net_classes: Vec<NetClass>,
    /// DRC floors (min_track_width, min_clearance, …). KiCad keeps these in the
    /// **project** file (`.kicad_pro` → `board.design_settings.rules`), never in
    /// the board — see [`crate::pro`].
    pub rules: Option<DesignRules>,
    pub footprints: Vec<Footprint>,
    pub segments: Vec<Segment>,
    pub vias: Vec<Via>,
    pub zones: Vec<Zone>,
    pub graphics: Vec<BoardGraphic>,
}

impl Default for Board {
    fn default() -> Self {
        Self {
            version: "20240108".into(),
            generator: "component-db".into(),
            general: BoardGeneral::default(),
            paper: "A4".into(),
            title_block: None,
            layers: default_layers(),
            setup: BoardSetup::default(),
            nets: vec![NetDef {
                id: 0,
                name: String::new(),
            }],
            net_classes: Vec::new(),
            rules: None,
            footprints: Vec::new(),
            segments: Vec::new(),
            vias: Vec::new(),
            zones: Vec::new(),
            graphics: Vec::new(),
        }
    }
}

impl Board {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_net(&mut self, name: &str) -> u32 {
        let id = self.nets.len() as u32;
        self.nets.push(NetDef {
            id,
            name: name.into(),
        });
        id
    }

    pub fn find_or_add_net(&mut self, name: &str) -> u32 {
        if let Some(nd) = self.nets.iter().find(|n| n.name == name) {
            return nd.id;
        }
        self.add_net(name)
    }

    /// Parse a `.kicad_pcb` S-expression AST into Board IR.
    pub fn from_sexpr(sexpr: &SExpr) -> Result<Self> {
        // The toolchain dialect writes pads/segments/vias/zones as `(net "NAME")`
        // (name-only, no numeric id, no top-level net table). KiCad tolerates it,
        // but the numeric-net parsers below would silently drop every net.
        // Normalize name-only net refs into `(net ID "NAME")` on a mutable clone,
        // allocating ids for names not yet in the net table.
        let mut sexpr = sexpr.clone();
        let mut dialect_nets: Vec<NetDef> = Vec::new();
        // Seed the registry with the board's declared net table first: name-only
        // refs for already-declared nets must reuse their real ids, otherwise the
        // freshly allocated dialect ids (1..) collide with table ids and every
        // pad that used the name-only dialect is corrupted on write-back.
        if let Some(root_items) = sexpr.as_list() {
            for n in find_children(&root_items[1..], "net") {
                if let (Some(id), Some(name)) = (
                    n.get(1).and_then(|v| v.as_number()),
                    n.get(2).and_then(|v| v.as_string()),
                ) {
                    let id = id as u32;
                    if !dialect_nets.iter().any(|d| d.id == id) {
                        dialect_nets.push(NetDef {
                            id,
                            name: name.to_string(),
                        });
                    }
                }
            }
        }
        normalize_net_name_refs(&mut sexpr, &mut dialect_nets);

        let root = sexpr.as_list().ok_or_else(|| Error::ParserError {
            line: 0,
            column: 0,
            message: "expected (kicad_pcb ...)".into(),
        })?;
        if root.first().and_then(|h| h.as_ident()) != Some("kicad_pcb") {
            return Err(Error::ParserError {
                line: 0,
                column: 0,
                message: "root must be (kicad_pcb ...)".into(),
            });
        }

        let items = &root[1..];
        let mut board = Board::new();

        // version / generator
        if let Some(v) = find_child(items, "version") {
            board.version = v
                .get(1)
                .and_then(|n| n.as_number())
                .map(|n| format!("{}", n as u64))
                .unwrap_or_default();
        }
        if let Some(g) = find_child(items, "generator") {
            board.generator = g
                .get(1)
                .and_then(|s| s.as_string())
                .unwrap_or("")
                .to_string();
        }
        if let Some(gv) = find_child(items, "generator_version") {
            let _ = gv; // skip
        }

        // general
        if let Some(gen) = find_child(items, "general") {
            if let Some(t) = find_child(&gen[1..], "thickness") {
                board.general.thickness = t.get(1).and_then(|n| n.as_number()).unwrap_or(1.6);
            }
        }

        // paper — "(paper "A4")" or "(paper "User" 159.995 140.005)"
        if let Some(p) = find_child(items, "paper") {
            let mut paper_str = String::new();
            if let Some(s) = p.get(1).and_then(|n| n.as_string()) {
                paper_str.push_str(s);
            }
            // Check for "User w h" format
            if p.len() > 2 {
                for item in &p[2..] {
                    if let Some(n) = item.as_number() {
                        paper_str.push_str(&format!(" {}", n));
                    }
                }
            }
            if !paper_str.is_empty() {
                board.paper = paper_str;
            }
        }

        // title_block
        if let Some(tb) = find_child(items, "title_block") {
            let mut title_block = BoardTitleBlock::default();
            let tb_items = &tb[1..];
            if let Some(v) = find_child(tb_items, "title") {
                title_block.title = v.get(1).and_then(|s| s.as_string()).map(|s| s.to_string());
            }
            if let Some(v) = find_child(tb_items, "date") {
                title_block.date = v.get(1).and_then(|s| s.as_string()).map(|s| s.to_string());
            }
            if let Some(v) = find_child(tb_items, "rev") {
                title_block.rev = v.get(1).and_then(|s| s.as_string()).map(|s| s.to_string());
            }
            if let Some(v) = find_child(tb_items, "company") {
                title_block.company = v.get(1).and_then(|s| s.as_string()).map(|s| s.to_string());
            }
            for c in find_children(tb_items, "comment") {
                if c.len() >= 3 {
                    let idx = c[1].as_number().unwrap_or(0.0) as usize;
                    let text = c[2].as_string().unwrap_or("").to_string();
                    title_block.comment.push((idx, text));
                }
            }
            board.title_block = Some(title_block);
        }

        // layers
        if let Some(layers_node) = find_child(items, "layers") {
            board.layers.clear();
            for child in &layers_node[1..] {
                if let Some(layer_items) = child.as_list() {
                    if layer_items.len() >= 3 {
                        let ordinal = layer_items[0].as_number().unwrap_or(0.0) as u32;
                        let name = layer_items[1].as_string().unwrap_or("").to_string();
                        let layer_type = layer_items[2].as_ident().unwrap_or("user").to_string();
                        board.layers.push(LayerDef {
                            ordinal,
                            name,
                            layer_type,
                        });
                    }
                }
            }
        }

        // nets (top-level)
        let mut nets: Vec<NetDef> = Vec::new();
        for n in find_children(items, "net") {
            if n.len() >= 3 {
                let id = n[1].as_number().unwrap_or(0.0) as u32;
                let name = n[2].as_string().unwrap_or("").to_string();
                // KiCad 保留 netcode 0 = 未连接：命名网占用 id 0 会在装载/重存时被
                // KiCad 重排（实测 P30b: net4 GND 孔被标成 VBAT_SW），这里告警。
                if id == 0 && !name.is_empty() {
                    eprintln!(
                        "警告: nets 表命名网占用保留码 0 (name=\"{}\")，KiCad 装载后网名会错乱；请改用非零 id",
                        name
                    );
                }
                nets.push(NetDef { id, name });
            }
        }
        // KiCad always has net 0 (empty name) even if not explicit
        if !nets.iter().any(|n| n.id == 0) {
            nets.insert(
                0,
                NetDef {
                    id: 0,
                    name: String::new(),
                },
            );
        }
        // top-level table is authoritative; then add ids synthesized for
        // dialect name-only net refs
        board.nets = nets;
        for d in &dialect_nets {
            if !board.nets.iter().any(|n| n.id == d.id) {
                board.nets.push(d.clone());
            }
        }

        // setup / stackup. Missing legs here = silent loss on reverse compile
        // (this is exactly how the four v42 boards lost their stackup).
        if let Some(setup_node) = find_child(items, "setup") {
            let si = &setup_node[1..];
            if let Some(stackup) = find_child(si, "stackup") {
                for child in &stackup[1..] {
                    let Some(li) = child.as_list() else { continue };
                    if li.first().and_then(|h| h.as_ident()) != Some("layer") {
                        continue;
                    }
                    let name = li.get(1).and_then(|v| v.as_string()).unwrap_or("").to_string();
                    // `(type top)` (ident) and `(type "top")` (string) both occur
                    let layer_type = li
                        .get(2)
                        .and_then(|c| c.as_list())
                        .and_then(|c| c.get(1))
                        .map(|v| {
                            v.as_string()
                                .or_else(|| v.as_ident())
                                .unwrap_or("dielectric")
                                .to_string()
                        })
                        .unwrap_or_else(|| "dielectric".to_string());
                    let num = |tag: &str| {
                        find_child(&li[2..], tag).and_then(|c| c.get(1)).and_then(|v| v.as_number())
                    };
                    board.setup.stackup.push(StackupLayer {
                        name,
                        layer_type,
                        color: None,
                        thickness: num("thickness"),
                        material: find_child(&li[2..], "material")
                            .and_then(|c| c.get(1))
                            .and_then(|v| v.as_string())
                            .map(|s| s.to_string()),
                        epsilon_r: num("epsilon_r"),
                        loss_tangent: num("loss_tangent"),
                    });
                }
            }
            board.setup.pad_to_mask_clearance = find_child(si, "pad_to_mask_clearance")
                .and_then(|c| c.get(1))
                .and_then(|v| v.as_number())
                .unwrap_or(0.0);
            board.setup.pad_to_paste_clearance = find_child(si, "pad_to_paste_clearance")
                .and_then(|c| c.get(1))
                .and_then(|v| v.as_number())
                .unwrap_or(0.0);
            board.setup.pad_to_paste_clearance_ratio = find_child(si, "pad_to_paste_clearance_ratio")
                .and_then(|c| c.get(1))
                .and_then(|v| v.as_number())
                .unwrap_or(0.0);
            let origin = |tag: &str| {
                find_child(si, tag)
                    .and_then(|c| c.get(1))
                    .and_then(|v| v.as_number())
                    .zip(find_child(si, tag).and_then(|c| c.get(2)).and_then(|v| v.as_number()))
            };
            board.setup.aux_axis_origin = origin("aux_axis_origin");
            board.setup.grid_origin = origin("grid_origin");
        }

        // net_class — the per-net rule plane. Silently dropping it (as this
        // parser used to) means a reverse compile erases the board's clearance
        // and via rules, same failure class as the via_type incident.
        for n in find_children(items, "net_class") {
            let Some(name) = n.get(1).and_then(|v| v.as_string().or_else(|| v.as_ident())) else {
                continue;
            };
            let mut nc = NetClass {
                name: name.to_string(),
                ..Default::default()
            };
            if let Some(desc) = n.get(2).and_then(|v| v.as_string()) {
                nc.description = desc.to_string();
            }
            let body = &n[2..];
            let num = |tag: &str| {
                find_child(body, tag)
                    .and_then(|c| c.get(1))
                    .and_then(|v| v.as_number())
            };
            nc.clearance = num("clearance");
            nc.trace_width = num("trace_width");
            nc.via_dia = num("via_dia");
            nc.via_drill = num("via_drill");
            nc.uvia_dia = num("uvia_dia");
            nc.uvia_drill = num("uvia_drill");
            nc.diff_pair_width = num("diff_pair_width");
            nc.diff_pair_gap = num("diff_pair_gap");
            for an in find_children(body, "add_net") {
                if let Some(m) = an.get(1).and_then(|v| v.as_string()) {
                    nc.nets.push(m.to_string());
                }
            }
            board.net_classes.push(nc);
        }

        // footprints
        for fp_node in find_children(items, "footprint") {
            if let Some(fp) = Self::parse_footprint(fp_node) {
                board.footprints.push(fp);
            }
        }

        // segments (traces)
        for seg_node in find_children(items, "segment") {
            let start = parse_point(&seg_node[1..], "start").unwrap_or((0.0, 0.0));
            let end = parse_point(&seg_node[1..], "end").unwrap_or((0.0, 0.0));
            let width = find_child(&seg_node[1..], "width")
                .and_then(|w| w.get(1)?.as_number())
                .unwrap_or(0.25);
            let layer = parse_layer(&seg_node[1..]);
            let net = find_child(&seg_node[1..], "net")
                .and_then(|n| n.get(1)?.as_number())
                .unwrap_or(0.0) as u32;
            board.segments.push(Segment {
                start,
                end,
                width,
                layer,
                net,
            });
        }

        // vias
        for via_node in find_children(items, "via") {
            let at_items = find_child(&via_node[1..], "at");
            let at = at_items
                .map(|a| {
                    (
                        a.get(1).unwrap().as_number().unwrap_or(0.0),
                        a.get(2).unwrap().as_number().unwrap_or(0.0),
                    )
                })
                .unwrap_or((0.0, 0.0));
            let size = find_child(&via_node[1..], "size")
                .and_then(|s| s.get(1)?.as_number())
                .unwrap_or(0.6);
            let drill = find_child(&via_node[1..], "drill")
                .and_then(|d| d.get(1)?.as_number())
                .unwrap_or(0.3);
            let layers: Vec<String> = find_child(&via_node[1..], "layers")
                .map(|l| {
                    l[1..]
                        .iter()
                        .filter_map(|i| i.as_string().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            let net = find_child(&via_node[1..], "net")
                .and_then(|n| n.get(1)?.as_number())
                .unwrap_or(0.0) as u32;
            board.vias.push(Via {
                at,
                size,
                drill,
                layers,
                net,
                via_type: via_node
                    .get(1)
                    .and_then(|t| match t {
                        crate::parser::ast::SExpr::Atom(crate::parser::ast::Atom::Identifier(s)) => {
                            Some(s.as_str())
                        }
                        _ => None,
                    })
                    .filter(|s| *s == "blind" || *s == "micro")
                    .unwrap_or("")
                    .to_string(),
            });
        }

        // resolve pad display names from the net table (dialect/numeric both)
        for fp in &mut board.footprints {
            for pad in &mut fp.pads {
                if pad.net_name.is_none() {
                    if let Some(id) = pad.net {
                        if let Some(n) = board.nets.iter().find(|n| n.id == id) {
                            pad.net_name = Some(n.name.clone());
                        }
                    }
                }
            }
        }
        // 官方来源板可无声明表但 pad 行内带名（`(net 1 "GND")`）——反向归入
        // 声明表，保证写端 name-only 方言查表必中（否则裸 `(net 1)` 华秋拒载）
        for fp in &mut board.footprints {
            for pad in &mut fp.pads {
                if let Some(id) = pad.net {
                    if id == 0 {
                        continue;
                    }
                    match board.nets.iter_mut().find(|n| n.id == id) {
                        Some(n) => {
                            if n.name.is_empty() {
                                if let Some(nm) = &pad.net_name {
                                    if !nm.is_empty() {
                                        n.name = nm.clone();
                                    }
                                }
                            }
                        }
                        None => {
                            let name = pad.net_name.clone().unwrap_or_default();
                            board.nets.push(NetDef { id, name });
                        }
                    }
                }
            }
        }

        // zones (top-level and inside footprints are already handled above)
        for zone_node in find_children(items, "zone") {
            if let Some(mut zone) = Self::parse_zone(zone_node) {
                if zone.net_name.is_empty() {
                    if let Some(nm) = board
                        .nets
                        .iter()
                        .find(|n| n.id == zone.net)
                        .map(|n| n.name.clone())
                    {
                        zone.net_name = nm;
                    }
                }
                board.zones.push(zone);
            }
        }

        // board graphics (gr_line, gr_arc, gr_circle, gr_poly, gr_text)
        for child in items {
            let list = match child.as_list() {
                Some(l) => l,
                None => continue,
            };
            let tag = match list.first().and_then(|h| h.as_ident()) {
                Some(t) => t,
                None => continue,
            };
            match tag {
                "gr_line" => {
                    if let Some(g) = Self::parse_gr_line(&list[1..]) {
                        board.graphics.push(g);
                    }
                }
                "gr_arc" => {
                    if let Some(g) = Self::parse_gr_arc(&list[1..]) {
                        board.graphics.push(g);
                    }
                }
                "gr_circle" => {
                    if let Some(g) = Self::parse_gr_circle(&list[1..]) {
                        board.graphics.push(g);
                    }
                }
                "gr_rect" => {
                    if let Some(g) = Self::parse_gr_rect(&list[1..]) {
                        board.graphics.push(g);
                    }
                }
                "gr_poly" => {
                    if let Some(g) = Self::parse_gr_poly(&list[1..]) {
                        board.graphics.push(g);
                    }
                }
                "gr_text" => {
                    if let Some(g) = Self::parse_gr_text(&list[1..], list.get(1)) {
                        board.graphics.push(g);
                    }
                }
                _ => {}
            }
        }

        Ok(board)
    }

    fn parse_footprint(items: &[SExpr]) -> Option<Footprint> {
        let lib_id = items.get(1)?.as_string()?.to_string();
        let layer = parse_layer(&items[2..]);
        let position = find_child(&items[2..], "at")
            .map(parse_at)
            .unwrap_or((0.0, 0.0, 0.0));

        // UUID
        let uuid = find_child(&items[2..], "uuid")
            .and_then(|u| u.get(1).and_then(|v| v.as_string().map(|s| s.to_string())));

        // locked
        let locked = find_child(&items[2..], "locked")
            .map(|l| Self::parse_bool_value(l.get(1)))
            .unwrap_or(false);

        // descr, tags
        let descr = find_child(&items[2..], "descr")
            .and_then(|d| d.get(1).and_then(|v| v.as_string().map(|s| s.to_string())));
        let tags = find_child(&items[2..], "tags")
            .and_then(|t| t.get(1).and_then(|v| v.as_string().map(|s| s.to_string())));

        // path, sheetname, sheetfile
        let path = find_child(&items[2..], "path")
            .and_then(|p| p.get(1).and_then(|v| v.as_string().map(|s| s.to_string())));
        let sheetname = find_child(&items[2..], "sheetname")
            .and_then(|s| s.get(1).and_then(|v| v.as_string().map(|s| s.to_string())));
        let sheetfile = find_child(&items[2..], "sheetfile")
            .and_then(|s| s.get(1).and_then(|v| v.as_string().map(|s| s.to_string())));

        // attr (smd, thru_hole, virtual, board_only, or multi-value like exclude_from_pos_files...)
        let attr = find_child(&items[2..], "attr").and_then(|a| {
            if a.len() == 2 {
                match a.get(1)?.as_ident().unwrap_or("") {
                    "smd" => Some(FootprintAttr::Smd),
                    "thru_hole" => Some(FootprintAttr::ThruHole),
                    "through_hole" => Some(FootprintAttr::ThruHole),
                    "virtual" => Some(FootprintAttr::Virtual),
                    "board_only" => Some(FootprintAttr::BoardOnly),
                    _ => None,
                }
            } else if a.len() > 2 {
                let values: Vec<String> = a[1..]
                    .iter()
                    .filter_map(|v| v.as_ident().map(|s| s.to_string()))
                    .collect();
                if values.is_empty() {
                    None
                } else {
                    Some(FootprintAttr::Other(values))
                }
            } else {
                None
            }
        });

        // solder_mask_margin, solder_paste_margin, solder_paste_ratio, clearance
        let solder_mask_margin =
            find_child(&items[2..], "solder_mask_margin").and_then(|s| s.get(1)?.as_number());
        let solder_paste_margin =
            find_child(&items[2..], "solder_paste_margin").and_then(|s| s.get(1)?.as_number());
        let solder_paste_ratio =
            find_child(&items[2..], "solder_paste_ratio").and_then(|s| s.get(1)?.as_number());
        let clearance = find_child(&items[2..], "clearance").and_then(|c| c.get(1)?.as_number());
        let zone_connect = find_child(&items[2..], "zone_connect")
            .and_then(|z| z.get(1)?.as_number().map(|n| n as i32));
        let thermal_bridge_width =
            find_child(&items[2..], "thermal_bridge_width").and_then(|t| t.get(1)?.as_number());
        let thermal_bridge_angle =
            find_child(&items[2..], "thermal_bridge_angle").and_then(|t| t.get(1)?.as_number());
        let thermal_gap =
            find_child(&items[2..], "thermal_gap").and_then(|t| t.get(1)?.as_number());

        // models（可多个: KiCad 允许一个 footprint 引用多个 3D 模型）。
        // 子字段全部可缺省 (KiCad 允许只写 path), 缺省按 KiCad 默认:
        // offset (0,0,0) / scale (1,1,1) / rotate (0,0,0)。
        let models: Vec<Model3D> = find_children(&items[2..], "model")
            .iter()
            .filter_map(|m| {
                let path = m.get(1)?.as_string()?.to_string();
                Some(Model3D {
                    path,
                    offset: parse_xyz_child(&m[2..], "offset").unwrap_or((0.0, 0.0, 0.0)),
                    scale: parse_xyz_child(&m[2..], "scale").unwrap_or((1.0, 1.0, 1.0)),
                    rotate: parse_xyz_child(&m[2..], "rotate").unwrap_or((0.0, 0.0, 0.0)),
                })
            })
            .collect();

        // Extract Reference and Value from (property "Reference" "R1" ...)
        let mut reference = String::new();
        let mut value = String::new();
        let mut properties = HashMap::new();
        let mut properties_ext = Vec::new();
        for prop in find_children(&items[2..], "property") {
            if prop.len() >= 3 {
                let key = prop[1].as_string().unwrap_or("").to_string();
                let val = prop[2].as_string().unwrap_or("").to_string();
                match key.as_str() {
                    "Reference" => reference = val.clone(),
                    "Value" => value = val.clone(),
                    _ => {}
                }
                properties.insert(key.clone(), val.clone());

                // Parse full property with effects
                let prop_rest = &prop[3..];
                let prop_pos = find_child(prop_rest, "at")
                    .map(parse_at)
                    .unwrap_or((0.0, 0.0, 0.0));
                let prop_layer = parse_layer(prop_rest);
                // hide 位置双兼容: KiCad 直属子级 或 effects 内 (本项目历史板文本手术产物)
                let prop_hide = find_child(prop_rest, "hide")
                    .map(|h| Self::parse_bool_value(h.get(1)))
                    .unwrap_or(false)
                    || find_child(prop_rest, "effects")
                        .and_then(|e| find_child(&e[1..], "hide"))
                        .map(|h| Self::parse_bool_value(h.get(1)))
                        .unwrap_or(false);
                let prop_unlocked = find_child(prop_rest, "unlocked")
                    .map(|u| Self::parse_bool_value(u.get(1)))
                    .unwrap_or(false);
                let prop_uuid = find_child(prop_rest, "uuid")
                    .and_then(|u| u.get(1).and_then(|v| v.as_string().map(|s| s.to_string())));

                // Parse effects
                let effects = find_child(prop_rest, "effects")
                    .map(|e| {
                        let font = find_child(&e[1..], "font");
                        let (fs_w, fs_h, thickness, bold, italic) = if let Some(f) = font {
                            let size_node = find_child(&f[1..], "size");
                            let (sw, sh) = size_node
                                .map(|s| {
                                    (
                                        s.get(1).and_then(|v| v.as_number()).unwrap_or(1.0),
                                        s.get(2).and_then(|v| v.as_number()).unwrap_or(1.0),
                                    )
                                })
                                .unwrap_or((1.0, 1.0));
                            let thick = find_child(&f[1..], "thickness")
                                .and_then(|t| t.get(1)?.as_number())
                                .unwrap_or(0.15);
                            let is_bold = find_child(&f[1..], "bold")
                                .map(|b| Self::parse_bool_value(b.get(1)))
                                .unwrap_or(false);
                            let is_italic = find_child(&f[1..], "italic")
                                .map(|i| Self::parse_bool_value(i.get(1)))
                                .unwrap_or(false);
                            (sw, sh, thick, is_bold, is_italic)
                        } else {
                            (1.0, 1.0, 0.15, false, false)
                        };
                        let justify_mirror = find_child(&e[1..], "justify")
                            .map(|j| {
                                j[1..].iter().any(|i| {
                                    let s = i
                                        .as_string()
                                        .or_else(|| i.as_ident())
                                        .unwrap_or("");
                                    s == "mirror"
                                })
                            })
                            .unwrap_or(false);
                        FpPropertyEffects {
                            font_size: (fs_w, fs_h),
                            font_thickness: thickness,
                            bold,
                            italic,
                            justify_mirror,
                        }
                    })
                    .unwrap_or_default();

                properties_ext.push(FpProperty {
                    name: key,
                    value: val,
                    position: prop_pos,
                    layer: prop_layer,
                    hide: prop_hide,
                    unlocked: prop_unlocked,
                    effects,
                    uuid: prop_uuid,
                });
            }
        }

        // Fallback: extract from (fp_text reference "J1" ...) if property not found
        if reference.is_empty() || value.is_empty() {
            for txt_node in find_children(&items[2..], "fp_text") {
                if txt_node.len() >= 3 {
                    let text_type = txt_node[1].as_ident().unwrap_or("");
                    let text = txt_node[2].as_string().unwrap_or("").to_string();
                    match text_type {
                        "reference" if reference.is_empty() => reference = text,
                        "value" if value.is_empty() => value = text,
                        _ => {}
                    }
                }
            }
        }

        let mut fp = Footprint::new(&lib_id, &reference, &value);
        fp.position = position;
        fp.layer = layer;
        fp.uuid = uuid;
        fp.locked = locked;
        fp.descr = descr;
        fp.tags = tags;
        fp.path = path;
        fp.sheetname = sheetname;
        fp.sheetfile = sheetfile;
        fp.attr = attr;
        fp.solder_mask_margin = solder_mask_margin;
        fp.solder_paste_margin = solder_paste_margin;
        fp.solder_paste_ratio = solder_paste_ratio;
        fp.clearance = clearance;
        fp.zone_connect = zone_connect;
        fp.thermal_bridge_width = thermal_bridge_width;
        fp.thermal_bridge_angle = thermal_bridge_angle;
        fp.thermal_gap = thermal_gap;
        fp.models = models;
        fp.properties = properties;
        fp.properties_ext = properties_ext;

        // pads
        for pad_node in find_children(&items[2..], "pad") {
            if let Some(pad) = Self::parse_pad(pad_node) {
                fp.pads.push(pad);
            }
        }
        // KiCad semantics: pad `(at rot)` in a .kicad_pcb stores the **world**
        // angle (fp rotation + pad's own rotation — pad.cpp GetFPRelativeOrientation
        // returns pad_world − fp, and the lib-parity DRC compares that). IR keeps
        // the *relative* convention (json5 truth, .kicad_mod, cdb "fp rot + pad
        // local"); convert back here so emit (write_pad adds fp rotation) stays
        // exactly symmetric — sexpr→IR→sexpr idempotent.
        if fp.position.2 != 0.0 {
            for pad in &mut fp.pads {
                pad.position.2 -= fp.position.2;
            }
        }

        // fp_line
        for line_node in find_children(&items[2..], "fp_line") {
            let start = parse_point(&line_node[1..], "start").unwrap_or((0.0, 0.0));
            let end = parse_point(&line_node[1..], "end").unwrap_or((0.0, 0.0));
            let stroke_width = parse_stroke_width(&line_node[1..]);
            let layer = parse_layer(&line_node[1..]);
            fp.fp_lines.push(FpLine {
                start,
                end,
                stroke_width,
                layer,
            });
        }

        // fp_circle
        for circ_node in find_children(&items[2..], "fp_circle") {
            let center = parse_point(&circ_node[1..], "center").unwrap_or((0.0, 0.0));
            let end = parse_point(&circ_node[1..], "end").unwrap_or((0.0, 0.0));
            let stroke_width = parse_stroke_width(&circ_node[1..]);
            let layer = parse_layer(&circ_node[1..]);
            let fill = parse_fill(&circ_node[1..]);
            fp.fp_circles.push(FpCircle {
                center,
                end,
                stroke_width,
                layer,
                fill,
            });
        }

        // fp_arc
        for arc_node in find_children(&items[2..], "fp_arc") {
            let start = parse_point(&arc_node[1..], "start").unwrap_or((0.0, 0.0));
            let mid = parse_point(&arc_node[1..], "mid").unwrap_or((0.0, 0.0));
            let end = parse_point(&arc_node[1..], "end").unwrap_or((0.0, 0.0));
            let stroke_width = parse_stroke_width(&arc_node[1..]);
            let layer = parse_layer(&arc_node[1..]);
            fp.fp_arcs.push(FpArc {
                start,
                mid,
                end,
                stroke_width,
                layer,
            });
        }

        // fp_rect
        for rect_node in find_children(&items[2..], "fp_rect") {
            let start = parse_point(&rect_node[1..], "start").unwrap_or((0.0, 0.0));
            let end = parse_point(&rect_node[1..], "end").unwrap_or((0.0, 0.0));
            let stroke_width = parse_stroke_width(&rect_node[1..]);
            let layer = parse_layer(&rect_node[1..]);
            let fill = parse_fill(&rect_node[1..]);
            fp.fp_rects.push(FpRect {
                start,
                end,
                stroke_width,
                layer,
                fill,
            });
        }

        // fp_poly
        for poly_node in find_children(&items[2..], "fp_poly") {
            let points = find_child(&poly_node[1..], "pts")
                .map(parse_xy_points)
                .unwrap_or_default();
            let stroke_width = parse_stroke_width(&poly_node[1..]);
            let layer = parse_layer(&poly_node[1..]);
            let fill = parse_fill(&poly_node[1..]);
            if !points.is_empty() {
                fp.fp_polys.push(FpPoly {
                    points,
                    stroke_width,
                    layer,
                    fill,
                });
            }
        }

        // fp_text (older format) and fp_text_user
        for txt_node in find_children(&items[2..], "fp_text") {
            if txt_node.len() >= 3 {
                let text_type = match txt_node[1].as_ident().unwrap_or("") {
                    "reference" => FpTextType::Reference,
                    "value" => FpTextType::Value,
                    _ => FpTextType::User,
                };
                let text = txt_node[2].as_string().unwrap_or("").to_string();
                let position = find_child(&txt_node[3..], "at")
                    .map(parse_at)
                    .unwrap_or((0.0, 0.0, 0.0));
                let layer = parse_layer(&txt_node[3..]);
                let font_size = find_child(&txt_node[3..], "effects")
                    .and_then(|e| find_child(&e[1..], "font"))
                    .and_then(|f| find_child(&f[1..], "size"))
                    .and_then(|s| s.get(1).and_then(|v| v.as_number()))
                    .unwrap_or(1.0);
                fp.fp_texts.push(FpText {
                    text,
                    text_type,
                    position,
                    layer,
                    font_size: (font_size, font_size),
                });
            }
        }

        Some(fp)
    }

    /// Parse a boolean value from SExpr, handling both Bool atom and "yes"/"no" identifier.
    fn parse_bool_value(val: Option<&SExpr>) -> bool {
        val.map(|v| match v {
            SExpr::Atom(Atom::Bool(b)) => *b,
            SExpr::Atom(Atom::Identifier(s)) => s == "yes",
            _ => false,
        })
        .unwrap_or(false)
    }

    fn parse_pad(items: &[SExpr]) -> Option<Pad> {
        // (pad "1" smd roundrect (at ...) (size ...) (layers ...) ...)
        let number = items.get(1)?.as_string()?.to_string();
        let pad_type = match items.get(2)?.as_ident().unwrap_or("") {
            "smd" => PadType::Smd,
            "thru_hole" => PadType::ThruHole,
            "connect" => PadType::Connect,
            "np_thru_hole" => PadType::NpThruHole,
            _ => PadType::Smd,
        };
        let shape = match items.get(3)?.as_ident().unwrap_or("") {
            "circle" => PadShape::Circle,
            "rect" => PadShape::Rect,
            "roundrect" => PadShape::RoundRect,
            "oval" => PadShape::Oval,
            "trapezoid" => PadShape::Trapezoid,
            "custom" => PadShape::Custom,
            _ => PadShape::Rect,
        };
        let rest = &items[4..];
        let position = find_child(rest, "at")
            .map(parse_at)
            .unwrap_or((0.0, 0.0, 0.0));
        let size = find_child(rest, "size")
            .map(|s| {
                let w = s.get(1).and_then(|v| v.as_number()).unwrap_or(0.0);
                let h = s.get(2).and_then(|v| v.as_number()).unwrap_or(0.0);
                (w, h)
            })
            .unwrap_or((0.5, 0.5));
        let layers: Vec<String> = find_child(rest, "layers")
            .map(|l| {
                l[1..]
                    .iter()
                    .filter_map(|i| i.as_string().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let drill = find_child(rest, "drill").map(|d| {
            // (drill <dia>) | (drill oval <w> <h>) — oval 载荷是裸 ident
            let is_oval = d
                .get(1)
                .and_then(|v| v.as_ident())
                .map(|s| s == "oval")
                .unwrap_or(false);
            let (diameter, width) = if is_oval {
                let w = d.get(2).and_then(|v| v.as_number()).unwrap_or(0.0);
                let h = d.get(3).and_then(|v| v.as_number()).unwrap_or(w);
                (h, Some(w))
            } else {
                (d.get(1).and_then(|v| v.as_number()).unwrap_or(0.0), None)
            };
            let offset = find_child(&d[1..], "offset")
                .and_then(|o| Some((o.get(1)?.as_number()?, o.get(2)?.as_number()?)));
            DrillDef {
                diameter,
                width,
                offset,
            }
        });
        let net = find_child(rest, "net").and_then(|n| Some(n.get(1)?.as_number()? as u32));
        let pin_function = find_child(rest, "pinfunction")
            .and_then(|p| p.get(1)?.as_string().map(|s| s.to_string()));
        let pin_type =
            find_child(rest, "pintype").and_then(|p| p.get(1)?.as_string().map(|s| s.to_string()));
        let roundrect_rratio =
            find_child(rest, "roundrect_rratio").and_then(|r| r.get(1)?.as_number());
        let solder_mask_margin =
            find_child(rest, "solder_mask_margin").and_then(|s| s.get(1)?.as_number());
        let thermal_bridge_width =
            find_child(rest, "thermal_bridge_width").and_then(|t| t.get(1)?.as_number());
        let thermal_bridge_angle =
            find_child(rest, "thermal_bridge_angle").and_then(|t| t.get(1)?.as_number());
        let thermal_gap = find_child(rest, "thermal_gap").and_then(|t| t.get(1)?.as_number());
        let clearance = find_child(rest, "clearance").and_then(|c| c.get(1)?.as_number());
        let zone_connect =
            find_child(rest, "zone_connect").and_then(|z| z.get(1)?.as_number().map(|n| n as i32));
        // 裸 `(remove_unused_layers)`（无值）是 KiCad 的"启用"语义，不能走 parse_bool_value 的 None=false。
        let remove_unused_layers =
            find_child(rest, "remove_unused_layers").map(|n| match n.get(1) {
                Some(_) => Self::parse_bool_value(n.get(1)),
                None => true,
            });
        let options = find_child(rest, "options").map(|o| PadOptions {
            clearance: find_child(&o[1..], "clearance")
                .and_then(|c| c.get(1)?.as_ident())
                .map(|s| s.to_string()),
            anchor: find_child(&o[1..], "anchor")
                .and_then(|a| a.get(1)?.as_ident())
                .map(|s| s.to_string()),
        });
        let primitives = find_child(rest, "primitives")
            .map(|p| Self::parse_pad_primitives(&p[1..]))
            .unwrap_or_default();

        // 行内名优先: 官方双参 `(net 1 "GND")` 或 name-only 方言 `(net "GND")`;
        // 无行内名时由 from_sexpr 后置 pass 按声明表回填
        let net_name_inline = find_child(rest, "net").and_then(|n| {
            n.get(2)
                .and_then(|x| x.as_string().map(|s| s.to_string()))
                .or_else(|| n.get(1).and_then(|x| x.as_string().map(|s| s.to_string())))
        });
        Some(Pad {
            number,
            pad_type,
            shape,
            position,
            size,
            layers,
            drill,
            net,
            net_name: net_name_inline,
            pin_function,
            pin_type,
            roundrect_rratio,
            solder_mask_margin,
            thermal_bridge_width,
            thermal_bridge_angle,
            thermal_gap,
            clearance,
            zone_connect,
            remove_unused_layers,
            options,
            primitives,
        })
    }

    /// Parse the body of a custom pad's `(primitives ...)` list.
    fn parse_pad_primitives(items: &[SExpr]) -> Vec<PadPrimitive> {
        let mut out = Vec::new();
        for node in items {
            let Some(list) = node.as_list() else { continue };
            let head = list.first().and_then(|h| h.as_ident()).unwrap_or("");
            let rest = &list[1..];
            let width = |r: &[SExpr]| {
                find_child(r, "width")
                    .and_then(|w| w.get(1)?.as_number())
                    .unwrap_or(0.0)
            };
            let fill =
                |r: &[SExpr]| Self::parse_bool_value(find_child(r, "fill").and_then(|f| f.get(1)));
            let prim = match head {
                "gr_poly" => find_child(rest, "pts").map(|pts| PadPrimitive::GrPoly {
                    pts: parse_xy_points(pts),
                    width: width(rest),
                    fill: fill(rest),
                }),
                "gr_rect" => Some(PadPrimitive::GrRect {
                    start: parse_point(rest, "start").unwrap_or((0.0, 0.0)),
                    end: parse_point(rest, "end").unwrap_or((0.0, 0.0)),
                    width: width(rest),
                    fill: fill(rest),
                }),
                "gr_line" => Some(PadPrimitive::GrLine {
                    start: parse_point(rest, "start").unwrap_or((0.0, 0.0)),
                    end: parse_point(rest, "end").unwrap_or((0.0, 0.0)),
                    width: width(rest),
                }),
                "gr_circle" => Some(PadPrimitive::GrCircle {
                    center: parse_point(rest, "center").unwrap_or((0.0, 0.0)),
                    end: parse_point(rest, "end").unwrap_or((0.0, 0.0)),
                    width: width(rest),
                    fill: fill(rest),
                }),
                "gr_arc" => Some(PadPrimitive::GrArc {
                    start: parse_point(rest, "start").unwrap_or((0.0, 0.0)),
                    mid: parse_point(rest, "mid").unwrap_or((0.0, 0.0)),
                    end: parse_point(rest, "end").unwrap_or((0.0, 0.0)),
                    width: width(rest),
                }),
                "segment" => Some(PadPrimitive::Segment {
                    start: parse_point(rest, "start").unwrap_or((0.0, 0.0)),
                    end: parse_point(rest, "end").unwrap_or((0.0, 0.0)),
                    width: width(rest),
                }),
                _ => None,
            };
            if let Some(p) = prim {
                out.push(p);
            }
        }
        out
    }

    fn parse_zone(items: &[SExpr]) -> Option<Zone> {
        let net = find_child(&items[1..], "net").and_then(|n| n.get(1)?.as_number())? as u32;
        // dialect zones carry the name inside (net "GND") only; recover via nets table
        let net_name = find_child(&items[1..], "net_name")
            .and_then(|n| n.get(1)?.as_string().map(|s| s.to_string()))
            .unwrap_or_default();
        let layer = parse_layer(&items[1..]);
        let hatch = find_child(&items[1..], "hatch")
            .map(|h| {
                let style = h
                    .get(1)
                    .and_then(|s| s.as_ident())
                    .unwrap_or("edge")
                    .to_string();
                let pitch = h.get(2).and_then(|n| n.as_number()).unwrap_or(0.508);
                (style, pitch)
            })
            .unwrap_or(("edge".into(), 0.508));
        let connect_pads_node = find_child(&items[1..], "connect_pads");
        // lexer 把 yes/no 归为 Bool 原子, 模式 token 要同时认 Identifier 与 Bool
        let pad_connect = connect_pads_node
            .and_then(|cp| cp.get(1))
            .map(|t| match t {
                SExpr::Atom(Atom::Identifier(s)) => s.clone(),
                SExpr::Atom(Atom::Bool(true)) => "yes".to_string(),
                SExpr::Atom(Atom::Bool(false)) => "no".to_string(),   // KiCad 10 connect_pads 词表 yes/no/clearance
                _ => String::new(),
            })
            .unwrap_or_default();
        let connect_pads_clearance = connect_pads_node
            .and_then(|cp| find_child(&cp[1..], "clearance"))
            .and_then(|c| c.get(1)?.as_number())
            .unwrap_or(0.2);
        let min_thickness = find_child(&items[1..], "min_thickness")
            .and_then(|m| m.get(1)?.as_number())
            .unwrap_or(0.254);
        let fill_node = find_child(&items[1..], "fill");
        let fill = fill_node.and_then(|f| f.get(1)?.as_bool()).unwrap_or(false);
        let thermal_gap = fill_node
            .and_then(|f| find_child(&f[1..], "thermal_gap"))
            .and_then(|g| g.get(1)?.as_number())
            .unwrap_or(0.508);
        let thermal_bridge_width = fill_node
            .and_then(|f| find_child(&f[1..], "thermal_bridge_width"))
            .and_then(|g| g.get(1)?.as_number())
            .unwrap_or(0.508);
        let island_removal_mode = fill_node
            .and_then(|f| find_child(&f[1..], "island_removal_mode"))
            .and_then(|g| g.get(1)?.as_number())
            .map(|n| n as i32)
            .unwrap_or(0);
        let island_area = fill_node
            .and_then(|f| find_child(&f[1..], "island_area"))
            .and_then(|g| g.get(1)?.as_number())
            .unwrap_or(10.0);

        // outline from (polygon (pts (xy ...) ...))
        let outline = find_child(&items[1..], "polygon")
            .and_then(|p| find_child(&p[1..], "pts"))
            .map(parse_xy_points)
            .unwrap_or_default();

        // Parse filled_polygon entries
        let mut filled_polygons = Vec::new();
        for fp_node in find_children(&items[1..], "filled_polygon") {
            let fp_layer = find_child(&fp_node[1..], "layer")
                .and_then(|l| l.get(1).and_then(|v| v.as_string()))
                .unwrap_or(&layer)
                .to_string();
            let pts = find_child(&fp_node[1..], "pts")
                .map(parse_xy_points)
                .unwrap_or_default();
            if !pts.is_empty() {
                filled_polygons.push(FilledPolygon {
                    layer: fp_layer,
                    points: pts,
                });
            }
        }

        Some(Zone {
            net,
            net_name,
            layer,
            hatch_style: hatch.0,
            hatch_pitch: hatch.1,
            pad_connect,
            connect_pads_clearance,
            min_thickness,
            fill,
            thermal_gap,
            thermal_bridge_width,
            island_removal_mode,
            island_area,
            outline,
            filled_polygons,
            keepout: None,
        })
    }

    fn parse_gr_line(items: &[SExpr]) -> Option<BoardGraphic> {
        let start = parse_point(items, "start")?;
        let end = parse_point(items, "end")?;
        let stroke_width = parse_stroke_width(items);
        let layer = parse_layer(items);
        Some(BoardGraphic {
            kind: BoardGraphicKind::Line { start, end },
            layer,
            stroke_width,
            fill: false,
        })
    }

    fn parse_gr_arc(items: &[SExpr]) -> Option<BoardGraphic> {
        let start = parse_point(items, "start")?;
        let mid = parse_point(items, "mid")?;
        let end = parse_point(items, "end")?;
        let stroke_width = parse_stroke_width(items);
        let layer = parse_layer(items);
        Some(BoardGraphic {
            kind: BoardGraphicKind::Arc { start, mid, end },
            layer,
            stroke_width,
            fill: false,
        })
    }

    fn parse_gr_circle(items: &[SExpr]) -> Option<BoardGraphic> {
        let center = parse_point(items, "center")?;
        let end = parse_point(items, "end")?;
        let stroke_width = parse_stroke_width(items);
        let layer = parse_layer(items);
        let fill = parse_fill(items);
        Some(BoardGraphic {
            kind: BoardGraphicKind::Circle { center, end },
            layer,
            stroke_width,
            fill,
        })
    }

    fn parse_gr_rect(items: &[SExpr]) -> Option<BoardGraphic> {
        let start = parse_point(items, "start")?;
        let end = parse_point(items, "end")?;
        let stroke_width = parse_stroke_width(items);
        let layer = parse_layer(items);
        let fill = parse_fill(items);
        Some(BoardGraphic {
            kind: BoardGraphicKind::Rect { start, end },
            layer,
            stroke_width,
            fill,
        })
    }

    fn parse_gr_poly(items: &[SExpr]) -> Option<BoardGraphic> {
        let pts = find_child(items, "pts")?;
        let points = parse_xy_points(pts);
        let stroke_width = parse_stroke_width(items);
        let layer = parse_layer(items);
        Some(BoardGraphic {
            kind: BoardGraphicKind::Poly { points },
            layer,
            stroke_width,
            fill: false,
        })
    }

    fn parse_gr_text(items: &[SExpr], text_node: Option<&SExpr>) -> Option<BoardGraphic> {
        let text = text_node?.as_string()?.to_string();
        let position = find_child(items, "at")
            .map(parse_at)
            .unwrap_or((0.0, 0.0, 0.0));
        let layer = parse_layer(items);
        let font_size = find_child(items, "effects")
            .and_then(|e| find_child(&e[1..], "font"))
            .and_then(|f| find_child(&f[1..], "size"))
            .and_then(|s| s.get(1).and_then(|v| v.as_number()))
            .unwrap_or(1.0);
        Some(BoardGraphic {
            kind: BoardGraphicKind::Text {
                text,
                position,
                font_size,
            },
            layer,
            stroke_width: 0.0,
            fill: false,
        })
    }
}

// ---------------------------------------------------------------------------
// Metadata
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct BoardGeneral {
    pub thickness: f64,
}

impl Default for BoardGeneral {
    fn default() -> Self {
        Self { thickness: 1.6 }
    }
}

#[derive(Debug, Clone, Default)]
pub struct BoardTitleBlock {
    pub title: Option<String>,
    pub date: Option<String>,
    pub rev: Option<String>,
    pub company: Option<String>,
    pub comment: Vec<(usize, String)>,
}

// ---------------------------------------------------------------------------
// Layers
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct LayerDef {
    pub ordinal: u32,
    pub name: String,
    pub layer_type: String,
}

fn default_layers() -> Vec<LayerDef> {
    vec![
        LayerDef {
            ordinal: 0,
            name: "F.Cu".into(),
            layer_type: "signal".into(),
        },
        LayerDef {
            ordinal: 31,
            name: "B.Cu".into(),
            layer_type: "signal".into(),
        },
        LayerDef {
            ordinal: 32,
            name: "B.Adhes".into(),
            layer_type: "user".into(),
        },
        LayerDef {
            ordinal: 33,
            name: "F.Adhes".into(),
            layer_type: "user".into(),
        },
        LayerDef {
            ordinal: 34,
            name: "B.Paste".into(),
            layer_type: "user".into(),
        },
        LayerDef {
            ordinal: 35,
            name: "F.Paste".into(),
            layer_type: "user".into(),
        },
        LayerDef {
            ordinal: 36,
            name: "B.SilkS".into(),
            layer_type: "user".into(),
        },
        LayerDef {
            ordinal: 37,
            name: "F.SilkS".into(),
            layer_type: "user".into(),
        },
        LayerDef {
            ordinal: 38,
            name: "B.Mask".into(),
            layer_type: "user".into(),
        },
        LayerDef {
            ordinal: 39,
            name: "F.Mask".into(),
            layer_type: "user".into(),
        },
        LayerDef {
            ordinal: 40,
            name: "Dwgs.User".into(),
            layer_type: "user".into(),
        },
        LayerDef {
            ordinal: 41,
            name: "Cmts.User".into(),
            layer_type: "user".into(),
        },
        LayerDef {
            ordinal: 42,
            name: "Eco1.User".into(),
            layer_type: "user".into(),
        },
        LayerDef {
            ordinal: 43,
            name: "Eco2.User".into(),
            layer_type: "user".into(),
        },
        LayerDef {
            ordinal: 44,
            name: "Edge.Cuts".into(),
            layer_type: "user".into(),
        },
        LayerDef {
            ordinal: 45,
            name: "Margin".into(),
            layer_type: "user".into(),
        },
        LayerDef {
            ordinal: 46,
            name: "B.CrtYd".into(),
            layer_type: "user".into(),
        },
        LayerDef {
            ordinal: 47,
            name: "F.CrtYd".into(),
            layer_type: "user".into(),
        },
        LayerDef {
            ordinal: 48,
            name: "B.Fab".into(),
            layer_type: "user".into(),
        },
        LayerDef {
            ordinal: 49,
            name: "F.Fab".into(),
            layer_type: "user".into(),
        },
    ]
}

// ---------------------------------------------------------------------------
// Setup / Stackup
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct BoardSetup {
    pub stackup: Vec<StackupLayer>,
    pub pad_to_mask_clearance: f64,
    pub pad_to_paste_clearance: f64,
    pub pad_to_paste_clearance_ratio: f64,
    pub aux_axis_origin: Option<(f64, f64)>,
    pub grid_origin: Option<(f64, f64)>,
}

#[derive(Debug, Clone)]
pub struct StackupLayer {
    pub name: String,
    pub layer_type: String,
    pub color: Option<String>,
    pub thickness: Option<f64>,
    pub material: Option<String>,
    pub epsilon_r: Option<f64>,
    pub loss_tangent: Option<f64>,
}

// ---------------------------------------------------------------------------
// Rule plane
// ---------------------------------------------------------------------------

/// `(net_class NAME "desc" ...)` — the board-level rule plane. Note that
/// `trace_width` here is the class **default** width, not a floor; the floor is
/// [`DesignRules::min_track_width`] in the project file (verified against
/// kicad-cli 10 on 2026-11-16: a 0.127 track passes with the class default at
/// 0.25, and only `rules.min_track_width` makes it a `track_width` violation).
#[derive(Debug, Clone, Default)]
pub struct NetClass {
    pub name: String,
    pub description: String,
    pub clearance: Option<f64>,
    pub trace_width: Option<f64>,
    pub via_dia: Option<f64>,
    pub via_drill: Option<f64>,
    pub uvia_dia: Option<f64>,
    pub uvia_drill: Option<f64>,
    pub diff_pair_width: Option<f64>,
    pub diff_pair_gap: Option<f64>,
    /// `add_net` members (names, not ids — KiCad writes names here).
    pub nets: Vec<String>,
}

/// DRC floors. Emitted into a sibling `.kicad_pro` (`board.design_settings.rules`),
/// because the board file has no legal home for them: `(constraints …)` is an
/// unknown token and `setup` rejects every `*track_width*` spelling.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DesignRules {
    pub min_clearance: Option<f64>,
    pub min_track_width: Option<f64>,
    pub min_via_diameter: Option<f64>,
    pub min_via_annular_width: Option<f64>,
    pub min_microvia_diameter: Option<f64>,
    pub min_microvia_drill: Option<f64>,
    pub min_through_hole_diameter: Option<f64>,
    pub min_hole_clearance: Option<f64>,
    pub min_hole_to_hole: Option<f64>,
    pub min_copper_edge_clearance: Option<f64>,
    pub min_silk_clearance: Option<f64>,
    pub min_text_height: Option<f64>,
    pub min_text_thickness: Option<f64>,
    pub min_connection: Option<f64>,
    pub min_groove_width: Option<f64>,
    pub solder_mask_to_copper_clearance: Option<f64>,
    pub max_error: Option<f64>,
    /// `min_resolved_spokes` — zone thermal spoke count.
    pub min_resolved_spokes: Option<f64>,
}

impl DesignRules {
    /// KiCad's own defaults, as observed in a stock KiCad 10 project. Handy as a
    /// starting point when pinning a board's rule plane to something explicit.
    pub fn kicad_defaults() -> Self {
        Self {
            min_clearance: Some(0.2),
            min_track_width: Some(0.2),
            min_via_diameter: Some(0.5),
            min_via_annular_width: Some(0.1),
            min_microvia_diameter: Some(0.2),
            min_microvia_drill: Some(0.1),
            min_through_hole_diameter: Some(0.3),
            min_hole_clearance: Some(0.25),
            min_hole_to_hole: Some(0.25),
            min_copper_edge_clearance: Some(0.5),
            min_silk_clearance: Some(0.0),
            min_text_height: Some(0.8),
            min_text_thickness: Some(0.08),
            min_connection: Some(0.0),
            min_groove_width: Some(0.0),
            solder_mask_to_copper_clearance: Some(0.0),
            max_error: Some(0.005),
            min_resolved_spokes: Some(2.0),
        }
    }

    /// No field set — nothing worth writing out.
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

// ---------------------------------------------------------------------------
// Net
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct NetDef {
    pub id: u32,
    pub name: String,
}

// ---------------------------------------------------------------------------
// Footprint
// ---------------------------------------------------------------------------

/// Footprint attribute type (smd, thru_hole, or virtual/board_only)
#[derive(Debug, Clone, PartialEq)]
pub enum FootprintAttr {
    Smd,
    ThruHole,
    Virtual,
    BoardOnly,
    /// Multi-value attributes like `exclude_from_pos_files exclude_from_bom dnp`
    Other(Vec<String>),
}

/// Full property with effects (for PCB footprints)
#[derive(Debug, Clone)]
pub struct FpProperty {
    pub name: String,
    pub value: String,
    pub position: (f64, f64, f64),
    pub layer: String,
    pub hide: bool,
    pub unlocked: bool,
    pub effects: FpPropertyEffects,
    pub uuid: Option<String>,
}

#[derive(Debug, Clone)]
pub struct FpPropertyEffects {
    pub font_size: (f64, f64),
    pub font_thickness: f64,
    pub bold: bool,
    pub italic: bool,
    /// KiCad `(justify mirror)` — back-side text renders mirrored
    pub justify_mirror: bool,
}

impl Default for FpPropertyEffects {
    fn default() -> Self {
        Self {
            font_size: (1.0, 1.0),
            font_thickness: 0.15,
            bold: false,
            justify_mirror: false,
            italic: false,
        }
    }
}

/// 3D model reference
#[derive(Debug, Clone)]
pub struct Model3D {
    pub path: String,
    pub offset: (f64, f64, f64),
    pub scale: (f64, f64, f64),
    pub rotate: (f64, f64, f64),
}

#[derive(Debug, Clone, Default)]
pub struct Footprint {
    pub lib_id: String,
    pub reference: String,
    pub value: String,
    pub position: (f64, f64, f64),
    pub layer: String,
    pub uuid: Option<String>,
    pub locked: bool,
    pub descr: Option<String>,
    pub tags: Option<String>,
    pub path: Option<String>,
    pub sheetname: Option<String>,
    pub sheetfile: Option<String>,
    pub attr: Option<FootprintAttr>,
    pub solder_mask_margin: Option<f64>,
    pub solder_paste_margin: Option<f64>,
    pub solder_paste_ratio: Option<f64>,
    pub clearance: Option<f64>,
    pub zone_connect: Option<i32>,
    pub thermal_bridge_width: Option<f64>,
    pub thermal_bridge_angle: Option<f64>,
    pub thermal_gap: Option<f64>,
    /// 3D 模型引用（含 ${KIPRJMOD} 变量的路径原样保真; 可多个）。
    pub models: Vec<Model3D>,
    pub pads: Vec<Pad>,
    pub fp_lines: Vec<FpLine>,
    pub fp_circles: Vec<FpCircle>,
    pub fp_arcs: Vec<FpArc>,
    pub fp_rects: Vec<FpRect>,
    pub fp_polys: Vec<FpPoly>,
    pub fp_texts: Vec<FpText>,
    /// Full properties with effects (parsed from PCB footprints)
    pub properties_ext: Vec<FpProperty>,
    /// Simple key-value properties (legacy, used by schematic flow)
    pub properties: HashMap<String, String>,
}

impl Footprint {
    pub fn new(lib_id: &str, reference: &str, value: &str) -> Self {
        Self {
            lib_id: lib_id.into(),
            reference: reference.into(),
            value: value.into(),
            position: (0.0, 0.0, 0.0),
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
            thermal_gap: None,
            models: Vec::new(),
            pads: Vec::new(),
            fp_lines: Vec::new(),
            fp_circles: Vec::new(),
            fp_arcs: Vec::new(),
            fp_rects: Vec::new(),
            fp_polys: Vec::new(),
            fp_texts: Vec::new(),
            properties_ext: Vec::new(),
            properties: HashMap::new(),
        }
    }

    /// Get absolute pad position (footprint position + pad offset)
    pub fn pad_absolute_pos(&self, pad_number: &str) -> Option<(f64, f64)> {
        let pad = self.pads.iter().find(|p| p.number == pad_number)?;
        let (fx, fy, _) = self.position;
        let (rx, ry) = self.pad_rotated_offset(pad);
        Some((fx + rx, fy + ry))
    }

    /// Pad offset rotated by the footprint rotation (KiCad semantics: pad's own
    /// `(at rot)` only spins the pad shape, the offset is rotated by the
    /// footprint). Add this to the footprint origin to get the absolute position.
    ///
    /// KiCad rotates counterclockwise on screen; with the file's Y axis pointing
    /// down that is algebraically (x, y) → (x·cosθ + y·sinθ, −x·sinθ + y·cosθ).
    pub fn pad_rotated_offset(&self, pad: &Pad) -> (f64, f64) {
        let (_, _, fr) = self.position;
        let (px, py, _) = pad.position;
        if fr.abs() < 1e-9 {
            return (px, py);
        }
        let angle = fr.to_radians();
        let cos = angle.cos();
        let sin = angle.sin();
        (px * cos + py * sin, -px * sin + py * cos)
    }
}

// ---------------------------------------------------------------------------
// Pad
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PadType {
    #[default]
    ThruHole,
    Smd,
    Connect,
    NpThruHole,
}

impl PadType {
    pub fn as_str(&self) -> &'static str {
        match self {
            PadType::Smd => "smd",
            PadType::ThruHole => "thru_hole",
            PadType::Connect => "connect",
            PadType::NpThruHole => "np_thru_hole",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PadShape {
    #[default]
    Circle,
    Rect,
    RoundRect,
    Oval,
    Trapezoid,
    Custom,
}

impl PadShape {
    pub fn as_str(&self) -> &'static str {
        match self {
            PadShape::Circle => "circle",
            PadShape::Rect => "rect",
            PadShape::RoundRect => "roundrect",
            PadShape::Oval => "oval",
            PadShape::Trapezoid => "trapezoid",
            PadShape::Custom => "custom",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Pad {
    pub number: String,
    pub pad_type: PadType,
    pub shape: PadShape,
    pub position: (f64, f64, f64),
    pub size: (f64, f64),
    pub layers: Vec<String>,
    pub drill: Option<DrillDef>,
    pub net: Option<u32>,
    /// Display name resolved from the net table (dialect files carry it in
    /// `(net "NAME")`); used by silk/polarity synthesis.
    pub net_name: Option<String>,
    pub pin_function: Option<String>,
    pub pin_type: Option<String>,
    pub roundrect_rratio: Option<f64>,
    pub solder_mask_margin: Option<f64>,
    pub thermal_bridge_width: Option<f64>,
    pub thermal_bridge_angle: Option<f64>,
    pub thermal_gap: Option<f64>,
    pub clearance: Option<f64>,
    pub zone_connect: Option<i32>,
    /// `(remove_unused_layers no)` — None = 字段缺失；裸 `(remove_unused_layers)` 归一化为 Some(true)。
    pub remove_unused_layers: Option<bool>,
    /// custom pad 的 `(options (clearance X) (anchor Y))`。
    pub options: Option<PadOptions>,
    /// custom pad 的 `(primitives ...)` 图形原语。
    pub primitives: Vec<PadPrimitive>,
}

#[derive(Debug, Clone)]
pub struct PadOptions {
    pub clearance: Option<String>,
    pub anchor: Option<String>,
}

#[derive(Debug, Clone)]
pub enum PadPrimitive {
    GrPoly {
        pts: Vec<(f64, f64)>,
        width: f64,
        fill: bool,
    },
    GrRect {
        start: (f64, f64),
        end: (f64, f64),
        width: f64,
        fill: bool,
    },
    GrLine {
        start: (f64, f64),
        end: (f64, f64),
        width: f64,
    },
    GrCircle {
        center: (f64, f64),
        end: (f64, f64),
        width: f64,
        fill: bool,
    },
    GrArc {
        start: (f64, f64),
        mid: (f64, f64),
        end: (f64, f64),
        width: f64,
    },
    Segment {
        start: (f64, f64),
        end: (f64, f64),
        width: f64,
    },
}

#[derive(Debug, Clone)]
pub struct DrillDef {
    pub diameter: f64,
    /// Slot drill: `(drill oval <width> <height>)` — X size; `diameter` is Y.
    pub width: Option<f64>,
    pub offset: Option<(f64, f64)>,
}

// ---------------------------------------------------------------------------
// Footprint graphics
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct FpLine {
    pub start: (f64, f64),
    pub end: (f64, f64),
    pub stroke_width: f64,
    pub layer: String,
}

#[derive(Debug, Clone)]
pub struct FpCircle {
    pub center: (f64, f64),
    pub end: (f64, f64),
    pub stroke_width: f64,
    pub layer: String,
    pub fill: bool,
}

#[derive(Debug, Clone)]
pub struct FpRect {
    pub start: (f64, f64),
    pub end: (f64, f64),
    pub stroke_width: f64,
    pub layer: String,
    pub fill: bool,
}

#[derive(Debug, Clone)]
pub struct FpPoly {
    pub points: Vec<(f64, f64)>,
    pub stroke_width: f64,
    pub layer: String,
    pub fill: bool,
}

#[derive(Debug, Clone)]
pub struct FpArc {
    pub start: (f64, f64),
    pub mid: (f64, f64),
    pub end: (f64, f64),
    pub stroke_width: f64,
    pub layer: String,
}

#[derive(Debug, Clone)]
pub struct FpText {
    pub text: String,
    pub text_type: FpTextType,
    pub position: (f64, f64, f64),
    pub layer: String,
    pub font_size: (f64, f64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FpTextType {
    Reference,
    Value,
    User,
}

// ---------------------------------------------------------------------------
// Routing
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Segment {
    pub start: (f64, f64),
    pub end: (f64, f64),
    pub width: f64,
    pub layer: String,
    pub net: u32,
}

#[derive(Debug, Clone)]
pub struct Via {
    pub at: (f64, f64),
    pub size: f64,
    pub drill: f64,
    pub layers: Vec<String>,
    pub net: u32,
    /// KiCad via 类型 token: "" = through (默认), "blind"/"micro" 原样带出。
    /// 回流自 downstream project an earlier change(2026-10-01): 起因=P3 analog-board v2.5 盲孔 via-in-pad
    /// 落板——无类型 via 被 KiCad 归一化成通孔致 7~9 shorting; 历史快照 json5
    /// 带 via_type:"blind" 真语义, 缺此字段=静默丢盲孔。
    pub via_type: String,
}

// ---------------------------------------------------------------------------
// Zones
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Zone {
    pub net: u32,
    pub net_name: String,
    pub layer: String,
    pub hatch_style: String,
    pub hatch_pitch: f64,
    /// connect_pads 模式原词: yes / thermal / thru_hole_only / full / none;
    /// 空串 = 文件省略 token (KiCad 默认 thermal)。华秋 fork 拒收裸 thermal,
    /// 导出时空串必须保持省略形态, 显式词原样带出。
    pub pad_connect: String,
    pub connect_pads_clearance: f64,
    pub min_thickness: f64,
    pub fill: bool,
    pub thermal_gap: f64,
    pub thermal_bridge_width: f64,
    /// 0=always remove, 1=never, 2=below island_area
    pub island_removal_mode: i32,
    pub island_area: f64,
    pub outline: Vec<(f64, f64)>,
    pub filled_polygons: Vec<FilledPolygon>,
    pub keepout: Option<Keepout>,
}

#[derive(Debug, Clone)]
pub struct FilledPolygon {
    pub layer: String,
    pub points: Vec<(f64, f64)>,
}

#[derive(Debug, Clone, Default)]
pub struct Keepout {
    pub tracks: bool,
    pub vias: bool,
    pub pads: bool,
    pub copperpour: bool,
    pub footprints: bool,
}

// ---------------------------------------------------------------------------
// Board graphics
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct BoardGraphic {
    pub kind: BoardGraphicKind,
    pub layer: String,
    pub stroke_width: f64,
    pub fill: bool,
}

#[derive(Debug, Clone)]
pub enum BoardGraphicKind {
    Line {
        start: (f64, f64),
        end: (f64, f64),
    },
    Arc {
        start: (f64, f64),
        mid: (f64, f64),
        end: (f64, f64),
    },
    Circle {
        center: (f64, f64),
        end: (f64, f64),
    },
    Rect {
        start: (f64, f64),
        end: (f64, f64),
    },
    Poly {
        points: Vec<(f64, f64)>,
    },
    Text {
        text: String,
        position: (f64, f64, f64),
        font_size: f64,
    },
}

/// Rewrite dialect name-only net refs `(net "NAME")` into `(net ID "NAME")`,
/// allocating ids in `nets` for unseen names. Recursive over the whole tree.
fn normalize_net_name_refs(node: &mut SExpr, nets: &mut Vec<NetDef>) {
    if let SExpr::List(items) = node {
        if items.len() >= 2 && items[0].as_ident() == Some("net") {
            if let SExpr::Atom(Atom::String(name)) = &items[1] {
                let name = name.clone();
                let next = nets.iter().map(|n| n.id).max().unwrap_or(0) + 1;
                let id = nets
                    .iter()
                    .find(|n| n.name == name)
                    .map(|n| n.id)
                    .unwrap_or_else(|| {
                        nets.push(NetDef {
                            id: next,
                            name: name.clone(),
                        });
                        next
                    });
                *node = SExpr::List(vec![
                    SExpr::Atom(Atom::Identifier("net".into())),
                    SExpr::Atom(Atom::Number(id as f64)),
                    SExpr::Atom(Atom::String(name)),
                ]);
                return;
            }
        }
        for child in items.iter_mut() {
            normalize_net_name_refs(child, nets);
        }
    }
}

// ---------------------------------------------------------------------------
// 走线接缝清理 (trace junction snapping)
// ---------------------------------------------------------------------------
//
// 布线器会在走线里留下两类"接缝", DRC 都不报, 在 gerber 里是零长度的 draw 或亚微米的缝:
//   * 准零长度碎片: freerouting 回导的板, 在多条走线的交汇点上, 汇到同一点的两根线被写了两个差
//     0.0001–0.01 mm 的坐标, 中间用一小段连起来 (2026-11-25 实测: analog-board 251 段 / carrier 16 段);
//   * 端点差一点却不相等: 自研 astar 布的板有 4 处 (焊盘中心在 1/8 mm 栅格上, 走线端点在 0.01 mm
//     栅格上), 两根线靠线宽重叠才连通.
// downstream project 的三块板认证按"走线接缝 = 0"把关. SES 的解析在 pcbnew (C++) 那一侧, 碎片是 freerouting
// 自己输出的; 本仓库能修的是"板数据进入生成器之前"这一关. [`Board::snap_trace_junctions`] 两个
// 编译方向都做, 幂等:
//   1. 删掉长度 < tol 的段;
//   2. 同网上距离 < tol 的**线端点**并成一簇, 每簇并到一个锚点上. 只有"线端 ↔ 线端"才算接缝 —— 线端落在
//      过孔铜盘 / 焊盘铜里 (哪怕不在圆心 / 中心) 连接本来就是可靠的, 不动. 锚点候选 = 簇内线端坐标 + 离簇
//      < tol 的同网过孔圆心 / 焊盘中心; 逐个算代价: 挪完之后"原来精确、现在不精确"的线里, 另一端是过孔
//      圆心的 (救不回来) 越少越好, 然后是不精确的条数、偏差角度总和; 再往后才轮到"是不是圆心 / 中心".
//      端点只挪 0.003, 一段 0.1 mm 的短线却能歪 2.7° —— 规整的板不能被弄脏;
//   3. 保方向平移: 因为挪端点而不再精确的线, 整段平移回去 —— 另一端沿垂直于线段的方向挪同一个分量, 以它
//      为端点的相邻段跟着平移, 一直传到走线尽头. 焊盘里的端点可以挪, 但不许挪出焊盘. 碰到过孔圆心
//      (过孔不动) / 端点要挪出焊盘 / 链太长: 这一条整链回退, 线带着微小偏差留下.
// "精确"= 量化到 nm 后 dx = 0 或 dy = 0 或 |dx| = |dy| (容差 5 nm: 平移后的坐标是浮点加法). 不用"偏差
// < 0.2° 算规整"的容差口径 —— 一段 2 mm 长的线偏 0.005 mm 只有 0.13°, 会被放过.
// 坐标全部先量化到 nm 整数 (与 sexpr 生成 fmt_num 的口径一致), 结果确定、可复现.
// 对拍基准是 downstream project 的 Python 版 (三块板实跑过, 同一算法):
//   the Python prototype

/// [`Board::snap_trace_junctions`] 的结果统计。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SnapReport {
    /// 删掉的碎片段数
    pub segments_removed: usize,
    /// 并掉的端点簇数
    pub clusters: usize,
    /// 被挪动的线端数 (含保方向平移带动的)
    pub endpoints_moved: usize,
    /// 做了保方向平移的线数
    pub straightened: usize,
    /// 保方向平移救不回来、带着微小偏差留下的线数
    pub leftover: usize,
}

/// "精确落在 45° 网格上"的容差 (nm).
const GRID_EPS_NM: i64 = 5;
/// 保方向平移一条链最多带动多少个线端.
const STRAIGHTEN_MAX_CHAIN: usize = 80;

type Nm = (i64, i64);

fn to_nm(p: (f64, f64)) -> Nm {
    ((p.0 * 1e6).round() as i64, (p.1 * 1e6).round() as i64)
}

fn nm_dist2(a: Nm, b: Nm) -> i64 {
    let (dx, dy) = (a.0 - b.0, a.1 - b.1);
    dx * dx + dy * dy
}

/// 这段线是否**精确**落在 45° 网格上.
fn on_grid(a: (f64, f64), b: (f64, f64)) -> bool {
    let (a, b) = (to_nm(a), to_nm(b));
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    dx.abs() <= GRID_EPS_NM || dy.abs() <= GRID_EPS_NM || (dx.abs() - dy.abs()).abs() <= GRID_EPS_NM
}

/// 这段线偏离最近的 0°/45°/90°/135° 线多少度.
fn off_grid_deg(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    if dx.hypot(dy) < 1e-9 {
        return 0.0;
    }
    let ang = dy.atan2(dx).to_degrees().rem_euclid(180.0);
    (ang - (ang / 45.0).round() * 45.0).abs()
}

impl Footprint {
    /// 焊盘中心的世界坐标.
    pub fn pad_world_center(&self, pad: &Pad) -> (f64, f64) {
        let (ox, oy) = self.pad_rotated_offset(pad);
        (self.position.0 + ox, self.position.1 + oy)
    }

    /// 点 `p` 是否落在焊盘铜内. 矩形按 (封装转角 + 焊盘转角) 的真实朝向算; 正方形按外接圆算
    /// (circle); 其余形状 (圆角矩形 / 椭圆 / custom) 当直角矩形 —— 比真实铜略松, 用在"端点可以挪"
    /// 的判据上偏保守方向 (宁可不动).
    pub fn pad_contains(&self, pad: &Pad, p: (f64, f64)) -> bool {
        let (cx, cy) = self.pad_world_center(pad);
        let theta = (self.position.2 + pad.position.2).to_radians();
        let (dx, dy) = (p.0 - cx, p.1 - cy);
        // 逆旋转, 与 pad_rotated_offset 的正旋转 (x·cos + y·sin, −x·sin + y·cos) 互逆
        let (lx, ly) = (
            dx * theta.cos() - dy * theta.sin(),
            dx * theta.sin() + dy * theta.cos(),
        );
        let (hw, hh) = (pad.size.0 / 2.0, pad.size.1 / 2.0);
        if (pad.size.0 - pad.size.1).abs() < 1e-9 {
            lx.hypot(ly) <= hw + 1e-9
        } else {
            lx.abs() <= hw + 1e-9 && ly.abs() <= hh + 1e-9
        }
    }
}

impl Board {
    /// 走线接缝清理, 见本节开头. `tol_mm` <= 0 时什么都不做.
    pub fn snap_trace_junctions(&mut self, tol_mm: f64) -> SnapReport {
        let mut rep = SnapReport::default();
        if !(tol_mm > 0.0) || self.segments.is_empty() {
            return rep;
        }
        let tol_nm = (tol_mm * 1e6).round().max(1.0) as i64;
        rep.segments_removed += self.drop_tiny(tol_nm);
        for _ in 0..30 {
            // 不动点: 没有簇可并、也没有新出现的短段
            let pass = self.snap_once(tol_nm);
            rep.clusters += pass.clusters;
            rep.endpoints_moved += pass.endpoints_moved;
            rep.straightened += pass.straightened;
            rep.leftover += pass.leftover;
            let dropped = self.drop_tiny(tol_nm);
            rep.segments_removed += dropped;
            if pass.clusters == 0 && dropped == 0 {
                return rep;
            }
        }
        panic!("snap_trace_junctions: 30 轮还没收敛");
    }

    /// 删掉量化后长度 < tol_nm 的段, 返回删了几段.
    fn drop_tiny(&mut self, tol_nm: i64) -> usize {
        let before = self.segments.len();
        self.segments
            .retain(|s| nm_dist2(to_nm(s.start), to_nm(s.end)) >= tol_nm * tol_nm);
        before - self.segments.len()
    }

    fn snap_once(&mut self, tol_nm: i64) -> SnapReport {
        let mut rep = SnapReport::default();
        // 每条线进入本轮时的两端 —— 保方向平移只认它 ("原来是否精确"、"原始方向")
        let orig: Vec<((f64, f64), (f64, f64))> =
            self.segments.iter().map(|s| (s.start, s.end)).collect();
        // ---- 点表: 同网的线端点 (去重). 度数 = 以它为端点的线端数 ----
        let mut deg: HashMap<(u32, Nm), usize> = HashMap::new();
        for s in &self.segments {
            *deg.entry((s.net, to_nm(s.start))).or_default() += 1;
            *deg.entry((s.net, to_nm(s.end))).or_default() += 1;
        }
        // 锚点候选的补充: 同网过孔圆心 / 焊盘中心 (只有离簇 < tol 的才用得上)
        let via_pts: Vec<(u32, Nm)> = self.vias.iter().map(|v| (v.net, to_nm(v.at))).collect();
        let pad_pts: Vec<(u32, Nm)> = self
            .footprints
            .iter()
            .flat_map(|fp| {
                fp.pads.iter().filter_map(move |pad| {
                    pad.net.map(|net| (net, to_nm(fp.pad_world_center(pad))))
                })
            })
            .collect();
        let mut pts: Vec<(u32, Nm)> = deg.keys().copied().collect();
        pts.sort_unstable();
        pts.dedup();
        let n = pts.len();
        // ---- 并查集 (bucket 限定邻域搜索): 同网、距离 < tol 的线端点并成一簇 ----
        let mut parent: Vec<usize> = (0..n).collect();
        fn find(parent: &mut [usize], mut x: usize) -> usize {
            while parent[x] != x {
                parent[x] = parent[parent[x]];
                x = parent[x];
            }
            x
        }
        let mut buckets: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
        for (i, (_, p)) in pts.iter().enumerate() {
            buckets
                .entry((p.0.div_euclid(tol_nm), p.1.div_euclid(tol_nm)))
                .or_default()
                .push(i);
        }
        for i in 0..n {
            let (net_i, p_i) = pts[i];
            let (bx, by) = (p_i.0.div_euclid(tol_nm), p_i.1.div_euclid(tol_nm));
            for dx in -1..=1 {
                for dy in -1..=1 {
                    if let Some(group) = buckets.get(&(bx + dx, by + dy)) {
                        for &j in group {
                            if j > i
                                && pts[j].0 == net_i
                                && nm_dist2(p_i, pts[j].1) < tol_nm * tol_nm
                            {
                                let (ra, rb) = (find(&mut parent, i), find(&mut parent, j));
                                if ra != rb {
                                    parent[ra.max(rb)] = ra.min(rb);
                                }
                            }
                        }
                    }
                }
            }
        }
        let mut clusters: HashMap<usize, Vec<usize>> = HashMap::new();
        for i in 0..n {
            clusters.entry(find(&mut parent, i)).or_default().push(i);
        }
        // 以 p 为端点的同网线段: (段下标, 0 = start / 1 = end)
        let ends_of = |board: &Board, net: u32, p: Nm| -> Vec<(usize, u8)> {
            board
                .segments
                .iter()
                .enumerate()
                .filter(|(_, s)| s.net == net && (to_nm(s.start) == p || to_nm(s.end) == p))
                .map(|(i, s)| if to_nm(s.start) == p { (i, 0) } else { (i, 1) })
                .collect()
        };
        let near = |c: Nm, coords: &[Nm]| coords.iter().any(|x| nm_dist2(c, *x) < tol_nm * tol_nm);
        let mut absorbed: Vec<(usize, u8, (f64, f64), (f64, f64))> = Vec::new();
        let mut members_by_root: Vec<&Vec<usize>> = clusters.values().collect();
        members_by_root.sort(); // 确定性: 按成员下标排序
        for members in members_by_root {
            if members.len() < 2 {
                continue;
            }
            let net = pts[members[0]].0;
            let coords: Vec<Nm> = members.iter().map(|&m| pts[m].1).collect();
            // ---- 选锚点 (见本节开头第 2 条) ----
            let mut cands: Vec<Nm> = coords.clone();
            for c in via_pts
                .iter()
                .chain(pad_pts.iter())
                .filter(|(vn, _)| *vn == net)
                .map(|(_, c)| *c)
            {
                if near(c, &coords) && !cands.contains(&c) {
                    cands.push(c);
                }
            }
            let mut best: Option<((usize, usize, i64, u8, i64), Nm)> = None;
            for cand in &cands {
                let cf = (cand.0 as f64 / 1e6, cand.1 as f64 / 1e6);
                let (mut n_hard, mut n_off, mut dev_sum) = (0usize, 0usize, 0.0);
                for q in &coords {
                    if q == cand {
                        continue;
                    }
                    let qf = (q.0 as f64 / 1e6, q.1 as f64 / 1e6);
                    for (seg_i, side) in ends_of(self, net, *q) {
                        let s = &self.segments[seg_i];
                        let o = if side == 0 { s.end } else { s.start };
                        if !on_grid(qf, o) || on_grid(cf, o) {
                            continue;
                        }
                        n_off += 1;
                        dev_sum += off_grid_deg(cf, o);
                        if via_pts.contains(&(net, to_nm(o))) {
                            n_hard += 1; // 另一端是过孔圆心: 过孔不动, 这条线救不回来
                        }
                    }
                }
                let centred = via_pts.contains(&(net, *cand)) || pad_pts.contains(&(net, *cand));
                let key = (
                    n_hard,
                    n_off,
                    (dev_sum * 1e4).round() as i64,
                    if centred { 0 } else { 1 },
                    -(*deg.get(&(net, *cand)).unwrap_or(&0) as i64),
                );
                let better = match &best {
                    None => true,
                    Some((bk, bc)) => key < *bk || (key == *bk && *cand < *bc),
                };
                if better {
                    best = Some((key, *cand));
                }
            }
            let Some((_, anchor)) = best else { continue };
            rep.clusters += 1;
            let af = (anchor.0 as f64 / 1e6, anchor.1 as f64 / 1e6);
            for &q in &coords {
                if q == anchor {
                    continue;
                }
                for (seg_i, side) in ends_of(self, net, q) {
                    let qf = (q.0 as f64 / 1e6, q.1 as f64 / 1e6);
                    let ep = if side == 0 {
                        &mut self.segments[seg_i].start
                    } else {
                        &mut self.segments[seg_i].end
                    };
                    *ep = af;
                    rep.endpoints_moved += 1;
                    absorbed.push((seg_i, side, qf, af));
                }
            }
        }
        // ---- 保方向平移 ----
        let (fixed, moved, leftover) = self.straighten(absorbed, &orig);
        rep.straightened += fixed;
        rep.endpoints_moved += moved;
        rep.leftover += leftover;
        rep
    }

    /// 保方向平移. `absorbed` 是刚被并过的线端 (段下标, 哪一端, 原坐标, 新坐标); `orig` 是每条线进入本轮时的两端.
    /// 一条线原来精确、现在不精确: 把它的另一端挪到"吸附端当前坐标 + 原始方向向量" —— 线段 = 原线平移 (< tol).
    /// 以这一端为端点的同网线段那一端都跟着挪; 其中"原来精确、现在不精确"的继续往它们的另一端传, 一直到走线尽头.
    /// 焊盘里的端点可以挪, 但不许挪出焊盘. 碰到过孔圆心 (过孔不动) / 端点要挪出焊盘 / 链太长: 这一条整链回退,
    /// 线带着微小偏差留下. 返回 (修好的线数, 带动的线端数, 救不回来的线数).
    ///
    /// "原来精确"和"原始方向"**只认 orig**. 拿当前坐标算会出两种错 (analog-board 上各实锤一次): 一条线已被前面某条线的
    /// 平移链顺带修好, 仍被当成"歪了"再平移一次, 把整条链拉回去; 另一端已被挪过, 一条被弄歪的线被当成
    /// "本来就不精确"放过.
    fn straighten(
        &mut self,
        absorbed: Vec<(usize, u8, (f64, f64), (f64, f64))>,
        orig: &[((f64, f64), (f64, f64))],
    ) -> (usize, usize, usize) {
        let pads_by_net: HashMap<u32, Vec<(&Footprint, &Pad)>> = {
            let mut m: HashMap<u32, Vec<(&Footprint, &Pad)>> = HashMap::new();
            for fp in &self.footprints {
                for pad in &fp.pads {
                    if let Some(net) = pad.net {
                        m.entry(net).or_default().push((fp, pad));
                    }
                }
            }
            m
        };
        let via_centers: Vec<(u32, (f64, f64))> = self.vias.iter().map(|v| (v.net, v.at)).collect();
        // p 所在的焊盘 (同网, 点落在铜内)
        let pad_at = |net: u32, p: (f64, f64)| -> Option<usize> {
            pads_by_net
                .get(&net)?
                .iter()
                .position(|(fp, pad)| fp.pad_contains(pad, p))
        };
        let same = |a: (f64, f64), b: (f64, f64)| a.0 == b.0 && a.1 == b.1;
        // 以 p 为端点的同网线段 (当前坐标): (段下标, 哪一端, 另一端坐标)
        let ends_at = |board: &Board, net: u32, p: (f64, f64)| -> Vec<(usize, u8, (f64, f64))> {
            board
                .segments
                .iter()
                .enumerate()
                .filter(|(_, s)| s.net == net && (same(s.start, p) || same(s.end, p)))
                .map(|(i, s)| {
                    if same(s.start, p) {
                        (i, 0, s.end)
                    } else {
                        (i, 1, s.start)
                    }
                })
                .collect()
        };
        // 量化到 nm (与 fmt_num 一致; 也与 Python 版对拍)
        let q_nm = |v: f64| ((v * 1e6).round() / 1e6, 0.0).0;
        let mut fixed = 0usize;
        let mut moved_total = 0usize;
        let mut leftover = 0usize;
        for (seg_i, side, _q, _r) in absorbed {
            let s = &self.segments[seg_i];
            let (q0, o0) = if side == 0 {
                (orig[seg_i].0, orig[seg_i].1)
            } else {
                (orig[seg_i].1, orig[seg_i].0)
            };
            let dbg = std::env::var_os("KICAD_JSON5_SNAP_DEBUG").is_some();
            // 原来就不精确的 / 现在已经精确的 (可能已被前面某条线的平移链顺带修好): 不管
            if !on_grid(q0, o0) || on_grid(s.start, s.end) {
                if dbg {
                    eprintln!(
                        "snap debug: net {} seg {seg_i} side {side} was ({:.4},{:.4})-({:.4},{:.4}) now ({:.4},{:.4})-({:.4},{:.4}): skip (was off-grid {}, now on-grid {})",
                        s.net, q0.0, q0.1, o0.0, o0.1, s.start.0, s.start.1, s.end.0, s.end.1,
                        !on_grid(q0, o0), on_grid(s.start, s.end)
                    );
                }
                continue;
            }
            let net = s.net;
            let r_ = if side == 0 { s.start } else { s.end }; // 当前的两端
            let o = if side == 0 { s.end } else { s.start };
            let dp = (r_.0 + (o0.0 - q0.0) - o.0, r_.1 + (o0.1 - q0.1) - o.1);
            // 先进先出 (与 Python 版一致): 链有分叉时, 处理顺序会影响链太长 / 碰到过孔的判定
            let mut todo: std::collections::VecDeque<((f64, f64), (f64, f64))> =
                std::collections::VecDeque::from([(o, dp)]);
            let mut trail: Vec<(usize, u8, (f64, f64))> = Vec::new();
            let mut seen: std::collections::HashSet<(Nm, u64, u64)> = Default::default();
            let mut ok = true;
            while let Some((x, d)) = todo.pop_front() {
                if d.0.hypot(d.1) < 1e-9 {
                    continue;
                }
                if !seen.insert((to_nm(x), d.0.to_bits(), d.1.to_bits())) {
                    continue;
                }
                let x2 = (q_nm(x.0 + d.0), q_nm(x.1 + d.1));
                let pad0 = pad_at(net, x);
                let is_via = via_centers
                    .iter()
                    .any(|(vn, vc)| *vn == net && same(*vc, x));
                let leaves_pad = pad0.is_some_and(|pi| {
                    let (fp, pad) = pads_by_net[&net][pi];
                    !fp.pad_contains(pad, x2)
                });
                if is_via || leaves_pad || trail.len() > STRAIGHTEN_MAX_CHAIN {
                    if std::env::var_os("KICAD_JSON5_SNAP_DEBUG").is_some() {
                        eprintln!(
                            "snap debug:   roll back at ({:.4},{:.4}) -> ({:.4},{:.4}): via {} leaves_pad {} trail {}",
                            x.0, x.1, x2.0, x2.1, is_via, leaves_pad, trail.len()
                        );
                    }
                    ok = false;
                    break;
                }
                for (s2, e2, o2) in ends_at(self, net, x) {
                    trail.push((s2, e2, x));
                    if e2 == 0 {
                        self.segments[s2].start = x2;
                    } else {
                        self.segments[s2].end = x2;
                    }
                    let (x0, o20) = if e2 == 0 {
                        (orig[s2].0, orig[s2].1)
                    } else {
                        (orig[s2].1, orig[s2].0)
                    };
                    // 这一段原来精确、现在不精确 → 另一端按原始方向跟着平移
                    if on_grid(x0, o20) && !on_grid(x2, o2) {
                        todo.push_back((
                            o2,
                            (x2.0 + (o20.0 - x0.0) - o2.0, x2.1 + (o20.1 - x0.1) - o2.1),
                        ));
                    }
                }
            }
            if dbg {
                eprintln!(
                    "snap debug: net {net} seg {seg_i} side {side} was ({:.4},{:.4})-({:.4},{:.4}) now ({:.4},{:.4})-({:.4},{:.4}): {} {}",
                    q0.0, q0.1, o0.0, o0.1, self.segments[seg_i].start.0, self.segments[seg_i].start.1,
                    self.segments[seg_i].end.0, self.segments[seg_i].end.1,
                    if ok { "slid" } else { "rolled back" }, trail.len()
                );
            }
            if ok {
                fixed += 1;
                moved_total += trail.len();
            } else {
                leftover += 1;
                for (s2, e2, x) in trail.into_iter().rev() {
                    if e2 == 0 {
                        self.segments[s2].start = x;
                    } else {
                        self.segments[s2].end = x;
                    }
                }
            }
        }
        (fixed, moved_total, leftover)
    }
}
