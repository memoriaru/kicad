//! H3: Footprint library metadata — scan KiCad system .pretty libraries and
//! extract dimensional metadata (pitch, pad count, body size, drill) so that
//! `generate_footprint_for_component` can use real dimensions instead of
//! hardcoded defaults.
//!
//! The scanner reads `.kicad_mod` files using a lightweight sexpr pad extractor
//! (not the full kicad-json5 parser — we only need pad positions/sizes/drills).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use rusqlite::params;
use serde::Serialize;

use crate::ComponentDb;

/// Dimensional metadata extracted from a KiCad footprint (.kicad_mod).
#[derive(Debug, Clone, Serialize)]
pub struct FootprintMeta {
    /// Full lib_id, e.g. "Package_SO:SOIC-8_3.9x4.9mm_P1.27mm"
    pub lib_id: String,
    pub pad_count: u32,
    /// Pin pitch in mm (mode of adjacent-pad distances).
    pub pitch: f64,
    /// Row spacing for dual-row packages (mm), if detectable.
    pub row_spacing: Option<f64>,
    /// Body bounding box (width, height) in mm, if detectable.
    pub body_size: Option<(f64, f64)>,
    /// THT drill diameter (mm), if any plated through-hole pads.
    pub drill_size: Option<f64>,
    /// SMD pad size (width, height) in mm.
    pub pad_size: Option<(f64, f64)>,
    /// Source .pretty library name, e.g. "Package_SO".
    pub source_lib: String,
}

/// Detect the KiCad system footprint library directory.
///
/// Checks `KICAD_PATH` env var first, then platform-specific defaults.
/// Returns None if KiCad is not installed (caller falls back gracefully).
pub fn detect_kicad_footprint_dir() -> Option<PathBuf> {
    // 1. KICAD_PATH env var (user override)
    if let Ok(p) = std::env::var("KICAD_PATH") {
        let dir = PathBuf::from(&p).join("footprints");
        if dir.is_dir() {
            return Some(dir);
        }
        // Maybe KICAD_PATH already points at footprints/
        let direct = PathBuf::from(&p);
        if direct.is_dir() && direct.file_name().and_then(|n| n.to_str()) == Some("footprints") {
            return Some(direct);
        }
    }

    // 2. Platform defaults
    let candidates: Vec<PathBuf> = if cfg!(target_os = "macos") {
        vec![
            PathBuf::from("/Applications/KiCad/KiCad.app/Contents/SharedSupport/footprints"),
            PathBuf::from("/Applications/KiCad/KiCad.app/Contents/SharedSupport/Modules"), // KiCad 5
        ]
    } else if cfg!(target_os = "windows") {
        vec![
            PathBuf::from(r"C:\Program Files\KiCad\share\kicad\footprints"),
            PathBuf::from(r"C:\Program Files\KiCad\7.0\share\kicad\footprints"),
        ]
    } else {
        vec![
            PathBuf::from("/usr/share/kicad/footprints"),
            PathBuf::from("/usr/local/share/kicad/footprints"),
        ]
    };

    candidates.into_iter().find(|d| d.is_dir())
}

/// Scan all `.pretty` directories under `dir`, extracting metadata from each
/// `.kicad_mod` file. Returns one `FootprintMeta` per footprint found.
pub fn scan_footprint_library(dir: &Path) -> Vec<FootprintMeta> {
    let mut results = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return results;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        // .pretty directories are footprint libraries
        if !path.is_dir() || !name.ends_with(".pretty") {
            continue;
        }
        let lib_name = name.trim_end_matches(".pretty");
        let Ok(mods) = std::fs::read_dir(&path) else {
            continue;
        };
        for mod_entry in mods.flatten() {
            let mod_path = mod_entry.path();
            if mod_path.extension().and_then(|e| e.to_str()) != Some("kicad_mod") {
                continue;
            }
            let mod_name = mod_path
                .file_stem()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown");
            let lib_id = format!("{}:{}", lib_name, mod_name);
            let Ok(content) = std::fs::read_to_string(&mod_path) else {
                continue;
            };
            if let Some(meta) = parse_footprint_meta(&content, &lib_id, lib_name) {
                results.push(meta);
            }
        }
    }
    results
}

