//! Schematic ↔ PCB netlist consistency gate (P1-1).
//!
//! Extracts `(ref,pin) → net` maps from both sides and diffs them pin-by-pin.
//! Net names are normalized (leading hierarchical `/` stripped) and
//! `unconnected-…` auto-nets are skipped — those are intentionally floating pins.

use anyhow::{bail, Context, Result};
use kicad_json5::parser::{Parser, SExpr};
use kicad_json5::Lexer;
use std::collections::BTreeMap;
use std::process::Command;

pub fn normalize(name: &str) -> String {
    name.trim_start_matches('/').to_string()
}

/// Walk the netlist S-expression collecting every `(node (ref ..) (pin ..))`
/// together with its enclosing `(net (name ".."))`.
pub fn extract_sch_netmap(netlist_text: &str) -> Result<BTreeMap<String, String>> {
    let lexer = Lexer::new(netlist_text);
    let mut parser = Parser::new(lexer);
    let root = parser.parse_sexpr()?;

    let mut map = BTreeMap::new();
    fn walk(node: &SExpr, cur_net: Option<&str>, map: &mut BTreeMap<String, String>) {
        if let SExpr::List(items) = node {
            if let Some(head) = items.first().and_then(|h| h.as_ident()) {
                let net_name = if head == "net" {
                    items.iter().find_map(|i| {
                        if let SExpr::List(sub) = i {
                            if sub.first().and_then(|h| h.as_ident()) == Some("name") {
                                return sub
                                    .get(2)
                                    .and_then(|v| v.as_string())
                                    .or_else(|| sub.get(1).and_then(|v| v.as_string()));
                            }
                        }
                        None
                    })
                } else {
                    None
                };
                let cur = net_name.or(cur_net);
                if head == "node" {
                    let mut r = None;
                    let mut p = None;
                    for i in items.iter().skip(1) {
                        if let SExpr::List(sub) = i {
                            match sub.first().and_then(|h| h.as_ident()) {
                                Some("ref") => r = sub.get(1).and_then(|v| v.as_string()),
                                Some("pin") => p = sub.get(1).and_then(|v| v.as_string()),
                                _ => {}
                            }
                        }
                    }
                    if let (Some(r), Some(p)) = (r, p) {
                        if let Some(n) = cur {
                            map.insert(format!("{}.{}", r, p), n.to_string());
                        }
                    }
                }
                for i in items.iter().skip(1) {
                    walk(i, cur, map);
                }
            }
        }
    }
    walk(&root, None, &mut map);
    Ok(map)
}

/// Extract `(ref,pin) → net` from a .kicad_pcb via the Board IR.
pub fn extract_pcb_netmap(pcb_text: &str) -> Result<BTreeMap<String, String>> {
    let board = kicad_json5::parse_board(pcb_text)?;
    if std::env::var("KDESIGN_DEBUG").is_ok() {
        eprintln!(
            "dbg: nets={} fps={} pad0={:?}",
            board.nets.len(),
            board.footprints.len(),
            board.footprints.first().map(|f| (
                f.reference.clone(),
                f.pads
                    .iter()
                    .map(|p| (p.number.clone(), p.net))
                    .collect::<Vec<_>>()
            ))
        );
    }
    let name_by_id: BTreeMap<u32, String> =
        board.nets.iter().map(|n| (n.id, n.name.clone())).collect();
    let mut map = BTreeMap::new();
    for fp in &board.footprints {
        for pad in &fp.pads {
            let Some(id) = pad.net else { continue };
            if id == 0 {
                continue;
            }
            let Some(name) = name_by_id.get(&id) else {
                continue;
            };
            if name.is_empty() {
                continue;
            }
            map.insert(format!("{}.{}", fp.reference, pad.number), name.clone());
        }
    }
    Ok(map)
}

pub struct DiffResult {
    pub matched: usize,
    pub mismatch: Vec<(String, String, String)>,
    pub only_in_sch: Vec<(String, String)>,
    pub only_in_pcb: Vec<(String, String)>,
    /// refs allowed to exist only on the PCB (fixture pads: test points, mounting holes)
    pub pcb_only_prefixes: Vec<String>,
}

