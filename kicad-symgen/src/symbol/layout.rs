use crate::model::*;
use crate::fmt;

const PIN_LENGTH: f64 = 2.54;
const PIN_SPACING: f64 = 2.54;

/// Layout result for a symbol
pub struct LayoutResult {
    pub body_width: f64,
    pub body_height: f64,
    pub pins: Vec<LayoutPin>,
}

#[derive(Debug, Clone)]
pub struct LayoutPin {
    pub index: usize,
    pub x: f64,
    pub y: f64,
    pub side: PinSide,
    pub rotation: f64,
}

/// Pin classification for layout
enum PinClass {
    PowerTop,
    PowerBottom,
    Left,
    Right,
}

fn classify_pin(pin: &SymbolPin) -> PinClass {
    if pin.electrical_type.is_power() {
        let name_upper = pin.name.to_uppercase();
        // Ground pins go bottom
        if is_ground_pin(&name_upper) {
            return PinClass::PowerBottom;
        }
        // Power supply pins go top
        if is_supply_pin(&name_upper) {
            return PinClass::PowerTop;
        }
    }

    match pin.electrical_type {
        ElectricalType::Output | ElectricalType::OpenCollector | ElectricalType::OpenEmitter => {
            PinClass::Right
        }
        ElectricalType::Input => PinClass::Left,
        _ => PinClass::Left,
    }
}

fn is_ground_pin(name: &str) -> bool {
    name == "GND"
        || name == "VSS"
        || name.starts_with("PGND")
        || name.starts_with("SGND")
        || name.starts_with("AGND")
        || name.starts_with("DGND")
        || name.starts_with("EP") // exposed pad (thermal)
        || name.starts_with("PAD")
}

fn is_supply_pin(name: &str) -> bool {
    name.starts_with("VCC")
        || name.starts_with("VDD")
        || name.starts_with("VIN")
        || name.starts_with("V+")
        || name.starts_with("AVCC")
        || name.starts_with("DVCC")
        || name.starts_with("VREG")
        || name.starts_with("VBAT")
        || name.starts_with("VCCO")
        || name.starts_with("VREF")
}