/// Lightweight pad extractor: parses only `(pad ... (at x y) (size w h) (drill d))`
/// blocks from a .kicad_mod sexpr string. Returns raw pad tuples.
/// (x, y, w, h, drill_diameter_or_0, is_tht)
struct RawPad {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    drill: f64, // 0 if none
    is_tht: bool,
}

/// Parse one line's (at)/(size)/(drill) into the pad accumulator. Used for
/// both the header line (v5/v6 single-line pads carry all fields embedded in
/// one self-balanced line) and each line inside a multi-line pad block —
/// fields are located by substring, not line prefix.
fn parse_pad_fields(l: &str, pad: &mut RawPad) {
    if let Some(i) = l.find("(at ") {
        let nums: Vec<f64> = l[i + 4..]
            .split_whitespace()
            .filter_map(|s| s.trim_end_matches(')').parse().ok())
            .collect();
        if nums.len() >= 2 {
            pad.x = nums[0];
            pad.y = nums[1];
        }
    }
    if let Some(i) = l.find("(size ") {
        let nums: Vec<f64> = l[i + 6..]
            .split_whitespace()
            .filter_map(|s| s.trim_end_matches(')').parse().ok())
            .collect();
        if nums.len() >= 2 {
            pad.w = nums[0];
            pad.h = nums[1];
        }
    }
    if let Some(i) = l.find("(drill") {
        let nums: Vec<f64> = l[i + 6..]
            .split_whitespace()
            .skip_while(|s| *s == "oval" || *s == "Oval" || *s == "(drill")
            .map(|s| s.trim_end_matches(')'))
            .filter_map(|s| s.parse().ok())
            .collect();
        if let Some(&d) = nums.first() {
            pad.drill = d;
        }
    }
}

fn extract_pads(content: &str) -> Vec<RawPad> {
    let mut pads = Vec::new();
    // Simple line-based scan for (pad "..." [smd|thru_hole|np_thru_hole] ... blocks
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();
        if line.starts_with("(pad ") {
            let is_tht = line.contains("thru_hole") && !line.contains("np_thru_hole");
            let mut pad = RawPad {
                x: 0.0,
                y: 0.0,
                w: 0.0,
                h: 0.0,
                drill: 0.0,
                is_tht,
            };
            // v5/v6 单行 pad 的括号在头行自平衡（depth=0），块内循环不会跑——
            // 头行必须先做一次字段解析，否则 at/size/drill 全零。
            parse_pad_fields(line, &mut pad);
            // Scan within this pad block (until matching close paren depth).
            // depth starts from the pad header line; the loop must begin at the
            // NEXT line, otherwise the header's balance is counted twice and a
            // multi-line pad block swallows the rest of the file.
            let mut depth = line.matches('(').count() as i32 - line.matches(')').count() as i32;
            let mut j = i + 1;
            while j < lines.len() && depth > 0 {
                let l = lines[j].trim();
                parse_pad_fields(l, &mut pad);
                depth +=
                    lines[j].matches('(').count() as i32 - lines[j].matches(')').count() as i32;
                j += 1;
            }
            pads.push(pad);
            i = j;
        } else {
            i += 1;
        }
    }
    pads
}

/// Compute pitch (mode of y-distances between vertically-adjacent pads).
fn compute_pitch(pads: &[RawPad]) -> f64 {
    if pads.len() < 2 {
        return 1.27; // fallback
    }
    let mut distances: HashMap<i64, u32> = HashMap::new(); // rounded to 0.01mm
    for i in 0..pads.len() {
        for j in (i + 1)..pads.len() {
            let dy = (pads[i].y - pads[j].y).abs();
            let dx = (pads[i].x - pads[j].x).abs();
            // Adjacent pads in same column: small dx, significant dy
            if dx < 0.1 && dy > 0.1 {
                let key = (dy * 100.0).round() as i64;
                *distances.entry(key).or_insert(0) += 1;
            }
        }
    }
    distances
        .iter()
        .max_by_key(|(_, &count)| count)
        .map(|(&key, _)| key as f64 / 100.0)
        .unwrap_or(1.27)
}

