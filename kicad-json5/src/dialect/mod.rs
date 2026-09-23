//! 源文件方言分支（多版本支持）——解析与文本手术共用的唯一分派点。
//!
//! 历史教训（zone / net / Reference / fp_text 四次方言漂移各自散点打补丁）：
//! 版本差异的知识必须集中在这里，消费方 `match` 分支分派，禁止再写散点 if。
//!
//! 已知方言（.kicad_pcb 实测）：
//! | version   | 来源                    | footprint 标识              |
//! |-----------|-------------------------|-----------------------------|
//! | 20260206  | KiCad 10 定制工具链生成 | `(property "Reference" …)` |
//! | 20240108  | KiCad 8 原生保存        | `(fp_text reference "…")`  |
//!
//! 分派规则：内容特征优先（版本头与风格非单调映射，见 detect 注释）。

/// 板级源文件方言
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardDialect {
    /// 新链（KiCad 9/10+）：footprint 标识为 `(property "Reference" …)`，
    /// 值可能同行、也可能在下一行独立引号行
    PropertyRef,
    /// 旧版（KiCad 8 原生）：footprint 标识为 `(fp_text reference "…")`，值同行
    FpTextRef,
}

/// 提取首个引号串（`"abc"` → `abc`；行尾有尾随内容也安全）
fn quoted(v: &str) -> Option<String> {
    let v = v.trim();
    let v = v.strip_prefix('"')?;
    v.split('"').next().map(String::from)
}

impl BoardDialect {
    /// 按内容特征探测方言（版本头不可靠：自研生成器给 20231120 头配 property 风格，
    /// KiCad 8 原生 20240108 却是 fp_text 风格——版本→风格非单调映射，内容才是真值）。
    /// 板内无任何 footprint 标识行时默认 PropertyRef（新链生成器的空板形态）。
    pub fn detect(text: &str) -> Self {
        if text.contains("(property \"Reference\"") {
            BoardDialect::PropertyRef
        } else if text.contains("(fp_text reference") {
            BoardDialect::FpTextRef
        } else {
            BoardDialect::PropertyRef
        }
    }

