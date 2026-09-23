//! Board JSON5 to IR parser

use serde_json::Value;

use crate::error::{Error, Result};
use crate::ir::board::*;

/// Parse a JSON5 string into a Board IR.
pub fn parse_board_json5(source: &str) -> Result<Board> {
    let value: Value =
        serde_json5::from_str(source).map_err(|e| Error::Json5Parse(format!("{}", e)))?;
    value_to_board(&value)
}

fn err(msg: impl Into<String>) -> Error {
    Error::Json5Parse(msg.into())
}

fn get_str<'a>(obj: &'a serde_json::Map<String, Value>, key: &str) -> Option<&'a str> {
    obj.get(key).and_then(|v| v.as_str())
}

fn get_f64(obj: &serde_json::Map<String, Value>, key: &str) -> Option<f64> {
    obj.get(key).and_then(|v| v.as_f64())
}

fn get_obj<'a>(v: &'a Value, key: &str) -> Option<&'a serde_json::Map<String, Value>> {
    v.get(key).and_then(|v| v.as_object())
}

fn get_arr<'a>(v: &'a Value, key: &str) -> Option<&'a Vec<Value>> {
    v.get(key).and_then(|v| v.as_array())
}

fn parse_point(v: &Value) -> Option<(f64, f64)> {
    let arr = v.as_array()?;
    Some((arr.first()?.as_f64()?, arr.get(1)?.as_f64()?))
}

fn parse_position(obj: &serde_json::Map<String, Value>) -> (f64, f64, f64) {
    let x = obj.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let y = obj.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let r = obj.get("rotation").and_then(|v| v.as_f64()).unwrap_or(0.0);
    (x, y, r)
}

fn parse_string_list(v: &Value, key: &str) -> Vec<String> {
    v.get(key)
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|i| i.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

fn parse_point_list(v: &Value, key: &str) -> Vec<(f64, f64)> {
    v.get(key)
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(parse_point).collect())
        .unwrap_or_default()
}

#[allow(clippy::field_reassign_with_default)] // title_block fields assigned stepwise below
fn value_to_board(value: &Value) -> Result<Board> {
    let obj = value
        .as_object()
        .ok_or_else(|| err("Board JSON5 root must be an object"))?;

    let mut board = Board::new();

    board.version = get_str(obj, "version").unwrap_or("20240108").to_string();
    board.generator = get_str(obj, "generator")
        .unwrap_or("kicad-json5")
        .to_string();
    board.paper = get_str(obj, "paper").unwrap_or("A4").to_string();

    // general
    if let Some(gen) = get_obj(value, "general") {
        board.general.thickness = get_f64(gen, "thickness").unwrap_or(1.6);
    }

    // title_block
    if let Some(tb) = get_obj(value, "title_block") {
        let mut title_block = BoardTitleBlock::default();
        title_block.title = get_str(tb, "title").map(|s| s.to_string());
        title_block.date = get_str(tb, "date").map(|s| s.to_string());
        title_block.rev = get_str(tb, "rev").map(|s| s.to_string());
        title_block.company = get_str(tb, "company").map(|s| s.to_string());
        board.title_block = Some(title_block);
    }

    // layers
    if let Some(layers) = get_arr(value, "layers") {
        board.layers.clear();
        for lv in layers {
            if let Some(lo) = lv.as_object() {
                board.layers.push(LayerDef {
                    ordinal: get_f64(lo, "ordinal").unwrap_or(0.0) as u32,
                    name: get_str(lo, "name").unwrap_or("").to_string(),
                    layer_type: get_str(lo, "type").unwrap_or("user").to_string(),
                });
            }
        }
    }

    // nets
    if let Some(nets) = get_arr(value, "nets") {
        board.nets.clear();
        for nv in nets {
            if let Some(no) = nv.as_object() {
                board.nets.push(NetDef {
                    id: get_f64(no, "id").unwrap_or(0.0) as u32,
                    name: get_str(no, "name").unwrap_or("").to_string(),
                });
            }
        }
    }

    // footprints
    if let Some(fps) = get_arr(value, "footprints") {
        for fv in fps {
            board.footprints.push(value_to_footprint(fv));
        }
    }

    // segments
    if let Some(segs) = get_arr(value, "segments") {
        for sv in segs {
            if let Some(so) = sv.as_object() {
                board.segments.push(Segment {
                    start: parse_point(sv.get("start").unwrap_or(&Value::Null))
                        .unwrap_or((0.0, 0.0)),
                    end: parse_point(sv.get("end").unwrap_or(&Value::Null)).unwrap_or((0.0, 0.0)),
                    width: get_f64(so, "width").unwrap_or(0.25),
                    layer: get_str(so, "layer").unwrap_or("F.Cu").to_string(),
                    net: get_f64(so, "net").unwrap_or(0.0) as u32,
                });
            }
        }
    }

    // vias
    if let Some(vias) = get_arr(value, "vias") {
        for vv in vias {
            if let Some(vo) = vv.as_object() {
                let at = parse_point(vv.get("at").unwrap_or(&Value::Null)).unwrap_or((0.0, 0.0));
                board.vias.push(Via {
                    at,
                    size: get_f64(vo, "size").unwrap_or(0.6),
                    drill: get_f64(vo, "drill").unwrap_or(0.3),
                    layers: parse_string_list(vv, "layers"),
                    net: get_f64(vo, "net").unwrap_or(0.0) as u32,
                });
            }
        }
    }

    // zones
    if let Some(zones) = get_arr(value, "zones") {
        for zv in zones {
            board.zones.push(value_to_zone(zv));
        }
    }

    // graphics
    if let Some(graphics) = get_arr(value, "graphics") {
        for gv in graphics {
            if let Some(gr) = value_to_graphic(gv) {
                board.graphics.push(gr);
            }
        }
    }

    Ok(board)
}

