use crate::footprint::pad;
use crate::model::{FootprintSpec, PackageType};
use super::TemplateResult;

pub fn generate_soic(spec: &FootprintSpec) -> Option<TemplateResult> {
    if spec.pin_count < 4 || spec.pin_count % 2 != 0 {
        return None;
    }

    let pitch = if spec.pitch > 0.0 { spec.pitch } else { 1.27 };
    let row_spacing = spec.row_spacing.unwrap_or(match spec.package_type {
        PackageType::Tssop => 6.4,
        PackageType::Sop => 8.0,
        PackageType::MsoP => 5.4,
        _ => 5.4, // SOIC default
    });
    let pad_size = spec.options.pad_size.unwrap_or(match spec.package_type {
        PackageType::Tssop => (0.45, 1.8),
        PackageType::Sop | PackageType::MsoP => (0.6, 2.0),
        _ => (0.6, 2.2),
    });

    let pads = pad::compute_smd_pads(spec.pin_count, pitch, row_spacing, pad_size);

    let type_name = match spec.package_type {
        PackageType::Tssop => "TSSOP",
        PackageType::Sop => "SOP",
        PackageType::MsoP => "MSOP",
        _ => "SOIC",
    };

    Some(TemplateResult {
        pads,
        name: spec.name.clone(),
        description: format!(
            "{}-pin {} package, {:.2}mm pitch, {:.2}mm body width",
            spec.pin_count, type_name, pitch, row_spacing
        ),
        tags: format!("{} {} SMD {:.2}mm {}pin", type_name, type_name, pitch, spec.pin_count),
        is_through_hole: false,
    })
}
