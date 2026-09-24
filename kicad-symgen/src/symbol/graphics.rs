use crate::model::*;

/// Body graphic primitive for symbol generation
#[derive(Debug, Clone)]
pub enum BodyGraphic {
    /// Standard rectangle body (default for most ICs)
    Rectangle,
    /// Op-amp / comparator triangle body
    Triangle,
    /// MCU with functional partition blocks
    FunctionBlock { sections: Vec<String> },
}

/// Infer body graphic from spec pin names and count
pub fn infer_body_graphic(spec: &SymbolSpec) -> BodyGraphic {
    let pin_names: Vec<String> = spec.pins.iter().map(|p| p.name.to_uppercase()).collect();
    infer_body_graphic_from_names(&pin_names)
}

/// Infer body graphic from a list of pin names (uppercased) and total pin count.
pub fn infer_body_graphic_from_names(pin_names: &[String]) -> BodyGraphic {
    if is_opamp(pin_names) {
        return BodyGraphic::Triangle;
    }
    if pin_names.len() >= 16 && has_mcu_peripherals(pin_names) {
        let sections = extract_mcu_sections(pin_names);
        if !sections.is_empty() {
            return BodyGraphic::FunctionBlock { sections };
        }
    }
    BodyGraphic::Rectangle
}

fn is_opamp(pin_names: &[String]) -> bool {
    let has_non_inv = pin_names.iter().any(|n| {
        n.starts_with("+IN")
            || n.starts_with("IN+")
            || n.as_str() == "NON_INV"
            || n.starts_with("VP")
    });
    let has_inv = pin_names.iter().any(|n| {
        n.starts_with("-IN")
            || n.starts_with("IN-")
            || n.as_str() == "INV"
            || n.starts_with("VN")
            || n.starts_with("IN-")
    });
    let has_out = pin_names
        .iter()
        .any(|n| n.starts_with("OUT") || n.starts_with("VOUT") || n.starts_with("OUTPUT"));
    (has_non_inv || has_inv) && has_out && pin_names.len() <= 8
}

fn has_mcu_peripherals(pin_names: &[String]) -> bool {
    let peripheral_keywords = [
        "PA", "PB", "PC", "PD", "PE", "PF", "PG", "TX", "RX", "SDA", "SCL", "MOSI", "MISO", "SCK",
        "CS", "ADC", "TIM", "PWM", "UART", "SPI", "I2C", "GPIO", "BOOT", "RST", "NRST", "SWD",
        "SWCLK", "SWDIO",
    ];
    let match_count = pin_names
        .iter()
        .filter(|name| peripheral_keywords.iter().any(|kw| name.starts_with(kw)))
        .count();
    match_count >= 4
}

fn extract_mcu_sections(pin_names: &[String]) -> Vec<String> {
    let mut sections = Vec::new();

    let has_power = pin_names.iter().any(|n| {
        n.starts_with("VDD") || n.starts_with("VCC") || n.starts_with("GND") || n.starts_with("VSS")
    });
    if has_power {
        sections.push("PWR".to_string());
    }

    let port_names: [&str; 8] = ["PA", "PB", "PC", "PD", "PE", "PF", "PG", "PH"];
    let ports_used: Vec<&str> = port_names
        .iter()
        .filter(|&&port| pin_names.iter().any(|n| n.starts_with(port)))
        .copied()
        .collect();
    if !ports_used.is_empty() {
        sections.push(ports_used.join("/"));
    }

    let has_comms = pin_names.iter().any(|n| {
        n.starts_with("TX")
            || n.starts_with("RX")
            || n.starts_with("SDA")
            || n.starts_with("SCL")
            || n.starts_with("MOSI")
            || n.starts_with("MISO")
    });
    if has_comms {
        sections.push("COM".to_string());
    }

    let has_debug = pin_names.iter().any(|n| {
        n.starts_with("SWD")
            || n.starts_with("SWCLK")
            || n.starts_with("SWDIO")
            || n.starts_with("JTAG")
            || n.starts_with("BOOT")
    });
    if has_debug {
        sections.push("DBG".to_string());
    }

    let has_analog = pin_names
        .iter()
        .any(|n| n.starts_with("ADC") || n.starts_with("DAC") || n.starts_with("COMP"));
    if has_analog {
        sections.push("ANA".to_string());
    }

    sections
}
