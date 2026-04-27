//! KiCad standard library symbols extracted from Device.kicad_sym
//!
//! These are verbatim copies of KiCad's standard Device library symbols.
//! Using these eliminates lib_symbol_mismatch ERC warnings and ensures
//! pin positions match KiCad's standard library exactly.

/// Returns the standard symbol S-expression for well-known Device library components.
/// The returned string uses the short symbol name (e.g., "R" not "Device:R").
pub fn get_standard_symbol(lib_id: &str) -> Option<&'static str> {
    // Strip library prefix for matching
    let short = lib_id.split(':').last().unwrap_or(lib_id);
    match short {
        "R" => Some(SYMBOL_R),
        "C" => Some(SYMBOL_C),
        "L" => Some(SYMBOL_L),
        "D" => Some(SYMBOL_D),
        "LED" => Some(SYMBOL_LED),
        "Thermistor_NTC" | "NTC" => Some(SYMBOL_NTC),
        _ => None,
    }
}

const SYMBOL_R: &str = r#"	(symbol "R"
		(pin_numbers
			(hide yes)
		)
		(pin_names
			(offset 0)
		)
		(exclude_from_sim no)
		(in_bom yes)
		(on_board yes)
		(in_pos_files yes)
		(duplicate_pin_numbers_are_jumpers no)
		(property "Reference" "R"
			(at 2.032 0 90)
			(show_name no)
			(do_not_autoplace no)
			(effects
				(font
					(size 1.27 1.27)
				)
			)
		)
		(property "Value" "R"
			(at 0 0 90)
			(show_name no)
			(do_not_autoplace no)
			(effects
				(font
					(size 1.27 1.27)
				)
			)
		)
		(property "Footprint" ""
			(at -1.778 0 90)
			(show_name no)
			(do_not_autoplace no)
			(hide yes)
			(effects
				(font
					(size 1.27 1.27)
				)
			)
		)
		(property "Datasheet" ""
			(at 0 0 0)
			(show_name no)
			(do_not_autoplace no)
			(hide yes)
			(effects
				(font
					(size 1.27 1.27)
				)
			)
		)
		(property "Description" "Resistor"
			(at 0 0 0)
			(show_name no)
			(do_not_autoplace no)
			(hide yes)
			(effects
				(font
					(size 1.27 1.27)
				)
			)
		)
		(property "ki_keywords" "R res resistor"
			(at 0 0 0)
			(show_name no)
			(do_not_autoplace no)
			(hide yes)
			(effects
				(font
					(size 1.27 1.27)
				)
			)
		)
		(property "ki_fp_filters" "R_*"
			(at 0 0 0)
			(show_name no)
			(do_not_autoplace no)
			(hide yes)
			(effects
				(font
					(size 1.27 1.27)
				)
			)
		)
		(symbol "R_0_1"
			(rectangle
				(start -1.016 -2.54)
				(end 1.016 2.54)
				(stroke
					(width 0.254)
					(type default)
				)
				(fill
					(type none)
				)
			)
		)
		(symbol "R_1_1"
			(pin passive line
				(at 0 3.81 270)
				(length 1.27)
				(name ""
					(effects
						(font
							(size 1.27 1.27)
						)
					)
				)
				(number "1"
					(effects
						(font
							(size 1.27 1.27)
						)
					)
				)
			)
			(pin passive line
				(at 0 -3.81 90)
				(length 1.27)
				(name ""
					(effects
						(font
							(size 1.27 1.27)
						)
					)
				)
				(number "2"
					(effects
						(font
							(size 1.27 1.27)
						)
					)
				)
			)
		)
		(embedded_fonts no)
	)	"#;

