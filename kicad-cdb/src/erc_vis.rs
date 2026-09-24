use crate::erc::{ErcReport, ErcSeverity};

/// Overlay ERC error/warning markers onto an existing SVG.
/// `svg_content` is the complete SVG string from kicad-render.
/// `scale` is the render scale factor (typically 3.0).
/// Returns the SVG with error markers inserted before `</svg>`.
pub fn annotate_svg(svg_content: &str, report: &ErcReport, scale: f64) -> String {
    let mut markers = String::new();

    for sheet in &report.sheets {
        for v in &sheet.violations {
            let (color, label) = match v.severity {
                ErcSeverity::Error => ("#FF0000", "ERROR"),
                ErcSeverity::Warning => ("#FF8C00", "WARN"),
            };

            for loc in &v.locations {
                let sx = loc.x_mm * scale;
                let sy = loc.y_mm * scale;
                let r = 1.5 * scale;

                // Circle marker
                markers.push_str(&format!(
                    "  <circle cx=\"{:.2}\" cy=\"{:.2}\" r=\"{:.2}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{:.2}\" opacity=\"0.8\"/>\n",
                    sx, sy, r, color, 0.3 * scale
                ));

                // Cross inside circle
                let d = r * 0.6;
                markers.push_str(&format!(
                    "  <line x1=\"{:.2}\" y1=\"{:.2}\" x2=\"{:.2}\" y2=\"{:.2}\" stroke=\"{}\" stroke-width=\"{:.2}\" opacity=\"0.8\"/>\n",
                    sx - d, sy - d, sx + d, sy + d, color, 0.2 * scale
                ));
                markers.push_str(&format!(
                    "  <line x1=\"{:.2}\" y1=\"{:.2}\" x2=\"{:.2}\" y2=\"{:.2}\" stroke=\"{}\" stroke-width=\"{:.2}\" opacity=\"0.8\"/>\n",
                    sx + d, sy - d, sx - d, sy + d, color, 0.2 * scale
                ));
            }

            // Label for first location only
            if let Some(loc) = v.locations.first() {
                let sx = loc.x_mm * scale + 2.0 * scale;
                let sy = loc.y_mm * scale - 0.5 * scale;
                let detail =
                    escape_xml(&format!("[{}] {}: {}", label, v.error_type, v.description));
                markers.push_str(&format!(
                    "  <text x=\"{:.2}\" y=\"{:.2}\" font-size=\"{:.1}\" fill=\"{}\" font-family=\"monospace\" opacity=\"0.9\">{}</text>\n",
                    sx, sy, 0.8 * scale, color, detail
                ));
            }
        }
    }

    // Summary badge
    if report.summary.total > 0 {
        let badge_color = if report.summary.errors > 0 {
            "#FF0000"
        } else {
            "#FF8C00"
        };
        markers.push_str(&format!(
            "  <rect x=\"{:.2}\" y=\"{:.2}\" width=\"{:.2}\" height=\"{:.2}\" rx=\"3\" fill=\"{}\" opacity=\"0.85\"/>\n",
            2.0 * scale, 2.0 * scale, 50.0 * scale, 3.0 * scale, badge_color
        ));
        let summary_text = format!(
            "ERC: {} errors, {} warnings",
            report.summary.errors, report.summary.warnings
        );
        markers.push_str(&format!(
            "  <text x=\"{:.2}\" y=\"{:.2}\" font-size=\"{:.1}\" fill=\"white\" font-family=\"monospace\" font-weight=\"bold\">{}</text>\n",
            3.0 * scale, 4.0 * scale, 1.2 * scale, summary_text
        ));
    }

    // Insert before </svg>
    if let Some(pos) = svg_content.rfind("</svg>") {
        let mut result = svg_content[..pos].to_string();
        result.push_str(&markers);
        result.push_str("</svg>");
        result
    } else {
        let mut result = svg_content.to_string();
        result.push_str(&markers);
        result
    }
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::erc::*;

    fn make_test_report() -> ErcReport {
        ErcReport {
            timestamp: Some("test".into()),
            includes: vec![],
            sheets: vec![ErcSheet {
                path: "/".into(),
                violations: vec![ErcViolation {
                    error_type: "power_pin_not_driven".into(),
                    description: "Input Power pin not driven".into(),
                    severity: ErcSeverity::Error,
                    locations: vec![ErcLocation {
                        x_mm: 31.75,
                        y_mm: 30.48,
                        detail: "Symbol U1 pin 2 [GND]".into(),
                    }],
                }],
            }],
            summary: ErcSummary {
                total: 1,
                errors: 1,
                warnings: 0,
            },
            ignored_checks: vec![],
        }
    }

    #[test]
    fn test_annotate_svg_adds_markers() {
        let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 100 100\">\n<rect x=\"0\" y=\"0\" width=\"100\" height=\"100\" fill=\"white\"/>\n</svg>";
        let report = make_test_report();
        let result = annotate_svg(svg, &report, 3.0);

        assert!(result.contains("circle"), "Should contain circle marker");
        assert!(result.contains("line"), "Should contain cross lines");
        assert!(result.contains("text"), "Should contain text labels");
        assert!(
            result.contains("power_pin_not_driven"),
            "Should contain error type"
        );
        assert!(
            result.contains("ERC: 1 errors"),
            "Should contain summary badge"
        );
        assert!(result.contains("#FF0000"), "Should use red for errors");
        assert!(result.ends_with("</svg>"), "Should still end with </svg>");
    }

    #[test]
    fn test_annotate_svg_empty_report() {
        let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 100 100\">\n</svg>";
        let report = ErcReport {
            timestamp: None,
            includes: vec![],
            sheets: vec![],
            summary: ErcSummary {
                total: 0,
                errors: 0,
                warnings: 0,
            },
            ignored_checks: vec![],
        };
        let result = annotate_svg(svg, &report, 3.0);
        assert!(!result.contains("circle"), "No markers for empty report");
    }
}
