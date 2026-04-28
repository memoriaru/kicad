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
