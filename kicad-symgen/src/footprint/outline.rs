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

/// Compute outlines for a DIP package
pub fn compute_dip_outlines(
    pin_count: u32,
    pitch: f64,
    row_spacing: f64,
    courtyard_margin: f64,
) -> (Vec<OutlineLine>, Option<OutlineArc>) {
    let half = (pin_count / 2) as f64;
    let body_h = (half - 1.0) * pitch;
    let silk_margin = 1.33; // Standard silkscreen margin
    let fab_margin = 1.27;

    let mut lines = Vec::new();

    // Silkscreen outline
    let sx = -silk_margin;
    let sy_top = -silk_margin;
    let sy_bot = body_h + silk_margin;
    let ex = row_spacing + silk_margin;

    lines.push(OutlineLine { x1: sx, y1: sy_top, x2: ex, y2: sy_top, layer: OutlineLayer::SilkS, width: 0.12 });
    lines.push(OutlineLine { x1: sx, y1: sy_bot, x2: ex, y2: sy_bot, layer: OutlineLayer::SilkS, width: 0.12 });
    lines.push(OutlineLine { x1: sx, y1: sy_top, x2: sx, y2: sy_bot, layer: OutlineLayer::SilkS, width: 0.12 });
    lines.push(OutlineLine { x1: ex, y1: sy_top, x2: ex, y2: sy_bot, layer: OutlineLayer::SilkS, width: 0.12 });

    // Fab outline
    let fx = -fab_margin;
    let fy_top = -fab_margin;
    let fy_bot = body_h + fab_margin;
    let fex = row_spacing + fab_margin;

    lines.push(OutlineLine { x1: fx, y1: fy_top, x2: fex, y2: fy_top, layer: OutlineLayer::Fab, width: 0.1 });
    lines.push(OutlineLine { x1: fx, y1: fy_bot, x2: fex, y2: fy_bot, layer: OutlineLayer::Fab, width: 0.1 });
    lines.push(OutlineLine { x1: fx, y1: fy_top, x2: fx, y2: fy_bot, layer: OutlineLayer::Fab, width: 0.1 });
    lines.push(OutlineLine { x1: fex, y1: fy_top, x2: fex, y2: fy_bot, layer: OutlineLayer::Fab, width: 0.1 });

    // Courtyard outline
    let cx = fx - courtyard_margin;
    let cy_top = fy_top - courtyard_margin;
    let cy_bot = fy_bot + courtyard_margin;
    let cex = fex + courtyard_margin;

    lines.push(OutlineLine { x1: cx, y1: cy_top, x2: cex, y2: cy_top, layer: OutlineLayer::CrtYd, width: 0.05 });
    lines.push(OutlineLine { x1: cx, y1: cy_bot, x2: cex, y2: cy_bot, layer: OutlineLayer::CrtYd, width: 0.05 });
    lines.push(OutlineLine { x1: cx, y1: cy_top, x2: cx, y2: cy_bot, layer: OutlineLayer::CrtYd, width: 0.05 });
    lines.push(OutlineLine { x1: cex, y1: cy_top, x2: cex, y2: cy_bot, layer: OutlineLayer::CrtYd, width: 0.05 });

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
        width: 0.1,
    };

    (lines, Some(arc))
}