impl DiffResult {
    pub fn pass(&self) -> bool {
        let extra = self
            .only_in_pcb
            .iter()
            .filter(|(k, _)| !self.pcb_only_prefixes.iter().any(|p| k.starts_with(p)))
            .count();
        self.mismatch.is_empty() && self.only_in_sch.is_empty() && extra == 0
    }
    pub fn report(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("matched: {}\n", self.matched));
        if !self.mismatch.is_empty() {
            s.push_str(&format!("MISMATCH ({}):\n", self.mismatch.len()));
            for (k, a, b) in &self.mismatch {
                s.push_str(&format!("  {k}: sch='{a}' pcb='{b}'\n"));
            }
        }
        if !self.only_in_sch.is_empty() {
            s.push_str(&format!("MISSING IN PCB ({}):\n", self.only_in_sch.len()));
            for (k, n) in &self.only_in_sch {
                s.push_str(&format!("  {k}: net='{n}'\n"));
            }
        }
        if !self.only_in_pcb.is_empty() {
            s.push_str(&format!("EXTRA IN PCB ({}):\n", self.only_in_pcb.len()));
            for (k, n) in &self.only_in_pcb {
                s.push_str(&format!("  {k}: net='{n}'\n"));
            }
        }
        if self.pass() {
            let extra: Vec<_> = self.only_in_pcb.iter().map(|(k, _)| k.clone()).collect();
            s.push_str(&format!(
                "NETLIST GATE: PASS ({} pins consistent{})\n",
                self.matched,
                if extra.is_empty() {
                    String::new()
                } else {
                    format!(", pcb-only fixture pads: {}", extra.join(", "))
                }
            ));
        } else {
            s.push_str("NETLIST GATE: FAIL\n");
        }
        s
    }
}

pub fn diff(
    sch: BTreeMap<String, String>,
    pcb: BTreeMap<String, String>,
    pcb_only_prefixes: Vec<String>,
) -> DiffResult {
    let mut res = DiffResult {
        matched: 0,
        mismatch: vec![],
        only_in_sch: vec![],
        only_in_pcb: vec![],
        pcb_only_prefixes,
    };
    for (k, sn) in &sch {
        if sn.starts_with("unconnected-") {
            continue;
        }
        match pcb.get(k) {
            Some(pn) if normalize(pn) == normalize(sn) => res.matched += 1,
            Some(pn) => res.mismatch.push((k.clone(), sn.clone(), pn.clone())),
            None => res.only_in_sch.push((k.clone(), sn.clone())),
        }
    }
    for (k, pn) in &pcb {
        match sch.get(k) {
            Some(sn) if normalize(sn) == normalize(pn) => {}
            Some(_) => {}
            None => res.only_in_pcb.push((k.clone(), pn.clone())),
        }
    }
    res
}

