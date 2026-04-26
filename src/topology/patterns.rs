//! Circuit pattern recognition for module identification
//!
//! This module defines patterns for common circuit sub-modules
//! and provides pattern matching functionality.

use super::types::ComponentKind;

/// A pattern for identifying functional modules in circuits
#[derive(Debug, Clone)]
pub struct ModulePattern {
    /// Pattern name
    pub name: String,
    /// Human-readable description
    pub description: String,
    /// Required component types
    pub component_types: Vec<ComponentKind>,
    /// Connection pattern description
    pub connection_pattern: ConnectionPattern,
    /// Keywords that suggest this pattern
    pub keywords: Vec<String>,
}

/// Describes how components should be connected
#[derive(Debug, Clone)]
pub enum ConnectionPattern {
    /// Components connected in series
    Series,
    /// Components connected in parallel
    Parallel,
    /// Components connected to a common net
    CommonNet(String),
    /// Custom pattern with description
    Custom(String),
}

/// Get built-in circuit patterns
pub fn builtin_patterns() -> Vec<ModulePattern> {
    vec![
        // I2C Pull-up Pattern
        ModulePattern {
            name: "i2c_pullup".into(),
            description: "I2C 总线上拉电阻".into(),
            component_types: vec![ComponentKind::Resistor],
            connection_pattern: ConnectionPattern::Custom(
                "VCC - R - Signal (SDA/SCL)".into(),
            ),
            keywords: vec!["SDA".into(), "SCL".into(), "I2C".into()],
        },
        // LED Indicator Pattern
        ModulePattern {
            name: "led_indicator".into(),
            description: "LED 指示灯电路".into(),
            component_types: vec![ComponentKind::Diode, ComponentKind::Resistor],
            connection_pattern: ConnectionPattern::Series,
            keywords: vec!["LED".into(), "INDICATOR".into()],
        },
        // Power Decoupling Pattern
        ModulePattern {
            name: "power_decoupling".into(),
            description: "电源去耦电容".into(),
            component_types: vec![ComponentKind::Capacitor],
            connection_pattern: ConnectionPattern::Custom("VCC - C - GND".into()),
            keywords: vec!["DECOUPLING".into(), "BYPASS".into()],
        },
        // Crystal Oscillator Pattern
        ModulePattern {
            name: "crystal_oscillator".into(),
            description: "晶振电路".into(),
            component_types: vec![ComponentKind::Crystal, ComponentKind::Capacitor],
            connection_pattern: ConnectionPattern::Custom(
                "Crystal with load capacitors to ground".into(),
            ),
            keywords: vec!["XTAL".into(), "OSC".into(), "CRYSTAL".into()],
        },
        // Voltage Regulator Pattern
        ModulePattern {
            name: "voltage_regulator".into(),
            description: "电压调节器电路".into(),
            component_types: vec![ComponentKind::Power, ComponentKind::Capacitor],
            connection_pattern: ConnectionPattern::Custom(
                "VIN - Regulator - VOUT with input/output capacitors".into(),
            ),
            keywords: vec!["LDO".into(), "REGULATOR".into(), "DCDC".into()],
        },
        // Reset Circuit Pattern
        ModulePattern {
            name: "reset_circuit".into(),
            description: "复位电路".into(),
            component_types: vec![ComponentKind::Resistor, ComponentKind::Capacitor],
            connection_pattern: ConnectionPattern::Custom("RC reset with pullup".into()),
            keywords: vec!["RESET".into(), "NRST".into(), "RST".into()],
        },
        // USB ESD Protection Pattern
        ModulePattern {
            name: "usb_esd".into(),
            description: "USB ESD 保护".into(),
            component_types: vec![ComponentKind::Diode],
            connection_pattern: ConnectionPattern::CommonNet("USB".into()),
            keywords: vec!["USB".into(), "ESD".into(), "TVS".into()],
        },
    ]
}

/// Pattern matcher for circuit modules
pub struct PatternMatcher {
    patterns: Vec<ModulePattern>,
}

impl PatternMatcher {
    /// Create a new pattern matcher with built-in patterns
    pub fn new() -> Self {
        Self {
            patterns: builtin_patterns(),
        }
    }

    /// Add a custom pattern
    pub fn add_pattern(&mut self, pattern: ModulePattern) {
        self.patterns.push(pattern);
    }

    /// Get all patterns
    pub fn patterns(&self) -> &[ModulePattern] {
        &self.patterns
    }

    /// Find patterns that match the given keywords
    pub fn match_keywords(&self, keywords: &[&str]) -> Vec<&ModulePattern> {
        self.patterns
            .iter()
            .filter(|p| {
                keywords.iter().any(|kw| {
                    p.keywords
                        .iter()
                        .any(|pkw| pkw.to_uppercase().contains(&kw.to_uppercase()))
                })
            })
            .collect()
    }
}

impl Default for PatternMatcher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_patterns() {
        let patterns = builtin_patterns();
        assert!(!patterns.is_empty());
    }

    #[test]
    fn test_pattern_matcher() {
        let matcher = PatternMatcher::new();
        let patterns = matcher.match_keywords(&["SDA", "I2C"]);
        assert!(!patterns.is_empty());

        let i2c_pattern = patterns.iter().find(|p| p.name == "i2c_pullup");
        assert!(i2c_pattern.is_some());
    }
}
