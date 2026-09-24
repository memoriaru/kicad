use anyhow::{Context, Result};
use serde::Serialize;
use std::process::Command;

use kicad_json5::ir::board::{Board, PadShape};

// ---------------------------------------------------------------------------
// DRC report data structures
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Clone)]
pub struct DrcReport {
    pub timestamp: Option<String>,
    pub violations: Vec<DrcViolation>,
    pub unconnected_items: Vec<DrcItem>,
    pub summary: DrcSummary,
    pub ignored_checks: Vec<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct DrcViolation {
    pub violation_type: String,
    pub description: String,
    pub severity: DrcSeverity,
    pub items: Vec<DrcItem>,
}

#[derive(Debug, Serialize, Clone)]
pub enum DrcSeverity {
    Error,
    Warning,
    Exclusion,
}

#[derive(Debug, Serialize, Clone)]
pub struct DrcItem {
    pub description: String,
    pub x_mm: Option<f64>,
    pub y_mm: Option<f64>,
}

#[derive(Debug, Serialize, Clone)]
pub struct DrcSummary {
    pub total: usize,
    pub errors: usize,
    pub warnings: usize,
    /// Missing-connection violations (unconnected_items) — a board with 0 shorts
    /// but N unconnected is NOT done.
    pub unconnected: usize,
    /// shorting_items count
    pub shorts: usize,
    /// track_dangling + via_dangling count
    pub dangling: usize,
}

impl DrcSummary {
    /// Completion gate: a board passes only with 0 errors, 0 shorts and 0 unconnected.
    /// Dangling tracks are reported but only warn (cosmetic copper, not a fault).
    pub fn gate(&self) -> (&'static str, Vec<String>) {
        let mut reasons = Vec::new();
        if self.errors > 0 {
            reasons.push(format!("{} errors", self.errors));
        }
        if self.shorts > 0 {
            reasons.push(format!("{} shorts", self.shorts));
        }
        if self.unconnected > 0 {
            reasons.push(format!("{} unconnected", self.unconnected));
        }
        if reasons.is_empty() {
            ("PASS", reasons)
        } else {
            ("FAIL", reasons)
        }
    }
}

impl DrcReport {
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    pub fn to_markdown(&self) -> String {
        let mut md = String::new();
        md.push_str("# DRC Report\n\n");
        md.push_str(&format!("**Summary**: {} errors, {} warnings, {} total | shorts: {}, unconnected: {}, dangling: {}\n\n",
            self.summary.errors, self.summary.warnings, self.summary.total,
            self.summary.shorts, self.summary.unconnected, self.summary.dangling));
        let (verdict, reasons) = self.summary.gate();
        if verdict == "PASS" {
            md.push_str("**GATE: PASS** (0 errors, 0 shorts, 0 unconnected)\n\n");
        } else {
            md.push_str(&format!("**GATE: FAIL** — {}\n\n", reasons.join(", ")));
        }

        let errors: Vec<_> = self
            .violations
            .iter()
            .filter(|v| matches!(v.severity, DrcSeverity::Error))
            .collect();
        let warnings: Vec<_> = self
            .violations
            .iter()
            .filter(|v| matches!(v.severity, DrcSeverity::Warning))
            .collect();

        if !errors.is_empty() {
            md.push_str("## Errors\n\n");
            md.push_str("| Type | Description | Location |\n");
            md.push_str("|------|-------------|----------|\n");
            for v in &errors {
                let loc = v
                    .items
                    .first()
                    .map(|i| {
                        format!(
                            "({:.1}, {:.1})",
                            i.x_mm.unwrap_or(0.0),
                            i.y_mm.unwrap_or(0.0)
                        )
                    })
                    .unwrap_or_else(|| "—".into());
                md.push_str(&format!(
                    "| {} | {} | {} |\n",
                    v.violation_type, v.description, loc
                ));
            }
            md.push('\n');
        }

        if !warnings.is_empty() {
            md.push_str("## Warnings\n\n");
            md.push_str("| Type | Description | Location |\n");
            md.push_str("|------|-------------|----------|\n");
            for v in &warnings {
                let loc = v
                    .items
                    .first()
                    .map(|i| {
                        format!(
                            "({:.1}, {:.1})",
                            i.x_mm.unwrap_or(0.0),
                            i.y_mm.unwrap_or(0.0)
                        )
                    })
                    .unwrap_or_else(|| "—".into());
                md.push_str(&format!(
                    "| {} | {} | {} |\n",
                    v.violation_type, v.description, loc
                ));
            }
            md.push('\n');
        }

        if !self.unconnected_items.is_empty() {
            md.push_str(&format!(
                "## Unconnected Items ({})\n\n",
                self.unconnected_items.len()
            ));
            for item in &self.unconnected_items {
                md.push_str(&format!("- {}\n", item.description));
            }
        }

        md
    }
}

// ---------------------------------------------------------------------------
// DRC report parser (text format, similar to ERC)
// ---------------------------------------------------------------------------

pub fn parse_drc_report(text: &str) -> Result<DrcReport> {
    let mut timestamp = None;
    let mut violations: Vec<DrcViolation> = Vec::new();
    let mut unconnected_items: Vec<DrcItem> = Vec::new();
    let mut summary = DrcSummary {
        total: 0,
        errors: 0,
        warnings: 0,
        unconnected: 0,
        shorts: 0,
        dangling: 0,
    };
    let mut ignored_checks = Vec::new();

    let mut current_violation: Option<DrcViolation> = None;
    let mut in_unconnected = false;

    for line in text.lines() {
        let trimmed = line.trim();

        // Header: "DRC report (...)"
        if trimmed.starts_with("DRC report (") {
            timestamp = Some(trimmed.to_string());
            continue;
        }

        // Summary: "** DRC messages: N  Errors E  Warnings W"
        if trimmed.starts_with("** DRC messages:") || trimmed.starts_with("** Found ") {
            flush_violation(&mut current_violation, &mut violations);

            let rest = if let Some(r) = trimmed.strip_prefix("** DRC messages:") {
                r.trim()
            } else {
                trimmed["** Found ".len()..].trim()
            };
            let nums: Vec<usize> = rest
                .split(|c: char| !c.is_ascii_digit())
                .filter(|s| !s.is_empty())
                .filter_map(|s| s.parse::<usize>().ok())
                .collect();
            if nums.len() >= 3 {
                summary.total = nums[0];
                summary.errors = nums[1];
                summary.warnings = nums[2];
            } else if !nums.is_empty() {
                summary.total = nums[0];
            }
            continue;
        }

        // Ignored check: "    - check name" or "    -check_name"
        if (trimmed.starts_with("- ") || (trimmed.starts_with('-') && !trimmed.starts_with("**")))
            && !trimmed.contains("DRC messages")
        {
            let check_name = if let Some(r) = trimmed.strip_prefix("- ") {
                r.trim()
            } else {
                trimmed[1..].trim()
            };
            if !check_name.is_empty() {
                ignored_checks.push(check_name.to_string());
            }
            continue;
        }

        // Unconnected items section (legacy "Found N unconnected" format).
        // Must not swallow `[unconnected_items]:` violation blocks — those are
        // parsed below as violations (one per missing connection pair).
        if trimmed.contains("unconnected") && trimmed.contains(':') && !trimmed.starts_with('[') {
            flush_violation(&mut current_violation, &mut violations);
            in_unconnected = true;
            continue;
        }

        // Violation type line: "[type]: description" or "(type): description"
        if (trimmed.starts_with('[') || trimmed.starts_with('('))
            && (trimmed.contains("]: ") || trimmed.contains("): "))
        {
            flush_violation(&mut current_violation, &mut violations);
            in_unconnected = false;

            let (vtype, desc) = if trimmed.starts_with('[') {
                let bracket_end = trimmed.find("]: ").context("malformed violation line")?;
                (
                    trimmed[1..bracket_end].to_string(),
                    trimmed[bracket_end + 3..].to_string(),
                )
            } else {
                let bracket_end = trimmed.find("): ").context("malformed violation line")?;
                (
                    trimmed[1..bracket_end].to_string(),
                    trimmed[bracket_end + 3..].to_string(),
                )
            };

            current_violation = Some(DrcViolation {
                violation_type: vtype,
                description: desc,
                severity: DrcSeverity::Warning,
                items: Vec::new(),
            });
            continue;
        }

        // Severity line: "; error" / "; warning" / "; exclusion"
        if let Some(sev_str) = trimmed.strip_prefix(';') {
            let sev_str = sev_str.trim();
            if let Some(ref mut v) = current_violation {
                v.severity = match sev_str {
                    "error" => DrcSeverity::Error,
                    "warning" => DrcSeverity::Warning,
                    _ => DrcSeverity::Exclusion,
                };
            }
            continue;
        }

        // Location/detail line: "@(x mm, y mm): detail" or plain detail
        if trimmed.starts_with("@(") {
            if let Some(ref mut v) = current_violation {
                if let Some(item) = parse_drc_item(trimmed) {
                    v.items.push(item);
                }
            } else if in_unconnected {
                if let Some(item) = parse_drc_item(trimmed) {
                    unconnected_items.push(item);
                }
            }
            continue;
        }

        // Plain detail line (pad/via/track description)
        if !trimmed.is_empty()
            && !trimmed.starts_with("Report includes")
            && !trimmed.starts_with("*****")
            && !trimmed.starts_with("**")
        {
            if let Some(ref mut v) = current_violation {
                v.items.push(DrcItem {
                    description: trimmed.to_string(),
                    x_mm: None,
                    y_mm: None,
                });
            }
        }
    }

    flush_violation(&mut current_violation, &mut violations);

    let summary = DrcSummary {
        total: summary.total,
        errors: summary.errors,
        warnings: summary.warnings,
        unconnected: violations
            .iter()
            .filter(|v| v.violation_type == "unconnected_items")
            .count()
            .max(if unconnected_items.is_empty() {
                0
            } else {
                unconnected_items.len()
            }),
        shorts: violations
            .iter()
            .filter(|v| v.violation_type.contains("shorting"))
            .count(),
        dangling: violations
            .iter()
            .filter(|v| v.violation_type.ends_with("dangling"))
            .count(),
    };

    Ok(DrcReport {
        timestamp,
        violations,
        unconnected_items,
        summary,
        ignored_checks,
    })
}

fn flush_violation(current: &mut Option<DrcViolation>, violations: &mut Vec<DrcViolation>) {
    if let Some(v) = current.take() {
        violations.push(v);
    }
}

fn parse_drc_item(s: &str) -> Option<DrcItem> {
    let s = s.strip_prefix("@(")?;
    let paren_end = s.find(')')?;
    let coords_str = &s[..paren_end];
    let detail = s
        .get(paren_end + 2..)
        .map(|d| d.trim_start_matches(": ").to_string())
        .unwrap_or_default();

    let parts: Vec<&str> = coords_str.split(',').collect();
    if parts.len() != 2 {
        return None;
    }

    let x_mm = parts[0].replace(" mm", "").trim().parse().ok()?;
    let y_mm = parts[1].replace(" mm", "").trim().parse().ok()?;

    Some(DrcItem {
        description: detail,
        x_mm: Some(x_mm),
        y_mm: Some(y_mm),
    })
}

// ---------------------------------------------------------------------------
// Run DRC via kicad-cli
// ---------------------------------------------------------------------------

pub fn run_drc(kicad_cli_path: &str, pcb_path: &str) -> Result<DrcReport> {
    let tmp_dir = tempfile::tempdir()?;
    let rpt_path = tmp_dir.path().join("drc_report.rpt");

    // --severity-all: default severities hide warnings/exclusions (e.g. dangling tracks)
    // --refill-zones: stale zone fills produce wrong unconnected results around GND vias
    let output = Command::new(kicad_cli_path)
        .args([
            "pcb",
            "drc",
            "--severity-all",
            "--refill-zones",
            pcb_path,
            "-o",
        ])
        .arg(&rpt_path)
        .output()
        .with_context(|| format!("Failed to execute '{}'", kicad_cli_path))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("kicad-cli drc failed: {}", stderr);
    }

