//! Component classification based on library ID
//!
//! This module provides heuristics for classifying components
//! based on their KiCad library identifiers.

use super::types::ComponentKind;

/// Classify a component based on its library ID
///
/// # Arguments
/// * `lib_id` - The KiCad library ID (e.g., "Device:R", "MCU_ST_STM32:STM32F103")
///
/// # Returns
/// The classified component kind
pub fn classify_component(lib_id: &str) -> ComponentKind {
    // Split library:component_name
    let (lib, name) = if let Some(idx) = lib_id.find(':') {
        (&lib_id[..idx], &lib_id[idx + 1..])
    } else {
        ("", lib_id)
    };

    let lib_lower = lib.to_lowercase();
    let name_lower = name.to_lowercase();

    // Check library prefix first (higher priority)
    match lib_lower.as_str() {
        // Resistors
        "device" if name_lower.starts_with('r') || name_lower.contains("resistor") => {
            return ComponentKind::Resistor
        }
        // Capacitors
        "device" if name_lower.starts_with('c') || name_lower.contains("capacitor") => {
            return ComponentKind::Capacitor
        }
        // Inductors
        "device" if name_lower.starts_with('l') && !name_lower.contains("led") => {
            return ComponentKind::Inductor
        }
        // Diodes and LEDs
        "device"
            if name_lower.starts_with('d')
                || name_lower.contains("led")
                || name_lower.contains("diode") =>
        {
            return ComponentKind::Diode
        }
        // Transistors
        "device"
            if name_lower.starts_with('q')
                || name_lower.contains("transistor")
                || name_lower.contains("mosfet")
                || name_lower.contains("fet")
                || name_lower.contains("bjt") =>
        {
            return ComponentKind::Transistor
        }
        // Crystal/Oscillator
        "device" if name_lower.contains("crystal") || name_lower.contains("oscillator") => {
            return ComponentKind::Crystal
        }
        // Fuses
        "device" if name_lower.contains("fuse") => return ComponentKind::Fuse,
        // Switches
        "device" if name_lower.starts_with("sw_") => return ComponentKind::Switch,
        _ => {}
    }

    // Check library name patterns
    match lib_lower.as_str() {
        // MCU and IC libraries
        "mcu" | "mcu_st_stm32" | "mcu_nxp" | "mcu_microchip" | "mcu_espressif" | "mcu_esp32"
        | "ic" | "cpu" | "dsp" | "fpga" => {
            return ComponentKind::Ic;
        }
        // Connector libraries
        "connector" | "connector_generic" => {
            return ComponentKind::Connector;
        }
        // Transistor libraries
        "transistor" | "transistor_fet" | "transistor_bjt" => {
            return ComponentKind::Transistor;
        }
        // Power/regulator libraries
        "regulator" | "regulator_linear" | "regulator_switching" => {
            return ComponentKind::Power;
        }
        // Diode libraries
        "diode" => {
            return ComponentKind::Diode;
        }
        // Crystal/Oscillator libraries
        "crystal" | "oscillator" => {
            return ComponentKind::Crystal;
        }
        // Switch libraries
        "switch" => {
            return ComponentKind::Switch;
        }
        _ => {}
    }

    // Check component name patterns (fallback)
    if name_lower.starts_with('r') && is_reference_pattern(&name_lower, 'r') {
        return ComponentKind::Resistor;
    }
    if name_lower.starts_with('c') && is_reference_pattern(&name_lower, 'c') {
        return ComponentKind::Capacitor;
    }
    if name_lower.starts_with('l') && is_reference_pattern(&name_lower, 'l') {
        return ComponentKind::Inductor;
    }
    if name_lower.starts_with('d') && is_reference_pattern(&name_lower, 'd') {
        return ComponentKind::Diode;
    }
    if name_lower.starts_with('q') && is_reference_pattern(&name_lower, 'q') {
        return ComponentKind::Transistor;
    }
    if name_lower.starts_with('u') && is_reference_pattern(&name_lower, 'u') {
        return ComponentKind::Ic;
    }
    if name_lower.starts_with('j') && is_reference_pattern(&name_lower, 'j') {
        return ComponentKind::Connector;
    }
    if name_lower.starts_with('y') && is_reference_pattern(&name_lower, 'y') {
        return ComponentKind::Crystal;
    }

    // Keyword-based detection
    if name_lower.contains("resistor") || name_lower.contains("_r_") {
        return ComponentKind::Resistor;
    }
    if name_lower.contains("capacitor") || name_lower.contains("_c_") {
        return ComponentKind::Capacitor;
    }
    if name_lower.contains("inductor") || name_lower.contains("_l_") {
        return ComponentKind::Inductor;
    }
    if name_lower.contains("diode") || name_lower.contains("led") {
        return ComponentKind::Diode;
    }
    if name_lower.contains("transistor")
        || name_lower.contains("mosfet")
        || name_lower.contains("fet")
        || name_lower.contains("bjt")
    {
        return ComponentKind::Transistor;
    }
    if name_lower.contains("regulator") || name_lower.contains("ldo") || name_lower.contains("dcdc")
    {
        return ComponentKind::Power;
    }
    if name_lower.contains("connector") || name_lower.contains("header") {
        return ComponentKind::Connector;
    }
    if name_lower.contains("crystal") || name_lower.contains("oscillator") {
        return ComponentKind::Crystal;
    }
    if name_lower.contains("switch") || name_lower.contains("button") {
        return ComponentKind::Switch;
    }
    if name_lower.contains("fuse") {
        return ComponentKind::Fuse;
    }

    ComponentKind::Unknown
}

