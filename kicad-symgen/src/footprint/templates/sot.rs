use crate::footprint::pad::{Pad, PadType, PadShape};
use crate::model::{FootprintSpec, PackageType};
use super::TemplateResult;

pub fn generate_sot(spec: &FootprintSpec) -> Option<TemplateResult> {
    match spec.package_type {
        PackageType::Sot23 => generate_sot23(spec),
        PackageType::Sot223 => generate_sot223(spec),
        _ => None,
    }
}

fn generate_sot23(spec: &FootprintSpec) -> Option<TemplateResult> {
    let pad_w = 0.7;
    let pad_h = 0.9;

    let pads = match spec.pin_count {
        3 => vec![
            Pad { number: "1".into(), x: -0.95, y: 0.5, width: pad_w, height: pad_h, drill: None, pad_type: PadType::Smd, shape: PadShape::Rect },
            Pad { number: "2".into(), x: -0.95, y: -0.5, width: pad_w, height: pad_h, drill: None, pad_type: PadType::Smd, shape: PadShape::Rect },
            Pad { number: "3".into(), x: 0.95, y: 0.0, width: pad_w, height: pad_h, drill: None, pad_type: PadType::Smd, shape: PadShape::Rect },
        ],
        5 => vec![
            Pad { number: "1".into(), x: -0.95, y: 0.5, width: pad_w, height: pad_h, drill: None, pad_type: PadType::Smd, shape: PadShape::Rect },
            Pad { number: "2".into(), x: -0.95, y: 0.0, width: pad_w, height: pad_h, drill: None, pad_type: PadType::Smd, shape: PadShape::Rect },
            Pad { number: "3".into(), x: -0.95, y: -0.5, width: pad_w, height: pad_h, drill: None, pad_type: PadType::Smd, shape: PadShape::Rect },
            Pad { number: "4".into(), x: 0.95, y: -0.5, width: pad_w, height: pad_h, drill: None, pad_type: PadType::Smd, shape: PadShape::Rect },
            Pad { number: "5".into(), x: 0.95, y: 0.5, width: pad_w, height: pad_h, drill: None, pad_type: PadType::Smd, shape: PadShape::Rect },
        ],
        6 => vec![
            Pad { number: "1".into(), x: -0.95, y: 0.5, width: pad_w, height: pad_h, drill: None, pad_type: PadType::Smd, shape: PadShape::Rect },
            Pad { number: "2".into(), x: -0.95, y: 0.0, width: pad_w, height: pad_h, drill: None, pad_type: PadType::Smd, shape: PadShape::Rect },
            Pad { number: "3".into(), x: -0.95, y: -0.5, width: pad_w, height: pad_h, drill: None, pad_type: PadType::Smd, shape: PadShape::Rect },
            Pad { number: "4".into(), x: 0.95, y: -0.5, width: pad_w, height: pad_h, drill: None, pad_type: PadType::Smd, shape: PadShape::Rect },
            Pad { number: "5".into(), x: 0.95, y: 0.0, width: pad_w, height: pad_h, drill: None, pad_type: PadType::Smd, shape: PadShape::Rect },
            Pad { number: "6".into(), x: 0.95, y: 0.5, width: pad_w, height: pad_h, drill: None, pad_type: PadType::Smd, shape: PadShape::Rect },
        ],
        _ => return None,
    };

    Some(TemplateResult {
        pads,
        name: spec.name.clone(),
        description: format!("SOT-23-{} SMD package", spec.pin_count),
        tags: format!("SOT-23 SMD {}pin", spec.pin_count),
        is_through_hole: false,
    })
}

fn generate_sot223(spec: &FootprintSpec) -> Option<TemplateResult> {
    let pads = vec![
        Pad { number: "1".into(), x: -1.5, y: 0.95, width: 0.9, height: 1.2, drill: None, pad_type: PadType::Smd, shape: PadShape::Rect },
        Pad { number: "2".into(), x: -1.5, y: -0.95, width: 0.9, height: 1.2, drill: None, pad_type: PadType::Smd, shape: PadShape::Rect },
        Pad { number: "3".into(), x: 1.5, y: 0.0, width: 0.9, height: 1.2, drill: None, pad_type: PadType::Smd, shape: PadShape::Rect },
        Pad { number: "4".into(), x: 0.0, y: -1.5, width: 2.4, height: 1.2, drill: None, pad_type: PadType::Smd, shape: PadShape::Rect },
    ];

    Some(TemplateResult {
        pads,
        name: spec.name.clone(),
        description: "SOT-223 SMD package".to_string(),
        tags: "SOT-223 SMD 4pin".to_string(),
        is_through_hole: false,
    })
}
