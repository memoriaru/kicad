pub mod dip;
pub mod soic;
pub mod sot;

use crate::model::{FootprintSpec, PackageType};
use crate::footprint::pad::Pad;

/// Result of template-based footprint generation
pub struct TemplateResult {
    pub pads: Vec<Pad>,
    pub name: String,
    pub description: String,
    pub tags: String,
    pub is_through_hole: bool,
}

/// Generate footprint from a spec using the appropriate template
pub fn generate_from_spec(spec: &FootprintSpec) -> Option<TemplateResult> {
    match spec.package_type {
        PackageType::Dip | PackageType::Sip | PackageType::DipSocket => {
            dip::generate_dip(spec)
        }
        PackageType::Soic | PackageType::Tssop | PackageType::Sop | PackageType::MsoP => {
            soic::generate_soic(spec)
        }
        PackageType::Sot23 | PackageType::Sot223 | PackageType::Sot89
        | PackageType::Sot353 | PackageType::Sot363 => {
            sot::generate_sot(spec)
        }
        _ => None,
    }
}
