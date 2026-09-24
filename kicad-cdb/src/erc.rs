use anyhow::{Context, Result};
use serde::Serialize;
use std::process::Command;

// ---------------------------------------------------------------------------
// ERC report data structures
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Clone)]
pub struct ErcReport {
    pub timestamp: Option<String>,
    pub includes: Vec<String>,
    pub sheets: Vec<ErcSheet>,
    pub summary: ErcSummary,
    pub ignored_checks: Vec<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct ErcSheet {
    pub path: String,
    pub violations: Vec<ErcViolation>,
}

#[derive(Debug, Serialize, Clone)]
pub struct ErcViolation {
    pub error_type: String,
    pub description: String,
    pub severity: ErcSeverity,
    pub locations: Vec<ErcLocation>,
}

#[derive(Debug, Serialize, Clone)]
pub enum ErcSeverity {
    Error,
    Warning,
}

#[derive(Debug, Serialize, Clone)]
pub struct ErcLocation {
    pub x_mm: f64,
    pub y_mm: f64,
    pub detail: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct ErcSummary {
    pub total: usize,
    pub errors: usize,
    pub warnings: usize,
}

// ---------------------------------------------------------------------------
// ERC report parser
// ---------------------------------------------------------------------------

pub fn parse_erc_report(text: &str) -> Result<ErcReport> {
    let mut timestamp = None;
    let mut includes = Vec::new();
    let mut sheets: Vec<ErcSheet> = Vec::new();
    let mut summary = ErcSummary {
        total: 0,
        errors: 0,
        warnings: 0,
    };
    let mut ignored_checks = Vec::new();

    let mut current_sheet_path: Option<String> = None;
    let mut current_violations: Vec<ErcViolation> = Vec::new();
    let mut current_violation: Option<ErcViolation> = None;

    for line in text.lines() {
        let trimmed = line.trim();

        // Header line: "ERC report (...)"
        if trimmed.starts_with("ERC report (") {
            timestamp = Some(trimmed.to_string());
            continue;
        }

        // Includes line: "Report includes: Errors, Warnings"
        if trimmed.starts_with("Report includes:") {
            let parts: Vec<String> = trimmed["Report includes:".len()..]
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            includes = parts;
            continue;
        }

        // Sheet boundary: "***** Sheet /path/"
        if trimmed.starts_with("***** Sheet ") {
            // Flush current violation into current sheet
            if let Some(v) = current_violation.take() {
                current_violations.push(v);
            }
            // Flush previous sheet
            if let Some(path) = current_sheet_path.take() {
                sheets.push(ErcSheet {
                    path,
                    violations: current_violations,
                });
                current_violations = Vec::new();
            }
            let path = trimmed["***** Sheet ".len()..].trim().to_string();
            current_sheet_path = Some(path);
            continue;
        }

        // Summary line: "** ERC messages: N  Errors E  Warnings W"
        if trimmed.starts_with("** ERC messages:") {
            // Flush current violation and current sheet
            if let Some(v) = current_violation.take() {
                current_violations.push(v);
            }
            if let Some(path) = current_sheet_path.take() {
                sheets.push(ErcSheet {
                    path,
                    violations: current_violations,
                });
                current_violations = Vec::new();
            }

            let rest = &trimmed["** ERC messages:".len()..];
            let nums: Vec<usize> = rest
                .split(|c: char| !c.is_ascii_digit())
                .filter(|s| !s.is_empty())
                .filter_map(|s| s.parse::<usize>().ok())
                .collect();
            if nums.len() >= 3 {
                summary.total = nums[0];
                summary.errors = nums[1];
                summary.warnings = nums[2];
            }
            continue;
        }

        // Ignored check: "    - check name"
        if trimmed.starts_with("- ") && !trimmed.contains("ERC messages") {
            ignored_checks.push(trimmed[2..].trim().to_string());
            continue;
        }

        // Error type line: "[error_type]: description"
        if trimmed.starts_with('[') && trimmed.contains("]: ") {
            // Flush previous violation
            if let Some(v) = current_violation.take() {
                current_violations.push(v);
            }

            let bracket_end = trimmed.find("]: ").context("malformed error type line")?;
            let error_type = trimmed[1..bracket_end].to_string();
            let description = trimmed[bracket_end + 3..].to_string();

            current_violation = Some(ErcViolation {
                error_type,
                description,
                severity: ErcSeverity::Warning, // default, updated below
                locations: Vec::new(),
            });
            continue;
        }

        // Severity line: "    ; error" or "    ; warning"
        if trimmed.starts_with(';') {
            let sev_str = trimmed[1..].trim();
            if let Some(ref mut v) = current_violation {
                v.severity = if sev_str == "error" {
                    ErcSeverity::Error
                } else {
                    ErcSeverity::Warning
                };
            }
            continue;
        }

        // Location line: "@(x mm, y mm): detail"
        if trimmed.starts_with("@(") {
            if let Some(ref mut v) = current_violation {
                if let Some(loc) = parse_location(trimmed) {
                    v.locations.push(loc);
                }
            }
            continue;
        }
    }

    // Flush last violation and sheet
    if let Some(v) = current_violation.take() {
        current_violations.push(v);
    }
    if let Some(path) = current_sheet_path.take() {
        sheets.push(ErcSheet {
            path,
            violations: current_violations,
        });
    }

    Ok(ErcReport {
        timestamp,
        includes,
        sheets,
        summary,
        ignored_checks,
    })
}

fn parse_location(s: &str) -> Option<ErcLocation> {
    // Format: "@(30.48 mm, 204.47 mm): 符号 J2 [Conn_01x08]"
    let s = s.strip_prefix("@(")?;
    let paren_end = s.find(')')?;
    let coords_str = &s[..paren_end];
    let detail = s.get(paren_end + 2..)?.trim_start_matches(": ").to_string();

    let parts: Vec<&str> = coords_str.split(',').collect();
    if parts.len() != 2 {
        return None;
    }

    let x_clean = parts[0].replace(" mm", "");
    let y_clean = parts[1].replace(" mm", "");
    let x_mm = x_clean.trim().parse().ok()?;
    let y_mm = y_clean.trim().parse().ok()?;

    Some(ErcLocation { x_mm, y_mm, detail })
}

// ---------------------------------------------------------------------------
// Run ERC via kicad-cli
// ---------------------------------------------------------------------------

pub fn run_erc(kicad_cli_path: &str, sch_path: &str) -> Result<ErcReport> {
    let tmp_dir = tempfile::tempdir()?;
    let rpt_path = tmp_dir.path().join("erc_report.rpt");

    // P2-7: --severity-all reports checks that the schematic's project file
    // leaves at default/ignored severity (off-grid pins, lib symbol issues,
    // four-way junctions, ...). They surface as warnings; the error count in
    // the gate still reflects only true errors.
    let output = Command::new(kicad_cli_path)
        .args(["sch", "erc", sch_path, "--severity-all", "-o"])
        .arg(&rpt_path)
        .output()
        .with_context(|| format!("Failed to execute '{}'", kicad_cli_path))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("kicad-cli erc failed: {}", stderr);
    }

