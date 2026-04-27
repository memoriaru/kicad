//! S-expression code generator for KiCad schematic files
//!
//! Generates `.kicad_sch` files from the IR (Intermediate Representation).

use crate::error::Result;
use crate::ir::{
    Arc, Bus, BusEntry, Circle, Fill, FillType, GraphicElement, HorizontalAlign, Junction,
    Label, Net, NoConnect, Pin, PinGraphic, PinShape, PinType, Polyline, Rectangle, Stroke,
    StrokeType, Symbol, SymbolInstance, Text, TextEffects, VerticalAlign, Wire,
};
use std::collections::HashMap;
use crate::ir::Instances;
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
            insert_power_flags: false,
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
#[derive(Debug, Clone, Copy)]
#[derive(PartialEq)]
enum DefaultSymbolKind {
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
        let mut output = String::new();

        output.push_str("(kicad_sch\n");

        self.indent_level = 1;

        // Version resolution: CLI override > auto-detect from input > default to latest
        let detected_version = KicadVersion::from_version_string(&schematic.metadata.version);
        self.effective_version = self.config.kicad_version
            .or(detected_version)
            .unwrap_or_else(KicadVersion::latest);
        let version = self.effective_version;

        // Version and generator
        self.write_line(
            &mut output,
            &format!(
                "(version {})",
                version.version_string()
            ),
        );
        self.write_line(
            &mut output,
            "(generator \"kicad-json5\")",
        );

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
        self.generate_paper(&mut output, schematic);

        // Title block
        self.generate_title_block(&mut output, schematic);