    let rpt_text =
        std::fs::read_to_string(&rpt_path).with_context(|| "DRC report file not generated")?;

    parse_drc_report(&rpt_text)
}

// ---------------------------------------------------------------------------
// Built-in DRC checks (work on Board IR, no kicad-cli needed)
// ---------------------------------------------------------------------------

use crate::layout_directives::LayoutDirectives;

/// Minimum clearance between pads (mm)
const MIN_PAD_CLEARANCE: f64 = 0.2;
/// Minimum trace width for power nets (mm)
const MIN_POWER_TRACE_WIDTH: f64 = 0.3;
/// Minimum trace width for signal nets (mm)
const MIN_SIGNAL_TRACE_WIDTH: f64 = 0.15;

/// Run built-in DRC checks on a Board IR.
pub fn builtin_drc(board: &Board, directives: Option<&LayoutDirectives>) -> DrcReport {
    let mut violations = Vec::new();
    let mut unconnected = Vec::new();

    // 1. Pad-to-pad clearance check
    check_pad_clearance(board, &mut violations);

    // 2. Trace width validation (net-class aware)
    check_trace_widths(board, directives, &mut violations);

    // 3. Unconnected net detection
    check_unconnected_nets(board, &mut unconnected, &mut violations);

    // 4. Board outline check
    check_board_outline(board, &mut violations);

    // 5. Component overlap detection
    check_component_overlap(board, &mut violations);

    // 6. Post-routing checks (trace-to-trace, trace-to-pad, via clearance)
    check_trace_clearance(board, directives, &mut violations);
    check_trace_pad_clearance(board, &mut violations);
    check_via_clearance(board, &mut violations);

    // 7. Via compliance (net-class aware)
    check_via_compliance(board, directives, &mut violations);

    // 8. Decoupling capacitor proximity check
    check_decoupling_caps(board, &mut violations);

    // 9. Floating pin detection
    check_floating_pins(board, &mut violations);

    // 10. Power integrity check
    check_power_integrity(board, &mut violations);

    // 11. Thermal design review
    check_thermal_design(board, &mut violations);

    let errors = violations
        .iter()
        .filter(|v| matches!(v.severity, DrcSeverity::Error))
        .count();
    let warnings = violations
        .iter()
        .filter(|v| matches!(v.severity, DrcSeverity::Warning))
        .count();
    let summary = DrcSummary {
        total: errors + warnings,
        errors,
        warnings,
        unconnected: unconnected.len(),
        shorts: violations
            .iter()
            .filter(|v| v.violation_type.contains("shorting"))
            .count(),
        dangling: violations
            .iter()
            .filter(|v| v.violation_type.ends_with("dangling"))
            .count(),
    };

    DrcReport {
        timestamp: Some(format!(
            "Built-in DRC ({})",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        )),
        violations,
        unconnected_items: unconnected,
        summary,
        ignored_checks: Vec::new(),
    }
}

