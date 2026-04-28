//! Schematic Renderer - main rendering dispatcher
//!
//! Corresponds to JS `Jh` SchematicPainter class.
//! Orchestrates all painters (wires, symbols, junctions, labels) from a parsed Schematic IR.

use kicad_json5::ir::{GraphicElement, Schematic, Symbol};

use crate::bridge;
use crate::layer::{LayerSet, LayerId, LayerElement, LayerElementType};
use crate::painter::{
    JunctionPainter, LabelPainter, SymbolPainter, Painter,
};
use crate::painter::{Junction, Label, LabelType};
use crate::render_core::{Point, BoundingBox};
use crate::render_core::graphics::{Polyline, Polygon, Stroke};
use crate::renderer::Renderer;
use crate::constants;

/// Render a no-connect marker (X shape) directly to the renderer.
fn render_no_connect(renderer: &mut dyn Renderer, pos: (f64, f64)) {
    let color = constants::no_connect_color();
    let size = constants::NOCONNECT_SIZE / 2.0;
    let width = constants::LINE_WIDTH;

    // Two crossed diagonal lines forming an X
    renderer.draw_polyline(&Polyline::from_points(
        &[(pos.0 - size, pos.1 - size), (pos.0 + size, pos.1 + size)],
        Stroke::new(width, color),
    ));
    renderer.draw_polyline(&Polyline::from_points(
        &[(pos.0 + size, pos.1 - size), (pos.0 - size, pos.1 + size)],
        Stroke::new(width, color),
    ));
}

/// Render bus lines directly to the renderer.
fn render_buses(renderer: &mut dyn Renderer, buses: &[kicad_json5::ir::Bus]) {
    let color = constants::bus_color();
    let width = constants::BUS_WIDTH;

    for bus in buses {
        if bus.points.len() < 2 {
            continue;
        }
        let pts: Vec<(f64, f64)> = bus.points.iter().map(|(x, y)| (*x, *y)).collect();
        renderer.draw_polyline(&Polyline::from_points(&pts, Stroke::new(width, color)));
    }
}

/// Render bus entries (diagonal short lines) directly to the renderer.
fn render_bus_entries(renderer: &mut dyn Renderer, bus_entries: &[kicad_json5::ir::BusEntry]) {
    let color = constants::bus_color();
    let width = constants::WIRE_WIDTH;

    for be in bus_entries {
        let start = be.position;
        let end = (be.position.0 + be.size.0, be.position.1 + be.size.1);
        renderer.draw_polyline(&Polyline::from_points(
            &[(start.0, start.1), (end.0, end.1)],
            Stroke::new(width, color),
        ));
    }
}

/// Standard KiCad paper sizes (width, height) in mm
fn paper_dimensions(size: &str, portrait: bool) -> (f64, f64) {
    let (w, h) = match size {
        "A0" => (841.0, 1189.0),
        "A1" => (594.0, 841.0),
        "A2" => (420.0, 594.0),
        "A3" => (420.0, 297.0),
        "A4" => (297.0, 210.0),
        "A5" => (210.0, 148.0),
        "A" => (279.4, 215.9),  // US Letter
        "B" => (431.8, 279.4),
        "C" => (558.8, 431.8),
        "D" => (863.6, 558.8),
        "E" => (1117.6, 863.6),
        _ => (297.0, 210.0),     // Default A4
    };
    if portrait { (h, w) } else { (w, h) }
}

/// Schematic Renderer - converts a parsed Schematic IR into rendered output.
pub struct SchematicRenderer<'a> {
    schematic: &'a Schematic,
    file_name: String,
}

impl<'a> SchematicRenderer<'a> {
    pub fn new(schematic: &'a Schematic) -> Self {
        Self { schematic, file_name: String::new() }
    }

    pub fn with_file_name(mut self, name: String) -> Self {
        self.file_name = name;
        self
    }