/// Compute smart pin layout for an IC symbol
pub fn compute_layout(spec: &SymbolSpec) -> LayoutResult {
    let pins = &spec.pins;
    if pins.is_empty() {
        return LayoutResult {
            body_width: fmt::BODY_HALF_WIDTH * 2.0,
            body_height: PIN_SPACING * 2.0,
            pins: vec![],
        };
    }

    // Classify pins
    let mut top_pins: Vec<usize> = Vec::new();
    let mut bottom_pins: Vec<usize> = Vec::new();
    let mut left_pins: Vec<usize> = Vec::new();
    let mut right_pins: Vec<usize> = Vec::new();

    for (i, pin) in pins.iter().enumerate() {
        match classify_pin(pin) {
            PinClass::PowerTop => top_pins.push(i),
            PinClass::PowerBottom => bottom_pins.push(i),
            PinClass::Left => left_pins.push(i),
            PinClass::Right => right_pins.push(i),
        }
    }

    // If all pins went to one side, do simple left/right split
    if left_pins.is_empty() && right_pins.is_empty() {
        let half = (pins.len() + 1) / 2;
        left_pins = (0..half).collect();
        right_pins = (half..pins.len()).collect();
        top_pins.clear();
        bottom_pins.clear();
    }

    // If only left pins and no right, move half to right
    if !left_pins.is_empty() && right_pins.is_empty() && top_pins.is_empty() && bottom_pins.is_empty() {
        let half = (left_pins.len() + 1) / 2;
        right_pins = left_pins.split_off(half);
    }

    let signal_rows = left_pins.len().max(right_pins.len());
    let has_top = !top_pins.is_empty();
    let has_bottom = !bottom_pins.is_empty();

    // Body height: signal rows + space for power pins
    let body_half_h = if has_top || has_bottom {
        ((signal_rows + 1) as f64) * PIN_SPACING / 2.0
    } else {
        (signal_rows as f64) * PIN_SPACING / 2.0
    };

    let body_h = body_half_h * 2.0;

    let mut layout_pins = Vec::new();

    // Place left pins (top to bottom)
    let y_start_left = body_half_h - PIN_SPACING / 2.0;
    for (rank, &idx) in left_pins.iter().enumerate() {
        let y = y_start_left - rank as f64 * PIN_SPACING;
        layout_pins.push(LayoutPin {
            index: idx,
            x: -(fmt::BODY_HALF_WIDTH + PIN_LENGTH),
            y,
            side: PinSide::Left,
            rotation: PinSide::Left.rotation(),
        });
    }

    // Place right pins (top to bottom)
    let y_start_right = body_half_h - PIN_SPACING / 2.0;
    for (rank, &idx) in right_pins.iter().enumerate() {
        let y = y_start_right - rank as f64 * PIN_SPACING;
        layout_pins.push(LayoutPin {
            index: idx,
            x: fmt::BODY_HALF_WIDTH + PIN_LENGTH,
            y,
            side: PinSide::Right,
            rotation: PinSide::Right.rotation(),
        });
    }

    // Place top power pins (left to right)
    if !top_pins.is_empty() {
        let top_count = top_pins.len() as f64;
        let x_start = -(top_count - 1.0) * PIN_SPACING / 2.0;
        for (rank, &idx) in top_pins.iter().enumerate() {
            layout_pins.push(LayoutPin {
                index: idx,
                x: x_start + rank as f64 * PIN_SPACING,
                y: body_half_h + PIN_LENGTH,
                side: PinSide::Top,
                rotation: PinSide::Top.rotation(),
            });
        }
    }

    // Place bottom ground pins (left to right)
    if !bottom_pins.is_empty() {
        let bot_count = bottom_pins.len() as f64;
        let x_start = -(bot_count - 1.0) * PIN_SPACING / 2.0;
        for (rank, &idx) in bottom_pins.iter().enumerate() {
            layout_pins.push(LayoutPin {
                index: idx,
                x: x_start + rank as f64 * PIN_SPACING,
                y: -(body_half_h + PIN_LENGTH),
                side: PinSide::Bottom,
                rotation: PinSide::Bottom.rotation(),
            });
        }
    }

    LayoutResult {
        body_width: fmt::BODY_HALF_WIDTH * 2.0,
        body_height: body_h,
        pins: layout_pins,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fmt::test_helpers::*;

    #[test]
    fn test_simple_6pin_ic() {
        let spec = SymbolSpec {
            footprint: Some("SOT-23-6".to_string()),
            pins: vec![
                make_pin("1", "LX", ElectricalType::PowerOut),
                make_pin("2", "GND", ElectricalType::PowerIn),
                make_pin("3", "EN", ElectricalType::Input),
                make_pin("4", "FB", ElectricalType::Input),
                make_pin("5", "VCC", ElectricalType::PowerIn),
                make_pin("6", "SW", ElectricalType::PowerOut),
            ],
            ..make_spec("FP6277", vec![])
        };
        let spec = SymbolSpec { pins: vec![
            make_pin("1", "LX", ElectricalType::PowerOut),
            make_pin("2", "GND", ElectricalType::PowerIn),
            make_pin("3", "EN", ElectricalType::Input),
            make_pin("4", "FB", ElectricalType::Input),
            make_pin("5", "VCC", ElectricalType::PowerIn),
            make_pin("6", "SW", ElectricalType::PowerOut),
        ], ..spec };

        let layout = compute_layout(&spec);
        assert_eq!(layout.pins.len(), 6);

        assert_eq!(layout.pins.iter().find(|p| p.index == 1).unwrap().side, PinSide::Bottom);
        assert_eq!(layout.pins.iter().find(|p| p.index == 4).unwrap().side, PinSide::Top);
        assert_eq!(layout.pins.iter().find(|p| p.index == 2).unwrap().side, PinSide::Left);
    }

    #[test]
    fn test_empty_pins() {
        let layout = compute_layout(&make_spec("Test", vec![]));
        assert!(layout.pins.is_empty());
    }

    #[test]
    fn test_2pin_passive() {
        let spec = make_spec("R", vec![
            make_pin("1", "~", ElectricalType::Passive),
            make_pin("2", "~", ElectricalType::Passive),
        ]);
        let layout = compute_layout(&spec);
        assert_eq!(layout.pins.len(), 2);
        let sides: Vec<PinSide> = layout.pins.iter().map(|p| p.side).collect();
        assert!(sides.contains(&PinSide::Left));
        assert!(sides.contains(&PinSide::Right));
    }
}
