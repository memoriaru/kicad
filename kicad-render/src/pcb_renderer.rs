//! PCB SVG Renderer
//!
//! Renders a Board IR to SVG string directly.
//! Colors aligned with KiCad default theme (kicad-default.ts).
//! Rendering logic aligned with ecad-viewer (painter.ts, pad-painter.ts, zone-painter.ts).

use kicad_json5::ir::board::*;
use std::collections::HashSet;

/// P1-4: Rendering view mode — controls which layer categories are drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RenderMode {
    /// Assembly view (default, backward-compatible): copper traces + vias +
    /// zones + footprints + silkscreen. The original behavior.
    #[default]
    Assembly,
    /// Fabrication view: board outline + silkscreen + drill holes + solder
    /// mask graphics. Copper traces/vias/zones are hidden — for viewing the
    /// bare board fabrication artwork.
    Fabrication,
    /// Copper-only view: traces + vias + zones + pads. Silkscreen, mask, and
    /// decorative graphics hidden — for inspecting routing density.
    CopperOnly,
}

/// PCB SVG Renderer — converts Board IR to an SVG string.
pub struct PcbRenderer<'a> {
    board: &'a Board,
    dpi: f64,
    /// P1-4: view mode controlling which layer categories are drawn.
    mode: RenderMode,
    /// P1-4: optional layer whitelist. `None` = draw all layers (default).
    /// `Some(set)` = only draw elements whose layer is in the set (or matched
    /// by the `*.Cu` wildcard for copper layers).
    layer_filter: Option<HashSet<String>>,
}

// KiCad default theme colors (from kicad-default.ts / ecad-viewer)
const BG_COLOR: &str = "rgb(0,16,35)";
const BOARD_SUBSTRATE: &str = "rgb(25,55,40)";
const COPPER_F: &str = "rgb(200,52,52)";
const COPPER_B: &str = "rgb(77,127,196)";
const EDGE_CUTS: &str = "rgb(208,210,205)";
const SILK_F: &str = "rgb(242,237,161)";
const SILK_B: &str = "rgb(232,178,167)";
const FAB_F: &str = "rgb(175,175,175)";
const FAB_B: &str = "rgb(88,93,132)";
const PAD_THR_HOLE: &str = "rgb(227,183,46)";
const NON_PLATED_HOLE: &str = "rgb(26,196,210)";
const VIA_THROUGH: &str = "rgb(236,236,236)";
const VIA_HOLE: &str = "rgb(227,183,46)";
#[allow(dead_code)] // KiCad theme constants kept for 1:1 port completeness
const VIA_BLIND_BURIED: &str = "rgb(187,151,38)";
#[allow(dead_code)]
const VIA_MICRO: &str = "rgb(0,132,132)";
#[allow(dead_code)]
const PAD_PLATED_HOLE: &str = "rgb(194,194,0)";
#[allow(dead_code)]
const DRC_ERROR: &str = "rgba(215,91,107,0.80)";
#[allow(dead_code)]
const RATSNEST: &str = "rgba(245,255,213,0.70)";
const DWGS_USER_COLOR: &str = "rgb(194,194,194)";
const CMTS_USER_COLOR: &str = "rgb(89,148,220)";
const CRTYD_F: &str = "rgb(255,38,226)";
const CRTYD_B: &str = "rgb(38,233,255)";
const MARGIN_COLOR: &str = "rgb(255,38,226)";
const WORKSHEET_COLOR: &str = "rgb(200,114,171)";
const MASK_F: &str = "rgba(216,100,255,0.40)";
const MASK_B: &str = "rgba(2,255,238,0.40)";
const PASTE_F: &str = "rgba(180,160,154,0.90)";
const PASTE_B: &str = "rgba(0,194,194,0.90)";
const ADHES_F: &str = "rgb(132,0,132)";
const ADHES_B: &str = "rgb(0,0,132)";
const ECO1_COLOR: &str = "rgb(180,219,210)";
const ECO2_COLOR: &str = "rgb(216,200,82)";
const USER1_COLOR: &str = "rgb(194,194,194)";
const USER2_COLOR: &str = "rgb(89,148,220)";
const USER3_COLOR: &str = "rgb(180,219,210)";
const USER4_COLOR: &str = "rgb(216,200,82)";
const USER5_COLOR: &str = "rgb(194,194,194)";
const USER6_COLOR: &str = "rgb(89,148,220)";
const USER7_COLOR: &str = "rgb(180,219,210)";
const USER8_COLOR: &str = "rgb(216,200,82)";
const USER9_COLOR: &str = "rgb(232,178,167)";

// Zone default opacity (ecad-viewer viewer.ts: ZONE_DEFAULT_OPACITY = 0.6)
#[allow(dead_code)]
const ZONE_OPACITY: f64 = 0.6;
const COPPER_IN1: &str = "rgb(127,200,127)";
const COPPER_IN2: &str = "rgb(206,125,44)";
const COPPER_IN3: &str = "rgb(79,203,203)";
const COPPER_IN4: &str = "rgb(219,98,139)";
const COPPER_IN5: &str = "rgb(167,165,198)";
const COPPER_IN6: &str = "rgb(40,204,217)";
const COPPER_IN7: &str = "rgb(232,178,167)";
const COPPER_IN8: &str = "rgb(242,237,161)";
const COPPER_IN9: &str = "rgb(141,203,129)";
const COPPER_IN10: &str = "rgb(237,124,51)";
const COPPER_IN11: &str = "rgb(91,195,235)";
const COPPER_IN12: &str = "rgb(247,111,142)";
const COPPER_IN13: &str = "rgb(167,165,198)";
const COPPER_IN14: &str = "rgb(40,204,217)";
const COPPER_IN15: &str = "rgb(232,178,167)";
const COPPER_IN16: &str = "rgb(242,237,161)";
const COPPER_IN17: &str = "rgb(237,124,51)";
const COPPER_IN18: &str = "rgb(91,195,235)";
const COPPER_IN19: &str = "rgb(247,111,142)";
const COPPER_IN20: &str = "rgb(167,165,198)";
const COPPER_IN21: &str = "rgb(40,204,217)";
const COPPER_IN22: &str = "rgb(232,178,167)";
const COPPER_IN23: &str = "rgb(242,237,161)";
const COPPER_IN24: &str = "rgb(237,124,51)";
const COPPER_IN25: &str = "rgb(91,195,235)";
const COPPER_IN26: &str = "rgb(247,111,142)";
const COPPER_IN27: &str = "rgb(167,165,198)";
const COPPER_IN28: &str = "rgb(40,204,217)";
const COPPER_IN29: &str = "rgb(232,178,167)";
const COPPER_IN30: &str = "rgb(242,237,161)";

impl<'a> PcbRenderer<'a> {
    /// DPI for mm→px conversion (default: 96, standard screen resolution).
    const DEFAULT_DPI: f64 = 96.0;

    pub fn new(board: &'a Board) -> Self {
        Self {
            board,
            dpi: Self::DEFAULT_DPI,
            mode: RenderMode::Assembly,
            layer_filter: None,
        }
    }

    /// Set DPI for mm→px conversion. Higher DPI = larger SVG pixel dimensions.
    pub fn with_dpi(mut self, dpi: f64) -> Self {
        self.dpi = dpi;
        self
    }

    /// Backward-compatible alias for `with_dpi`.
    pub fn with_scale(mut self, scale: f64) -> Self {
        self.dpi = scale * Self::DEFAULT_DPI;
        self
    }

    /// P1-4: Set render view mode (Assembly / Fabrication / CopperOnly).
    pub fn with_mode(mut self, mode: RenderMode) -> Self {
        self.mode = mode;
        self
    }

    /// P1-4: Set a layer whitelist. Only elements on these layers are drawn.
    /// Use `"*.Cu"` to match all copper layers (F.Cu, B.Cu, In1.Cu, ...).
    pub fn with_layer_filter(mut self, layers: HashSet<String>) -> Self {
        self.layer_filter = Some(layers);
        self
    }

    /// P1-4: Check whether a layer should be rendered under the current filter.
    /// Returns true when no filter is set, or the layer matches the whitelist
    /// (directly or via the `*.Cu` wildcard for copper layers).
    /// Quote-escape a layer name for embedding in a data-layer attribute.
    fn escape_layer(layer: &str) -> &str {
        // Layer names are [A-Za-z0-9.*_-]; keep this cheap and defensive.
        layer
    }

    /// One drill feature: filled hole (pad view) or outline (drill mark).
    /// Oval `(drill oval W H)` renders as a capsule; `diameter` is the Y size.
    fn write_drill_hole(
        out: &mut String,
        cx: f64,
        cy: f64,
        drill: &DrillDef,
        fill: &str,
        outline: bool,
    ) {
        let style = if outline {
            format!("fill=\"none\" stroke=\"{}\" stroke-width=\"0.15\"", fill)
        } else {
            format!("fill=\"{}\" stroke=\"none\"", fill)
        };
        match drill.width {
            Some(w) => {
                let (w, h) = (w, drill.diameter);
                let r = w.min(h) / 2.0;
                out.push_str(&format!(
                    "<rect x=\"{:.3}\" y=\"{:.3}\" width=\"{:.3}\" height=\"{:.3}\" rx=\"{:.3}\" ry=\"{:.3}\" {}/>",
                    cx - w / 2.0,
                    cy - h / 2.0,
                    w,
                    h,
                    r,
                    r,
                    style
                ));
            }
            None => {
                out.push_str(&format!(
                    "<circle cx=\"{:.3}\" cy=\"{:.3}\" r=\"{:.3}\" {}/>",
                    cx,
                    cy,
                    drill.diameter / 2.0,
                    style
                ));
            }
        }
    }

