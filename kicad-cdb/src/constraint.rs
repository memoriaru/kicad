//! 分组等长约束：声明式 schema（json5）→ 解析到板上的网 → 等长偏差报告。
//!
//! 判据来源：DDR 布线规则（数据组 11 根一组、组内 ±25mil = 0.635mm / DQS·时钟差分对内
//! <5mil = 0.127mm / 数据不换层 / fly-by 靠 CS 分时复用）、差分对与网络接口设计
//! （USB2.0 无等长 / 百兆只对内 / SGMII 对间 ±100mil = 2.54mm / 等长宁可做小）。
//!
//! 一源三吃（三层分职）：
//!   声明（本模块 schema，人或 AI 产）→ ①router 消费（BusGroupSpec 注入口=层协调
//!   与 skew 骨架打通）②评估器（eval_length_matching=组内偏差报告，判据层）
//!   ③报告（format_length_report）。
//! "宁可做小"口径：达标线 tolerance，预警线 tolerance×0.7。

use std::collections::HashMap;

use kicad_json5::ir::board::Board;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Schema（json5 声明形态）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ConstraintSet {
    #[serde(default)]
    pub groups: Vec<ConstraintGroup>,
    #[serde(default)]
    pub diff_pairs: Vec<DiffPairConstraint>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ConstraintGroup {
    pub name: String,
    /// 支持通配 "DQ0..7"（展开 DQ0..DQ7 共 8 根）与裸名。
    pub nets: Vec<String>,
    pub tolerance_mm: f64,
    /// "longest" | "net:CLK0"（解析失败默认 longest）。
    #[serde(default = "default_reference")]
    pub reference: String,
    /// "any" | "same_layer" | "fly_by"。
    #[serde(default = "default_topology")]
    pub topology: String,
    #[serde(default)]
    pub priority: u32,
}

fn default_reference() -> String {
    "longest".into()
}
fn default_topology() -> String {
    "any".into()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DiffPairConstraint {
    pub nets: [String; 2],
    pub z0_ohm: f64,
    /// None = 无等长要求（USB2.0）。
    pub tolerance_mm: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RefMode {
    Longest,
    Net(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum TopologyMode {
    Any,
    SameLayer,
    FlyBy,
}

impl TopologyMode {
    pub fn parse(s: &str) -> TopologyMode {
        match s {
            "same_layer" => TopologyMode::SameLayer,
            "fly_by" => TopologyMode::FlyBy,
            _ => TopologyMode::Any,
        }
    }
}

/// 解析声明（json5 文本）。
pub fn parse_constraints(text: &str) -> anyhow::Result<ConstraintSet> {
    let mut cs: ConstraintSet = json5::from_str(text)?;
    if cs.groups.is_empty() && cs.diff_pairs.is_empty() {
        anyhow::bail!("constraints 声明为空（groups/diff_pairs 至少其一）");
    }
    expand_in_place(&mut cs);
    Ok(cs)
}

/// 通配展开就地应用（parse 与 template 共用——template 首版绕过 parse 致
/// BYTE0 "DQ0..7" 不展开只剩 4 根的实测回归）。
pub fn expand_in_place(cs: &mut ConstraintSet) {
    for g in &mut cs.groups {
        g.nets = g.nets.iter().flat_map(|n| expand_net_spec(n)).collect();
    }
}

/// "DQ0..7" → DQ0..DQ7；裸名原样。
pub fn expand_net_spec(spec: &str) -> Vec<String> {
    if let Some((head, tail)) = spec.rsplit_once("..") {
        if let Ok(hi) = tail.trim().parse::<u32>() {
            // head = "DQ0"，取其尾部数字做 lo
            let digits: String = head
                .chars()
                .rev()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .chars()
                .rev()
                .collect();
            if !digits.is_empty() {
                if let Ok(lo) = digits.parse::<u32>() {
                    let prefix = &head[..head.len() - digits.len()];
                    if lo <= hi && hi - lo < 64 {
                        return (lo..=hi).map(|i| format!("{prefix}{i}")).collect();
                    }
                }
            }
        }
    }
    vec![spec.to_string()]
}

// ---------------------------------------------------------------------------
// 接口模板库（数值来自 DDR / 差分接口的公开布线规范）
// ---------------------------------------------------------------------------

/// 一键生成：usb2 / eth-100m / sgmii / rgmii / ddr3-1600。
pub fn template(name: &str) -> Option<ConstraintSet> {
    let s = match name {
        "usb2" => json5::from_str(
            r#"{ diff_pairs: [ { nets: ["USB_DP","USB_DM"], z0_ohm: 90, tolerance_mm: null } ] }"#,
        ),
        "eth-100m" => json5::from_str(
            r#"{ groups: [ { name: "MDI_PAIR", nets: ["TX_P","TX_N","RX_P","RX_N"],
                tolerance_mm: 0.127, reference: "longest" } ] }"#,
        ),
        "sgmii" => json5::from_str(
            r#"{ groups: [
                { name: "SGMII_TX", nets: ["SGMII_TX_P","SGMII_TX_N"], tolerance_mm: 0.127, reference: "longest" },
                { name: "SGMII_RX", nets: ["SGMII_RX_P","SGMII_RX_N"], tolerance_mm: 0.127, reference: "longest" } ],
              diff_pairs: [ { nets: ["SGMII_TX_P","SGMII_TX_N"], z0_ohm: 100, tolerance_mm: 0.127 },
                            { nets: ["SGMII_RX_P","SGMII_RX_N"], z0_ohm: 100, tolerance_mm: 0.127 } ] }"#,
        ),
        "rgmii" => json5::from_str(
            r#"{ groups: [
                { name: "RGMII_TXC", nets: ["TXC"], tolerance_mm: 0.635, reference: "longest" },
                { name: "RGMII_TXD", nets: ["TXD0..3","TX_CTL"], tolerance_mm: 0.635, reference: "net:TXC", topology: "same_layer", priority: 1 },
                { name: "RGMII_RXD", nets: ["RXD0..3","RX_CTL","RXC"], tolerance_mm: 0.635, reference: "longest", topology: "same_layer", priority: 1 } ] }"#,
        ),
        "ddr3-1600" => json5::from_str(
            r#"{ groups: [
                { name: "BYTE0", nets: ["DQ0..7","DQS0_P","DQS0_N","DQM0"], tolerance_mm: 0.635,
                  reference: "longest", topology: "same_layer", priority: 2 },
                { name: "BYTE1", nets: ["DQ8..15","DQS1_P","DQS1_N","DQM1"], tolerance_mm: 0.635,
                  reference: "longest", topology: "same_layer", priority: 2 },
                { name: "DQS_PAIRS", nets: ["DQS0_P","DQS0_N","DQS1_P","DQS1_N","CLK_P","CLK_N"],
                  tolerance_mm: 0.127, reference: "longest", priority: 3 },
                { name: "ADDR_CTRL", nets: ["A0..15","CAS","RAS","WE","BA0","BA1","BA2","CKE","ODT","CS"],
                  tolerance_mm: 1.27, reference: "net:CLK_P", topology: "fly_by", priority: 1 } ],
              diff_pairs: [ { nets: ["CLK_P","CLK_N"], z0_ohm: 100, tolerance_mm: 0.127 } ] }"#,
        ),
        _ => return None,
    };
    let mut cs = s.ok()?;
    expand_in_place(&mut cs);
    Some(cs)
}