const SYMBOL_C: &str = r#"	(symbol "C"
		(pin_numbers
			(hide yes)
		)
		(pin_names
			(offset 0.254)
		)
		(exclude_from_sim no)
		(in_bom yes)
		(on_board yes)
		(in_pos_files yes)
		(duplicate_pin_numbers_are_jumpers no)
		(property "Reference" "C"
			(at 0.635 2.54 0)
			(show_name no)
			(do_not_autoplace no)
			(effects
				(font
					(size 1.27 1.27)
				)
				(justify left)
			)
		)
		(property "Value" "C"
			(at 0.635 -2.54 0)
			(show_name no)
			(do_not_autoplace no)
			(effects
				(font
					(size 1.27 1.27)
				)
				(justify left)
			)
		)
		(property "Footprint" ""
			(at 0.9652 -3.81 0)
			(show_name no)
			(do_not_autoplace no)
			(hide yes)
			(effects
				(font
					(size 1.27 1.27)
				)
			)
		)
		(property "Datasheet" ""
			(at 0 0 0)
			(show_name no)
			(do_not_autoplace no)
			(hide yes)
			(effects
				(font
					(size 1.27 1.27)
				)
			)
		)
		(property "Description" "Unpolarized capacitor"
			(at 0 0 0)
			(show_name no)
			(do_not_autoplace no)
			(hide yes)
			(effects
				(font
					(size 1.27 1.27)
				)
			)
		)
		(property "ki_keywords" "cap capacitor"
			(at 0 0 0)
			(show_name no)
			(do_not_autoplace no)
			(hide yes)
			(effects
				(font
					(size 1.27 1.27)
				)
			)
		)
		(property "ki_fp_filters" "C_*"
			(at 0 0 0)
			(show_name no)
			(do_not_autoplace no)
			(hide yes)
			(effects
				(font
					(size 1.27 1.27)
				)
			)
		)
		(symbol "C_0_1"
			(polyline
				(pts
					(xy -2.032 0.762) (xy 2.032 0.762)
				)
				(stroke
					(width 0.508)
					(type default)
				)
				(fill
					(type none)
				)
			)
			(polyline
				(pts
					(xy -2.032 -0.762) (xy 2.032 -0.762)
				)
				(stroke
					(width 0.508)
					(type default)
				)
				(fill
					(type none)
				)
			)
		)
		(symbol "C_1_1"
			(pin passive line
				(at 0 3.81 270)
				(length 2.794)
				(name ""
					(effects
						(font
							(size 1.27 1.27)
						)
					)
				)
				(number "1"
					(effects
						(font
							(size 1.27 1.27)
						)
					)
				)
			)
			(pin passive line
				(at 0 -3.81 90)
				(length 2.794)
				(name ""
					(effects
						(font
							(size 1.27 1.27)
						)
					)
				)
				(number "2"
					(effects
						(font
							(size 1.27 1.27)
						)
					)
				)
			)
		)
		(embedded_fonts no)
	)	"#;

