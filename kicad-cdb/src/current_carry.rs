//! 载流→线宽/过孔数规则引擎
//!
//! 判据来源：
//! - 布局标准序（电源三参数必答：输入从哪来/输出到哪去/每路载流多大；PMU/DC-DC
//!   输出保底 6 过孔，3A/4A+ 再加；线宽与过孔数量同步加宽）
//! - 一个公开案例（它的天花板=没有载流层）
//! 验收锚点（设计审查清单，1oz 外层 30°C 温升工程表）：
//!   0.25mm≈0.9A / 0.5mm≈1.6A / 0.8mm≈2.5A
//!   —— battery 板 5V 主干 0.25mm 只扛 0.9A（对 3A buck 不足，人工修复 1.0/0.8mm）
//!      正是这套判据要自动复算的案例（现为 merged-power-board 合板电源真源 + battery 只参考）。
//!
//! 三层分职（哲学纲领）：本模块=确定性引擎（判据层）；电流需求（语义层）来自
//! VRM 铭牌表 + 人工 override —— 板上没有的东西不臆造，缺数据就报 "need_override"。
//!
//! 模型口径（诚实记账）：
//! - 主模型 = 工程锚点表对数插值（1oz/30°C/外层），IPC-2221 幂律作跨铜厚/温升/内外层
//!   的**缩放因子**：ampacity(w,oz,dt,int) = anchor(w) · oz^0.725 · (dt/30)^0.44 ·
//!   (0.5 if 内层)。IPC-2221 闭式本身（I=0.048·ΔT^0.44·A_mil^0.725）在同区间比人工
//!   表乐观 40-56%，故只取其缩放形状、不取其绝对值——这是"锚点表校准的 IPC 缩放"。

use std::collections::HashMap;

use kicad_json5::ir::board::Board;

// ---------------------------------------------------------------------------
// 模型层：载流
// ---------------------------------------------------------------------------

/// 工程锚点表（1oz 外层 30°C）：设计审查清单 §3 的人工表。
/// 对数插值连续化；表外区间用两端斜率外推（只往保守方向用）。
const ANCHOR_1OZ_30C_EXT: [(f64, f64); 4] = [(0.25, 0.9), (0.5, 1.6), (0.8, 2.5), (1.3, 4.0)];

fn interp_log(table: &[(f64, f64)], x: f64) -> f64 {
    if x <= table[0].0 {
        // 下外推：沿用首段对数斜率（更窄 ⇒ 更小电流）
        let (x0, y0) = table[0];
        let (x1, y1) = table[1];
        let s = (y1.ln() - y0.ln()) / (x1.ln() - x0.ln());
        return (y0.ln() + s * (x.ln() - x0.ln())).exp();
    }
    if x >= table[table.len() - 1].0 {
        let n = table.len();
        let (x0, y0) = table[n - 2];
        let (x1, y1) = table[n - 1];
        let s = (y1.ln() - y0.ln()) / (x1.ln() - x0.ln());
        return (y1.ln() + s * (x.ln() - x1.ln())).exp();
    }
    for w in table.windows(2) {
        if x >= w[0].0 && x <= w[1].0 {
            let t = (x.ln() - w[0].0.ln()) / (w[1].0.ln() - w[0].0.ln());
            return (w[0].1.ln() + t * (w[1].1.ln() - w[0].1.ln())).exp();
        }
    }
    table[0].1
}

/// 过孔载流工程表（保守口径，30°C）：0.2mm→0.5A … 0.8mm→2.5A（孔径=drill）。
const VIA_ANCHOR: [(f64, f64); 5] =
    [(0.2, 0.5), (0.3, 0.8), (0.4, 1.2), (0.5, 1.5), (0.8, 2.5)];

/// 铜厚参数（oz）、温升 ΔT(°C)、是否内层。
#[derive(Debug, Clone, Copy)]
pub struct CopperParams {
    pub oz: f64,
    pub dt_c: f64,
    pub internal: bool,
}

impl Default for CopperParams {
    fn default() -> Self {
        CopperParams { oz: 1.0, dt_c: 30.0, internal: false }
    }
}

/// 线宽载流量（A）。锚点表(1oz/30°C/外层) × IPC-2221 缩放因子。
pub fn ampacity(width_mm: f64, p: &CopperParams) -> f64 {
    let base = interp_log(&ANCHOR_1OZ_30C_EXT, width_mm);
    let oz_scale = p.oz.powf(0.725);
    let dt_scale = (p.dt_c / 30.0).powf(0.44);
    let layer_scale = if p.internal { 0.5 } else { 1.0 };
    base * oz_scale * dt_scale * layer_scale
}

