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
    list.iter().filter_map(|item| {
        let children = item.as_list()?;
        children.first()?.as_ident().filter(|id| *id == tag)?;
        Some(children)
    }).collect()
}

/// Parse `(at x y [angle])` into `(x, y, angle)`.
fn parse_at(items: &[SExpr]) -> (f64, f64, f64) {
    if items.len() >= 4 {
        (items[1].as_number().unwrap_or(0.0), items[2].as_number().unwrap_or(0.0), items[3].as_number().unwrap_or(0.0))
    } else if items.len() >= 3 {
        (items[1].as_number().unwrap_or(0.0), items[2].as_number().unwrap_or(0.0), 0.0)
    } else {
        (0.0, 0.0, 0.0)
    }
}

/// Parse `(xy x y)` pairs from a `(pts ...)` list.
fn parse_xy_points(items: &[SExpr]) -> Vec<(f64, f64)> {
    items.iter().skip(1).filter_map(|item| {
        let pair = item.as_list()?;
        pair.first()?.as_ident().filter(|id| *id == "xy")?;
        Some((pair.get(1)?.as_number().unwrap_or(0.0), pair.get(2)?.as_number().unwrap_or(0.0)))
    }).collect()
}

/// Parse `(start x y)` or `(end x y)` from items.
fn parse_point(items: &[SExpr], tag: &str) -> Option<(f64, f64)> {
    let child = find_child(items, tag)?;
    Some((child.get(1)?.as_number().unwrap_or(0.0), child.get(2)?.as_number().unwrap_or(0.0)))
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
            nets: vec![NetDef { id: 0, name: String::new() }],
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
        self.nets.push(NetDef { id, name: name.into() });
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
                if let (Some(id), Some(name)) =
                    (n.get(1).and_then(|v| v.as_number()), n.get(2).and_then(|v| v.as_string()))
                {
                    let id = id as u32;
                    if !dialect_nets.iter().any(|d| d.id == id) {
                        dialect_nets.push(NetDef { id, name: name.to_string() });
                    }
                }
            }
        }
        normalize_net_name_refs(&mut sexpr, &mut dialect_nets);

        let root = sexpr.as_list().ok_or_else(|| Error::ParserError {
            line: 0, column: 0,
            message: "expected (kicad_pcb ...)".into(),
        })?;
        if root.first().and_then(|h| h.as_ident()) != Some("kicad_pcb") {
            return Err(Error::ParserError { line: 0, column: 0, message: "root must be (kicad_pcb ...)".into() });
        }

        let items = &root[1..];
        let mut board = Board::new();

        // version / generator
        if let Some(v) = find_child(items, "version") {
            board.version = v.get(1).and_then(|n| n.as_number()).map(|n| format!("{}", n as u64)).unwrap_or_default();
        }
        if let Some(g) = find_child(items, "generator") {
            board.generator = g.get(1).and_then(|s| s.as_string()).unwrap_or("").to_string();
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
                for i in 2..p.len() {
                    if let Some(n) = p[i].as_number() {
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
                        board.layers.push(LayerDef { ordinal, name, layer_type });
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
                nets.push(NetDef { id, name });
            }
        }
        // KiCad always has net 0 (empty name) even if not explicit
        if !nets.iter().any(|n| n.id == 0) {
            nets.insert(0, NetDef { id: 0, name: String::new() });
        }
        // top-level table is authoritative; then add ids synthesized for
        // dialect name-only net refs
        board.nets = nets;
        for d in &dialect_nets {
            if !board.nets.iter().any(|n| n.id == d.id) {
                board.nets.push(d.clone());
            }
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
            let width = find_child(&seg_node[1..], "width").and_then(|w| w.get(1)?.as_number()).unwrap_or(0.25);
            let layer = parse_layer(&seg_node[1..]);
            let net = find_child(&seg_node[1..], "net").and_then(|n| n.get(1)?.as_number()).unwrap_or(0.0) as u32;
            board.segments.push(Segment { start, end, width, layer, net });
        }

        // vias
        for via_node in find_children(items, "via") {
            let at_items = find_child(&via_node[1..], "at");
            let at = at_items.map(|a| (a.get(1).unwrap().as_number().unwrap_or(0.0), a.get(2).unwrap().as_number().unwrap_or(0.0))).unwrap_or((0.0, 0.0));
            let size = find_child(&via_node[1..], "size").and_then(|s| s.get(1)?.as_number()).unwrap_or(0.6);
            let drill = find_child(&via_node[1..], "drill").and_then(|d| d.get(1)?.as_number()).unwrap_or(0.3);
            let layers: Vec<String> = find_child(&via_node[1..], "layers")
                .map(|l| l[1..].iter().filter_map(|i| i.as_string().map(|s| s.to_string())).collect())
                .unwrap_or_default();
            let net = find_child(&via_node[1..], "net").and_then(|n| n.get(1)?.as_number()).unwrap_or(0.0) as u32;
            board.vias.push(Via { at, size, drill, layers, net });
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
                    if id == 0 { continue; }
                    match board.nets.iter_mut().find(|n| n.id == id) {
                        Some(n) => {
                            if n.name.is_empty() {
                                if let Some(nm) = &pad.net_name {
                                    if !nm.is_empty() { n.name = nm.clone(); }
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
                    if let Some(nm) = board.nets.iter().find(|n| n.id == zone.net).map(|n| n.name.clone()) {
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
        let position = find_child(&items[2..], "at").map(|a| parse_at(a)).unwrap_or((0.0, 0.0, 0.0));

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
                let values: Vec<String> = a[1..].iter()
                    .filter_map(|v| v.as_ident().map(|s| s.to_string()))
                    .collect();
                if values.is_empty() { None } else { Some(FootprintAttr::Other(values)) }
            } else {
                None
            }
        });

        // solder_mask_margin, solder_paste_margin, solder_paste_ratio, clearance
        let solder_mask_margin = find_child(&items[2..], "solder_mask_margin")
            .and_then(|s| s.get(1)?.as_number());
        let solder_paste_margin = find_child(&items[2..], "solder_paste_margin")
            .and_then(|s| s.get(1)?.as_number());
        let solder_paste_ratio = find_child(&items[2..], "solder_paste_ratio")
            .and_then(|s| s.get(1)?.as_number());
        let clearance = find_child(&items[2..], "clearance")
            .and_then(|c| c.get(1)?.as_number());
        let zone_connect = find_child(&items[2..], "zone_connect")
            .and_then(|z| z.get(1)?.as_number().map(|n| n as i32));
        let thermal_bridge_width = find_child(&items[2..], "thermal_bridge_width")
            .and_then(|t| t.get(1)?.as_number());
        let thermal_bridge_angle = find_child(&items[2..], "thermal_bridge_angle")
            .and_then(|t| t.get(1)?.as_number());
        let thermal_gap = find_child(&items[2..], "thermal_gap")
            .and_then(|t| t.get(1)?.as_number());

        // models（可多个: KiCad 允许一个 footprint 引用多个 3D 模型）。
        // 子字段全部可缺省 (KiCad 允许只写 path), 缺省按 KiCad 默认:
        // offset (0,0,0) / scale (1,1,1) / rotate (0,0,0)。
        let models: Vec<Model3D> = find_children(&items[2..], "model").iter().filter_map(|m| {
            let path = m.get(1)?.as_string()?.to_string();
            Some(Model3D {
                path,
                offset: parse_xyz_child(&m[2..], "offset").unwrap_or((0.0, 0.0, 0.0)),
                scale: parse_xyz_child(&m[2..], "scale").unwrap_or((1.0, 1.0, 1.0)),
                rotate: parse_xyz_child(&m[2..], "rotate").unwrap_or((0.0, 0.0, 0.0)),
            })
        }).collect();

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
                let prop_pos = find_child(prop_rest, "at").map(|a| parse_at(a)).unwrap_or((0.0, 0.0, 0.0));
                let prop_layer = parse_layer(prop_rest);
                let prop_hide = find_child(prop_rest, "hide")
                    .map(|h| Self::parse_bool_value(h.get(1)))
                    .unwrap_or(false);
                let prop_unlocked = find_child(prop_rest, "unlocked")
                    .map(|u| Self::parse_bool_value(u.get(1)))
                    .unwrap_or(false);
                let prop_uuid = find_child(prop_rest, "uuid")
                    .and_then(|u| u.get(1).and_then(|v| v.as_string().map(|s| s.to_string())));

                // Parse effects
                let effects = find_child(prop_rest, "effects").map(|e| {
                    let font = find_child(&e[1..], "font");
                    let (fs_w, fs_h, thickness, bold, italic) = if let Some(f) = font {
                        let size_node = find_child(&f[1..], "size");
                        let (sw, sh) = size_node.map(|s| {
                            (s.get(1).and_then(|v| v.as_number()).unwrap_or(1.0),
                             s.get(2).and_then(|v| v.as_number()).unwrap_or(1.0))
                        }).unwrap_or((1.0, 1.0));
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
                    FpPropertyEffects {
                        font_size: (fs_w, fs_h),
                        font_thickness: thickness,
                        bold,
                        italic,
                    }
                }).unwrap_or_default();

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

        // fp_line
        for line_node in find_children(&items[2..], "fp_line") {
            let start = parse_point(&line_node[1..], "start").unwrap_or((0.0, 0.0));
            let end = parse_point(&line_node[1..], "end").unwrap_or((0.0, 0.0));
            let stroke_width = parse_stroke_width(&line_node[1..]);
            let layer = parse_layer(&line_node[1..]);
            fp.fp_lines.push(FpLine { start, end, stroke_width, layer });
        }

        // fp_circle
        for circ_node in find_children(&items[2..], "fp_circle") {
            let center = parse_point(&circ_node[1..], "center").unwrap_or((0.0, 0.0));
            let end = parse_point(&circ_node[1..], "end").unwrap_or((0.0, 0.0));
            let stroke_width = parse_stroke_width(&circ_node[1..]);
            let layer = parse_layer(&circ_node[1..]);
            let fill = parse_fill(&circ_node[1..]);
            fp.fp_circles.push(FpCircle { center, end, stroke_width, layer, fill });
        }

        // fp_arc
        for arc_node in find_children(&items[2..], "fp_arc") {
            let start = parse_point(&arc_node[1..], "start").unwrap_or((0.0, 0.0));
            let mid = parse_point(&arc_node[1..], "mid").unwrap_or((0.0, 0.0));
            let end = parse_point(&arc_node[1..], "end").unwrap_or((0.0, 0.0));
            let stroke_width = parse_stroke_width(&arc_node[1..]);
            let layer = parse_layer(&arc_node[1..]);
            fp.fp_arcs.push(FpArc { start, mid, end, stroke_width, layer });
        }

        // fp_rect
        for rect_node in find_children(&items[2..], "fp_rect") {
            let start = parse_point(&rect_node[1..], "start").unwrap_or((0.0, 0.0));
            let end = parse_point(&rect_node[1..], "end").unwrap_or((0.0, 0.0));
            let stroke_width = parse_stroke_width(&rect_node[1..]);
            let layer = parse_layer(&rect_node[1..]);
            let fill = parse_fill(&rect_node[1..]);
            fp.fp_rects.push(FpRect { start, end, stroke_width, layer, fill });
        }

        // fp_poly
        for poly_node in find_children(&items[2..], "fp_poly") {
            let points = find_child(&poly_node[1..], "pts")
                .map(|pts| parse_xy_points(pts))
                .unwrap_or_default();
            let stroke_width = parse_stroke_width(&poly_node[1..]);
            let layer = parse_layer(&poly_node[1..]);
            let fill = parse_fill(&poly_node[1..]);
            if !points.is_empty() {
                fp.fp_polys.push(FpPoly { points, stroke_width, layer, fill });
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
                let position = find_child(&txt_node[3..], "at").map(|a| parse_at(a)).unwrap_or((0.0, 0.0, 0.0));
                let layer = parse_layer(&txt_node[3..]);
                let font_size = find_child(&txt_node[3..], "effects")
                    .and_then(|e| find_child(&e[1..], "font"))
                    .and_then(|f| find_child(&f[1..], "size"))
                    .and_then(|s| s.get(1).and_then(|v| v.as_number()))
                    .unwrap_or(1.0);
                fp.fp_texts.push(FpText { text, text_type, position, layer, font_size: (font_size, font_size) });
            }
        }

        Some(fp)
    }

    /// Parse a boolean value from SExpr, handling both Bool atom and "yes"/"no" identifier.
    fn parse_bool_value(val: Option<&SExpr>) -> bool {
        val.map(|v| {
            match v {
                SExpr::Atom(Atom::Bool(b)) => *b,
                SExpr::Atom(Atom::Identifier(s)) => s == "yes",
                _ => false,
            }
        }).unwrap_or(false)
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
        let position = find_child(rest, "at").map(|a| parse_at(a)).unwrap_or((0.0, 0.0, 0.0));
        let size = find_child(rest, "size")
            .and_then(|s| {
                let w = s.get(1).and_then(|v| v.as_number()).unwrap_or(0.0);
                let h = s.get(2).and_then(|v| v.as_number()).unwrap_or(0.0);
                Some((w, h))
            })
            .unwrap_or((0.5, 0.5));
        let layers: Vec<String> = find_child(rest, "layers")
            .map(|l| l[1..].iter().filter_map(|i| i.as_string().map(|s| s.to_string())).collect())
            .unwrap_or_default();
        let drill = find_child(rest, "drill").map(|d| {
            let diameter = d.get(1).unwrap().as_number().unwrap_or(0.0);
            DrillDef { diameter, offset: None }
        });
        let net = find_child(rest, "net").and_then(|n| Some(n.get(1)?.as_number()? as u32));
        let pin_function = find_child(rest, "pinfunction").and_then(|p| p.get(1)?.as_string().map(|s| s.to_string()));
        let pin_type = find_child(rest, "pintype").and_then(|p| p.get(1)?.as_string().map(|s| s.to_string()));
        let roundrect_rratio = find_child(rest, "roundrect_rratio").and_then(|r| r.get(1)?.as_number());
        let solder_mask_margin = find_child(rest, "solder_mask_margin").and_then(|s| s.get(1)?.as_number());
        let thermal_bridge_width = find_child(rest, "thermal_bridge_width").and_then(|t| t.get(1)?.as_number());
        let thermal_bridge_angle = find_child(rest, "thermal_bridge_angle").and_then(|t| t.get(1)?.as_number());
        let thermal_gap = find_child(rest, "thermal_gap").and_then(|t| t.get(1)?.as_number());
        let clearance = find_child(rest, "clearance").and_then(|c| c.get(1)?.as_number());
        let zone_connect = find_child(rest, "zone_connect").and_then(|z| z.get(1)?.as_number().map(|n| n as i32));
        // 裸 `(remove_unused_layers)`（无值）是 KiCad 的"启用"语义，不能走 parse_bool_value 的 None=false。
        let remove_unused_layers = find_child(rest, "remove_unused_layers")
            .map(|n| match n.get(1) {
                Some(_) => Self::parse_bool_value(n.get(1)),
                None => true,
            });
        let options = find_child(rest, "options").and_then(|o| {
            Some(PadOptions {
                clearance: find_child(&o[1..], "clearance").and_then(|c| c.get(1)?.as_ident()).map(|s| s.to_string()),
                anchor: find_child(&o[1..], "anchor").and_then(|a| a.get(1)?.as_ident()).map(|s| s.to_string()),
            })
        });
        let primitives = find_child(rest, "primitives")
            .map(|p| Self::parse_pad_primitives(&p[1..]))
            .unwrap_or_default();

        // 行内名优先: 官方双参 `(net 1 "GND")` 或 name-only 方言 `(net "GND")`;
        // 无行内名时由 from_sexpr 后置 pass 按声明表回填
        let net_name_inline = find_child(rest, "net").and_then(|n| {
            n.get(2).and_then(|x| x.as_string().map(|s| s.to_string()))
                .or_else(|| n.get(1).and_then(|x| x.as_string().map(|s| s.to_string())))
        });
        Some(Pad {
            number, pad_type, shape, position, size, layers, drill, net,
            net_name: net_name_inline,
            pin_function, pin_type, roundrect_rratio,
            solder_mask_margin, thermal_bridge_width, thermal_bridge_angle,
            thermal_gap, clearance, zone_connect,
            remove_unused_layers, options, primitives,
        })
    }

    /// Parse the body of a custom pad's `(primitives ...)` list.
    fn parse_pad_primitives(items: &[SExpr]) -> Vec<PadPrimitive> {
        let mut out = Vec::new();
        for node in items {
            let Some(list) = node.as_list() else { continue };
            let head = list.first().and_then(|h| h.as_ident()).unwrap_or("");
            let rest = &list[1..];
            let width = |r: &[SExpr]| find_child(r, "width").and_then(|w| w.get(1)?.as_number()).unwrap_or(0.0);
            let fill = |r: &[SExpr]| Self::parse_bool_value(find_child(r, "fill").and_then(|f| f.get(1)));
            let prim = match head {
                "gr_poly" => find_child(rest, "pts").map(|pts| PadPrimitive::GrPoly {
                    pts: parse_xy_points(pts), width: width(rest), fill: fill(rest),
                }),
                "gr_rect" => Some(PadPrimitive::GrRect {
                    start: parse_point(rest, "start").unwrap_or((0.0, 0.0)),
                    end: parse_point(rest, "end").unwrap_or((0.0, 0.0)),
                    width: width(rest), fill: fill(rest),
                }),
                "gr_line" => Some(PadPrimitive::GrLine {
                    start: parse_point(rest, "start").unwrap_or((0.0, 0.0)),
                    end: parse_point(rest, "end").unwrap_or((0.0, 0.0)),
                    width: width(rest),
                }),
                "gr_circle" => Some(PadPrimitive::GrCircle {
                    center: parse_point(rest, "center").unwrap_or((0.0, 0.0)),
                    end: parse_point(rest, "end").unwrap_or((0.0, 0.0)),
                    width: width(rest), fill: fill(rest),
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
                let style = h.get(1).and_then(|s| s.as_ident()).unwrap_or("edge").to_string();
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
                SExpr::Atom(Atom::Bool(false)) => "none".to_string(),
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
        let fill = fill_node
            .and_then(|f| f.get(1)?.as_bool())
            .unwrap_or(false);
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
            .map(|pts| parse_xy_points(pts))
            .unwrap_or_default();

        // Parse filled_polygon entries
        let mut filled_polygons = Vec::new();
        for fp_node in find_children(&items[1..], "filled_polygon") {
            let fp_layer = find_child(&fp_node[1..], "layer")
                .and_then(|l| l.get(1).and_then(|v| v.as_string()))
                .unwrap_or(&layer)
                .to_string();
            let pts = find_child(&fp_node[1..], "pts")
                .map(|pts| parse_xy_points(pts))
                .unwrap_or_default();
            if !pts.is_empty() {
                filled_polygons.push(FilledPolygon { layer: fp_layer, points: pts });
            }
        }

        Some(Zone {
            net, net_name, layer,
            hatch_style: hatch.0, hatch_pitch: hatch.1,
            pad_connect,
            connect_pads_clearance, min_thickness, fill,
            thermal_gap, thermal_bridge_width,
            island_removal_mode, island_area,
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
        Some(BoardGraphic { kind: BoardGraphicKind::Line { start, end }, layer, stroke_width, fill: false })
    }

    fn parse_gr_arc(items: &[SExpr]) -> Option<BoardGraphic> {
        let start = parse_point(items, "start")?;
        let mid = parse_point(items, "mid")?;
        let end = parse_point(items, "end")?;
        let stroke_width = parse_stroke_width(items);
        let layer = parse_layer(items);
        Some(BoardGraphic { kind: BoardGraphicKind::Arc { start, mid, end }, layer, stroke_width, fill: false })
    }

    fn parse_gr_circle(items: &[SExpr]) -> Option<BoardGraphic> {
        let center = parse_point(items, "center")?;
        let end = parse_point(items, "end")?;
        let stroke_width = parse_stroke_width(items);
        let layer = parse_layer(items);
        let fill = parse_fill(items);
        Some(BoardGraphic { kind: BoardGraphicKind::Circle { center, end }, layer, stroke_width, fill })
    }

    fn parse_gr_rect(items: &[SExpr]) -> Option<BoardGraphic> {
        let start = parse_point(items, "start")?;
        let end = parse_point(items, "end")?;
        let stroke_width = parse_stroke_width(items);
        let layer = parse_layer(items);
        let fill = parse_fill(items);
        Some(BoardGraphic { kind: BoardGraphicKind::Rect { start, end }, layer, stroke_width, fill })
    }

    fn parse_gr_poly(items: &[SExpr]) -> Option<BoardGraphic> {
        let pts = find_child(items, "pts")?;
        let points = parse_xy_points(pts);
        let stroke_width = parse_stroke_width(items);
        let layer = parse_layer(items);
        Some(BoardGraphic { kind: BoardGraphicKind::Poly { points }, layer, stroke_width, fill: false })
    }

    fn parse_gr_text(items: &[SExpr], text_node: Option<&SExpr>) -> Option<BoardGraphic> {
        let text = text_node?.as_string()?.to_string();
        let position = find_child(items, "at").map(|a| parse_at(a)).unwrap_or((0.0, 0.0, 0.0));
        let layer = parse_layer(items);
        let font_size = find_child(items, "effects")
            .and_then(|e| find_child(&e[1..], "font"))
            .and_then(|f| find_child(&f[1..], "size"))
            .and_then(|s| s.get(1).and_then(|v| v.as_number()))
            .unwrap_or(1.0);
        Some(BoardGraphic { kind: BoardGraphicKind::Text { text, position, font_size }, layer, stroke_width: 0.0, fill: false })
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
        LayerDef { ordinal: 0,  name: "F.Cu".into(),        layer_type: "signal".into() },
        LayerDef { ordinal: 31, name: "B.Cu".into(),        layer_type: "signal".into() },
        LayerDef { ordinal: 32, name: "B.Adhes".into(),     layer_type: "user".into() },
        LayerDef { ordinal: 33, name: "F.Adhes".into(),     layer_type: "user".into() },
        LayerDef { ordinal: 34, name: "B.Paste".into(),     layer_type: "user".into() },
        LayerDef { ordinal: 35, name: "F.Paste".into(),     layer_type: "user".into() },
        LayerDef { ordinal: 36, name: "B.SilkS".into(),     layer_type: "user".into() },
        LayerDef { ordinal: 37, name: "F.SilkS".into(),     layer_type: "user".into() },
        LayerDef { ordinal: 38, name: "B.Mask".into(),      layer_type: "user".into() },
        LayerDef { ordinal: 39, name: "F.Mask".into(),      layer_type: "user".into() },
        LayerDef { ordinal: 40, name: "Dwgs.User".into(),   layer_type: "user".into() },
        LayerDef { ordinal: 41, name: "Cmts.User".into(),   layer_type: "user".into() },
        LayerDef { ordinal: 42, name: "Eco1.User".into(),   layer_type: "user".into() },
        LayerDef { ordinal: 43, name: "Eco2.User".into(),   layer_type: "user".into() },
        LayerDef { ordinal: 44, name: "Edge.Cuts".into(),   layer_type: "user".into() },
        LayerDef { ordinal: 45, name: "Margin".into(),      layer_type: "user".into() },
        LayerDef { ordinal: 46, name: "B.CrtYd".into(),     layer_type: "user".into() },
        LayerDef { ordinal: 47, name: "F.CrtYd".into(),     layer_type: "user".into() },
        LayerDef { ordinal: 48, name: "B.Fab".into(),       layer_type: "user".into() },
        LayerDef { ordinal: 49, name: "F.Fab".into(),       layer_type: "user".into() },
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
}

impl Default for FpPropertyEffects {
    fn default() -> Self {
        Self { font_size: (1.0, 1.0), font_thickness: 0.15, bold: false, italic: false }
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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadType {
    Smd,
    ThruHole,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadShape {
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

#[derive(Debug, Clone)]
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
    GrPoly { pts: Vec<(f64, f64)>, width: f64, fill: bool },
    GrRect { start: (f64, f64), end: (f64, f64), width: f64, fill: bool },
    GrLine { start: (f64, f64), end: (f64, f64), width: f64 },
    GrCircle { center: (f64, f64), end: (f64, f64), width: f64, fill: bool },
    GrArc { start: (f64, f64), mid: (f64, f64), end: (f64, f64), width: f64 },
    Segment { start: (f64, f64), end: (f64, f64), width: f64 },
}

#[derive(Debug, Clone)]
pub struct DrillDef {
    pub diameter: f64,
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
    Line { start: (f64, f64), end: (f64, f64) },
    Arc { start: (f64, f64), mid: (f64, f64), end: (f64, f64) },
    Circle { center: (f64, f64), end: (f64, f64) },
    Rect { start: (f64, f64), end: (f64, f64) },
    Poly { points: Vec<(f64, f64)> },
    Text { text: String, position: (f64, f64, f64), font_size: f64 },
}

/// Rewrite dialect name-only net refs `(net "NAME")` into `(net ID "NAME")`,
/// allocating ids in `nets` for unseen names. Recursive over the whole tree.
fn normalize_net_name_refs(node: &mut SExpr, nets: &mut Vec<NetDef>) {
    if let SExpr::List(items) = node {
        if items.len() >= 2 {
            if items[0].as_ident() == Some("net") {
                if let SExpr::Atom(Atom::String(name)) = &items[1] {
                    let name = name.clone();
                    let next = nets.iter().map(|n| n.id).max().unwrap_or(0) + 1;
                    let id = nets.iter().find(|n| n.name == name).map(|n| n.id)
                        .unwrap_or_else(|| {
                            nets.push(NetDef { id: next, name: name.clone() });
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
        }
        for child in items.iter_mut() {
            normalize_net_name_refs(child, nets);
        }
    }
}