    /// footprint 块内单行是否为 Reference 声明，是则解出 ref 值。
    /// `next_line`：PropertyRef 的多行写法（值在下一行独立引号行）需要它。
    pub fn parse_reference_line(&self, line: &str, next_line: Option<&str>) -> Option<String> {
        let t = line.trim_start();
        match self {
            BoardDialect::PropertyRef => {
                if !t.starts_with("(property \"Reference\"") {
                    return None;
                }
                // 同行带值：(property "Reference" "C15"
                let rest = t["(property \"Reference\"".len()..].trim();
                if rest.starts_with('"') {
                    return quoted(rest);
                }
                // 值在下一行独立引号行
                next_line.and_then(quoted)
            }
            BoardDialect::FpTextRef => {
                let rest = t.strip_prefix("(fp_text reference")?;
                quoted(rest)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_by_content() {
        // 版本头不可信（自研生成器 20231120 + property；KiCad8 20240108 + fp_text）——内容优先
        assert_eq!(
            BoardDialect::detect("(kicad_pcb (version 20231120)\n(property \"Reference\" \"R1\")"),
            BoardDialect::PropertyRef
        );
        assert_eq!(
            BoardDialect::detect("(kicad_pcb (version 20240108)\n(fp_text reference \"R1\")"),
            BoardDialect::FpTextRef
        );
        // 无 footprint 标识行 → 默认 PropertyRef
        assert_eq!(
            BoardDialect::detect("(kicad_pcb (version 20240108)"),
            BoardDialect::PropertyRef
        );
    }

    #[test]
    fn parse_reference_both_styles() {
        let d = BoardDialect::PropertyRef;
        // 多行：值在下一行
        assert_eq!(
            d.parse_reference_line("\t\t(property \"Reference\"", Some("\t\t\t\"TP8\"")),
            Some("TP8".into())
        );
        // 同行带值
        assert_eq!(
            d.parse_reference_line("\t\t(property \"Reference\" \"C15\"", None),
            Some("C15".into())
        );
        let d = BoardDialect::FpTextRef;
        assert_eq!(
            d.parse_reference_line("\t\t(fp_text reference \"C1\"", None),
            Some("C1".into())
        );
        // 非声明行
        assert_eq!(d.parse_reference_line("\t\t(at 0 0)", None), None);
    }
}

// ---------------------------------------------------------------------------
// 厂商方言适配器：官方 KiCad ↔ 华秋 fork
// ---------------------------------------------------------------------------
/// 两家 kicad-cli 对同一 s-expr 语法的接受域不同（同名不同义、此有彼无）。
/// 知识必须集中在本模块（同 BoardDialect 纪律：消费方 match 分派，禁止散点 if）。
///
/// 实证差异全集（各附出处）：
/// | # | 差异点 | 官方 | 华秋 fork | 状态 |
/// |---|--------|------|-----------|------|
/// | V1 | net 引用 | `(net 1 "GND")` 编号+名 | pad/seg/via/zone 曾见 name-only `(net "GND")`，载入要求**有名** | 读端 normalize_net_name_refs 归一；写端双参 ✓ |
/// | V2 | connect_pads token | 宽容 | 拒收裸 `thermal`（"应为 yes, no, or clearance"，09-18 afe 板实测） | 生成端空串=省略形态 ✓ |
/// | V3 | paper | "User" W H 合法 | 拒载非标 paper（电池板 09-13 实测 → 兜底 A4） | 本模块 emit 整备 |
/// | V4 | 丝印层名 | KiCad 10 新名 `F.Silkscreen` | 板内只认旧名 `F.SilkS`（新名报未定义层；且 CLI gerber 导出旧名也静默跳过——绘制器缺陷另案） | 本模块 emit 映射 |
/// | V5 | 版本头 | 20240108(K8)/20231120… | 20260206（fork 10 定制） | 仅探测特征，不改写 |
///
/// 立场：**IR 是中间表示，方言是读写策略**——读入已归一，本模块管 emit 整备
/// 与探测；官方侧差异暂为恒等（官方对旧名/省略 token 均兼容，有实证再扩）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VendorDialect {
    /// 官方 KiCad（kicad-cli 上游）
    Official,
    /// 华秋 fork（10.0.6 huaqiu，本项目权威 DRC 裁判）
    Huaqiu,
}

impl VendorDialect {
    /// 内容特征探测（版本头不可信，同 BoardDialect 哲学）：
    /// - 板内出现 KiCad 10 新丝印名 `F.Silkscreen` → 官方产物（华秋拒载新名，
    ///   能写出这文件的一定不是华秋链）；
    /// - 版本头 20260206 → 华秋定制头；
    /// - 其余（旧层名 + 常规版本头）→ 不可分辨，默认 Huaqiu
    ///   （本项目权威链是华秋，emit 侧从严总是安全的：官方兼容旧名/省略 token）。
    pub fn detect_board(text: &str) -> Self {
        if text.contains("F.Silkscreen") || text.contains("B.Silkscreen") {
            return VendorDialect::Official;
        }
        if text.contains("(version 20260206)") {
            return VendorDialect::Huaqiu;
        }
        VendorDialect::Huaqiu
    }

    /// 标准图纸名集合（ISO A 系 + US 系；华秋 fork 实测拒载其外的取值）
    fn is_standard_paper(paper: &str) -> bool {
        matches!(
            paper,
            "A0" | "A1" | "A2" | "A3" | "A4" | "A5" | "A" | "B" | "C" | "D" | "E"
        )
    }

    /// emit 前整备：按目标方言原地修整 Board IR。
    /// 幂等（对已整备的板重复调用零变化）。
    pub fn prepare_board(&self, board: &mut crate::ir::board::Board) {
        match self {
            // 官方：恒等（现有 IR 形态即官方兼容形态）
            VendorDialect::Official => {}
            VendorDialect::Huaqiu => {
                // V3 paper 兜底：非标准名（含 "User" 尺寸形态）→ A4
                let paper = board.paper.clone();
                let head = paper.split_whitespace().next().unwrap_or("A4");
                if !Self::is_standard_paper(head) {
                    board.paper = "A4".into();
                }
                // V4 丝印层名压回 fork 旧名（FpText / FpLine / FpCircle / FpArc /
                // FpRect / FpPoly 的 layer 字段；Zone 同理）
                let rename = |l: &mut String| {
                    if l == "F.Silkscreen" {
                        *l = "F.SilkS".into();
                    } else if l == "B.Silkscreen" {
                        *l = "B.SilkS".into();
                    }
                };
                for fp in &mut board.footprints {
                    for t in &mut fp.fp_texts {
                        rename(&mut t.layer);
                    }
                    for l in &mut fp.fp_lines {
                        rename(&mut l.layer);
                    }
                    for c in &mut fp.fp_circles {
                        rename(&mut c.layer);
                    }
                    for a in &mut fp.fp_arcs {
                        rename(&mut a.layer);
                    }
                    for r in &mut fp.fp_rects {
                        rename(&mut r.layer);
                    }
                    for p in &mut fp.fp_polys {
                        rename(&mut p.layer);
                    }
                }
                for z in &mut board.zones {
                    // zone 的层是单层名（board.rs Zone.layer）
                    rename(&mut z.layer);
                }
                // V4 层声明表同映射（华秋 fixed layer hash 按名查，新名直接拒载）
                for l in &mut board.layers {
                    rename(&mut l.name);
                }
                // V2 connect_pads：裸 thermal 已在生成端以空串省略（board_gen），
                // IR 若被显式填入 "thermal"（如手工构造）也压回省略形态
                for z in &mut board.zones {
                    if z.pad_connect == "thermal" {
                        z.pad_connect.clear();
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod vendor_tests {
    use super::*;

    #[test]
    fn detect_by_silkscreen_name() {
        assert_eq!(
            VendorDialect::detect_board("(layer \"F.Silkscreen\")"),
            VendorDialect::Official
        );
        assert_eq!(
            VendorDialect::detect_board("(version 20260206)\n(layer \"F.SilkS\")"),
            VendorDialect::Huaqiu
        );
        // 不可分辨默认华秋（权威链从严）
        assert_eq!(
            VendorDialect::detect_board("(version 20240108)\n(layer \"F.SilkS\")"),
            VendorDialect::Huaqiu
        );
    }

    #[test]
    fn huaqiu_prepare_is_idempotent_and_renames_silk() {
        let mut b = crate::ir::board::Board::new();
        b.paper = "User 159.995 140.005".into();
        use crate::ir::board::*;
        let mut fp = Footprint::new("T:R", "R1", "1k");
        fp.fp_texts.push(FpText {
            text: "R1".into(),
            text_type: FpTextType::Reference,
            position: (0.0, 0.0, 0.0),
            layer: "F.Silkscreen".into(),
            font_size: (1.0, 1.0),
        });
        b.footprints.push(fp);
        VendorDialect::Huaqiu.prepare_board(&mut b);
        assert_eq!(b.paper, "A4");
        assert_eq!(b.footprints[0].fp_texts[0].layer, "F.SilkS");
        // 幂等（重复整备零变化）
        VendorDialect::Huaqiu.prepare_board(&mut b);
        assert_eq!(b.paper, "A4");
        assert_eq!(b.footprints[0].fp_texts[0].layer, "F.SilkS");
    }

    #[test]
    fn official_prepare_is_identity() {
        let mut b = crate::ir::board::Board::new();
        b.paper = "User 100 80".into();
        VendorDialect::Official.prepare_board(&mut b);
        assert_eq!(b.paper, "User 100 80");
    }
}