fn value_to_footprint(v: &Value) -> Footprint {
    let obj = match v.as_object() {
        Some(o) => o,
        None => return Footprint::new("", "", ""),
    };

    let mut fp = Footprint::new(
        get_str(obj, "lib_id").unwrap_or(""),
        get_str(obj, "reference").unwrap_or(""),
        get_str(obj, "value").unwrap_or(""),
    );

    fp.position = v
        .get("position")
        .and_then(|p| p.as_object())
        .map(parse_position)
        .unwrap_or((0.0, 0.0, 0.0));

    fp.layer = get_str(obj, "layer").unwrap_or("F.Cu").to_string();

    // 标量字段回读 (写端 generate_board_json5 全部输出, 缺读会在 json5 hop 丢失:
    // uuid 丢失触发 sexpr_gen 确定性假 uuid, whole-board 往返后 KiCad 侧 uuid 全换)
    fp.uuid = get_str(obj, "uuid").map(|s| s.to_string());
    if obj.get("locked").and_then(|x| x.as_bool()).unwrap_or(false) {
        fp.locked = true;
    }
    fp.descr = get_str(obj, "descr").map(|s| s.to_string());
    fp.tags = get_str(obj, "tags").map(|s| s.to_string());
    fp.path = get_str(obj, "path").map(|s| s.to_string());
    fp.sheetname = get_str(obj, "sheetname").map(|s| s.to_string());
    fp.sheetfile = get_str(obj, "sheetfile").map(|s| s.to_string());
    fp.solder_mask_margin = get_f64(obj, "solder_mask_margin");
    fp.attr = match get_str(obj, "attr") {
        Some("smd") => Some(FootprintAttr::Smd),
        Some("thru_hole") => Some(FootprintAttr::ThruHole),
        Some("virtual") => Some(FootprintAttr::Virtual),
        Some("board_only") => Some(FootprintAttr::BoardOnly),
        Some(_) => {
            let vals: Vec<String> = v
                .get("attr")
                .and_then(|a| a.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            (!vals.is_empty()).then_some(FootprintAttr::Other(vals))
        }
        None => None,
    };

    // models（3D 模型引用; 兼容旧单数 model 键——无 models 数组时回退读取）
    if let Some(ms) = get_arr(v, "models") {
        for mv in ms {
            if let Some(m) = value_to_model(mv) {
                fp.models.push(m);
            }
        }
    } else if let Some(mv) = v.get("model") {
        if let Some(m) = value_to_model(mv) {
            fp.models.push(m);
        }
    }

    // pads
    if let Some(pads) = get_arr(v, "pads") {
        for pv in pads {
            fp.pads.push(value_to_pad(pv));
        }
    }

    // fp_lines
    if let Some(lines) = get_arr(v, "fp_lines") {
        for lv in lines {
            if let Some(lo) = lv.as_object() {
                fp.fp_lines.push(FpLine {
                    start: parse_point(lv.get("start").unwrap_or(&Value::Null))
                        .unwrap_or((0.0, 0.0)),
                    end: parse_point(lv.get("end").unwrap_or(&Value::Null)).unwrap_or((0.0, 0.0)),
                    stroke_width: get_f64(lo, "stroke").unwrap_or(0.12),
                    layer: get_str(lo, "layer").unwrap_or("F.SilkS").to_string(),
                });
            }
        }
    }

    // fp_circles
    if let Some(circles) = get_arr(v, "fp_circles") {
        for cv in circles {
            if let Some(co) = cv.as_object() {
                fp.fp_circles.push(FpCircle {
                    center: parse_point(cv.get("center").unwrap_or(&Value::Null))
                        .unwrap_or((0.0, 0.0)),
                    end: parse_point(cv.get("end").unwrap_or(&Value::Null)).unwrap_or((0.0, 0.0)),
                    stroke_width: get_f64(co, "stroke").unwrap_or(0.12),
                    layer: get_str(co, "layer").unwrap_or("F.SilkS").to_string(),
                    fill: co.get("fill").and_then(|v| v.as_bool()).unwrap_or(false),
                });
            }
        }
    }

    // fp_arcs
    if let Some(arcs) = get_arr(v, "fp_arcs") {
        for av in arcs {
            if let Some(ao) = av.as_object() {
                fp.fp_arcs.push(FpArc {
                    start: parse_point(av.get("start").unwrap_or(&Value::Null))
                        .unwrap_or((0.0, 0.0)),
                    mid: parse_point(av.get("mid").unwrap_or(&Value::Null)).unwrap_or((0.0, 0.0)),
                    end: parse_point(av.get("end").unwrap_or(&Value::Null)).unwrap_or((0.0, 0.0)),
                    stroke_width: get_f64(ao, "stroke").unwrap_or(0.12),
                    layer: get_str(ao, "layer").unwrap_or("F.SilkS").to_string(),
                });
            }
        }
    }

    // fp_rects
    if let Some(rects) = get_arr(v, "fp_rects") {
        for rv in rects {
            if let Some(ro) = rv.as_object() {
                fp.fp_rects.push(FpRect {
                    start: parse_point(rv.get("start").unwrap_or(&Value::Null))
                        .unwrap_or((0.0, 0.0)),
                    end: parse_point(rv.get("end").unwrap_or(&Value::Null)).unwrap_or((0.0, 0.0)),
                    stroke_width: get_f64(ro, "stroke").unwrap_or(0.12),
                    layer: get_str(ro, "layer").unwrap_or("F.SilkS").to_string(),
                    fill: ro.get("fill").and_then(|v| v.as_bool()).unwrap_or(false),
                });
            }
        }
    }

    // fp_polys
    if let Some(polys) = get_arr(v, "fp_polys") {
        for pv in polys {
            if let Some(po) = pv.as_object() {
                let points = parse_point_list(pv, "points");
                if !points.is_empty() {
                    fp.fp_polys.push(FpPoly {
                        points,
                        stroke_width: get_f64(po, "stroke").unwrap_or(0.12),
                        layer: get_str(po, "layer").unwrap_or("F.SilkS").to_string(),
                        fill: po.get("fill").and_then(|v| v.as_bool()).unwrap_or(false),
                    });
                }
            }
        }
    }

    // fp_texts (P1-10: added alongside the exporter's fp_texts emission)
    if let Some(texts) = get_arr(v, "fp_texts") {
        for tv in texts {
            if let Some(to) = tv.as_object() {
                let text = get_str(to, "text").unwrap_or("").to_string();
                let ty = match get_str(to, "type").unwrap_or("user") {
                    "reference" => FpTextType::Reference,
                    "value" => FpTextType::Value,
                    _ => FpTextType::User,
                };
                let position = to
                    .get("at")
                    .and_then(|a| a.as_array())
                    .map(|a| {
                        (
                            a.first().and_then(|x| x.as_f64()).unwrap_or(0.0),
                            a.get(1).and_then(|y| y.as_f64()).unwrap_or(0.0),
                            a.get(2).and_then(|r| r.as_f64()).unwrap_or(0.0),
                        )
                    })
                    .unwrap_or((0.0, 0.0, 0.0));
                let sz = get_f64(to, "size").unwrap_or(1.0);
                fp.fp_texts.push(FpText {
                    text,
                    text_type: ty,
                    position,
                    layer: get_str(to, "layer").unwrap_or("F.SilkS").to_string(),
                    font_size: (sz, sz),
                });
            }
        }
    }

    // properties (full-fidelity blocks — Reference/Value/Datasheet/Description;
    // without this the json5 hop drops them and the writer falls back to legacy
    // fp_text synthesis, which KiCad flags as lib_footprint_mismatch)
    if let Some(props) = get_arr(v, "properties") {
        for pv in props {
            if let Some(po) = pv.as_object() {
                let position = po
                    .get("position")
                    .and_then(|a| a.as_object())
                    .map(|p| {
                        (
                            p.get("x").and_then(|x| x.as_f64()).unwrap_or(0.0),
                            p.get("y").and_then(|y| y.as_f64()).unwrap_or(0.0),
                            p.get("rotation").and_then(|r| r.as_f64()).unwrap_or(0.0),
                        )
                    })
                    .unwrap_or((0.0, 0.0, 0.0));
                let effects = po.get("effects").and_then(|e| e.as_object());
                let font_size = effects
                    .and_then(|e| e.get("font_size"))
                    .and_then(|a| a.as_array())
                    .map(|a| {
                        (
                            a.first().and_then(|x| x.as_f64()).unwrap_or(1.0),
                            a.get(1).and_then(|y| y.as_f64()).unwrap_or(1.0),
                        )
                    })
                    .unwrap_or((1.0, 1.0));
                fp.properties_ext.push(FpProperty {
                    name: get_str(po, "name").unwrap_or("").to_string(),
                    value: get_str(po, "value").unwrap_or("").to_string(),
                    position,
                    layer: get_str(po, "layer").unwrap_or("F.Fab").to_string(),
                    hide: po.get("hide").and_then(|x| x.as_bool()).unwrap_or(false),
                    unlocked: po
                        .get("unlocked")
                        .and_then(|x| x.as_bool())
                        .unwrap_or(false),
                    effects: FpPropertyEffects {
                        font_size,
                        font_thickness: effects
                            .and_then(|e| e.get("thickness"))
                            .and_then(|x| x.as_f64())
                            .unwrap_or(0.15),
                        bold: effects
                            .and_then(|e| e.get("bold"))
                            .and_then(|x| x.as_bool())
                            .unwrap_or(false),
                        italic: effects
                            .and_then(|e| e.get("italic"))
                            .and_then(|x| x.as_bool())
                            .unwrap_or(false),
                    },
                    uuid: None,
                });
            }
        }
    }

    fp
}

/// models 数组元素 → Model3D。子字段可缺省, 缺省按 KiCad 默认
/// (offset 0,0,0 / scale 1,1,1 / rotate 0,0,0); path 含 ${KIPRJMOD} 变量原样保真。
fn value_to_model(v: &Value) -> Option<Model3D> {
    let obj = v.as_object()?;
    let xyz = |key: &str, dflt: (f64, f64, f64)| -> (f64, f64, f64) {
        obj.get(key)
            .and_then(|a| a.as_array())
            .map(|a| {
                (
                    a.first().and_then(|x| x.as_f64()).unwrap_or(dflt.0),
                    a.get(1).and_then(|x| x.as_f64()).unwrap_or(dflt.1),
                    a.get(2).and_then(|x| x.as_f64()).unwrap_or(dflt.2),
                )
            })
            .unwrap_or(dflt)
    };
    Some(Model3D {
        path: get_str(obj, "path")?.to_string(),
        offset: xyz("offset", (0.0, 0.0, 0.0)),
        scale: xyz("scale", (1.0, 1.0, 1.0)),
        rotate: xyz("rotate", (0.0, 0.0, 0.0)),
    })
}

fn value_to_pad(v: &Value) -> Pad {
    let obj = match v.as_object() {
        Some(o) => o,
        None => {
            return Pad {
                number: String::new(),
                pad_type: PadType::Smd,
                shape: PadShape::Rect,
                position: (0.0, 0.0, 0.0),
                size: (0.5, 0.5),
                layers: Vec::new(),
                drill: None,
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
            }
        }
    };

    let pad_type = match get_str(obj, "type").unwrap_or("smd") {
        "thru_hole" => PadType::ThruHole,
        "connect" => PadType::Connect,
        "np_thru_hole" => PadType::NpThruHole,
        _ => PadType::Smd,
    };

    let shape = match get_str(obj, "shape").unwrap_or("rect") {
        "circle" => PadShape::Circle,
        "roundrect" => PadShape::RoundRect,
        "oval" => PadShape::Oval,
        "trapezoid" => PadShape::Trapezoid,
        "custom" => PadShape::Custom,
        _ => PadShape::Rect,
    };

    let position = v
        .get("position")
        .and_then(|p| p.as_object())
        .map(parse_position)
        .unwrap_or((0.0, 0.0, 0.0));

    let size = v.get("size").and_then(parse_point).unwrap_or((0.5, 0.5));

    let drill = match v.get("drill") {
        Some(Value::Number(n)) => Some(DrillDef {
            diameter: n.as_f64().unwrap_or(0.0),
            width: None,
            offset: None,
        }),
        Some(dv) => {
            let dobj = dv.as_object();
            Some(DrillDef {
                diameter: dobj.and_then(|o| get_f64(o, "diameter")).unwrap_or(0.0),
                width: dobj.and_then(|o| get_f64(o, "width")),
                offset: dobj.and_then(|o| o.get("offset")).and_then(parse_point),
            })
        }
        None => None,
    };

    Pad {
        number: get_str(obj, "number").unwrap_or("").to_string(),
        pad_type,
        shape,
        position,
        size,
        layers: parse_string_list(v, "layers"),
        drill,
        net: get_f64(obj, "net").map(|n| n as u32),
        net_name: get_str(obj, "net_name").map(|s| s.to_string()),
        pin_function: get_str(obj, "pin_function").map(|s| s.to_string()),
        pin_type: get_str(obj, "pin_type").map(|s| s.to_string()),
        roundrect_rratio: get_f64(obj, "roundrect_rratio"),
        solder_mask_margin: get_f64(obj, "solder_mask_margin"),
        thermal_bridge_width: get_f64(obj, "thermal_bridge_width"),
        thermal_bridge_angle: get_f64(obj, "thermal_bridge_angle"),
        thermal_gap: get_f64(obj, "thermal_gap"),
        clearance: get_f64(obj, "clearance"),
        zone_connect: get_f64(obj, "zone_connect").map(|n| n as i32),
        remove_unused_layers: v.get("remove_unused_layers").and_then(|x| x.as_bool()),
        options: v
            .get("options")
            .and_then(|o| o.as_object())
            .map(|o| PadOptions {
                clearance: get_str(o, "clearance").map(|s| s.to_string()),
                anchor: get_str(o, "anchor").map(|s| s.to_string()),
            }),
        primitives: value_to_pad_primitives(v.get("primitives")),
    }
}

fn value_to_pad_primitives(v: Option<&Value>) -> Vec<PadPrimitive> {
    let mut out = Vec::new();
    let arr = match v.and_then(|x| x.as_array()) {
        Some(a) => a,
        None => return out,
    };
    for pv in arr {
        let obj = match pv.as_object() {
            Some(o) => o,
            None => continue,
        };
        let pt = |k: &str| obj.get(k).and_then(parse_point);
        let width = || get_f64(obj, "width").unwrap_or(0.0);
        let fill = || obj.get("fill").and_then(|f| f.as_bool()).unwrap_or(false);
        match get_str(obj, "kind").unwrap_or("") {
            "gr_poly" => {
                let pts = parse_point_list(pv, "points");
                if !pts.is_empty() {
                    out.push(PadPrimitive::GrPoly {
                        pts,
                        width: width(),
                        fill: fill(),
                    });
                }
            }
            "gr_rect" => {
                if let (Some(s), Some(e)) = (pt("start"), pt("end")) {
                    out.push(PadPrimitive::GrRect {
                        start: s,
                        end: e,
                        width: width(),
                        fill: fill(),
                    });
                }
            }
            "gr_line" => {
                if let (Some(s), Some(e)) = (pt("start"), pt("end")) {
                    out.push(PadPrimitive::GrLine {
                        start: s,
                        end: e,
                        width: width(),
                    });
                }
            }
            "gr_circle" => {
                if let (Some(c), Some(e)) = (pt("center"), pt("end")) {
                    out.push(PadPrimitive::GrCircle {
                        center: c,
                        end: e,
                        width: width(),
                        fill: fill(),
                    });
                }
            }
            "gr_arc" => {
                if let (Some(s), Some(m), Some(e)) = (pt("start"), pt("mid"), pt("end")) {
                    out.push(PadPrimitive::GrArc {
                        start: s,
                        mid: m,
                        end: e,
                        width: width(),
                    });
                }
            }
            "segment" => {
                if let (Some(s), Some(e)) = (pt("start"), pt("end")) {
                    out.push(PadPrimitive::Segment {
                        start: s,
                        end: e,
                        width: width(),
                    });
                }
            }
            _ => {}
        }
    }
    out
}

fn value_to_zone(v: &Value) -> Zone {
    let obj = v.as_object();

    let net = obj.and_then(|o| get_f64(o, "net")).unwrap_or(0.0) as u32;
    let net_name = obj
        .and_then(|o| get_str(o, "net_name"))
        .unwrap_or("")
        .to_string();
    let layer = obj
        .and_then(|o| get_str(o, "layer"))
        .unwrap_or("F.Cu")
        .to_string();

    let (hatch_style, hatch_pitch) = v
        .get("hatch")
        .and_then(|h| h.as_object())
        .map(|ho| {
            (
                get_str(ho, "style").unwrap_or("edge").to_string(),
                get_f64(ho, "pitch").unwrap_or(0.508),
            )
        })
        .unwrap_or(("edge".into(), 0.508));

    Zone {
        net,
        net_name,
        layer,
        hatch_style,
        hatch_pitch,
        pad_connect: obj
            .and_then(|o| get_str(o, "pad_connect"))
            .unwrap_or("")
            .to_string(),
        connect_pads_clearance: obj.and_then(|o| get_f64(o, "clearance")).unwrap_or(0.2),
        min_thickness: obj
            .and_then(|o| get_f64(o, "min_thickness"))
            .unwrap_or(0.254),
        fill: obj
            .and_then(|o| o.get("fill").and_then(|v| v.as_bool()))
            .unwrap_or(false),
        thermal_gap: obj.and_then(|o| get_f64(o, "thermal_gap")).unwrap_or(0.508),
        thermal_bridge_width: obj
            .and_then(|o| get_f64(o, "thermal_bridge_width"))
            .unwrap_or(0.508),
        island_removal_mode: obj
            .and_then(|o| get_f64(o, "island_removal_mode"))
            .unwrap_or(0.0) as i32,
        island_area: obj.and_then(|o| get_f64(o, "island_area")).unwrap_or(10.0),
        outline: parse_point_list(v, "outline"),
        filled_polygons: parse_filled_polygons(v),
        keepout: None,
    }
}

fn parse_filled_polygons(v: &Value) -> Vec<FilledPolygon> {
    v.get("filled_polygons")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|fv| {
                    let obj = fv.as_object()?;
                    let layer = get_str(obj, "layer").unwrap_or("F.Cu").to_string();
                    let points = parse_point_list(fv, "points");
                    if points.is_empty() {
                        return None;
                    }
                    Some(FilledPolygon { layer, points })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn value_to_graphic(v: &Value) -> Option<BoardGraphic> {
    let obj = v.as_object()?;
    // P0-9 fix: exporter historically wrote "kind", parser only read "type" —
    // every board graphic (incl. Edge.Cuts!) silently vanished on json5 reparse.
    // Accept both keys; exports now write "type".
    let gtype = get_str(obj, "type").or_else(|| get_str(obj, "kind"))?;
    let layer = get_str(obj, "layer").unwrap_or("Edge.Cuts").to_string();
    let stroke_width = get_f64(obj, "stroke").unwrap_or(0.15);

    let kind = match gtype {
        "line" => {
            let start = parse_point(v.get("start")?).unwrap_or((0.0, 0.0));
            let end = parse_point(v.get("end")?).unwrap_or((0.0, 0.0));
            BoardGraphicKind::Line { start, end }
        }
        "arc" => {
            let start = parse_point(v.get("start")?).unwrap_or((0.0, 0.0));
            let mid = parse_point(v.get("mid")?).unwrap_or((0.0, 0.0));
            let end = parse_point(v.get("end")?).unwrap_or((0.0, 0.0));
            BoardGraphicKind::Arc { start, mid, end }
        }
        "circle" => {
            let center = parse_point(v.get("center")?).unwrap_or((0.0, 0.0));
            let end = parse_point(v.get("end")?).unwrap_or((0.0, 0.0));
            BoardGraphicKind::Circle { center, end }
        }
        "rect" => {
            let start = parse_point(v.get("start")?).unwrap_or((0.0, 0.0));
            let end = parse_point(v.get("end")?).unwrap_or((0.0, 0.0));
            BoardGraphicKind::Rect { start, end }
        }
        "poly" => {
            let points = parse_point_list(v, "points");
            BoardGraphicKind::Poly { points }
        }
        "text" => {
            let text = get_str(obj, "text").unwrap_or("").to_string();
            let position = v
                .get("position")
                .and_then(|p| p.as_object())
                .map(parse_position)
                .unwrap_or((0.0, 0.0, 0.0));
            let font_size = get_f64(obj, "font_size").unwrap_or(1.0);
            BoardGraphicKind::Text {
                text,
                position,
                font_size,
            }
        }
        _ => return None,
    };

    Some(BoardGraphic {
        kind,
        layer,
        stroke_width,
        fill: obj.get("fill").and_then(|v| v.as_bool()).unwrap_or(false),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_empty_board() {
        let json5 = "{ version: \"20240108\", generator: \"test\" }";
        let board = parse_board_json5(json5).unwrap();
        assert_eq!(board.version, "20240108");
        assert_eq!(board.generator, "test");
    }

    #[test]
    fn test_parse_board_with_nets() {
        let json5 = r#"{
            nets: [
                { id: 0, name: "" },
                { id: 1, name: "+5V" },
                { id: 2, name: "GND" }
            ]
        }"#;
        let board = parse_board_json5(json5).unwrap();
        assert_eq!(board.nets.len(), 3);
        assert_eq!(board.nets[1].name, "+5V");
    }

    #[test]
    fn test_parse_footprint_with_pad() {
        let json5 = r#"{
            footprints: [{
                lib_id: "Capacitor_SMD:C_0402",
                reference: "C1",
                value: "100nF",
                position: { x: 10.5, y: 20.0, rotation: 90.0 },
                layer: "F.Cu",
                pads: [{
                    number: "1",
                    type: "smd",
                    shape: "roundrect",
                    position: { x: -0.37, y: 0, rotation: 0 },
                    size: [0.74, 0.62],
                    layers: ["F.Cu", "F.Paste", "F.Mask"],
                    net: 1
                }]
            }]
        }"#;
        let board = parse_board_json5(json5).unwrap();
        assert_eq!(board.footprints.len(), 1);
        let fp = &board.footprints[0];
        assert_eq!(fp.lib_id, "Capacitor_SMD:C_0402");
        assert_eq!(fp.position, (10.5, 20.0, 90.0));
        assert_eq!(fp.pads.len(), 1);
        assert_eq!(fp.pads[0].number, "1");
        assert_eq!(fp.pads[0].shape, PadShape::RoundRect);
        assert_eq!(fp.pads[0].net, Some(1));
    }

    #[test]
    fn test_parse_pad_optional_fields() {
        let json5 = r#"{
            footprints: [{
                lib_id: "Test:CustomPad",
                reference: "U1",
                value: "TP",
                position: { x: 0, y: 0, rotation: 0 },
                layer: "F.Cu",
                pads: [{
                    number: "1",
                    type: "thru_hole",
                    shape: "custom",
                    position: { x: 0, y: 0, rotation: 0 },
                    size: [1.0, 1.0],
                    layers: ["*.Cu", "F.Mask"],
                    drill: { diameter: 0.8, offset: [0.1, -0.2] },
                    net: 3,
                    net_name: "GND",
                    pin_function: "passive",
                    pin_type: "thermal",
                    thermal_bridge_width: 0.3,
                    thermal_bridge_angle: 90.0,
                    thermal_gap: 0.4,
                    clearance: 0.25,
                    zone_connect: 2,
                    remove_unused_layers: false,
                    options: { clearance: "outline", anchor: "circle" },
                    primitives: [
                        { kind: "gr_poly", points: [[-1, -1], [1, -1], [1, 1], [-1, 1]], width: 0, fill: true },
                        { kind: "gr_circle", center: [0, 0], end: [0.5, 0], width: 0.05, fill: false },
                        { kind: "gr_arc", start: [1, 0], mid: [0.7, 0.7], end: [0, 1], width: 0.1 },
                        { kind: "segment", start: [-0.5, 0], end: [0.5, 0], width: 0.1 }
                    ]
                }]
            }]
        }"#;
        let board = parse_board_json5(json5).unwrap();
        let pad = &board.footprints[0].pads[0];
        assert_eq!(pad.pad_type, PadType::ThruHole);
        assert_eq!(pad.shape, PadShape::Custom);
        let drill = pad.drill.as_ref().unwrap();
        assert_eq!(drill.diameter, 0.8);
        assert_eq!(drill.offset, Some((0.1, -0.2)));
        assert_eq!(pad.net, Some(3));
        assert_eq!(pad.net_name.as_deref(), Some("GND"));
        assert_eq!(pad.pin_function.as_deref(), Some("passive"));
        assert_eq!(pad.pin_type.as_deref(), Some("thermal"));
        assert_eq!(pad.thermal_bridge_width, Some(0.3));
        assert_eq!(pad.thermal_bridge_angle, Some(90.0));
        assert_eq!(pad.thermal_gap, Some(0.4));
        assert_eq!(pad.clearance, Some(0.25));
        assert_eq!(pad.zone_connect, Some(2));
        assert_eq!(pad.remove_unused_layers, Some(false));
        let opts = pad.options.as_ref().unwrap();
        assert_eq!(opts.clearance.as_deref(), Some("outline"));
        assert_eq!(opts.anchor.as_deref(), Some("circle"));
        assert_eq!(pad.primitives.len(), 4);
        assert!(matches!(
            &pad.primitives[0],
            PadPrimitive::GrPoly { pts, width, fill }
            if pts.len() == 4 && *width == 0.0 && *fill
        ));
        assert!(matches!(
            &pad.primitives[3],
            PadPrimitive::Segment { start, end, width }
            if *start == (-0.5, 0.0) && *end == (0.5, 0.0) && *width == 0.1
        ));
    }

    #[test]
    fn test_parse_segments_and_graphics() {
        let json5 = r#"{
            segments: [
                { start: [10.0, 20.0], end: [30.0, 20.0], width: 0.5, layer: "F.Cu", net: 1 }
            ],
            graphics: [
                { type: "line", start: [0, 0], end: [50, 0], stroke: 0.15, layer: "Edge.Cuts" },
                { type: "circle", center: [25, 15], end: [30, 15], stroke: 0.1, layer: "Edge.Cuts" }
            ]
        }"#;
        let board = parse_board_json5(json5).unwrap();
        assert_eq!(board.segments.len(), 1);
        assert_eq!(board.segments[0].width, 0.5);
        assert_eq!(board.graphics.len(), 2);
    }

    // Task 4 Phase 2: json5 的 models 数组 + 旧单数 model 键都要落到 fp.models
    #[test]
    fn test_parse_models_array_and_legacy_key() {
        let json5 = r#"{
            footprints: [
                {
                    lib_id: "Test:A", reference: "A1", value: "x", layer: "F.Cu",
                    models: [
                        { path: "${KIPRJMOD}/3d/A.wrl", offset: [0, 0, 1.25], scale: [1, 1, 1], rotate: [0, 0, 0] },
                        { path: "b.wrl" }
                    ]
                },
                {
                    lib_id: "Test:B", reference: "B1", value: "x", layer: "F.Cu",
                    model: { path: "${KIPRJMOD}/3d/B.wrl", offset: [0, 0, 0.6], scale: [1, 1, 1], rotate: [0, 0, 0] }
                }
            ]
        }"#;
        let board = parse_board_json5(json5).unwrap();
        let a = &board.footprints[0];
        assert_eq!(a.models.len(), 2);
        assert_eq!(a.models[0].path, "${KIPRJMOD}/3d/A.wrl");
        assert_eq!(a.models[0].offset, (0.0, 0.0, 1.25));
        // 缺省子字段 → KiCad 默认值
        assert_eq!(a.models[1].offset, (0.0, 0.0, 0.0));
        assert_eq!(a.models[1].scale, (1.0, 1.0, 1.0));
        assert_eq!(a.models[1].rotate, (0.0, 0.0, 0.0));
        // 旧单数 model 键兼容读取
        let b = &board.footprints[1];
        assert_eq!(b.models.len(), 1);
        assert_eq!(b.models[0].path, "${KIPRJMOD}/3d/B.wrl");
        assert_eq!(b.models[0].offset, (0.0, 0.0, 0.6));
    }

    #[test]
    #[ignore = "generate_board_json5 not yet implemented — re-enable when Json5Generator::generate_board exists"]
    fn test_round_trip_simple() {
        // This test requires Json5Generator::generate_board() which is not yet implemented.
        // When generate_board_json5 is properly implemented, re-enable this test body:
        /*
        let mut board = Board::new();
        board.add_net("GND");
        let vcc = board.add_net("+3V3");
        board.footprints.push(Footprint::new("Device:R", "R1", "10k"));
        board.footprints[0].position = (10.0, 20.0, 0.0);
        board.footprints[0].pads.push(Pad {
            number: "1".into(), pad_type: PadType::Smd, shape: PadShape::RoundRect,
            position: (-0.5, 0.0, 0.0), size: (0.6, 0.8),
            layers: vec!["F.Cu".into()], drill: None, net: Some(vcc),
            net_name: None, pin_function: None, pin_type: None, roundrect_rratio: None,
            solder_mask_margin: None, thermal_bridge_width: None, thermal_bridge_angle: None,
            thermal_gap: None, clearance: None, zone_connect: None,
        });
        board.segments.push(Segment {
            start: (10.0, 20.0), end: (20.0, 20.0),
            width: 0.25, layer: "F.Cu".into(), net: vcc,
        });

        // IR → JSON5 → IR
        let gen = crate::codegen::Json5Generator::new();
        let json5 = gen.generate_board(&board).unwrap();
        let board2 = parse_board_json5(&json5).unwrap();

        assert_eq!(board2.nets.len(), board.nets.len());
        assert_eq!(board2.footprints.len(), board.footprints.len());
        assert_eq!(board2.footprints[0].reference, "R1");
        assert_eq!(board2.footprints[0].pads[0].net, Some(vcc));
        assert_eq!(board2.segments.len(), 1);
        assert_eq!(board2.segments[0].net, vcc);
        */
    }
}