    /// Find the library symbol definition for a given lib_id.
    fn find_lib_symbol(&self, lib_id: &str) -> Option<&Symbol> {
        self.schematic.lib_symbols.iter().find(|s| s.lib_id == lib_id)
    }

    /// Collect body graphics for a specific unit from a library symbol.
    fn collect_body_graphics(lib: &Symbol, unit: i32) -> Vec<GraphicElement> {
        let mut graphics = Vec::new();

        if unit <= 1 {
            for ge in &lib.graphics {
                if !matches!(ge, GraphicElement::Pin(_)) {
                    graphics.push(ge.clone());
                }
            }
        }

        for lib_unit in &lib.units {
            if lib_unit.unit_id == 0 || lib_unit.unit_id as i32 == unit {
                for ge in &lib_unit.graphics {
                    if !matches!(ge, GraphicElement::Pin(_)) {
                        graphics.push(ge.clone());
                    }
                }
            }
        }

        graphics
    }

    /// Compute the overall bounding box of the schematic content.
    pub fn bbox(&self) -> BoundingBox {
        let mut bbox = BoundingBox::empty();

        for wire in &self.schematic.wires {
            bbox.expand_point(wire.start.0, wire.start.1);
            bbox.expand_point(wire.end.0, wire.end.1);
        }

        for j in &self.schematic.junctions {
            bbox.expand_point(j.position.0, j.position.1);
        }

        for label in &self.schematic.labels {
            bbox.expand_point(label.position.0, label.position.1);
        }

        for comp in &self.schematic.components {
            bbox.expand_point(comp.position.0, comp.position.1);
            if let Some(lib) = self.find_lib_symbol(&comp.lib_id) {
                for ge in &lib.graphics {
                    if let GraphicElement::Pin(p) = ge {
                        let dx = p.length * match p.position.2 as i32 % 360 {
                            0 => 1.0, 90 | -270 => 0.0, 180 | -180 => -1.0, 270 | -90 => 0.0, _ => 0.0,
                        };
                        let dy = p.length * match p.position.2 as i32 % 360 {
                            0 => 0.0, 90 | -270 => -1.0, 180 | -180 => 0.0, 270 | -90 => 1.0, _ => 0.0,
                        };
                        bbox.expand_point(comp.position.0 + p.position.0 + dx, comp.position.1 + p.position.1 + dy);
                    }
                }
                for unit in &lib.units {
                    for ge in &unit.graphics {
                        if let GraphicElement::Pin(p) = ge {
                            let dx = p.length * match p.position.2 as i32 % 360 {
                                0 => 1.0, 90 | -270 => 0.0, 180 | -180 => -1.0, 270 | -90 => 0.0, _ => 0.0,
                            };
                            let dy = p.length * match p.position.2 as i32 % 360 {
                                0 => 0.0, 90 | -270 => -1.0, 180 | -180 => 0.0, 270 | -90 => 1.0, _ => 0.0,
                            };
                            bbox.expand_point(comp.position.0 + p.position.0 + dx, comp.position.1 + p.position.1 + dy);
                        }
                    }
                }
            }
        }

        if bbox.is_empty() {
            let (w, h) = self.paper_size();
            BoundingBox::from_min_max(0.0, 0.0, w, h)
        } else {
            bbox.with_padding(5.0)
        }
    }

    /// Get the paper size from metadata, falling back to content bbox.
    pub fn paper_size(&self) -> (f64, f64) {
        let paper = &self.schematic.metadata.paper;
        if let (Some(w), Some(h)) = (paper.width, paper.height) {
            if w > 0.0 && h > 0.0 {
                return if paper.portrait { (h, w) } else { (w, h) };
            }
        }
        paper_dimensions(&paper.size, paper.portrait)
    }