const SYMBOL_L: &str = r#"	(symbol "L"
		(pin_numbers
			(hide yes)
		)
		(pin_names
			(offset 1.016)
			(hide yes)
		)
		(exclude_from_sim no)
		(in_bom yes)
		(on_board yes)
		(in_pos_files yes)
		(duplicate_pin_numbers_are_jumpers no)
		(property "Reference" "L"
			(at -1.27 0 90)
			(show_name no)
			(do_not_autoplace no)
			(effects
				(font
					(size 1.27 1.27)
				)
			)
		)
		(property "Value" "L"
			(at 1.905 0 90)
			(show_name no)
			(do_not_autoplace no)
			(effects
				(font
					(size 1.27 1.27)
				)
			)
		)
		(property "Footprint" ""
			(at 0 0 0)
			(show_name no)
			(do_not_autoplace no)
			(hide yes)
			(effects
				(font
					(size 1.27 1.27)
				)
			)
		)
		(property "Datasheet" ""
			(at 0 0 0)
			(show_name no)
			(do_not_autoplace no)
			(hide yes)
			(effects
				(font
					(size 1.27 1.27)
				)
			)
		)
		(property "Description" "Inductor"
			(at 0 0 0)
			(show_name no)
			(do_not_autoplace no)
			(hide yes)
			(effects
				(font
					(size 1.27 1.27)
				)
			)
		)
		(property "ki_keywords" "inductor choke coil reactor magnetic"
			(at 0 0 0)
			(show_name no)
			(do_not_autoplace no)
			(hide yes)
			(effects
				(font
					(size 1.27 1.27)
				)
			)
		)
		(property "ki_fp_filters" "Choke_* *Coil* Inductor_* L_*"
			(at 0 0 0)
			(show_name no)
			(do_not_autoplace no)
			(hide yes)
			(effects
				(font
					(size 1.27 1.27)
				)
			)
		)
		(symbol "L_0_1"
			(arc
				(start 0 2.54)
				(mid 0.6323 1.905)
				(end 0 1.27)
				(stroke
					(width 0)
					(type default)
				)
				(fill
					(type none)
				)
			)
			(arc
				(start 0 1.27)
				(mid 0.6323 0.635)
				(end 0 0)
				(stroke
					(width 0)
					(type default)
				)
				(fill
					(type none)
				)
			)
			(arc
				(start 0 0)
				(mid 0.6323 -0.635)
				(end 0 -1.27)
				(stroke
					(width 0)
					(type default)
				)
				(fill
					(type none)
				)
			)
			(arc
				(start 0 -1.27)
				(mid 0.6323 -1.905)
				(end 0 -2.54)
				(stroke
					(width 0)
					(type default)
				)
				(fill
					(type none)
				)
			)
		)
		(symbol "L_1_1"
			(pin passive line
				(at 0 3.81 270)
				(length 1.27)
				(name "1"
					(effects
						(font
							(size 1.27 1.27)
						)
					)
				)
				(number "1"
					(effects
						(font
							(size 1.27 1.27)
						)
					)
				)
			)
			(pin passive line
				(at 0 -3.81 90)
				(length 1.27)
				(name "2"
					(effects
						(font
							(size 1.27 1.27)
						)
					)
				)
				(number "2"
					(effects
						(font
							(size 1.27 1.27)
						)
					)
				)
			)
		)
		(embedded_fonts no)
	)	"#;

