use crate::footprint::pad;
use crate::model::FootprintSpec;
use super::TemplateResult;

pub fn generate_bga(spec: &FootprintSpec) -> Option<TemplateResult> {
    // For BGA, pin_count = rows × cols, or we infer a square grid
    if spec.pin_count < 4 {
        return None;
    }

    let pitch = if spec.pitch > 0.0 { spec.pitch } else { 0.8 };
    let pad_diameter = spec.options.pad_size.unwrap_or((0.4, 0.4)).0;

    // Infer grid: try square first, then find best rectangular fit
    let (rows, cols) = infer_grid(spec.pin_count);

    Some(TemplateResult {
        pads: pad::compute_bga_pads(rows, cols, pitch, pad_diameter),
        name: spec.name.clone(),
        description: format!(
            "BGA-{} ({}x{} grid), {:.2}mm pitch, {:.2}mm pad",
            spec.pin_count, rows, cols, pitch, pad_diameter
        ),
        tags: format!("BGA SMD {:.2}mm {}pin grid", pitch, spec.pin_count),
        is_through_hole: false,
    })
}

fn infer_grid(pin_count: u32) -> (u32, u32) {
    let sqrt = (pin_count as f64).sqrt() as u32;
    if sqrt * sqrt == pin_count {
        (sqrt, sqrt)
    } else {
        // Find closest rectangular fit
        for r in (1..=pin_count).rev() {
            if pin_count % r == 0 {
                let c = pin_count / r;
                return (r, c);
            }
        }
        (1, pin_count)
    }
}
