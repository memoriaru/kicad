use crate::footprint::pad;
use crate::model::{FootprintSpec, PackageType};
use super::TemplateResult;

pub fn generate_dip(spec: &FootprintSpec) -> Option<TemplateResult> {
    if spec.pin_count < 4 || spec.pin_count % 2 != 0 {
        return None;
    }

    let pitch = if spec.pitch > 0.0 { spec.pitch } else { 2.54 };
    let row_spacing = spec.row_spacing.unwrap_or(7.62);
    let pad_size = spec.options.pad_size.unwrap_or((1.6, 1.6));
    let drill = spec.options.drill_size.unwrap_or(0.8);

    let pads = pad::compute_dip_pads(spec.pin_count, pitch, row_spacing, pad_size, drill);

    let type_name = match spec.package_type {
        PackageType::Sip => "SIP",
        PackageType::DipSocket => "DIP Socket",
        _ => "DIP",
    };

    Some(TemplateResult {
        pads,
        name: spec.name.clone(),
        description: format!(
            "{}-pin {} package, {:.2}mm pitch, {:.2}mm row spacing",
            spec.pin_count, type_name, pitch, row_spacing
        ),
        tags: format!("DIL {} PDIP {} {:.2}mm {}pin", type_name, type_name, row_spacing, spec.pin_count),
        is_through_hole: true,
    })
}