fn check_pad_clearance(board: &Board, violations: &mut Vec<DrcViolation>) {
    // P2-3: shape-aware clearance. The old check modeled every pad as a circle of
    // radius max(w,h)/2 — systematically overestimating rotated rect pads and
    // producing the 0.13-0.19mm "edge-adjacent" noise floor vs the 0.2 spec.
    let pads = collect_pad_geoms(board);
    for i in 0..pads.len() {
        for j in (i + 1)..pads.len() {
            let a = &pads[i];
            let b = &pads[j];
            // Skip pads from virtual symbols (power flags, power symbols)
            if a.ref_.starts_with('#') || b.ref_.starts_with('#') {
                continue;
            }
            // Skip same-net pads
            if a.net != 0 && a.net == b.net {
                continue;
            }
            // Skip pads in the same footprint (they have defined spacing)
            if a.ref_ == b.ref_ {
                continue;
            }

            let dist = pad_pair_dist(a, b);
            let req = MIN_PAD_CLEARANCE;
            match clearance_band(dist, req) {
                ClearanceBand::Clear => {}
                ClearanceBand::Marginal => {
                    violations.push(DrcViolation {
                        violation_type: "pad_clearance_marginal".into(),
                        description: format!(
                            "Pad clearance {:.3}mm within tolerance band of {:.3}mm between {}.{} and {}.{} (advisory)",
                            dist, req, a.ref_, a.num, b.ref_, b.num
                        ),
                        severity: DrcSeverity::Warning,
                        items: vec![
                            DrcItem { description: format!("{}.{}", a.ref_, a.num), x_mm: Some(a.x), y_mm: Some(a.y) },
                            DrcItem { description: format!("{}.{}", b.ref_, b.num), x_mm: Some(b.x), y_mm: Some(b.y) },
                        ],
                    });
                }
                ClearanceBand::Violation => {
                    violations.push(DrcViolation {
                        violation_type: "pad_clearance".into(),
                        description: format!(
                            "Pad clearance {:.3}mm < {:.3}mm between {}.{} and {}.{}",
                            dist, req, a.ref_, a.num, b.ref_, b.num
                        ),
                        severity: DrcSeverity::Error,
                        items: vec![
                            DrcItem {
                                description: format!("{}.{}", a.ref_, a.num),
                                x_mm: Some(a.x),
                                y_mm: Some(a.y),
                            },
                            DrcItem {
                                description: format!("{}.{}", b.ref_, b.num),
                                x_mm: Some(b.x),
                                y_mm: Some(b.y),
                            },
                        ],
                    });
                }
            }
        }
    }
}

/// P2-3: rotation-aware pad geometry — circle radius for round pads, corner
/// polygon for everything else (rect/roundrect/oval approximated by their
/// bounding rect; roundrect corners overestimate by ≤ half the corner radius).
struct PadGeom {
    ref_: String,
    num: String,
    x: f64,
    y: f64,
    net: u32,
    /// Some(radius) for circle pads
    circle: Option<f64>,
    /// corner polygon for non-circle pads
    poly: Option<Vec<(f64, f64)>>,
}

fn collect_pad_geoms(board: &Board) -> Vec<PadGeom> {
    let mut pads = Vec::new();
    for fp in &board.footprints {
        let (fx, fy, fr) = fp.position;
        for pad in &fp.pads {
            let net = pad.net.unwrap_or(0);
            let (lx, ly, lrot) = pad.position;
            // KiCad convention: pad absolute rotation = footprint rotation + pad local rotation
            let total = (fr + lrot).to_radians();
            let (sin_t, cos_t) = (total.sin(), total.cos());
            // local→board position (file Y-down convention, verified against kicad-cli)
            let bx = fx + lx * cos_t + ly * sin_t;
            let by = fy - lx * sin_t + ly * cos_t;
            let (w, h) = pad.size;
            let (circle, poly) = if pad.shape == PadShape::Circle {
                (Some(w.max(h) / 2.0), None)
            } else {
                let corners = [
                    (-w / 2.0, -h / 2.0),
                    (w / 2.0, -h / 2.0),
                    (w / 2.0, h / 2.0),
                    (-w / 2.0, h / 2.0),
                ]
                .map(|(cx, cy)| (bx + cx * cos_t + cy * sin_t, by - cx * sin_t + cy * cos_t));
                (None, Some(corners.to_vec()))
            };
            pads.push(PadGeom {
                ref_: fp.reference.clone(),
                num: pad.number.clone(),
                x: bx,
                y: by,
                net,
                circle,
                poly,
            });
        }
    }
    pads
}

/// Exact distance between two pads: circle-circle / circle-polygon / polygon-polygon.
fn pad_pair_dist(a: &PadGeom, b: &PadGeom) -> f64 {
    match (a.circle, b.circle) {
        (Some(ra), Some(rb)) => ((a.x - b.x).hypot(a.y - b.y) - ra - rb).max(0.0),
        (Some(ra), None) => {
            (point_polygon_dist((a.x, a.y), b.poly.as_ref().unwrap()) - ra).max(0.0)
        }
        (None, Some(rb)) => {
            (point_polygon_dist((b.x, b.y), a.poly.as_ref().unwrap()) - rb).max(0.0)
        }
        (None, None) => {
            let pa = a.poly.as_ref().unwrap();
            let pb = b.poly.as_ref().unwrap();
            if point_in_convex_poly((a.x, a.y), pb) || point_in_convex_poly((b.x, b.y), pa) {
                return 0.0;
            }
            let mut m = f64::MAX;
            for k in 0..4 {
                for q in 0..4 {
                    m = m.min(segment_to_segment_dist(
                        pa[k],
                        pa[(k + 1) % 4],
                        pb[q],
                        pb[(q + 1) % 4],
                    ));
                }
            }
            m
        }
    }
}

fn point_polygon_dist(p: (f64, f64), poly: &[(f64, f64)]) -> f64 {
    if point_in_convex_poly(p, poly) {
        return 0.0;
    }
    let mut m = f64::MAX;
    for k in 0..poly.len() {
        m = m.min(point_to_segment_dist(
            p,
            poly[k],
            poly[(k + 1) % poly.len()],
        ));
    }
    m
}

fn point_in_convex_poly(p: (f64, f64), poly: &[(f64, f64)]) -> bool {
    let n = poly.len();
    if n < 3 {
        return false;
    }
    // sign-consistent cross products (works for either winding)
    let mut has_pos = false;
    let mut has_neg = false;
    for k in 0..n {
        let a = poly[k];
        let b = poly[(k + 1) % n];
        let cross = (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0);
        if cross > 1e-12 {
            has_pos = true;
        } else if cross < -1e-12 {
            has_neg = true;
        }
    }
    !(has_pos && has_neg)
}

/// P2-3: violations within the tolerance band of the limit are advisory, not
/// errors — half a fine-grid cell (0.125/0.25mm) of measurement slack avoids
/// wolf-crying on edge-adjacent pairs that KiCad's own DRC accepts.
const CLEARANCE_TOLERANCE_BAND: f64 = 0.07;

enum ClearanceBand {
    Clear,
    Marginal,
    Violation,
}

fn clearance_band(dist: f64, req: f64) -> ClearanceBand {
    if dist < req {
        ClearanceBand::Violation
    } else if dist < req + CLEARANCE_TOLERANCE_BAND {
        ClearanceBand::Marginal
    } else {
        ClearanceBand::Clear
    }
}