    /// Render drawing sheet border and title block.
    ///
    /// Matches KiCad's default `.kicad_wks` layout (default_drawing_sheet.kicad_wks):
    /// - 10mm margins, title block rect start=(110,34) end=(2,2) from rbcorner
    /// - Horizontal dividers at 5.5, 8.5, 12.5, 18.5mm from rbcorner
    /// - Vertical dividers at 90mm and 26mm from rbcorner
    /// - Grid reference tick marks and labels along all four edges
    fn render_drawing_sheet(&self, renderer: &mut dyn Renderer) {
        let (w, h) = self.paper_size();
        let border_color = constants::sheet_border_color();
        let text_color = constants::sheet_text_color();
        let line_w = 0.15;

        // Margins (default .kicad_wks setup: 10mm all sides)
        let (lm, rm, tm, bm) = (10.0_f64, 10.0, 10.0, 10.0);
        // Inner border corners (margin area)
        let (ix1, iy1) = (lm, tm);
        let (ix2, iy2) = (w - rm, h - bm);

        // Outer border (page outline)
        renderer.draw_polygon(&Polygon::from_points(&[
            (0.0, 0.0), (w, 0.0), (w, h), (0.0, h), (0.0, 0.0),
        ]).with_stroke(Stroke::new(line_w, border_color)));

        // Inner border — two rects 2mm apart (from .kicad_wks: repeat 2, incrx 2, incry 2)
        // First rect: (start 0 0 ltcorner) (end 0 0 rbcorner) = margin area
        renderer.draw_polygon(&Polygon::from_points(&[
            (ix1, iy1), (ix2, iy1), (ix2, iy2), (ix1, iy2), (ix1, iy1),
        ]).with_stroke(Stroke::new(line_w, border_color)));
        // Second rect: 2mm inset from first
        let spacing = constants::SHEET_BORDER_SPACING;
        renderer.draw_polygon(&Polygon::from_points(&[
            (ix1 + spacing, iy1 + spacing), (ix2 - spacing, iy1 + spacing),
            (ix2 - spacing, iy2 - spacing), (ix1 + spacing, iy2 - spacing), (ix1 + spacing, iy1 + spacing),
        ]).with_stroke(Stroke::new(line_w, border_color)));

        // ── Grid reference lines ──────────────────────────────
        // From .kicad_wks: tick marks and labels along all 4 edges at 50mm intervals
        let ref_font = constants::SHEET_REF_FONT;

        // Top edge: tick marks and column numbers between middle and inner border
        let mut col = 1usize;
        let mut x = ix1 + 50.0;
        while x < ix2 - 1.0 {
            renderer.draw_polyline(&Polyline::from_points(
                &[(x, iy1), (x, iy1 + spacing)],
                Stroke::new(line_w, border_color),
            ));
            renderer.draw_text(&Point::new(x - 25.0, iy1 + 1.0), &col.to_string(), ref_font, &text_color, false, 0.0, "", "");
            col += 1;
            x += 50.0;
        }

        // Bottom edge
        let mut col = 1usize;
        let mut x = ix1 + 50.0;
        while x < ix2 - 1.0 {
            renderer.draw_polyline(&Polyline::from_points(
                &[(x, iy2), (x, iy2 - spacing)],
                Stroke::new(line_w, border_color),
            ));
            renderer.draw_text(&Point::new(x - 25.0, iy2 - 1.0), &col.to_string(), ref_font, &text_color, false, 0.0, "", "");
            col += 1;
            x += 50.0;
        }

        // Left edge
        let mut row = 0usize;
        let mut y = iy1 + 50.0;
        while y < iy2 - 1.0 {
            renderer.draw_polyline(&Polyline::from_points(
                &[(ix1, y), (ix1 + spacing, y)],
                Stroke::new(line_w, border_color),
            ));
            let letter = (b'A' + (row % 26) as u8) as char;
            renderer.draw_text(&Point::new(ix1 + 1.0, y - 25.0), &letter.to_string(), ref_font, &text_color, false, 0.0, "", "");
            row += 1;
            y += 50.0;
        }

        // Right edge
        let mut row = 0usize;
        let mut y = iy1 + 50.0;
        while y < iy2 - 1.0 {
            renderer.draw_polyline(&Polyline::from_points(
                &[(ix2, y), (ix2 - spacing, y)],
                Stroke::new(line_w, border_color),
            ));
            let letter = (b'A' + (row % 26) as u8) as char;
            renderer.draw_text(&Point::new(ix2 - 1.0, y - 25.0), &letter.to_string(), ref_font, &text_color, false, 0.0, "", "");
            row += 1;
            y += 50.0;
        }

        // ── Title block ───────────────────────────────────────
        // From .kicad_wks: (rect (start 110 34) (end 2 2)) with default rbcorner anchor
        // rbcorner offset: br.sub(point) where br = (ix2, iy2)
        // start rbcorner: (ix2-110, iy2-34), end rbcorner: (ix2-2, iy2-2)
        let tb_left = ix2 - 110.0;
        let tb_top = iy2 - 34.0;
        let tb_right = ix2 - 2.0;
        let tb_bottom = iy2 - 2.0;

        // Title block rectangle
        renderer.draw_polygon(&Polygon::from_points(&[
            (tb_left, tb_top), (tb_right, tb_top),
            (tb_right, tb_bottom), (tb_left, tb_bottom), (tb_left, tb_top),
        ]).with_stroke(Stroke::new(line_w, border_color)));

        // Horizontal dividers (from .kicad_wks: start (110, dy) end (2, dy) at rbcorner)
        for &dy in &[5.5, 8.5, 12.5, 18.5] {
            let y = iy2 - dy;
            if y > tb_top {
                renderer.draw_polyline(&Polyline::from_points(
                    &[(ix2 - 110.0, y), (ix2 - 2.0, y)],
                    Stroke::new(line_w, border_color),
                ));
            }
        }

        // Vertical divider at 90mm from rbcorner: (line (start 90 8.5) (end 90 5.5))
        renderer.draw_polyline(&Polyline::from_points(
            &[(ix2 - 90.0, iy2 - 5.5), (ix2 - 90.0, iy2 - 8.5)],
            Stroke::new(line_w, border_color),
        ));

        // Vertical divider at 26mm from rbcorner: (line (start 26 8.5) (end 26 2))
        renderer.draw_polyline(&Polyline::from_points(
            &[(ix2 - 26.0, iy2 - 2.0), (ix2 - 26.0, iy2 - 8.5)],
            Stroke::new(line_w, border_color),
        ));

        // ── Title block text ──────────────────────────────────
        // All positions from rbcorner: pos = (ix2 - px, iy2 - py)
        let tb = &self.schematic.metadata.title_block;
        let font = constants::SHEET_TEXT_FONT;
        let title_font = constants::SHEET_TITLE_FONT;

        // (tbtext "${KICAD_VERSION}" (pos 109 4.1))
        renderer.draw_text(&Point::new(ix2 - 109.0, iy2 - 4.1), "kicad-render", font, &text_color, false, 0.0, "", "");

        // (tbtext "Id: ${#}/${##}" (pos 24 4.1))
        renderer.draw_text(&Point::new(ix2 - 24.0, iy2 - 4.1), "Id: 1/1", font, &text_color, false, 0.0, "", "");

        // (tbtext "Date: ${ISSUE_DATE}" (pos 87 6.9))
        if let Some(date) = &tb.date {
            renderer.draw_text(&Point::new(ix2 - 87.0, iy2 - 6.9), &format!("Date: {}", date), font, &text_color, false, 0.0, "", "");
        }

        // (tbtext "Rev: ${REVISION}" (pos 24 6.9) (font bold))
        if let Some(rev) = &tb.rev {
            renderer.draw_text(&Point::new(ix2 - 24.0, iy2 - 6.9), &format!("Rev: {}", rev), font, &text_color, true, 0.0, "", "");
        }

        // (tbtext "Size: ${PAPER}" (pos 109 6.9))
        renderer.draw_text(&Point::new(ix2 - 109.0, iy2 - 6.9), &format!("Size: {}", self.schematic.metadata.paper.size), font, &text_color, false, 0.0, "", "");

        // (tbtext "Title: ${TITLE}" (pos 109 10.7) (font (size 2 2) bold italic))
        if let Some(title) = &tb.title {
            renderer.draw_text(&Point::new(ix2 - 109.0, iy2 - 10.7), title, title_font, &text_color, false, 0.0, "", "");
        }

        // (tbtext "File: ${FILENAME}" (pos 109 14.3))
        if !self.file_name.is_empty() {
            renderer.draw_text(&Point::new(ix2 - 109.0, iy2 - 14.3), &format!("File: {}", self.file_name), font, &text_color, false, 0.0, "", "");
        }

        // (tbtext "Sheet: ${SHEETPATH}" (pos 109 17))
        renderer.draw_text(&Point::new(ix2 - 109.0, iy2 - 17.0), "Sheet: /", font, &text_color, false, 0.0, "", "");

        // (tbtext "${COMPANY}" (pos 109 20) (font bold))
        if let Some(company) = &tb.company {
            renderer.draw_text(&Point::new(ix2 - 109.0, iy2 - 20.0), company, font, &text_color, false, 0.0, "", "");
        }

        // Comments 1-4: (tbtext "${COMMENT1-4}" (pos 109 23/26/29/32))
        for (comment, dy) in tb.comments.iter().zip([23.0_f64, 26.0, 29.0, 32.0].iter()) {
            if !comment.is_empty() {
                renderer.draw_text(&Point::new(ix2 - 109.0, iy2 - dy), comment, font, &text_color, false, 0.0, "", "");
            }
        }
    }

