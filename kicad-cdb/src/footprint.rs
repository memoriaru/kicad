use anyhow::Result;

use crate::ComponentDb;

/// Generate a .kicad_mod footprint for a component based on its package field.
pub fn generate_footprint_for_component(
    comp: &crate::Component,
    db: &ComponentDb,
) -> Result<String> {
    let package_owned;
    let package = match comp.package.as_deref().filter(|s| !s.is_empty()) {
        Some(p) => p,
        None => {
            let cid = comp
                .id
                .ok_or_else(|| anyhow::anyhow!("Component '{}' has no ID", comp.mpn))?;
            let params = db.get_parameters(cid)?;
            package_owned = params
                .iter()
                .find(|p| p.name == "package")
                .and_then(|p| p.value_text.clone())
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow::anyhow!("Component '{}' has no package field", comp.mpn))?;
            &package_owned
        }
    };

    let pkg_type =
        kicad_symgen::model::PackageType::from_package_str(package).ok_or_else(|| {
            anyhow::anyhow!(
                "Cannot parse package '{}'. Examples: SOT-23-6, SOIC-8, TSSOP-20, QFP-48",
                package
            )
        })?;

    let pin_count = kicad_symgen::model::extract_pin_count(package)
        .ok_or_else(|| anyhow::anyhow!("No pin count in package '{}'", package))?;

    // H3: try to look up real dimensions from imported KiCad library metadata
    // before falling back to hardcoded defaults. Query by kicad_footprint lib_id
    // (exact, e.g. "Package_SO:SOIC-8_3.9x4.9mm_P1.27mm") or by package name.
    let lookup_key = comp
        .kicad_footprint
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(package);
    let (pitch, row_spacing, options) =
        match crate::footprint_lib::lookup_footprint_meta(db, lookup_key) {
            Some(meta) => {
                // Use real dimensions from the KiCad library
                let opts = kicad_symgen::model::FootprintOptions {
                    pad_size: meta.pad_size,
                    drill_size: meta.drill_size,
                    courtyard_margin: 0.5,
                };
                (meta.pitch, meta.row_spacing, opts)
            }
            None => default_params_for_package(&pkg_type), // hardcoded fallback
        };

    let spec = kicad_symgen::model::FootprintSpec {
        name: package.to_string(),
        package_type: pkg_type,
        pin_count,
        pitch,
        row_spacing,
        options,
    };

    let result =
        kicad_symgen::footprint::templates::generate_from_spec(&spec).ok_or_else(|| {
            anyhow::anyhow!(
                "No template available for package '{}' ({} pins)",
                package,
                pin_count
            )
        })?;

    let (lines, arc) = if result.is_through_hole {
        kicad_symgen::footprint::outline::compute_dip_outlines(
            pin_count,
            spec.pitch,
            row_spacing.unwrap_or(7.62),
            0.5,
        )
    } else {
        let x_min = result
            .pads
            .iter()
            .map(|p| p.x - p.width / 2.0)
            .fold(f64::INFINITY, f64::min);
        let x_max = result
            .pads
            .iter()
            .map(|p| p.x + p.width / 2.0)
            .fold(f64::NEG_INFINITY, f64::max);
        let y_min = result
            .pads
            .iter()
            .map(|p| p.y - p.height / 2.0)
            .fold(f64::INFINITY, f64::min);
        let y_max = result
            .pads
            .iter()
            .map(|p| p.y + p.height / 2.0)
            .fold(f64::NEG_INFINITY, f64::max);
        let body_w = x_max - x_min;
        let body_h = y_max - y_min;
        kicad_symgen::footprint::outline::compute_smd_outlines(
            body_w,
            body_h,
            spec.options.courtyard_margin,
        )
    };

    let content = kicad_symgen::footprint::sexpr::generate_footprint(
        &result.name,
        &result.description,
        &result.tags,
        result.is_through_hole,
        &result.pads,
        &lines,
        arc.as_ref(),
        kicad_symgen::model::KicadVersion::default(),
    );

    Ok(content)
}

/// Generate a combined .kicad_mod library file with footprints for multiple components.
/// Actually generates individual footprints concatenated (KiCad pretty lib format).
pub fn generate_footprint_lib(
    components: &[crate::Component],
    db: &ComponentDb,
) -> Result<Vec<(String, String)>> {
    let mut results = Vec::new();
    for comp in components {
        match generate_footprint_for_component(comp, db) {
            Ok(content) => {
                let name = comp
                    .package
                    .as_deref()
                    .unwrap_or("unknown")
                    .replace(['.', ' ', '/'], "_");
                results.push((name, content));
            }
            Err(e) => {
                eprintln!("  Skipping {}: {}", comp.mpn, e);
            }
        }
    }
    Ok(results)
}

fn default_params_for_package(
    pkg_type: &kicad_symgen::model::PackageType,
) -> (f64, Option<f64>, kicad_symgen::model::FootprintOptions) {
    use kicad_symgen::model::PackageType::*;
    match pkg_type {
        Dip | Sip | DipSocket => (
            2.54,
            Some(7.62),
            kicad_symgen::model::FootprintOptions::default(),
        ),
        Tssop => (
            0.65,
            Some(6.4),
            kicad_symgen::model::FootprintOptions::default(),
        ),
        Soic | Sop | MsoP => (
            1.27,
            Some(5.4),
            kicad_symgen::model::FootprintOptions::default(),
        ),
        Qfp | Lqfp | Tqfp => (0.5, None, kicad_symgen::model::FootprintOptions::default()),
        Qfn | Dfn => (0.5, None, kicad_symgen::model::FootprintOptions::default()),
        Sot23 | Sot223 | Sot89 | Sot353 | Sot363 => {
            (0.95, None, kicad_symgen::model::FootprintOptions::default())
        }
        Bga => (1.0, None, kicad_symgen::model::FootprintOptions::default()),
        _ => (1.27, None, kicad_symgen::model::FootprintOptions::default()),
    }
}
