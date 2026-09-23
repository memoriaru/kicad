//! KiCad S-expression / JSON5 bidirectional compiler
//!
//! This crate provides tools to convert KiCad schematic files (.kicad_sch)
//! between S-expression and JSON5 formats, and extract circuit topology
//! for AI-friendly semantic analysis.

pub mod codegen;
pub mod dialect;
pub mod error;
pub mod hierarchy;
pub mod ir;
pub mod layout;
pub mod lexer;
pub mod parser;
pub mod topology;

pub use codegen::{BoardSexprConfig, BoardSexprGenerator, Json5Config, Json5Generator, KicadVersion, SexprConfig, SexprGenerator};
pub use error::{Error, Result};
pub use ir::Board;
pub use ir::Schematic;
pub use lexer::Lexer;
pub use parser::{parse_board_json5, parse_json5, Parser};

use std::path::Path;

/// Detect input format from file extension
pub fn detect_input_format(path: &Path) -> InputFormat {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .as_deref()
    {
        Some("json5" | "json") => InputFormat::Json5,
        Some("kicad_pcb") => InputFormat::PcbSexpr,
        _ => InputFormat::Sexpr,
    }
}

/// Input file format
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputFormat {
    Sexpr,
    PcbSexpr,
    Json5,
}

/// Parse a schematic from source string, auto-detecting format
pub fn parse_schematic(source: &str, format: InputFormat) -> Result<Schematic> {
    match format {
        InputFormat::Sexpr | InputFormat::PcbSexpr => {
            let lexer = Lexer::new(source);
            let mut parser = Parser::new(lexer);
            parser.parse()
        }
        InputFormat::Json5 => parse_json5(source),
    }
}

/// Convert a KiCad schematic file to JSON5
pub fn convert_file(input: &Path, output: &Path) -> Result<()> {
    let source = std::fs::read_to_string(input)?;
    let json5 = convert_str(&source)?;
    std::fs::write(output, json5)?;
    Ok(())
}

/// Convert a KiCad schematic string to JSON5
pub fn convert_str(source: &str) -> Result<String> {
    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer);
    let schematic = parser.parse()?;
    let generator = Json5Generator::new();
    generator.generate(&schematic)
}

/// Convert a PCB file between formats (.kicad_pcb ↔ JSON5)
pub fn convert_board_file(input: &Path, output: &Path) -> Result<()> {
    let source = std::fs::read_to_string(input)?;
    let board = parse_board(&source)?;
    let ext = output.extension().and_then(|e| e.to_str()).unwrap_or("");
    if ext == "json5" || ext == "json" {
        let json5 = generate_board_json5(&board)?;
        std::fs::write(output, json5)?;
    } else {
        let mut gen = BoardSexprGenerator::new();
        let sexpr = gen.generate(&board)?;
        std::fs::write(output, sexpr)?;
    }
    Ok(())
}

/// Convert a .kicad_pcb string to JSON5, or a JSON5 board string to .kicad_pcb
pub fn convert_board_str(source: &str, format: InputFormat) -> Result<String> {
    match format {
        InputFormat::PcbSexpr | InputFormat::Sexpr => {
            let board = parse_board(source)?;
            generate_board_json5(&board)
        }
        InputFormat::Json5 => {
            let board = parse_board_json5(source)?;
            let mut gen = BoardSexprGenerator::new();
            gen.generate(&board)
        }
    }
}

/// Parse a `.kicad_pcb` S-expression string into a Board IR
pub fn parse_board(source: &str) -> Result<ir::Board> {
    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer);
    let sexpr = parser.parse_sexpr()?;
    ir::Board::from_sexpr(&sexpr)
}