/// Compute row spacing (x-distance between left and right pad columns).
fn compute_row_spacing(pads: &[RawPad]) -> Option<f64> {
    if pads.len() < 4 {
        return None;
    }
    let xs: Vec<f64> = pads.iter().map(|p| p.x).collect();
    let (min_x, max_x) = (
        xs.iter().cloned().fold(f64::MAX, f64::min),
        xs.iter().cloned().fold(f64::MIN, f64::max),
    );
    let span = max_x - min_x;
    if span > 1.0 {
        Some(span)
    } else {
        None
    }
}

/// Parse a .kicad_mod string into FootprintMeta. Returns None if no pads found.
fn parse_footprint_meta(content: &str, lib_id: &str, source_lib: &str) -> Option<FootprintMeta> {
    let pads = extract_pads(content);
    if pads.is_empty() {
        return None;
    }
    let pad_count = pads.len() as u32;
    let pitch = compute_pitch(&pads);
    let row_spacing = compute_row_spacing(&pads);

    let drill_size = pads
        .iter()
        .filter(|p| p.is_tht && p.drill > 0.0)
        .map(|p| p.drill)
        .next(); // first THT drill (typically uniform)

    // Pad size from first SMD pad (typically uniform within a package)
    let pad_size = pads
        .iter()
        .find(|p| !p.is_tht && p.w > 0.0)
        .map(|p| (p.w, p.h));

    // Body size: best-effort from courtyard/fabrication fp_rect or fp_line bounds.
    // Skip for now — requires parsing graphics blocks. Left as None.
    let body_size = None;

    Some(FootprintMeta {
        lib_id: lib_id.to_string(),
        pad_count,
        pitch,
        row_spacing,
        body_size,
        drill_size,
        pad_size,
        source_lib: source_lib.to_string(),
    })
}

// ── DB import / lookup ─────────────────────────────────────────────

/// Import a batch of footprint metadata into the `footprint_metadata` table.
/// Uses INSERT OR REPLACE so re-imports update existing entries.
/// Returns the number of rows inserted/updated.
pub fn import_footprint_metadata(db: &ComponentDb, metas: &[FootprintMeta]) -> Result<usize> {
    let tx = db.conn.unchecked_transaction()?;
    let mut count = 0usize;
    for m in metas {
        tx.execute(
            "INSERT OR REPLACE INTO footprint_metadata
             (lib_id, pad_count, pitch, row_spacing, body_w, body_h, drill_size, pad_w, pad_h, source_lib)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                m.lib_id,
                m.pad_count as i64,
                m.pitch,
                m.row_spacing,
                m.body_size.map(|(w, _)| w),
                m.body_size.map(|(_, h)| h),
                m.drill_size,
                m.pad_size.map(|(w, _)| w),
                m.pad_size.map(|(_, h)| h),
                m.source_lib,
            ],
        )?;
        count += 1;
    }
    tx.commit()?;
    Ok(count)
}

/// Look up footprint metadata by lib_id (exact) or package name (fuzzy).
/// Tries exact lib_id match first, then LIKE search on the package portion.
pub fn lookup_footprint_meta(db: &ComponentDb, lib_id_or_package: &str) -> Option<FootprintMeta> {
    // 1. Exact lib_id match
    if let Some(m) = query_meta_exact(&db.conn, lib_id_or_package) {
        return Some(m);
    }
    // 2. Fuzzy: lib_id contains the package name (e.g. "SOIC-8" → "Package_SO:SOIC-8_...")
    query_meta_fuzzy(&db.conn, lib_id_or_package)
}

fn query_meta_exact(conn: &rusqlite::Connection, lib_id: &str) -> Option<FootprintMeta> {
    let row = conn
        .query_row(
            "SELECT * FROM footprint_metadata WHERE lib_id = ?1",
            params![lib_id],
            row_to_meta,
        )
        .ok()?;
    Some(row)
}

fn query_meta_fuzzy(conn: &rusqlite::Connection, package: &str) -> Option<FootprintMeta> {
    let pattern = format!("%{}%", package);
    let row = conn
        .query_row(
            "SELECT * FROM footprint_metadata WHERE lib_id LIKE ?1 LIMIT 1",
            params![pattern],
            row_to_meta,
        )
        .ok()?;
    Some(row)
}

