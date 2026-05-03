use crate::footprint::pad;
use crate::model::{FootprintSpec, PackageType};
use super::TemplateResult;

pub fn generate_qfp(spec: &FootprintSpec) -> Option<TemplateResult> {
    if spec.pin_count < 16 || spec.pin_count % 4 != 0 {
        return None;
    }

    let pitch = if spec.pitch > 0.0 { spec.pitch } else { 0.5 };
    let per_side = spec.pin_count / 4;

    // Body size from pin span, with margin for pad overhang
    let pin_span = (per_side - 1) as f64 * pitch;
    let pad_size = spec.options.pad_size.unwrap_or(match spec.package_type {
        PackageType::Tqfp => (0.27, 1.4),
        PackageType::Lqfp => (0.27, 1.4),
        _ => (0.3, 1.5), // QFP default
    });
    let body_size = pin_span + pad_size.1 + 1.0;

    let type_name = match spec.package_type {
        PackageType::Tqfp => "TQFP",
        PackageType::Lqfp => "LQFP",
        _ => "QFP",
    };

    Some(TemplateResult {
        pads: pad::compute_qfp_pads(spec.pin_count, pitch, body_size, pad_size),
        name: spec.name.clone(),
        description: format!(
            "{}-pin {} package, {:.2}mm pitch, {:.2}x{:.2}mm body",
            spec.pin_count, type_name, pitch, body_size, body_size
        ),
        tags: format!(
            "{} QFP SMD {:.2}mm {}pin quad",
            type_name, pitch, spec.pin_count
        ),
        is_through_hole: false,
    })
}