const SYMBOL_D: &str = r#"	(symbol "D"
		(pin_numbers
			(hide yes)
		)
		(pin_names
			(offset 1.016)
			(hide yes)
		)
		(exclude_from_sim no)
		(in_bom yes)
		(on_board yes)
		(in_pos_files yes)
		(duplicate_pin_numbers_are_jumpers no)
		(property "Reference" "D"
			(at 0 2.54 0)
			(show_name no)
			(do_not_autoplace no)
			(effects
				(font
					(size 1.27 1.27)
				)
			)
		)
		(property "Value" "D"
			(at 0 -2.54 0)
			(show_name no)
			(do_not_autoplace no)
			(effects
				(font
					(size 1.27 1.27)
				)
			)
		)
		(property "Footprint" ""
			(at 0 0 0)
			(show_name no)
			(do_not_autoplace no)
			(hide yes)
			(effects
				(font
					(size 1.27 1.27)
				)
			)
		)
		(property "Datasheet" ""
			(at 0 0 0)
			(show_name no)
			(do_not_autoplace no)
			(hide yes)
			(effects
				(font
					(size 1.27 1.27)
				)
			)
		)
		(property "Description" "Diode"
			(at 0 0 0)
			(show_name no)
			(do_not_autoplace no)
			(hide yes)
			(effects
				(font
					(size 1.27 1.27)
				)
			)
		)
		(property "Sim.Device" "D"
			(at 0 0 0)
			(show_name no)
			(do_not_autoplace no)
			(hide yes)
			(effects
				(font
					(size 1.27 1.27)
				)
			)
		)
		(property "Sim.Pins" "1=K 2=A"
			(at 0 0 0)
			(show_name no)
			(do_not_autoplace no)
			(hide yes)
			(effects
				(font
					(size 1.27 1.27)
				)
			)
		)
		(property "ki_keywords" "diode"
			(at 0 0 0)
			(show_name no)
			(do_not_autoplace no)
			(hide yes)
			(effects
				(font
					(size 1.27 1.27)
				)
			)
		)
		(property "ki_fp_filters" "TO-???* *_Diode_* *SingleDiode* D_*"
			(at 0 0 0)
			(show_name no)
			(do_not_autoplace no)
			(hide yes)
			(effects
				(font
					(size 1.27 1.27)
				)
			)
		)
		(symbol "D_0_1"
			(polyline
				(pts
					(xy -1.27 1.27) (xy -1.27 -1.27)
				)
				(stroke
					(width 0.254)
					(type default)
				)
				(fill
					(type none)
				)
			)
			(polyline
				(pts
					(xy 1.27 1.27) (xy 1.27 -1.27) (xy -1.27 0) (xy 1.27 1.27)
				)
				(stroke
					(width 0.254)
					(type default)
				)
				(fill
					(type none)
				)
			)
			(polyline
				(pts
					(xy 1.27 0) (xy -1.27 0)
				)
				(stroke
					(width 0)
					(type default)
				)
				(fill
					(type none)
				)
			)
		)
		(symbol "D_1_1"
			(pin passive line
				(at -3.81 0 0)
				(length 2.54)
				(name "K"
					(effects
						(font
							(size 1.27 1.27)
						)
					)
				)
				(number "1"
					(effects
						(font
							(size 1.27 1.27)
						)
					)
				)
			)
			(pin passive line
				(at 3.81 0 180)
				(length 2.54)
				(name "A"
					(effects
						(font
							(size 1.27 1.27)
						)
					)
				)
				(number "2"
					(effects
						(font
							(size 1.27 1.27)
						)
					)
				)
			)
		)
		(embedded_fonts no)
	)	"#;