fn check_trace_widths(
    board: &Board,
    directives: Option<&LayoutDirectives>,
    violations: &mut Vec<DrcViolation>,
) {
    let power_net_ids: std::collections::HashSet<u32> = board
        .nets
        .iter()
        .filter(|n| is_power_net(&n.name))
        .map(|n| n.id)
        .collect();

    for seg in &board.segments {
        let is_power = power_net_ids.contains(&seg.net);
        let net_name = board
            .nets
            .iter()
            .find(|n| n.id == seg.net)
            .map(|n| n.name.as_str())
            .unwrap_or("?");

        let min_width = if let Some(dir) = directives {
            dir.trace_width_for(net_name)
        } else if is_power {
            MIN_POWER_TRACE_WIDTH
        } else {
            MIN_SIGNAL_TRACE_WIDTH
        };

        if seg.width < min_width {
            violations.push(DrcViolation {
                violation_type: "track_width".into(),
                description: format!(
                    "Track width {:.3}mm < {:.3}mm on net {} ({})",
                    seg.width, min_width, seg.net, net_name
                ),
                severity: if is_power {
                    DrcSeverity::Error
                } else {
                    DrcSeverity::Warning
                },
                items: vec![DrcItem {
                    description: format!("Segment on {} net {}", seg.layer, net_name),
                    x_mm: Some((seg.start.0 + seg.end.0) / 2.0),
                    y_mm: Some((seg.start.1 + seg.end.1) / 2.0),
                }],
            });
        }
    }
}

fn check_unconnected_nets(
    board: &Board,
    unconnected: &mut Vec<DrcItem>,
    violations: &mut Vec<DrcViolation>,
) {
    // Find nets that have pads but no segments connecting them
    let nets_with_pads: std::collections::HashSet<u32> = board
        .footprints
        .iter()
        .flat_map(|fp| fp.pads.iter().filter_map(|p| p.net))
        .filter(|&n| n != 0)
        .collect();

    let nets_with_segments: std::collections::HashSet<u32> =
        board.segments.iter().map(|s| s.net).collect();

    let nets_with_zones: std::collections::HashSet<u32> =
        board.zones.iter().map(|z| z.net).collect();

    for net_id in &nets_with_pads {
        if !nets_with_segments.contains(net_id) && !nets_with_zones.contains(net_id) {
            let net_name = board
                .nets
                .iter()
                .find(|n| n.id == *net_id)
                .map(|n| n.name.clone())
                .unwrap_or_else(|| format!("net_{}", net_id));

            // Count pads on this net
            let pad_count: usize = board
                .footprints
                .iter()
                .flat_map(|fp| fp.pads.iter().filter(|p| p.net == Some(*net_id)))
                .count();

            if pad_count > 1 {
                unconnected.push(DrcItem {
                    description: format!("Net {} ({} pads, no routes)", net_name, pad_count),
                    x_mm: None,
                    y_mm: None,
                });
            }
        }
    }

    if !unconnected.is_empty() {
        // Signal nets without routing → Error; power nets → Warning
        let (signal_nets, power_nets): (Vec<_>, Vec<_>) = unconnected.iter().partition(|item| {
            let name = item.description.split_whitespace().nth(1).unwrap_or("");
            !is_power_net(name)
        });

        if !signal_nets.is_empty() {
            violations.push(DrcViolation {
                violation_type: "unconnected_signal_nets".into(),
                description: format!(
                    "{} signal nets have pads but no routing (ERROR)",
                    signal_nets.len()
                ),
                severity: DrcSeverity::Error,
                items: signal_nets.into_iter().cloned().collect(),
            });
        }
        if !power_nets.is_empty() {
            violations.push(DrcViolation {
                violation_type: "unconnected_power_nets".into(),
                description: format!("{} power nets have pads but no routing", power_nets.len()),
                severity: DrcSeverity::Warning,
                items: power_nets.into_iter().cloned().collect(),
            });
        }
    }
}

fn check_board_outline(board: &Board, violations: &mut Vec<DrcViolation>) {
    let has_outline = board.graphics.iter().any(|g| g.layer == "Edge.Cuts");
    if !has_outline && !board.footprints.is_empty() {
        violations.push(DrcViolation {
            violation_type: "missing_outline".into(),
            description: "No board outline (Edge.Cuts) found".into(),
            severity: DrcSeverity::Error,
            items: Vec::new(),
        });
    }
}

fn check_component_overlap(board: &Board, violations: &mut Vec<DrcViolation>) {
    let bodies = collect_body_rects(board);
    for i in 0..bodies.len() {
        for j in (i + 1)..bodies.len() {
            let (ref1, x1_min, y1_min, x1_max, y1_max) = &bodies[i];
            let (ref2, x2_min, y2_min, x2_max, y2_max) = &bodies[j];
            // Skip virtual symbols (power flags)
            if ref1.starts_with('#') || ref2.starts_with('#') {
                continue;
            }

            if *x1_max > *x2_min && *x2_max > *x1_min && *y1_max > *y2_min && *y2_max > *y1_min {
                let overlap_x = ((*x1_max).min(*x2_max) - (*x1_min).max(*x2_min)).max(0.0);
                let overlap_y = ((*y1_max).min(*y2_max) - (*y1_min).max(*y2_min)).max(0.0);
                if overlap_x > 0.01 && overlap_y > 0.01 {
                    violations.push(DrcViolation {
                        violation_type: "component_overlap".into(),
                        description: format!(
                            "Components {} and {} overlap ({:.1}x{:.1}mm)",
                            ref1, ref2, overlap_x, overlap_y
                        ),
                        severity: DrcSeverity::Error,
                        items: vec![
                            DrcItem {
                                description: ref1.clone(),
                                x_mm: Some((*x1_min + *x1_max) / 2.0),
                                y_mm: Some((*y1_min + *y1_max) / 2.0),
                            },
                            DrcItem {
                                description: ref2.clone(),
                                x_mm: Some((*x2_min + *x2_max) / 2.0),
                                y_mm: Some((*y2_min + *y2_max) / 2.0),
                            },
                        ],
                    });
                }
            }
        }
    }
}

// Helpers

type PadInfo = (String, String, f64, f64, f64, u32);

#[allow(dead_code)] // 公共调试工具面, 暂无内部调用方
fn collect_pad_positions(board: &Board) -> Vec<PadInfo> {
    let mut pads = Vec::new();
    for fp in &board.footprints {
        let (fx, fy, fr) = fp.position;
        let rad = fr.to_radians();
        let cos_r = rad.cos();
        let sin_r = rad.sin();
        for pad in &fp.pads {
            let net = pad.net.unwrap_or(0);
            // Transform pad local position to board position
            // KiCad rotation: CCW on screen == (x,y)→(x·cos+y·sin, −x·sin+y·cos) in
            // the file's Y-down frame (verified against kicad-cli pad positions)
            let (lx, ly, _) = pad.position;
            let bx = fx + lx * cos_r + ly * sin_r;
            let by = fy - lx * sin_r + ly * cos_r;
            let max_size = pad.size.0.max(pad.size.1);
            pads.push((
                fp.reference.clone(),
                pad.number.clone(),
                bx,
                by,
                max_size,
                net,
            ));
        }
    }
    pads
}

type BodyRect = (String, f64, f64, f64, f64);

fn collect_body_rects(board: &Board) -> Vec<BodyRect> {
    board
        .footprints
        .iter()
        .map(|fp| {
            let (fx, fy, _) = fp.position;
            let (w, h) = crate::layout_engine::infer_body_size(&fp.lib_id, fp.pads.len());
            (
                fp.reference.clone(),
                fx - w / 2.0,
                fy - h / 2.0,
                fx + w / 2.0,
                fy + h / 2.0,
            )
        })
        .collect()
}