/// 反向：目标电流 → 最小线宽（mm）。二分 [0.05, 8]，40 次收敛。
pub fn width_for_current(i_a: f64, p: &CopperParams) -> f64 {
    let mut lo = 0.05;
    let mut hi = 8.0;
    if ampacity(hi, p) < i_a {
        return hi; // 超表程：返回上限并让调用方报 fail
    }
    for _ in 0..40 {
        let mid = (lo + hi) / 2.0;
        if ampacity(mid, p) < i_a {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    hi
}

/// 过孔载流量（A，孔径 drill mm）。
pub fn via_ampacity(drill_mm: f64) -> f64 {
    interp_log(&VIA_ANCHOR, drill_mm)
}

/// 换层/输出所需过孔数。PMU 保底法则：VRM 输出环节数量至少 6，
/// 其余按电流/单孔容量上取整。
pub fn min_via_count(i_a: f64, drill_mm: f64, pmu_floor: bool) -> usize {
    let per = via_ampacity(drill_mm).max(0.05);
    let need = (i_a / per).ceil() as usize;
    if pmu_floor {
        need.max(6)
    } else {
        need
    }
}

// ---------------------------------------------------------------------------
// 抽取层：电源网 / VRM / 树
// ---------------------------------------------------------------------------

/// VRM 角色（识别方向：buck/ldo 高压→低压；boost 低压→高压）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VrmRole {
    Buck,
    Boost,
    Ldo,
}

impl VrmRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            VrmRole::Buck => "buck",
            VrmRole::Boost => "boost",
            VrmRole::Ldo => "ldo",
        }
    }
}

/// VRM 器件识别表（value/lib_id 小写子串匹配）。可由 override 扩充。
/// 铭牌电流 = 该 VRM 的输出电流额定（保守口径），真实负载用 net 级 override 覆盖。
fn vrm_catalog() -> Vec<(&'static str, VrmRole, f64)> {
    vec![
        // buck（常见同步降压）
        ("sy8120", VrmRole::Buck, 3.0),
        ("sy8088", VrmRole::Buck, 2.0),
        ("tps623", VrmRole::Buck, 3.0),
        ("tps628", VrmRole::Buck, 2.0),
        ("tps543", VrmRole::Buck, 3.0),
        ("tps562", VrmRole::Buck, 2.0),
        ("mp2315", VrmRole::Buck, 3.0),
        ("mp2451", VrmRole::Buck, 0.6),
        ("lmr516", VrmRole::Buck, 1.0),
        // boost（升压）
        ("tps61088", VrmRole::Boost, 3.0),
        ("tps6108", VrmRole::Boost, 2.0),
        ("mp3425", VrmRole::Boost, 2.5),
        ("mt3608", VrmRole::Boost, 1.0),
        // LDO
        ("ams1117", VrmRole::Ldo, 1.0),
        ("rt9193", VrmRole::Ldo, 0.3),
        ("ap2114", VrmRole::Ldo, 0.6),
        ("az1117", VrmRole::Ldo, 1.0),
        ("xc6206", VrmRole::Ldo, 0.25),
        ("me6211", VrmRole::Ldo, 0.5),
        // 负压 LDO（analog-board 板 U50 LT1964 = -9V 轨实锤）与反相/多拓扑 DC-DC
        ("lt1964", VrmRole::Ldo, 0.2),
        ("lt3092", VrmRole::Ldo, 0.2),
        ("lt3093", VrmRole::Ldo, 0.2),
        ("mc34063", VrmRole::Buck, 1.5),
    ]
}

/// 电源网名 → 电压（V）。两种形态：
///   "数字V…"     —— 5V / +13V / 12V_AUX / 13.5V → 5 / 13 / 12 / 13.5
///   "整数V整数"  —— 3V3 / 1V8 / 5V0 → 3.3 / 1.8 / 5.0（KiCad 惯用小数电压名）
/// 解析失败返回 None（非电源网或命名不规范）。GND 系返回 None（由 is_ground 另判）。
pub fn net_voltage(name: &str) -> Option<f64> {
    let n = name.trim();
    if is_ground(n) {
        return None;
    }
    let neg = n.starts_with('-');
    let core = n.trim_start_matches(['+', '-']).to_uppercase();
    let head: String = core
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let rest = &core[head.len()..];
    if head.is_empty() || !rest.starts_with('V') {
        return None;
    }
    let sign = |v: f64| if neg { -v } else { v };
    let tail = &rest[1..]; // V 之后
    // "整数V整数" 形态（尾段以数字开头，允许 _STBY 等后缀）：3V3 / 3V3_STBY → 3.3
    if !head.contains('.') {
        let frac: String = tail.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !frac.is_empty() {
            return format!("{}.{}", head, frac).parse::<f64>().ok().map(sign);
        }
    }
    // "数字V…" 形态：5V / 12V_AUX / 13.5V
    head.parse::<f64>().ok().map(sign)
}

