//! P1-5: Hierarchical schematic support — multi-file loading + interface validation.
//!
//! KiCad hierarchical designs span multiple `.kicad_sch` files: a root sheet
//! references sub-sheets via `(sheet (property "Sheetfile" "child.kicad_sch"))`.
//! This module loads the full hierarchy into a `HierarchicalProject` and
//! validates that every `SheetPin` on a parent has a matching
//! `hierarchical_label` in the child sheet.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::ir::{Label, Schematic, Sheet};
use crate::parse_schematic;
use crate::InputFormat;

// ─── Project container ─────────────────────────────────────────────

/// A fully-loaded hierarchical KiCad project: root sheet + all recursively
/// referenced sub-sheets, keyed by filename.
#[derive(Debug)]
pub struct HierarchicalProject {
    /// The root schematic (the file passed to `load_from_file`).
    pub root: Schematic,
    /// All sub-sheets keyed by their `Sheetfile` property value.
    /// Shared sub-sheets (referenced by multiple parents) appear once.
    pub sub_sheets: HashMap<String, Schematic>,
    /// Directory of the root file, for resolving relative sheet paths.
    pub root_dir: PathBuf,
}

impl HierarchicalProject {
    /// Load a hierarchical project from a root `.kicad_sch` file.
    ///
    /// Recursively follows every `Sheet.sheet_file.value` reference, parsing
    /// each sub-sheet file relative to the root directory. Cyclic references
    /// (A→B→A) and shared sub-sheets are handled by deduplicating on filename.
    pub fn load_from_file(root_path: &Path) -> crate::Result<Self> {
        let root_dir = root_path.parent().unwrap_or_else(|| Path::new(".")).to_path_buf();
        let source = std::fs::read_to_string(root_path)
            .map_err(|e| crate::Error::InvalidSExpr(format!("Failed to read root schematic {}: {}", root_path.display(), e)))?;
        let root = parse_schematic(&source, InputFormat::Sexpr)?;

        let mut sub_sheets: HashMap<String, Schematic> = HashMap::new();
        let mut visited: HashSet<String> = HashSet::new();
        // Seed visited with the root's own filename so a sub-sheet cannot
        // re-import the root (cyclic reference A→B→A).
        if let Some(root_name) = root_path.file_name().and_then(|n| n.to_str()) {
            visited.insert(root_name.to_string());
        }
        load_sub_sheets_recursive(&root, &root_dir, &mut sub_sheets, &mut visited)?;

        Ok(Self { root, sub_sheets, root_dir })
    }

    /// Convenience: get a sub-sheet by its filename, if loaded.
    pub fn get_sub_sheet(&self, filename: &str) -> Option<&Schematic> {
        self.sub_sheets.get(filename)
    }

    /// Total sheet count including root (for reporting).
    pub fn total_sheets(&self) -> usize {
        1 + self.sub_sheets.len()
    }
}

/// Recursively load sub-sheets referenced by `parent.sheets`.
///
/// `visited` tracks filenames already loaded (by basename) to break cycles
/// and deduplicate shared sub-sheets.
fn load_sub_sheets_recursive(
    parent: &Schematic,
    dir: &Path,
    sub_sheets: &mut HashMap<String, Schematic>,
    visited: &mut HashSet<String>,
) -> crate::Result<()> {
    for sheet in &parent.sheets {
        let filename = sheet_filename(sheet);
        if filename.is_empty() { continue; }
        if visited.contains(&filename) { continue; }
        visited.insert(filename.clone());

        let sub_path = dir.join(&filename);
        let source = match std::fs::read_to_string(&sub_path) {
            Ok(s) => s,
            Err(_) => {
                // File missing — recorded as a finding by the interface checker.
                // Don't fail the whole load; let validation report it.
                continue;
            }
        };
        let sub_schematic = match parse_schematic(&source, InputFormat::Sexpr) {
            Ok(s) => s,
            Err(_) => continue, // parse error → skip, validator will report missing
        };

        // Recurse into this sub-sheet's own children before inserting.
        load_sub_sheets_recursive(&sub_schematic, dir, sub_sheets, visited)?;
        sub_sheets.insert(filename, sub_schematic);
    }
    Ok(())
}

/// Extract the sub-sheet filename from a Sheet's Sheetfile property.
fn sheet_filename(sheet: &Sheet) -> String {
    sheet.sheet_file.value.clone()
}

// ─── Interface validation ──────────────────────────────────────────

/// Result of checking hierarchical sheet interface consistency.
#[derive(Debug, Clone, Serialize)]
pub struct InterfaceCheckResult {
    pub passed: bool,
    pub findings: Vec<InterfaceFinding>,
}

