use crate::footprint::outline::{OutlineArc, OutlineLine};
use crate::footprint::pad::{Pad, PadShape, PadType};
use crate::model::KicadVersion;
use crate::fmt;

/// Generate a complete .kicad_mod file
pub fn generate_footprint(
    name: &str,
    description: &str,
    tags: &str,
    is_through_hole: bool,
    pads: &[Pad],
    lines: &[OutlineLine],
    arc: Option<&OutlineArc>,
    version: KicadVersion,
) -> String {
    let mut s = String::new();

    s.push_str(&format!(
        "(footprint \"{}\" (version {}) (generator kicad-symgen)\n",
        name,
        version.footprint_version()
    ));
    s.push_str("  (layer \"F.Cu\")\n");
    s.push_str(&format!("  (descr \"{}\")\n", description));
    s.push_str(&format!("  (tags \"{}\")\n", tags));

    if is_through_hole {
        s.push_str("  (attr through_hole)\n");
    } else {
        s.push_str("  (attr smd)\n");
    }

    // Reference text
    let ref_y = pads
        .iter()
        .map(|p| p.y)
        .fold(f64::INFINITY, |a, b| a.min(b))
        - 2.33;
    let center_x = compute_center_x(pads);
    s.push_str(&format!(
        "  (fp_text reference \"U**\" (at {} {}) (layer \"F.SilkS\")\n",
        fmt::fmt_f(center_x),
        fmt::fmt_f(ref_y)
    ));
    s.push_str("    (effects (font (size 1 1) (thickness 0.15))))\n");

    // Value text
    let val_y = pads
        .iter()
        .map(|p| p.y)
        .fold(f64::NEG_INFINITY, |a, b| a.max(b))
        + 2.33;
    s.push_str(&format!(
        "  (fp_text value \"{}\" (at {} {}) (layer \"F.Fab\")\n",
        name,
        fmt::fmt_f(center_x),
        fmt::fmt_f(val_y)
    ));
    s.push_str("    (effects (font (size 1 1) (thickness 0.15))))\n");

    // Outline lines
    for line in lines {
        s.push_str(&format!(
            "  (fp_line (start {} {}) (end {} {}) (layer \"{}\") (width {}))\n",
            fmt::fmt_f(line.x1),
            fmt::fmt_f(line.y1),
            fmt::fmt_f(line.x2),
            fmt::fmt_f(line.y2),
            line.layer.name(),
            fmt::fmt_f(line.width)
        ));
    }

    // Pin-1 arc marker
    if let Some(a) = arc {
        s.push_str(&format!(
            "  (fp_arc (start {} {}) (mid {} {}) (end {} {}) (layer \"{}\") (width {}))\n",
            fmt::fmt_f(a.x),
            fmt::fmt_f(a.y),
            fmt::fmt_f(a.mid_x),
            fmt::fmt_f(a.mid_y),
            fmt::fmt_f(a.end_x),
            fmt::fmt_f(a.end_y),
            a.layer.name(),
            fmt::fmt_f(a.width)
        ));
    }

    // Pads
    for pad in pads {
        let pad_type_str = match pad.pad_type {
            PadType::ThruHole => "thru_hole",
            PadType::Smd => "smd",
        };
        let shape_str = match pad.shape {
            PadShape::Rect => "rect",
            PadShape::Oval => "oval",
            PadShape::RoundRect => "roundrect",
            PadShape::Circle => "circle",
        };

        s.push_str(&format!(
            "  (pad \"{}\" {} {} (at {} {}) (size {} {})",
            pad.number,
            pad_type_str,
            shape_str,
            fmt::fmt_f(pad.x),
            fmt::fmt_f(pad.y),
            fmt::fmt_f(pad.width),
            fmt::fmt_f(pad.height)
        ));

        if let Some(drill) = pad.drill {
            s.push_str(&format!(" (drill {})", fmt::fmt_f(drill)));
        }

        s.push_str(" (layers \"*.Cu\" \"*.Mask\"))\n");
    }

    s.push_str(")\n");
    s
}

fn compute_center_x(pads: &[Pad]) -> f64 {
    if pads.is_empty() {
        return 0.0;
    }
    let min_x = pads.iter().map(|p| p.x).fold(f64::INFINITY, f64::min);
    let max_x = pads.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max);
    (min_x + max_x) / 2.0
}
