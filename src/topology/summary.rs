//! Topology summary structures for AI-friendly output
//!
//! These types provide a condensed, semantic view of circuit topology
//! optimized for AI understanding rather than complete graphical representation.

use std::collections::HashMap;

/// Helper function to escape a string for JSON5
fn json5_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

/// A power domain in the circuit
#[derive(Debug, Clone)]
pub struct PowerDomain {
    /// Name of the power net (e.g., "3.3V", "5V", "VCC")
    pub name: String,
    /// Voltage level if known
    pub voltage: Option<String>,
    /// Components powered by this domain
    pub consumers: Vec<String>,
    /// Components that provide this power
    pub sources: Vec<String>,
}

/// A signal path through the circuit
#[derive(Debug, Clone)]
pub struct SignalPath {
    /// Name/identifier of the signal
    pub name: String,
    /// Direction (input, output, bidirectional)
    pub direction: String,
    /// Starting point (component.pin)
    pub from: Option<String>,
    /// Ending point (component.pin)
    pub to: Option<String>,
    /// Intermediate components in the path
    pub via: Vec<String>,
    /// Pull-up/down resistor if present
    pub pullup: Option<String>,
    /// Series components (e.g., current limiting resistor)
    pub series: Vec<String>,
}

/// A functional module identified in the circuit
#[derive(Debug, Clone)]
pub struct FunctionalModule {
    /// Type of module (e.g., "i2c_pullup", "led_indicator", "decoupling")
    pub module_type: String,
    /// Human-readable purpose description
    pub purpose: String,
    /// Components that make up this module
    pub components: Vec<String>,
    /// Target component (for decoupling, etc.)
    pub target: Option<String>,
    /// Additional metadata
    pub metadata: HashMap<String, String>,
}

/// Component statistics
#[derive(Debug, Clone, Default)]
pub struct ComponentSummary {
    /// Total number of components
    pub total: usize,
    /// Count by component type
    pub by_type: HashMap<String, usize>,
    /// List of component references by type
    pub by_type_refs: HashMap<String, Vec<String>>,
}

/// The main topology summary for AI consumption
#[derive(Debug, Clone, Default)]
pub struct TopologySummary {
    /// Power domains identified in the circuit
    pub power_domains: Vec<PowerDomain>,
    /// Ground nets and their connections
    pub ground_nets: Vec<String>,
    /// Signal paths through the circuit
    pub signal_paths: Vec<SignalPath>,
    /// Functional modules identified
    pub modules: Vec<FunctionalModule>,
    /// Connection adjacency list (simplified)
    /// Format: "Component1" -> ["Component2", "Component3", ...]
    pub connections: HashMap<String, Vec<String>>,
    /// Component statistics
    pub component_summary: ComponentSummary,
    /// Net to component mapping
    pub net_components: HashMap<String, Vec<String>>,
    /// Warnings about incomplete or problematic schematic data
    pub warnings: Vec<String>,
}

impl TopologySummary {
    /// Create a new empty topology summary
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if the summary is empty
    pub fn is_empty(&self) -> bool {
        self.power_domains.is_empty()
            && self.ground_nets.is_empty()
            && self.signal_paths.is_empty()
            && self.modules.is_empty()
            && self.connections.is_empty()
            && self.warnings.is_empty()
    }

    /// Generate a human-readable summary
    pub fn to_text_summary(&self) -> String {
        let mut output = String::new();

        // Power domains
        if !self.power_domains.is_empty() {
            output.push_str("=== Power Domains ===\n");
            for pd in &self.power_domains {
                let voltage = pd.voltage.as_deref().unwrap_or("?");
                output.push_str(&format!("  {} ({}V): {}\n", pd.name, voltage, pd.consumers.join(", ")));
            }
            output.push('\n');
        }

        // Ground nets
        if !self.ground_nets.is_empty() {
            output.push_str("=== Ground Nets ===\n");
            output.push_str(&format!("  {}\n\n", self.ground_nets.join(", ")));
        }

        // Signal paths
        if !self.signal_paths.is_empty() {
            output.push_str("=== Signal Paths ===\n");
            for sp in &self.signal_paths {
                output.push_str(&format!("  {} ({}): ", sp.name, sp.direction));
                if let Some(from) = &sp.from {
                    output.push_str(from);
                }
                if !sp.via.is_empty() {
                    output.push_str(&format!(" -> {}", sp.via.join(" -> ")));
                }
                if let Some(to) = &sp.to {
                    output.push_str(&format!(" -> {}", to));
                }
                output.push('\n');
            }
            output.push('\n');
        }

        // Modules
        if !self.modules.is_empty() {
            output.push_str("=== Functional Modules ===\n");
            for m in &self.modules {
                output.push_str(&format!(
                    "  {} ({}): {}\n",
                    m.module_type,
                    m.purpose,
                    m.components.join(", ")
                ));
            }
            output.push('\n');
        }

        // Component summary
        output.push_str("=== Components ===\n");
        output.push_str(&format!("  Total: {}\n", self.component_summary.total));
        for (kind, count) in &self.component_summary.by_type {
            output.push_str(&format!("  {}: {}\n", kind, count));
        }

        output
    }