/// 地网判定（GND/AGND/DGND/PGND/GND_x/…）。
pub fn is_ground(name: &str) -> bool {
    let n = name.to_uppercase();
    n == "GND" || n.starts_with("GND") || n.ends_with("GND") || n == "Earth"
}

/// 芯片内部轨/基准网判定——VRM pads 上不算输入输出候选（VCC/VDD/VREF 前缀）。
/// 实锤案：TPS61088 pin1 VCC_AUX = 内部 LDO 输出（挂去耦），被首版误判成 boost 输入。
pub fn is_internal_rail(name: &str) -> bool {
    let u = name.trim().to_uppercase();
    ["VCC", "VDD", "VREF", "VINT", "VLDO"].iter().any(|p| u == *p || u.starts_with(&format!("{}_", p)))
}

/// 源轨判定——电池/系统输入母线（VBAT/VIN/VBUS/VSYS/PWR 前缀），VRM 输入端最优先。
pub fn is_source_rail(name: &str) -> bool {
    let u = name.trim().to_uppercase();
    ["VBAT", "VIN", "VBUS", "VSYS", "VINL", "VINH"].iter().any(|p| u == *p || u.starts_with(&format!("{}_", p)))
}

/// 开关节点/控制信号网判定——VRM pads 上这些网不是电源候选（SW/BOOT/FB/EN/…）。
/// 名单口径：前缀等于名单项（SW/BOOT/FB…）或以 _EN 结尾的使能网。
pub fn is_signal_net(name: &str) -> bool {
    let u = name.trim().to_uppercase();
    if u.is_empty() {
        return true;
    }
    const SIGNAL_PREFIX: [&str; 17] = [
        "SW", "BOOT", "FB", "EN", "SS", "COMP", "LX", "PG", "NC", "ILIM", "FREQ", "ADDR", "TS",
        "BYP", "CT", "SENSE", "IPK",
    ];
    SIGNAL_PREFIX.iter().any(|s| u == *s || u.starts_with(&format!("{}_", s))) || u.ends_with("_EN")
}

/// 识别出的 VRM（电源树节点）。
#[derive(Debug, Clone)]
pub struct VrmNode {
    pub refdes: String,
    pub role: VrmRole,
    /// 铭牌输出电流（A）——override.net_current 优先于它。
    pub iout_rating: f64,
    pub vin_net: String,
    pub vout_net: String,
    pub vin_v: Option<f64>,
    pub vout_v: Option<f64>,
}

/// 一条电源网的审计账。
#[derive(Debug, Clone)]
pub struct PowerNetAudit {
    pub net: String,
    pub voltage: Option<f64>,
    /// 驱动它的 VRM（None = 板级输入/电池，无上游）。
    pub source_vrm: Option<String>,
    pub downstream_vrms: Vec<String>,
    /// 该网挂的非 VRM 器件数（负载规模参考）。
    pub loads: usize,
    /// 电流需求（A）：override > VRM 铭牌 > None(需人工)。
    pub need_i: Option<f64>,
    /// 板上实际：最窄段宽（mm）、段数、过孔数、最小孔径。
    pub min_width: f64,
    pub seg_count: usize,
    pub via_count: usize,
    pub min_drill: f64,
    /// 判定（need_i 已知时）。
    pub pass: Option<bool>,
    pub have_i: Option<f64>,
    pub need_w: Option<f64>,
}

/// 人工输入通道：网级电流 override、扩展 VRM 表、铜参数。
#[derive(Debug, Clone, Default)]
pub struct CurrentOverrides {
    pub net_current: HashMap<String, f64>,
    /// (子串, 角色, 铭牌 A)
    pub extra_vrms: Vec<(String, VrmRole, f64)>,
    pub copper: CopperParams,
    /// 板级源网（电池/接插件入口）→ 电流，用于无 VRM 上游的输入网。
    pub source_current: HashMap<String, f64>,
}

