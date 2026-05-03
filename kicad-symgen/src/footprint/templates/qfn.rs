use crate::footprint::pad;
use crate::model::{FootprintSpec, PackageType};
use super::TemplateResult;

pub fn generate_qfn(spec: &FootprintSpec) -> Option<TemplateResult> {
    // DFN is always 2-sided; QFN with %4==0 is 4-sided, otherwise 2-sided
    let is_dual = match spec.package_type {
        PackageType::Dfn => true,
        _ => spec.pin_count % 4 != 0,
    };

    if spec.pin_count < 4 || spec.pin_count % 2 != 0 {
        return None;
    }
    if !is_dual && spec.pin_count % 4 != 0 {
        return None;
    }

    let pitch = if spec.pitch > 0.0 { spec.pitch } else { 0.5 };
    let pad_size = spec.options.pad_size.unwrap_or(if is_dual {
        (0.3, 0.8)
    } else {
        (0.25, 0.75)
    });

    let pin_count_per_side = if is_dual {
        spec.pin_count / 2
    } else {
        spec.pin_count / 4
    };

    let pin_span = (pin_count_per_side - 1) as f64 * pitch;
    let body_size = spec.row_spacing.unwrap_or(pin_span + pad_size.1 + 1.0);

    let center_pad = if is_dual {
        None
    } else {
        Some((body_size * 0.5, body_size * 0.5))
    };

    let type_name = match spec.package_type {
        PackageType::Dfn => "DFN",
        _ => "QFN",
    };

    Some(TemplateResult {
        pads: pad::compute_qfn_pads(spec.pin_count, pitch, body_size, pad_size, center_pad),
        name: spec.name.clone(),
        description: format!(
            "{}-pin {} package, {:.2}mm pitch, {:.2}x{:.2}mm body{}",
            spec.pin_count,
            type_name,
            pitch,
            body_size,
            if is_dual { body_size * 0.4 } else { body_size },
            if center_pad.is_some() { ", exposed pad" } else { "" }
        ),
        tags: format!(
            "{} {} SMD {:.2}mm {}pin",
            type_name,
            if is_dual { "DFN" } else { "QFN" },
            pitch,
            spec.pin_count
        ),
        is_through_hole: false,
    })
}