// ---------------------------------------------------------------------------
// resolve：声明 → 板上网 id（附缺失清单）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ResolvedGroup {
    pub name: String,
    pub net_ids: Vec<u32>,
    pub tolerance_mm: f64,
    pub reference: RefMode,
    pub topology: TopologyMode,
    pub priority: u32,
    /// 声明了但板上不存在的网。
    pub missing: Vec<String>,
}

pub fn resolve(board: &Board, cs: &ConstraintSet) -> Vec<ResolvedGroup> {
    let by_name: HashMap<&str, u32> = board.nets.iter().map(|n| (n.name.as_str(), n.id)).collect();
    let mut out = Vec::new();
    for g in &cs.groups {
        let mut ids = Vec::new();
        let mut missing = Vec::new();
        for n in &g.nets {
            match by_name.get(n.as_str()) {
                Some(id) => ids.push(*id),
                None => missing.push(n.clone()),
            }
        }
        let reference = if let Some(net) = g.reference.strip_prefix("net:") {
            RefMode::Net(net.to_string())
        } else {
            RefMode::Longest
        };
        out.push(ResolvedGroup {
            name: g.name.clone(),
            net_ids: ids,
            tolerance_mm: g.tolerance_mm,
            reference,
            topology: TopologyMode::parse(&g.topology),
            priority: g.priority,
            missing,
        });
    }
    out
}