/// Generate a JSON5 string from a Board IR
pub fn generate_board_json5(board: &ir::Board) -> Result<String> {
    let mut s = String::new();
    s.push_str("{\n");

    // version & generator
    s.push_str(&format!("  version: \"{}\",\n", board.version));
    s.push_str(&format!("  generator: \"{}\",\n", board.generator));

    // general
    s.push_str("  general: {\n");
    s.push_str(&format!("    thickness: {},\n", board.general.thickness));
    s.push_str("  },\n");

    // paper
    s.push_str(&format!("  paper: \"{}\",\n", board.paper));

    // title_block
    if let Some(ref tb) = board.title_block {
        s.push_str("  title_block: {\n");
        if let Some(ref v) = tb.title { s.push_str(&format!("    title: \"{}\",\n", escape_json5(v))); }
        if let Some(ref v) = tb.date { s.push_str(&format!("    date: \"{}\",\n", v)); }
        if let Some(ref v) = tb.rev { s.push_str(&format!("    rev: \"{}\",\n", v)); }
        if let Some(ref v) = tb.company { s.push_str(&format!("    company: \"{}\",\n", escape_json5(v))); }
        s.push_str("  },\n");
    }

    // layers
    s.push_str("  layers: [\n");
    for layer in &board.layers {
        s.push_str(&format!("    {{ ordinal: {}, name: \"{}\", type: \"{}\" }},\n",
            layer.ordinal, layer.name, layer.layer_type));
    }
    s.push_str("  ],\n");

    // nets
    s.push_str("  nets: [\n");
    for net in &board.nets {
        s.push_str(&format!("    {{ id: {}, name: \"{}\" }},\n", net.id, escape_json5(&net.name)));
    }
    s.push_str("  ],\n");

    // footprints
    if !board.footprints.is_empty() {
        s.push_str("  footprints: [\n");
        for fp in &board.footprints {
            s.push_str("    {\n");
            s.push_str(&format!("      lib_id: \"{}\",\n", escape_json5(&fp.lib_id)));
            s.push_str(&format!("      reference: \"{}\",\n", escape_json5(&fp.reference)));
            s.push_str(&format!("      value: \"{}\",\n", escape_json5(&fp.value)));
            s.push_str(&format!("      position: {{ x: {}, y: {}, rotation: {} }},\n",
                fp.position.0, fp.position.1, fp.position.2));
            s.push_str(&format!("      layer: \"{}\",\n", fp.layer));
            if let Some(ref uuid) = fp.uuid {
                s.push_str(&format!("      uuid: \"{}\",\n", uuid));
            }
            if fp.locked {
                s.push_str("      locked: true,\n");
            }
            if let Some(ref descr) = fp.descr {
                s.push_str(&format!("      descr: \"{}\",\n", escape_json5(descr)));
            }
            if let Some(ref tags) = fp.tags {
                s.push_str(&format!("      tags: \"{}\",\n", escape_json5(tags)));
            }
            if let Some(ref path) = fp.path {
                s.push_str(&format!("      path: \"{}\",\n", path));
            }
            if let Some(ref sn) = fp.sheetname {
                s.push_str(&format!("      sheetname: \"{}\",\n", escape_json5(sn)));
            }
            if let Some(ref sf) = fp.sheetfile {
                s.push_str(&format!("      sheetfile: \"{}\",\n", escape_json5(sf)));
            }
            if let Some(ref attr) = fp.attr {
                let attr_str = match attr {
                    ir::board::FootprintAttr::Smd => "\"smd\"".into(),
                    ir::board::FootprintAttr::ThruHole => "\"thru_hole\"".into(),
                    ir::board::FootprintAttr::Virtual => "\"virtual\"".into(),
                    ir::board::FootprintAttr::BoardOnly => "\"board_only\"".into(),
                    ir::board::FootprintAttr::Other(vals) => format!("[{}]", vals.iter().map(|v| format!("\"{}\"", v)).collect::<Vec<_>>().join(", ")),
                };
                s.push_str(&format!("      attr: {},\n", attr_str));
            }
            if let Some(margin) = fp.solder_mask_margin {
                s.push_str(&format!("      solder_mask_margin: {},\n", margin));
            }
            if !fp.models.is_empty() {
                s.push_str("      models: [\n");
                for model in &fp.models {
                    s.push_str(&format!("        {{ path: \"{}\", offset: [{}, {}, {}], scale: [{}, {}, {}], rotate: [{}, {}, {}] }},\n",
                        escape_json5(&model.path),
                        model.offset.0, model.offset.1, model.offset.2,
                        model.scale.0, model.scale.1, model.scale.2,
                        model.rotate.0, model.rotate.1, model.rotate.2));
                }
                s.push_str("      ],\n");
            }

            // properties (full format with effects)
            if !fp.properties_ext.is_empty() {
                s.push_str("      properties: [\n");
                for prop in &fp.properties_ext {
                    s.push_str("        {\n");
                    s.push_str(&format!("          name: \"{}\",\n", escape_json5(&prop.name)));
                    s.push_str(&format!("          value: \"{}\",\n", escape_json5(&prop.value)));
                    s.push_str(&format!("          position: {{ x: {}, y: {}, rotation: {} }},\n",
                        prop.position.0, prop.position.1, prop.position.2));
                    s.push_str(&format!("          layer: \"{}\",\n", prop.layer));
                    if prop.hide { s.push_str("          hide: true,\n"); }
                    if prop.unlocked { s.push_str("          unlocked: true,\n"); }
                    s.push_str(&format!("          effects: {{ font_size: [{}, {}], thickness: {}{}{} }},\n",
                        prop.effects.font_size.0, prop.effects.font_size.1,
                        prop.effects.font_thickness,
                        if prop.effects.bold { ", bold: true" } else { "" },
                        if prop.effects.italic { ", italic: true" } else { "" },
                    ));
                    s.push_str("        },\n");
                }
                s.push_str("      ],\n");
            }

            // pads
            if !fp.pads.is_empty() {
                s.push_str("      pads: [\n");
                for pad in &fp.pads {
                    s.push_str("        {\n");
                    s.push_str(&format!("          number: \"{}\",\n", escape_json5(&pad.number)));
                    s.push_str(&format!("          type: \"{}\",\n", pad_type_str(&pad.pad_type)));
                    s.push_str(&format!("          shape: \"{}\",\n", pad_shape_str(&pad.shape)));
                    s.push_str(&format!("          position: {{ x: {}, y: {}, rotation: {} }},\n",
                        pad.position.0, pad.position.1, pad.position.2));
                    s.push_str(&format!("          size: [{}, {}],\n", pad.size.0, pad.size.1));
                    s.push_str(&format!("          layers: [{}],\n",
                        pad.layers.iter().map(|l| format!("\"{}\"", l)).collect::<Vec<_>>().join(", ")));
                    match &pad.drill {
                        Some(d) if d.offset.is_some() => {
                            let (ox, oy) = d.offset.unwrap();
                            s.push_str(&format!("          drill: {{ diameter: {}, offset: [{}, {}] }},\n", d.diameter, ox, oy));
                        }
                        Some(d) => {
                            s.push_str(&format!("          drill: {},\n", d.diameter));
                        }
                        None => {}
                    }
                    if let Some(net) = pad.net {
                        s.push_str(&format!("          net: {},\n", net));
                    }
                    if let Some(ref nn) = pad.net_name {
                        s.push_str(&format!("          net_name: \"{}\",\n", escape_json5(nn)));
                    }
                    if let Some(ref pf) = pad.pin_function {
                        s.push_str(&format!("          pin_function: \"{}\",\n", escape_json5(pf)));
                    }
                    if let Some(ref pt) = pad.pin_type {
                        s.push_str(&format!("          pin_type: \"{}\",\n", escape_json5(pt)));
                    }
                    if let Some(ratio) = pad.roundrect_rratio {
                        s.push_str(&format!("          roundrect_rratio: {},\n", ratio));
                    }
                    if let Some(margin) = pad.solder_mask_margin {
                        s.push_str(&format!("          solder_mask_margin: {},\n", margin));
                    }
                    if let Some(tw) = pad.thermal_bridge_width {
                        s.push_str(&format!("          thermal_bridge_width: {},\n", tw));
                    }
                    if let Some(ta) = pad.thermal_bridge_angle {
                        s.push_str(&format!("          thermal_bridge_angle: {},\n", ta));
                    }
                    if let Some(tg) = pad.thermal_gap {
                        s.push_str(&format!("          thermal_gap: {},\n", tg));
                    }
                    if let Some(c) = pad.clearance {
                        s.push_str(&format!("          clearance: {},\n", c));
                    }
                    if let Some(zc) = pad.zone_connect {
                        s.push_str(&format!("          zone_connect: {},\n", zc));
                    }
                    if let Some(rul) = pad.remove_unused_layers {
                        s.push_str(&format!("          remove_unused_layers: {},\n", rul));
                    }
                    if let Some(ref opts) = pad.options {
                        s.push_str("          options: {\n");
                        if let Some(ref c) = opts.clearance {
                            s.push_str(&format!("            clearance: \"{}\",\n", escape_json5(c)));
                        }
                        if let Some(ref a) = opts.anchor {
                            s.push_str(&format!("            anchor: \"{}\",\n", escape_json5(a)));
                        }
                        s.push_str("          },\n");
                    }
                    if !pad.primitives.is_empty() {
                        s.push_str("          primitives: [\n");
                        for prim in &pad.primitives {
                            use crate::ir::board::PadPrimitive as P;
                            let body = match prim {
                                P::GrPoly { pts, width, fill } => {
                                    let pts: Vec<String> = pts.iter().map(|(x, y)| format!("[{}, {}]", x, y)).collect();
                                    format!("kind: \"gr_poly\", points: [{}], width: {}, fill: {}", pts.join(", "), width, fill)
                                }
                                P::GrRect { start, end, width, fill } =>
                                    format!("kind: \"gr_rect\", start: [{}, {}], end: [{}, {}], width: {}, fill: {}", start.0, start.1, end.0, end.1, width, fill),
                                P::GrLine { start, end, width } =>
                                    format!("kind: \"gr_line\", start: [{}, {}], end: [{}, {}], width: {}", start.0, start.1, end.0, end.1, width),
                                P::GrCircle { center, end, width, fill } =>
                                    format!("kind: \"gr_circle\", center: [{}, {}], end: [{}, {}], width: {}, fill: {}", center.0, center.1, end.0, end.1, width, fill),
                                P::GrArc { start, mid, end, width } =>
                                    format!("kind: \"gr_arc\", start: [{}, {}], mid: [{}, {}], end: [{}, {}], width: {}", start.0, start.1, mid.0, mid.1, end.0, end.1, width),
                                P::Segment { start, end, width } =>
                                    format!("kind: \"segment\", start: [{}, {}], end: [{}, {}], width: {}", start.0, start.1, end.0, end.1, width),
                            };
                            s.push_str(&format!("            {{ {} }},\n", body));
                        }
                        s.push_str("          ],\n");
                    }
                    s.push_str("        },\n");
                }
                s.push_str("      ],\n");
            }

            // fp_lines
            if !fp.fp_lines.is_empty() {
                s.push_str("      fp_lines: [\n");
                for l in &fp.fp_lines {
                    s.push_str(&format!("        {{ start: [{}, {}], end: [{}, {}], stroke: {}, layer: \"{}\" }},\n",
                        l.start.0, l.start.1, l.end.0, l.end.1, l.stroke_width, l.layer));
                }
                s.push_str("      ],\n");
            }

            // fp_circles
            if !fp.fp_circles.is_empty() {
                s.push_str("      fp_circles: [\n");
                for c in &fp.fp_circles {
                    s.push_str(&format!("        {{ center: [{}, {}], end: [{}, {}], stroke: {}, layer: \"{}\", fill: {} }},\n",
                        c.center.0, c.center.1, c.end.0, c.end.1, c.stroke_width, c.layer, c.fill));
                }
                s.push_str("      ],\n");
            }

            // fp_arcs
            if !fp.fp_arcs.is_empty() {
                s.push_str("      fp_arcs: [\n");
                for a in &fp.fp_arcs {
                    s.push_str(&format!("        {{ start: [{}, {}], mid: [{}, {}], end: [{}, {}], stroke: {}, layer: \"{}\" }},\n",
                        a.start.0, a.start.1, a.mid.0, a.mid.1, a.end.0, a.end.1, a.stroke_width, a.layer));
                }
                s.push_str("      ],\n");
            }

            // fp_rects
            if !fp.fp_rects.is_empty() {
                s.push_str("      fp_rects: [\n");
                for r in &fp.fp_rects {
                    s.push_str(&format!("        {{ start: [{}, {}], end: [{}, {}], stroke: {}, layer: \"{}\", fill: {} }},\n",
                        r.start.0, r.start.1, r.end.0, r.end.1, r.stroke_width, r.layer, r.fill));
                }
                s.push_str("      ],\n");
            }

            // fp_polys
            if !fp.fp_polys.is_empty() {
                s.push_str("      fp_polys: [\n");
                for p in &fp.fp_polys {
                    let pts: Vec<String> = p.points.iter().map(|(x,y)| format!("[{}, {}]", x, y)).collect();
                    s.push_str(&format!("        {{ points: [{}], stroke: {}, layer: \"{}\", fill: {} }},\n",
                        pts.join(", "), p.stroke_width, p.layer, p.fill));
                }
                s.push_str("      ],\n");
            }

            // fp_texts (P1-10: previously missing entirely — texts lost on json5 hop)
            if !fp.fp_texts.is_empty() {
                s.push_str("      fp_texts: [\n");
                for t in &fp.fp_texts {
                    let ty = match t.text_type {
                        ir::board::FpTextType::Reference => "reference",
                        ir::board::FpTextType::Value => "value",
                        ir::board::FpTextType::User => "user",
                    };
                    s.push_str(&format!("        {{ text: \"{}\", type: \"{}\", at: [{}, {}, {}], layer: \"{}\", size: {} }},\n",
                        escape_json5(&t.text), ty, t.position.0, t.position.1, t.position.2,
                        t.layer, t.font_size.0));
                }
                s.push_str("      ],\n");
            }

            s.push_str("    },\n");
        }
        s.push_str("  ],\n");
    }

    // segments
    if !board.segments.is_empty() {
        s.push_str("  segments: [\n");
        for seg in &board.segments {
            s.push_str(&format!("    {{ start: [{}, {}], end: [{}, {}], width: {}, layer: \"{}\", net: {} }},\n",
                seg.start.0, seg.start.1, seg.end.0, seg.end.1, seg.width, seg.layer, seg.net));
        }
        s.push_str("  ],\n");
    }

    // vias
    if !board.vias.is_empty() {
        s.push_str("  vias: [\n");
        for via in &board.vias {
            s.push_str(&format!("    {{ at: [{}, {}], size: {}, drill: {}, layers: [{}], net: {} }},\n",
                via.at.0, via.at.1, via.size, via.drill,
                via.layers.iter().map(|l| format!("\"{}\"", l)).collect::<Vec<_>>().join(", "),
                via.net));
        }
        s.push_str("  ],\n");
    }

    // zones
    if !board.zones.is_empty() {
        s.push_str("  zones: [\n");
        for zone in &board.zones {
            s.push_str("    {\n");
            s.push_str(&format!("      net: {},\n", zone.net));
            s.push_str(&format!("      net_name: \"{}\",\n", escape_json5(&zone.net_name)));
            s.push_str(&format!("      layer: \"{}\",\n", zone.layer));
            s.push_str(&format!("      hatch: {{ style: \"{}\", pitch: {} }},\n", zone.hatch_style, zone.hatch_pitch));
            s.push_str(&format!("      clearance: {},\n", zone.connect_pads_clearance));
            s.push_str(&format!("      pad_connect: \"{}\",\n", zone.pad_connect));
            s.push_str(&format!("      min_thickness: {},\n", zone.min_thickness));
            s.push_str(&format!("      fill: {},\n", zone.fill));
            s.push_str(&format!("      thermal_gap: {},\n", zone.thermal_gap));
            s.push_str(&format!("      thermal_bridge_width: {},\n", zone.thermal_bridge_width));
            s.push_str(&format!("      island_removal_mode: {},\n", zone.island_removal_mode));
            if zone.island_removal_mode == 2 {
                s.push_str(&format!("      island_area: {},\n", zone.island_area));
            }
            let pts: Vec<String> = zone.outline.iter().map(|(x,y)| format!("[{}, {}]", x, y)).collect();
            s.push_str(&format!("      outline: [{}],\n", pts.join(", ")));
            if !zone.filled_polygons.is_empty() {
                s.push_str("      filled_polygons: [\n");
                for fpoly in &zone.filled_polygons {
                    let pts: Vec<String> = fpoly.points.iter().map(|(x,y)| format!("[{}, {}]", x, y)).collect();
                    s.push_str(&format!("        {{ layer: \"{}\", points: [{}] }},\n",
                        fpoly.layer, pts.join(", ")));
                }
                s.push_str("      ],\n");
            }
            s.push_str("    },\n");
        }
        s.push_str("  ],\n");
    }

    // graphics
    if !board.graphics.is_empty() {
        s.push_str("  graphics: [\n");
        for g in &board.graphics {
            match &g.kind {
                ir::board::BoardGraphicKind::Line { start, end } => {
                    s.push_str(&format!("    {{ type: \"line\", start: [{}, {}], end: [{}, {}], layer: \"{}\", stroke: {} }},\n",
                        start.0, start.1, end.0, end.1, g.layer, g.stroke_width));
                }
                ir::board::BoardGraphicKind::Arc { start, mid, end } => {
                    s.push_str(&format!("    {{ type: \"arc\", start: [{}, {}], mid: [{}, {}], end: [{}, {}], layer: \"{}\", stroke: {} }},\n",
                        start.0, start.1, mid.0, mid.1, end.0, end.1, g.layer, g.stroke_width));
                }
                ir::board::BoardGraphicKind::Circle { center, end } => {
                    s.push_str(&format!("    {{ type: \"circle\", center: [{}, {}], end: [{}, {}], layer: \"{}\", stroke: {}, fill: {} }},\n",
                        center.0, center.1, end.0, end.1, g.layer, g.stroke_width, g.fill));
                }
                ir::board::BoardGraphicKind::Rect { start, end } => {
                    s.push_str(&format!("    {{ type: \"rect\", start: [{}, {}], end: [{}, {}], layer: \"{}\", stroke: {}, fill: {} }},\n",
                        start.0, start.1, end.0, end.1, g.layer, g.stroke_width, g.fill));
                }
                ir::board::BoardGraphicKind::Poly { points } => {
                    let pts: Vec<String> = points.iter().map(|(x,y)| format!("[{}, {}]", x, y)).collect();
                    s.push_str(&format!("    {{ type: \"poly\", points: [{}], layer: \"{}\", stroke: {}, fill: {} }},\n",
                        pts.join(", "), g.layer, g.stroke_width, g.fill));
                }
                ir::board::BoardGraphicKind::Text { text, position, font_size } => {
                    s.push_str(&format!("    {{ type: \"text\", text: \"{}\", position: {{ x: {}, y: {}, rotation: {} }}, font_size: {}, layer: \"{}\" }},\n",
                        escape_json5(text), position.0, position.1, position.2, font_size, g.layer));
                }
            }
        }
        s.push_str("  ],\n");
    }

    s.push_str("}\n");
    Ok(s)
}