const SYMBOL_LED: &str = r#"	(symbol "LED"
		(pin_numbers
			(hide yes)
		)
		(pin_names
			(offset 1.016)
			(hide yes)
		)
		(exclude_from_sim no)
		(in_bom yes)
		(on_board yes)
		(in_pos_files yes)
		(duplicate_pin_numbers_are_jumpers no)
		(property "Reference" "D"
			(at 0 2.54 0)
			(show_name no)
			(do_not_autoplace no)
			(effects
				(font
					(size 1.27 1.27)
				)
			)
		)
		(property "Value" "LED"
			(at 0 -2.54 0)
			(show_name no)
			(do_not_autoplace no)
			(effects
				(font
					(size 1.27 1.27)
				)
			)
		)
		(property "Footprint" ""
			(at 0 0 0)
			(show_name no)
			(do_not_autoplace no)
			(hide yes)
			(effects
				(font
					(size 1.27 1.27)
				)
			)
		)
		(property "Datasheet" ""
			(at 0 0 0)
			(show_name no)
			(do_not_autoplace no)
			(hide yes)
			(effects
				(font
					(size 1.27 1.27)
				)
			)
		)
		(property "Description" "Light emitting diode"
			(at 0 0 0)
			(show_name no)
			(do_not_autoplace no)
			(hide yes)
			(effects
				(font
					(size 1.27 1.27)
				)
			)
		)
		(property "Sim.Pins" "1=K 2=A"
			(at 0 0 0)
			(show_name no)
			(do_not_autoplace no)
			(hide yes)
			(effects
				(font
					(size 1.27 1.27)
				)
			)
		)
		(property "ki_keywords" "LED diode"
			(at 0 0 0)
			(show_name no)
			(do_not_autoplace no)
			(hide yes)
			(effects
				(font
					(size 1.27 1.27)
				)
			)
		)
		(property "ki_fp_filters" "LED* LED_SMD:* LED_THT:*"
			(at 0 0 0)
			(show_name no)
			(do_not_autoplace no)
			(hide yes)
			(effects
				(font
					(size 1.27 1.27)
				)
			)
		)
		(symbol "LED_0_1"
			(polyline
				(pts
					(xy -3.048 -0.762) (xy -4.572 -2.286) (xy -3.81 -2.286) (xy -4.572 -2.286) (xy -4.572 -1.524)
				)
				(stroke
					(width 0)
					(type default)
				)
				(fill
					(type none)
				)
			)
			(polyline
				(pts
					(xy -1.778 -0.762) (xy -3.302 -2.286) (xy -2.54 -2.286) (xy -3.302 -2.286) (xy -3.302 -1.524)
				)
				(stroke
					(width 0)
					(type default)
				)
				(fill
					(type none)
				)
			)
			(polyline
				(pts
					(xy -1.27 0) (xy 1.27 0)
				)
				(stroke
					(width 0)
					(type default)
				)
				(fill
					(type none)
				)
			)
			(polyline
				(pts
					(xy -1.27 -1.27) (xy -1.27 1.27)
				)
				(stroke
					(width 0.254)
					(type default)
				)
				(fill
					(type none)
				)
			)
			(polyline
				(pts
					(xy 1.27 -1.27) (xy 1.27 1.27) (xy -1.27 0) (xy 1.27 -1.27)
				)
				(stroke
					(width 0.254)
					(type default)
				)
				(fill
					(type none)
				)
			)
		)
		(symbol "LED_1_1"
			(pin passive line
				(at -3.81 0 0)
				(length 2.54)
				(name "K"
					(effects
						(font
							(size 1.27 1.27)
						)
					)
				)
				(number "1"
					(effects
						(font
							(size 1.27 1.27)
						)
					)
				)
			)
			(pin passive line
				(at 3.81 0 180)
				(length 2.54)
				(name "A"
					(effects
						(font
							(size 1.27 1.27)
						)
					)
				)
				(number "2"
					(effects
						(font
							(size 1.27 1.27)
						)
					)
				)
			)
		)
		(embedded_fonts no)
	)	"#;