// ---------------------------------------------------------------------------
// 评估器（判据层）：布线后组内长度偏差
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct GroupLengthReport {
    pub group: String,
    /// 每网 (名, 段总长 mm, via 数)。
    pub net_lengths: Vec<(String, f64, usize)>,
    /// 基准网（最长或指定参考）。
    pub reference_net: String,
    /// 组内最大偏差（相对基准，mm）。
    pub max_dev: f64,
    /// max_dev ≤ tolerance。
    pub pass: bool,
    /// "宁可做小"预警：max_dev > tolerance×0.7（未超容差先黄牌）。
    pub warn: bool,
    /// same_layer 违规网（via>0）。
    pub layer_changes: Vec<String>,
}

/// 单网铜长 = Σ段长（via 几何长度不计入，单独计 via 数——时延一致性换算留口径不做）。
fn net_length(board: &Board, id: u32) -> (f64, usize) {
    let mut len = 0.0;
    let mut vias = 0;
    for s in &board.segments {
        if s.net == id {
            len += (s.end.0 - s.start.0).hypot(s.end.1 - s.start.1);
        }
    }
    for v in &board.vias {
        if v.net == id {
            vias += 1;
        }
    }
    (len, vias)
}

pub fn eval_length_matching(board: &Board, groups: &[ResolvedGroup]) -> Vec<GroupLengthReport> {
    let name_of = |id: u32| -> String {
        board
            .nets
            .iter()
            .find(|n| n.id == id)
            .map(|n| n.name.clone())
            .unwrap_or_else(|| format!("#{id}"))
    };
    let mut out = Vec::new();
    for g in groups {
        if g.net_ids.len() < 2 {
            continue;
        }
        let mut lens: Vec<(String, f64, usize)> = g
            .net_ids
            .iter()
            .map(|id| {
                let (l, v) = net_length(board, *id);
                (name_of(*id), l, v)
            })
            .collect();
        // 基准网：参考网指定（不在组内则退最长——"时钟是最长基准"）
        let longest = |lens: &[(String, f64, usize)]| -> (String, f64) {
            lens.iter()
                .fold(None, |acc: Option<&(String, f64, usize)>, x| match acc {
                    Some(a) if a.1 >= x.1 => Some(a),
                    _ => Some(x),
                })
                .map(|(n, l, _)| (n.clone(), *l))
                .unwrap_or_default()
        };
        let (ref_net, ref_len) = match &g.reference {
            RefMode::Net(rn) => lens
                .iter()
                .find(|(n, _, _)| n == rn)
                .map(|(n, l, _)| (n.clone(), *l))
                .unwrap_or_else(|| longest(&lens)),
            RefMode::Longest => longest(&lens),
        };
        let max_dev = lens
            .iter()
            .map(|(_, l, _)| (ref_len - l).abs())
            .fold(0.0, f64::max);
        let layer_changes = if g.topology == TopologyMode::SameLayer {
            lens.iter()
                .filter(|(_, _, v)| *v > 0)
                .map(|(n, _, _)| n.clone())
                .collect()
        } else {
            vec![]
        };
        lens.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        out.push(GroupLengthReport {
            group: g.name.clone(),
            net_lengths: lens,
            reference_net: ref_net,
            max_dev,
            pass: max_dev <= g.tolerance_mm + 1e-9,
            warn: max_dev > g.tolerance_mm * 0.7 && max_dev <= g.tolerance_mm + 1e-9,
            layer_changes,
        });
    }
    out
}

