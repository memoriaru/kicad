//! Footprint silk/courtyard synthesis (P1-10).
//!
//! The board generator emits bare pads with no assembly graphics:
//! - no pin-1 marker (a SOT-23 mounted 180° is electrically a different circuit —
//!   the battery board's Q1 S/D swap proved this bug class is real),
//! - no polarity marking on electrolytic caps,
//! - no courtyard (KiCad's component-overlap check goes blind).
//!   Synthesizes all three from pad geometry, in footprint-local coordinates.
//!   Idempotent: footprints that already carry silk circles/lines are untouched.

use kicad_json5::ir::board::{Board, FpCircle, FpLine, FpRect, FpText, FpTextType};
use std::collections::BTreeMap;

/// Returns (pin1_dots, polarity_marks, courtyards).
pub fn add_footprint_silk(board: &mut Board) -> (usize, usize, usize) {
    let names: BTreeMap<u32, String> = board.nets.iter().map(|n| (n.id, n.name.clone())).collect();
    let mut dots = 0;
    let mut polarity = 0;
    let mut courtyards = 0;
    for fp in &mut board.footprints {
        if fp.pads.is_empty() {
            continue;
        }
        let has_silk = !fp.fp_circles.is_empty() || !fp.fp_lines.is_empty();
        let has_courtyard = fp.fp_rects.iter().any(|r| r.layer.contains("CrtYd"));
        if has_silk && has_courtyard {
            continue;
        }
        let add_silk = !has_silk;
        let add_courtyard = !has_courtyard;
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        for pad in &fp.pads {
            let (ox, oy) = fp.pad_rotated_offset(pad);
            min_x = min_x.min(ox - pad.size.0 / 2.0);
            max_x = max_x.max(ox + pad.size.0 / 2.0);
            min_y = min_y.min(oy - pad.size.1 / 2.0);
            max_y = max_y.max(oy + pad.size.1 / 2.0);
        }
        // courtyard: pads bbox + 0.5mm
        if add_courtyard {
            fp.fp_rects.push(FpRect {
                start: (min_x - 0.5, min_y - 0.5),
                end: (max_x + 0.5, max_y + 0.5),
                stroke_width: 0.05,
                layer: "F.CrtYd".into(),
                fill: false,
            });
            courtyards += 1;
        }

        let cx_local = (min_x + max_x) / 2.0;
        let cy_local = (min_y + max_y) / 2.0;
        let n_pads = fp.pads.len();

        let is_passive2 = n_pads == 2
            && (fp.lib_id.contains("Resistor_SMD")
                || fp.lib_id.contains("Capacitor_SMD")
                || fp.lib_id.contains("Inductor_SMD")
                || fp.lib_id.contains("Diode_SMD")
                || fp.lib_id.contains("Fuse:")
                || fp.lib_id.contains("LED_SMD"));
        let is_connector = fp.lib_id.contains("Connector_") || fp.lib_id.contains("Conn_");
        let is_polarized_cap = (fp.lib_id.contains("CP_Elec") || fp.lib_id.contains(":CP_"))
            && (fp.value.contains("µF") || fp.value.contains("μF") || fp.value.contains("uF"));

        // electrolytic polarity: stripe over the GND(-) pad, "+" near the other
        if add_silk && is_polarized_cap && n_pads == 2 {
            let mut neg_local: Option<((f64, f64), usize)> = None;
            for (i, pad) in fp.pads.iter().enumerate() {
                let nm = pad
                    .net_name
                    .clone()
                    .or_else(|| pad.net.and_then(|id| names.get(&id).cloned()))
                    .unwrap_or_default();
                if nm.to_uppercase().contains("GND") {
                    neg_local = Some((fp.pad_rotated_offset(pad), i));
                }
            }
            if let Some(((nx, ny), neg_i)) = neg_local {
                let dir = if nx > cx_local { 1.0 } else { -1.0 };
                fp.fp_lines.push(FpLine {
                    start: (nx + dir * 0.7, ny - 1.6),
                    end: (nx + dir * 0.7, ny + 1.6),
                    stroke_width: 0.5,
                    layer: "F.SilkS".into(),
                });
                let pos_local = fp.pad_rotated_offset(&fp.pads[1 - neg_i]);
                let pdir = if pos_local.0 > cx_local { 1.0 } else { -1.0 };
                fp.fp_texts.push(FpText {
                    text: "+".into(),
                    text_type: FpTextType::User,
                    position: (pos_local.0 + pdir * 1.1, pos_local.1 - 1.2, 0.0),
                    layer: "F.SilkS".into(),
                    font_size: (1.0, 1.0),
                });
                polarity += 1;
            }
        }

        // pin-1 dot for ICs / transistors (≥3 pads, not passives/connectors/caps)
        if add_silk && n_pads >= 3 && !is_passive2 && !is_connector && !is_polarized_cap {
            let p1_local = fp.pad_rotated_offset(&fp.pads[0]);
            let (dx, dy) = (p1_local.0 - cx_local, p1_local.1 - cy_local);
            let ln = dx.hypot(dy);
            let (ux, uy) = if ln > 1e-9 {
                (dx / ln, dy / ln)
            } else {
                (-1.0, 0.0)
            };
            fp.fp_circles.push(FpCircle {
                center: (p1_local.0 + ux * 0.95, p1_local.1 + uy * 0.95),
                end: (p1_local.0 + ux * 0.95 + 0.3, p1_local.1 + uy * 0.95 + 0.3),
                stroke_width: 0.15,
                layer: "F.SilkS".into(),
                fill: false,
            });
            dots += 1;
        }
    }
    (dots, polarity, courtyards)
}