/// VRM 输入/输出方向判定（启发式核心，独立成函数供回归测试）。
/// 规则按优先级：
///   1. VBAT/VIN/VBUS/VSYS 前缀的**源轨**当输入；输出 = 其余里 |V| 最高的已知电压网
///   2. 未知电压网（母线，None）当输入；输出同上
///   3. 全部已知：buck/ldo 高|V|→低|V|、boost 低|V|→高|V|（**绝对值**——负压 LDO
///      输入是绝对值更大的轨，analog-board U50 LT1964 pin1(-9V)=IN→pin5(-6V)=OUT 实锤案）
/// 输出位只从已知电压网里选——None 网（CT/SENSE 类）不能因排序键退化抢占输出位
/// （U31 输出曾被 CT 顶掉的回归案）。
pub fn pick_direction(
    role: VrmRole,
    pnets: &[(String, Option<f64>)],
) -> (String, Option<f64>, String, Option<f64>) {
    let absv = |v: &Option<f64>| v.unwrap_or(f64::NEG_INFINITY).abs();
    let pick_vout = |cands: &mut Vec<(String, Option<f64>)>| -> (String, Option<f64>) {
        if cands.iter().any(|(_, v)| v.is_some()) {
            let mut known: Vec<_> = cands.iter().filter(|(_, v)| v.is_some()).cloned().collect();
            known.sort_by(|a, b| absv(&a.1).partial_cmp(&absv(&b.1)).unwrap());
            known.pop().unwrap()
        } else {
            cands.pop().unwrap()
        }
    };
    let mut rest: Vec<(String, Option<f64>)> = pnets.to_vec();
    if let Some(i) = rest.iter().position(|(n, _)| is_source_rail(n)) {
        let vin = rest.remove(i);
        let vout = pick_vout(&mut rest);
        return (vin.0, vin.1, vout.0, vout.1);
    }
    if let Some(i) = rest.iter().position(|(_, v)| v.is_none()) {
        let vin = rest.remove(i);
        let vout = pick_vout(&mut rest);
        return (vin.0, vin.1, vout.0, vout.1);
    }
    rest.sort_by(|a, b| absv(&a.1).partial_cmp(&absv(&b.1)).unwrap());
    match role {
        VrmRole::Boost => {
            let vin = rest.remove(0);
            let vout = rest.pop().unwrap();
            (vin.0, vin.1, vout.0, vout.1)
        }
        _ => {
            let vin = rest.pop().unwrap();
            let vout = rest.remove(0);
            (vin.0, vin.1, vout.0, vout.1)
        }
    }
}

/// 从 Board 抽电源树（VRM 节点 + 网归属）。
/// 电源网 = net_voltage() 可解析的网 ∪ VRM pads 上出现的非地网（VBAT_SW 这类
/// 电池/母线名解析不出电压，但它们是 VRM 的真实输入——merged-power-board 首跑实证：
/// 只认可解析网会把 boost/buck 全跳过）。
pub fn extract_power_tree(board: &Board, ov: &CurrentOverrides) -> Vec<VrmNode> {
    let mut catalog = vrm_catalog();
    for (s, r, i) in &ov.extra_vrms {
        catalog.push((s.as_str(), *r, *i));
    }

    let mut out = Vec::new();
    for fp in &board.footprints {
        let hay = format!("{} {}", fp.value, fp.lib_id).to_lowercase();
        let hit = catalog.iter().find(|(s, _, _)| hay.contains(s));
        let (role, rating) = match hit {
            Some((_, r, i)) => (*r, *i),
            None => continue,
        };
        // 该器件 pad 上的电源候选网：可解析电压网 ∪ 电池/母线形态网（VBAT_SW 等解析
        // 不出电压但是真实输入）；三类排除——SW/BOOT/FB/EN 等信号网、VCC/VDD/VREF 等
        // 芯片内部轨（TPS61088 pin1 VCC_AUX=内部 LDO 输出，首版误判输入的实锤案）。
        let mut pnets: Vec<(String, Option<f64>)> = Vec::new();
        for p in &fp.pads {
            if let Some(n) = &p.net_name {
                if is_ground(n) || n.is_empty() || is_signal_net(n) || is_internal_rail(n) {
                    continue;
                }
                if !pnets.iter().any(|(x, _)| x == n) {
                    pnets.push((n.clone(), net_voltage(n)));
                }
            }
        }
        if pnets.len() < 2 {
            continue; // 输入输出不可分辨，跳过（报 need_override 由 audit 层兜底）
        }
        let (vin_name, vin_v, vout_name, vout_v) = pick_direction(role, &pnets);
        out.push(VrmNode {
            refdes: fp.reference.clone(),
            role,
            iout_rating: rating,
            vin_net: vin_name,
            vout_net: vout_name,
            vin_v,
            vout_v,
        });
    }
    out.sort_by(|a, b| a.refdes.cmp(&b.refdes));
    out
}