    /// Generate JSON5 output for AI consumption
    pub fn to_json5(&self) -> String {
        let mut output = String::new();
        output.push_str("{\n");

        // Power domains
        output.push_str("  // 电源域\n");
        output.push_str("  power_domains: [\n");
        for (i, pd) in self.power_domains.iter().enumerate() {
            output.push_str("    {\n");
            output.push_str(&format!("      name: \"{}\",\n", json5_escape(&pd.name)));
            if let Some(v) = &pd.voltage {
                output.push_str(&format!("      voltage: \"{}\",\n", json5_escape(v)));
            }
            output.push_str(&format!(
                "      consumers: [{}],\n",
                pd.consumers
                    .iter()
                    .map(|c| format!("\"{}\"", json5_escape(c)))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            output.push_str(&format!(
                "      sources: [{}]\n",
                pd.sources
                    .iter()
                    .map(|c| format!("\"{}\"", json5_escape(c)))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            output.push_str("    }");
            if i < self.power_domains.len() - 1 {
                output.push(',');
            }
            output.push('\n');
        }
        output.push_str("  ],\n\n");

        // Ground nets
        output.push_str("  // 地网络\n");
        output.push_str(&format!(
            "  ground_nets: [{}],\n\n",
            self.ground_nets
                .iter()
                .map(|n| format!("\"{}\"", json5_escape(n)))
                .collect::<Vec<_>>()
                .join(", ")
        ));

        // Signal paths
        output.push_str("  // 信号路径\n");
        output.push_str("  signal_paths: [\n");
        for (i, sp) in self.signal_paths.iter().enumerate() {
            output.push_str("    {\n");
            output.push_str(&format!("      name: \"{}\",\n", json5_escape(&sp.name)));
            output.push_str(&format!("      direction: \"{}\",\n", json5_escape(&sp.direction)));
            if let Some(from) = &sp.from {
                output.push_str(&format!("      from: \"{}\",\n", json5_escape(from)));
            }
            if let Some(to) = &sp.to {
                output.push_str(&format!("      to: \"{}\",\n", json5_escape(to)));
            }
            output.push_str(&format!(
                "      via: [{}],\n",
                sp.via
                    .iter()
                    .map(|v| format!("\"{}\"", json5_escape(v)))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            if let Some(pu) = &sp.pullup {
                output.push_str(&format!("      pullup: \"{}\",\n", json5_escape(pu)));
            }
            output.push_str(&format!(
                "      series: [{}]\n",
                sp.series
                    .iter()
                    .map(|s| format!("\"{}\"", json5_escape(s)))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            output.push_str("    }");
            if i < self.signal_paths.len() - 1 {
                output.push(',');
            }
            output.push('\n');
        }
        output.push_str("  ],\n\n");

        // Modules
        output.push_str("  // 功能模块\n");
        output.push_str("  modules: [\n");
        for (i, m) in self.modules.iter().enumerate() {
            output.push_str("    {\n");
            output.push_str(&format!("      type: \"{}\",\n", json5_escape(&m.module_type)));
            output.push_str(&format!("      purpose: \"{}\",\n", json5_escape(&m.purpose)));
            output.push_str(&format!(
                "      components: [{}]",
                m.components
                    .iter()
                    .map(|c| format!("\"{}\"", json5_escape(c)))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            if let Some(t) = &m.target {
                output.push_str(&format!(",\n      target: \"{}\"", json5_escape(t)));
            }
            output.push('\n');
            output.push_str("    }");
            if i < self.modules.len() - 1 {
                output.push(',');
            }
            output.push('\n');
        }
        output.push_str("  ],\n\n");

        // Component summary
        output.push_str("  // 元件统计\n");
        output.push_str("  component_summary: {\n");
        output.push_str(&format!("    total: {},\n", self.component_summary.total));
        output.push_str("    by_type: {");
        let by_type_items: Vec<String> = self
            .component_summary
            .by_type
            .iter()
            .map(|(k, v)| format!("\n      \"{}\": {}", json5_escape(k), v))
            .collect();
        output.push_str(&by_type_items.join(","));
        if !by_type_items.is_empty() {
            output.push('\n');
            output.push_str("    ");
        }
        output.push_str("}\n");
        output.push_str("  },\n\n");

        // Net to components mapping (simplified)
        output.push_str("  // 网络连接\n");
        output.push_str("  net_components: {\n");
        let mut net_items: Vec<(&String, &Vec<String>)> = self.net_components.iter().collect();
        net_items.sort_by_key(|(k, _)| *k);
        for (i, (net, components)) in net_items.iter().enumerate() {
            output.push_str(&format!(
                "    \"{}\": [{}]",
                json5_escape(net),
                components
                    .iter()
                    .map(|c| format!("\"{}\"", json5_escape(c)))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            if i < net_items.len() - 1 {
                output.push(',');
            }
            output.push('\n');
        }
        output.push_str("  }\n");

        // Warnings
        if !self.warnings.is_empty() {
            output.push_str(",\n\n  // 警告信息\n");
            output.push_str("  warnings: [\n");
            for (i, warning) in self.warnings.iter().enumerate() {
                output.push_str(&format!("    \"{}\"", json5_escape(warning)));
                if i < self.warnings.len() - 1 {
                    output.push(',');
                }
                output.push('\n');
            }
            output.push_str("  ]\n");
        }

        output.push_str("}\n");
        output
    }
}

/// Builder for creating TopologySummary
#[derive(Debug, Default)]
pub struct TopologySummaryBuilder {
    summary: TopologySummary,
}

impl TopologySummaryBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a power domain
    pub fn add_power_domain(&mut self, domain: PowerDomain) {
        self.summary.power_domains.push(domain);
    }

    /// Add a ground net
    pub fn add_ground_net(&mut self, name: impl Into<String>) {
        self.summary.ground_nets.push(name.into());
    }

    /// Add a signal path
    pub fn add_signal_path(&mut self, path: SignalPath) {
        self.summary.signal_paths.push(path);
    }

    /// Add a functional module
    pub fn add_module(&mut self, module: FunctionalModule) {
        self.summary.modules.push(module);
    }

    /// Add a connection between components
    pub fn add_connection(&mut self, from: impl Into<String>, to: impl Into<String>) {
        self.summary
            .connections
            .entry(from.into())
            .or_default()
            .push(to.into());
    }

    /// Set all connections at once
    pub fn set_connections(&mut self, connections: HashMap<String, Vec<String>>) {
        self.summary.connections = connections;
    }

    /// Set net to components mapping
    pub fn set_net_components(&mut self, net: impl Into<String>, components: Vec<String>) {
        self.summary.net_components.insert(net.into(), components);
    }

    /// Set component summary
    pub fn set_component_summary(&mut self, summary: ComponentSummary) {
        self.summary.component_summary = summary;
    }

    /// Add a warning about schematic issues
    pub fn add_warning(&mut self, warning: impl Into<String>) {
        self.summary.warnings.push(warning.into());
    }

    /// Build the final summary
    pub fn build(self) -> TopologySummary {
        self.summary
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_summary() {
        let summary = TopologySummary::new();
        assert!(summary.is_empty());
    }

    #[test]
    fn test_text_summary() {
        let mut builder = TopologySummaryBuilder::new();

        builder.add_power_domain(PowerDomain {
            name: "3.3V".to_string(),
            voltage: Some("3.3".to_string()),
            consumers: vec!["U1".to_string(), "R1".to_string()],
            sources: vec![],
        });

        builder.add_ground_net("GND");

        let summary = builder.build();
        let text = summary.to_text_summary();
        assert!(text.contains("3.3V"));
        assert!(text.contains("GND"));
    }
}