/// Check if the name follows a reference designator pattern (e.g., R, R_*, R_0805)
fn is_reference_pattern(name: &str, prefix: char) -> bool {
    let name_lower = name.to_lowercase();
    // Exact match
    if name_lower.len() == 1 && name_lower.starts_with(prefix) {
        return true;
    }
    // Pattern: X_*
    if name_lower.starts_with(prefix) && name_lower.chars().nth(1) == Some('_') {
        return true;
    }
    false
}

/// Classify a net based on its name
///
/// # Arguments
/// * `net_name` - The net name (e.g., "VCC", "GND", "SDA")
///
/// # Returns
/// The classified net kind
pub fn classify_net(net_name: &str) -> super::types::NetKind {
    let name_upper = net_name.to_uppercase();

    // Ground patterns (check first, as some may contain VCC-like patterns)
    let ground_patterns = [
        "GND", "AGND", "DGND", "GNDA", "GNDD", "VSS", "PGND", "SGND", "CHASSIS", "EARTH",
    ];
    for pattern in ground_patterns {
        if name_upper == pattern || name_upper.starts_with(&format!("{}_", pattern)) {
            return super::types::NetKind::Ground;
        }
    }

    // Power patterns
    let power_patterns = [
        "VCC", "VDD", "VIN", "VOUT", "VBUS", "VBAT", "VREF", "AVCC", "AVDD", "DVCC", "DVDD",
        "PVCC", "PVDD",
    ];
    for pattern in power_patterns {
        if name_upper == pattern || name_upper.starts_with(&format!("{}_", pattern)) {
            return super::types::NetKind::Power;
        }
    }

    // Voltage patterns (3V3, 5V, 12V, +3.3V, etc.)
    if name_upper.contains("V") {
        // Check for voltage patterns like "3V3", "5V", "+12V"
        let voltage_patterns = ["3V3", "5V", "12V", "24V", "9V", "3.3V", "1V8", "2V5", "+3V3"];
        for pattern in voltage_patterns {
            if name_upper.contains(pattern) {
                return super::types::NetKind::Power;
            }
        }
        // Pattern: +XV or -XV
        if name_upper.starts_with('+') || name_upper.starts_with('-') {
            return super::types::NetKind::Power;
        }
    }

    // Bus patterns (contains brackets or common bus prefixes)
    if net_name.contains('[')
        || net_name.contains('{')
        || name_upper.starts_with("BUS_")
        || name_upper.ends_with("_BUS")
    {
        return super::types::NetKind::Bus;
    }

    // Default to signal
    super::types::NetKind::Signal
}

/// Extract voltage from a power net name
///
/// # Examples
/// - "3V3" -> "3.3"
/// - "5V" -> "5"
/// - "+12V" -> "12"
/// - "VCC" -> None
pub fn extract_voltage(net_name: &str) -> Option<String> {
    let name = net_name.replace('+', "").replace('-', "");

    // Pattern: XvY (e.g., 3v3, 1v8)
    if let Some(caps) = regex_match_pattern(&name, r"(\d+)[vV](\d+)") {
        return Some(caps);
    }

    // Pattern: X.YV (e.g., 3.3V, 1.8V)
    if let Some(caps) = regex_match_pattern(&name, r"(\d+\.\d+)[vV]") {
        return Some(caps);
    }

    // Pattern: XV (e.g., 5V, 12V)
    if let Some(caps) = regex_match_pattern(&name, r"(\d+)[vV]$") {
        return Some(caps);
    }

    None
}