/// 逐电源网审计：需求电流 vs 板上实际线宽/过孔。
pub fn audit_current_carry(board: &Board, vrms: &[VrmNode], ov: &CurrentOverrides) -> Vec<PowerNetAudit> {
    let copper = &ov.copper;
    // 每网统计：最窄段 / 段数 / via 数 / 负载数
    let mut nets: HashMap<String, (f64, usize, usize, f64)> = HashMap::new(); // (min_w, segs, vias, min_drill)
    let mut net_loads: HashMap<String, usize> = HashMap::new();
    let vrm_refs: Vec<&str> = vrms.iter().map(|v| v.refdes.as_str()).collect();

    for seg in &board.segments {
        let name = net_display(board, seg.net);
        let e = nets.entry(name).or_insert((f64::INFINITY, 0, 0, f64::INFINITY));
        e.0 = e.0.min(seg.width);
        e.1 += 1;
    }
    for via in &board.vias {
        let name = net_display(board, via.net);
        let e = nets.entry(name).or_insert((f64::INFINITY, 0, 0, f64::INFINITY));
        e.2 += 1;
        e.3 = e.3.min(via.drill);
    }
    for fp in &board.footprints {
        if vrm_refs.contains(&fp.reference.as_str()) {
            continue;
        }
        let mut seen: Vec<&str> = Vec::new();
        for p in &fp.pads {
            if let Some(n) = &p.net_name {
                if !seen.contains(&n.as_str()) {
                    seen.push(n);
                }
            }
        }
        for n in seen {
            *net_loads.entry(n.to_string()).or_insert(0) += 1;
        }
    }

    let mut all_power_nets: Vec<String> = board
        .nets
        .iter()
        .filter(|nd| {
            net_voltage(&nd.name).is_some()
                || is_source_rail(&nd.name)
                || ["TEC", "PWR", "VDRV", "VMOT"].iter().any(|p| {
                    let u = nd.name.to_uppercase();
                    u == *p || u.starts_with(&format!("{}_", p)) || u.ends_with(&format!("_{}", p))
                })
        })
        .map(|nd| nd.name.clone())
        .collect();
    for v in vrms {
        for n in [&v.vin_net, &v.vout_net] {
            if !all_power_nets.contains(n) {
                all_power_nets.push(n.clone());
            }
        }
    }
    for n in ov.source_current.keys() {
        if !all_power_nets.contains(n) {
            all_power_nets.push(n.clone());
        }
    }
    all_power_nets.sort();

    let mut out = Vec::new();
    for net in all_power_nets {
        if is_ground(&net) {
            continue;
        }
        let st = nets
            .get(&net)
            .cloned()
            .unwrap_or((f64::INFINITY, 0, 0, f64::INFINITY));
        let source_vrm = vrms
            .iter()
            .find(|v| v.vout_net == net)
            .map(|v| v.refdes.clone());
        let downstream: Vec<String> = vrms
            .iter()
            .filter(|v| v.vin_net == net)
            .map(|v| v.refdes.clone())
            .collect();
        // 电流需求优先级：override > 铭牌（+ 级联下游铭牌和）> source 表 > None
        let mut need_i = ov.net_current.get(&net).copied();
        if need_i.is_none() {
            if let Some(v) = vrms.iter().find(|v| v.vout_net == net) {
                let mut i = v.iout_rating;
                for d in &downstream {
                    if let Some(dn) = vrms.iter().find(|x| x.refdes == *d) {
                        // 下游输入电流 ≈ 下游输出功率 / 本网电压（效率 0.85 计）
                        if let (Some(vo), Some(vi)) = (dn.vout_v, net_voltage(&net)) {
                            if vi.abs() > 0.1 {
                                i += dn.iout_rating * vo.abs() / vi.abs() / 0.85;
                            }
                        }
                    }
                }
                need_i = Some(i);
            } else if let Some(i) = ov.source_current.get(&net) {
                need_i = Some(*i);
            }
        }
        let have_i = if st.0.is_finite() { Some(ampacity(st.0, copper)) } else { None };
        let need_w = need_i.map(|i| width_for_current(i, copper));
        let pass = match (have_i, need_i) {
            (Some(h), Some(n)) => Some(h + 1e-9 >= n),
            _ => None,
        };
        let loads = *net_loads.get(&net).unwrap_or(&0);
        out.push(PowerNetAudit {
            voltage: net_voltage(&net),
            net,
            source_vrm,
            downstream_vrms: downstream,
            loads,
            need_i,
            min_width: if st.0.is_finite() { st.0 } else { 0.0 },
            seg_count: st.1,
            via_count: st.2,
            min_drill: if st.3.is_finite() { st.3 } else { 0.0 },
            pass,
            have_i,
            need_w,
        });
    }
    out
}

fn net_display(board: &Board, id: u32) -> String {
    board
        .nets
        .iter()
        .find(|n| n.id == id)
        .map(|n| n.name.clone())
        .unwrap_or_else(|| format!("net#{}", id))
}