    /// Drill marks for technical-layer plots: kicad-cli plots pad drill
    /// outlines on non-copper exports (silkscreen shows dots, Edge.Cuts the
    /// mechanical holes/slots). Emitted only when a --layers filter is active
    /// and contains no copper layer — in copper/full views the pads already
    /// carry their holes.
    fn write_drill_marks(&self, out: &mut String) {
        let technical_only = match &self.layer_filter {
            None => false,
            Some(set) => !set.iter().any(|l| Self::is_copper_layer(l) || l == "*.Cu"),
        };
        if !technical_only {
            return;
        }
        for fp in &self.board.footprints {
            let (fx, fy, _fr) = fp.position;
            for pad in &fp.pads {
                if let Some(ref drill) = pad.drill {
                    let (px, py, _) = pad.position;
                    out.push_str(&format!(
                        "<g data-layer=\"Drills\" transform=\"translate({:.3},{:.3})\">",
                        fx, fy
                    ));
                    Self::write_drill_hole(out, px, py, drill, EDGE_CUTS, true);
                    out.push_str("</g>");
                }
            }
        }
    }

    fn layer_visible(&self, layer: &str) -> bool {
        match &self.layer_filter {
            None => true,
            Some(set) => {
                set.contains(layer)
                    || (set.contains("*.Cu") && Self::is_copper_layer(layer))
                    || (set.contains("*.SilkS") && (layer.ends_with(".SilkS")))
                    || (set.contains("*.Mask") && (layer.ends_with(".Mask")))
            }
        }
    }

