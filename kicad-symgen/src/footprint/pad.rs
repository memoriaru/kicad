/// A pad in a footprint
#[derive(Debug, Clone)]
pub struct Pad {
    pub number: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub drill: Option<f64>,
    pub pad_type: PadType,
    pub shape: PadShape,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadType {
    ThruHole,
    Smd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadShape {
    Rect,
    Oval,
    RoundRect,
    Circle,
}

/// Compute pad positions for DIP/SIP packages
pub fn compute_dip_pads(
    pin_count: u32,
    pitch: f64,
    row_spacing: f64,
    pad_size: (f64, f64),
    drill: f64,
) -> Vec<Pad> {
    let half = pin_count / 2;
    let mut pads = Vec::new();

    // Left column: pins 1..N/2, top to bottom
    for i in 0..half {
        pads.push(Pad {
            number: (i + 1).to_string(),
            x: 0.0,
            y: i as f64 * pitch,
            width: pad_size.0,
            height: pad_size.1,
            drill: Some(drill),
            pad_type: PadType::ThruHole,
            shape: if i == 0 { PadShape::Rect } else { PadShape::Oval },
        });
    }

    // Right column: pins N/2+1..N, bottom to top
    for i in 0..half {
        pads.push(Pad {
            number: (half + i + 1).to_string(),
            x: row_spacing,
            y: (half - 1 - i) as f64 * pitch,
            width: pad_size.0,
            height: pad_size.1,
            drill: Some(drill),
            pad_type: PadType::ThruHole,
            shape: PadShape::Oval,
        });
    }

    pads
}

/// Compute pad positions for SOIC/TSSOP packages
pub fn compute_smd_pads(
    pin_count: u32,
    pitch: f64,
    row_spacing: f64,
    pad_size: (f64, f64),
) -> Vec<Pad> {
    let half = pin_count / 2;
    let mut pads = Vec::new();

    // Left column: pins 1..N/2
    for i in 0..half {
        pads.push(Pad {
            number: (i + 1).to_string(),
            x: -row_spacing / 2.0,
            y: ((half - 1) as f64 / 2.0 - i as f64) * pitch,
            width: pad_size.0,
            height: pad_size.1,
            drill: None,
            pad_type: PadType::Smd,
            shape: PadShape::Rect,
        });
    }

    // Right column: pins N/2+1..N (reversed order, bottom to top)
    for i in 0..half {
        pads.push(Pad {
            number: (half + i + 1).to_string(),
            x: row_spacing / 2.0,
            y: ((half - 1) as f64 / 2.0 - i as f64) * pitch,
            width: pad_size.0,
            height: pad_size.1,
            drill: None,
            pad_type: PadType::Smd,
            shape: PadShape::Rect,
        });
    }

    pads
}

/// Compute pad positions for QFP/LQFP/TQFP packages (pads on all 4 sides)
pub fn compute_qfp_pads(
    pin_count: u32,
    pitch: f64,
    body_size: f64,
    pad_size: (f64, f64),
) -> Vec<Pad> {
    let per_side = pin_count / 4;
    let mut pads = Vec::new();
    let half_body = body_size / 2.0;
    let half_span = (per_side - 1) as f64 * pitch / 2.0;

    // Left side: pins 1..per_side, top to bottom
    for i in 0..per_side {
        pads.push(Pad {
            number: (i + 1).to_string(),
            x: -half_body,
            y: half_span - i as f64 * pitch,
            width: pad_size.0,
            height: pad_size.1,
            drill: None,
            pad_type: PadType::Smd,
            shape: if i == 0 { PadShape::Rect } else { PadShape::Rect },
        });
    }

    // Bottom side: pins per_side+1..2*per_side, left to right
    for i in 0..per_side {
        pads.push(Pad {
            number: (per_side + i + 1).to_string(),
            x: -half_span + i as f64 * pitch,
            y: half_body,
            width: pad_size.1,
            height: pad_size.0,
            drill: None,
            pad_type: PadType::Smd,
            shape: PadShape::Rect,
        });
    }

    // Right side: pins 2*per_side+1..3*per_side, bottom to top
    for i in 0..per_side {
        pads.push(Pad {
            number: (2 * per_side + i + 1).to_string(),
            x: half_body,
            y: -half_span + i as f64 * pitch,
            width: pad_size.0,
            height: pad_size.1,
            drill: None,
            pad_type: PadType::Smd,
            shape: PadShape::Rect,
        });
    }

    // Top side: pins 3*per_side+1..4*per_side, right to left
    for i in 0..per_side {
        pads.push(Pad {
            number: (3 * per_side + i + 1).to_string(),
            x: half_span - i as f64 * pitch,
            y: -half_body,
            width: pad_size.1,
            height: pad_size.0,
            drill: None,
            pad_type: PadType::Smd,
            shape: PadShape::Rect,
        });
    }

    pads
}

/// Compute pad positions for QFN/DFN packages (pads on 2 or 4 sides + optional center pad)
pub fn compute_qfn_pads(
    pin_count: u32,
    pitch: f64,
    body_size: f64,
    pad_size: (f64, f64),
    center_pad: Option<(f64, f64)>,
) -> Vec<Pad> {
    let mut pads = Vec::new();

    if pin_count % 4 == 0 {
        // 4-sided QFN
        let per_side = pin_count / 4;
        let half_body = body_size / 2.0;
        let half_span = (per_side - 1) as f64 * pitch / 2.0;

        // Left side: pins 1..per_side, top to bottom
        for i in 0..per_side {
            pads.push(Pad {
                number: (i + 1).to_string(),
                x: -half_body,
                y: half_span - i as f64 * pitch,
                width: pad_size.0,
                height: pad_size.1,
                drill: None,
                pad_type: PadType::Smd,
                shape: PadShape::Rect,
            });
        }

        // Bottom side
        for i in 0..per_side {
            pads.push(Pad {
                number: (per_side + i + 1).to_string(),
                x: -half_span + i as f64 * pitch,
                y: half_body,
                width: pad_size.1,
                height: pad_size.0,
                drill: None,
                pad_type: PadType::Smd,
                shape: PadShape::Rect,
            });
        }

        // Right side
        for i in 0..per_side {
            pads.push(Pad {
                number: (2 * per_side + i + 1).to_string(),
                x: half_body,
                y: -half_span + i as f64 * pitch,
                width: pad_size.0,
                height: pad_size.1,
                drill: None,
                pad_type: PadType::Smd,
                shape: PadShape::Rect,
            });
        }

        // Top side
        for i in 0..per_side {
            pads.push(Pad {
                number: (3 * per_side + i + 1).to_string(),
                x: half_span - i as f64 * pitch,
                y: -half_body,
                width: pad_size.1,
                height: pad_size.0,
                drill: None,
                pad_type: PadType::Smd,
                shape: PadShape::Rect,
            });
        }
    } else if pin_count % 2 == 0 {
        // 2-sided DFN
        let half = pin_count / 2;
        let half_body = body_size / 2.0;
        let half_span = (half - 1) as f64 * pitch / 2.0;

        // Left side
        for i in 0..half {
            pads.push(Pad {
                number: (i + 1).to_string(),
                x: -half_body,
                y: half_span - i as f64 * pitch,
                width: pad_size.0,
                height: pad_size.1,
                drill: None,
                pad_type: PadType::Smd,
                shape: PadShape::Rect,
            });
        }

        // Right side
        for i in 0..half {
            pads.push(Pad {
                number: (half + i + 1).to_string(),
                x: half_body,
                y: -half_span + i as f64 * pitch,
                width: pad_size.0,
                height: pad_size.1,
                drill: None,
                pad_type: PadType::Smd,
                shape: PadShape::Rect,
            });
        }
    }

    // Exposed center/thermal pad
    if let Some((w, h)) = center_pad {
        pads.push(Pad {
            number: "EP".to_string(),
            x: 0.0,
            y: 0.0,
            width: w,
            height: h,
            drill: None,
            pad_type: PadType::Smd,
            shape: PadShape::Rect,
        });
    }

    pads
}

/// Compute pad positions for BGA packages (grid layout)
/// row_col: (rows, cols), letter_rows: use letter+number naming (A1, B2...) vs numeric
pub fn compute_bga_pads(
    rows: u32,
    cols: u32,
    pitch: f64,
    pad_diameter: f64,
) -> Vec<Pad> {
    let mut pads = Vec::new();
    let half_x = (cols - 1) as f64 * pitch / 2.0;
    let half_y = (rows - 1) as f64 * pitch / 2.0;

    let letters: Vec<u8> = (b'A'..=b'Y').collect();
    // Skip 'I', 'O', 'Q', 'S', 'X' per JEDEC BGA standard
    let valid_letters: Vec<u8> = letters
        .into_iter()
        .filter(|&c| !matches!(c, b'I' | b'O' | b'Q' | b'S' | b'X'))
        .collect();

    for r in 0..rows {
        for c in 0..cols {
            let number = if (r as usize) < valid_letters.len() {
                format!("{}{}", valid_letters[r as usize] as char, c + 1)
            } else {
                format!("{}{}", r + 1, c + 1)
            };

            pads.push(Pad {
                number,
                x: -half_x + c as f64 * pitch,
                y: -half_y + r as f64 * pitch,
                width: pad_diameter,
                height: pad_diameter,
                drill: None,
                pad_type: PadType::Smd,
                shape: PadShape::Circle,
            });
        }
    }

    pads
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dip_8_pads() {
        let pads = compute_dip_pads(8, 2.54, 7.62, (1.6, 1.6), 0.8);
        assert_eq!(pads.len(), 8);

        // Pin 1 is rect at top-left
        assert_eq!(pads[0].number, "1");
        assert_eq!(pads[0].shape, PadShape::Rect);
        assert_eq!(pads[0].x, 0.0);
        assert_eq!(pads[0].y, 0.0);

        // Pin 5 is first on right column (bottom-left of right side)
        assert_eq!(pads[4].number, "5");
        assert_eq!(pads[4].x, 7.62);

        // Pin 8 is last on right column (top-right)
        assert_eq!(pads[7].number, "8");
        assert_eq!(pads[7].x, 7.62);
        assert_eq!(pads[7].y, 0.0);
    }

    #[test]
    fn test_soic_8_pads() {
        let pads = compute_smd_pads(8, 1.27, 5.4, (0.6, 2.2));
        assert_eq!(pads.len(), 8);

        // Pin 1 on left side
        assert_eq!(pads[0].number, "1");
        assert_eq!(pads[0].pad_type, PadType::Smd);
        assert!(pads[0].x < 0.0);

        // Pin 5 on right side
        assert_eq!(pads[4].number, "5");
        assert!(pads[4].x > 0.0);
    }
}