fn is_power_net(name: &str) -> bool {
    let upper = name.to_uppercase();
    upper.contains("VCC")
        || upper.contains("VIN")
        || upper.contains("VDD")
        || upper.contains("5V")
        || upper.contains("3V3")
        || upper.contains("3.3V")
        || upper.contains("12V")
        || upper.contains("9V")
        || upper.contains("VOUT")
        || upper.contains("PWR")
        || upper.contains("+")
        || upper == "GND"
}

/// Check that each IC has a decoupling capacitor within 10mm on the same power net.
fn check_decoupling_caps(board: &Board, violations: &mut Vec<DrcViolation>) {
    // Collect ICs: footprints with 8+ pads
    let ics: Vec<(&str, f64, f64, Vec<(u32, String)>)> = board
        .footprints
        .iter()
        .filter(|fp| fp.pads.len() >= 8 && !fp.reference.starts_with('#'))
        .map(|fp| {
            let (fx, fy, _) = fp.position;
            let power_nets: Vec<(u32, String)> = fp
                .pads
                .iter()
                .filter_map(|p| p.net)
                .filter_map(|nid| {
                    board
                        .nets
                        .iter()
                        .find(|n| n.id == nid)
                        .filter(|n| is_power_net(&n.name) && n.name.to_uppercase() != "GND")
                        .map(|n| (n.id, n.name.clone()))
                })
                .collect();
            (fp.reference.as_str(), fx, fy, power_nets)
        })
        .collect();

    // Collect capacitors: reference starts with "C" or lib_id contains "C_"
    let caps: Vec<(&str, f64, f64, Vec<u32>)> = board
        .footprints
        .iter()
        .filter(|fp| {
            fp.reference.starts_with('C')
                || fp.lib_id.to_uppercase().contains("CAP")
                || fp.lib_id.contains("C_")
        })
        .map(|fp| {
            let (fx, fy, _) = fp.position;
            let net_ids: Vec<u32> = fp.pads.iter().filter_map(|p| p.net).collect();
            (fp.reference.as_str(), fx, fy, net_ids)
        })
        .collect();

    for (ic_ref, ic_x, ic_y, power_nets) in &ics {
        for (pnet_id, pnet_name) in power_nets {
            let has_nearby_cap = caps.iter().any(|(_, cx, cy, cap_nets)| {
                let dx = ic_x - cx;
                let dy = ic_y - cy;
                let dist = (dx * dx + dy * dy).sqrt();
                dist <= 10.0 && cap_nets.contains(pnet_id)
            });
            if !has_nearby_cap {
                violations.push(DrcViolation {
                    violation_type: "missing_decoupling_cap".into(),
                    description: format!(
                        "IC {} has no decoupling capacitor within 10mm on power net {}",
                        ic_ref, pnet_name
                    ),
                    severity: DrcSeverity::Warning,
                    items: vec![DrcItem {
                        description: format!("{} power pin on {}", ic_ref, pnet_name),
                        x_mm: Some(*ic_x),
                        y_mm: Some(*ic_y),
                    }],
                });
            }
        }
    }
}

/// Check for floating (unconnected) input pins on ICs.
fn check_floating_pins(board: &Board, violations: &mut Vec<DrcViolation>) {
    for fp in &board.footprints {
        if fp.reference.starts_with('#') {
            continue;
        }
        // Only check ICs (8+ pads) and smaller active components (4+ pads)
        if fp.pads.len() < 4 {
            continue;
        }

        for pad in &fp.pads {
            if pad.net.is_some() {
                continue;
            }

            // Skip power supply pins (VCC, VDD, GND, etc.) by name
            let is_power_pin = pad.pin_function.as_ref().is_some_and(|f| {
                let fu = f.to_uppercase();
                fu.contains("VCC")
                    || fu.contains("VDD")
                    || fu.contains("GND")
                    || fu.contains("VIN")
                    || fu.contains("VSS")
            });
            if is_power_pin {
                continue;
            }

            // Skip passive pins (passive components connect any net)
            let is_passive = pad.pin_type.as_ref().is_some_and(|t| t == "passive");
            if is_passive {
                continue;
            }

            let (px, py, _) = pad.position;
            let (fx, fy, _) = fp.position;
            violations.push(DrcViolation {
                violation_type: "floating_pin".into(),
                description: format!(
                    "{}.{} is not connected to any net",
                    fp.reference, pad.number
                ),
                severity: DrcSeverity::Warning,
                items: vec![DrcItem {
                    description: format!("{}.{} ({})", fp.reference, pad.number, fp.lib_id),
                    x_mm: Some(fx + px),
                    y_mm: Some(fy + py),
                }],
            });
        }
    }
}

/// Minimum clearance between traces of different nets (mm)
const MIN_TRACE_CLEARANCE: f64 = 0.15;
/// Minimum clearance between a trace and a pad of a different net (mm)
const MIN_TRACE_PAD_CLEARANCE: f64 = 0.2;
/// Minimum clearance between vias (mm)
const MIN_VIA_CLEARANCE: f64 = 0.2;

/// Check trace-to-trace clearance between different nets on the same layer.
fn check_trace_clearance(
    board: &Board,
    directives: Option<&LayoutDirectives>,
    violations: &mut Vec<DrcViolation>,
) {
    // Build net-class clearance lookup
    let nc_clearance = |net_id: u32| -> f64 {
        if let Some(dir) = directives {
            let name = board
                .nets
                .iter()
                .find(|n| n.id == net_id)
                .map(|n| n.name.as_str())
                .unwrap_or("");
            dir.net_class_for(name)
                .map(|nc| nc.clearance_mm)
                .unwrap_or(MIN_TRACE_CLEARANCE)
        } else {
            MIN_TRACE_CLEARANCE
        }
    };

    // Filter segments on copper layers, grouped by layer
    let mut by_layer: std::collections::HashMap<
        &str,
        Vec<(usize, &kicad_json5::ir::board::Segment)>,
    > = std::collections::HashMap::new();
    for (i, seg) in board.segments.iter().enumerate() {
        if seg.layer.ends_with(".Cu") {
            by_layer.entry(&seg.layer).or_default().push((i, seg));
        }
    }

    for segs in by_layer.values() {
        for i in 0..segs.len() {
            for j in (i + 1)..segs.len() {
                let (idx1, s1) = &segs[i];
                let (idx2, s2) = &segs[j];
                // Skip same-net traces
                if s1.net == s2.net {
                    continue;
                }
                // Skip zero-width
                if s1.width <= 0.0 || s2.width <= 0.0 {
                    continue;
                }

                let clearance = nc_clearance(s1.net).max(nc_clearance(s2.net));
                let min_dist = s1.width / 2.0 + s2.width / 2.0 + clearance;
                let dist = segment_to_segment_dist(s1.start, s1.end, s2.start, s2.end);

                if dist < min_dist {
                    let net1_name = board
                        .nets
                        .iter()
                        .find(|n| n.id == s1.net)
                        .map(|n| n.name.as_str())
                        .unwrap_or("?");
                    let net2_name = board
                        .nets
                        .iter()
                        .find(|n| n.id == s2.net)
                        .map(|n| n.name.as_str())
                        .unwrap_or("?");
                    violations.push(DrcViolation {
                        violation_type: "trace_clearance".into(),
                        description: format!(
                            "Trace clearance {:.3}mm < {:.3}mm between net {} ({}) and net {} ({}) on {}",
                            dist, min_dist, s1.net, net1_name, s2.net, net2_name, s1.layer
                        ),
                        severity: DrcSeverity::Error,
                        items: vec![
                            DrcItem {
                                description: format!("Seg {} on net {} ({})", idx1, s1.net, net1_name),
                                x_mm: Some((s1.start.0 + s1.end.0) / 2.0),
                                y_mm: Some((s1.start.1 + s1.end.1) / 2.0),
                            },
                            DrcItem {
                                description: format!("Seg {} on net {} ({})", idx2, s2.net, net2_name),
                                x_mm: Some((s2.start.0 + s2.end.0) / 2.0),
                                y_mm: Some((s2.start.1 + s2.end.1) / 2.0),
                            },
                        ],
                    });
                }
            }
        }
    }
}