/// 人读报告。
pub fn format_length_report(reports: &[GroupLengthReport], tolerance_hint: &str) -> String {
    let mut s = format!("== 分组等长报告（容差 {tolerance_hint}；预警线=容差×0.7 宁可做小） ==\n");
    if reports.is_empty() {
        s.push_str("  （无可评估组——组内网数 <2 或全部缺失）\n");
        return s;
    }
    for r in reports {
        let v = if r.pass && !r.warn {
            "✅"
        } else if r.pass {
            "🟡"
        } else {
            "❌"
        };
        s.push_str(&format!(
            "  {v} [{}] 基准 {} 组内最大偏差 {:+.3}mm（{} 根）\n",
            r.group,
            r.reference_net,
            r.max_dev,
            r.net_lengths.len()
        ));
        for (n, l, v) in &r.net_lengths {
            s.push_str(&format!(
                "      {n:<12} {l:>8.2}mm  via={v} {}\n",
                if *v > 0 && r.layer_changes.contains(n) {
                    "⚠️换层"
                } else {
                    ""
                }
            ));
        }
    }
    s
}

// ---------------------------------------------------------------------------
// router 桥（数学层入口）：BusGroupSpec → router 内部 BusGroup（thread_local override）
// ---------------------------------------------------------------------------

/// 声明形态的 bus group（pub；router.rs 的 set_bus_groups_override 消费）。
#[derive(Debug, Clone)]
pub struct BusGroupSpec {
    pub name: String,
    pub net_ids: Vec<u32>,
    pub intra_skew_mm: f64,
    pub inter_skew_mm: Option<f64>,
    pub reference_group: Option<usize>,
    pub priority: u32,
}

