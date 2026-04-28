use super::*;
use crate::ir::{Net, RenderHint};
use std::collections::HashSet;

// ============== Symbol Library Generation ==============

impl SexprGenerator {
    pub(super) fn generate_lib_symbols(&mut self, output: &mut String, symbols: &[Symbol], nets: &[Net]) {
        self.write_line(output, "(lib_symbols");
        self.indent_level += 1;

        for symbol in symbols {
            self.generate_symbol_def(output, symbol);
        }

        // Inject power symbol lib definitions for render=Power nets
        let mut injected: HashSet<String> = HashSet::new();
        for net in nets {
            if net.render == RenderHint::Power {
                if let Some(lib_id) = Self::resolve_power_symbol(&net.name) {
                    if injected.insert(lib_id.clone()) {
                        self.generate_minimal_power_lib(output, &lib_id, &net.name);
                    }
                }
            }
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

    /// Generate a minimal power symbol lib definition for render=Power nets.
    /// Follows KiCad's power symbol format: pin in _0_0, graphics in _0_1.
    fn generate_minimal_power_lib(&mut self, output: &mut String, lib_id: &str, _net_name: &str) {
        let short = lib_id.split(':').last().unwrap_or(lib_id);
        let is_v9_plus = matches!(self.effective_version, KicadVersion::V9 | KicadVersion::V10);

        self.write_line(output, &format!("(symbol \"{}\"", lib_id));
        self.indent_level += 1;
        self.write_line(output, "(power)");
        self.write_line(output, "(pin_names");
        self.indent_level += 1;
        self.write_line(output, "(offset 0)");
        self.write_line(output, "(hide yes)");
        self.indent_level -= 1;
        self.write_line(output, ")");

        if is_v9_plus {
            self.write_line(output, "(exclude_from_sim no)");
        }
        self.write_line(output, "(in_bom yes)");
        self.write_line(output, "(on_board yes)");
        if is_v9_plus {
            self.write_line(output, "(in_pos_files yes)");
            self.write_line(output, "(duplicate_pin_numbers_are_jumpers no)");
        }

        // Reference property — hidden, shown as #PWR
        self.write_line(output, "(property \"Reference\" \"#PWR\"");
        self.indent_level += 1;
        self.write_line(output, "(at 0 0 0)");
        if is_v9_plus {
            self.write_line(output, "(show_name no)");
            self.write_line(output, "(do_not_autoplace no)");
        }
        self.write_line(output, "(hide yes)");
        self.write_line(output, "(effects");
        self.indent_level += 1;
        self.write_line(output, "(font (size 1.27 1.27))");
        self.indent_level -= 1;
        self.write_line(output, ")");
        self.indent_level -= 1;
        self.write_line(output, ")");

        // Value property — visible label showing net name
        self.write_line(output, &format!("(property \"Value\" \"{}\"", short));
        self.indent_level += 1;
        self.write_line(output, "(at 0 3.81 0)");
        if is_v9_plus {
            self.write_line(output, "(show_name no)");
            self.write_line(output, "(do_not_autoplace no)");
        }
        self.write_line(output, "(effects");
        self.indent_level += 1;
        self.write_line(output, "(font (size 1.27 1.27))");
        self.indent_level -= 1;
        self.write_line(output, ")");
        self.indent_level -= 1;
        self.write_line(output, ")");

        // Footprint property
        self.write_line(output, "(property \"Footprint\" \"\"");
        self.indent_level += 1;
        self.write_line(output, "(at 0 0 0)");
        if is_v9_plus {
            self.write_line(output, "(show_name no)");
            self.write_line(output, "(do_not_autoplace no)");
        }
        self.write_line(output, "(hide yes)");
        self.write_line(output, "(effects");
        self.indent_level += 1;
        self.write_line(output, "(font (size 1.27 1.27))");
        self.indent_level -= 1;
        self.write_line(output, ")");
        self.indent_level -= 1;
        self.write_line(output, ")");

        // Datasheet property
        self.write_line(output, "(property \"Datasheet\" \"\"");
        self.indent_level += 1;
        self.write_line(output, "(at 0 0 0)");
        if is_v9_plus {
            self.write_line(output, "(show_name no)");
            self.write_line(output, "(do_not_autoplace no)");
        }
        self.write_line(output, "(hide yes)");
        self.write_line(output, "(effects");
        self.indent_level += 1;
        self.write_line(output, "(font (size 1.27 1.27))");
        self.indent_level -= 1;
        self.write_line(output, ")");
        self.indent_level -= 1;
        self.write_line(output, ")");

        // Unit 0_1: vertical line graphic
        self.write_line(output, &format!("(symbol \"{}_0_1\"", short));
        self.indent_level += 1;
        self.write_line(output, "(polyline");
        self.indent_level += 1;
        self.write_line(output, "(pts");
        self.indent_level += 1;
        self.write_line(output, "(xy 0 0) (xy 0 1.27)");
        self.indent_level -= 1;
        self.write_line(output, ")");
        self.write_line(output, "(stroke (width 0) (type default))");
        self.write_line(output, "(fill (type none))");
        self.indent_level -= 1;
        self.write_line(output, ")");
        self.indent_level -= 1;
        self.write_line(output, ")");

        // Unit 0_0: power_out pin at origin
        self.write_line(output, &format!("(symbol \"{}_0_0\"", short));
        self.indent_level += 1;
        self.write_line(output, "(pin power_out line (at 0 0 90) (length 0)");
        self.indent_level += 1;
        self.write_line(output, "(name \"\"");
        self.indent_level += 1;
        self.write_line(output, "(effects (font (size 1.27 1.27)))");
        self.indent_level -= 1;
        self.write_line(output, ")");
        self.write_line(output, "(number \"1\"");
        self.indent_level += 1;
        self.write_line(output, "(effects (font (size 1.27 1.27)))");
        self.indent_level -= 1;
        self.write_line(output, ")");
        self.indent_level -= 1;
        self.write_line(output, ")");
        self.indent_level -= 1;
        self.write_line(output, ")");

        if is_v9_plus {
            self.write_line(output, "(embedded_fonts no)");
        }
        self.indent_level -= 1;
        self.write_line(output, ")");
    }

    fn generate_symbol_def(&mut self, output: &mut String, symbol: &Symbol) {
        // Fast path: use embedded standard symbol (only for V10 — the embedded text is V10 format)
        if matches!(self.effective_version, KicadVersion::V10) {
            if let Some(sexpr) = crate::codegen::standard_symbols::get_standard_symbol(&symbol.lib_id) {
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

    pub(super) fn detect_default_kind(symbol: &Symbol) -> DefaultSymbolKind {
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
    pub(super) fn get_pin_or_default(symbol: &Symbol, index: usize, default_number: &str) -> Pin {
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
}