/// Check trace-to-pad clearance between different nets on the same layer.
fn check_trace_pad_clearance(board: &Board, violations: &mut Vec<DrcViolation>) {
    // P2-3: layer-aware + shape-aware. The old check ignored layers entirely
    // (B.Cu trace "violating" an F.Cu SMD pad) and modeled pads as circles.
    let pads = collect_pad_geoms(board);
    for seg in &board.segments {
        if seg.width <= 0.0 {
            continue;
        }
        let seg_layer = seg.layer.as_str();
        let half_w = seg.width / 2.0;

        for pad in &pads {
            if seg.net == pad.net {
                continue;
            }
            if pad.ref_.starts_with('#') {
                continue;
            }

            // Pad layer check: THT pads exist on all copper; SMD pads only on
            // their listed layers.
            let pad_layer_names = pad_layers(board, &pad.ref_, &pad.num);
            let on_seg_layer = pad_layer_names
                .iter()
                .any(|l| l == seg_layer || l == "*.Cu" || l.ends_with(".Cu"));
            if !on_seg_layer {
                continue;
            }

            let edge_dist = match pad.circle {
                Some(r) => point_to_segment_dist((pad.x, pad.y), seg.start, seg.end) - r,
                None => seg_polygon_dist(seg.start, seg.end, pad.poly.as_ref().unwrap()),
            };
            let req = half_w + MIN_TRACE_PAD_CLEARANCE;

            match clearance_band(edge_dist, req) {
                ClearanceBand::Clear => {}
                ClearanceBand::Marginal => {
                    let net_name = board
                        .nets
                        .iter()
                        .find(|n| n.id == seg.net)
                        .map(|n| n.name.as_str())
                        .unwrap_or("?");
                    violations.push(DrcViolation {
                        violation_type: "trace_pad_clearance_marginal".into(),
                        description: format!(
                            "Trace-pad clearance {:.3}mm within tolerance band of {:.3}mm: net {} ({}) near {}.{} on {} (advisory)",
                            edge_dist, req, seg.net, net_name, pad.ref_, pad.num, seg_layer
                        ),
                        severity: DrcSeverity::Warning,
                        items: vec![
                            DrcItem {
                                description: format!("Trace on {} net {}", seg_layer, net_name),
                                x_mm: Some((seg.start.0 + seg.end.0) / 2.0),
                                y_mm: Some((seg.start.1 + seg.end.1) / 2.0),
                            },
                            DrcItem {
                                description: format!("{}.{}", pad.ref_, pad.num),
                                x_mm: Some(pad.x),
                                y_mm: Some(pad.y),
                            },
                        ],
                    });
                }
                ClearanceBand::Violation => {
                    let net_name = board
                        .nets
                        .iter()
                        .find(|n| n.id == seg.net)
                        .map(|n| n.name.as_str())
                        .unwrap_or("?");
                    violations.push(DrcViolation {
                        violation_type: "trace_pad_clearance".into(),
                        description: format!(
                            "Trace-pad clearance {:.3}mm < {:.3}mm: net {} ({}) near {}.{} on {}",
                            edge_dist, req, seg.net, net_name, pad.ref_, pad.num, seg_layer
                        ),
                        severity: DrcSeverity::Error,
                        items: vec![
                            DrcItem {
                                description: format!("Trace on {} net {}", seg_layer, net_name),
                                x_mm: Some((seg.start.0 + seg.end.0) / 2.0),
                                y_mm: Some((seg.start.1 + seg.end.1) / 2.0),
                            },
                            DrcItem {
                                description: format!("{}.{}", pad.ref_, pad.num),
                                x_mm: Some(pad.x),
                                y_mm: Some(pad.y),
                            },
                        ],
                    });
                }
            }
        }
    }
}

/// Check via-to-via and via-to-trace clearance.
/// Copper layers a pad exists on, looked up by refdes+pad number.
fn pad_layers(board: &Board, ref_: &str, num: &str) -> Vec<String> {
    for fp in &board.footprints {
        if fp.reference != ref_ {
            continue;
        }
        for pad in &fp.pads {
            if pad.number == num {
                return pad.layers.clone();
            }
        }
    }
    Vec::new()
}

/// Minimum distance from a segment to a convex polygon (0 if intersecting/inside).
fn seg_polygon_dist(a: (f64, f64), b: (f64, f64), poly: &[(f64, f64)]) -> f64 {
    let mut m = f64::MAX;
    for k in 0..poly.len() {
        m = m.min(segment_to_segment_dist(
            a,
            b,
            poly[k],
            poly[(k + 1) % poly.len()],
        ));
    }
    m
}

fn check_via_clearance(board: &Board, violations: &mut Vec<DrcViolation>) {
    // Via-to-via
    for i in 0..board.vias.len() {
        for j in (i + 1)..board.vias.len() {
            let v1 = &board.vias[i];
            let v2 = &board.vias[j];
            if v1.net == v2.net {
                continue;
            }

            let dx = (v1.at.0 - v2.at.0).abs();
            let dy = (v1.at.1 - v2.at.1).abs();
            let dist = (dx * dx + dy * dy).sqrt();
            let min_dist = v1.size / 2.0 + v2.size / 2.0 + MIN_VIA_CLEARANCE;

            if dist < min_dist {
                let net1 = board
                    .nets
                    .iter()
                    .find(|n| n.id == v1.net)
                    .map(|n| n.name.as_str())
                    .unwrap_or("?");
                let net2 = board
                    .nets
                    .iter()
                    .find(|n| n.id == v2.net)
                    .map(|n| n.name.as_str())
                    .unwrap_or("?");
                violations.push(DrcViolation {
                    violation_type: "via_clearance".into(),
                    description: format!(
                        "Via clearance {:.3}mm < {:.3}mm between net {} ({}) and net {} ({})",
                        dist, min_dist, v1.net, net1, v2.net, net2
                    ),
                    severity: DrcSeverity::Error,
                    items: vec![
                        DrcItem {
                            description: format!("Via net {} ({})", v1.net, net1),
                            x_mm: Some(v1.at.0),
                            y_mm: Some(v1.at.1),
                        },
                        DrcItem {
                            description: format!("Via net {} ({})", v2.net, net2),
                            x_mm: Some(v2.at.0),
                            y_mm: Some(v2.at.1),
                        },
                    ],
                });
            }
        }
    }

    // Via-to-trace (different nets)
    for via in &board.vias {
        for seg in &board.segments {
            if via.net == seg.net {
                continue;
            }

            let dist = point_to_segment_dist((via.at.0, via.at.1), seg.start, seg.end);
            let min_dist = via.size / 2.0 + seg.width / 2.0 + MIN_TRACE_CLEARANCE;

            if dist < min_dist {
                let via_net = board
                    .nets
                    .iter()
                    .find(|n| n.id == via.net)
                    .map(|n| n.name.as_str())
                    .unwrap_or("?");
                let seg_net = board
                    .nets
                    .iter()
                    .find(|n| n.id == seg.net)
                    .map(|n| n.name.as_str())
                    .unwrap_or("?");
                violations.push(DrcViolation {
                    violation_type: "via_trace_clearance".into(),
                    description: format!(
                        "Via-trace clearance {:.3}mm < {:.3}mm: via net {} ({}) vs trace net {} ({})",
                        dist, min_dist, via.net, via_net, seg.net, seg_net
                    ),
                    severity: DrcSeverity::Error,
                    items: vec![
                        DrcItem { description: format!("Via net {} ({})", via.net, via_net), x_mm: Some(via.at.0), y_mm: Some(via.at.1) },
                        DrcItem { description: format!("Trace net {} ({})", seg.net, seg_net), x_mm: Some((seg.start.0 + seg.end.0) / 2.0), y_mm: Some((seg.start.1 + seg.end.1) / 2.0) },
                    ],
                });
            }
        }
    }
}