fn row_to_meta(row: &rusqlite::Row) -> rusqlite::Result<FootprintMeta> {
    let lib_id: String = row.get(0)?;
    let pad_count: i64 = row.get(1)?;
    let pitch: f64 = row.get(2)?;
    let row_spacing: Option<f64> = row.get(3)?;
    let body_w: Option<f64> = row.get(4)?;
    let body_h: Option<f64> = row.get(5)?;
    let drill_size: Option<f64> = row.get(6)?;
    let pad_w: Option<f64> = row.get(7)?;
    let pad_h: Option<f64> = row.get(8)?;
    let source_lib: String = row.get(9)?;
    Ok(FootprintMeta {
        lib_id,
        pad_count: pad_count as u32,
        pitch,
        row_spacing,
        body_size: body_w.zip(body_h),
        drill_size,
        pad_size: pad_w.zip(pad_h),
        source_lib,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_pads_soic8() {
        // Minimal SOIC-8 .kicad_mod fragment with 8 SMD pads.
        let content = r#"
(module SOIC-8 (layer F.Cu)
  (pad 1 smd rect (at -2.7 -1.905) (size 1.5 0.6) (layers F.Cu F.Paste F.Mask))
  (pad 2 smd rect (at -2.7 -0.635) (size 1.5 0.6) (layers F.Cu F.Paste F.Mask))
  (pad 3 smd rect (at -2.7 0.635) (size 1.5 0.6) (layers F.Cu F.Paste F.Mask))
  (pad 4 smd rect (at -2.7 1.905) (size 1.5 0.6) (layers F.Cu F.Paste F.Mask))
  (pad 5 smd rect (at 2.7 1.905) (size 1.5 0.6) (layers F.Cu F.Paste F.Mask))
  (pad 6 smd rect (at 2.7 0.635) (size 1.5 0.6) (layers F.Cu F.Paste F.Mask))
  (pad 7 smd rect (at 2.7 -0.635) (size 1.5 0.6) (layers F.Cu F.Paste F.Mask))
  (pad 8 smd rect (at 2.7 -1.905) (size 1.5 0.6) (layers F.Cu F.Paste F.Mask))
)"#;
        let pads = extract_pads(content);
        assert_eq!(pads.len(), 8);
        assert!(!pads[0].is_tht);
        assert_eq!(pads[0].w, 1.5);
        assert_eq!(pads[0].h, 0.6);
    }

    #[test]
    fn test_extract_pads_dip_with_drill() {
        // DIP-4 with THT drill holes
        let content = r#"
(module DIP-4 (layer F.Cu)
  (pad 1 thru_hole rect (at -3.81 -1.27) (size 1.6 1.6) (drill 0.8) (layers *.Cu *.Mask))
  (pad 2 thru_hole circle (at -3.81 1.27) (size 1.6 1.6) (drill 0.8) (layers *.Cu *.Mask))
  (pad 3 thru_hole circle (at 3.81 1.27) (size 1.6 1.6) (drill 0.8) (layers *.Cu *.Mask))
  (pad 4 thru_hole circle (at 3.81 -1.27) (size 1.6 1.6) (drill 0.8) (layers *.Cu *.Mask))
)"#;
        let pads = extract_pads(content);
        assert_eq!(pads.len(), 4);
        assert!(pads[0].is_tht);
        assert_eq!(pads[0].drill, 0.8);
    }

    #[test]
    fn test_compute_pitch_soic8() {
        // SOIC-8 pads at y = ±1.905, ±0.635 → pitch = 1.27mm
        let pads = vec![
            RawPad {
                x: -2.7,
                y: -1.905,
                w: 1.5,
                h: 0.6,
                drill: 0.0,
                is_tht: false,
            },
            RawPad {
                x: -2.7,
                y: -0.635,
                w: 1.5,
                h: 0.6,
                drill: 0.0,
                is_tht: false,
            },
            RawPad {
                x: -2.7,
                y: 0.635,
                w: 1.5,
                h: 0.6,
                drill: 0.0,
                is_tht: false,
            },
            RawPad {
                x: -2.7,
                y: 1.905,
                w: 1.5,
                h: 0.6,
                drill: 0.0,
                is_tht: false,
            },
        ];
        let pitch = compute_pitch(&pads);
        assert!(
            (pitch - 1.27).abs() < 0.01,
            "pitch should be ~1.27mm, got {}",
            pitch
        );
    }

    #[test]
    fn test_compute_row_spacing_soic8() {
        // Two columns at x=±2.7 → row_spacing = 5.4mm.
        // SOIC-8 pad rows (y=±1.905/±0.635) — the <4-pad guard in
        // compute_row_spacing needs a realistic pad count.
        let pads = vec![
            RawPad {
                x: -2.7,
                y: -1.905,
                w: 1.5,
                h: 0.6,
                drill: 0.0,
                is_tht: false,
            },
            RawPad {
                x: -2.7,
                y: -0.635,
                w: 1.5,
                h: 0.6,
                drill: 0.0,
                is_tht: false,
            },
            RawPad {
                x: 2.7,
                y: 0.635,
                w: 1.5,
                h: 0.6,
                drill: 0.0,
                is_tht: false,
            },
            RawPad {
                x: 2.7,
                y: 1.905,
                w: 1.5,
                h: 0.6,
                drill: 0.0,
                is_tht: false,
            },
        ];
        let spacing = compute_row_spacing(&pads).unwrap();
        assert!(
            (spacing - 5.4).abs() < 0.01,
            "row_spacing should be ~5.4mm, got {}",
            spacing
        );
    }

    #[test]
    fn test_parse_footprint_meta_complete() {
        let content = r#"
(module SOIC-8_3.9x4.9mm_P1.27mm (layer F.Cu)
  (pad 1 smd rect (at -2.7 -1.905) (size 1.5 0.6) (drill 0) (layers F.Cu F.Paste F.Mask))
  (pad 2 smd rect (at -2.7 -0.635) (size 1.5 0.6) (layers F.Cu F.Paste F.Mask))
  (pad 3 smd rect (at -2.7 0.635) (size 1.5 0.6) (layers F.Cu F.Paste F.Mask))
  (pad 4 smd rect (at -2.7 1.905) (size 1.5 0.6) (layers F.Cu F.Paste F.Mask))
  (pad 5 smd rect (at 2.7 1.905) (size 1.5 0.6) (layers F.Cu F.Paste F.Mask))
)"#;
        let meta =
            parse_footprint_meta(content, "Package_SO:SOIC-8_3.9x4.9mm_P1.27mm", "Package_SO");
        assert!(meta.is_some());
        let m = meta.unwrap();
        assert_eq!(m.pad_count, 5);
        assert!((m.pitch - 1.27).abs() < 0.01);
        assert!(m.row_spacing.is_some());
        assert_eq!(m.pad_size, Some((1.5, 0.6)));
        assert_eq!(m.source_lib, "Package_SO");
    }

    #[test]
    fn test_detect_kicad_dir_returns_none_if_not_installed() {
        // On CI / machines without KiCad, this returns None — that's OK.
        let dir = detect_kicad_footprint_dir();
        // Don't assert Some/None — just ensure it doesn't panic.
        let _ = dir;
    }

    #[test]
    fn test_import_and_lookup_metadata() {
        let db = ComponentDb::open_in_memory().unwrap();
        let metas = vec![FootprintMeta {
            lib_id: "Package_SO:SOIC-8_3.9x4.9mm_P1.27mm".into(),
            pad_count: 8,
            pitch: 1.27,
            row_spacing: Some(5.4),
            body_size: None,
            drill_size: None,
            pad_size: Some((0.6, 1.5)),
            source_lib: "Package_SO".into(),
        }];
        let count = import_footprint_metadata(&db, &metas).unwrap();
        assert_eq!(count, 1);

        // Exact lookup
        let m = lookup_footprint_meta(&db, "Package_SO:SOIC-8_3.9x4.9mm_P1.27mm");
        assert!(m.is_some());
        assert_eq!(m.unwrap().pad_count, 8);

        // Fuzzy lookup
        let m = lookup_footprint_meta(&db, "SOIC-8");
        assert!(m.is_some(), "fuzzy match should find SOIC-8");

        // Nonexistent
        let m = lookup_footprint_meta(&db, "QFP-999");
        assert!(m.is_none());
    }
}