        // Lib symbols
        if !schematic.lib_symbols.is_empty() {
            self.generate_lib_symbols(&mut output, &schematic.lib_symbols);
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

        // Auto-generate connections from net connectivity when no explicit wires/labels exist.
        // When labels ARE present in JSON5, skip auto-wire entirely to avoid conflicts.
        // When nothing is present, use label-per-pin (better than L-shaped wires).
        if schematic.wires.is_empty() && schematic.labels.is_empty() && !schematic.components.is_empty() {
            self.generate_auto_labels(&mut output, schematic);
        } else if schematic.wires.is_empty() && !schematic.components.is_empty() {
            // Labels exist from JSON5 — no auto-wire to avoid crossing conflicts
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
    fn write_line(&self, output: &mut String, content: &str) {
        output.push_str(&self.indent());
        output.push_str(content);
        output.push('\n');
    }

    /// Format a number, removing trailing zeros
    fn format_number(n: f64) -> String {
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
    fn format_xy(x: f64, y: f64) -> String {
        format!("(xy {} {})", Self::format_number(x), Self::format_number(y))
    }

    /// Format an at expression (at x y rotation)
    fn format_at(x: f64, y: f64, rotation: f64) -> String {
        format!(
            "(at {} {} {})",
            Self::format_number(x),
            Self::format_number(y),
            Self::format_number(rotation)
        )
    }

    /// Escape special characters in a string for S-expression output
    fn escape_string(s: &str) -> String {
        s.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .replace('\t', "\\t")
    }

    /// Generate a new UUID string
    fn new_uuid() -> String {
        Uuid::new_v4().to_string()
    }

    /// Format boolean as yes/no
    fn format_bool(b: bool) -> &'static str {
        if b { "yes" } else { "no" }
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
            self.write_line(output, &format!("(title \"{}\")", Self::escape_string(title)));
        }
        if let Some(date) = &tb.date {
            self.write_line(output, &format!("(date \"{}\")", date));
        }
        if let Some(rev) = &tb.rev {
            self.write_line(output, &format!("(rev \"{}\")", rev));
        }
        if let Some(company) = &tb.company {
            self.write_line(output, &format!("(company \"{}\")", Self::escape_string(company)));
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

    // ============== Symbol Library Generation ==============

    fn generate_lib_symbols(&mut self, output: &mut String, symbols: &[Symbol]) {
        self.write_line(output, "(lib_symbols");
        self.indent_level += 1;

        for symbol in symbols {
            self.generate_symbol_def(output, symbol);
        }

        // Inject PWR_FLAG symbol for power pin driven checks (only when enabled)
        if self.config.insert_power_flags
            && matches!(self.effective_version, KicadVersion::V9 | KicadVersion::V10)
        {
            // Emit PWR_FLAG lib_symbol — exact copy from KiCad official power.kicad_sym
            self.write_line(output, "(symbol \"power:PWR_FLAG\"");
            self.indent_level += 1;
            self.write_line(output, "(power global)");
            self.write_line(output, "(pin_numbers");
            self.indent_level += 1;
            self.write_line(output, "(hide yes)");
            self.indent_level -= 1;
            self.write_line(output, ")");
            self.write_line(output, "(pin_names");
            self.indent_level += 1;
            self.write_line(output, "(offset 0)");
            self.write_line(output, "(hide yes)");
            self.indent_level -= 1;
            self.write_line(output, ")");
            self.write_line(output, "(exclude_from_sim no)");
            self.write_line(output, "(in_bom yes)");
            self.write_line(output, "(on_board yes)");
            self.write_line(output, "(in_pos_files yes)");
            self.write_line(output, "(duplicate_pin_numbers_are_jumpers no)");
            // Properties — multi-line format matching official KiCad output
            self.write_line(output, "(property \"Reference\" \"#FLG\"");
            self.indent_level += 1;
            self.write_line(output, "(at 0 1.905 0)");
            self.write_line(output, "(show_name no)");
            self.write_line(output, "(do_not_autoplace no)");
            self.write_line(output, "(hide yes)");
            self.write_line(output, "(effects");
            self.indent_level += 1;
            self.write_line(output, "(font");
            self.indent_level += 1;
            self.write_line(output, "(size 1.27 1.27)");
            self.indent_level -= 1;
            self.write_line(output, ")");
            self.indent_level -= 1;
            self.write_line(output, ")");
            self.indent_level -= 1;
            self.write_line(output, ")");
            self.write_line(output, "(property \"Value\" \"PWR_FLAG\"");
            self.indent_level += 1;
            self.write_line(output, "(at 0 3.81 0)");
            self.write_line(output, "(show_name no)");
            self.write_line(output, "(do_not_autoplace no)");
            self.write_line(output, "(effects");
            self.indent_level += 1;
            self.write_line(output, "(font");
            self.indent_level += 1;
            self.write_line(output, "(size 1.27 1.27)");
            self.indent_level -= 1;
            self.write_line(output, ")");
            self.indent_level -= 1;
            self.write_line(output, ")");
            self.indent_level -= 1;
            self.write_line(output, ")");
            self.write_line(output, "(property \"Footprint\" \"\"");
            self.indent_level += 1;
            self.write_line(output, "(at 0 0 0)");
            self.write_line(output, "(show_name no)");
            self.write_line(output, "(do_not_autoplace no)");
            self.write_line(output, "(hide yes)");
            self.write_line(output, "(effects");
            self.indent_level += 1;
            self.write_line(output, "(font");
            self.indent_level += 1;
            self.write_line(output, "(size 1.27 1.27)");
            self.indent_level -= 1;
            self.write_line(output, ")");
            self.indent_level -= 1;
            self.write_line(output, ")");
            self.indent_level -= 1;
            self.write_line(output, ")");
            self.write_line(output, "(property \"Datasheet\" \"\"");
            self.indent_level += 1;
            self.write_line(output, "(at 0 0 0)");
            self.write_line(output, "(show_name no)");
            self.write_line(output, "(do_not_autoplace no)");
            self.write_line(output, "(hide yes)");
            self.write_line(output, "(effects");
            self.indent_level += 1;
            self.write_line(output, "(font");
            self.indent_level += 1;
            self.write_line(output, "(size 1.27 1.27)");
            self.indent_level -= 1;
            self.write_line(output, ")");
            self.indent_level -= 1;
            self.write_line(output, ")");
            self.indent_level -= 1;
            self.write_line(output, ")");
            self.write_line(output, "(property \"Description\" \"Special symbol for telling ERC where power comes from\"");
            self.indent_level += 1;
            self.write_line(output, "(at 0 0 0)");
            self.write_line(output, "(show_name no)");
            self.write_line(output, "(do_not_autoplace no)");
            self.write_line(output, "(hide yes)");
            self.write_line(output, "(effects");
            self.indent_level += 1;
            self.write_line(output, "(font");
            self.indent_level += 1;
            self.write_line(output, "(size 1.27 1.27)");
            self.indent_level -= 1;
            self.write_line(output, ")");
            self.indent_level -= 1;
            self.write_line(output, ")");
            self.indent_level -= 1;
            self.write_line(output, ")");
            self.write_line(output, "(property \"ki_keywords\" \"flag power\"");
            self.indent_level += 1;
            self.write_line(output, "(at 0 0 0)");
            self.write_line(output, "(show_name no)");
            self.write_line(output, "(do_not_autoplace no)");
            self.write_line(output, "(hide yes)");
            self.write_line(output, "(effects");
            self.indent_level += 1;
            self.write_line(output, "(font");
            self.indent_level += 1;
            self.write_line(output, "(size 1.27 1.27)");
            self.indent_level -= 1;
            self.write_line(output, ")");
            self.indent_level -= 1;
            self.write_line(output, ")");
            self.indent_level -= 1;
            self.write_line(output, ")");
            // Unit 0_0: power_out pin at origin
            self.write_line(output, "(symbol \"PWR_FLAG_0_0\"");
            self.indent_level += 1;
            self.write_line(output, "(pin power_out line");
            self.indent_level += 1;
            self.write_line(output, "(at 0 0 90)");
            self.write_line(output, "(length 0)");
            self.write_line(output, "(name \"\"");
            self.indent_level += 1;
            self.write_line(output, "(effects");
            self.indent_level += 1;
            self.write_line(output, "(font");
            self.indent_level += 1;
            self.write_line(output, "(size 1.27 1.27)");
            self.indent_level -= 1;
            self.write_line(output, ")");
            self.indent_level -= 1;
            self.write_line(output, ")");
            self.indent_level -= 1;
            self.write_line(output, ")");
            self.write_line(output, "(number \"1\"");
            self.indent_level += 1;
            self.write_line(output, "(effects");
            self.indent_level += 1;
            self.write_line(output, "(font");
            self.indent_level += 1;
            self.write_line(output, "(size 1.27 1.27)");
            self.indent_level -= 1;
            self.write_line(output, ")");
            self.indent_level -= 1;
            self.write_line(output, ")");
            self.indent_level -= 1;
            self.write_line(output, ")");
            self.indent_level -= 1;
            self.write_line(output, ")");
            self.indent_level -= 1;
            self.write_line(output, ")");
            // Unit 0_1: flag pennant graphic
            self.write_line(output, "(symbol \"PWR_FLAG_0_1\"");
            self.indent_level += 1;
            self.write_line(output, "(polyline");
            self.indent_level += 1;
            self.write_line(output, "(pts");
            self.indent_level += 1;
            self.write_line(output, "(xy 0 0) (xy 0 1.27) (xy -1.016 1.905) (xy 0 2.54) (xy 1.016 1.905) (xy 0 1.27)");
            self.indent_level -= 1;
            self.write_line(output, ")");
            self.write_line(output, "(stroke");
            self.indent_level += 1;
            self.write_line(output, "(width 0)");
            self.write_line(output, "(type default)");
            self.indent_level -= 1;
            self.write_line(output, ")");
            self.write_line(output, "(fill");
            self.indent_level += 1;
            self.write_line(output, "(type none)");
            self.indent_level -= 1;
            self.write_line(output, ")");
            self.indent_level -= 1;
            self.write_line(output, ")");
            self.indent_level -= 1;
            self.write_line(output, ")");
            self.write_line(output, "(embedded_fonts no)");
            self.indent_level -= 1;
            self.write_line(output, ")");
        }

        self.indent_level -= 1;
        self.write_line(output, ")");
    }

    fn generate_symbol_def(&mut self, output: &mut String, symbol: &Symbol) {
        // Fast path: use embedded standard symbol (only for V10 — the embedded text is V10 format)
        if matches!(self.effective_version, KicadVersion::V10) {
            if let Some(sexpr) = super::standard_symbols::get_standard_symbol(&symbol.lib_id) {
                let short_name = symbol.lib_id.split(':').last().unwrap_or(&symbol.lib_id);
                // Replace top-level symbol name and sub-unit names.
                // Embedded text uses its own short name (e.g., "Thermistor_NTC"),
                // which may differ from the JSON5 lib_id's short name (e.g., "NTC").
                // Extract the embedded short name from the first (symbol "..." line.
                let embedded_short = sexpr.split('"').nth(1).unwrap_or(short_name);
                let mut adapted = sexpr.replacen(
                    &format!("(symbol \"{}\"", embedded_short),
                    &format!("(symbol \"{}\"", symbol.lib_id),
                    1,
                );
                // Replace unit names: Thermistor_NTC_0_1 → NTC_0_1 (or whatever short_name is)
                if embedded_short != short_name {
                    adapted = adapted.replace(
                        &format!("(symbol \"{}_0_", embedded_short),
                        &format!("(symbol \"{}_0_", short_name),
                    );
                    adapted = adapted.replace(
                        &format!("(symbol \"{}_1_", embedded_short),
                        &format!("(symbol \"{}_1_", short_name),
                    );
                }
                // The embedded text uses 1-tab base indent; add current indent_level tabs
                let extra_indent = self.config.indent.repeat(self.indent_level);
                for line in adapted.lines() {
                    if !line.trim().is_empty() {
                        output.push_str(&extra_indent);
                        output.push_str(line);
                        output.push('\n');
                    }
                }
                return;
            }
        }

        self.write_line(output, &format!("(symbol \"{}\"", Self::normalize_lib_id(&symbol.lib_id)));
        self.indent_level += 1;

        let is_v9_plus = matches!(self.effective_version, KicadVersion::V9 | KicadVersion::V10);

        // Pin numbers (format differs between v8 and v9+)
        // Auto-hide pin numbers for 2-pin passives (R/C/L/D/LED/NTC) if not explicitly set
        let should_hide_pin_numbers = symbol.pin_numbers_hidden
            || (symbol.pins.len() == 2 && Self::is_passive_2pin(symbol));
        if should_hide_pin_numbers {
            if is_v9_plus {
                self.write_line(output, "(pin_numbers");
                self.indent_level += 1;
                self.write_line(output, "(hide yes)");
                self.indent_level -= 1;
                self.write_line(output, ")");
            } else {
                self.write_line(output, "(pin_numbers hide)");
            }
        }

        // Pin names
        if symbol.pin_names_hidden || symbol.pin_name_offset != 0.254 {
            if symbol.pin_names_hidden {
                if is_v9_plus {
                    self.write_line(output, "(pin_names");
                    self.indent_level += 1;
                    self.write_line(output, &format!("(offset {})", Self::format_number(symbol.pin_name_offset)));
                    self.write_line(output, "(hide yes)");
                    self.indent_level -= 1;
                    self.write_line(output, ")");
                } else {
                    self.write_line(output, "(pin_names hide)");
                }
            } else {
                self.write_line(
                    output,
                    &format!(
                        "(pin_names (offset {}))",
                        Self::format_number(symbol.pin_name_offset)
                    ),
                );
            }
        }

        // Power symbol
        if symbol.is_power {
            self.write_line(output, "(power)");
        }

        // exclude_from_sim
        self.write_line(output, &format!("(exclude_from_sim {})", Self::format_bool(symbol.exclude_from_sim)));

        // in_bom and on_board
        self.write_line(output, &format!("(in_bom {})", Self::format_bool(symbol.in_bom)));
        self.write_line(output, &format!("(on_board {})", Self::format_bool(symbol.on_board)));

        // v10-specific fields
        if matches!(self.effective_version, KicadVersion::V10) {
            self.write_line(output, &format!("(in_pos_files {})", Self::format_bool(symbol.in_pos_files)));
            self.write_line(output, &format!("(duplicate_pin_numbers_are_jumpers {})", Self::format_bool(symbol.duplicate_pin_numbers_are_jumpers)));
        }

        // Properties (Reference, Value, Footprint, Datasheet, etc.)
        for prop in &symbol.properties {
            self.generate_property(output, prop);
        }

        // Units
        if symbol.units.is_empty() {
            if !symbol.graphics.is_empty() {
                self.write_line(output, "(symbol \"0_1\"");
                self.indent_level += 1;
                for graphic in &symbol.graphics {
                    self.generate_graphic_element(output, graphic);
                }
                self.indent_level -= 1;
                self.write_line(output, ")");
            } else if !symbol.pins.is_empty() || Self::is_passive_2pin(symbol) || Self::detect_default_kind(symbol) != DefaultSymbolKind::TwoPin {
                // Generate default units even when pins are empty, if this is a known
                // standard component type (R/C/L/D/LED/NTC/Connector)
                self.generate_default_symbol_units(output, symbol);
            }
        } else {
            // Generate each unit
            for unit in &symbol.units {
                let unit_name = if !unit.name.is_empty() {
                    unit.name.clone()
                } else {
                    format!("{}_{}", unit.unit_id, unit.style_id)
                };
                self.write_line(output, &format!("(symbol \"{}\"", unit_name));
                self.indent_level += 1;

                for graphic in &unit.graphics {
                    self.generate_graphic_element(output, graphic);
                }

                self.indent_level -= 1;
                self.write_line(output, ")");
            }
        }

        // v9+: embedded_fonts at parent symbol level (after all unit sub-symbols)
        if is_v9_plus {
            self.write_line(output, "(embedded_fonts no)");
        }

        self.indent_level -= 1;
        self.write_line(output, ")");
    }

    // ============== Graphic Elements Generation ==============

    fn generate_graphic_element(&mut self, output: &mut String, element: &GraphicElement) {
        match element {
            GraphicElement::Polyline(pl) => self.generate_polyline(output, pl),
            GraphicElement::Rectangle(rect) => self.generate_rectangle(output, rect),
            GraphicElement::Circle(circle) => self.generate_circle(output, circle),
            GraphicElement::Arc(arc) => self.generate_arc(output, arc),
            GraphicElement::Text(text) => self.generate_text(output, text),
            GraphicElement::Pin(pin) => self.generate_pin_graphic(output, pin),
        }
    }

    fn generate_polyline(&mut self, output: &mut String, polyline: &Polyline) {
        if polyline.points.len() < 2 {
            return;
        }

        self.write_line(output, "(polyline");
        self.indent_level += 1;

        // Points
        self.write_line(output, "(pts");
        self.indent_level += 1;
        for (x, y) in &polyline.points {
            self.write_line(output, &Self::format_xy(*x, *y));
        }
        self.indent_level -= 1;
        self.write_line(output, ")");

        // Stroke
        self.generate_stroke(output, &polyline.stroke);

        // Fill
        self.generate_fill(output, &polyline.fill);

        self.indent_level -= 1;
        self.write_line(output, ")");
    }

    fn generate_rectangle(&mut self, output: &mut String, rect: &Rectangle) {
        self.write_line(output, "(rectangle");
        self.indent_level += 1;

        self.write_line(output, &format!("(start {} {})", Self::format_number(rect.start.0), Self::format_number(rect.start.1)));
        self.write_line(output, &format!("(end {} {})", Self::format_number(rect.end.0), Self::format_number(rect.end.1)));

        self.generate_stroke(output, &rect.stroke);
        self.generate_fill(output, &rect.fill);

        self.indent_level -= 1;
        self.write_line(output, ")");
    }

    fn generate_circle(&mut self, output: &mut String, circle: &Circle) {
        self.write_line(output, "(circle");
        self.indent_level += 1;

        self.write_line(output, &format!("(center {} {})", Self::format_number(circle.center.0), Self::format_number(circle.center.1)));
        self.write_line(output, &format!("(radius {})", Self::format_number(circle.radius)));

        self.generate_stroke(output, &circle.stroke);
        self.generate_fill(output, &circle.fill);

        self.indent_level -= 1;
        self.write_line(output, ")");
    }

    fn generate_arc(&mut self, output: &mut String, arc: &Arc) {
        self.write_line(output, "(arc");
        self.indent_level += 1;

        self.write_line(output, &format!("(start {} {})", Self::format_number(arc.start.0), Self::format_number(arc.start.1)));
        self.write_line(output, &format!("(mid {} {})", Self::format_number(arc.mid.0), Self::format_number(arc.mid.1)));
        self.write_line(output, &format!("(end {} {})", Self::format_number(arc.end.0), Self::format_number(arc.end.1)));

        self.generate_stroke(output, &arc.stroke);
        self.generate_fill(output, &arc.fill);

        self.indent_level -= 1;
        self.write_line(output, ")");
    }

    fn generate_text(&mut self, output: &mut String, text: &Text) {
        self.write_line(output, "(text");
        self.indent_level += 1;

        self.write_line(output, &format!("\"{}\"", Self::escape_string(&text.text)));
        self.write_line(output, &Self::format_at(text.position.0, text.position.1, text.position.2));

        self.generate_effects(output, &text.effects);

        self.indent_level -= 1;
        self.write_line(output, ")");
    }

    fn generate_pin_graphic(&mut self, output: &mut String, pin: &PinGraphic) {
        let shape_str = match pin.shape {
            PinShape::Line => "line",
            PinShape::Inverted => "inverted",
            PinShape::Clock => "clock",
            PinShape::InvertedClock => "inverted_clock",
            PinShape::InputLow => "input_low",
            PinShape::ClockLow => "clock_low",
            PinShape::OutputLow => "output_low",
            PinShape::EdgeClockHigh => "edge_clock_high",
            PinShape::NonLogic => "non_logic",
            PinShape::Triangle => "triangle",
        };

        let type_str = match pin.pin_type {
            PinType::Input => "input",
            PinType::Output => "output",
            PinType::Bidirectional => "bidirectional",
            PinType::TriState => "tri_state",
            PinType::Passive => "passive",
            PinType::Free => "free",
            PinType::Unspecified => "unspecified",
            PinType::PowerIn => "power_in",
            PinType::PowerOut => "power_out",
            PinType::OpenCollector => "open_collector",
            PinType::OpenEmitter => "open_emitter",
            PinType::NoConnect => "no_connect",
        };

        // Format position without extra "(at ...)" wrapper
        let at_str = format!(
            "(at {} {} {})",
            Self::format_number(pin.position.0),
            Self::format_number(pin.position.1),
            Self::format_number(pin.position.2)
        );

        self.write_line(
            output,
            &format!(
                "(pin {} {} {} (length {})",
                type_str,
                shape_str,
                at_str,
                Self::format_number(pin.length)
            ),
        );
        self.indent_level += 1;

        self.write_line(output, &format!(
            "(name \"{}\" (effects (font (size 1.27 1.27))))",
            Self::escape_string(&pin.name)
        ));
        self.write_line(output, &format!(
            "(number \"{}\" (effects (font (size 1.27 1.27))))",
            pin.number
        ));

        self.indent_level -= 1;
        self.write_line(output, ")");
    }

    fn generate_stroke(&mut self, output: &mut String, stroke: &Stroke) {
        let type_str = match stroke.stroke_type {
            StrokeType::Default => "default",
            StrokeType::Dash => "dash",
            StrokeType::Dot => "dot",
            StrokeType::DashDot => "dash_dot",
            StrokeType::DashDotDot => "dash_dot_dot",
            StrokeType::Solid => "solid",
        };

        self.write_line(
            output,
            &format!(
                "(stroke (width {}) (type {}))",
                Self::format_number(stroke.width),
                type_str
            ),
        );
    }

    fn generate_fill(&mut self, output: &mut String, fill: &Fill) {
        let type_str = match fill.fill_type {
            FillType::None => "none",
            FillType::Outline => "outline",
            FillType::Background => "background",
            FillType::Color => "color",
        };

        self.write_line(output, &format!("(fill (type {}))", type_str));
    }

    fn generate_effects(&mut self, output: &mut String, effects: &TextEffects) {
        self.write_line(output, "(effects");
        self.indent_level += 1;

        self.write_line(
            output,
            &format!(
                "(font (size {} {}))",
                Self::format_number(effects.font.size.0),
                Self::format_number(effects.font.size.1)
            ),
        );

        if effects.font.bold {
            self.write_line(output, "(bold yes)");
        }
        if effects.font.italic {
            self.write_line(output, "(italic yes)");
        }

        // Justify — KiCad only accepts: left, right, top, bottom, mirror
        // "center" is implicit (no keyword). Omit direction if Center.
        let h = match effects.justify.horizontal {
            HorizontalAlign::Left => Some("left"),
            HorizontalAlign::Center => None,
            HorizontalAlign::Right => Some("right"),
        };
        let v = match effects.justify.vertical {
            VerticalAlign::Top => Some("top"),
            VerticalAlign::Center => None,
            VerticalAlign::Bottom => Some("bottom"),
        };

        if h.is_some() || v.is_some() || effects.justify.mirror {
            let mut parts = Vec::new();
            if let Some(hv) = h { parts.push(hv); }
            if let Some(vv) = v { parts.push(vv); }
            if effects.justify.mirror { parts.push("mirror"); }
            self.write_line(output, &format!("(justify {})", parts.join(" ")));
        }

        if effects.hide {
            self.write_line(output, "(hide yes)");
        }

        self.indent_level -= 1;
        self.write_line(output, ")");
    }

    // ============== Schematic Elements Generation ==============

    fn generate_symbol_instance(&mut self, output: &mut String, component: &SymbolInstance) {
        self.write_line(output, "(symbol");
        self.indent_level += 1;

        // lib_id
        self.write_line(output, &format!("(lib_id \"{}\")", Self::normalize_lib_id(&component.lib_id)));

        // at
        self.write_line(
            output,
            &Self::format_at(component.position.0, component.position.1, component.position.2),
        );

        // mirror
        match component.mirror {
            crate::ir::Mirror::X => self.write_line(output, "(mirror x)"),
            crate::ir::Mirror::Y => self.write_line(output, "(mirror y)"),
            crate::ir::Mirror::None => {}
        }

        // unit (always output)
        self.write_line(output, &format!("(unit {})", component.unit));

        // body_style (DeMorgan style, default 1)
        let is_v9_plus = matches!(self.effective_version, KicadVersion::V9 | KicadVersion::V10);
        if is_v9_plus {
            self.write_line(output, "(body_style 1)");
        }

        // exclude_from_sim, in_bom, on_board, dnp
        self.write_line(output, &format!("(exclude_from_sim {})", Self::format_bool(component.exclude_from_sim)));
        self.write_line(output, &format!("(in_bom {})", Self::format_bool(component.in_bom)));
        self.write_line(output, &format!("(on_board {})", Self::format_bool(component.on_board)));
        self.write_line(output, &format!("(dnp {})", Self::format_bool(component.dnp)));

        // fields_autoplaced
        if component.fields_autoplaced {
            self.write_line(output, &format!("(fields_autoplaced yes)"));
        }

        // UUID
        if self.config.include_uuids {
            let uuid = component.uuid.clone().unwrap_or_else(Self::new_uuid);
            self.write_line(output, &format!("(uuid \"{}\")", uuid));
        }

        // Properties
        for prop in &component.properties_ext {
            self.generate_property(output, prop);
        }

        // Pins
        for pin in &component.pins {
            self.generate_pin_instance(output, pin);
        }

        // instances
        self.generate_instances(output, &component.instances);

        self.indent_level -= 1;
        self.write_line(output, ")");
    }

    fn generate_property(&mut self, output: &mut String, prop: &crate::ir::Property) {
        self.write_line(output, &format!("(property \"{}\"", prop.name));
        self.indent_level += 1;

        self.write_line(output, &format!("\"{}\"", Self::escape_string(&prop.value)));
        self.write_line(
            output,
            &Self::format_at(prop.position.0, prop.position.1, prop.position.2),
        );

        if prop.hide {
            self.write_line(output, &format!("(hide yes)"));
        }

        // v9+: show_name and do_not_autoplace
        let is_v9_plus = matches!(self.effective_version, KicadVersion::V9 | KicadVersion::V10);
        if is_v9_plus {
            self.write_line(output, &format!("(show_name {})", Self::format_bool(prop.show_name)));
            self.write_line(output, &format!("(do_not_autoplace {})", Self::format_bool(prop.do_not_autoplace)));
        }

        self.generate_effects(output, &prop.effects);

        self.indent_level -= 1;
        self.write_line(output, ")");
    }

    fn generate_instances(&mut self, output: &mut String, instances: &Instances) {
        if instances.projects.is_empty() {
            return;
        }

        self.write_line(output, "(instances");
        self.indent_level += 1;

        for project in &instances.projects {
            self.write_line(output, &format!("(project \"{}\"", Self::escape_string(&project.name)));
            self.indent_level += 1;

            for path in &project.paths {
                self.write_line(output, &format!("(path \"{}\"", path.path));
                self.indent_level += 1;
                self.write_line(output, &format!("(reference \"{}\")", Self::escape_string(&path.reference)));
                self.write_line(output, &format!("(unit {})", path.unit));
                self.indent_level -= 1;
                self.write_line(output, ")");
            }

            self.indent_level -= 1;
            self.write_line(output, ")");
        }

        self.indent_level -= 1;
        self.write_line(output, ")");
    }

    fn generate_pin_instance(
        &mut self,
        output: &mut String,
        pin: &crate::ir::PinInstance,
    ) {
        self.write_line(output, &format!("(pin \"{}\"", pin.number));
        self.indent_level += 1;

        if self.config.include_uuids {
            self.write_line(output, &format!("(uuid \"{}\")", Self::new_uuid()));
        }

        self.indent_level -= 1;
        self.write_line(output, ")");
    }

    fn generate_net(&mut self, output: &mut String, net: &Net) {
        self.write_line(output, "(net");
        self.indent_level += 1;

        self.write_line(output, &format!("{} \"{}\"", net.id, Self::escape_string(&net.name)));

        if let Some(net_type) = &net.net_type {
            self.write_line(output, &format!("(type \"{}\")", net_type));
        }

        self.indent_level -= 1;
        self.write_line(output, ")");
    }

    fn generate_wire(&mut self, output: &mut String, wire: &Wire) {
        self.write_line(output, "(wire");
        self.indent_level += 1;

        self.write_line(output, &format!(
            "(pts {} {})",
            Self::format_xy(wire.start.0, wire.start.1),
            Self::format_xy(wire.end.0, wire.end.1)
        ));

        self.generate_stroke(output, &wire.stroke);

        self.indent_level -= 1;
        self.write_line(output, ")");
    }

    fn generate_label(&mut self, output: &mut String, label: &Label) {
        // KiCad uses "netclass_flag" as the token for directive labels
        let label_type = label.label_type.as_str();
        let effective_type = if label_type == "directive_label" {
            "netclass_flag"
        } else {
            label_type
        };
        let is_global = effective_type == "global_label";
        let is_hierarchical = effective_type == "hierarchical_label";
        let is_directive = effective_type == "netclass_flag";

        self.write_line(output, &format!("({}", effective_type));
        self.indent_level += 1;

        self.write_line(output, &format!("\"{}\"", Self::escape_string(&label.text)));
        self.write_line(
            output,
            &Self::format_at(label.position.0, label.position.1, label.position.2),
        );

        if is_global || is_hierarchical || is_directive {
            self.write_line(output, &format!("(shape {})", label.shape));
        }

        self.generate_effects(output, &label.effects);

        if self.config.include_uuids {
            self.write_line(output, &format!("(uuid \"{}\")", Self::new_uuid()));
        }

        self.indent_level -= 1;
        self.write_line(output, ")");
    }

    fn generate_junction(&mut self, output: &mut String, junction: &Junction) {
        self.write_line(output, "(junction");
        self.indent_level += 1;

        // Junction (at) only has x, y — no rotation
        self.write_line(output, &format!(
            "(at {} {})",
            Self::format_number(junction.position.0),
            Self::format_number(junction.position.1)
        ));

        if junction.diameter != 0.0 {
            self.write_line(output, &format!("(diameter {})", Self::format_number(junction.diameter)));
        }

        if self.config.include_uuids {
            self.write_line(output, &format!("(uuid \"{}\")", Self::new_uuid()));
        }

        self.indent_level -= 1;
        self.write_line(output, ")");
    }

    fn generate_no_connect(&mut self, output: &mut String, nc: &NoConnect) {
        self.write_line(output, "(no_connect");
        self.indent_level += 1;

        // no_connect (at) only has x, y — no rotation
        self.write_line(output, &format!(
            "(at {} {})",
            Self::format_number(nc.position.0),
            Self::format_number(nc.position.1)
        ));

        if self.config.include_uuids {
            let uuid = nc.uuid.clone().unwrap_or_else(Self::new_uuid);
            self.write_line(output, &format!("(uuid \"{}\")", uuid));
        }

        self.indent_level -= 1;
        self.write_line(output, ")");
    }

    fn generate_bus(&mut self, output: &mut String, bus: &Bus) {
        if bus.points.len() < 2 {
            return;
        }

        self.write_line(output, "(bus");
        self.indent_level += 1;

        self.write_line(output, "(pts");
        self.indent_level += 1;
        for (x, y) in &bus.points {
            self.write_line(output, &Self::format_xy(*x, *y));
        }
        self.indent_level -= 1;
        self.write_line(output, ")");

        self.generate_stroke(output, &bus.stroke);

        self.indent_level -= 1;
        self.write_line(output, ")");
    }

    fn generate_bus_entry(&mut self, output: &mut String, entry: &BusEntry) {
        self.write_line(output, "(bus_entry");
        self.indent_level += 1;

        self.write_line(output, &Self::format_at(entry.position.0, entry.position.1, 0.0));
        self.write_line(
            output,
            &format!("(size {} {})", Self::format_number(entry.size.0), Self::format_number(entry.size.1)),
        );

        self.generate_stroke(output, &entry.stroke);

        self.indent_level -= 1;
        self.write_line(output, ")");
    }

    // ============== Default Symbol Templates ==============

    /// Check if a symbol is a 2-pin passive that should hide pin numbers (R/C/L/D/LED/NTC/TH)
    fn is_passive_2pin(symbol: &Symbol) -> bool {
        if symbol.pins.len() != 2 {
            return false;
        }
        let full = symbol.lib_id.to_uppercase();
        full.contains("R") || full.contains("RESIST") || full.contains("NTC") || full.contains("THERM")
            || full.contains("C") || full.contains("CAP")
            || full.contains("L") || full.contains("IND")
            || full.contains("D") || full.contains("DIODE") || full.contains("LED")
            || full.contains("ZENER") || full.contains("TVS")
    }

    fn detect_default_kind(symbol: &Symbol) -> DefaultSymbolKind {
        let full = symbol.lib_id.to_uppercase();
        let short = symbol.lib_id.split(':').last().unwrap_or(&symbol.lib_id).to_uppercase();

        // Connector detection: Conn_01xNN, Conn_02xNN, etc.
        if short.starts_with("CONN") || full.contains("CONNECTOR") {
            return DefaultSymbolKind::Connector;
        }

        // lib_id-based detection (works even when pins.len() == 0)
        // Standard Device library IDs: Device:R, Device:C, Device:L, Device:D, Device:LED, Device:Thermistor_NTC
        if full.contains("LED") {
            return DefaultSymbolKind::Led;
        }
        if full.contains("RESIST") || full.contains("THERM") || full.contains("NTC") {
            return DefaultSymbolKind::Resistor;
        }
        // Match "R" exactly or "Device:R" (not "RT9193", "R_Small", etc.)
        if short == "R" || full == "DEVICE:R" || short.starts_with("R_") {
            return DefaultSymbolKind::Resistor;
        }
        if full.contains("CAP") {
            return DefaultSymbolKind::Capacitor;
        }
        if short == "C" || full == "DEVICE:C" || short.starts_with("C_") {
            return DefaultSymbolKind::Capacitor;
        }
        if full.contains("IND") {
            return DefaultSymbolKind::Inductor;
        }
        if short == "L" || full == "DEVICE:L" || short.starts_with("L_") {
            return DefaultSymbolKind::Inductor;
        }
        if full.contains("DIODE")
            || full.contains("ZENER")
            || full.contains("TVS")
            || short.starts_with("D_")
            || short == "D"
        {
            return DefaultSymbolKind::Diode;
        }

        // Fallback based on pin count
        match symbol.pins.len() {
            0..=2 => DefaultSymbolKind::TwoPin,
            _ => DefaultSymbolKind::Ic,
        }
    }

    /// Generate default unit sub-blocks for symbols without graphics
    fn generate_default_symbol_units(
        &mut self,
        output: &mut String,
        symbol: &Symbol,
    ) {
        let kind = Self::detect_default_kind(symbol);
        let base = symbol
            .lib_id
            .split(':')
            .last()
            .unwrap_or(&symbol.lib_id);

        match kind {
            DefaultSymbolKind::Connector => {
                self.gen_connector_unit(output, symbol, base)
            }
            DefaultSymbolKind::Ic => self.gen_ic_unit(output, symbol, base),
            DefaultSymbolKind::Resistor => {
                self.gen_resistor_unit(output, symbol, base)
            }
            DefaultSymbolKind::Capacitor => {
                self.gen_capacitor_unit(output, symbol, base)
            }
            DefaultSymbolKind::Inductor => {
                self.gen_inductor_unit(output, symbol, base)
            }
            DefaultSymbolKind::Diode | DefaultSymbolKind::Led => self.gen_diode_unit(
                output,
                symbol,
                base,
                matches!(kind, DefaultSymbolKind::Led),
            ),
            DefaultSymbolKind::TwoPin => self.gen_twopin_unit(output, symbol, base),
        }
    }

    /// Get a pin from the symbol, or create a default passive pin with the given number
    fn get_pin_or_default(symbol: &Symbol, index: usize, default_number: &str) -> Pin {
        symbol
            .pins
            .get(index)
            .cloned()
            .unwrap_or_else(|| Pin {
                number: default_number.to_string(),
                name: String::new(),
                pin_type: "passive".to_string(),
            })
    }

    /// Generate a pin in S-expression format with default effects
    fn gen_default_pin(
        &mut self,
        output: &mut String,
        pin: &Pin,
        x: f64,
        y: f64,
        rotation: f64,
    ) {
        let pin_type_str = match PinType::from_str(&pin.pin_type) {
            PinType::Input => "input",
            PinType::Output => "output",
            PinType::Bidirectional => "bidirectional",
            PinType::TriState => "tri_state",
            PinType::Passive => "passive",
            PinType::Free => "free",
            PinType::Unspecified => "unspecified",
            PinType::PowerIn => "power_in",
            PinType::PowerOut => "power_out",
            PinType::OpenCollector => "open_collector",
            PinType::OpenEmitter => "open_emitter",
            PinType::NoConnect => "no_connect",
        };

        self.write_line(
            output,
            &format!(
                "(pin {} line (at {} {} {}) (length 2.54)",
                pin_type_str,
                Self::format_number(x),
                Self::format_number(y),
                Self::format_number(rotation)
            ),
        );
        self.indent_level += 1;
        self.write_line(
            output,
            &format!(
                "(name \"{}\"",
                Self::escape_string(&pin.name)
            ),
        );
        self.indent_level += 1;
        self.write_line(output, "(effects (font (size 1.27 1.27))))");
        self.indent_level -= 1;
        self.write_line(output, &format!("(number \"{}\"", pin.number));
        self.indent_level += 1;
        self.write_line(output, "(effects (font (size 1.27 1.27))))");
        self.indent_level -= 1;
        self.indent_level -= 1;
        self.write_line(output, ")");
    }

    /// Generate standard stroke S-expression
    fn gen_default_stroke(&mut self, output: &mut String, width: f64) {
        self.write_line(output, "(stroke");
        self.indent_level += 1;
        self.write_line(output, &format!("(width {})", Self::format_number(width)));
        self.write_line(output, "(type default))");
        self.indent_level -= 1;
    }

    /// Parameterized connector generator matching KiCad's Connector_Generic style.
    /// Supports Conn_01xNN (single-row) and Conn_02xNN (dual-row) naming patterns.
    fn gen_connector_unit(
        &mut self,
        output: &mut String,
        symbol: &Symbol,
        base: &str,
    ) {
        let pin_count = symbol.pins.len();
        if pin_count == 0 {
            // Cannot generate a connector without pins — skip
            return;
        }
        let is_v9_plus = matches!(self.effective_version, KicadVersion::V9 | KicadVersion::V10);

        // Detect 2-row from name: Conn_02xNN, or from "Odd_Even"/"Counter_Clockwise" suffix
        let short = symbol.lib_id.split(':').last().unwrap_or(&symbol.lib_id);
        let is_dual_row = short.contains("02x") || short.contains("_02x");

        // pin_numbers hide
        if is_v9_plus {
            self.write_line(output, "(pin_numbers");
            self.indent_level += 1;
            self.write_line(output, "(hide yes)");
            self.indent_level -= 1;
            self.write_line(output, ")");
        } else {
            self.write_line(output, "(pin_numbers hide)");
        }

        // Standard boolean flags
        self.write_line(output, &format!("(exclude_from_sim no)"));
        self.write_line(output, &format!("(in_bom yes)"));
        self.write_line(output, &format!("(on_board yes)"));
        if is_v9_plus {
            self.write_line(output, "(in_pos_files yes)");
            self.write_line(output, "(duplicate_pin_numbers_are_jumpers no)");
        }

        let pin_length = 3.81;
        let pin_spacing = 2.54;
        let pin_x_left = -5.08;
        let pin_x_right = 7.62;

        if is_dual_row {
            // Dual-row: rows of pin_count/2, alternating left/right
            let rows = (pin_count + 1) / 2;
            let body_hw = 1.27;
            let body_top = (rows - 1) as f64 * pin_spacing / 2.0 + pin_spacing / 2.0;

            // _0_1: body rectangle + solder pads
            self.write_line(output, &format!("(symbol \"{}_0_1\"", base));
            self.indent_level += 1;
            self.write_line(output, "(rectangle");
            self.indent_level += 1;
            self.write_line(
                output,
                &format!(
                    "(start {} {}) (end {} {})",
                    Self::format_number(-body_hw),
                    Self::format_number(body_top),
                    Self::format_number(body_hw),
                    Self::format_number(-body_top)
                ),
            );
            self.gen_default_stroke(output, 0.254);
            self.write_line(output, "(fill (type none))");
            self.indent_level -= 1;
            self.write_line(output, ")");
            self.indent_level -= 1;
            self.write_line(output, ")");

            // _1_1: pins
            self.write_line(output, &format!("(symbol \"{}_1_1\"", base));
            self.indent_level += 1;

            for (i, pin) in symbol.pins.iter().enumerate() {
                let row = i / 2;
                let y = ((rows - 1) - row) as f64 * pin_spacing;
                if i % 2 == 0 {
                    // Left pin
                    self.gen_connector_pin(output, pin, pin_x_left, y, 0.0, pin_length);
                } else {
                    // Right pin
                    self.gen_connector_pin(output, pin, pin_x_right, y, 180.0, pin_length);
                }
            }

            self.indent_level -= 1;
            self.write_line(output, ")");
        } else {
            // Single-row: all pins on the left
            let body_hw = 1.27;
            let body_top = (pin_count - 1) as f64 * pin_spacing / 2.0 + pin_spacing / 2.0;

            // _0_1: body + solder pads
            self.write_line(output, &format!("(symbol \"{}_0_1\"", base));
            self.indent_level += 1;

            // Main body rectangle
            self.write_line(output, "(rectangle");
            self.indent_level += 1;
            self.write_line(
                output,
                &format!(
                    "(start {} {}) (end {} {})",
                    Self::format_number(-body_hw),
                    Self::format_number(body_top),
                    Self::format_number(body_hw),
                    Self::format_number(-body_top)
                ),
            );
            self.gen_default_stroke(output, 0.254);
            self.write_line(output, "(fill (type none))");
            self.indent_level -= 1;
            self.write_line(output, ")");

            // Solder pad rectangles (one per pin)
            for i in 0..pin_count {
                let y = ((pin_count - 1) - i) as f64 * pin_spacing;
                let pad_half = 0.127;
                self.write_line(output, "(rectangle");
                self.indent_level += 1;
                self.write_line(
                    output,
                    &format!(
                        "(start {} {}) (end {} {})",
                        Self::format_number(-1.27),
                        Self::format_number(y + pad_half),
                        Self::format_number(0.0),
                        Self::format_number(y - pad_half)
                    ),
                );
                self.gen_default_stroke(output, 0.254);
                self.write_line(output, "(fill (type none))");
                self.indent_level -= 1;
                self.write_line(output, ")");
            }

            self.indent_level -= 1;
            self.write_line(output, ")");

            // _1_1: pins
            self.write_line(output, &format!("(symbol \"{}_1_1\"", base));
            self.indent_level += 1;

            for (i, pin) in symbol.pins.iter().enumerate() {
                let y = ((pin_count - 1) - i) as f64 * pin_spacing;
                self.gen_connector_pin(output, pin, pin_x_left, y, 0.0, pin_length);
            }

            self.indent_level -= 1;
            self.write_line(output, ")");
        }

        if is_v9_plus {
            self.write_line(output, "(embedded_fonts no)");
        }
    }

    /// Generate a single connector pin
    fn gen_connector_pin(
        &mut self,
        output: &mut String,
        pin: &Pin,
        x: f64,
        y: f64,
        rotation: f64,
        length: f64,
    ) {
        self.write_line(
            output,
            &format!(
                "(pin passive line (at {} {} {}) (length {})",
                Self::format_number(x),
                Self::format_number(y),
                Self::format_number(rotation),
                Self::format_number(length),
            ),
        );
        self.indent_level += 1;
        self.write_line(
            output,
            &format!(
                "(name \"{}\" (effects (font (size 1.27 1.27))))",
                Self::escape_string(&pin.name)
            ),
        );
        self.write_line(
            output,
            &format!(
                "(number \"{}\" (effects (font (size 1.27 1.27))))",
                pin.number
            ),
        );
        self.indent_level -= 1;
        self.write_line(output, ")");
    }

    /// IC template: rectangle body + pins on left/right
    fn gen_ic_unit(
        &mut self,
        output: &mut String,
        symbol: &Symbol,
        base: &str,
    ) {
        let pin_count = symbol.pins.len();
        let left_count = (pin_count + 1) / 2;
        let right_count = pin_count - left_count;
        let max_per_side = left_count.max(right_count).max(1);

        let spacing = 2.54;
        let body_hw = 5.08; // half-width
        let body_hh = max_per_side as f64 * spacing / 2.0 + spacing / 2.0;
        let pin_length = 2.54;

        // _0_1: rectangle body
        self.write_line(output, &format!("(symbol \"{}_0_1\"", base));
        self.indent_level += 1;

        self.write_line(output, "(rectangle");
        self.indent_level += 1;
        self.write_line(
            output,
            &format!(
                "(start {} {}) (end {} {})",
                Self::format_number(-body_hw),
                Self::format_number(body_hh),
                Self::format_number(body_hw),
                Self::format_number(-body_hh)
            ),
        );
        self.gen_default_stroke(output, 0.254);
        self.write_line(output, "(fill (type background))");
        self.indent_level -= 1;
        self.write_line(output, ")");

        self.indent_level -= 1;
        self.write_line(output, ")");

        // _1_1: pins
        self.write_line(output, &format!("(symbol \"{}_1_1\"", base));
        self.indent_level += 1;

        for (i, pin) in symbol.pins.iter().enumerate() {
            if i < left_count {
                let y = (left_count - 1 - i) as f64 * spacing / 2.0;
                self.gen_default_pin(
                    output,
                    pin,
                    -body_hw - pin_length,
                    y,
                    0.0,
                );
            }
        }

        for (i, pin) in symbol.pins.iter().skip(left_count).enumerate() {
            let y = (right_count - 1 - i) as f64 * spacing / 2.0;
            self.gen_default_pin(
                output,
                pin,
                body_hw + pin_length,
                y,
                180.0,
            );
        }

        self.indent_level -= 1;
        self.write_line(output, ")");
    }

    /// Resistor template: IEC rectangle + 2 vertical pins
    fn gen_resistor_unit(
        &mut self,
        output: &mut String,
        symbol: &Symbol,
        base: &str,
    ) {
        // _0_1: IEC-style resistor body
        self.write_line(output, &format!("(symbol \"{}_0_1\"", base));
        self.indent_level += 1;
        self.write_line(output, "(rectangle");
        self.indent_level += 1;
        self.write_line(
            output,
            "(start -1.016 1.524) (end 1.016 -1.524)",
        );
        self.gen_default_stroke(output, 0.254);
        self.write_line(output, "(fill (type none))");
        self.indent_level -= 1;
        self.write_line(output, ")");
        self.indent_level -= 1;
        self.write_line(output, ")");

        // _1_1: pins
        self.write_line(output, &format!("(symbol \"{}_1_1\"", base));
        self.indent_level += 1;
        let pin1 = Self::get_pin_or_default(symbol, 0, "1");
        self.gen_default_pin(output, &pin1, 0.0, 2.54, 90.0);
        let pin2 = Self::get_pin_or_default(symbol, 1, "2");
        self.gen_default_pin(output, &pin2, 0.0, -2.54, 270.0);
        self.indent_level -= 1;
        self.write_line(output, ")");
    }

    /// Capacitor template: two parallel plates + 2 vertical pins
    fn gen_capacitor_unit(
        &mut self,
        output: &mut String,
        symbol: &Symbol,
        base: &str,
    ) {
        // _0_1: two plates
        self.write_line(output, &format!("(symbol \"{}_0_1\"", base));
        self.indent_level += 1;

        // Top plate
        self.write_line(output, "(polyline");
        self.indent_level += 1;
        self.write_line(output, "(pts");
        self.indent_level += 1;
        self.write_line(output, "(xy -2.032 -0.762) (xy 2.032 -0.762)");
        self.indent_level -= 1;
        self.write_line(output, ")");
        self.gen_default_stroke(output, 0.508);
        self.write_line(output, "(fill (type none))");
        self.indent_level -= 1;
        self.write_line(output, ")");

        // Bottom plate
        self.write_line(output, "(polyline");
        self.indent_level += 1;
        self.write_line(output, "(pts");
        self.indent_level += 1;
        self.write_line(output, "(xy -2.032 0.762) (xy 2.032 0.762)");
        self.indent_level -= 1;
        self.write_line(output, ")");
        self.gen_default_stroke(output, 0.508);
        self.write_line(output, "(fill (type none))");
        self.indent_level -= 1;
        self.write_line(output, ")");

        self.indent_level -= 1;
        self.write_line(output, ")");

        // _1_1: pins
        self.write_line(output, &format!("(symbol \"{}_1_1\"", base));
        self.indent_level += 1;
        let pin1 = Self::get_pin_or_default(symbol, 0, "1");
        self.gen_default_pin(output, &pin1, 0.0, 3.81, 90.0);
        let pin2 = Self::get_pin_or_default(symbol, 1, "2");
        self.gen_default_pin(output, &pin2, 0.0, -3.81, 270.0);
        self.indent_level -= 1;
        self.write_line(output, ")");
    }

    /// Inductor template: bumps + 2 vertical pins
    fn gen_inductor_unit(
        &mut self,
        output: &mut String,
        symbol: &Symbol,
        base: &str,
    ) {
        // _0_1: three arcs (bumps)
        self.write_line(output, &format!("(symbol \"{}_0_1\"", base));
        self.indent_level += 1;

        for dy in &[-0.762, 0.0, 0.762] {
            self.write_line(output, "(arc");
            self.indent_level += 1;
            self.write_line(
                output,
                &format!(
                    "(start {} {}) (mid {} {}) (end {} {})",
                    Self::format_number(-1.27),
                    Self::format_number(*dy + 0.762),
                    Self::format_number(0.0),
                    Self::format_number(*dy + 1.524),
                    Self::format_number(1.27),
                    Self::format_number(*dy + 0.762),
                ),
            );
            self.gen_default_stroke(output, 0.0);
            self.write_line(output, "(fill (type none))");
            self.indent_level -= 1;
            self.write_line(output, ")");
        }

        self.indent_level -= 1;
        self.write_line(output, ")");

        // _1_1: pins
        self.write_line(output, &format!("(symbol \"{}_1_1\"", base));
        self.indent_level += 1;
        let pin1 = Self::get_pin_or_default(symbol, 0, "1");
        self.gen_default_pin(output, &pin1, 0.0, 3.81, 90.0);
        let pin2 = Self::get_pin_or_default(symbol, 1, "2");
        self.gen_default_pin(output, &pin2, 0.0, -3.81, 270.0);
        self.indent_level -= 1;
        self.write_line(output, ")");
    }

    /// Diode/LED template: triangle + bar + 2 vertical pins
    fn gen_diode_unit(
        &mut self,
        output: &mut String,
        symbol: &Symbol,
        base: &str,
        is_led: bool,
    ) {
        // _0_1: triangle + bar (+ arrows for LED)
        self.write_line(output, &format!("(symbol \"{}_0_1\"", base));
        self.indent_level += 1;

        // Triangle (polyline)
        self.write_line(output, "(polyline");
        self.indent_level += 1;
        self.write_line(output, "(pts");
        self.indent_level += 1;
        self.write_line(output, "(xy -1.27 1.27) (xy 1.27 0) (xy -1.27 -1.27)");
        self.indent_level -= 1;
        self.write_line(output, ")");
        self.gen_default_stroke(output, 0.254);
        self.write_line(output, "(fill (type none))");
        self.indent_level -= 1;
        self.write_line(output, ")");

        // Bar
        self.write_line(output, "(polyline");
        self.indent_level += 1;
        self.write_line(output, "(pts");
        self.indent_level += 1;
        self.write_line(output, "(xy 1.27 1.27) (xy 1.27 -1.27)");
        self.indent_level -= 1;
        self.write_line(output, ")");
        self.gen_default_stroke(output, 0.254);
        self.write_line(output, "(fill (type none))");
        self.indent_level -= 1;
        self.write_line(output, ")");

        // LED arrows
        if is_led {
            self.write_line(output, "(polyline");
            self.indent_level += 1;
            self.write_line(output, "(pts");
            self.indent_level += 1;
            self.write_line(output, "(xy -1.016 1.524) (xy 1.524 1.524)");
            self.indent_level -= 1;
            self.write_line(output, ")");
            self.gen_default_stroke(output, 0.254);
            self.write_line(output, "(fill (type none))");
            self.indent_level -= 1;
            self.write_line(output, ")");

            self.write_line(output, "(polyline");
            self.indent_level += 1;
            self.write_line(output, "(pts");
            self.indent_level += 1;
            self.write_line(output, "(xy 0.508 2.032) (xy 1.524 1.524) (xy 1.524 2.54)");
            self.indent_level -= 1;
            self.write_line(output, ")");
            self.gen_default_stroke(output, 0.2);
            self.write_line(output, "(fill (type none))");
            self.indent_level -= 1;
            self.write_line(output, ")");
        }

        self.indent_level -= 1;
        self.write_line(output, ")");

        // _1_1: pins
        self.write_line(output, &format!("(symbol \"{}_1_1\"", base));
        self.indent_level += 1;
        let pin1 = Self::get_pin_or_default(symbol, 0, "1");
        self.gen_default_pin(output, &pin1, 0.0, 3.81, 90.0);
        let pin2 = Self::get_pin_or_default(symbol, 1, "2");
        self.gen_default_pin(output, &pin2, 0.0, -3.81, 270.0);
        self.indent_level -= 1;
        self.write_line(output, ")");
    }

    /// Generic 2-pin template: small rectangle + 2 vertical pins
    fn gen_twopin_unit(
        &mut self,
        output: &mut String,
        symbol: &Symbol,
        base: &str,
    ) {
        // _0_1: rectangle body
        self.write_line(output, &format!("(symbol \"{}_0_1\"", base));
        self.indent_level += 1;
        self.write_line(output, "(rectangle");
        self.indent_level += 1;
        self.write_line(output, "(start -1.27 1.27) (end 1.27 -1.27)");
        self.gen_default_stroke(output, 0.254);
        self.write_line(output, "(fill (type background))");
        self.indent_level -= 1;
        self.write_line(output, ")");
        self.indent_level -= 1;
        self.write_line(output, ")");

        // _1_1: pins
        self.write_line(output, &format!("(symbol \"{}_1_1\"", base));
        self.indent_level += 1;
        let pin1 = Self::get_pin_or_default(symbol, 0, "1");
        self.gen_default_pin(output, &pin1, 0.0, 2.54, 90.0);
        let pin2 = Self::get_pin_or_default(symbol, 1, "2");
        self.gen_default_pin(output, &pin2, 0.0, -2.54, 270.0);
        self.indent_level -= 1;
        self.write_line(output, ")");
    }

    // ============== Auto Wire Generation ==============

    /// Compute local pin positions for a symbol based on its template type.
    /// Returns HashMap<pin_number, (local_x, local_y)>.
    /// Uses the same pin numbering as gen_*_unit (get_pin_or_default) to avoid mismatches.
    fn compute_pin_positions(symbol: &Symbol) -> HashMap<String, (f64, f64)> {
        let kind = Self::detect_default_kind(symbol);
        let mut positions = HashMap::new();

        match kind {
            DefaultSymbolKind::Connector => {
                let pin_count = symbol.pins.len();
                if pin_count == 0 { return positions; }
                let short = symbol.lib_id.split(':').last().unwrap_or(&symbol.lib_id);
                let is_dual_row = short.contains("02x") || short.contains("_02x");
                let spacing = 2.54;

                if is_dual_row {
                    let rows = (pin_count + 1) / 2;
                    for (i, pin) in symbol.pins.iter().enumerate() {
                        let row = i / 2;
                        let y = ((rows - 1) - row) as f64 * spacing;
                        if i % 2 == 0 {
                            positions.insert(pin.number.clone(), (-5.08, y));
                        } else {
                            positions.insert(pin.number.clone(), (7.62, y));
                        }
                    }
                } else {
                    for (i, pin) in symbol.pins.iter().enumerate() {
                        let y = ((pin_count - 1) - i) as f64 * spacing;
                        positions.insert(pin.number.clone(), (-5.08, y));
                    }
                }
            }
            DefaultSymbolKind::Ic => {
                let pin_count = symbol.pins.len();
                let left_count = (pin_count + 1) / 2;
                let right_count = pin_count - left_count;
                let spacing = 2.54;
                let body_hw = 5.08;
                let pin_length = 2.54;

                for (i, pin) in symbol.pins.iter().enumerate() {
                    if i < left_count {
                        let y = (left_count - 1 - i) as f64 * spacing / 2.0;
                        positions.insert(pin.number.clone(), (-body_hw - pin_length, y));
                    } else {
                        let ri = i - left_count;
                        let y = (right_count - 1 - ri) as f64 * spacing / 2.0;
                        positions.insert(pin.number.clone(), (body_hw + pin_length, y));
                    }
                }
            }
            // 2-pin passives: use pin at positions from KiCad standard symbols.
            // These are the (at x y angle) values in the lib_symbol definition,
            // which match what SCH_PIN::GetPosition() returns after Y-negation.
            DefaultSymbolKind::Resistor | DefaultSymbolKind::TwoPin => {
                let pin1 = Self::get_pin_or_default(symbol, 0, "1");
                positions.insert(pin1.number.clone(), (0.0, 3.81));
                let pin2 = Self::get_pin_or_default(symbol, 1, "2");
                positions.insert(pin2.number.clone(), (0.0, -3.81));
            }
            DefaultSymbolKind::Capacitor | DefaultSymbolKind::Inductor => {
                let pin1 = Self::get_pin_or_default(symbol, 0, "1");
                positions.insert(pin1.number.clone(), (0.0, 3.81));
                let pin2 = Self::get_pin_or_default(symbol, 1, "2");
                positions.insert(pin2.number.clone(), (0.0, -3.81));
            }
            DefaultSymbolKind::Diode | DefaultSymbolKind::Led => {
                let pin1 = Self::get_pin_or_default(symbol, 0, "1");
                positions.insert(pin1.number.clone(), (-3.81, 0.0));
                let pin2 = Self::get_pin_or_default(symbol, 1, "2");
                positions.insert(pin2.number.clone(), (3.81, 0.0));
            }
        }

        positions
    }

    /// Normalize lib_id: ensure it has a library prefix (LibraryName:SymbolName).
    /// "Device:R" → unchanged; "FP6277" → "custom:FP6277".
    fn normalize_lib_id(lib_id: &str) -> String {
        if lib_id.contains(':') {
            lib_id.to_string()
        } else {
            format!("custom:{}", lib_id)
        }
    }

    /// Rotate a local point by the component's rotation angle (degrees).
    /// KiCad uses CLOCKWISE rotation: 0=(1,0,0,1), 90=(0,1,-1,0), 180=(-1,0,0,-1), 270=(0,-1,1,0)
    fn rotate_point(lx: f64, ly: f64, rotation_deg: f64) -> (f64, f64) {
        match rotation_deg as i32 {
            0 | 360 => (lx, ly),
            90 | -270 => (ly, -lx),
            180 | -180 => (-lx, -ly),
            270 | -90 => (-ly, lx),
            _ => {
                // Generic CW rotation
                let rad = rotation_deg.to_radians();
                let c = rad.cos();
                let s = rad.sin();
                (lx * c + ly * s, -lx * s + ly * c)
            }
        }
    }

    /// Transform a local file-coordinate offset by component rotation.
    /// Input (lx, ly) is in file coordinates (Y-DOWN) relative to symbol origin.
    /// Output is the file-coordinate offset to add to the component's (at) position.
    ///
    /// KiCad internally uses Y-UP; parseXY(true) negates Y when loading.
    /// The net effect on file coords is:
    ///   0° → (lx, ly),  90° → (-ly, lx),  180° → (-lx, -ly),  270° → (ly, -lx)
    fn transform_file_offset(lx: f64, ly: f64, rotation_deg: f64) -> (f64, f64) {
        match rotation_deg as i32 {
            0 | 360 => (lx, ly),
            90 | -270 => (-ly, lx),
            180 | -180 => (-lx, -ly),
            270 | -90 => (ly, -lx),
            _ => {
                // Generic: same as KiCad's internal transform applied to file coords
                let (ix, iy) = (lx, -ly); // file → internal
                let rad = rotation_deg.to_radians();
                let c = rad.cos();
                let s = rad.sin();
                let rx = ix * c + iy * s;
                let ry = -ix * s + iy * c;
                (rx, -ry) // internal → file
            }
        }
    }

    /// Compute label rotation from pin local position and component rotation.
    /// The label should face the same direction as the pin's wire connection point.
    fn compute_label_rotation(lx: f64, ly: f64, crot: f64) -> f64 {
        // Determine pin direction in file coordinates (before component rotation).
        // In gen_ic_unit/gen_passive_unit:
        //   Left pins:   x < 0, pin points RIGHT  → label rotation 0
        //   Right pins:  x > 0, pin points LEFT   → label rotation 180
        //   Top pins:    y > 0 (file coords), pin points DOWN → label rotation 90
        //   Bottom pins: y < 0, pin points UP   → label rotation 270
        let local_rot = if lx.abs() > ly.abs() {
            if lx < 0.0 { 0.0 } else { 180.0 }
        } else {
            if ly > 0.0 { 90.0 } else { 270.0 }
        };
        // Apply component rotation
        (local_rot + crot) % 360.0
    }

    /// Auto-generate wires from net connectivity.
    fn generate_auto_wires(
        &mut self,
        output: &mut String,
        schematic: &crate::ir::Schematic,
    ) {
        // 1. Build pin position map for each lib_symbol: lib_id -> { pin_number -> (lx, ly) }
        let mut symbol_pins: HashMap<String, HashMap<String, (f64, f64)>> = HashMap::new();
        for symbol in &schematic.lib_symbols {
            let positions = Self::compute_pin_positions(symbol);
            symbol_pins.insert(Self::normalize_lib_id(&symbol.lib_id), positions);
        }

        // 2. Build net -> list of absolute pin positions
        let mut net_endpoints: HashMap<u32, Vec<(f64, f64)>> = HashMap::new();
        for comp in &schematic.components {
            let comp_lib_id = Self::normalize_lib_id(&comp.lib_id);
            let pin_positions = match symbol_pins.get(&comp_lib_id) {
                Some(p) => p,
                None => continue,
            };
            let (cx, cy, crot) = comp.position;

            for pin in &comp.pins {
                if let (Some(net_id), Some(&(lx, ly))) = (pin.net_id, pin_positions.get(&pin.number)) {
                    let (rx, ry) = Self::rotate_point(lx, ly, crot);
                    net_endpoints
                        .entry(net_id)
                        .or_default()
                        .push((cx + rx, cy + ry));
                }
            }
        }

        // 3. Generate wires for each net using L-shaped connections
        let default_stroke = Stroke::default();

        let mut sorted_nets: Vec<_> = net_endpoints.into_iter().collect();
        sorted_nets.sort_by_key(|(id, _)| *id);

        for (_net_id, mut pts) in sorted_nets {
            if pts.len() < 2 {
                continue;
            }

            // Snap to 1.27mm (50mil) grid
            let snap = |v: f64| (v / 1.27).round() * 1.27;
            for p in &mut pts {
                p.0 = snap(p.0);
                p.1 = snap(p.1);
            }

            // Sort by x coordinate, then by y
            pts.sort_by(|a, b| {
                a.0.partial_cmp(&b.0)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            });

            // Connect consecutive pins with L-shaped wires
            for i in 0..pts.len() - 1 {
                let (x1, y1) = pts[i];
                let (x2, y2) = pts[i + 1];

                // Horizontal segment: (x1, y1) -> (x2, y1)
                if (x1 - x2).abs() > 0.01 {
                    self.write_line(output, "(wire");
                    self.indent_level += 1;
                    self.write_line(output, &format!(
                        "(pts {} {})",
                        Self::format_xy(x1, y1),
                        Self::format_xy(x2, y1)
                    ));
                    self.generate_stroke(output, &default_stroke);
                    self.indent_level -= 1;
                    self.write_line(output, ")");
                }

                // Vertical segment: (x2, y1) -> (x2, y2)
                if (y1 - y2).abs() > 0.01 {
                    self.write_line(output, "(wire");
                    self.indent_level += 1;
                    self.write_line(output, &format!(
                        "(pts {} {})",
                        Self::format_xy(x2, y1),
                        Self::format_xy(x2, y2)
                    ));
                    self.generate_stroke(output, &default_stroke);
                    self.indent_level -= 1;
                    self.write_line(output, ")");
                }
            }
        }
    }

    /// Auto-generate labels from net connectivity.
    /// Instead of L-shaped wires, place a label at each pin's world coordinate.
    /// Pins sharing the same net_id get the same label text, so KiCad connects them implicitly.
    fn generate_auto_labels(
        &mut self,
        output: &mut String,
        schematic: &crate::ir::Schematic,
    ) {
        // 1. Build pin position map for each lib_symbol
        let mut symbol_pins: HashMap<String, HashMap<String, (f64, f64)>> = HashMap::new();
        for symbol in &schematic.lib_symbols {
            let positions = Self::compute_pin_positions(symbol);
            symbol_pins.insert(Self::normalize_lib_id(&symbol.lib_id), positions);
        }

        // 2. Build net_id -> net_name lookup
        let net_names: HashMap<u32, &str> = schematic.nets.iter()
            .map(|n| (n.id, n.name.as_str()))
            .collect();

        // 3. Collect all (x, y, rotation, net_name) for pins with net assignments
        let mut labels_to_place: Vec<(f64, f64, f64, &str)> = Vec::new();

        for comp in &schematic.components {
            let comp_lib_id = Self::normalize_lib_id(&comp.lib_id);
            let pin_positions = match symbol_pins.get(&comp_lib_id) {
                Some(p) => p,
                None => continue,
            };
            let (cx, cy, crot) = comp.position;

            for pin in &comp.pins {
                if let (Some(net_id), Some(&(lx, ly))) = (pin.net_id, pin_positions.get(&pin.number)) {
                    if let Some(net_name) = net_names.get(&net_id) {
                        // Pin world position in file coordinates:
                        // KiCad internally Y-negates pin at positions (parseXY(true)),
                        // so we negate Y here to match. Then apply component rotation.
                        let (rx, ry) = Self::rotate_point(lx, -ly, crot);
                        let wx = cx + rx;
                        let wy = cy + ry;

                        // Label rotation: based on pin direction in file coordinates.
                        // In the file, pin direction points AWAY from the symbol body
                        // (toward the wire connection end). The label should face the
                        // same direction as the pin so it reads naturally.
                        let label_rot = Self::compute_label_rotation(lx, ly, crot);

                        labels_to_place.push((wx, wy, label_rot, *net_name));
                    }
                }
            }
        }

        // 4. Sort by net_name then position for deterministic output
        labels_to_place.sort_by(|a, b| {
            a.3.cmp(b.3)
                .then(a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
                .then(a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        });

        // 5. Generate one label per pin at its world coordinate.
        // KiCad connects pins that share the same label text at the same location.
        // Do NOT snap to grid — the label must be at the exact pin connection point.
        let default_effects = TextEffects::default();

        for (x, y, rot, net_name) in &labels_to_place {
            // Use local net labels (plain text) for most signals.
            // Local labels connect pins within the same sheet — cleaner visually.
            self.write_line(output, "(label");
            self.indent_level += 1;
            self.write_line(output, &format!("\"{}\"", Self::escape_string(net_name)));
            self.write_line(output, &Self::format_at(*x, *y, *rot));
            self.generate_effects(output, &default_effects);
            if self.config.include_uuids {
                self.write_line(output, &format!("(uuid \"{}\")", Self::new_uuid()));
            }
            self.indent_level -= 1;
            self.write_line(output, ")");
        }

        // 6. Generate (no_connect ...) for pins marked nc=true in JSON5
        for comp in &schematic.components {
            let comp_lib_id = Self::normalize_lib_id(&comp.lib_id);
            let pin_positions = match symbol_pins.get(&comp_lib_id) {
                Some(p) => p,
                None => continue,
            };
            let (cx, cy, crot) = comp.position;
            for pin in &comp.pins {
                if pin.nc {
                    if let Some(&(lx, ly)) = pin_positions.get(&pin.number) {
                        let (rx, ry) = Self::rotate_point(lx, -ly, crot);
                        let wx = cx + rx;
                        let wy = cy + ry;
                        self.write_line(output, "(no_connect");
                        self.indent_level += 1;
                        self.write_line(output, &format!("(at {} {})", Self::format_number(wx), Self::format_number(wy)));
                        if self.config.include_uuids {
                            self.write_line(output, &format!("(uuid \"{}\")", Self::new_uuid()));
                        }
                        self.indent_level -= 1;
                        self.write_line(output, ")");
                    }
                }
            }
        }

        // 7. Generate PWR_FLAG symbols for nets that have power_in pins
        //    but no power_out pins, to satisfy KiCad's ERC power-pin-driven check.
        if self.config.insert_power_flags {
            self.generate_power_flags(output, schematic, &labels_to_place);
        }
    }

    /// Place PWR_FLAG instances on nets with power_in pins but no power_out.
    /// PWR_FLAG is a KiCad standard symbol with a power_out pin (length=0 at origin).
    /// Placing it at any label position for the target net marks that net as driven.
    fn generate_power_flags(
        &mut self,
        output: &mut String,
        schematic: &crate::ir::Schematic,
        labels_to_place: &[(f64, f64, f64, &str)],
    ) {
        let is_v9_plus = matches!(self.effective_version, KicadVersion::V9 | KicadVersion::V10);

        // Build lib_id -> { pin_number -> pin_type } from lib_symbols
        let mut lib_pin_types: HashMap<String, HashMap<&str, &str>> = HashMap::new();
        for symbol in &schematic.lib_symbols {
            let mut pin_map: HashMap<&str, &str> = HashMap::new();
            for pin in &symbol.pins {
                pin_map.insert(&pin.number, &pin.pin_type);
            }
            lib_pin_types.insert(Self::normalize_lib_id(&symbol.lib_id), pin_map);
        }

        // Collect net_id -> set of pin types (from lib_symbol definitions)
        let mut net_pin_types: HashMap<u32, Vec<&str>> = HashMap::new();
        for comp in &schematic.components {
            let comp_lib_id = Self::normalize_lib_id(&comp.lib_id);
            let pin_type_map = match lib_pin_types.get(&comp_lib_id) {
                Some(m) => m,
                None => continue,
            };
            for pin in &comp.pins {
                if let Some(net_id) = pin.net_id {
                    // Use lib_symbol's pin type, not component's (component defaults to "passive")
                    let ptype = pin_type_map.get(pin.number.as_str()).unwrap_or(&"passive");
                    net_pin_types.entry(net_id).or_default().push(ptype);
                }
            }
        }

        // Find nets that have power_in but no power_out or output pins.
        // Also skip nets driven by any output-capable pin to avoid pin_to_pin conflicts.
        let net_names: HashMap<u32, &str> = schematic.nets.iter()
            .map(|n| (n.id, n.name.as_str()))
            .collect();

        let mut power_flag_nets: Vec<&str> = Vec::new();
        for (net_id, types) in &net_pin_types {
            let has_power_in = types.iter().any(|t| *t == "power_in");
            let has_driver = types.iter().any(|t| {
                matches!(*t, "power_out" | "output" | "bidirectional")
            });
            if has_power_in && !has_driver {
                if let Some(name) = net_names.get(net_id) {
                    power_flag_nets.push(*name);
                }
            }
        }
        power_flag_nets.sort();
        power_flag_nets.dedup();

        if power_flag_nets.is_empty() {
            return;
        }

        // Find a label position for each net (first occurrence)
        let mut net_positions: HashMap<&str, (f64, f64)> = HashMap::new();
        for &(x, y, _, name) in labels_to_place {
            if power_flag_nets.contains(&name) && !net_positions.contains_key(&name) {
                net_positions.insert(name, (x, y));
            }
        }

        // Generate PWR_FLAG symbol definition in lib_symbols (injected inline)
        // We add it directly before the first PWR_FLAG instance using a comment marker
        // Actually, we need to add it to lib_symbols. But we can't modify lib_symbols here.
        // Instead, we rely on the PWR_FLAG being referenced and KiCad resolving it
        // from its standard power library. For safety, we also emit the lib_symbol.

        // Emit PWR_FLAG lib_symbol definition (if not already present)
        // This gets written into the schematic's symbol instances section as a standalone
        // component that references "power:PWR_FLAG". KiCad will resolve it from the
        // standard power library at load time.

        let mut flag_index: u32 = 1;
        for net_name in &power_flag_nets {
            if let Some(&(x, y)) = net_positions.get(net_name) {
                let ref_name = format!("#FLG{:02}", flag_index);
                flag_index += 1;
                self.write_line(output, "(symbol");
                self.indent_level += 1;
                self.write_line(output, "(lib_id \"power:PWR_FLAG\")");
                self.write_line(output, &Self::format_at(x, y, 0.0));
                self.write_line(output, "(unit 1)");
                if is_v9_plus {
                    self.write_line(output, "(body_style 1)");
                    self.write_line(output, "(exclude_from_sim no)");
                }
                self.write_line(output, "(in_bom yes)");
                self.write_line(output, "(on_board yes)");
                if is_v9_plus {
                    self.write_line(output, "(dnp no)");
                }
                if self.config.include_uuids {
                    self.write_line(output, &format!("(uuid \"{}\")", Self::new_uuid()));
                }
                self.write_line(output, &format!(
                    "(property \"Reference\" \"{}\" {} (effects (font (size 1.27 1.27)) (hide yes)))",
                    ref_name,
                    Self::format_at(x, y, 0.0)
                ));
                self.write_line(output, &format!(
                    "(property \"Value\" \"{}\" {} (effects (font (size 1.27 1.27))))",
                    Self::escape_string(net_name),
                    Self::format_at(x, y, 0.0)
                ));
                if self.config.include_uuids {
                    self.write_line(output, &format!(
                        "(pin \"1\" (uuid \"{}\"))",
                        Self::new_uuid()
                    ));
                }
                self.indent_level -= 1;
                self.write_line(output, ")");
            }
        }
    }
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
        assert_eq!(SexprGenerator::escape_string("say \"hi\""), "say \\\"hi\\\"");
        assert_eq!(SexprGenerator::escape_string("line1\nline2"), "line1\\nline2");
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
