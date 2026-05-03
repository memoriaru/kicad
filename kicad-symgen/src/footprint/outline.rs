/// Silkscreen / Fab / Courtyard outline lines
#[derive(Debug, Clone)]
pub struct OutlineLine {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
    pub layer: OutlineLayer,
    pub width: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutlineLayer {
    SilkS,
    Fab,
    CrtYd,
}

impl OutlineLayer {
    pub fn name(&self) -> &str {
        match self {
            Self::SilkS => "F.SilkS",
            Self::Fab => "F.Fab",
            Self::CrtYd => "F.CrtYd",
        }
    }
}

/// Arc for pin-1 marker
#[derive(Debug, Clone)]
pub struct OutlineArc {
    pub x: f64,
    pub y: f64,
    pub mid_x: f64,
    pub mid_y: f64,
    pub end_x: f64,
    pub end_y: f64,
    pub layer: OutlineLayer,
    pub width: f64,
}

// Standard line widths (mm)
const SILK_LINE_WIDTH: f64 = 0.12;
const FAB_LINE_WIDTH: f64 = 0.1;
const CRTYD_LINE_WIDTH: f64 = 0.05;

// Standard margins from body edge (mm)
const SILK_MARGIN: f64 = 0.13;
const FAB_MARGIN: f64 = 0.13;

/// Compute outlines for a DIP package
pub fn compute_dip_outlines(
    pin_count: u32,
    pitch: f64,
    row_spacing: f64,
    courtyard_margin: f64,
) -> (Vec<OutlineLine>, Option<OutlineArc>) {
    let half = (pin_count / 2) as f64;
    let body_h = (half - 1.0) * pitch;

    let mut lines = Vec::new();

    // Silkscreen outline
    let sx = -SILK_MARGIN;
    let sy_top = -SILK_MARGIN;
    let sy_bot = body_h + SILK_MARGIN;
    let ex = row_spacing + SILK_MARGIN;

    lines.push(OutlineLine { x1: sx, y1: sy_top, x2: ex, y2: sy_top, layer: OutlineLayer::SilkS, width: SILK_LINE_WIDTH });
    lines.push(OutlineLine { x1: sx, y1: sy_bot, x2: ex, y2: sy_bot, layer: OutlineLayer::SilkS, width: SILK_LINE_WIDTH });
    lines.push(OutlineLine { x1: sx, y1: sy_top, x2: sx, y2: sy_bot, layer: OutlineLayer::SilkS, width: SILK_LINE_WIDTH });
    lines.push(OutlineLine { x1: ex, y1: sy_top, x2: ex, y2: sy_bot, layer: OutlineLayer::SilkS, width: SILK_LINE_WIDTH });

    // Fab outline
    let fx = -FAB_MARGIN;
    let fy_top = -FAB_MARGIN;
    let fy_bot = body_h + FAB_MARGIN;
    let fex = row_spacing + FAB_MARGIN;

    lines.push(OutlineLine { x1: fx, y1: fy_top, x2: fex, y2: fy_top, layer: OutlineLayer::Fab, width: FAB_LINE_WIDTH });
    lines.push(OutlineLine { x1: fx, y1: fy_bot, x2: fex, y2: fy_bot, layer: OutlineLayer::Fab, width: FAB_LINE_WIDTH });
    lines.push(OutlineLine { x1: fx, y1: fy_top, x2: fx, y2: fy_bot, layer: OutlineLayer::Fab, width: FAB_LINE_WIDTH });
    lines.push(OutlineLine { x1: fex, y1: fy_top, x2: fex, y2: fy_bot, layer: OutlineLayer::Fab, width: FAB_LINE_WIDTH });

    // Courtyard outline
    let cx = fx - courtyard_margin;
    let cy_top = fy_top - courtyard_margin;
    let cy_bot = fy_bot + courtyard_margin;
    let cex = fex + courtyard_margin;

    lines.push(OutlineLine { x1: cx, y1: cy_top, x2: cex, y2: cy_top, layer: OutlineLayer::CrtYd, width: CRTYD_LINE_WIDTH });
    lines.push(OutlineLine { x1: cx, y1: cy_bot, x2: cex, y2: cy_bot, layer: OutlineLayer::CrtYd, width: CRTYD_LINE_WIDTH });
    lines.push(OutlineLine { x1: cx, y1: cy_top, x2: cx, y2: cy_bot, layer: OutlineLayer::CrtYd, width: CRTYD_LINE_WIDTH });
    lines.push(OutlineLine { x1: cex, y1: cy_top, x2: cex, y2: cy_bot, layer: OutlineLayer::CrtYd, width: CRTYD_LINE_WIDTH });

    // Pin-1 arc marker on Fab layer
    let center_x = row_spacing / 2.0;
    let arc = OutlineArc {
        x: center_x,
        y: fy_top,
        mid_x: center_x - 1.0,
        mid_y: fy_top + 1.0,
        end_x: center_x - 1.0,
        end_y: fy_top,
        layer: OutlineLayer::Fab,
        width: FAB_LINE_WIDTH,
    };

    (lines, Some(arc))
}

/// Compute 4 outline lines forming a rectangle centered at origin
pub fn rect_lines(hw: f64, hh: f64, layer: OutlineLayer, width: f64) -> Vec<OutlineLine> {
    vec![
        OutlineLine { x1: -hw, y1: -hh, x2: hw, y2: -hh, layer, width },
        OutlineLine { x1: hw, y1: -hh, x2: hw, y2: hh, layer, width },
        OutlineLine { x1: hw, y1: hh, x2: -hw, y2: hh, layer, width },
        OutlineLine { x1: -hw, y1: hh, x2: -hw, y2: -hh, layer, width },
    ]
}

/// Compute outlines for SMD packages (SOIC/TSSOP/SOP/MSOP/QFP/QFN)
/// Body centered at origin, width × height
pub fn compute_smd_outlines(
    body_w: f64,
    body_h: f64,
    courtyard_margin: f64,
) -> (Vec<OutlineLine>, Option<OutlineArc>) {
    let mut lines = Vec::new();
    let hw = body_w / 2.0;
    let hh = body_h / 2.0;

    lines.extend(rect_lines(hw, hh, OutlineLayer::Fab, FAB_LINE_WIDTH));

    let shw = hw + SILK_MARGIN;
    let shh = hh + SILK_MARGIN;
    lines.extend(rect_lines(shw, shh, OutlineLayer::SilkS, SILK_LINE_WIDTH));

    let chw = shw + courtyard_margin;
    let chh = shh + courtyard_margin;
    lines.extend(rect_lines(chw, chh, OutlineLayer::CrtYd, CRTYD_LINE_WIDTH));

    let arc = OutlineArc {
        x: -hw + 0.5,
        y: -hh,
        mid_x: -hw + 0.25,
        mid_y: -hh + 0.43,
        end_x: -hw,
        end_y: -hh,
        layer: OutlineLayer::Fab,
        width: FAB_LINE_WIDTH,
    };

    (lines, Some(arc))
}