const SYMBOL_NTC: &str = r#"	(symbol "Thermistor_NTC"
		(pin_numbers
			(hide yes)
		)
		(pin_names
			(offset 0)
		)
		(exclude_from_sim no)
		(in_bom yes)
		(on_board yes)
		(in_pos_files yes)
		(duplicate_pin_numbers_are_jumpers no)
		(property "Reference" "TH"
			(at -4.445 0 90)
			(show_name no)
			(do_not_autoplace no)
			(effects
				(font
					(size 1.27 1.27)
				)
			)
		)
		(property "Value" "Thermistor_NTC"
			(at 3.175 0 90)
			(show_name no)
			(do_not_autoplace no)
			(effects
				(font
					(size 1.27 1.27)
				)
			)
		)
		(property "Footprint" ""
			(at 0 1.27 0)
			(show_name no)
			(do_not_autoplace no)
			(hide yes)
			(effects
				(font
					(size 1.27 1.27)
				)
			)
		)
		(property "Datasheet" ""
			(at 0 1.27 0)
			(show_name no)
			(do_not_autoplace no)
			(hide yes)
			(effects
				(font
					(size 1.27 1.27)
				)
			)
		)
		(property "Description" "Temperature dependent resistor, negative temperature coefficient"
			(at 0 0 0)
			(show_name no)
			(do_not_autoplace no)
			(hide yes)
			(effects
				(font
					(size 1.27 1.27)
				)
			)
		)
		(property "ki_keywords" "thermistor NTC resistor sensor RTD"
			(at 0 0 0)
			(show_name no)
			(do_not_autoplace no)
			(hide yes)
			(effects
				(font
					(size 1.27 1.27)
				)
			)
		)
		(property "ki_fp_filters" "R_* RV_*"
			(at 0 0 0)
			(show_name no)
			(do_not_autoplace no)
			(hide yes)
			(effects
				(font
					(size 1.27 1.27)
				)
			)
		)
		(symbol "Thermistor_NTC_0_1"
			(arc
				(start -3.175 2.413)
				(mid -3.0506 2.3165)
				(end -3.048 2.159)
				(stroke
					(width 0)
					(type default)
				)
				(fill
					(type none)
				)
			)
			(arc
				(start -3.048 2.794)
				(mid -2.9736 2.9736)
				(end -2.794 3.048)
				(stroke
					(width 0)
					(type default)
				)
				(fill
					(type none)
				)
			)
			(arc
				(start -2.794 3.048)
				(mid -2.6144 2.9736)
				(end -2.54 2.794)
				(stroke
					(width 0)
					(type default)
				)
				(fill
					(type none)
				)
			)
			(arc
				(start -2.794 2.54)
				(mid -2.9736 2.6144)
				(end -3.048 2.794)
				(stroke
					(width 0)
					(type default)
				)
				(fill
					(type none)
				)
			)
			(arc
				(start -2.794 1.905)
				(mid -2.9736 1.9794)
				(end -3.048 2.159)
				(stroke
					(width 0)
					(type default)
				)
				(fill
					(type none)
				)
			)
			(arc
				(start -2.54 2.159)
				(mid -2.6144 1.9794)
				(end -2.794 1.905)
				(stroke
					(width 0)
					(type default)
				)
				(fill
					(type none)
				)
			)
			(arc
				(start -2.159 2.794)
				(mid -2.434 2.5608)
				(end -2.794 2.54)
				(stroke
					(width 0)
					(type default)
				)
				(fill
					(type none)
				)
			)
			(polyline
				(pts
					(xy -2.54 2.159) (xy -2.54 2.794)
				)
				(stroke
					(width 0)
					(type default)
				)
				(fill
					(type none)
				)
			)
			(polyline
				(pts
					(xy -2.54 -3.683) (xy -2.54 -1.397) (xy -2.794 -2.159) (xy -2.286 -2.159) (xy -2.54 -1.397) (xy -2.54 -1.651)
				)
				(stroke
					(width 0)
					(type default)
				)
				(fill
					(type outline)
				)
			)
			(polyline
				(pts
					(xy -1.778 2.54) (xy -1.778 1.524) (xy 1.778 -1.524) (xy 1.778 -2.54)
				)
				(stroke
					(width 0)
					(type default)
				)
				(fill
					(type none)
				)
			)
			(polyline
				(pts
					(xy -1.778 -1.397) (xy -1.778 -3.683) (xy -2.032 -2.921) (xy -1.524 -2.921) (xy -1.778 -3.683)
					(xy -1.778 -3.429)
				)
				(stroke
					(width 0)
					(type default)
				)
				(fill
					(type outline)
				)
			)
			(rectangle
				(start -1.016 2.54)
				(end 1.016 -2.54)
				(stroke
					(width 0.254)
					(type default)
				)
				(fill
					(type none)
				)
			)
		)
		(symbol "Thermistor_NTC_1_1"
			(pin passive line
				(at 0 3.81 270)
				(length 1.27)
				(name ""
					(effects
						(font
							(size 1.27 1.27)
						)
					)
				)
				(number "1"
					(effects
						(font
							(size 1.27 1.27)
						)
					)
				)
			)
			(pin passive line
				(at 0 -3.81 90)
				(length 1.27)
				(name ""
					(effects
						(font
							(size 1.27 1.27)
						)
					)
				)
				(number "2"
					(effects
						(font
							(size 1.27 1.27)
						)
					)
				)
			)
		)
		(embedded_fonts no)
	)	"#;

