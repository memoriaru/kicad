use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use crate::composition::Composition;
use crate::power_tree::TreeNode;
use crate::topology::load_builtin_template;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewIssue {
    pub severity: String,
    pub category: String,
    pub module_id: Option<String>,
    pub message: String,
    pub suggestion: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesignReviewResult {
    pub passed: bool,
    pub issues: Vec<ReviewIssue>,
    pub checks_run: usize,
}

pub fn review_composition(composition: &Composition, tree: &[TreeNode]) -> DesignReviewResult {
    let mut issues = Vec::new();

    issues.extend(check_decoupling_caps(composition, tree));
    issues.extend(check_ldo_dropout(tree));
    issues.extend(check_net_integrity(composition));
    issues.extend(check_cascade_voltage_chain(tree));

    let checks_run = 4;
    let has_errors = issues.iter().any(|i| i.severity == "error");
    DesignReviewResult {
        passed: !has_errors,
        issues,
        checks_run,
    }
}

/// H5: Render a self-contained HTML design review report.
///
/// Generates a standalone HTML document (inline CSS, no external dependencies)
/// that can be opened directly in a browser. Shows pass/fail status, issue
/// counts by severity, and a color-coded table of all findings.
///
/// Style mirrors `erc_vis::annotate_svg` (format! string concatenation, no
/// template engine dependency). All user-supplied text (message, suggestion,
/// module_id) is XML-escaped to prevent injection.
pub fn render_html_report(result: &DesignReviewResult, title: &str) -> String {
    let error_count = result
        .issues
        .iter()
        .filter(|i| i.severity == "error")
        .count();
    let warn_count = result
        .issues
        .iter()
        .filter(|i| i.severity == "warning")
        .count();
    let info_count = result
        .issues
        .iter()
        .filter(|i| i.severity == "info")
        .count();

    let (status_text, status_class) = if result.passed {
        ("PASSED", "passed")
    } else {
        ("FAILED", "failed")
    };

    // Sort issues: errors first, then warnings, then info.
    let severity_rank = |s: &str| match s {
        "error" => 0,
        "warning" => 1,
        _ => 2,
    };
    let mut sorted_issues = result.issues.clone();
    sorted_issues.sort_by_key(|a| severity_rank(&a.severity));

    let rows: String = sorted_issues.iter().map(|issue| {
        let sev_class = match issue.severity.as_str() {
            "error" => "error",
            "warning" => "warning",
            _ => "info",
        };
        let sev_label = match issue.severity.as_str() {
            "error" => "ERROR",
            "warning" => "WARN",
            _ => "INFO",
        };
        let module = issue.module_id.as_deref().unwrap_or("—");
        let suggestion = issue.suggestion.as_deref().unwrap_or("—");
        format!(
            "      <tr class=\"{sev_class}\">\n        <td><span class=\"badge {sev_class}\">{sev_label}</span></td>\n        <td>{}</td>\n        <td><code>{}</code></td>\n        <td>{}</td>\n        <td>{}</td>\n      </tr>\n",
            escape_html(&issue.category),
            escape_html(module),
            escape_html(&issue.message),
            escape_html(suggestion),
        )
    }).collect();

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Design Review — {title}</title>
<style>
  body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; margin: 0; padding: 20px; background: #fafafa; color: #222; }}
  .header {{ background: #fff; border-radius: 8px; padding: 24px; margin-bottom: 20px; box-shadow: 0 1px 3px rgba(0,0,0,0.1); }}
  h1 {{ margin: 0 0 8px 0; font-size: 1.5em; }}
  .status {{ font-size: 1.3em; font-weight: 700; padding: 4px 12px; border-radius: 4px; }}
  .passed {{ color: #2e7d32; background: #e8f5e9; }}
  .failed {{ color: #c62828; background: #ffebee; }}
  .counts {{ margin-top: 12px; color: #666; font-size: 0.95em; }}
  table {{ width: 100%; border-collapse: collapse; background: #fff; border-radius: 8px; overflow: hidden; box-shadow: 0 1px 3px rgba(0,0,0,0.1); }}
  th {{ background: #455a64; color: #fff; text-align: left; padding: 10px 12px; font-size: 0.85em; text-transform: uppercase; letter-spacing: 0.5px; }}
  td {{ padding: 10px 12px; border-bottom: 1px solid #eee; vertical-align: top; font-size: 0.9em; }}
  tr.error {{ background: #fff5f5; }}
  tr.warning {{ background: #fff8e1; }}
  tr.info {{ background: #e3f2fd; }}
  .badge {{ display: inline-block; padding: 2px 8px; border-radius: 3px; font-size: 0.75em; font-weight: 700; }}
  .badge.error {{ background: #c62828; color: #fff; }}
  .badge.warning {{ background: #ef6c00; color: #fff; }}
  .badge.info {{ background: #1565c0; color: #fff; }}
  code {{ background: #f0f0f0; padding: 1px 4px; border-radius: 3px; font-size: 0.85em; }}
  .footer {{ margin-top: 20px; color: #999; font-size: 0.8em; }}
</style>
</head>
<body>
  <div class="header">
    <h1>Design Review — {title}</h1>
    <span class="status {status_class}">{status_text}</span>
    <div class="counts">
      <strong>{checks}</strong> checks run ·
      <span style="color:#c62828"><strong>{errors}</strong> errors</span> ·
      <span style="color:#ef6c00"><strong>{warnings}</strong> warnings</span> ·
      <span style="color:#1565c0"><strong>{infos}</strong> info</span>
    </div>
  </div>
  <table>
    <thead>
      <tr><th>Severity</th><th>Category</th><th>Module</th><th>Issue</th><th>Suggestion</th></tr>
    </thead>
    <tbody>
{rows}    </tbody>
  </table>
  <div class="footer">Generated by kdesign design review · {issues_total} findings</div>
</body>
</html>
"#,
        title = escape_html(title),
        status_class = status_class,
        status_text = status_text,
        checks = result.checks_run,
        errors = error_count,
        warnings = warn_count,
        infos = info_count,
        rows = rows,
        issues_total = result.issues.len(),
    )
}

/// Escape HTML special characters to prevent injection in user-supplied text.
fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

/// Check that each module's topology template includes input/output capacitors.
fn check_decoupling_caps(composition: &Composition, _tree: &[TreeNode]) -> Vec<ReviewIssue> {
    let mut issues = Vec::new();

    for module in &composition.modules {
        if module.template_type != "topology" {
            continue;
        }

        let template = match load_builtin_template(&module.template) {
            Ok(t) => t,
            Err(_) => continue,
        };

        let roles: HashSet<&str> = template
            .components
            .iter()
            .map(|c| c.role.as_str())
            .collect();

        let has_c_in = roles.contains("c_in");
        let has_c_out = roles.contains("c_out");

        if !has_c_in {
            issues.push(ReviewIssue {
                severity: "warning".to_string(),
                category: "decoupling".to_string(),
                module_id: Some(module.id.clone()),
                message: format!(
                    "{} topology missing input capacitor (c_in)",
                    module.template
                ),
                suggestion: Some("Add input capacitor for input stability".to_string()),
            });
        }

        if !has_c_out {
            issues.push(ReviewIssue {
                severity: "warning".to_string(),
                category: "decoupling".to_string(),
                module_id: Some(module.id.clone()),
                message: format!(
                    "{} topology missing output capacitor (c_out)",
                    module.template
                ),
                suggestion: Some("Add output capacitor for output stability".to_string()),
            });
        }
    }

    issues
}

/// Check LDO dropout voltage — warn if vin - vout < 0.5V.
fn check_ldo_dropout(tree: &[TreeNode]) -> Vec<ReviewIssue> {
    let mut issues = Vec::new();

    for node in tree {
        if node.topology.to_lowercase() == "ldo" {
            let dropout = node.vin - node.vout;
            if dropout < 0.5 {
                issues.push(ReviewIssue {
                    severity: "warning".to_string(),
                    category: "voltage".to_string(),
                    module_id: Some(node.id.clone()),
                    message: format!(
                        "LDO dropout only {:.2}V ({}V → {}V), may be insufficient",
                        dropout, node.vin, node.vout
                    ),
                    suggestion: Some(
                        "Increase input voltage or reduce output voltage for LDO regulation"
                            .to_string(),
                    ),
                });
            } else if dropout > 5.0 {
                issues.push(ReviewIssue {
                    severity: "info".to_string(),
                    category: "voltage".to_string(),
                    module_id: Some(node.id.clone()),
                    message: format!(
                        "LDO high dropout {:.2}V ({}V → {}V), consider switching converter for efficiency",
                        dropout, node.vin, node.vout
                    ),
                    suggestion: Some("High dropout wastes power as heat. Consider buck converter.".to_string()),
                });
            }
        }
    }

    issues
}

/// Verify every module has VIN, VOUT, GND mapped to global nets.
fn check_net_integrity(composition: &Composition) -> Vec<ReviewIssue> {
    let mut issues = Vec::new();

    let global_net_names: HashSet<&str> = composition
        .global_nets
        .iter()
        .map(|n| n.name.as_str())
        .collect();

    for module in &composition.modules {
        for required_net in &["VIN", "VOUT", "GND"] {
            match module.nets.get(*required_net) {
                Some(mapped) => {
                    if !global_net_names.contains(mapped.as_str()) {
                        issues.push(ReviewIssue {
                            severity: "error".to_string(),
                            category: "integrity".to_string(),
                            module_id: Some(module.id.clone()),
                            message: format!(
                                "Module {} maps {} to '{}' which is not a global net",
                                module.id, required_net, mapped
                            ),
                            suggestion: Some(format!(
                                "Add '{}' to global_nets or fix module net mapping",
                                mapped
                            )),
                        });
                    }
                }
                None => {
                    issues.push(ReviewIssue {
                        severity: "error".to_string(),
                        category: "integrity".to_string(),
                        module_id: Some(module.id.clone()),
                        message: format!(
                            "Module {} missing required net mapping for {}",
                            module.id, required_net
                        ),
                        suggestion: Some(format!("Add '{}' net mapping to module", required_net)),
                    });
                }
            }
        }
    }

    issues
}

/// Check cascade chain — vin should be monotonically decreasing and always > vout.
fn check_cascade_voltage_chain(tree: &[TreeNode]) -> Vec<ReviewIssue> {
    let mut issues = Vec::new();

    for node in tree {
        if node.vin <= node.vout {
            issues.push(ReviewIssue {
                severity: "error".to_string(),
                category: "voltage".to_string(),
                module_id: Some(node.id.clone()),
                message: format!(
                    "Module {} has vin({}V) <= vout({}V), step-down impossible",
                    node.id, node.vin, node.vout
                ),
                suggestion: Some(
                    "Check power tree decomposition for voltage ordering errors".to_string(),
                ),
            });
        }

        if (node.vin - node.vout).abs() < 0.01 {
            issues.push(ReviewIssue {
                severity: "warning".to_string(),
                category: "voltage".to_string(),
                module_id: Some(node.id.clone()),
                message: format!(
                    "Module {} has vin ≈ vout ({}V), no conversion needed",
                    node.id, node.vin
                ),
                suggestion: Some("Remove this module or use a direct connection".to_string()),
            });
        }
    }

    issues
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::composition::{ModuleInstance, NetDef, TopologyInputs};
    use crate::power_tree::{decompose_power_tree, PowerTreeRequest, RailSpec};
    use std::collections::HashMap;

    fn make_composition(tree: &[TreeNode], request: &PowerTreeRequest) -> Composition {
        crate::power_tree::build_composition(tree, request)
    }

    #[test]
    fn test_review_single_buck() {
        let request = PowerTreeRequest {
            vin: 12.0,
            outputs: vec![RailSpec {
                vout: 5.0,
                iout: 2.0,
                name: None,
            }],
            isolated: false,
        };
        let tree = decompose_power_tree(&request);
        let comp = make_composition(&tree, &request);
        let result = review_composition(&comp, &tree);

        // buck template has c_in and c_out, net integrity OK
        assert!(result.passed);
        assert_eq!(result.checks_run, 4);
    }

    #[test]
    fn test_review_cascade_buck() {
        let request = PowerTreeRequest {
            vin: 12.0,
            outputs: vec![
                RailSpec {
                    vout: 5.0,
                    iout: 2.0,
                    name: None,
                },
                RailSpec {
                    vout: 3.3,
                    iout: 1.0,
                    name: None,
                },
            ],
            isolated: false,
        };
        let tree = decompose_power_tree(&request);
        let comp = make_composition(&tree, &request);
        let result = review_composition(&comp, &tree);

        assert!(result.passed);
    }

    #[test]
    fn test_review_net_integrity_error() {
        let comp = Composition {
            name: "test".to_string(),
            description: String::new(),
            modules: vec![ModuleInstance {
                id: "buck_test".to_string(),
                template: "buck".to_string(),
                template_type: "topology".to_string(),
                params: HashMap::new(),
                nets: {
                    let mut m = HashMap::new();
                    m.insert("VIN".to_string(), "VIN".to_string());
                    m.insert("VOUT".to_string(), "5V_RAIL".to_string());
                    // Missing GND mapping
                    m
                },
                y_offset: None,
                topology_inputs: Some(TopologyInputs {
                    vin: 12.0,
                    vout: 5.0,
                    iout: 2.0,
                }),
                computed_values: Default::default(),
            }],
            global_nets: vec![
                NetDef {
                    name: "VIN".to_string(),
                    net_type: Some("power".to_string()),
                },
                NetDef {
                    name: "5V_RAIL".to_string(),
                    net_type: Some("power".to_string()),
                },
                // GND missing from global_nets
            ],
        };

        let result = review_composition(&comp, &[]);
        assert!(!result.passed);
        let integrity_issues: Vec<_> = result
            .issues
            .iter()
            .filter(|i| i.category == "integrity")
            .collect();
        assert!(!integrity_issues.is_empty());
    }

    #[test]
    fn test_review_cascade_voltage_error() {
        let tree = vec![TreeNode {
            id: "bad_module".to_string(),
            topology: "buck".to_string(),
            template_name: "buck".to_string(),
            template_type: "topology".to_string(),
            vin: 3.3,
            vout: 5.0, // vin < vout for buck — error
            iout: 1.0,
            input_net: "3V3_RAIL".to_string(),
            output_net: "5V_RAIL".to_string(),
            vin_source: crate::power_tree::VinSource::GlobalInput,
            y_offset: 0.0,
            pipeline_passed: None,
            pipeline_failed: None,
            pipeline_outputs: Default::default(),
        }];

        let result = review_composition(
            &Composition {
                name: "test".to_string(),
                description: String::new(),
                modules: vec![],
                global_nets: vec![],
            },
            &tree,
        );

        assert!(!result.passed);
        let voltage_issues: Vec<_> = result
            .issues
            .iter()
            .filter(|i| i.category == "voltage" && i.severity == "error")
            .collect();
        assert_eq!(voltage_issues.len(), 1);
    }

    #[test]
    fn test_review_ldo_dropout_warning() {
        let tree = vec![TreeNode {
            id: "ldo_3v3".to_string(),
            topology: "ldo".to_string(),
            template_name: "ldo".to_string(),
            template_type: "topology".to_string(),
            vin: 3.4,
            vout: 3.3, // dropout = 0.1V < 0.5V
            iout: 0.5,
            input_net: "3V4_RAIL".to_string(),
            output_net: "3V3_RAIL".to_string(),
            vin_source: crate::power_tree::VinSource::GlobalInput,
            y_offset: 0.0,
            pipeline_passed: None,
            pipeline_failed: None,
            pipeline_outputs: Default::default(),
        }];

        let result = review_composition(
            &Composition {
                name: "test".to_string(),
                description: String::new(),
                modules: vec![],
                global_nets: vec![],
            },
            &tree,
        );

        let dropout_issues: Vec<_> = result
            .issues
            .iter()
            .filter(|i| i.category == "voltage" && i.message.contains("dropout"))
            .collect();
        assert_eq!(dropout_issues.len(), 1);
        assert_eq!(dropout_issues[0].severity, "warning");
    }

    // ----- H5: HTML report rendering -----

    fn make_issue(severity: &str, category: &str, msg: &str) -> ReviewIssue {
        ReviewIssue {
            severity: severity.into(),
            category: category.into(),
            module_id: Some("test_mod".into()),
            message: msg.into(),
            suggestion: Some("fix it".into()),
        }
    }

    #[test]
    fn test_render_html_passed_no_issues() {
        let result = DesignReviewResult {
            passed: true,
            issues: vec![],
            checks_run: 4,
        };
        let html = render_html_report(&result, "Clean Board");
        assert!(html.contains("PASSED"), "should show PASSED status");
        assert!(!html.contains(r#"<tr class="#), "no issue rows");
        assert!(
            html.contains("<strong>0</strong> errors")
                && html.contains("<strong>0</strong> warnings")
        );
    }

    #[test]
    fn test_render_html_failed_with_issues() {
        let result = DesignReviewResult {
            passed: false,
            issues: vec![
                make_issue("error", "voltage", "VIN too low"),
                make_issue("warning", "decoupling", "missing C_IN"),
                make_issue("info", "thermal", "consider heatsink"),
            ],
            checks_run: 4,
        };
        let html = render_html_report(&result, "Test Board");
        assert!(html.contains("FAILED"), "has error → FAILED");
        // Counts are wrapped in <strong> tags, so check the number + label separately
        assert!(html.contains("<strong>1</strong> errors"));
        assert!(html.contains("<strong>1</strong> warnings"));
        assert!(html.contains("<strong>1</strong> info"));
        // All three severity rows present with correct classes
        assert!(html.contains(r#"<tr class="error">"#));
        assert!(html.contains(r#"<tr class="warning">"#));
        assert!(html.contains(r#"<tr class="info">"#));
        // Errors sort first
        let err_pos = html.find(r#"<tr class="error">"#).unwrap();
        let warn_pos = html.find(r#"<tr class="warning">"#).unwrap();
        assert!(err_pos < warn_pos, "errors should sort before warnings");
    }

    #[test]
    fn test_render_html_escapes_special_chars() {
        let result = DesignReviewResult {
            passed: false,
            issues: vec![ReviewIssue {
                severity: "error".into(),
                category: "test".into(),
                module_id: Some("<script>alert(1)</script>".into()),
                message: "a < b & c > d".into(),
                suggestion: Some("use &amp; properly".into()),
            }],
            checks_run: 1,
        };
        let html = render_html_report(&result, "Esc & <test>");
        // No raw script tag survives
        assert!(
            !html.contains("<script>alert"),
            "script tag must be escaped"
        );
        assert!(html.contains("&lt;script&gt;"), "angle brackets escaped");
        assert!(
            html.contains("a &lt; b &amp; c &gt; d"),
            "message chars escaped"
        );
        // Title escaped too
        assert!(html.contains("Esc &amp; &lt;test&gt;"));
    }
}
