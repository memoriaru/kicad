//! S-expression code generator for KiCad schematic files
//!
//! Generates `.kicad_sch` files from the IR (Intermediate Representation).

mod auto;
pub(crate) mod board_gen;
mod element;
mod graphic;
mod symbol;

use crate::error::Result;
use crate::ir::Instances;
use crate::ir::{
    Arc, Bus, BusEntry, Circle, Fill, FillType, GraphicElement, HorizontalAlign, Junction, Label,
    NoConnect, Pin, PinGraphic, PinShape, PinType, Polyline, Rectangle, Stroke, StrokeType, Symbol,
    SymbolInstance, Text, TextEffects, VerticalAlign, Wire,
};
use uuid::Uuid;

/// KiCad file format version
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KicadVersion {
    /// KiCad 7.x - version "20221219"
    V7,
    /// KiCad 8.x - version "20231120"
    V8,
    /// KiCad 9.x - version "20250114"
    V9,
    /// KiCad 10.x - version "20260306"
    #[default]
    V10,
}

impl KicadVersion {
    /// Get the version string for the schematic file
    pub fn version_string(&self) -> &'static str {
        match self {
            KicadVersion::V7 => "20221219",
            KicadVersion::V8 => "20231120",
            KicadVersion::V9 => "20250114",
            KicadVersion::V10 => "20260306",
        }
    }

    /// Get the generator_version string for the schematic file
    pub fn generator_version_string(&self) -> &'static str {
        match self {
            KicadVersion::V7 => "7.0",
            KicadVersion::V8 => "8.0",
            KicadVersion::V9 => "9.0",
            KicadVersion::V10 => "10.0",
        }
    }

    /// Try to detect version from a KiCad version string
    pub fn from_version_string(s: &str) -> Option<Self> {
        match s {
            "20221219" => Some(KicadVersion::V7),
            "20231120" => Some(KicadVersion::V8),
            "20250114" => Some(KicadVersion::V9),
            "20260306" => Some(KicadVersion::V10),
            // Pattern match: 20260307+ → V10 (future KiCad 10.x)
            _ if s.len() == 8 && s.chars().all(|c| c.is_ascii_digit()) => {
                let val: u32 = s.parse().ok()?;
                if val >= 20260306 {
                    Some(KicadVersion::V10)
                } else if val >= 20250101 {
                    Some(KicadVersion::V9)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Get the latest supported version (used as default for pure JSON5 → .kicad_sch)
    pub fn latest() -> Self {
        KicadVersion::V10
    }
}

/// S-expression generator configuration
#[derive(Debug, Clone)]
pub struct SexprConfig {
    /// Indentation string (default: "\t")
    pub indent: String,
    /// Whether to include UUIDs in output
    pub include_uuids: bool,
    /// Target KiCad version (None = auto-detect from input)
    pub kicad_version: Option<KicadVersion>,
    /// Auto-generate UUIDs if missing
    pub generate_uuids: bool,
    /// Insert PWR_FLAG symbols for nets with power_in but no power_out pins.
    /// Only applies to JSON5→S-expression generation (reverse direction is unaffected).
    /// This is a kicad-json5 extension not present in official KiCad output.
    pub insert_power_flags: bool,
}

impl Default for SexprConfig {
    fn default() -> Self {
        Self {
            indent: "\t".to_string(),
            include_uuids: true,
            kicad_version: None, // auto-detect
            generate_uuids: true,
            insert_power_flags: true,
        }
    }
}

/// S-expression generator for KiCad schematic files
pub struct SexprGenerator {
    config: SexprConfig,
    indent_level: usize,
    /// Effective version detected from input, used for version-specific output
    effective_version: KicadVersion,
}

/// Default symbol template kinds for auto-generating graphics
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum DefaultSymbolKind {
    Ic,
    Resistor,
    Capacitor,
    Inductor,
    Diode,
    Led,
    TwoPin,
    Connector,
}

impl SexprGenerator {
    /// Create a new generator with default config
    pub fn new() -> Self {
        Self {
            config: SexprConfig::default(),
            indent_level: 0,
            effective_version: KicadVersion::default(),
        }
    }

    /// Create a generator with custom config
    pub fn with_config(config: SexprConfig) -> Self {
        Self {
            config,
            indent_level: 0,
            effective_version: KicadVersion::latest(), // default, overridden in generate()
        }
    }

    /// Generate S-expression from IR Schematic
    pub fn generate(&mut self, schematic: &crate::ir::Schematic) -> Result<String> {
        // Clone and snap component positions to 1.27mm grid so all pin endpoints
        // land on KiCad's connection grid (eliminates endpoint_off_grid warnings).
        let mut schematic = schematic.clone();
        let snap = |v: f64| -> f64 { (v / 1.27).round() * 1.27 };
        for comp in &mut schematic.components {
            comp.position.0 = snap(comp.position.0);
            comp.position.1 = snap(comp.position.1);
        }
        for sheet in &mut schematic.sheets {
            sheet.position.0 = snap(sheet.position.0);
            sheet.position.1 = snap(sheet.position.1);
            sheet.size.0 = snap(sheet.size.0);
            sheet.size.1 = snap(sheet.size.1);
        }
        for label in &mut schematic.labels {
            label.position.0 = snap(label.position.0);
            label.position.1 = snap(label.position.1);
        }

        let mut output = String::new();

        output.push_str("(kicad_sch\n");

        self.indent_level = 1;

        // Version resolution: CLI override > auto-detect from input > default to latest
        let detected_version = KicadVersion::from_version_string(&schematic.metadata.version);
        self.effective_version = self
            .config
            .kicad_version
            .or(detected_version)
            .unwrap_or_else(KicadVersion::latest);
        let version = self.effective_version;

        // Version and generator
        self.write_line(
            &mut output,
            &format!("(version {})", version.version_string()),
        );
        self.write_line(&mut output, "(generator \"kicad-json5\")");

        // generator_version (always output, match the effective version)
        let gv = if self.config.kicad_version.is_some() {
            // CLI override: use version-appropriate generator_version
            version.generator_version_string().to_string()
        } else if let Some(ref gv) = schematic.metadata.generator_version {
            // Auto-detect: preserve original
            gv.clone()
        } else {
            version.generator_version_string().to_string()
        };
        self.write_line(&mut output, &format!("(generator_version \"{}\")", gv));

        // UUID
        if self.config.include_uuids {
            let uuid = if schematic.metadata.uuid.is_empty() {
                if self.config.generate_uuids {
                    Uuid::new_v4().to_string()
                } else {
                    String::new()
                }
            } else {
                schematic.metadata.uuid.clone()
            };
            if !uuid.is_empty() {
                self.write_line(&mut output, &format!("(uuid \"{}\")", uuid));
            }
        }

        // Paper
        self.generate_paper(&mut output, &schematic);

        // Title block
        self.generate_title_block(&mut output, &schematic);

        // Lib symbols
        if !schematic.lib_symbols.is_empty()
            || schematic
                .nets
                .iter()
                .any(|n| n.render == crate::ir::RenderHint::Power)
        {
            self.generate_lib_symbols(&mut output, &schematic.lib_symbols, &schematic.nets);
        }

        // Note: KiCad 7+ schematics don't use top-level (net ...) declarations.
        // Nets are represented by wires and labels instead.

        // Symbol instances (components)
        for component in &schematic.components {
            self.generate_symbol_instance(&mut output, component);
        }

        // Wires
        for wire in &schematic.wires {
            self.generate_wire(&mut output, wire);
        }

        // Auto-generate connections from net connectivity when no explicit wires exist.
        // Skip auto-wire only when user-provided non-hierarchical labels exist (to avoid conflicts).
        // Hierarchical labels are for cross-sheet connections and don't conflict with auto-wire.
        let has_user_labels = schematic
            .labels
            .iter()
            .any(|l| l.label_type != "hierarchical_label");
        if schematic.wires.is_empty() && !has_user_labels && !schematic.components.is_empty() {
            self.generate_auto_labels(&mut output, &schematic);
        }

        // Labels
        for label in &schematic.labels {
            self.generate_label(&mut output, label);
        }

        // Junctions
        for junction in &schematic.junctions {
            self.generate_junction(&mut output, junction);
        }

        // No-connects
        for nc in &schematic.no_connects {
            self.generate_no_connect(&mut output, nc);
        }

        // Buses
        for bus in &schematic.buses {
            self.generate_bus(&mut output, bus);
        }

        // Bus entries
        for entry in &schematic.bus_entries {
            self.generate_bus_entry(&mut output, entry);
        }

        // Sheets (hierarchical)
        for sheet in &schematic.sheets {
            self.generate_sheet(&mut output, sheet);
        }

        // For root-level sheets, place local labels at each sheet pin position
        // to create nets by name for sheet pin connectivity.
        // Pin positions are already in absolute coordinates.
        for sheet in &schematic.sheets {
            let distributed = Self::distribute_sheet_pins(sheet);
            for pin in &distributed {
                let (px, py, _pr) = pin.position;
                let default_effects = crate::ir::TextEffects::default();
                self.write_line(&mut output, "(label");
                self.indent_level += 1;
                self.write_line(
                    &mut output,
                    &format!("\"{}\"", Self::escape_string(&pin.name)),
                );
                self.write_line(&mut output, &Self::format_at(px, py, 0.0));
                self.generate_effects(&mut output, &default_effects);
                if self.config.include_uuids {
                    self.write_line(&mut output, &format!("(uuid \"{}\")", Self::new_uuid()));
                }
                self.indent_level -= 1;
                self.write_line(&mut output, ")");
            }
        }

        // v10: sheet_instances at file level
        if matches!(self.effective_version, KicadVersion::V10) {
            self.write_line(&mut output, "(sheet_instances");
            self.indent_level += 1;
            let path = if schematic.metadata.uuid.is_empty() {
                "/".to_string()
            } else {
                format!("/{}", schematic.metadata.uuid)
            };
            self.write_line(&mut output, &format!("(path \"{}\"", path));
            self.indent_level += 1;
            self.write_line(&mut output, "(page \"1\")");
            self.indent_level -= 1;
            self.write_line(&mut output, ")");
            self.indent_level -= 1;
            self.write_line(&mut output, ")");
            // v10: embedded_fonts at file level
            self.write_line(&mut output, "(embedded_fonts no)");
        }

        self.indent_level = 0;
        output.push_str(")\n");

        Ok(output)
    }

    // ============== Helper Methods ==============

    /// Get current indentation string
    fn indent(&self) -> String {
        self.config.indent.repeat(self.indent_level)
    }

    /// Write a line with proper indentation
    pub(super) fn write_line(&self, output: &mut String, content: &str) {
        output.push_str(&self.indent());
        output.push_str(content);
        output.push('\n');
    }

    /// Format a number, removing trailing zeros
    pub(super) fn format_number(n: f64) -> String {
        // Round to 6 decimal places to eliminate floating-point artifacts
        let n = (n * 1e6).round() / 1e6;
        if (n - n.round()).abs() < 1e-9 {
            format!("{}", n.round() as i64)
        } else {
            let s = format!("{}", n);
            let trimmed = s.trim_end_matches('0').trim_end_matches('.');
            trimmed.to_string()
        }
    }

    /// Format a point (xy x y)
    pub(super) fn format_xy(x: f64, y: f64) -> String {
        format!("(xy {} {})", Self::format_number(x), Self::format_number(y))
    }

    /// Format an at expression (at x y rotation)
    pub(super) fn format_at(x: f64, y: f64, rotation: f64) -> String {
        format!(
            "(at {} {} {})",
            Self::format_number(x),
            Self::format_number(y),
            Self::format_number(rotation)
        )
    }

    /// Escape special characters in a string for S-expression output
    pub(super) fn escape_string(s: &str) -> String {
        s.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .replace('\t', "\\t")
    }

    /// Generate a new UUID string
    pub(super) fn new_uuid() -> String {
        Uuid::new_v4().to_string()
    }

    /// Format boolean as yes/no
    fn format_bool(b: bool) -> &'static str {
        if b {
            "yes"
        } else {
            "no"
        }
    }

    // ============== Metadata Generation ==============

    fn generate_paper(&self, output: &mut String, schematic: &crate::ir::Schematic) {
        let paper = &schematic.metadata.paper;
        if let (Some(w), Some(h)) = (paper.width, paper.height) {
            self.write_line(
                output,
                &format!(
                    "(paper \"{}\" {} {})",
                    paper.size,
                    Self::format_number(w),
                    Self::format_number(h)
                ),
            );
        } else {
            self.write_line(output, &format!("(paper \"{}\")", paper.size));
        }
    }

    fn generate_title_block(&mut self, output: &mut String, schematic: &crate::ir::Schematic) {
        let tb = &schematic.metadata.title_block;
        let has_content = tb.title.is_some()
            || tb.date.is_some()
            || tb.rev.is_some()
            || tb.company.is_some()
            || !tb.comments.is_empty();

        if !has_content {
            return;
        }

        self.write_line(output, "(title_block");
        self.indent_level += 1;

        if let Some(title) = &tb.title {
            self.write_line(
                output,
                &format!("(title \"{}\")", Self::escape_string(title)),
            );
        }
        if let Some(date) = &tb.date {
            self.write_line(output, &format!("(date \"{}\")", date));
        }
        if let Some(rev) = &tb.rev {
            self.write_line(output, &format!("(rev \"{}\")", rev));
        }
        if let Some(company) = &tb.company {
            self.write_line(
                output,
                &format!("(company \"{}\")", Self::escape_string(company)),
            );
        }

        for (i, comment) in tb.comments.iter().enumerate() {
            self.write_line(
                output,
                &format!("(comment {} \"{}\")", i + 1, Self::escape_string(comment)),
            );
        }

        self.indent_level -= 1;
        self.write_line(output, ")");
    }

    // (moved to symbol.rs, graphic.rs, element.rs, auto.rs)
}

impl Default for SexprGenerator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Metadata, Paper, Schematic, TitleBlock};

    #[test]
    fn test_format_number() {
        assert_eq!(SexprGenerator::format_number(1.0), "1");
        assert_eq!(SexprGenerator::format_number(1.5), "1.5");
        assert_eq!(SexprGenerator::format_number(10.25), "10.25");
        assert_eq!(SexprGenerator::format_number(0.0), "0");
        assert_eq!(SexprGenerator::format_number(1.23456), "1.23456");
    }

    #[test]
    fn test_format_xy() {
        assert_eq!(SexprGenerator::format_xy(10.0, 20.0), "(xy 10 20)");
        assert_eq!(SexprGenerator::format_xy(10.5, 20.25), "(xy 10.5 20.25)");
    }

    #[test]
    fn test_format_at() {
        assert_eq!(SexprGenerator::format_at(10.0, 20.0, 0.0), "(at 10 20 0)");
        assert_eq!(SexprGenerator::format_at(10.0, 20.0, 90.0), "(at 10 20 90)");
    }

    #[test]
    fn test_escape_string() {
        assert_eq!(SexprGenerator::escape_string("hello"), "hello");
        assert_eq!(
            SexprGenerator::escape_string("say \"hi\""),
            "say \\\"hi\\\""
        );
        assert_eq!(
            SexprGenerator::escape_string("line1\nline2"),
            "line1\\nline2"
        );
    }

    #[test]
    fn test_kicad_version() {
        assert_eq!(KicadVersion::V7.version_string(), "20221219");
        assert_eq!(KicadVersion::V8.version_string(), "20231120");
    }

    #[test]
    fn test_generate_empty_schematic() {
        let mut schematic = Schematic::new();
        schematic.metadata = Metadata {
            uuid: "test-uuid-1234".to_string(),
            version: "20231120".to_string(),
            generator: "kicad-json5".to_string(),
            generator_version: None,
            paper: Paper {
                size: "A4".to_string(),
                width: None,
                height: None,
                portrait: false,
            },
            title_block: TitleBlock {
                title: Some("Test Schematic".to_string()),
                date: Some("2024-01-01".to_string()),
                rev: Some("1.0".to_string()),
                company: Some("Test Company".to_string()),
                comments: vec!["Comment 1".to_string()],
            },
        };

        let mut generator = SexprGenerator::new();
        let result = generator.generate(&schematic).unwrap();

        // Verify basic structure
        assert!(result.starts_with("(kicad_sch"));
        assert!(result.contains("(version 20231120)"));
        assert!(result.contains("(generator \"kicad-json5\")"));
        assert!(result.contains("(uuid \"test-uuid-1234\")"));
        assert!(result.contains("(paper \"A4\")"));
        assert!(result.contains("(title \"Test Schematic\")"));
        assert!(result.contains("(date \"2024-01-01\")"));
        assert!(result.contains("(rev \"1.0\")"));
        assert!(result.contains("(company \"Test Company\")"));
        assert!(result.ends_with(")\n"));
    }

    #[test]
    fn test_instance_reference_value_and_auto_wire_emitted() {
        // 回归锁(2026-11-12 mini 实验): ①json5 ref/value 必须发射为实例
        // Reference/Value property(缺 Reference → ERC "Symbol U?");
        // ②pin 带 net_name 解析出 net_id 后 auto-wire 必须真的画线(旧行为=静默零发射)
        let src = r#"{
            nets: [{ id: 1, name: "GND", type: "power", render: "global" },
                   { id: 2, name: "SIG", render: "wire" }],
            lib_symbols: [{ lib_id: "tbtest:R", reference: "R",
                            pins: [{ number: "1" }, { number: "2" }] },
                           { lib_id: "tbtest:C", reference: "C",
                             pins: [{ number: "1" }, { number: "2" }] }],
            components: [
                { lib_id: "tbtest:R", ref: "R1", value: "10k",
                  position: { x: 50.8, y: 50.8 },
                  pins: [{ number: "1", net_name: "GND" },
                         { number: "2", net_name: "SIG" }] },
                { lib_id: "tbtest:C", ref: "C1", value: "100n",
                  position: { x: 76.2, y: 50.8 },
                  pins: [{ number: "1", net_name: "SIG" },
                         { number: "2", net_name: "GND" }] },
            ],
        }"#;
        let sch = crate::parser::json5_parser::parse_json5(src).unwrap();
        let mut generator = SexprGenerator::new();
        let result = generator.generate(&sch).unwrap();
        // 实例三大字段全落 property(发射是多行形态: token 分行)
        assert!(result.contains("(property \"Reference\""), "{result}");
        assert!(result.contains("\"R1\""));
        assert!(result.contains("(property \"Value\""));
        assert!(result.contains("\"10k\""));
        // auto-wire 兑现: SIG 网络至少画出 wire/label
        assert!(result.contains("(wire"), "auto-wire 未画出 SIG 网络:\n{result}");
        assert!(result.contains("SIG"));
    }

    #[test]
    fn test_auto_wire_uses_explicit_graphics_pin_positions() {
        // 回归锁(2026-11-12 mini5 实验): lib_symbols 带显式 graphics pin 几何时,
        // auto-wire 的 compute_pin_positions 必须吃它 — 发射的 lib_symbol 正是这套
        // 位置; 旧行为=无视 graphics 走启发式/backfill ±5.81 → wire/label 端点与
        // pin 实际世界位置差 0.73mm → pin_not_connected×4 + label_dangling×3
        let src = r#"{
            nets: [{ id: 1, name: "GND", type: "power", render: "global" },
                   { id: 2, name: "SIG", render: "wire" }],
            lib_symbols: [{ lib_id: "tbtest:R", reference: "R",
                            pins: [{ number: "1" }, { number: "2" }],
                            graphics: [
                              { type: "pin", number: "1", name: "", at: [0.0, 5.08, 270], length: 2.54 },
                              { type: "pin", number: "2", name: "", at: [0.0, -5.08, 90], length: 2.54 }] }],
            components: [
                { lib_id: "tbtest:R", ref: "R1", value: "10k",
                  position: { x: 50.8, y: 50.8 },
                  pins: [{ number: "1", net_name: "GND" },
                         { number: "2", net_name: "SIG" }] },
                { lib_id: "tbtest:R", ref: "R2", value: "1k",
                  position: { x: 76.2, y: 50.8 },
                  pins: [{ number: "1", net_name: "SIG" },
                         { number: "2", net_name: "GND" }] },
            ],
        }"#;
        let sch = crate::parser::json5_parser::parse_json5(src).unwrap();
        let mut generator = SexprGenerator::new();
        let result = generator.generate(&sch).unwrap();
        // pin2 世界坐标 = cy − (−5.08) = 55.88 (symbol-local y-up 惯例):
        // SIG 的 wire/label 必须落在 55.88, 而不是启发式 5.81 口径的 56.61
        assert!(result.contains("55.88"), "auto-wire 未用 graphics 几何(端点应为 55.88):\n{result}");
        assert!(!result.contains("56.61"), "端点仍走 ±5.81 启发式(出现 56.61):\n{result}");
        // GND global 标签同理应落 45.72 (=50.8−5.08) 而非 44.99
        assert!(result.contains("45.72"), "global 标签未用 graphics 几何(应为 45.72)");
        assert!(!result.contains("44.99"), "标签仍走 ±5.81 启发式(出现 44.99)");
    }

    #[test]
    fn test_json5_graphics_body_rect_line_emitted() {
        // 回归锁(2026-11-12): json5 lib_symbols.graphics 旧版只解析 pin,
        // rect/line 体几何被静默 skip → 渲染出无体符号(只有引脚 stub).
        // 修复后: rect(at+size)/line(pts) 入 IR, symbol.rs 拆 unit 发射
        // (非 power 符号: 公共图形入 _0_1, pin 入 _1_1).
        let src = r#"{
            nets: [{ id: 1, name: "GND", type: "power", render: "global" }],
            lib_symbols: [{ lib_id: "tbtest:R", reference: "R",
                            pins: [{ number: "1" }, { number: "2" }],
                            graphics: [
                              { type: "rect", at: [-2.54, -3.81], size: [5.08, 7.62] },
                              { type: "line", pts: [[-1.27, 1.27], [1.27, -1.27]] },
                              { type: "pin", number: "1", name: "", at: [0.0, 5.08, 270], length: 2.54 },
                              { type: "pin", number: "2", name: "", at: [0.0, -5.08, 90], length: 2.54 }] }],
            components: [
                { lib_id: "tbtest:R", ref: "R1", value: "10k",
                  position: { x: 50.8, y: 50.8 },
                  pins: [{ number: "1", net_name: "GND" },
                         { number: "2", net_name: "GND" }] },
            ],
        }"#;
        let sch = crate::parser::json5_parser::parse_json5(src).unwrap();
        let sym = &sch.lib_symbols[0];
        let non_pin = sym.graphics.iter()
            .filter(|g| !matches!(g, crate::ir::GraphicElement::Pin(_))).count();
        assert_eq!(non_pin, 2, "json5 graphics 非 pin 体几何未入 IR(rect/line 被 skip)");
        let mut generator = SexprGenerator::new();
        let result = generator.generate(&sch).unwrap();
        assert!(result.contains("(rectangle"), "rectangle 未发射:\n{result}");
        assert!(result.contains("(start -2.54 -3.81)") && result.contains("(end 2.54 3.81)"),
                "rectangle start/end 口径不对(end 应为 at+size)");
        assert!(result.contains("(polyline"), "line(pts) 未发射为 polyline");
        // 非 power 有 pin 符号: 体图形入 _0_1, pin 入 _1_1
        assert!(result.contains("(symbol \"R_0_1\"") && result.contains("(symbol \"R_1_1\""),
                "unit 拆分缺失(_0_1 图形 / _1_1 pin)");
        let g0 = result.split("(symbol \"R_1_1\"").next().unwrap();
        assert!(g0.contains("(rectangle") && !g0.contains("(pin "),
                "_0_1 应只含图形不含 pin");
    }

    #[test]
    fn test_generate_wire() {
        let schematic = Schematic::new();

        let mut generator = SexprGenerator::new();
        let result = generator.generate(&schematic).unwrap();

        // Basic wire generation test (empty schematic should still produce valid output)
        assert!(result.contains("(kicad_sch"));
    }

    #[test]
    fn test_roundtrip_metadata() {
        // Create a schematic with known metadata
        let mut original = Schematic::new();
        original.metadata.uuid = "roundtrip-test-uuid".to_string();
        original.metadata.generator = "test-generator".to_string();
        original.metadata.title_block.title = Some("Roundtrip Test".to_string());

        // Generate S-expr
        let mut generator = SexprGenerator::new();
        let sexpr_output = generator.generate(&original).unwrap();

        // Parse the generated S-expr
        use crate::lexer::Lexer;
        use crate::parser::Parser;

        let lexer = Lexer::new(&sexpr_output);
        let mut parser = Parser::new(lexer);
        let parsed = parser.parse().unwrap();

        // Verify metadata matches
        assert_eq!(parsed.metadata.uuid, original.metadata.uuid);
        // Note: generator field is preserved through the roundtrip
        assert_eq!(
            parsed.metadata.title_block.title,
            original.metadata.title_block.title
        );
    }
}