    let rpt_text =
        std::fs::read_to_string(&rpt_path).with_context(|| "ERC report file not generated")?;

    parse_erc_report(&rpt_text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_erc_report() {
        let report_text = r#"ERC report (2026-05-01T21:21:26, Encoding UTF8)
Report includes: Errors, Warnings

***** Sheet /
[lib_symbol_issues]: 当前配置不包含符号库 'custom'
    ; warning
    @(30.48 mm, 204.47 mm): 符号 J2 [Conn_01x08]
[power_pin_not_driven]: Input Power pin not driven by any Output Power pins
    ; error
    @(31.75 mm, 30.48 mm): Symbol U1 引脚 2 [GND, 电源输入, 图线]

***** Sheet /Power Supply/

 ** ERC messages: 33  Errors 5  Warnings 28

 ** Ignored checks:
    - Global label only appears once in the schematic
"#;
        let report = parse_erc_report(report_text).unwrap();
        assert_eq!(
            report.timestamp,
            Some("ERC report (2026-05-01T21:21:26, Encoding UTF8)".into())
        );
        assert_eq!(report.includes, vec!["Errors", "Warnings"]);
        assert_eq!(report.sheets.len(), 2);

        let root_sheet = &report.sheets[0];
        assert_eq!(root_sheet.path, "/");
        assert_eq!(root_sheet.violations.len(), 2);

        assert_eq!(root_sheet.violations[0].error_type, "lib_symbol_issues");
        assert!(matches!(
            root_sheet.violations[0].severity,
            ErcSeverity::Warning
        ));
        assert_eq!(root_sheet.violations[0].locations.len(), 1);
        assert!((root_sheet.violations[0].locations[0].x_mm - 30.48).abs() < 0.01);

        assert_eq!(root_sheet.violations[1].error_type, "power_pin_not_driven");
        assert!(matches!(
            root_sheet.violations[1].severity,
            ErcSeverity::Error
        ));

        let ps_sheet = &report.sheets[1];
        assert_eq!(ps_sheet.path, "/Power Supply/");
        assert!(ps_sheet.violations.is_empty());

        assert_eq!(report.summary.total, 33);
        assert_eq!(report.summary.errors, 5);
        assert_eq!(report.summary.warnings, 28);
        assert_eq!(report.ignored_checks.len(), 1);
        assert!(report.ignored_checks[0].contains("Global label"));
    }

    #[test]
    fn test_parse_pin_to_pin_violation() {
        let report_text = r#"ERC report (2026-04-30T10:13:21, Encoding UTF8)
Report includes: Errors, Warnings, Exclusions

***** Sheet /
[pin_to_pin]: 类型为 电源输出 和 电源输出 的引脚已连接
    ; error
    @(54.92 mm, 197.38 mm): Symbol #PWR?? 引脚 1 [电源输出, 图线]
    @(47.62 mm, 133.73 mm): Symbol U4 引脚 5 [VOUT, 电源输出, 图线]

 ** ERC messages: 3  Errors 1  Warnings 2
"#;
        let report = parse_erc_report(report_text).unwrap();
        assert_eq!(report.sheets.len(), 1);
        let v = &report.sheets[0].violations[0];
        assert_eq!(v.error_type, "pin_to_pin");
        assert!(matches!(v.severity, ErcSeverity::Error));
        assert_eq!(v.locations.len(), 2);
        assert!((v.locations[0].x_mm - 54.92).abs() < 0.01);
        assert!((v.locations[1].x_mm - 47.62).abs() < 0.01);
        assert_eq!(report.summary.total, 3);
    }
}