/// A single interface mismatch between a parent sheet and its child.
#[derive(Debug, Clone, Serialize)]
pub struct InterfaceFinding {
    pub severity: String,   // "error" | "warning"
    pub sheet_name: String, // Parent sheet's display name
    pub sheet_file: String, // Child sheet's filename
    pub pin_name: String,   // Pin/label name involved
    pub message: String,
}

/// Validate that every hierarchical sheet's pins match the child's labels.
///
/// For each `Sheet` in the project (root + any sub-sheet that itself has
/// children), checks:
/// 1. The referenced `sheet_file` exists in `sub_sheets` (else error).
/// 2. Every `SheetPin` has a matching `hierarchical_label` in the child
///    (by name) — else error.
/// 3. Every `hierarchical_label` in the child has a matching `SheetPin` in
///    the parent — else warning (orphan label).
pub fn check_sheet_interfaces(project: &HierarchicalProject) -> InterfaceCheckResult {
    let mut findings = Vec::new();

    // Check root's sheets, then recurse into sub-sheets that have children.
    check_one_level(&project.root, &project.sub_sheets, &mut findings);
    for (_filename, sub) in &project.sub_sheets {
        if sub.sheets.is_empty() { continue; }
        // Sub-sheets with their own children: check them too.
        let mut level_findings = Vec::new();
        check_one_level(sub, &project.sub_sheets, &mut level_findings);
        // Tag findings with the intermediate sheet context.
        for f in level_findings {
            findings.push(f);
        }
    }

    let has_errors = findings.iter().any(|f| f.severity == "error");
    InterfaceCheckResult {
        passed: !has_errors,
        findings,
    }
}

/// Check one level of hierarchy: a parent schematic's sheets vs their children.
fn check_one_level(
    parent: &Schematic,
    sub_sheets: &HashMap<String, Schematic>,
    findings: &mut Vec<InterfaceFinding>,
) {
    for sheet in &parent.sheets {
        let sheet_name = sheet.sheet_name.value.clone();
        let sheet_file = sheet_filename(sheet);

        // Rule 1: sub-sheet file must exist.
        let child = match sub_sheets.get(&sheet_file) {
            Some(s) => s,
            None => {
                findings.push(InterfaceFinding {
                    severity: "error".into(),
                    sheet_name,
                    sheet_file: sheet_file.clone(),
                    pin_name: "—".into(),
                    message: format!("Sub-sheet file '{}' not found or failed to parse", sheet_file),
                });
                continue;
            }
        };

        // Collect child's hierarchical labels by name.
        let child_labels: HashMap<&str, &Label> = child.labels.iter()
            .filter(|l| l.label_type == "hierarchical_label")
            .map(|l| (l.text.as_str(), l))
            .collect();

        // Rule 2: every SheetPin must have a matching hierarchical_label.
        for pin in &sheet.pins {
            if !child_labels.contains_key(pin.name.as_str()) {
                findings.push(InterfaceFinding {
                    severity: "error".into(),
                    sheet_name: sheet_name.clone(),
                    sheet_file: sheet_file.clone(),
                    pin_name: pin.name.clone(),
                    message: format!(
                        "Sheet pin '{}' has no matching hierarchical label in sub-sheet '{}'",
                        pin.name, sheet_file
                    ),
                });
            }
        }

        // Rule 3: orphan hierarchical labels (in child but no matching pin in parent).
        let parent_pin_names: HashSet<&str> = sheet.pins.iter()
            .map(|p| p.name.as_str())
            .collect();
        for (label_name, _label) in &child_labels {
            if !parent_pin_names.contains(label_name) {
                findings.push(InterfaceFinding {
                    severity: "warning".into(),
                    sheet_name: sheet_name.clone(),
                    sheet_file: sheet_file.clone(),
                    pin_name: label_name.to_string(),
                    message: format!(
                        "Hierarchical label '{}' in sub-sheet '{}' has no matching sheet pin in parent",
                        label_name, sheet_file
                    ),
                });
            }
        }
    }
}