/// Export netlist for `sch` (runs kicad-cli unless `sch` is already a .net file).
pub fn sch_netmap(sch: &str, kicad_cli_path: &str) -> Result<BTreeMap<String, String>> {
    let text = if sch.ends_with(".net") {
        std::fs::read_to_string(sch).with_context(|| format!("read {}", sch))?
    } else {
        let out = std::env::temp_dir().join(format!("kdesign-netlist-{}.net", std::process::id()));
        let output = Command::new(kicad_cli_path)
            .args(["sch", "export", "netlist", sch, "-o"])
            .arg(&out)
            .output()
            .with_context(|| format!("Failed to execute '{}'", kicad_cli_path))?;
        if !output.status.success() {
            bail!(
                "kicad-cli sch export netlist failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::fs::read_to_string(&out).context("netlist file not generated")?
    };
    extract_sch_netmap(&text)
}

pub fn pcb_netmap(pcb: &str) -> Result<BTreeMap<String, String>> {
    let text = std::fs::read_to_string(pcb).with_context(|| format!("read {}", pcb))?;
    extract_pcb_netmap(&text)
}

// ---------------------------------------------------------------------------
// 单元测试: 一致性门禁纯函数(归一化/网表提取/diff 判定)
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    const SCH_NETLIST: &str = r#"
(export (version "E")
  (nets
    (net (code "1") (name "GND")
      (node (ref "C1") (pin "1"))
      (node (ref "U1") (pin "2")))
    (net (code "2") (name "/PSU/3V3")
      (node (ref "C1") (pin "2"))
      (node (ref "U1") (pin "4")))
    (net (code "3") (name "unconnected-(U1-Pad5)")
      (node (ref "U1") (pin "5")))))
"#;

    #[test]
    fn normalize_strips_hierarchical_prefix() {
        assert_eq!(normalize("/PSU/3V3"), "PSU/3V3");
        assert_eq!(normalize("GND"), "GND");
    }

    #[test]
    fn extract_sch_netmap_walks_nodes() {
        let m = extract_sch_netmap(SCH_NETLIST).expect("parse");
        assert_eq!(m.get("C1.1").map(String::as_str), Some("GND"));
        assert_eq!(m.get("U1.2").map(String::as_str), Some("GND"));
        assert_eq!(m.get("C1.2").map(String::as_str), Some("/PSU/3V3"));
        assert_eq!(m.get("U1.4").map(String::as_str), Some("/PSU/3V3"));
        assert!(
            m.contains_key("U1.5"),
            "unconnected pins extracted, filtered in diff"
        );
        assert_eq!(m.len(), 5);
    }

    #[test]
    fn pcb_netmap_reads_board_ir_pads() {
        // Official 方言最小板: 顶层 net 表 + 双元 pad 引用
        let pcb = r#"(kicad_pcb (version "20240108") (generator "test")
	(net 0 "")
	(net 1 "GND")
	(net 2 "3V3")
	(footprint "Test:C" (layer "F.Cu") (at 0 0)
		(property "Reference" "C1" (at 0 -2 0) (layer "F.SilkS") (uuid "11111111-1111-1111-1111-111111111111"))
		(pad "1" smd rect (at -1 0) (size 1 1) (layers "F.Cu") (net 1 "GND"))
		(pad "2" smd rect (at 1 0) (size 1 1) (layers "F.Cu") (net 2 "3V3"))
	)
)"#;
        let m = extract_pcb_netmap(pcb).expect("parse board");
        assert_eq!(m.get("C1.1").map(String::as_str), Some("GND"));
        assert_eq!(m.get("C1.2").map(String::as_str), Some("3V3"));
    }

    fn sch_map() -> BTreeMap<String, String> {
        [
            ("C1.1".to_string(), "GND".to_string()),
            ("C1.2".to_string(), "/PSU/3V3".to_string()),
            ("R1.1".to_string(), "SIG".to_string()),
        ]
        .into_iter()
        .collect()
    }

    #[test]
    fn diff_pass_mismatch_and_pcb_only_prefixes() {
        let ok: BTreeMap<String, String> = [
            ("C1.1".to_string(), "GND".to_string()),
            ("C1.2".to_string(), "PSU/3V3".to_string()), // 归一只剥首"/", 保留层级路径
            ("R1.1".to_string(), "SIG".to_string()),
            ("TP1.1".to_string(), "GND".to_string()), // pcb-only fixture pad
        ]
        .into_iter()
        .collect();
        let res = diff(sch_map(), ok, vec!["TP".into()]);
        assert!(res.pass(), "report: {}", res.report());
        assert_eq!(res.matched, 3);

        // 网名不一致 → mismatch
        let bad: BTreeMap<String, String> = [
            ("C1.1".to_string(), "GND".to_string()),
            ("C1.2".to_string(), "5V".to_string()),
            ("R1.1".to_string(), "SIG".to_string()),
        ]
        .into_iter()
        .collect();
        let res = diff(sch_map(), bad, vec![]);
        assert!(!res.pass());
        assert_eq!(res.mismatch.len(), 1);
        assert!(res.report().contains("NETLIST GATE: FAIL"));

        // sch 有 pcb 无 → only_in_sch 计 FAIL
        let missing: BTreeMap<String, String> = [
            ("C1.1".to_string(), "GND".to_string()),
            ("C1.2".to_string(), "PSU/3V3".to_string()),
        ]
        .into_iter()
        .collect();
        let res = diff(sch_map(), missing, vec![]);
        assert!(!res.pass());
        assert_eq!(res.only_in_sch.len(), 1);
    }

    #[test]
    fn diff_skips_unconnected_autonets() {
        let sch: BTreeMap<String, String> = [(
            "U1.5".to_string(),
            "unconnected-(U1-Pad5)-00000000".to_string(),
        )]
        .into_iter()
        .collect();
        let res = diff(sch, BTreeMap::new(), vec![]);
        assert!(
            res.pass(),
            "unconnected auto-nets are intentionally floating"
        );
    }
}
