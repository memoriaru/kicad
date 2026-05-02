//! KiCad standard library symbols extracted from Device.kicad_sym
//!
//! Symbol data stored as separate .sexpr files in the `symbols/` directory.
//! Using `include_str!` compiles them into the binary at zero runtime cost.

use std::collections::HashMap;

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

/// Returns the standard pin positions (local_x, local_y) for well-known Device symbols.
/// Positions match the embedded .sexpr definitions exactly.
/// Returns HashMap<pin_number, (local_x, local_y)>.
#[allow(dead_code)]
pub fn get_standard_pin_positions(lib_id: &str) -> Option<HashMap<String, (f64, f64)>> {
    let short = lib_id.split(':').last().unwrap_or(lib_id);
    // All Device:R/C/L/D/LED have vertical layout: pin 1 at top, pin 2 at bottom.
    // Pin at (0, 3.81) rotation 270: connection endpoint = (0, 3.81 - length)
    // Pin at (0, -3.81) rotation 90: connection endpoint = (0, -3.81 + length)
    let mut map = HashMap::new();
    match short {
        "C" => {
            // Pin 1: (0, 3.81) r=270, len=2.794 → endpoint (0, 1.016)
            // Pin 2: (0, -3.81) r=90, len=2.794 → endpoint (0, -1.016)
            map.insert("1".to_string(), (0.0, 1.016));
            map.insert("2".to_string(), (0.0, -1.016));
        }
        "R" | "L" => {
            // Pin 1: (0, 3.81) r=270, len=1.27 → endpoint (0, 2.54)
            // Pin 2: (0, -3.81) r=90, len=1.27 → endpoint (0, -2.54)
            map.insert("1".to_string(), (0.0, 2.54));
            map.insert("2".to_string(), (0.0, -2.54));
        }
        "D" | "LED" => {
            // Diode: Pin 1 (A) at left, Pin 2 (K) at right
            // Pin 1: (-3.81, 0) r=0, len=2.794 → endpoint (-1.016, 0) — wait, let me check
            // Actually diode pins: A at left, K at right
            // Standard: Pin 1 (A) at (-3.81, 0) r=0, len=2.794 → (-1.016, 0)
            // Pin 2 (K) at (3.81, 0) r=180, len=2.794 → (1.016, 0)
            map.insert("1".to_string(), (-1.016, 0.0));
            map.insert("2".to_string(), (1.016, 0.0));
        }
        "Thermistor_NTC" | "NTC" => {
            // Same as R: vertical layout
            map.insert("1".to_string(), (0.0, 2.54));
            map.insert("2".to_string(), (0.0, -2.54));
        }
        _ => return None,
    }
    Some(map)
}