fn escape_json5(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n").replace('\r', "\\r").replace('\t', "\\t")
}

fn pad_type_str(pt: &ir::board::PadType) -> &'static str {
    use ir::board::PadType;
    match pt {
        PadType::Smd => "smd",
        PadType::ThruHole => "thru_hole",
        PadType::Connect => "connect",
        PadType::NpThruHole => "np_thru_hole",
    }
}

fn pad_shape_str(ps: &ir::board::PadShape) -> &'static str {
    use ir::board::PadShape;
    match ps {
        PadShape::Circle => "circle",
        PadShape::Rect => "rect",
        PadShape::RoundRect => "roundrect",
        PadShape::Oval => "oval",
        PadShape::Trapezoid => "trapezoid",
        PadShape::Custom => "custom",
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_generate_empty_board() {
        let board = ir::Board::new();
        let json5 = generate_board_json5(&board).unwrap();
        assert!(json5.contains("version:"));
        assert!(json5.contains("generator:"));
        assert!(json5.contains("layers:"));
        assert!(json5.contains("nets:"));
    }

    #[test]
    fn test_roundtrip_board_with_footprint() {
        let mut board = ir::Board::new();
        board.add_net("VIN");
        board.add_net("GND");

        let mut fp = ir::board::Footprint::new("Device:R", "R1", "10k");
        fp.position = (25.4, 50.8, 90.0);
        fp.pads.push(ir::board::Pad {
            number: "1".into(),
            pad_type: ir::board::PadType::Smd,
            shape: ir::board::PadShape::Rect,
            position: (0.0, 0.0, 0.0),
            size: (1.0, 0.5),
            layers: vec!["F.Cu".into()],
            drill: None,
            net: Some(1),
            net_name: None, pin_function: None,
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

        let json5 = generate_board_json5(&board).unwrap();
        let parsed = parse_board_json5(&json5).unwrap();

        assert_eq!(parsed.version, board.version);
        assert_eq!(parsed.nets.len(), 3); // "" + "VIN" + "GND"
        assert_eq!(parsed.footprints.len(), 1);
        assert_eq!(parsed.footprints[0].lib_id, "Device:R");
        assert_eq!(parsed.footprints[0].reference, "R1");
        assert_eq!(parsed.footprints[0].value, "10k");
        assert_eq!(parsed.footprints[0].position, (25.4, 50.8, 90.0));
        assert_eq!(parsed.footprints[0].pads.len(), 1);
        assert_eq!(parsed.footprints[0].pads[0].net, Some(1));
    }

    // P1-10 残余: footprint graphics + board graphics must survive the
    // pcb → json5 → pcb round trip (P0-9 regression guard)
    #[test]
    fn test_roundtrip_full_graphics() {
        use ir::board::*;
        let mut board = ir::Board::new();
        board.add_net("GND");

        let mut fp = Footprint::new("Test:IC", "U1", "X");
        fp.position = (30.0, 30.0, 0.0);
        fp.fp_lines.push(FpLine { start: (-1.0, 0.0), end: (1.0, 0.0), stroke_width: 0.15, layer: "F.SilkS".into() });
        fp.fp_circles.push(FpCircle { center: (0.0, -1.6), end: (0.3, -1.6), stroke_width: 0.15, layer: "F.SilkS".into(), fill: false });
        fp.fp_arcs.push(FpArc { start: (-1.0, 1.0), mid: (0.0, 1.4), end: (1.0, 1.0), stroke_width: 0.12, layer: "F.Fab".into() });
        fp.fp_rects.push(FpRect { start: (-2.0, -2.0), end: (2.0, 2.0), stroke_width: 0.05, layer: "F.CrtYd".into(), fill: false });
        fp.fp_polys.push(FpPoly { points: vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)], stroke_width: 0.1, layer: "F.Fab".into(), fill: true });
        fp.fp_texts.push(FpText { text: "pin1".into(), text_type: FpTextType::User, position: (0.0, -2.5, 0.0), font_size: (1.0, 1.0), layer: "F.SilkS".into() });
        board.footprints.push(fp);

        board.graphics.push(BoardGraphic {
            kind: BoardGraphicKind::Line { start: (5.0, 5.0), end: (15.0, 5.0) },
            layer: "Edge.Cuts".into(), stroke_width: 0.1, fill: false,
        });
        board.graphics.push(BoardGraphic {
            kind: BoardGraphicKind::Rect { start: (5.0, 5.0), end: (60.0, 42.0) },
            layer: "Edge.Cuts".into(), stroke_width: 0.1, fill: false,
        });
        board.graphics.push(BoardGraphic {
            kind: BoardGraphicKind::Circle { center: (10.0, 10.0), end: (11.6, 10.0) },
            layer: "Dwgs.User".into(), stroke_width: 0.15, fill: false,
        });

        let json5 = generate_board_json5(&board).unwrap();
        assert!(json5.contains("fp_lines"), "json5 export must emit fp_lines");
        assert!(json5.contains("fp_rects"), "json5 export must emit fp_rects");
        let reparsed = parse_board_json5(&json5).unwrap();
        assert_eq!(reparsed.footprints[0].fp_lines.len(), 1);
        assert_eq!(reparsed.footprints[0].fp_circles.len(), 1);
        assert_eq!(reparsed.footprints[0].fp_arcs.len(), 1);
        assert_eq!(reparsed.footprints[0].fp_rects.len(), 1);
        assert_eq!(reparsed.footprints[0].fp_polys.len(), 1);
        assert_eq!(reparsed.footprints[0].fp_texts.len(), 1);
        assert_eq!(reparsed.graphics.len(), 3, "board graphics must survive");

        // second hop: json5 board → .kicad_pcb sexpr → IR
        let sexpr = {
            let mut gen = BoardSexprGenerator::new();
            gen.generate(&reparsed).unwrap()
        };
        assert_eq!(sexpr.matches("fp_line").count() >= 1, true);
        assert_eq!(sexpr.matches("fp_rect").count() >= 1, true);
        let final_board = parse_board(&sexpr).unwrap();
        assert_eq!(final_board.footprints[0].fp_lines.len(), 1, "fp_lines lost in sexpr hop");
        assert_eq!(final_board.footprints[0].fp_circles.len(), 1, "fp_circles lost in sexpr hop");
        assert_eq!(final_board.footprints[0].fp_arcs.len(), 1, "fp_arcs lost in sexpr hop");
        assert_eq!(final_board.footprints[0].fp_rects.len(), 1, "fp_rects lost in sexpr hop");
        assert_eq!(final_board.footprints[0].fp_polys.len(), 1, "fp_polys lost in sexpr hop");
        // legacy synthesis always adds reference/value fp_texts on top of ours (P2-4)
        assert!(final_board.footprints[0].fp_texts.iter()
            .any(|t| t.text == "pin1" && t.text_type == FpTextType::User),
            "user fp_texts lost in sexpr hop");
        assert_eq!(final_board.graphics.len(), 3, "board graphics lost in sexpr hop");
    }

    // P2-4: legacy fp_text synthesis must not duplicate reference/value that
    // fp_texts already carries
    #[test]
    fn test_fp_text_dedup() {
        use ir::board::*;
        let mut fp = Footprint::new("Test:R", "R1", "10k");
        fp.fp_texts.push(FpText { text: "R1".into(), text_type: FpTextType::Reference, position: (0.0, -1.0, 0.0), font_size: (1.0, 1.0), layer: "F.SilkS".into() });
        let mut board = ir::Board::new();
        board.footprints.push(fp);
        let out = {
            let mut gen = codegen::BoardSexprGenerator::new();
            gen.generate(&board).unwrap()
        };
        let refs = out.matches("(fp_text reference \"R1\"").count();
        assert_eq!(refs, 1, "reference duplicated: {} occurrences", refs);
    }

    #[test]
    fn test_roundtrip_segment_via() {
        let mut board = ir::Board::new();
        board.add_net("VCC");

        board.segments.push(ir::board::Segment {
            start: (0.0, 0.0), end: (10.0, 5.0),
            width: 0.25, layer: "F.Cu".into(), net: 1,
        });
        board.vias.push(ir::board::Via {
            at: (5.0, 2.5), size: 0.8, drill: 0.4,
            layers: vec!["F.Cu".into(), "B.Cu".into()], net: 1,
        });

        let json5 = generate_board_json5(&board).unwrap();
        let parsed = parse_board_json5(&json5).unwrap();

        assert_eq!(parsed.segments.len(), 1);
        assert_eq!(parsed.segments[0].start, (0.0, 0.0));
        assert_eq!(parsed.segments[0].end, (10.0, 5.0));
        assert_eq!(parsed.vias.len(), 1);
        assert_eq!(parsed.vias[0].at, (5.0, 2.5));
        assert_eq!(parsed.vias[0].layers.len(), 2);
    }
}
