//! KiCad standard library symbols extracted from Device.kicad_sym
//!
//! Symbol data stored as separate .sexpr files in the `symbols/` directory.
//! Using `include_str!` compiles them into the binary at zero runtime cost.



/// Returns the standard symbol S-expression for well-known Device library components.
/// The returned string uses the short symbol name (e.g., "R" not "Device:R").
pub fn get_standard_symbol(lib_id: &str) -> Option<&'static str> {
    let short = lib_id.split(':').last().unwrap_or(lib_id);
    match short {
        "R" => Some(include_str!("symbols/R.sexpr")),
        "C" => Some(include_str!("symbols/C.sexpr")),
        "L" => Some(include_str!("symbols/L.sexpr")),
        "D" => Some(include_str!("symbols/D.sexpr")),
        "LED" => Some(include_str!("symbols/LED.sexpr")),
        "Thermistor_NTC" | "NTC" => Some(include_str!("symbols/NTC.sexpr")),
        _ => None,
    }
}