// ─── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Minimal valid .kicad_sch wrapping optional sheet blocks.
    fn make_sch(sheets_sexpr: &str) -> String {
        format!(
            r#"(kicad_sch (version 20231120) (generator "eeschema")
  (uuid "00000000-0000-0000-0000-000000000001")
  (paper "A4")
{}
)"#,
            sheets_sexpr
        )
    }

    fn write_temp(name: &str, content: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("p15_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        path
    }

    #[test]
    fn test_load_flat_schematic_no_sheets() {
        // A schematic with no hierarchical sheets should load with 0 sub-sheets.
        let path = write_temp("flat.kicad_sch", &make_sch(""));
        let project = HierarchicalProject::load_from_file(&path).unwrap();
        assert_eq!(project.sub_sheets.len(), 0);
        assert_eq!(project.total_sheets(), 1);
    }

    #[test]
    fn test_load_with_missing_subsheet_does_not_panic() {
        // Root references a sub-sheet that doesn't exist on disk.
        // Load should succeed (sub_sheets empty), validator will report the error.
        let root = make_sch(
            r#"  (sheet (at 50 50) (size 20 10)
    (property "Sheetname" "Missing" (at 50 50))
    (property "Sheetfile" "nonexistent.kicad_sch" (at 50 50))
  )"#,
        );
        let path = write_temp("missing_sub.kicad_sch", &root);
        let project = HierarchicalProject::load_from_file(&path).unwrap();
        assert!(project.sub_sheets.is_empty(), "missing file → not loaded");

        // Validator should flag it.
        let result = check_sheet_interfaces(&project);
        assert!(!result.passed, "missing sub-sheet → fail");
        assert!(result.findings.iter().any(|f| f.message.contains("not found")));
    }

    #[test]
    fn test_cyclic_reference_safe() {
        // A.kicad_sch references B.kicad_sch, B references A → must not loop forever.
        let a = make_sch(
            r#"  (sheet (at 50 50) (size 20 10)
    (property "Sheetname" "B" (at 50 50))
    (property "Sheetfile" "cyc_b.kicad_sch" (at 50 50))
  )"#,
        );
        let b = make_sch(
            r#"  (sheet (at 50 50) (size 20 10)
    (property "Sheetname" "A" (at 50 50))
    (property "Sheetfile" "cyc_a.kicad_sch" (at 50 50))
  )"#,
        );
        write_temp("cyc_b.kicad_sch", &b);
        let path_a = write_temp("cyc_a.kicad_sch", &a);

        // Should complete without hanging.
        let project = HierarchicalProject::load_from_file(&path_a).unwrap();
        assert_eq!(project.sub_sheets.len(), 1, "B loaded once, A deduped");
    }

    #[test]
    fn test_interface_pin_label_match() {
        // Pin "DATA" on parent ↔ hierarchical_label "DATA" in child → no error.
        let child = make_sch(
            r#"  (hierarchical_label "DATA" (shape input) (at 100 100))"#,
        );
        write_temp("match_child.kicad_sch", &child);

        let root = make_sch(
            r#"  (sheet (at 50 50) (size 20 10)
    (property "Sheetname" "Child" (at 50 50))
    (property "Sheetfile" "match_child.kicad_sch" (at 50 50))
    (pin "DATA" input (at 50 55))
  )"#,
        );
        let path = write_temp("match_root.kicad_sch", &root);
        let project = HierarchicalProject::load_from_file(&path).unwrap();
        let result = check_sheet_interfaces(&project);
        assert!(result.passed, "matching pin/label → pass");
        assert!(result.findings.is_empty(), "no findings expected");
    }

    #[test]
    fn test_interface_missing_label() {
        // Pin "CLK" on parent, but child has no hierarchical_label "CLK" → error.
        let child = make_sch(
            r#"  (hierarchical_label "OTHER" (shape output) (at 100 100))"#,
        );
        write_temp("miss_child.kicad_sch", &child);

        let root = make_sch(
            r#"  (sheet (at 50 50) (size 20 10)
    (property "Sheetname" "Child" (at 50 50))
    (property "Sheetfile" "miss_child.kicad_sch" (at 50 50))
    (pin "CLK" input (at 50 55))
  )"#,
        );
        let path = write_temp("miss_root.kicad_sch", &root);
        let project = HierarchicalProject::load_from_file(&path).unwrap();
        let result = check_sheet_interfaces(&project);
        assert!(!result.passed, "missing label → fail");
        assert!(result.findings.iter().any(|f|
            f.severity == "error" && f.pin_name == "CLK" && f.message.contains("no matching")
        ));
    }

    #[test]
    fn test_interface_orphan_label() {
        // Child has hierarchical_label "EXTRA" but parent has no matching pin → warning.
        let child = make_sch(
            r#"  (hierarchical_label "DATA" (shape input) (at 100 100))
  (hierarchical_label "EXTRA" (shape output) (at 100 120))"#,
        );
        write_temp("orphan_child.kicad_sch", &child);

        let root = make_sch(
            r#"  (sheet (at 50 50) (size 20 10)
    (property "Sheetname" "Child" (at 50 50))
    (property "Sheetfile" "orphan_child.kicad_sch" (at 50 50))
    (pin "DATA" input (at 50 55))
  )"#,
        );
        let path = write_temp("orphan_root.kicad_sch", &root);
        let project = HierarchicalProject::load_from_file(&path).unwrap();
        let result = check_sheet_interfaces(&project);
        // DATA matches (no error), EXTRA is orphan (warning) → passed=true (no errors)
        assert!(result.passed, "orphan is warning not error");
        assert!(result.findings.iter().any(|f|
            f.severity == "warning" && f.pin_name == "EXTRA"
        ));
    }
}