/// Check via size compliance with net class rules.
fn check_via_compliance(
    board: &Board,
    directives: Option<&LayoutDirectives>,
    violations: &mut Vec<DrcViolation>,
) {
    let Some(dir) = directives else { return };

    for via in &board.vias {
        let net_name = board
            .nets
            .iter()
            .find(|n| n.id == via.net)
            .map(|n| n.name.as_str())
            .unwrap_or("");
        if let Some(nc) = dir.net_class_for(net_name) {
            if via.size < nc.via_size_mm {
                violations.push(DrcViolation {
                    violation_type: "via_size".into(),
                    description: format!(
                        "Via size {:.3}mm < {:.3}mm (net class '{}') on net {} ({})",
                        via.size, nc.via_size_mm, nc.name, via.net, net_name
                    ),
                    severity: DrcSeverity::Warning,
                    items: vec![DrcItem {
                        description: format!("Via on net {} ({})", via.net, net_name),
                        x_mm: Some(via.at.0),
                        y_mm: Some(via.at.1),
                    }],
                });
            }
        }
    }
}

/// Check that IC power pins are actually connected to their power net (via trace or zone).
fn check_power_integrity(board: &Board, violations: &mut Vec<DrcViolation>) {
    for fp in &board.footprints {
        if fp.pads.len() < 8 || fp.reference.starts_with('#') {
            continue;
        }
        let (fx, fy, _) = fp.position;

        for pad in &fp.pads {
            let Some(net_id) = pad.net else {
                continue;
            };
            let Some(net_def) = board.nets.iter().find(|n| n.id == net_id) else {
                continue;
            };
            if !is_power_net(&net_def.name) {
                continue;
            }

            let (px, py, _) = pad.position;
            let bx = fx + px;
            let by = fy + py;

            let has_segment = board.segments.iter().any(|s| {
                s.net == net_id
                    && ((s.start.0 - bx).hypot(s.start.1 - by) < 1.0
                        || (s.end.0 - bx).hypot(s.end.1 - by) < 1.0)
            });
            let has_zone = board.zones.iter().any(|z| z.net == net_id);

            if !has_segment && !has_zone {
                violations.push(DrcViolation {
                    violation_type: "power_pin_not_connected".into(),
                    description: format!(
                        "{}.{} on power net {} has no trace or zone connection",
                        fp.reference, pad.number, net_def.name
                    ),
                    severity: DrcSeverity::Error,
                    items: vec![DrcItem {
                        description: format!("{}.{} ({})", fp.reference, pad.number, net_def.name),
                        x_mm: Some(bx),
                        y_mm: Some(by),
                    }],
                });
            }
        }
    }
}

/// Check that power devices have thermal vias or GND copper zone for heat dissipation.
fn check_thermal_design(board: &Board, violations: &mut Vec<DrcViolation>) {
    for fp in &board.footprints {
        if fp.reference.starts_with('#') {
            continue;
        }
        if !is_power_device(&fp.lib_id, &fp.value) {
            continue;
        }

        let (fx, fy, _) = fp.position;
        let gnd_net_ids: std::collections::HashSet<u32> = board
            .nets
            .iter()
            .filter(|n| n.name.to_uppercase() == "GND")
            .map(|n| n.id)
            .collect();

        let has_thermal_via = board.vias.iter().any(|v| {
            let dist = (v.at.0 - fx).hypot(v.at.1 - fy);
            dist < 5.0 && v.size > 0.4 && gnd_net_ids.contains(&v.net)
        });
        let has_gnd_zone = board.zones.iter().any(|z| {
            gnd_net_ids.contains(&z.net)
                && z.outline
                    .iter()
                    .any(|&(px, py)| (px - fx).hypot(py - fy) < 5.0)
        });

        if !has_thermal_via && !has_gnd_zone {
            violations.push(DrcViolation {
                violation_type: "thermal_management".into(),
                description: format!(
                    "Power device {} ({}) has no thermal vias or GND zone within 5mm",
                    fp.reference, fp.lib_id
                ),
                severity: DrcSeverity::Warning,
                items: vec![DrcItem {
                    description: format!("{} ({})", fp.reference, fp.value),
                    x_mm: Some(fx),
                    y_mm: Some(fy),
                }],
            });
        }
    }
}

/// Identify power devices by lib_id or value.
fn is_power_device(lib_id: &str, value: &str) -> bool {
    let upper_id = lib_id.to_uppercase();
    let upper_val = value.to_uppercase();
    upper_id.contains("REGULATOR")
        || upper_id.contains("LDO")
        || upper_id.contains("MOSFET")
        || upper_id.contains("DIODE")
        || upper_id.contains("DCDC")
        || upper_id.contains("BUCK")
        || upper_id.contains("BOOST")
        || upper_id.contains("SY7208")
        || upper_id.contains("FP6277")
        || upper_id.contains("RT9193")
        || upper_val.contains("REGULATOR")
        || upper_val.contains("LDO")
        || upper_val.contains("DCDC")
        || upper_val.contains("BUCK")
        || upper_val.contains("BOOST")
        || upper_val.contains("MOSFET")
}

/// Minimum distance between two line segments.
pub fn segment_to_segment_dist(
    a0: (f64, f64),
    a1: (f64, f64),
    b0: (f64, f64),
    b1: (f64, f64),
) -> f64 {
    let d1 = point_to_segment_dist(a0, b0, b1);
    let d2 = point_to_segment_dist(a1, b0, b1);
    let d3 = point_to_segment_dist(b0, a0, a1);
    let d4 = point_to_segment_dist(b1, a0, a1);
    d1.min(d2).min(d3).min(d4)
}