/// 文本报告（人读版）：逐网一行 + PMU 过孔建议。
pub fn format_report(audits: &[PowerNetAudit], vrms: &[VrmNode], ov: &CurrentOverrides) -> String {
    let mut s = String::from("== 电源树（自动抽取） ==\n");
    for v in vrms {
        s.push_str(&format!(
            "  {} [{}] {}({:?}V) → {}({:?}V)  铭牌 {}A\n",
            v.refdes, v.role.as_str(), v.vin_net, v.vin_v, v.vout_net, v.vout_v, v.iout_rating
        ));
    }
    s.push_str("\n== 载流审计（铜参数 ");
    s.push_str(&format!(
        "{}oz/ΔT{}°C/{}） ==\n",
        ov.copper.oz, ov.copper.dt_c, if ov.copper.internal { "内层" } else { "外层" }
    ));
    for a in audits {
        let verdict = match a.pass {
            Some(true) => "✅",
            Some(false) => "❌",
            None => "❓",
        };
        s.push_str(&format!(
            "  {v} {net:<14} V={vol:<5} 源={src:<6} 下游={dw:<10} 段={seg:<3} 最窄={w:>5.2}mm 扛={hi} 需={ni} 建议≥{nw}mm via={vc}(≥{vcn})\n",
            v = verdict,
            net = a.net,
            vol = a.voltage.map(|x| x.to_string()).unwrap_or_else(|| "?".into()),
            src = a.source_vrm.clone().unwrap_or_else(|| "(板级输入)".into()),
            dw = if a.downstream_vrms.is_empty() { "-".into() } else { a.downstream_vrms.join(",") },
            seg = a.seg_count,
            w = a.min_width,
            hi = a.have_i.map(|x| format!("{:.2}A", x)).unwrap_or_else(|| "-".into()),
            ni = a.need_i.map(|x| format!("{:.2}A", x)).unwrap_or_else(|| "需override".into()),
            nw = a.need_w.map(|x| format!("{:.2}", x)).unwrap_or_else(|| "?".into()),
            vc = a.via_count,
            vcn = a
                .need_i
                .map(|i| min_via_count(i, if a.min_drill > 0.0 { a.min_drill } else { 0.3 }, a.source_vrm.is_some()).to_string())
                .unwrap_or_else(|| "?".into()),
        ));
    }
    s
}

// ---------------------------------------------------------------------------
// 规则入库：把锚点校准模型写成 DesignRule 行（rules.rs 引擎可执行）
// ---------------------------------------------------------------------------

/// 载流规则集（IPC-2221 幂律形态，k 已按审查清单锚点表校准：mm 系 k=2.46，
/// 使 0.25mm/1oz/30°C 精确命中 0.9A；0.5/0.8mm 端幂律略低于人工表=保守方向）。
/// 返回 (name, description, parameters, formula_expr, check_expr, output_params)。
pub fn builtin_current_rules() -> Vec<(&'static str, &'static str, &'static str, &'static str, &'static str, &'static str)> {
    vec![
        (
            "ipc2221_external_ampacity",
            "外层载流量(锚点表校准): I = 2.46·(ΔT/30)^0.44·w_mm^0.725·oz^0.725 (1oz/30°C: 0.25mm→0.9A)",
            r#"["width_mm","dt_c","oz"]"#,
            "i_max = 2.46 * pow(dt_c / 30, 0.44) * pow(width_mm, 0.725) * pow(oz, 0.725)",
            "i_max > 0",
            r#"["i_max"]"#,
        ),
        (
            "current_min_width_check",
            "载流线宽判据: 板上 width_mm 的载流量须 ≥ 需求电流 i_need (A)",
            r#"["width_mm","i_need","dt_c","oz"]"#,
            "i_max = 2.46 * pow(dt_c / 30, 0.44) * pow(width_mm, 0.725) * pow(oz, 0.725)",
            "i_max >= i_need",
            r#"["i_max"]"#,
        ),
        (
            "via_count_check",
            "过孔数判据(工程表拟合): 单孔 i_via=3.25·d_mm^1.16 (0.3→0.8A/0.5→1.45A/0.8→2.5A), n_via ≥ i_need/i_via (PMU 输出保底 6 孔由代码层 min_via_count 实施)",
            r#"["n_via","i_need","drill_mm"]"#,
            "i_via = 3.25 * pow(drill_mm, 1.16)",
            "n_via >= i_need / i_via",
            r#"["i_via"]"#,
        ),
    ]
}