    /// Render the full schematic to the given renderer.
    pub fn render(&self, renderer: &mut dyn Renderer) {
        // 0. Drawing sheet border and title block
        self.render_drawing_sheet(renderer);

        let mut layers = LayerSet::default();

        // 1. Wires
        if !self.schematic.wires.is_empty() {
            let wire_painter = bridge::convert_wires(&self.schematic.wires);
            wire_painter.paint(&mut layers);
        }

        // 2. Junctions
        if !self.schematic.junctions.is_empty() {
            let junctions: Vec<Junction> = self.schematic.junctions
                .iter()
                .map(|j| bridge::convert_junction(j.position, j.diameter))
                .collect();
            let junction_painter = JunctionPainter::new(junctions, constants::junction_color());
            junction_painter.paint(&mut layers);
        }

        // 3. Symbols
        for comp in &self.schematic.components {
            let lib = self.find_lib_symbol(&comp.lib_id);
            let symbol_instance = bridge::convert_symbol(comp, lib);

            let body_graphics = if let Some(lib) = lib {
                Self::collect_body_graphics(lib, comp.unit as i32)
            } else {
                Vec::new()
            };

            let symbol_painter = SymbolPainter::with_graphics(symbol_instance, body_graphics);
            symbol_painter.paint(&mut layers);
        }

        // 4. Labels (net names)
        for label_ir in &self.schematic.labels {
            let label: Label = bridge::convert_label(label_ir);
            let color = match label.label_type {
                LabelType::Global => constants::global_label_color(),
                LabelType::Hierarchical => constants::hier_label_color(),
                LabelType::Local => constants::label_color(),
            };
            let label_painter = LabelPainter::new(label, color);
            label_painter.paint(&mut layers);
        }

        // 5. Text notes (blue annotation text on Notes layer)
        // Normalizes multi-line text: strips trailing newlines, removes empty lines,
        // and inserts paragraph spacing where double-newlines existed in the original.
        let notes_layer_id = LayerId::Notes;
        let interline_pitch_ratio = constants::INTERLINE_PITCH_RATIO;
        for text_item in &self.schematic.text_items {
            let font_size = text_item.effects.font.size.1.max(text_item.effects.font.size.0);
            let size = if font_size > 0.0 { font_size } else { constants::TEXT_SIZE };
            let color = constants::note_color();
            let bold = text_item.effects.font.bold;

            // Normalize: strip trailing newlines, split on \n, remove empty lines,
            // record paragraph break positions (where \n\n existed).
            let trimmed = text_item.text.trim_end_matches('\n');
            let raw_parts: Vec<&str> = trimmed.split('\n').collect();

            // Build (line_text, is_after_paragraph_break) pairs
            let mut text_lines: Vec<(&str, bool)> = Vec::new();
            let mut prev_was_empty = false;
            for part in &raw_parts {
                if part.is_empty() {
                    prev_was_empty = true;
                } else {
                    text_lines.push((*part, prev_was_empty));
                    prev_was_empty = false;
                }
            }
            if text_lines.is_empty() {
                continue;
            }
            // First line is never a paragraph break
            text_lines[0].1 = false;

            let num_lines = text_lines.len();
            let interline = size * interline_pitch_ratio;
            let paragraph_gap = interline * 2.0; // paragraph break = 2× normal interline

            // Compute total text block height accounting for paragraph gaps
            let mut total_height = size * 1.17;
            for i in 1..num_lines {
                let gap = if text_lines[i].1 { paragraph_gap } else { interline };
                total_height += gap;
            }

            // Offset based on v_align
            let justify = &text_item.effects.justify;
            let mut offset_y = size;
            match justify.vertical {
                kicad_json5::ir::VerticalAlign::Top => {}
                kicad_json5::ir::VerticalAlign::Center => offset_y -= total_height / 2.0,
                kicad_json5::ir::VerticalAlign::Bottom => offset_y -= total_height,
            }

            let at_x = text_item.position.0;
            let at_y = text_item.position.1;

            let mut line_y = at_y + offset_y;
            for (n, (line, is_para)) in text_lines.iter().enumerate() {
                if n > 0 {
                    line_y += if *is_para { paragraph_gap } else { interline };
                }

                let line_x = match justify.horizontal {
                    kicad_json5::ir::HorizontalAlign::Left => at_x,
                    kicad_json5::ir::HorizontalAlign::Center => {
                        let approx_width = line.len() as f64 * size * 0.5;
                        at_x - approx_width / 2.0
                    }
                    kicad_json5::ir::HorizontalAlign::Right => {
                        let approx_width = line.len() as f64 * size * 0.5;
                        at_x - approx_width
                    }
                };

                let element = LayerElement::new(LayerElementType::Text {
                    position: Point::new(line_x, line_y),
                    text: line.to_string(),
                    font_size: size,
                    color,
                    bold,
                    rotation: 0.0,
                    text_anchor: "",
                    dominant_baseline: "",
                });
                if let Some(layer) = layers.get_layer_mut(notes_layer_id) {
                    layer.add_element(element);
                }
            }
        }

        // 6. Schematic-level graphic polylines (section dividers, etc.)
        // JS PolylinePainter ignores dash type — always renders solid lines.
        // Use convert_polyline_solid to match JS behavior.
        for pl in &self.schematic.polylines {
            if pl.points.len() < 2 {
                continue;
            }
            let color = constants::note_color();
            let polyline = bridge::convert_polyline_solid(pl, color);
            let element = LayerElement::new(LayerElementType::Polyline(polyline));
            if let Some(layer) = layers.get_layer_mut(notes_layer_id) {
                layer.add_element(element);
            }
        }

        // Render all layers via the renderer
        layers.render(renderer);

        // 7. No-connect markers (X shapes) — rendered directly on Junction layer
        for nc in &self.schematic.no_connects {
            render_no_connect(renderer, nc.position);
        }

        // 8. Buses (thick lines)
        render_buses(renderer, &self.schematic.buses);

        // 9. Bus entries (diagonal short lines)
        render_bus_entries(renderer, &self.schematic.bus_entries);
    }
}
