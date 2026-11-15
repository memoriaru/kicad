//! 模块树（P1）：analog-board-modules.json5 的加载、扁平化与器件 bbox。
//!
//! 模块树是布局操作的语义层地基（ModuPlace 式）：LLM 生成初稿 + 人审定稿，
//! 本模块只做机械加载与几何计算，不解释语义。

use std::collections::BTreeMap;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct ModuleTree {
    #[serde(default)]
    pub modules: BTreeMap<String, ModuleDef>,
}

#[derive(Debug, Deserialize)]
pub struct ModuleDef {
    #[serde(default)]
    pub desc: String,
    #[serde(default)]
    pub devices: Vec<String>,
    #[serde(default)]
    pub children: BTreeMap<String, ModuleDef>,
}

/// 扁平化：模块路径（"power_tree/rail_15v"）→ 器件列表。
/// 顶层模块自身devices也保留在该路径下（与 children 分列）。
pub fn flatten(tree: &ModuleTree) -> BTreeMap<String, Vec<String>> {
    let mut out = BTreeMap::new();
    fn walk(prefix: &str, m: &ModuleDef, out: &mut BTreeMap<String, Vec<String>>) {
        if !m.devices.is_empty() {
            out.entry(prefix.to_string()).or_default().extend(m.devices.iter().cloned());
        }
        for (name, child) in &m.children {
            walk(&format!("{prefix}/{name}"), child, out);
        }
    }
    for (name, m) in &tree.modules {
        walk(name, m, &mut out);
    }
    out
}

/// 全部器件 → 模块路径 的反查表（器件重复归属时保留首次，与建树纪律一致）。
pub fn device_index(tree: &ModuleTree) -> BTreeMap<String, String> {
    let mut idx = BTreeMap::new();
    let flat = flatten(tree);
    for (path, devs) in &flat {
        for d in devs {
            idx.entry(d.clone()).or_insert_with(|| path.clone());
        }
    }
    idx
}

/// 模块 bbox：器件 pad 世界坐标（复用 audit::pad_world_points 的旋转口径）
/// 并集，空模块返回 None。返回 (min_x, min_y, max_x, max_y)。
pub fn module_bbox(
    board: &kicad_json5::ir::board::Board,
    devices: &[String],
) -> Option<(f64, f64, f64, f64)> {
    use kicad_json5::ir::board::Board as B;
    let want: std::collections::HashSet<&str> = devices.iter().map(|s| s.as_str()).collect();
    let mut mm = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    let mut any = false;
    for fp in &board.footprints {
        if !want.contains(fp.reference.as_str()) {
            continue;
        }
        // footprint position 本身入 bbox（body 中心），pad 世界坐标扩边
        let (px, py, _) = fp.position;
        mm.0 = mm.0.min(px); mm.1 = mm.1.min(py);
        mm.2 = mm.2.max(px); mm.3 = mm.3.max(py);
        any = true;
        for pad in &fp.pads {
            if pad.net.is_none() {
                continue;
            }
            let (ox, oy) = fp.pad_rotated_offset(pad);
            let (x, y) = (px + ox, py + oy);
            mm.0 = mm.0.min(x); mm.1 = mm.1.min(y);
            mm.2 = mm.2.max(x); mm.3 = mm.3.max(y);
            any = true;
        }
    }
    let _ = B::default; // 防未使用告警（无实际作用时移除）
    if any { Some(mm) } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flatten_and_index() {
        let tree: ModuleTree = json5::from_str(r#"
        {
            _meta: { note: "ignored" },
            modules: {
                power: { devices: ["U1", "C1"], children: {
                    rail: { devices: ["L1"] },
                }},
                digi: { devices: ["J1"] },
            },
        }"#).unwrap();
        let flat = flatten(&tree);
        assert_eq!(flat["power"], vec!["U1", "C1"]);
        assert_eq!(flat["power/rail"], vec!["L1"]);
        assert_eq!(flat["digi"], vec!["J1"]);
        let idx = device_index(&tree);
        assert_eq!(idx["L1"], "power/rail");
        assert_eq!(idx["U1"], "power");
    }
}