/// 把载流规则集写入 design_rules 表（rules.rs 引擎可直接 apply_rule 执行）。
/// 返回插入的规则 id。
pub fn seed_current_rules(db: &crate::ComponentDb) -> anyhow::Result<Vec<i64>> {
    let mut ids = Vec::new();
    for (name, desc, params, formula, check, outputs) in builtin_current_rules() {
        let id = db.insert_design_rule(&crate::models::DesignRule {
            id: None,
            name: name.to_string(),
            category_id: None,
            description: Some(desc.to_string()),
            condition_expr: None,
            formula_expr: Some(formula.to_string()),
            check_expr: Some(check.to_string()),
            parameters: Some(params.to_string()),
            output_params: Some(outputs.to_string()),
            source: Some(
                "工程经验表(1oz/30°C 温升锚点) + PMU 输出保底 6 过孔".into(),
            ),
            domain: Some("power".into()),
            tags: Some("current,ampacity,ipc2221,calibrated".into()),
        })?;
        ids.push(id);
    }
    Ok(ids)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 验收锚点 1：工程手册的人工表逐点命中（±5%）。
    #[test]
    fn test_anchor_table_hits_review_checklist() {
        let p = CopperParams::default(); // 1oz / 30°C / 外层
        for (w, i) in [(0.25, 0.9), (0.5, 1.6), (0.8, 2.5)] {
            let got = ampacity(w, &p);
            assert!(
                (got - i).abs() / i < 0.05,
                "w={}mm: got {:.3}A, expect {}A",
                w,
                got,
                i
            );
        }
    }

    /// 验收锚点 2：battery 5V 主干案例复算——0.25mm 扛 ~0.9A，
    /// 3A 需求反解线宽应落在人工修复档（0.8-1.0mm）。
    #[test]
    fn test_battery_5v_trunk_case() {
        let p = CopperParams::default();
        assert!(ampacity(0.25, &p) < 1.0, "0.25mm 必须扛不住 1A");
        let w = width_for_current(3.0, &p);
        assert!(w > 0.7 && w < 1.1, "3A 需求线宽 got {}mm, expect 0.8-1.0", w);
    }

    #[test]
    fn test_scaling_factors() {
        let base = CopperParams::default();
        // 铜厚 2oz ≈ ×2^0.725 ≈ 1.65
        let two_oz = CopperParams { oz: 2.0, ..base };
        let r = ampacity(0.5, &two_oz) / ampacity(0.5, &base);
        assert!((r - 1.65).abs() < 0.05, "2oz scale = {:.3}", r);
        // 内层减半
        let inner = CopperParams { internal: true, ..base };
        assert!((ampacity(0.5, &inner) / ampacity(0.5, &base) - 0.5).abs() < 1e-9);
        // ΔT=10°C ≈ (10/30)^0.44 ≈ 0.61
        let cool = CopperParams { dt_c: 10.0, ..base };
        let r = ampacity(0.5, &cool) / ampacity(0.5, &base);
        assert!((r - 0.61).abs() < 0.02, "dt scale = {:.3}", r);
    }

    #[test]
    fn test_width_for_current_roundtrip() {
        let p = CopperParams::default();
        for i in [0.3, 0.9, 1.6, 2.5, 3.5] {
            let w = width_for_current(i, &p);
            assert!(ampacity(w, &p) >= i - 1e-6, "i={} w={}", i, w);
        }
    }

    #[test]
    fn test_via_ampacity_and_pmu_floor() {
        assert!((via_ampacity(0.3) - 0.8).abs() < 0.05);
        assert!((via_ampacity(0.5) - 1.5).abs() < 0.05);
        // PMU 保底：小电流输出也至少 6 孔
        assert_eq!(min_via_count(0.3, 0.3, true), 6);
        // 3A / 0.8A每孔 = 4 → 保底 6
        assert_eq!(min_via_count(3.0, 0.3, true), 6);
        // 3A / 0.5mm 孔(1.5A) = 2 → 保底 6
        assert_eq!(min_via_count(3.0, 0.5, true), 6);
        // 非 PMU：按电流取整
        assert_eq!(min_via_count(3.0, 0.5, false), 2);
        assert_eq!(min_via_count(1.0, 0.3, false), 2);
    }

    /// 方向判定五类真板实锤案（merged-power-board B_U3 / analog-board U50 / analog-board U31 / battery SY8120 /
    /// analog-board VCC_AUX）全部锁死——这是抽取器启发式的回归门。
    #[test]
    fn test_pick_direction_five_real_cases() {
        let n = |s: &str| (s.to_string(), net_voltage(s));
        // ① boost + 源轨优先 + 内部轨已在上游排除：TPS61088 候选=电池(None)+12V_AUX
        let (vi, _, vo, _) = pick_direction(VrmRole::Boost, &[n("VBAT_SW"), n("12V_AUX")]);
        assert_eq!((vi.as_str(), vo.as_str()), ("VBAT_SW", "12V_AUX"));
        // ② 负压 LDO 绝对值排序：LT1964 -9V(IN)→-6V(OUT)
        let (vi, _, vo, _) = pick_direction(VrmRole::Ldo, &[n("-9V"), n("-6V")]);
        assert_eq!((vi.as_str(), vo.as_str()), ("-9V", "-6V"));
        // ③ 反相 DC-DC + CT/SENSE 信号网混入时输出位不被 None 网抢占：
        //    MC34063 候选=VBAT(None)+CT(None)+-9V(known)
        let (vi, _, vo, _) = pick_direction(
            VrmRole::Buck,
            &[n("VBAT_SW"), n("CT"), n("-9V")],
        );
        assert_eq!((vi.as_str(), vo.as_str()), ("VBAT_SW", "-9V"));
        // ④ 全已知 buck 高|V|→低|V|：SY8120 12V→5V
        let (vi, _, vo, _) = pick_direction(VrmRole::Buck, &[n("12V"), n("5V")]);
        assert_eq!((vi.as_str(), vo.as_str()), ("12V", "5V"));
        // ⑤ LDO 阶梯 + 未知母线：RT9193 候选=+15V、+13V 全已知
        let (vi, _, vo, _) = pick_direction(VrmRole::Ldo, &[n("+15V"), n("+13V")]);
        assert_eq!((vi.as_str(), vo.as_str()), ("+15V", "+13V"));
    }

    /// 入库端到端：seed → 按名取回 → apply_rule 引擎执行 → 锚点值 + 判定双向验证。
    #[test]
    fn test_seed_and_apply_via_db() {
        let db = crate::ComponentDb::open_in_memory().unwrap();
        let ids = seed_current_rules(&db).unwrap();
        assert_eq!(ids.len(), 3);

        let rule = db.get_rule_by_name("current_min_width_check").unwrap().unwrap();
        // 0.25mm/1oz/30°C 需 0.9A：恰好在锚点上 → pass（i_max≈0.9 ≥ 0.9）
        let ok = db
            .apply_rule(
                &rule,
                &serde_json::json!({"width_mm": 0.25, "i_need": 0.9, "dt_c": 30.0, "oz": 1.0}),
                None,
                None,
            )
            .unwrap();
        assert!(ok.pass, "0.25mm 扛 0.9A 应压线通过, i_max={:?}", ok.outputs.get("i_max"));
        // battery 5V 主干案例：0.25mm 需 3A → fail
        let bad = db
            .apply_rule(
                &rule,
                &serde_json::json!({"width_mm": 0.25, "i_need": 3.0, "dt_c": 30.0, "oz": 1.0}),
                None,
                None,
            )
            .unwrap();
        assert!(!bad.pass, "0.25mm 扛 3A 必须判失败");
    }

    #[test]
    fn test_net_voltage_parsing() {
        assert_eq!(net_voltage("5V"), Some(5.0));
        assert_eq!(net_voltage("3V3"), Some(3.3));
        assert_eq!(net_voltage("1V8"), Some(1.8));
        assert_eq!(net_voltage("5V0"), Some(5.0));
        assert_eq!(net_voltage("+13V"), Some(13.0));
        assert_eq!(net_voltage("12V_AUX"), Some(12.0));
        assert_eq!(net_voltage("13.5V"), Some(13.5));
        assert_eq!(net_voltage("VBAT_SW"), None); // 前缀非数字
        assert_eq!(net_voltage("GND"), None);
        assert_eq!(net_voltage("DGND"), None);
        assert!(is_ground("AGND") && is_ground("GND1") && !is_ground("5V"));
    }

    #[test]
    fn test_builtin_rules_executable_by_rule_engine() {
        // 与 rules.rs 的 EvalContext 语法对拍（pow/ln 已支持）
        let mut ctx = crate::rules::EvalContext::new();
        ctx.set("width_mm", 0.25);
        ctx.set("dt_c", 30.0);
        ctx.set("oz", 1.0);
        let out = ctx
            .eval_formula("i_max = 2.46 * pow(dt_c / 30, 0.44) * pow(width_mm, 0.725) * pow(oz, 0.725)")
            .unwrap();
        // 校准系数 2.46 下 0.25mm/30°C/1oz → 精确命中锚点 0.9A
        let i = out[0].1;
        assert!((i - 0.9).abs() < 0.02, "k_cal gives {}A", i);
        // 过孔表拟合：0.3mm 孔 → ~0.8A
        ctx.set("drill_mm", 0.3);
        let v = ctx.eval_formula("i_via = 3.25 * pow(drill_mm, 1.16)").unwrap();
        assert!((v[0].1 - 0.8).abs() < 0.05, "via 0.3mm gives {}A", v[0].1);
    }
}