    /// Map a KiCad layer name to its theme color.
    /// Mirrors ecad-viewer's `color_for()` in layers.ts.
    fn layer_color(layer: &str) -> &'static str {
        match layer {
            "Edge.Cuts" => EDGE_CUTS,
            "Margin" => MARGIN_COLOR,
            "Dwgs.User" => DWGS_USER_COLOR,
            "Cmts.User" => CMTS_USER_COLOR,
            "Eco1.User" => ECO1_COLOR,
            "Eco2.User" => ECO2_COLOR,
            "F.SilkS" => SILK_F,
            "B.SilkS" => SILK_B,
            "F.Fab" => FAB_F,
            "B.Fab" => FAB_B,
            "F.CrtYd" => CRTYD_F,
            "B.CrtYd" => CRTYD_B,
            "F.Mask" => MASK_F,
            "B.Mask" => MASK_B,
            "F.Paste" => PASTE_F,
            "B.Paste" => PASTE_B,
            "F.Adhes" => ADHES_F,
            "B.Adhes" => ADHES_B,
            "User.1" => USER1_COLOR,
            "User.2" => USER2_COLOR,
            "User.3" => USER3_COLOR,
            "User.4" => USER4_COLOR,
            "User.5" => USER5_COLOR,
            "User.6" => USER6_COLOR,
            "User.7" => USER7_COLOR,
            "User.8" => USER8_COLOR,
            "User.9" => USER9_COLOR,
            "F.Cu" => COPPER_F,
            "B.Cu" => COPPER_B,
            "In1.Cu" => COPPER_IN1,
            "In2.Cu" => COPPER_IN2,
            "In3.Cu" => COPPER_IN3,
            "In4.Cu" => COPPER_IN4,
            "In5.Cu" => COPPER_IN5,
            "In6.Cu" => COPPER_IN6,
            "In7.Cu" => COPPER_IN7,
            "In8.Cu" => COPPER_IN8,
            "In9.Cu" => COPPER_IN9,
            "In10.Cu" => COPPER_IN10,
            "In11.Cu" => COPPER_IN11,
            "In12.Cu" => COPPER_IN12,
            "In13.Cu" => COPPER_IN13,
            "In14.Cu" => COPPER_IN14,
            "In15.Cu" => COPPER_IN15,
            "In16.Cu" => COPPER_IN16,
            "In17.Cu" => COPPER_IN17,
            "In18.Cu" => COPPER_IN18,
            "In19.Cu" => COPPER_IN19,
            "In20.Cu" => COPPER_IN20,
            "In21.Cu" => COPPER_IN21,
            "In22.Cu" => COPPER_IN22,
            "In23.Cu" => COPPER_IN23,
            "In24.Cu" => COPPER_IN24,
            "In25.Cu" => COPPER_IN25,
            "In26.Cu" => COPPER_IN26,
            "In27.Cu" => COPPER_IN27,
            "In28.Cu" => COPPER_IN28,
            "In29.Cu" => COPPER_IN29,
            "In30.Cu" => COPPER_IN30,
            _ => {
                if layer.starts_with("B.") {
                    COPPER_B
                } else {
                    COPPER_F
                }
            }
        }
    }

    /// Whether this layer is a copper layer (full opacity).
    fn is_copper_layer(layer: &str) -> bool {
        layer.ends_with(".Cu")
            || layer == "F.Cu"
            || layer == "B.Cu"
            || layer.starts_with("In") && layer.ends_with(".Cu")
    }

    /// Get color for a non-copper layer with alpha 0.8 (ecad-viewer convention).
    fn layer_color_with_alpha(layer: &str) -> String {
        let base = Self::layer_color(layer);
        if Self::is_copper_layer(layer) {
            base.to_string()
        } else {
            Self::color_with_opacity(base, 0.8)
        }
    }

    /// Map a layer to its copper color (for traces/zones/pads).
    fn copper_color(layer: &str) -> &'static str {
        match layer {
            "F.Cu" => COPPER_F,
            "B.Cu" => COPPER_B,
            "In1.Cu" => COPPER_IN1,
            "In2.Cu" => COPPER_IN2,
            "In3.Cu" => COPPER_IN3,
            "In4.Cu" => COPPER_IN4,
            "In5.Cu" => COPPER_IN5,
            "In6.Cu" => COPPER_IN6,
            "In7.Cu" => COPPER_IN7,
            "In8.Cu" => COPPER_IN8,
            "In9.Cu" => COPPER_IN9,
            "In10.Cu" => COPPER_IN10,
            "In11.Cu" => COPPER_IN11,
            "In12.Cu" => COPPER_IN12,
            "In13.Cu" => COPPER_IN13,
            "In14.Cu" => COPPER_IN14,
            "In15.Cu" => COPPER_IN15,
            "In16.Cu" => COPPER_IN16,
            "In17.Cu" => COPPER_IN17,
            "In18.Cu" => COPPER_IN18,
            "In19.Cu" => COPPER_IN19,
            "In20.Cu" => COPPER_IN20,
            "In21.Cu" => COPPER_IN21,
            "In22.Cu" => COPPER_IN22,
            "In23.Cu" => COPPER_IN23,
            "In24.Cu" => COPPER_IN24,
            "In25.Cu" => COPPER_IN25,
            "In26.Cu" => COPPER_IN26,
            "In27.Cu" => COPPER_IN27,
            "In28.Cu" => COPPER_IN28,
            "In29.Cu" => COPPER_IN29,
            "In30.Cu" => COPPER_IN30,
            _ => {
                if layer.starts_with('B') {
                    COPPER_B
                } else {
                    COPPER_F
                }
            }
        }
    }

    /// Parse paper size string (e.g. "A4", "User 159.995 140.005") → (w, h) in mm.
    #[allow(dead_code)]
    fn paper_size(paper: &str) -> (f64, f64) {
        match paper.split_whitespace().collect::<Vec<_>>().as_slice() {
            ["A0"] => (1189.0, 841.0),
            ["A1"] => (841.0, 594.0),
            ["A2"] => (594.0, 420.0),
            ["A3"] => (420.0, 297.0),
            ["A4"] => (297.0, 210.0),
            ["A5"] => (210.0, 148.0),
            ["A"] => (279.4, 215.9),
            ["B"] => (431.8, 279.4),
            ["C"] => (558.8, 431.8),
            ["D"] => (863.6, 558.8),
            ["E"] => (1117.6, 863.6),
            ["User", w, h] => (w.parse().unwrap_or(200.0), h.parse().unwrap_or(150.0)),
            _ => (297.0, 210.0),
        }
    }

    /// Select smallest standard paper size that fits the board.
    /// Tries both orientations (portrait/landscape).
    fn select_paper_size(board_w: f64, board_h: f64) -> (f64, f64, &'static str) {
        // (width, height, name) — standard sizes
        let papers: &[(f64, f64, &str)] = &[
            (210.0, 148.0, "A5"),
            (297.0, 210.0, "A4"),
            (420.0, 297.0, "A3"),
            (594.0, 420.0, "A2"),
            (841.0, 594.0, "A1"),
            (1189.0, 841.0, "A0"),
        ];
        // Inner frame = page - 10mm margin - 2mm spacing, minus 34mm title block from bottom
        let frame = 12.0; // margin + spacing per side
        let tb = 34.0;

        for &(pw, ph, name) in papers {
            // Try landscape (w, h)
            let inner_w = pw - 2.0 * frame;
            let inner_h = ph - 2.0 * frame - tb;
            if board_w <= inner_w && board_h <= inner_h {
                return (pw, ph, name);
            }
            // Try portrait (h, w)
            let inner_w2 = ph - 2.0 * frame;
            let inner_h2 = pw - 2.0 * frame - tb;
            if board_w <= inner_w2 && board_h <= inner_h2 {
                return (ph, pw, name);
            }
        }
        // Fallback: A0 landscape
        (1189.0, 841.0, "A0")
    }

    /// Render the board to a standalone SVG string.
    /// Uses standard paper size selected to fit the board, with three-layer drawing sheet frame.
    pub fn render_to_string(&self) -> String {
        let (cx_min, cy_min, cx_max, cy_max) = self.compute_bbox();
        let board_w = cx_max - cx_min;
        let board_h = cy_max - cy_min;

        let (page_w, page_h, paper_name) = Self::select_paper_size(board_w, board_h);

        let margin = 10.0;
        let spacing = 2.0;
        let frame = margin + spacing;
        let tb_height = 34.0;

        // Board content centered in the inner frame area
        let inner_w = page_w - 2.0 * frame;
        let inner_h = page_h - 2.0 * frame - tb_height;
        let offset_x = frame + (inner_w - board_w) / 2.0 - cx_min;
        let offset_y = frame + (inner_h - board_h) / 2.0 - cy_min;

        let px_per_mm = self.dpi / 25.4;
        let svg_w = page_w * px_per_mm;
        let svg_h = page_h * px_per_mm;

        let mut out = String::new();

        out.push_str(&format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{:.1}" height="{:.1}" viewBox="0 0 {:.1} {:.1}" style="background-color:{}">"#,
            svg_w, svg_h, page_w, page_h, BG_COLOR
        ));

        // Drawing sheet (three-layer frame + title block) — bottom layer
        self.write_drawing_sheet(&mut out, page_w, page_h, paper_name);

        // Apply offset to board content via group transform
        out.push_str(&format!(
            r#"<g transform="translate({:.3} {:.3})">"#,
            offset_x, offset_y
        ));

        // Board content layers — mode controls which categories are drawn.
        // Assembly (default): all. Fabrication: no copper/zones/traces.
        // CopperOnly: copper traces/vias/zones/pads, no silk/mask decorations.
        self.write_board_bg(&mut out);
        let show_copper = match self.mode {
            RenderMode::Assembly | RenderMode::CopperOnly => true,
            RenderMode::Fabrication => false,
        };
        if show_copper {
            self.write_vias(&mut out);
            self.write_zones(&mut out);
            self.write_traces(&mut out);
        } else {
            // Fabrication: still draw via drill holes (for drill chart context)
            self.write_vias(&mut out);
        }
        self.write_footprints(&mut out);
        self.write_board_outline(&mut out);
        self.write_drill_marks(&mut out);

        out.push_str("</g>");
        out.push_str("</svg>");
        out
    }

    // ── Bounding box ────────────────────────────────────────────────

    #[allow(dead_code)]
    fn compute_bbox(&self) -> (f64, f64, f64, f64) {
        let mut x_min = f64::MAX;
        let mut y_min = f64::MAX;
        let mut x_max = f64::MIN;
        let mut y_max = f64::MIN;

        for fp in &self.board.footprints {
            let (fx, fy, fr) = fp.position;
            let cos_r = fr.to_radians().cos();
            let sin_r = fr.to_radians().sin();
            for pad in &fp.pads {
                let (px, py, _) = pad.position;
                let (sw, sh) = pad.size;
                let rx = px * cos_r - py * sin_r;
                let ry = px * sin_r + py * cos_r;
                let ax = fx + rx;
                let ay = fy + ry;
                let half = sw.max(sh) / 2.0;
                x_min = x_min.min(ax - half);
                y_min = y_min.min(ay - half);
                x_max = x_max.max(ax + half);
                y_max = y_max.max(ay + half);
            }
        }
        for seg in &self.board.segments {
            let w = seg.width / 2.0;
            x_min = x_min.min(seg.start.0.min(seg.end.0) - w);
            y_min = y_min.min(seg.start.1.min(seg.end.1) - w);
            x_max = x_max.max(seg.start.0.max(seg.end.0) + w);
            y_max = y_max.max(seg.start.1.max(seg.end.1) + w);
        }
        for gr in &self.board.graphics {
            match &gr.kind {
                BoardGraphicKind::Line { start, end } => {
                    x_min = x_min.min(start.0.min(end.0));
                    y_min = y_min.min(start.1.min(end.1));
                    x_max = x_max.max(start.0.max(end.0));
                    y_max = y_max.max(start.1.max(end.1));
                }
                BoardGraphicKind::Circle { center, end } => {
                    let r = ((end.0 - center.0).powi(2) + (end.1 - center.1).powi(2)).sqrt();
                    x_min = x_min.min(center.0 - r);
                    y_min = y_min.min(center.1 - r);
                    x_max = x_max.max(center.0 + r);
                    y_max = y_max.max(center.1 + r);
                }
                BoardGraphicKind::Rect { start, end } => {
                    x_min = x_min.min(start.0.min(end.0));
                    y_min = y_min.min(start.1.min(end.1));
                    x_max = x_max.max(start.0.max(end.0));
                    y_max = y_max.max(start.1.max(end.1));
                }
                _ => {}
            }
        }

        if x_min == f64::MAX {
            (0.0, 0.0, 50.0, 30.0)
        } else {
            (x_min, y_min, x_max, y_max)
        }
    }

    // ── Zone fills (copper layer color + opacity) ───────────────────
    // Matches ecad-viewer zone-painter.ts: polygon fill with copper layer color.

    fn write_zones(&self, out: &mut String) {
        for zone in &self.board.zones {
            // P1-4: layer filter — skip zones not in the whitelist.
            if !self.layer_visible(&zone.layer) {
                continue;
            }
            let color = Self::copper_color(&zone.layer);
            let fill_color = Self::color_with_opacity(color, ZONE_OPACITY);
            if !zone.filled_polygons.is_empty() {
                for fp in &zone.filled_polygons {
                    if fp.points.len() < 3 {
                        continue;
                    }
                    let pts: String = fp
                        .points
                        .iter()
                        .map(|(x, y)| format!("{:.2},{:.2}", x, y))
                        .collect::<Vec<_>>()
                        .join(" ");
                    out.push_str(&format!(
                        r#"<polygon data-layer="{}" points="{}" fill="{}" stroke="none"/>"#,
                        Self::escape_layer(&zone.layer),
                        pts,
                        fill_color
                    ));
                }
            } else if zone.outline.len() >= 3 {
                let pts: String = zone
                    .outline
                    .iter()
                    .map(|(x, y)| format!("{:.2},{:.2}", x, y))
                    .collect::<Vec<_>>()
                    .join(" ");
                out.push_str(&format!(
                    r#"<polygon data-layer="{}" points="{}" fill="{}" stroke="none"/>"#,
                    Self::escape_layer(&zone.layer),
                    pts,
                    fill_color
                ));
            }
        }
    }

    fn color_with_opacity(css: &str, alpha: f64) -> String {
        if let Some(inner) = css.strip_prefix("rgba(").and_then(|s| s.strip_suffix(')')) {
            // Already rgba — replace the alpha component
            let parts: Vec<&str> = inner.rsplitn(2, ',').collect();
            if parts.len() == 2 {
                return format!("rgba({},{:.2})", parts[1], alpha);
            }
        }
        let s = css.trim_start_matches("rgb(").trim_end_matches(')');
        format!("rgba({},{:.2})", s, alpha)
    }

    // ── Traces (copper layer color) ─────────────────────────────────
    // Matches ecad-viewer TraceSegmentPainter.

    fn write_traces(&self, out: &mut String) {
        for seg in &self.board.segments {
            // P1-4: layer filter — skip traces not in the whitelist.
            if !self.layer_visible(&seg.layer) {
                continue;
            }
            let color = Self::copper_color(&seg.layer);
            out.push_str(&format!(
                r#"<line data-layer="{}" x1="{:.3}" y1="{:.3}" x2="{:.3}" y2="{:.3}" stroke="{}" stroke-width="{:.3}" stroke-linecap="round"/>"#,
                Self::escape_layer(&seg.layer), seg.start.0, seg.start.1, seg.end.0, seg.end.1, color, seg.width
            ));
        }
    }

    // ── Vias (via_holewalls white ring + via_holes gold center) ──────
    // Matches ecad-viewer ViaPainter: HoleWalls circle(size/2) + Holes circle(drill/2).

    fn write_vias(&self, out: &mut String) {
        for via in &self.board.vias {
            // 过孔跨 F.Cu/B.Cu: 任一所跨层可见才渲染(对拍抓出的泄漏——
            // 此前任何 --layers 过滤下过孔都会画出)
            if !via.layers.iter().any(|l| self.layer_visible(l)) {
                continue;
            }
            let (x, y) = via.at;
            // Hole wall (outer ring) — via_through white/silver
            out.push_str(&format!(
                "<circle data-layer=\"Vias\" cx=\"{:.3}\" cy=\"{:.3}\" r=\"{:.3}\" fill=\"{}\"/>",
                x,
                y,
                via.size / 2.0,
                VIA_THROUGH
            ));
            // Drill hole (inner circle) — via_hole gold
            out.push_str(&format!(
                "<circle data-layer=\"Vias\" cx=\"{:.3}\" cy=\"{:.3}\" r=\"{:.3}\" fill=\"{}\"/>",
                x,
                y,
                via.drill / 2.0,
                VIA_HOLE
            ));
        }
    }

    // ── Footprints ──────────────────────────────────────────────────
    // Matches ecad-viewer FootprintPainter: translate + rotate transform,
    // then render fp_lines/fp_circles/fp_arcs/pads using layer color lookup.

    fn write_footprints(&self, out: &mut String) {
        for fp in &self.board.footprints {
            let (fx, fy, fr) = fp.position;

            // Always use transform group: translate to position + rotate
            // Matches ecad-viewer FootprintPainter matrix transform.
            out.push_str(&format!(
                r#"<g transform="translate({:.3},{:.3}) rotate({:.3})">"#,
                fx, fy, fr
            ));

            // Render order: mask/paste → pads → copper graphics → silk/fab/crtyd → text
            // Mask/paste below copper pads so copper color is visible on top.
            // P1-4: mode controls which categories appear.

            // ── 1. Mask/Paste fills (below copper) ──────────────────────
            // Hidden in CopperOnly mode (decoration, not routing).
            if self.mode != RenderMode::CopperOnly {
                self.write_fp_graphics_filtered(out, fp, |layer| {
                    (layer.ends_with(".Mask") || layer.ends_with(".Paste"))
                        && self.layer_visible(layer)
                });
            }

            // ── 2. Pads (copper layer) ──────────────────────────────────
            self.write_fp_pads(out, fp, fr);

            // ── 3. Copper-layer graphics (F.Cu/B.Cu lines/circles/etc) ──
            // Hidden in Fabrication mode (copper artwork, not fab layer).
            if self.mode != RenderMode::Fabrication {
                self.write_fp_graphics(out, fp, true);
            }

            // ── 4. Silk/Fab/Crtyd/etc (on top of copper) ───────────────
            // Hidden in CopperOnly mode (decoration, not routing).
            if self.mode != RenderMode::CopperOnly {
                self.write_fp_graphics_filtered(out, fp, |layer| {
                    !Self::is_copper_layer(layer)
                        && !layer.ends_with(".Mask")
                        && !layer.ends_with(".Paste")
                        && self.layer_visible(layer)
                });
            }

            // ── 5. Text (always on top) ─────────────────────────────────
            // Hidden in CopperOnly mode (reference designators are silk).
            if self.mode != RenderMode::CopperOnly {
                self.write_fp_text(out, fp, fr);
            }

            out.push_str("</g>");
        }
    }

    /// Render a pad shape at given position with given color and size.
    /// Used for copper pads, mask openings, and paste openings.
    #[allow(clippy::too_many_arguments)]
    fn render_pad_shape(
        out: &mut String,
        cx: f64,
        cy: f64,
        sw: f64,
        sh: f64,
        shape: &PadShape,
        rratio: Option<f64>,
        color: &str,
    ) {
        match shape {
            PadShape::Circle => {
                let r = sw.max(sh) / 2.0;
                out.push_str(&format!(
                    "<circle cx=\"{:.3}\" cy=\"{:.3}\" r=\"{:.3}\" fill=\"{}\"/>",
                    cx, cy, r, color
                ));
            }
            PadShape::Trapezoid | PadShape::RoundRect => {
                let ratio = rratio.unwrap_or(0.25);
                let rounding = sw.min(sh) * ratio;
                let half_w = sw / 2.0;
                let half_h = sh / 2.0;
                let pts = format!(
                    "{:.3},{:.3} {:.3},{:.3} {:.3},{:.3} {:.3},{:.3}",
                    cx - half_w + rounding,
                    cy - half_h + rounding,
                    cx + half_w - rounding,
                    cy - half_h + rounding,
                    cx + half_w - rounding,
                    cy + half_h - rounding,
                    cx - half_w + rounding,
                    cy + half_h - rounding
                );
                let stroke_w = rounding * 2.0;
                out.push_str(&format!(
                    "<polygon points=\"{}\" fill=\"{}\" stroke=\"{}\" stroke-width=\"{:.3}\" stroke-linejoin=\"round\"/>",
                    pts, color, color, stroke_w
                ));
            }
            PadShape::Oval => {
                let half_w = sw / 2.0;
                let half_h = sh / 2.0;
                if (sw - sh).abs() < 0.01 {
                    out.push_str(&format!(
                        "<circle cx=\"{:.3}\" cy=\"{:.3}\" r=\"{:.3}\" fill=\"{}\"/>",
                        cx,
                        cy,
                        sw / 2.0,
                        color
                    ));
                } else if sw > sh {
                    out.push_str(&format!(
                        "<line x1=\"{:.3}\" y1=\"{:.3}\" x2=\"{:.3}\" y2=\"{:.3}\" stroke=\"{}\" stroke-width=\"{:.3}\" stroke-linecap=\"round\"/>",
                        cx - half_w + half_h, cy, cx + half_w - half_h, cy, color, sh
                    ));
                } else {
                    out.push_str(&format!(
                        "<line x1=\"{:.3}\" y1=\"{:.3}\" x2=\"{:.3}\" y2=\"{:.3}\" stroke=\"{}\" stroke-width=\"{:.3}\" stroke-linecap=\"round\"/>",
                        cx, cy - half_h + half_w, cx, cy + half_h - half_w, color, sw
                    ));
                }
            }
            _ => {
                let hw = sw / 2.0;
                let hh = sh / 2.0;
                let pts = format!(
                    "{:.3},{:.3} {:.3},{:.3} {:.3},{:.3} {:.3},{:.3}",
                    cx - hw,
                    cy - hh,
                    cx + hw,
                    cy - hh,
                    cx + hw,
                    cy + hh,
                    cx - hw,
                    cy + hh
                );
                out.push_str(&format!(r#"<polygon points="{}" fill="{}"/>"#, pts, color));
            }
        }
    }

    /// Render all pads for a footprint (copper layer, bottom of z-order).
    fn write_fp_pads(&self, out: &mut String, fp: &Footprint, fr: f64) {
        for pad in &fp.pads {
            if !pad.layers.iter().any(|l| self.layer_visible(l)) {
                continue;
            }
            let (px, py, pr) = pad.position;
            let (sw, sh) = pad.size;
            let primary_layer = pad.layers.first().map(|s| s.as_str()).unwrap_or("F.Cu");
            out.push_str(&format!(
                r#"<g data-layer="{}">"#,
                Self::escape_layer(primary_layer)
            ));

            let effective_rot = pr - fr;
            let pad_needs_rotate = effective_rot.abs() > 0.01;
            if pad_needs_rotate {
                out.push_str(&format!(
                    r#"<g transform="translate({:.3},{:.3}) rotate({:.3})">"#,
                    px, py, effective_rot
                ));
            }

            let (fill_color, is_tht) = match pad.pad_type {
                PadType::ThruHole => {
                    let primary = pad.layers.first().map(|s| s.as_str()).unwrap_or("F.Cu");
                    (Self::copper_color(primary), true)
                }
                PadType::NpThruHole => (NON_PLATED_HOLE, true),
                _ => {
                    let primary = pad.layers.first().map(|s| s.as_str()).unwrap_or("F.Cu");
                    (Self::copper_color(primary), false)
                }
            };

            let drill_offset = pad.drill.as_ref().and_then(|d| d.offset);
            let (ox, oy) = drill_offset.unwrap_or((0.0, 0.0));
            let base_x = if pad_needs_rotate { 0.0 } else { px };
            let base_y = if pad_needs_rotate { 0.0 } else { py };
            let cx = base_x + ox;
            let cy = base_y + oy;

            // ── 1. Copper pad shape ───────────────────────────────────
            Self::render_pad_shape(
                out,
                cx,
                cy,
                sw,
                sh,
                &pad.shape,
                pad.roundrect_rratio,
                fill_color,
            );

            // ── 2. THT hole walls + drill hole ────────────────────────
            // ecad-viewer: :Pad:HoleWalls renders full pad shape in gold,
            // then :Pad:Holes renders drill hole in background color.
            if is_tht {
                // Gold hole wall (full pad shape, covers copper/mask/paste)
                Self::render_pad_shape(
                    out,
                    cx,
                    cy,
                    sw,
                    sh,
                    &pad.shape,
                    pad.roundrect_rratio,
                    PAD_THR_HOLE,
                );
                // Drill hole (background color center) — capsule for oval slots
                if let Some(ref drill) = pad.drill {
                    Self::write_drill_hole(out, cx, cy, drill, BG_COLOR, false);
                }
            }

            out.push_str("</g>");
            if pad_needs_rotate {
                out.push_str("</g>");
            }
        }
    }

    /// Render footprint graphics filtered by layer predicate.
    fn write_fp_graphics_filtered<F: Fn(&str) -> bool>(
        &self,
        out: &mut String,
        fp: &Footprint,
        filter: F,
    ) {
        // fp_lines
        for line in &fp.fp_lines {
            if !filter(&line.layer) {
                continue;
            }
            let color = Self::layer_color_with_alpha(&line.layer);
            out.push_str(&format!(
                r#"<line data-layer="{}" x1="{:.3}" y1="{:.3}" x2="{:.3}" y2="{:.3}" stroke="{}" stroke-width="{:.3}" stroke-linecap="round"/>"#,
                Self::escape_layer(&line.layer), line.start.0, line.start.1, line.end.0, line.end.1, color, line.stroke_width
            ));
        }

        // fp_circles
        for circ in &fp.fp_circles {
            if !filter(&circ.layer) {
                continue;
            }
            let (cx, cy) = circ.center;
            let (ex, ey) = circ.end;
            let r = ((ex - cx).powi(2) + (ey - cy).powi(2)).sqrt();
            let color = Self::layer_color_with_alpha(&circ.layer);
            if circ.fill {
                let fill_r = r + circ.stroke_width;
                out.push_str(&format!(
                    "<circle data-layer=\"{}\" cx=\"{:.3}\" cy=\"{:.3}\" r=\"{:.3}\" fill=\"{}\" stroke=\"none\"/>",
                    Self::escape_layer(&circ.layer), cx, cy, fill_r, color
                ));
            } else {
                out.push_str(&format!(
                    "<circle data-layer=\"{}\" cx=\"{:.3}\" cy=\"{:.3}\" r=\"{:.3}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{:.3}\"/>",
                    Self::escape_layer(&circ.layer), cx, cy, r, color, circ.stroke_width
                ));
            }
        }

        // fp_arcs
        for arc in &fp.fp_arcs {
            if !filter(&arc.layer) {
                continue;
            }
            let color = Self::layer_color_with_alpha(&arc.layer);
            let start = arc.start;
            let mid = arc.mid;
            let end = arc.end;
            let cross =
                (mid.0 - start.0) * (end.1 - start.1) - (mid.1 - start.1) * (end.0 - start.0);
            let sweep = if cross > 0.0 { 0 } else { 1 };
            let (cx, cy) = arc_center(&start, &mid, &end);
            let r = ((start.0 - cx).powi(2) + (start.1 - cy).powi(2)).sqrt();
            out.push_str(&format!(
                "<path data-layer=\"{}\" d=\"M {:.3} {:.3} A {:.3} {:.3} 0 0 {} {:.3} {:.3}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{:.3}\"/>",
                Self::escape_layer(&arc.layer), start.0, start.1, r, r, sweep, end.0, end.1, color, arc.stroke_width
            ));
        }

        // fp_rects
        for rect in &fp.fp_rects {
            if !filter(&rect.layer) {
                continue;
            }
            let color = Self::layer_color_with_alpha(&rect.layer);
            let (sx, sy) = rect.start;
            let (ex, ey) = rect.end;
            let points = format!(
                "{:.3},{:.3} {:.3},{:.3} {:.3},{:.3} {:.3},{:.3} {:.3},{:.3}",
                sx, sy, sx, ey, ex, ey, ex, sy, sx, sy
            );
            out.push_str(&format!(
                r#"<polyline data-layer="{}" points="{}" fill="none" stroke="{}" stroke-width="{:.3}" stroke-linejoin="round"/>"#,
                Self::escape_layer(&rect.layer), points, color, rect.stroke_width
            ));
            if rect.fill {
                out.push_str(&format!(
                    r#"<polygon data-layer="{}" points="{}" fill="{}" stroke="none"/>"#,
                    Self::escape_layer(&rect.layer),
                    points,
                    color
                ));
            }
        }

        // fp_polys
        for poly in &fp.fp_polys {
            if poly.points.len() < 2 {
                continue;
            }
            if !filter(&poly.layer) {
                continue;
            }
            let pts: String = poly
                .points
                .iter()
                .map(|(x, y)| format!("{:.3},{:.3}", x, y))
                .collect::<Vec<_>>()
                .join(" ");
            let color = Self::layer_color_with_alpha(&poly.layer);
            if poly.stroke_width > 0.0 {
                let closed_pts = format!("{} {:.3},{:.3}", pts, poly.points[0].0, poly.points[0].1);
                out.push_str(&format!(
                    "<polyline data-layer=\"{}\" points=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{:.3}\" stroke-linejoin=\"round\"/>",
                    Self::escape_layer(&poly.layer), closed_pts, color, poly.stroke_width
                ));
            }
            if poly.fill {
                out.push_str(&format!(
                    "<polygon points=\"{}\" fill=\"{}\" stroke=\"none\"/>",
                    pts, color
                ));
            }
        }
    }

    /// Render footprint graphics by copper/non-copper layer type.
    fn write_fp_graphics(&self, out: &mut String, fp: &Footprint, copper_only: bool) {
        self.write_fp_graphics_filtered(out, fp, |layer| {
            Self::is_copper_layer(layer) == copper_only
        });
    }

    /// Render footprint text (always on top of all graphics).
    fn write_fp_text(&self, out: &mut String, fp: &Footprint, fr: f64) {
        for txt in &fp.fp_texts {
            if !self.layer_visible(&txt.layer) {
                continue;
            }
            // KiCad 属性占位符: ${REFERENCE}/${VALUE} 解析为实际值后再渲染
            // (kicad-cli 丝印上输出 J6 等真实位号; 此前 ${ 开头直接跳过导致
            // 位号丝印整批消失——对拍抓出)。
            let text: String = match txt.text.as_str() {
                "${REFERENCE}" => fp.reference.clone(),
                "${VALUE}" => fp.value.clone(),
                other => other.to_string(),
            };
            if text.is_empty() || text.starts_with("${") {
                continue;
            }
            // ecad-viewer FpTextPainter: uses layer.color (with alpha 0.8 for non-copper).
            let color = Self::layer_color_with_alpha(&txt.layer);
            let (tx, ty, local_tr) = txt.position;
            let mut abs_angle = fr + local_tr;
            while abs_angle > 90.0 {
                abs_angle -= 180.0;
            }
            while abs_angle <= -90.0 {
                abs_angle += 180.0;
            }
            // 文本角度官方约定: 文件角 a → rotate(-a)（kicad-cli 实证）。组内
            // 已含 +fr，此处补发 -abs-fr 使屏幕净旋转 = -abs 与官方一致。
            let svg_rot = -abs_angle - fr;
            let fs = txt.font_size.0.max(txt.font_size.1);
            // 多行文本: 字面 "\n" 切分, 块以 (tx,ty) 为中心分布
            let lines: Vec<&str> = text.split("\\n").collect();
            let line_h = fs * crate::constants::INTERLINE_PITCH_RATIO;
            let n = lines.len() as f64;
            for (i, line) in lines.iter().enumerate() {
                let y = ty + (i as f64 - (n - 1.0) / 2.0) * line_h;
                out.push_str(&format!(
                    "<text data-layer=\"{}\" x=\"{:.3}\" y=\"{:.3}\" fill=\"{}\" font-size=\"{:.3}\" text-anchor=\"middle\" dominant-baseline=\"central\"{}>{}</text>",
                    Self::escape_layer(&txt.layer), tx, y, color, fs,
                    if svg_rot.abs() > 0.01 { format!(" transform=\"rotate({:.3},{:.3},{:.3})\"", svg_rot, tx, ty) } else { String::new() },
                    xml_escape(line)
                ));
            }
        }

        // KiCad 8+ 把位号/值存为 (property ...) 块而非 fp_text（v7 旧格式才是
        // fp_text reference/value）——不渲染 properties_ext 会让 KiCad 8 板的
        // 丝印位号整批消失（对拍抓出）。
        for prop in &fp.properties_ext {
            if prop.hide || !matches!(prop.name.as_str(), "Reference" | "Value" | "User") {
                continue;
            }
            if !self.layer_visible(&prop.layer) || prop.value.is_empty() {
                continue;
            }
            let color = Self::layer_color_with_alpha(&prop.layer);
            let (px, py, prot) = prop.position;
            let svg_rot = -prot;
            let fs = prop.effects.font_size.0.max(prop.effects.font_size.1);
            out.push_str(&format!(
                "<text data-layer=\"{}\" x=\"{:.3}\" y=\"{:.3}\" fill=\"{}\" font-size=\"{:.3}\" text-anchor=\"middle\" dominant-baseline=\"central\"{}>{}</text>",
                Self::escape_layer(&prop.layer), px, py, color, fs,
                if svg_rot.abs() > 0.01 { format!(" transform=\"rotate({:.3},{:.3},{:.3})\"", svg_rot, px, py) } else { String::new() },
                xml_escape(&prop.value)
            ));
        }
    }

    // ── Board outline ───────────────────────────────────────────────

    /// Fill the board area (inside Edge.Cuts) with substrate background color.
    fn write_board_bg(&self, out: &mut String) {
        // First try: a single Rect on Edge.Cuts
        for gr in &self.board.graphics {
            if gr.layer != "Edge.Cuts" {
                continue;
            }
            if let BoardGraphicKind::Rect { start, end } = &gr.kind {
                let pts = format!(
                    "{:.3},{:.3} {:.3},{:.3} {:.3},{:.3} {:.3},{:.3}",
                    start.0, start.1, start.0, end.1, end.0, end.1, end.0, start.1
                );
                out.push_str(&format!(
                    r#"<polygon points="{}" fill="{}" stroke="none"/>"#,
                    pts, BOARD_SUBSTRATE
                ));
                return;
            }
            if let BoardGraphicKind::Poly { points } = &gr.kind {
                if points.len() >= 3 {
                    let pts: String = points
                        .iter()
                        .map(|(x, y)| format!("{:.3},{:.3}", x, y))
                        .collect::<Vec<_>>()
                        .join(" ");
                    out.push_str(&format!(
                        r#"<polygon points="{}" fill="{}" stroke="none"/>"#,
                        pts, BOARD_SUBSTRATE
                    ));
                    return;
                }
            }
        }

        // Fallback: chain Edge.Cuts Line segments into a closed polygon
        let mut segments: Vec<((f64, f64), (f64, f64))> = self
            .board
            .graphics
            .iter()
            .filter(|gr| gr.layer == "Edge.Cuts")
            .filter_map(|gr| match &gr.kind {
                BoardGraphicKind::Line { start, end } => Some((*start, *end)),
                _ => None,
            })
            .collect();

        if segments.is_empty() {
            return;
        }

        let eps = 0.01;
        let mut outline = vec![segments[0].0, segments[0].1];
        segments.remove(0);

        while !segments.is_empty() {
            let last = *outline.last().unwrap();
            let found = segments.iter().position(|(a, b)| {
                (a.0 - last.0).abs() < eps && (a.1 - last.1).abs() < eps
                    || (b.0 - last.0).abs() < eps && (b.1 - last.1).abs() < eps
            });
            if let Some(idx) = found {
                let (a, b) = segments.remove(idx);
                if (a.0 - last.0).abs() < eps && (a.1 - last.1).abs() < eps {
                    outline.push(b);
                } else {
                    outline.push(a);
                }
            } else {
                break;
            }
        }

        if outline.len() >= 3 {
            let pts: String = outline
                .iter()
                .map(|(x, y)| format!("{:.3},{:.3}", x, y))
                .collect::<Vec<_>>()
                .join(" ");
            out.push_str(&format!(
                r#"<polygon points="{}" fill="{}" stroke="none"/>"#,
                pts, BOARD_SUBSTRATE
            ));
        }
    }

    fn write_board_outline(&self, out: &mut String) {
        for gr in &self.board.graphics {
            if !self.layer_visible(&gr.layer) {
                continue;
            }
            out.push_str(&format!(
                r#"<g data-layer="{}">"#,
                Self::escape_layer(&gr.layer)
            ));
            // ecad-viewer: non-copper layers get alpha 0.8 via color_for().
            let color = if Self::is_copper_layer(&gr.layer) {
                Self::layer_color(&gr.layer).to_string()
            } else {
                Self::layer_color_with_alpha(&gr.layer)
            };
            self.write_graphic(out, gr, color);
            out.push_str("</g>");
        }
    }

    fn write_graphic(&self, out: &mut String, gr: &BoardGraphic, color: String) {
        match &gr.kind {
            BoardGraphicKind::Line { start, end } => {
                out.push_str(&format!(
                    "<line x1=\"{:.3}\" y1=\"{:.3}\" x2=\"{:.3}\" y2=\"{:.3}\" stroke=\"{}\" stroke-width=\"{:.3}\" stroke-linecap=\"round\"/>",
                    start.0, start.1, end.0, end.1, color, gr.stroke_width
                ));
            }
            BoardGraphicKind::Circle { center, end } => {
                let r = ((end.0 - center.0).powi(2) + (end.1 - center.1).powi(2)).sqrt();
                if gr.fill {
                    // ecad-viewer CirclePainter: filled circle uses r + stroke_width
                    let fill_r = r + gr.stroke_width;
                    out.push_str(&format!(
                        "<circle cx=\"{:.3}\" cy=\"{:.3}\" r=\"{:.3}\" fill=\"{}\" stroke=\"none\"/>",
                        center.0, center.1, fill_r, color
                    ));
                } else {
                    out.push_str(&format!(
                        "<circle cx=\"{:.3}\" cy=\"{:.3}\" r=\"{:.3}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{:.3}\"/>",
                        center.0, center.1, r, color, gr.stroke_width
                    ));
                }
            }
            BoardGraphicKind::Rect { start, end } => {
                // ecad-viewer RectPainter: polyline (closed) outline + polygon fill
                let points = format!(
                    "{:.3},{:.3} {:.3},{:.3} {:.3},{:.3} {:.3},{:.3} {:.3},{:.3}",
                    start.0,
                    start.1,
                    start.0,
                    end.1,
                    end.0,
                    end.1,
                    end.0,
                    start.1,
                    start.0,
                    start.1
                );
                // Always draw polyline outline
                out.push_str(&format!(
                    r#"<polyline points="{}" fill="none" stroke="{}" stroke-width="{:.3}" stroke-linejoin="round"/>"#,
                    points, color, gr.stroke_width
                ));
                // Fill on top if needed
                if gr.fill {
                    out.push_str(&format!(
                        r#"<polygon points="{}" fill="{}" stroke="none"/>"#,
                        points, color
                    ));
                }
            }
            BoardGraphicKind::Poly { points } => {
                let pts: String = points
                    .iter()
                    .map(|(x, y)| format!("{:.3},{:.3}", x, y))
                    .collect::<Vec<_>>()
                    .join(" ");
                // ecad-viewer PolyPainter: stroke outline + fill
                if gr.stroke_width > 0.0 {
                    let closed_pts = format!("{} {:.3},{:.3}", pts, points[0].0, points[0].1);
                    out.push_str(&format!(
                        "<polyline points=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{:.3}\" stroke-linejoin=\"round\"/>",
                        closed_pts, color, gr.stroke_width
                    ));
                }
                if gr.fill {
                    out.push_str(&format!(
                        "<polygon points=\"{}\" fill=\"{}\" stroke=\"none\"/>",
                        pts, color
                    ));
                }
            }
            BoardGraphicKind::Arc { start, mid, end } => {
                let cross =
                    (mid.0 - start.0) * (end.1 - start.1) - (mid.1 - start.1) * (end.0 - start.0);
                let sweep = if cross > 0.0 { 0 } else { 1 };
                let (cx, cy) = arc_center(start, mid, end);
                let r = ((start.0 - cx).powi(2) + (start.1 - cy).powi(2)).sqrt();
                out.push_str(&format!(
                    "<path d=\"M {:.3} {:.3} A {:.3} {:.3} 0 0 {} {:.3} {:.3}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{:.3}\"/>",
                    start.0, start.1, r, r, sweep, end.0, end.1, color, gr.stroke_width
                ));
            }
            BoardGraphicKind::Text {
                text,
                position,
                font_size,
            } => {
                // KiCad 文件角 90° 在官方 SVG 输出为 rotate(-90)（kicad-cli
                // 实证）——KiCad 角为 y-down 系下的"逆时针"。
                let transform = if position.2.abs() > 0.01 {
                    format!(
                        " transform=\"rotate({:.3},{:.3},{:.3})\"",
                        -position.2, position.0, position.1
                    )
                } else {
                    String::new()
                };
                // 多行文本: 字面 "\n" 切分, 行距 INTERLINE_PITCH_RATIO, 块以
                // position 为中心上下分布(与 KiCad 多行文本居中语义一致)。
                let lines: Vec<&str> = text.split("\\n").collect();
                let line_h = font_size * crate::constants::INTERLINE_PITCH_RATIO;
                let n = lines.len() as f64;
                for (i, line) in lines.iter().enumerate() {
                    let y = position.1 + (i as f64 - (n - 1.0) / 2.0) * line_h;
                    out.push_str(&format!(
                        "<text x=\"{:.3}\" y=\"{:.3}\" fill=\"{}\" font-size=\"{:.1}\" text-anchor=\"middle\" dominant-baseline=\"central\"{}>{}</text>",
                        position.0, y, color, font_size, transform, xml_escape(line)
                    ));
                }
            }
        }
    }

    // ── Drawing Sheet (border + grid marks + title block) ────────────
    // Matches ecad-viewer's default_drawing_sheet.kicad_wks + DrawingSheetPainter.
    // Margins: 10mm all sides, linewidth: 0.15, title block: 110×34mm at rbcorner.

    fn write_drawing_sheet(&self, out: &mut String, page_w: f64, page_h: f64, paper_name: &str) {
        let lw = 0.15;
        let ws_color = WORKSHEET_COLOR;
        let margin = 10.0;
        let sp = 2.0;

        // ── Layer 1: Outer border (page outline) ──
        out.push_str(&format!(
            r#"<rect x="0" y="0" width="{:.1}" height="{:.1}" fill="none" stroke="{}" stroke-width="{:.2}"/>"#,
            page_w, page_h, ws_color, lw
        ));

        let cl = margin;
        let ct = margin;
        let cr = page_w - margin;
        let cb = page_h - margin;

        // ── Layer 2: Middle border (10mm margin) ──
        out.push_str(&format!(
            r#"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" fill="none" stroke="{}" stroke-width="{:.2}"/>"#,
            cl, ct, cr - cl, cb - ct, ws_color, lw
        ));

        // ── Layer 3: Inner border (2mm inset) ──
        out.push_str(&format!(
            r#"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" fill="none" stroke="{}" stroke-width="{:.2}"/>"#,
            cl + sp, ct + sp, cr - cl - 2.0 * sp, cb - ct - 2.0 * sp, ws_color, lw
        ));

        // ── Grid reference marks along top edge (between layer 2 and 3) ──
        let mut x = cl + 50.0;
        let mut col = 1u32;
        while x < cr - 1.0 {
            out.push_str(&format!(
                r#"<line x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}" stroke="{}" stroke-width="{:.2}"/>"#,
                x, ct, x, ct + sp, ws_color, lw
            ));
            out.push_str(&format!(
                "<text x=\"{:.1}\" y=\"{:.1}\" fill=\"{}\" font-size=\"1.3\" text-anchor=\"middle\" dominant-baseline=\"central\">{}</text>",
                x - 25.0, ct + 1.0, ws_color, col
            ));
            col += 1;
            x += 50.0;
        }

        // ── Grid reference marks along left edge ──
        let mut y = ct + 50.0;
        let mut row = b'A';
        while y < cb - 1.0 {
            out.push_str(&format!(
                r#"<line x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}" stroke="{}" stroke-width="{:.2}"/>"#,
                cl, y, cl + sp, y, ws_color, lw
            ));
            out.push_str(&format!(
                "<text x=\"{:.1}\" y=\"{:.1}\" fill=\"{}\" font-size=\"1.3\" text-anchor=\"middle\" dominant-baseline=\"central\">{}</text>",
                cl + 1.0, y - 25.0, ws_color, row as char
            ));
            row += 1;
            y += 50.0;
        }

        // ── Title block (adaptive size at bottom-right) ──
        let tb_w: f64 = 110.0_f64.min((cr - cl) * 0.8);
        let tb_h: f64 = 34.0_f64.min((cb - ct) * 0.4);
        let tb_l = cr - tb_w;
        let tb_t = cb - tb_h;

        out.push_str(&format!(
            r#"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" fill="none" stroke="{}" stroke-width="{:.2}"/>"#,
            tb_l, tb_t, tb_w, tb_h, ws_color, lw
        ));

        let text_color = ws_color;
        let _tb = self.board.title_block.as_ref();

        out.push_str(&format!(
            "<text x=\"{:.1}\" y=\"{:.1}\" fill=\"{}\" font-size=\"1.5\" text-anchor=\"end\">Sheet: /</text>",
            cr - 3.0, tb_t + 17.0, text_color
        ));

        out.push_str(&format!(
            "<text x=\"{:.1}\" y=\"{:.1}\" fill=\"{}\" font-size=\"1.5\" text-anchor=\"end\">Size: {}</text>",
            cr - 3.0, tb_t + 6.9, text_color, paper_name
        ));

        out.push_str(&format!(
            "<text x=\"{:.1}\" y=\"{:.1}\" fill=\"{}\" font-size=\"1.5\">Id: 1/1</text>",
            tb_l + 3.0,
            tb_t + 4.1,
            text_color
        ));

        out.push_str(&format!(
            "<text x=\"{:.1}\" y=\"{:.1}\" fill=\"{}\" font-size=\"1.5\" text-anchor=\"end\">{}</text>",
            cr - 3.0, tb_t + 4.1, text_color, xml_escape(&self.board.generator)
        ));
    }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Compute arc center from three points (start, mid, end) on the arc.
fn arc_center(start: &(f64, f64), mid: &(f64, f64), end: &(f64, f64)) -> (f64, f64) {
    let (x1, y1) = *start;
    let (x2, y2) = *mid;
    let (x3, y3) = *end;
    let d = 2.0 * (x1 * (y2 - y3) + x2 * (y3 - y1) + x3 * (y1 - y2));
    if d.abs() < 1e-10 {
        return ((x1 + x3) / 2.0, (y1 + y3) / 2.0);
    }
    let a = x1 * x1 + y1 * y1;
    let b = x2 * x2 + y2 * y2;
    let c = x3 * x3 + y3 * y3;
    let cx = (a * (y2 - y3) + b * (y3 - y1) + c * (y1 - y2)) / d;
    let cy = (a * (x3 - x2) + b * (x1 - x3) + c * (x2 - x1)) / d;
    (cx, cy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_empty_board() {
        let board = Board::new();
        let svg = PcbRenderer::new(&board).render_to_string();
        assert!(svg.starts_with("<svg"));
        assert!(svg.ends_with("</svg>"));
    }

    #[test]
    fn test_render_board_with_footprint() {
        let mut board = Board::new();
        let mut fp = Footprint::new("Test:IC", "U1", "TestIC");
        fp.position = (25.0, 25.0, 0.0);
        fp.pads.push(Pad {
            number: "1".into(),
            pad_type: PadType::Smd,
            shape: PadShape::Rect,
            position: (-1.0, 0.0, 0.0),
            size: (0.6, 1.2),
            layers: vec!["F.Cu".into()],
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
        });
        board.footprints.push(fp);
        let svg = PcbRenderer::new(&board).render_to_string();
        assert!(svg.contains("transform=\"translate(25.000,25.000) rotate(0.000)\""));
    }

    #[test]
    fn test_render_rotated_footprint() {
        let mut board = Board::new();
        let mut fp = Footprint::new("Test:IC", "U1", "TestIC");
        fp.position = (25.0, 25.0, 90.0);
        fp.fp_lines.push(FpLine {
            start: (-1.0, 0.0),
            end: (1.0, 0.0),
            stroke_width: 0.12,
            layer: "F.SilkS".into(),
        });
        board.footprints.push(fp);
        let svg = PcbRenderer::new(&board).render_to_string();
        assert!(svg.contains("transform=\"translate(25.000,25.000) rotate(90.000)\""));
    }

    #[test]
    fn test_render_via() {
        let mut board = Board::new();
        board.vias.push(Via {
            at: (15.0, 20.0),
            size: 0.6,
            drill: 0.3,
            layers: vec!["F.Cu".into(), "B.Cu".into()],
            net: 0,
        });
        let svg = PcbRenderer::new(&board).render_to_string();
        assert!(svg.contains(VIA_THROUGH));
        assert!(svg.contains(VIA_HOLE));
    }

    #[test]
    fn test_render_zone() {
        let mut board = Board::new();
        board.add_net("GND");
        board.zones.push(Zone {
            net: 1,
            net_name: "GND".into(),
            layer: "F.Cu".into(),
            hatch_style: "full".into(),
            hatch_pitch: 0.508,
            connect_pads_clearance: 0.3,
            min_thickness: 0.254,
            fill: true,
            outline: vec![(0.0, 0.0), (50.0, 0.0), (50.0, 30.0), (0.0, 30.0)],
            filled_polygons: Vec::new(),
            keepout: None,
            pad_connect: String::new(),
            thermal_gap: 0.508,
            thermal_bridge_width: 0.508,
            island_removal_mode: 0,
            island_area: 10.0,
        });
        let svg = PcbRenderer::new(&board).render_to_string();
        assert!(svg.contains("polygon"));
        // Zone now uses opacity 0.6
        assert!(svg.contains("rgba(200,52,52,0.60)") || svg.contains(COPPER_F));
    }

    #[test]
    fn test_render_tht_pad_with_drill() {
        let mut board = Board::new();
        let mut fp = Footprint::new("Test:Hole", "H1", "M3");
        fp.pads.push(Pad {
            number: "1".into(),
            pad_type: PadType::ThruHole,
            shape: PadShape::Circle,
            position: (0.0, 0.0, 0.0),
            size: (6.0, 6.0),
            layers: vec!["*.Cu".into()],
            drill: Some(DrillDef {
                diameter: 3.0,
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
        board.footprints.push(fp);
        let svg = PcbRenderer::new(&board).render_to_string();
        // THT pad: gold hole wall ring + background drill hole
        assert!(svg.contains(PAD_THR_HOLE));
        assert!(svg.contains(BG_COLOR));
    }

    #[test]
    fn test_layer_color_lookup() {
        assert_eq!(PcbRenderer::layer_color("F.SilkS"), SILK_F);
        assert_eq!(PcbRenderer::layer_color("B.Fab"), FAB_B);
        assert_eq!(PcbRenderer::layer_color("F.CrtYd"), CRTYD_F);
        assert_eq!(PcbRenderer::layer_color("Edge.Cuts"), EDGE_CUTS);
        assert_eq!(PcbRenderer::layer_color("Dwgs.User"), DWGS_USER_COLOR);
        assert_eq!(PcbRenderer::layer_color("Cmts.User"), CMTS_USER_COLOR);
    }

    // ── P1-4: RenderMode + LayerFilter tests ──────────────────────────

    fn make_board_with_traces() -> Board {
        let mut board = Board::new();
        // Two traces on different copper layers.
        board.segments.push(Segment {
            start: (10.0, 10.0),
            end: (20.0, 10.0),
            width: 0.25,
            layer: "F.Cu".into(),
            net: 1,
        });
        board.segments.push(Segment {
            start: (10.0, 20.0),
            end: (20.0, 20.0),
            width: 0.25,
            layer: "B.Cu".into(),
            net: 2,
        });
        // A silkscreen line on a footprint.
        let mut fp = Footprint::new("Test:R", "R1", "1k");
        fp.position = (15.0, 15.0, 0.0);
        fp.fp_lines.push(FpLine {
            start: (-2.0, 0.0),
            end: (2.0, 0.0),
            stroke_width: 0.15,
            layer: "F.SilkS".into(),
        });
        board.footprints.push(fp);
        board
    }

    #[test]
    fn test_render_mode_assembly_shows_traces() {
        // Default Assembly mode renders copper traces + silkscreen.
        let board = make_board_with_traces();
        let svg = PcbRenderer::new(&board).render_to_string();
        // Copper traces use COPPER_F/COPPER_B directly (no alpha wrapping).
        assert!(svg.contains(COPPER_F), "Assembly should show F.Cu traces");
        assert!(svg.contains(COPPER_B), "Assembly should show B.Cu traces");
        // Silkscreen color appears (as rgba with alpha, so check the rgb triplet).
        assert!(
            svg.contains("242,237,161"),
            "Assembly should show silkscreen color"
        );
    }

    #[test]
    fn test_render_mode_fabrication_hides_traces() {
        // Fabrication mode hides copper traces/zones but keeps vias + silk.
        let board = make_board_with_traces();
        let svg = PcbRenderer::new(&board)
            .with_mode(RenderMode::Fabrication)
            .render_to_string();
        assert!(
            !svg.contains(COPPER_F),
            "Fabrication should NOT show F.Cu traces"
        );
        assert!(
            !svg.contains(COPPER_B),
            "Fabrication should NOT show B.Cu traces"
        );
        // Silkscreen still visible (footprint silk graphics are kept).
        assert!(
            svg.contains("242,237,161"),
            "Fabrication should still show silkscreen"
        );
    }

    #[test]
    fn test_render_mode_copper_only_hides_silk() {
        // CopperOnly mode shows traces but hides silkscreen decorations.
        let board = make_board_with_traces();
        let svg = PcbRenderer::new(&board)
            .with_mode(RenderMode::CopperOnly)
            .render_to_string();
        assert!(svg.contains(COPPER_F), "CopperOnly should show F.Cu traces");
        assert!(
            !svg.contains("242,237,161"),
            "CopperOnly should hide silkscreen"
        );
    }

    #[test]
    fn test_layer_filter_fc_only() {
        // Filter to only F.Cu → B.Cu trace is skipped.
        let board = make_board_with_traces();
        let mut filter = HashSet::new();
        filter.insert("F.Cu".to_string());
        let svg = PcbRenderer::new(&board)
            .with_layer_filter(filter)
            .render_to_string();
        assert!(svg.contains(COPPER_F), "F.Cu trace should be visible");
        assert!(!svg.contains(COPPER_B), "B.Cu trace should be filtered out");
    }

    #[test]
    fn test_layered_output_carries_data_layer() {
        // Every layer-aware element must carry data-layer for the HTML
        // layer viewer to group/toggle.
        let board = make_board_with_traces();
        let svg = PcbRenderer::new(&board).render_to_string();
        assert!(
            svg.contains("data-layer=\"F.Cu\""),
            "F.Cu traces must be tagged"
        );
        assert!(
            svg.contains("data-layer=\"B.Cu\""),
            "B.Cu traces must be tagged"
        );
    }

    #[test]
    fn test_layer_filter_excludes_other_layers() {
        // F.Cu-only render must not carry any B.Cu-tagged element.
        let board = make_board_with_traces();
        let mut filter = HashSet::new();
        filter.insert("F.Cu".to_string());
        let svg = PcbRenderer::new(&board)
            .with_layer_filter(filter)
            .render_to_string();
        assert!(svg.contains("data-layer=\"F.Cu\""));
        assert!(
            !svg.contains("data-layer=\"B.Cu\""),
            "B.Cu must be filtered out"
        );
    }

    #[test]
    fn test_layer_filter_fp_graphics_and_pads() {
        // Footprint graphics and pads honor the filter too.
        let mut board = Board::new();
        let mut fp = Footprint::new("Test:IC", "U1", "TestIC");
        fp.position = (10.0, 10.0, 0.0);
        fp.fp_lines.push(FpLine {
            start: (-1.0, 0.0),
            end: (1.0, 0.0),
            stroke_width: 0.12,
            layer: "F.SilkS".into(),
        });
        fp.fp_lines.push(FpLine {
            start: (-1.0, 1.0),
            end: (1.0, 1.0),
            stroke_width: 0.12,
            layer: "B.SilkS".into(),
        });
        fp.pads.push(Pad {
            number: "1".into(),
            pad_type: PadType::Smd,
            shape: PadShape::Rect,
            position: (-1.0, 0.0, 0.0),
            size: (0.6, 1.2),
            layers: vec!["F.Cu".into()],
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
        });
        board.footprints.push(fp);

        let mut filter = HashSet::new();
        filter.insert("F.Cu".to_string());
        let svg = PcbRenderer::new(&board)
            .with_layer_filter(filter)
            .render_to_string();
        assert!(svg.contains("data-layer=\"F.Cu\""), "F.Cu pad must render");
        assert!(
            !svg.contains("data-layer=\"F.SilkS\"") && !svg.contains("data-layer=\"B.SilkS\""),
            "non-whitelisted footprint graphics must be filtered out"
        );
    }

    #[test]
    fn test_gr_text_rotation_and_multiline() {
        // 文件角 90 → rotate(-90)（kicad-cli 官方 SVG 实证）; "\n" 切多行
        let mut board = Board::new();
        board.graphics.push(BoardGraphic {
            kind: BoardGraphicKind::Text {
                text: "RESET\\nTARGET".into(),
                position: (80.8, 26.0, 90.0),
                font_size: 0.7,
            },
            layer: "F.SilkS".into(),
            stroke_width: 0.0,
            fill: false,
        });
        let svg = PcbRenderer::new(&board).render_to_string();
        assert!(
            svg.contains("rotate(-90.000,80.800,26.000)"),
            "kicad 90 must emit rotate(-90)"
        );
        assert_eq!(svg.matches(">RESET<").count(), 1, "line 1 split out");
        assert_eq!(svg.matches(">TARGET<").count(), 1, "line 2 split out");
        assert!(
            !svg.contains("RESET\\n"),
            "literal backslash-n must be split"
        );
    }

    #[test]
    fn test_fp_text_placeholder_resolution() {
        // ${REFERENCE}/${VALUE} 解析为真实值（位号丝印）
        let mut board = Board::new();
        let mut fp = Footprint::new("Test:R", "R7", "4k7");
        fp.position = (10.0, 10.0, 0.0);
        fp.fp_texts.push(FpText {
            text: "${REFERENCE}".into(),
            text_type: FpTextType::Reference,
            position: (0.0, -1.5, 0.0),
            layer: "F.SilkS".into(),
            font_size: (1.0, 1.0),
        });
        fp.fp_texts.push(FpText {
            text: "${VALUE}".into(),
            text_type: FpTextType::Value,
            position: (0.0, 1.5, 0.0),
            layer: "F.SilkS".into(),
            font_size: (1.0, 1.0),
        });
        board.footprints.push(fp);
        let svg = PcbRenderer::new(&board).render_to_string();
        assert!(
            svg.contains(">R7<"),
            "reference placeholder resolved, got no R7"
        );
        assert!(svg.contains(">4k7<"), "value placeholder resolved");
    }

    #[test]
    fn test_oval_drill_capsule() {
        // (drill oval W H) → 胶囊孔（rect rx），圆 drill 保持 circle
        let mut board = Board::new();
        let mut fp = Footprint::new("Test:Slot", "S1", "slot");
        fp.position = (10.0, 10.0, 0.0);
        fp.pads.push(Pad {
            number: "1".into(),
            pad_type: PadType::ThruHole,
            shape: PadShape::Oval,
            position: (0.0, 0.0, 0.0),
            size: (2.2, 1.7),
            layers: vec!["*.Cu".into()],
            drill: Some(DrillDef {
                diameter: 1.2,
                width: Some(1.7),
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
        board.footprints.push(fp);
        let svg = PcbRenderer::new(&board).render_to_string();
        assert!(svg.contains("<rect"), "oval drill renders as capsule rect");
    }

    #[test]
    fn test_drill_marks_on_technical_filter() {
        // 非铜层过滤出孔轮廓标记（kicad-cli silk/edge 导出行为）；无过滤不画
        let mut board = make_board_with_traces();
        let mut fp = Footprint::new("Test:Hole", "H1", "3mm");
        fp.position = (30.0, 30.0, 0.0);
        fp.pads.push(Pad {
            number: "1".into(),
            pad_type: PadType::NpThruHole,
            shape: PadShape::Circle,
            position: (0.0, 0.0, 0.0),
            size: (3.2, 3.2),
            layers: vec!["*.Cu".into()],
            drill: Some(DrillDef {
                diameter: 3.0,
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
        board.footprints.push(fp);
        let mut f = HashSet::new();
        f.insert("F.SilkS".to_string());
        let filtered = PcbRenderer::new(&board)
            .with_layer_filter(f)
            .render_to_string();
        assert!(
            filtered.contains("data-layer=\"Drills\""),
            "silk filter shows drill marks"
        );
        let full = PcbRenderer::new(&board).render_to_string();
        assert!(
            !full.contains("data-layer=\"Drills\""),
            "full render keeps holes in pads"
        );
    }

    #[test]
    fn test_layer_filter_wildcard_copper() {
        // Filter "*.Cu" matches all copper layers.
        let board = make_board_with_traces();
        let mut filter = HashSet::new();
        filter.insert("*.Cu".to_string());
        let svg = PcbRenderer::new(&board)
            .with_layer_filter(filter)
            .render_to_string();
        assert!(svg.contains(COPPER_F), "*.Cu should match F.Cu");
        assert!(svg.contains(COPPER_B), "*.Cu should match B.Cu");
    }
}