/// Simple regex-like pattern matching without regex dependency
fn regex_match_pattern(text: &str, pattern: &str) -> Option<String> {
    let text_upper = text.to_uppercase();

    // Handle XvY pattern (e.g., 3v3)
    if pattern == r"(\d+)[vV](\d+)" {
        let bytes = text_upper.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i].is_ascii_digit() {
                let start = i;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                if i < bytes.len() && bytes[i] == b'V' {
                    i += 1;
                    let volt_start = i;
                    while i < bytes.len() && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                    if i > volt_start {
                        let major = &text[start..volt_start - 1];
                        let minor = &text[volt_start..i];
                        return Some(format!("{}.{}", major, minor));
                    }
                }
            } else {
                i += 1;
            }
        }
    }

    // Handle X.YV pattern (e.g., 3.3V)
    if pattern == r"(\d+\.\d+)[vV]" {
        let bytes = text_upper.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i].is_ascii_digit() {
                let start = i;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                if i < bytes.len() && bytes[i] == b'.' {
                    i += 1;
                    while i < bytes.len() && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                    if i < bytes.len() && bytes[i] == b'V' {
                        return Some(text[start..i].to_string());
                    }
                }
            } else {
            i += 1;
        }
        }
    }

    // Handle XV pattern (e.g., 5V)
    if pattern == r"(\d+)[vV]$" {
        let upper = text_upper;
        if let Some(pos) = upper.find('V') {
            if pos == upper.len() - 1 {
                let before = &text[..pos];
                if before.chars().all(|c| c.is_ascii_digit()) {
                    return Some(before.to_string());
                }
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_resistor() {
        assert_eq!(classify_component("Device:R"), ComponentKind::Resistor);
        assert_eq!(classify_component("Device:R_0805"), ComponentKind::Resistor);
        assert_eq!(classify_component("Device:Resistor"), ComponentKind::Resistor);
    }

    #[test]
    fn test_classify_capacitor() {
        assert_eq!(classify_component("Device:C"), ComponentKind::Capacitor);
        assert_eq!(classify_component("Device:C_0805"), ComponentKind::Capacitor);
        assert_eq!(
            classify_component("Device:Capacitor"),
            ComponentKind::Capacitor
        );
    }

    #[test]
    fn test_classify_ic() {
        assert_eq!(
            classify_component("MCU_ST_STM32:STM32F103"),
            ComponentKind::Ic
        );
        assert_eq!(
            classify_component("MCU_ESP32:ESP32-WROOM"),
            ComponentKind::Ic
        );
    }

    #[test]
    fn test_classify_connector() {
        assert_eq!(
            classify_component("Connector:Conn_01x04"),
            ComponentKind::Connector
        );
        assert_eq!(
            classify_component("Connector_Generic:Conn_01x02"),
            ComponentKind::Connector
        );
    }

    #[test]
    fn test_classify_power() {
        assert_eq!(
            classify_component("Regulator_Linear:AMS1117"),
            ComponentKind::Power
        );
    }

    #[test]
    fn test_classify_net_power() {
        use super::super::types::NetKind;
        assert_eq!(classify_net("VCC"), NetKind::Power);
        assert_eq!(classify_net("3V3"), NetKind::Power);
        assert_eq!(classify_net("5V"), NetKind::Power);
        assert_eq!(classify_net("+12V"), NetKind::Power);
        assert_eq!(classify_net("VBUS"), NetKind::Power);
    }

    #[test]
    fn test_classify_net_ground() {
        use super::super::types::NetKind;
        assert_eq!(classify_net("GND"), NetKind::Ground);
        assert_eq!(classify_net("AGND"), NetKind::Ground);
        assert_eq!(classify_net("DGND"), NetKind::Ground);
        assert_eq!(classify_net("VSS"), NetKind::Ground);
    }

    #[test]
    fn test_classify_net_signal() {
        use super::super::types::NetKind;
        assert_eq!(classify_net("SDA"), NetKind::Signal);
        assert_eq!(classify_net("SCL"), NetKind::Signal);
        assert_eq!(classify_net("GPIO1"), NetKind::Signal);
    }

    #[test]
    fn test_extract_voltage() {
        assert_eq!(extract_voltage("3V3"), Some("3.3".to_string()));
        assert_eq!(extract_voltage("5V"), Some("5".to_string()));
        assert_eq!(extract_voltage("+12V"), Some("12".to_string()));
        assert_eq!(extract_voltage("VCC"), None);
    }
}
