//! `.kicad_pro` ↔ [`DesignRules`] bridge — the DRC floor plane.
//!
//! Why this module exists: the board file has **no legal home** for DRC floors.
//! Verified against kicad-cli 10 on 2026-11-16:
//!   * `(constraints …)` inside `.kicad_pcb` → "未知标记";
//!   * `setup` rejects `min_track_width` / `track_min_width` / `min_trace_width`;
//!   * `net_class`'s `trace_width` is the class *default* width, not a floor;
//!   * the floors live in the **project** file at
//!     `board.design_settings.rules`, and a project file containing *only* those
//!     three levels is enforced verbatim (min_clearance 0.5 → 499 clearance
//!     violations on a board that reports 4 without it; min_track_width 0.127 →
//!     the `track_width` violation on a 0.127 track disappears).
//!
//! So a json5 board that declares `rules:` gets a sibling `.kicad_pro` written
//! next to the emitted `.kicad_pcb`, and a forward compile (`.kicad_pcb` → json5)
//! reads that sibling back — otherwise every board silently inherits kicad-cli's
//! built-in defaults (0.2000 clearance / 0.2000 width), which is exactly what all
//! four v42 boards did.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::error::{Error, Result};
use crate::ir::board::DesignRules;

/// `(json, field name)` pairs, in the order KiCad writes them.
pub fn rule_fields(rules: &DesignRules) -> Vec<(&'static str, Option<f64>)> {
    vec![
        ("max_error", rules.max_error),
        ("min_clearance", rules.min_clearance),
        ("min_connection", rules.min_connection),
        ("min_copper_edge_clearance", rules.min_copper_edge_clearance),
        ("min_groove_width", rules.min_groove_width),
        ("min_hole_clearance", rules.min_hole_clearance),
        ("min_hole_to_hole", rules.min_hole_to_hole),
        ("min_microvia_diameter", rules.min_microvia_diameter),
        ("min_microvia_drill", rules.min_microvia_drill),
        ("min_resolved_spokes", rules.min_resolved_spokes),
        ("min_silk_clearance", rules.min_silk_clearance),
        ("min_text_height", rules.min_text_height),
        ("min_text_thickness", rules.min_text_thickness),
        ("min_through_hole_diameter", rules.min_through_hole_diameter),
        ("min_track_width", rules.min_track_width),
        ("min_via_annular_width", rules.min_via_annular_width),
        ("min_via_diameter", rules.min_via_diameter),
        (
            "solder_mask_to_copper_clearance",
            rules.solder_mask_to_copper_clearance,
        ),
    ]
}

fn rules_from_map(map: &Map<String, Value>) -> DesignRules {
    let get = |k: &str| map.get(k).and_then(|v| v.as_f64());
    DesignRules {
        max_error: get("max_error"),
        min_clearance: get("min_clearance"),
        min_connection: get("min_connection"),
        min_copper_edge_clearance: get("min_copper_edge_clearance"),
        min_groove_width: get("min_groove_width"),
        min_hole_clearance: get("min_hole_clearance"),
        min_hole_to_hole: get("min_hole_to_hole"),
        min_microvia_diameter: get("min_microvia_diameter"),
        min_microvia_drill: get("min_microvia_drill"),
        min_resolved_spokes: get("min_resolved_spokes"),
        min_silk_clearance: get("min_silk_clearance"),
        min_text_height: get("min_text_height"),
        min_text_thickness: get("min_text_thickness"),
        min_through_hole_diameter: get("min_through_hole_diameter"),
        min_track_width: get("min_track_width"),
        min_via_annular_width: get("min_via_annular_width"),
        min_via_diameter: get("min_via_diameter"),
        solder_mask_to_copper_clearance: get("solder_mask_to_copper_clearance"),
    }
}

/// Minimal but valid project file. KiCad opens it and fills in the rest; kicad-cli
/// enforces the rules verbatim (probed 2026-11-16).
pub fn rules_to_pro_json(pro_filename: &str, rules: &DesignRules) -> String {
    let mut rl = Map::new();
    for (key, v) in rule_fields(rules) {
        if let Some(v) = v {
            rl.insert(key.into(), Value::from(v));
        }
    }
    let mut ds = Map::new();
    ds.insert("rules".into(), Value::Object(rl));
    let mut board = Map::new();
    board.insert("design_settings".into(), Value::Object(ds));
    let mut meta = Map::new();
    meta.insert("filename".into(), Value::String(pro_filename.to_string()));
    meta.insert("version".into(), Value::from(3_u64));
    let mut root = Map::new();
    root.insert("board".into(), Value::Object(board));
    root.insert("meta".into(), Value::Object(meta));
    serde_json::to_string_pretty(&Value::Object(root)).unwrap_or_else(|_| "{}".into())
}

/// Sibling project path for a board path (`x.kicad_pcb` → `x.kicad_pro`).
pub fn pro_path_for(pcb_path: &Path) -> PathBuf {
    pcb_path.with_extension("kicad_pro")
}

/// Write the sibling project file for a compiled board.
pub fn write_pro_for(pcb_path: &Path, rules: &DesignRules) -> Result<PathBuf> {
    let pro_path = pro_path_for(pcb_path);
    let name = pro_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("board.kicad_pro")
        .to_string();
    std::fs::write(&pro_path, rules_to_pro_json(&name, rules)).map_err(|e| Error::ParserError {
        line: 0,
        column: 0,
        message: format!("cannot write {}: {}", pro_path.display(), e),
    })?;
    Ok(pro_path)
}

/// True when an existing project file is one we generated (root has nothing but
/// `board`+`meta`, and `board` nothing but `design_settings`). Anything richer is
/// a real KiCad project and must not be clobbered by a compile.
pub fn pro_is_generated(pro_path: &Path) -> bool {
    let Ok(src) = std::fs::read_to_string(pro_path) else {
        return false;
    };
    let Ok(root) = serde_json::from_str::<Value>(&src) else {
        return false;
    };
    let Some(obj) = root.as_object() else {
        return false;
    };
    obj.len() == 2
        && obj.contains_key("meta")
        && obj
            .get("board")
            .and_then(|b| b.as_object())
            .map(|b| b.len() == 1 && b.contains_key("design_settings"))
            .unwrap_or(false)
}

/// Read DRC floors back out of a project file, if it declares any.
pub fn read_rules_pro(pro_path: &Path) -> Option<DesignRules> {
    let src = std::fs::read_to_string(pro_path).ok()?;
    let root: Value = serde_json::from_str(&src).ok()?;
    let rules = root
        .get("board")?
        .get("design_settings")?
        .get("rules")?
        .as_object()?;
    let parsed = rules_from_map(rules);
    if parsed.is_empty() {
        None
    } else {
        Some(parsed)
    }
}