/// Minimum distance from point P to line segment AB.
pub fn point_to_segment_dist(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let dx = b.0 - a.0;
    let dy = b.1 - a.1;
    let len_sq = dx * dx + dy * dy;
    if len_sq < 1e-12 {
        let ex = p.0 - a.0;
        let ey = p.1 - a.1;
        return (ex * ex + ey * ey).sqrt();
    }
    let t = ((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len_sq;
    let t = t.clamp(0.0, 1.0);
    let proj_x = a.0 + t * dx;
    let proj_y = a.1 + t * dy;
    let ex = p.0 - proj_x;
    let ey = p.1 - proj_y;
    (ex * ex + ey * ey).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    use kicad_json5::ir::board::{Board, Footprint, Pad, PadShape, PadType, Segment};

    #[test]
    fn test_parse_drc_report() {
        let report_text = r#"DRC report (2026-05-04T10:30:00, Encoding UTF8)
Report includes: Errors, Warnings

(clearance): Clearance violation (0.15 mm < 0.20 mm)
    ; error
    @(10.5 mm, 20.3 mm): Track on F.Cu
    @(10.6 mm, 20.3 mm): Pad 1 of R1

(track_width): Track width (0.15 mm) too small
    ; warning
    @(15.0 mm, 30.0 mm): Track on B.Cu

** DRC messages: 5  Errors 1  Warnings 4

** Ignored checks:
    - courtyard_clearance
"#;
        let report = parse_drc_report(report_text).unwrap();
        assert!(report.timestamp.unwrap().contains("2026-05-04"));
        assert_eq!(report.violations.len(), 2);

        assert_eq!(report.violations[0].violation_type, "clearance");
        assert!(matches!(report.violations[0].severity, DrcSeverity::Error));
        assert_eq!(report.violations[0].items.len(), 2);
        assert!((report.violations[0].items[0].x_mm.unwrap() - 10.5).abs() < 0.01);

        assert_eq!(report.violations[1].violation_type, "track_width");
        assert!(matches!(
            report.violations[1].severity,
            DrcSeverity::Warning
        ));

        assert_eq!(report.summary.total, 5);
        assert_eq!(report.summary.errors, 1);
        assert_eq!(report.summary.warnings, 4);
        assert_eq!(report.ignored_checks.len(), 1);
    }

    fn make_test_board() -> Board {
        let mut board = Board::new();
        board.add_net("VIN");
        board.add_net("5V");
        board.add_net("GND");

        let mut fp1 = Footprint::new("Resistor_SMD:R_0805", "R1", "10k");
        fp1.position = (10.0, 10.0, 0.0);
        fp1.pads.push(Pad {
            number: "1".into(),
            pad_type: PadType::Smd,
            shape: PadShape::Rect,
            position: (0.0, 0.0, 0.0),
            size: (1.0, 0.6),
            layers: vec!["F.Cu".into()],
            drill: None,
            net: Some(1),
            net_name: None,
            pin_function: None,
            pin_type: None,
            roundrect_rratio: None,
            solder_mask_margin: None,
            thermal_bridge_width: None,
            thermal_bridge_angle: None,
            thermal_gap: None,
            clearance: None,
            zone_connect: None,
            remove_unused_layers: None,
            options: None,
            primitives: Vec::new(),
        });
        fp1.pads.push(Pad {
            number: "2".into(),
            pad_type: PadType::Smd,
            shape: PadShape::Rect,
            position: (1.5, 0.0, 0.0),
            size: (1.0, 0.6),
            layers: vec!["F.Cu".into()],
            drill: None,
            net: Some(2),
            net_name: None,
            pin_function: None,
            pin_type: None,
            roundrect_rratio: None,
            solder_mask_margin: None,
            thermal_bridge_width: None,
            thermal_bridge_angle: None,
            thermal_gap: None,
            clearance: None,
            zone_connect: None,
            remove_unused_layers: None,
            options: None,
            primitives: Vec::new(),
        });

        let mut fp2 = Footprint::new("Resistor_SMD:R_0805", "R2", "4.7k");
        fp2.position = (20.0, 10.0, 0.0);
        fp2.pads.push(Pad {
            number: "1".into(),
            pad_type: PadType::Smd,
            shape: PadShape::Rect,
            position: (0.0, 0.0, 0.0),
            size: (1.0, 0.6),
            layers: vec!["F.Cu".into()],
            drill: None,
            net: Some(2),
            net_name: None,
            pin_function: None,
            pin_type: None,
            roundrect_rratio: None,
            solder_mask_margin: None,
            thermal_bridge_width: None,
            thermal_bridge_angle: None,
            thermal_gap: None,
            clearance: None,
            zone_connect: None,
            remove_unused_layers: None,
            options: None,
            primitives: Vec::new(),
        });
        fp2.pads.push(Pad {
            number: "2".into(),
            pad_type: PadType::Smd,
            shape: PadShape::Rect,
            position: (1.5, 0.0, 0.0),
            size: (1.0, 0.6),
            layers: vec!["F.Cu".into()],
            drill: None,
            net: Some(3),
            net_name: None,
            pin_function: None,
            pin_type: None,
            roundrect_rratio: None,
            solder_mask_margin: None,
            thermal_bridge_width: None,
            thermal_bridge_angle: None,
            thermal_gap: None,
            clearance: None,
            zone_connect: None,
            remove_unused_layers: None,
            options: None,
            primitives: Vec::new(),
        });

        board.footprints.push(fp1);
        board.footprints.push(fp2);
        board
    }

    #[test]
    fn test_builtin_drc_clean_board() {
        let mut board = make_test_board();
        // Add board outline
        board.graphics.push(kicad_json5::ir::board::BoardGraphic {
            kind: kicad_json5::ir::board::BoardGraphicKind::Rect {
                start: (0.0, 0.0),
                end: (30.0, 20.0),
            },
            layer: "Edge.Cuts".into(),
            stroke_width: 0.15,
            fill: false,
        });
        // Add proper route on power net
        board.segments.push(Segment {
            start: (10.0, 10.0),
            end: (20.0, 10.0),
            width: 0.5,
            layer: "F.Cu".into(),
            net: 1,
        });

        let report = builtin_drc(&board, None);
        // Should have no pad clearance errors (pads are 10mm apart)
        assert!(
            report.summary.errors == 0
                || report
                    .violations
                    .iter()
                    .all(|v| v.violation_type != "pad_clearance")
        );
    }

    #[test]
    fn test_builtin_drc_thin_trace() {
        let mut board = make_test_board();
        board.segments.push(Segment {
            start: (10.0, 10.0),
            end: (15.0, 10.0),
            width: 0.1,
            layer: "F.Cu".into(),
            net: 1, // VIN — power net, too thin
        });

        let report = builtin_drc(&board, None);
        let thin: Vec<_> = report
            .violations
            .iter()
            .filter(|v| v.violation_type == "track_width")
            .collect();
        assert!(!thin.is_empty(), "Should flag thin power trace");
    }

    #[test]
    fn test_builtin_drc_missing_outline() {
        let board = make_test_board();
        let report = builtin_drc(&board, None);
        assert!(report
            .violations
            .iter()
            .any(|v| v.violation_type == "missing_outline"));
    }

    #[test]
    fn test_builtin_drc_component_overlap() {
        let mut board = Board::new();
        let mut fp1 = Footprint::new("custom:IC1", "U1", "MCU");
        fp1.position = (10.0, 10.0, 0.0);
        let mut fp2 = Footprint::new("custom:IC2", "U2", "Sensor");
        fp2.position = (11.0, 10.0, 0.0); // only 1mm apart, bodies overlap (2mm half-size)

        board.footprints.push(fp1);
        board.footprints.push(fp2);

        let report = builtin_drc(&board, None);
        assert!(report
            .violations
            .iter()
            .any(|v| v.violation_type == "component_overlap"));
    }

    #[test]
    fn test_builtin_drc_unconnected_net() {
        let board = make_test_board();
        // VIN (net 1) has pads on R1 and R2 but no segments, no zone
        let report = builtin_drc(&board, None);
        assert!(report
            .violations
            .iter()
            .any(|v| v.violation_type == "unconnected_signal_nets"
                || v.violation_type == "unconnected_power_nets"));
    }
}