/// ResolvedGroup → BusGroupSpec（router 骨架的层协调/skew 消费入口）。
pub fn to_bus_group_specs(groups: &[ResolvedGroup]) -> Vec<BusGroupSpec> {
    groups
        .iter()
        .map(|g| BusGroupSpec {
            name: g.name.clone(),
            net_ids: g.net_ids.clone(),
            intra_skew_mm: g.tolerance_mm,
            inter_skew_mm: None,
            reference_group: None,
            priority: g.priority,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use kicad_json5::ir::board::{NetDef, Segment, Via};

    #[test]
    fn test_expand_net_spec() {
        assert_eq!(
            expand_net_spec("DQ0..3"),
            ["DQ0", "DQ1", "DQ2", "DQ3"]
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        );
        assert_eq!(expand_net_spec("A0..15").len(), 16);
        assert_eq!(expand_net_spec("CLK_P"), vec!["CLK_P"]);
        // 越界/畸形不展开
        assert_eq!(expand_net_spec("DQ0..999"), vec!["DQ0..999"]);
        assert_eq!(expand_net_spec("X..3"), vec!["X..3"]);
    }

    #[test]
    fn test_parse_and_template_ddr3() {
        let cs = template("ddr3-1600").expect("模板存在");
        let byte0 = cs.groups.iter().find(|g| g.name == "BYTE0").unwrap();
        assert_eq!(byte0.nets.len(), 11, "数据组 11 根一组");
        assert!((byte0.tolerance_mm - 0.635).abs() < 1e-9, "±25mil");
        let dqs = cs.groups.iter().find(|g| g.name == "DQS_PAIRS").unwrap();
        assert!((dqs.tolerance_mm - 0.127).abs() < 1e-9, "对内 <5mil");
        let addr = cs.groups.iter().find(|g| g.name == "ADDR_CTRL").unwrap();
        assert_eq!(addr.reference, "net:CLK_P", "控制地址参考时钟");
        // json5 声明解析往返
        let text = json5::to_string(&cs).unwrap();
        let back = parse_constraints(&text).unwrap();
        assert_eq!(back.groups.len(), cs.groups.len());
    }

    fn mkboard() -> Board {
        Board {
            nets: vec![NetDef {
                id: 0,
                name: String::new(),
            }],
            ..Default::default()
        }
    }

    /// 评估器：组内偏差计算 + pass/warn(×0.7) + same_layer 换层违规。
    #[test]
    fn test_eval_length_matching() {
        let mut b = mkboard();
        let names = ["DQ0", "DQ1", "DQ2", "DQS0_P"];
        for (i, n) in names.iter().enumerate() {
            b.nets.push(NetDef {
                id: i as u32 + 1,
                name: n.to_string(),
            });
        }
        // 长度: DQ0=10, DQ1=9, DQ2=10.5(基准), DQS0_P=10.2; DQ1 加 1 via(same_layer 违规)
        b.segments.push(Segment {
            start: (0.0, 0.0),
            end: (10.0, 0.0),
            width: 0.2,
            layer: "F.Cu".into(),
            net: 1,
        });
        b.segments.push(Segment {
            start: (0.0, 1.0),
            end: (9.0, 1.0),
            width: 0.2,
            layer: "F.Cu".into(),
            net: 2,
        });
        b.segments.push(Segment {
            start: (0.0, 2.0),
            end: (10.5, 2.0),
            width: 0.2,
            layer: "F.Cu".into(),
            net: 3,
        });
        b.segments.push(Segment {
            start: (0.0, 3.0),
            end: (10.2, 3.0),
            width: 0.2,
            layer: "F.Cu".into(),
            net: 4,
        });
        b.vias.push(Via {
            at: (4.5, 1.0),
            size: 0.6,
            drill: 0.3,
            layers: vec!["F.Cu".into(), "B.Cu".into()],
            net: 2,
            via_type: String::new(),
        });

        let cs = parse_constraints(
            r#"{ groups: [ { name: "BYTE0", nets: ["DQ0","DQ1","DQ2","DQS0_P"],
                tolerance_mm: 0.635, topology: "same_layer" } ] }"#,
        )
        .unwrap();
        let groups = resolve(&b, &cs);
        assert!(groups[0].missing.is_empty());
        let rpt = eval_length_matching(&b, &groups);
        assert_eq!(rpt.len(), 1);
        let r = &rpt[0];
        assert_eq!(r.reference_net, "DQ2", "基准=最长");
        // 最大偏差 = |10.5-9.0| = 1.5 > 0.635 ⇒ fail
        assert!((r.max_dev - 1.5).abs() < 1e-9);
        assert!(!r.pass);
        assert!(!r.warn, "超容差时 warn 语义失效(❌)");
        assert_eq!(r.layer_changes, vec!["DQ1"], "DQ1 换层违规");
        // 收紧到刚好 0.7×tolerance < dev ≤ tolerance ⇒ warn 黄牌
        let g2 = ResolvedGroup {
            name: "G".into(),
            net_ids: groups[0].net_ids.clone(),
            tolerance_mm: 1.5,
            reference: RefMode::Longest,
            topology: TopologyMode::Any,
            priority: 0,
            missing: vec![],
        };
        let r2 = &eval_length_matching(&b, std::slice::from_ref(&g2))[0];
        assert!(r2.pass && r2.warn, "dev=tolerance 边界=黄牌");

        // 参考网指定: net:DQ0 ⇒ 基准 10.0, 全组最大偏差 = |10-9| = 1.0 (DQ1 最短)
        let g3 = ResolvedGroup {
            reference: RefMode::Net("DQ0".into()),
            topology: TopologyMode::Any,
            ..g2.clone()
        };
        let r3 = &eval_length_matching(&b, std::slice::from_ref(&g3))[0];
        assert_eq!(r3.reference_net, "DQ0");
        assert!((r3.max_dev - 1.0).abs() < 1e-9);
    }

    /// resolve 缺失网报告（板上没有的声明网如实列出，不臆造）。
    #[test]
    fn test_resolve_missing() {
        let b = mkboard();
        let cs = parse_constraints(
            r#"{ groups: [ { name: "G", nets: ["DQ0","GHOST"], tolerance_mm: 1.0 } ] }"#,
        )
        .unwrap();
        let g = resolve(&b, &cs).pop().unwrap();
        assert_eq!(g.missing, vec!["DQ0".to_string(), "GHOST".to_string()]);
        assert!(g.net_ids.is_empty());
    }
}
