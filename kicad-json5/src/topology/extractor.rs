//! Topology extractor for circuit analysis
//!
//! This module extracts semantic topology from KiCad schematic IR,
//! building connection graphs and identifying functional modules.

use std::collections::{HashMap, HashSet};

use crate::ir::Schematic;

use super::classify::{classify_component, classify_net, extract_voltage};
use super::connectivity::Connectivity;
use super::summary::{
    ComponentSummary, FunctionalModule, PowerDomain, SignalPath, TopologySummary,
    TopologySummaryBuilder,
};
use super::types::{ComponentInfo, ComponentKind, NetKind, PinConnection};

/// Topology extractor for circuit analysis
pub struct TopologyExtractor<'a> {
    schematic: &'a Schematic,
    /// Component info cache
    components: HashMap<String, ComponentInfo>,
    /// Net to connected components mapping
    net_components: HashMap<String, Vec<PinConnection>>,
    /// Component adjacency (which components are connected)
    adjacency: HashMap<String, HashSet<String>>,
    /// Power symbols: reference -> (net_name, position_xy)
    power_symbols: HashMap<String, (String, (f64, f64))>,
    /// Wire-based connectivity
    connectivity: Connectivity,
}

impl<'a> TopologyExtractor<'a> {
    /// Create a new topology extractor
    pub fn new(schematic: &'a Schematic) -> Self {
        Self {
            schematic,
            components: HashMap::new(),
            net_components: HashMap::new(),
            adjacency: HashMap::new(),
            power_symbols: HashMap::new(),
            connectivity: Connectivity::new(),
        }
    }

    /// Extract topology summary from the schematic
    pub fn extract(&mut self) -> TopologySummary {
        // Phase 0: Build wire-based connectivity
        self.connectivity = Connectivity::build(self.schematic);

        // Phase 1: Build component info
        self.build_component_info();

        // Phase 2: Build connection graph
        self.build_connection_graph();

        // Phase 3: Build summary
        self.build_summary()
    }

    /// Build component information cache
    fn build_component_info(&mut self) {
        for comp in &self.schematic.components {
            let kind = classify_component(&comp.lib_id);
            let mut connected_nets = HashMap::new();

            // Check if this is a power symbol (lib_id starts with "power:")
            let is_power_symbol = comp.lib_id.to_lowercase().starts_with("power:");

            // Extract connected nets from pins
            for pin in &comp.pins {
                if let Some(net_name) = &pin.net_name {
                    connected_nets.insert(pin.number.clone(), net_name.clone());
                }
            }

            // For power symbols, store them separately
            if is_power_symbol {
                let net_name = if !comp.value.is_empty() {
                    comp.value.clone()
                } else {
                    // Extract from lib_id (e.g., "power:GND" -> "GND")
                    comp.lib_id.split(':').last().unwrap_or(&comp.lib_id).to_string()
                };
                let position = (comp.position.0, comp.position.1); // Only x, y
                self.power_symbols.insert(comp.reference.clone(), (net_name, position));
            }

            let info = ComponentInfo {
                reference: comp.reference.clone(),
                lib_id: comp.lib_id.clone(),
                kind,
                value: if comp.value.is_empty() {
                    None
                } else {
                    Some(comp.value.clone())
                },
                footprint: comp.footprint.clone(),
                connected_nets,
            };

            self.components.insert(comp.reference.clone(), info);
        }
    }

    /// Build the connection graph from the schematic
    fn build_connection_graph(&mut self) {
        // Build net to components mapping from component pins
        for comp in &self.schematic.components {
            for pin in &comp.pins {
                if let Some(net_name) = &pin.net_name {
                    let conn = PinConnection {
                        component: comp.reference.clone(),
                        pin: pin.number.clone(),
                        pin_name: if pin.name.is_empty() {
                            None
                        } else {
                            Some(pin.name.clone())
                        },
                    };
                    self.net_components
                        .entry(net_name.clone())
                        .or_default()
                        .push(conn);
                }
            }
        }

        // Add power symbols to net_components
        // Power symbols create implicit nets based on their value
        for (pwr_ref, (net_name, _pos)) in &self.power_symbols {
            let conn = PinConnection {
                component: pwr_ref.clone(),
                pin: "1".to_string(),
                pin_name: None,
            };
            self.net_components
                .entry(net_name.clone())
                .or_default()
                .push(conn);
        }

        // Use wire-based connectivity to infer net names from labels
        // Build a map of connected wire points to net names
        let wire_nets = self.build_wire_net_map();

        // For each label, find connected components
        for (net_name, components) in wire_nets {
            for comp_ref in components {
                // Skip if already in this net
                if self.net_components.get(&net_name).map_or(false, |pins| {
                    pins.iter().any(|p| p.component == comp_ref)
                }) {
                    continue;
                }

                let conn = PinConnection {
                    component: comp_ref,
                    pin: "?".to_string(), // Unknown pin number
                    pin_name: None,
                };
                self.net_components
                    .entry(net_name.clone())
                    .or_default()
                    .push(conn);
            }
        }

        // Build adjacency from net connections
        for (_net, pins) in &self.net_components {
            // All components on the same net are connected to each other
            for pin1 in pins {
                for pin2 in pins {
                    if pin1.component != pin2.component {
                        self.adjacency
                            .entry(pin1.component.clone())
                            .or_default()
                            .insert(pin2.component.clone());
                    }
                }
            }
        }
    }

    /// Build a map of wire-connected nets to components
    /// Uses labels to identify net names
    fn build_wire_net_map(&self) -> HashMap<String, Vec<String>> {
        let mut result: HashMap<String, Vec<String>> = HashMap::new();

        // Build point-to-net-name mapping from labels
        // Labels are positioned at wire endpoints or junctions
        let mut point_nets: HashMap<(i64, i64), String> = HashMap::new();
        let tolerance = 2.54; // 2.54mm tolerance for KiCad grid

        for label in &self.schematic.labels {
            let label_pos = (
                (label.position.0 * 1000.0) as i64,
                (label.position.1 * 1000.0) as i64,
            );
            let net_name = label.text.clone();
            point_nets.insert(label_pos, net_name.clone());

            // Also check nearby wire endpoints
            for wire in &self.schematic.wires {
                let start = (
                    (wire.start.0 * 1000.0) as i64,
                    (wire.start.1 * 1000.0) as i64,
                );
                let end = (
                    (wire.end.0 * 1000.0) as i64,
                    (wire.end.1 * 1000.0) as i64,
                );

                // Check if label is near wire start or end
                let tol_scaled = (tolerance * 1000.0) as i64;
                if (start.0 - label_pos.0).abs() <= tol_scaled &&
                   (start.1 - label_pos.1).abs() <= tol_scaled {
                    point_nets.insert(start, net_name.clone());
                }
                if (end.0 - label_pos.0).abs() <= tol_scaled &&
                   (end.1 - label_pos.1).abs() <= tol_scaled {
                    point_nets.insert(end, net_name.clone());
                }
            }

            // Initialize result entry for this net
            result.entry(net_name).or_default();
        }

        // Now find components near wire endpoints with labels
        for comp in &self.schematic.components {
            // Skip power symbols - they're handled separately
            if comp.lib_id.to_lowercase().starts_with("power:") {
                continue;
            }

            let comp_pos = (
                (comp.position.0 * 1000.0) as i64,
                (comp.position.1 * 1000.0) as i64,
            );

            // Check if component is near any labeled wire point
            let comp_tol = (25.4 * 1000.0) as i64; // 25.4mm = 1 inch
            for (point, net_name) in &point_nets {
                // Use component position as approximation (pins would be more accurate)
                if (comp_pos.0 - point.0).abs() <= comp_tol &&
                   (comp_pos.1 - point.1).abs() <= comp_tol
                {
                    result
                        .entry(net_name.clone())
                        .or_default()
                        .push(comp.reference.clone());
                    break; // Only add each component once per net
                }
            }
        }

        result
    }

    /// Build the topology summary
    fn build_summary(&self) -> TopologySummary {
        let mut builder = TopologySummaryBuilder::new();

        // Check for incomplete schematic data and add warnings
        self.check_and_add_warnings(&mut builder);

        // Add power domains
        self.extract_power_domains(&mut builder);

        // Add ground nets
        self.extract_ground_nets(&mut builder);

        // Add signal paths
        self.extract_signal_paths(&mut builder);

        // Identify functional modules
        self.identify_modules(&mut builder);

        // Set component summary
        builder.set_component_summary(self.build_component_summary());

        // Set connections (convert HashSet to Vec)
        let connections: HashMap<String, Vec<String>> = self
            .adjacency
            .iter()
            .map(|(k, v)| (k.clone(), v.iter().cloned().collect()))
            .collect();
        builder.set_connections(connections);

        // Set net to components mapping
        for (net, pins) in &self.net_components {
            let components: Vec<String> =
                pins.iter().map(|p| p.component.clone()).collect();
            builder.set_net_components(net.clone(), components);
        }

        builder.build()
    }

    /// Check for incomplete schematic data and add warnings
    fn check_and_add_warnings(&self, builder: &mut TopologySummaryBuilder) {
        let has_components = !self.components.is_empty();
        let has_wires = !self.schematic.wires.is_empty();
        let has_labels = !self.schematic.labels.is_empty();
        let has_junctions = !self.schematic.junctions.is_empty();
        let has_power_symbols = !self.power_symbols.is_empty();
        let has_nets = !self.schematic.nets.is_empty();

        // Check for missing connection data
        if has_components && !has_wires && !has_labels && !has_nets {
            builder.add_warning(
                "原理图缺少连接信息：没有 wires、labels 或显式网络定义。\
                 元件之间无法建立连接关系，拓扑分析结果将不完整。\
                 请检查是否需要添加导线连接或网络标签。"
                    .to_string(),
            );
        }

        // Check for missing power symbols when components exist
        if has_components && !has_power_symbols && !has_wires {
            builder.add_warning(
                "未检测到电源符号 (power symbols) 或导线连接。\
                 电源网络可能无法正确识别。\
                 如果电路使用隐式电源连接，请确保在元件属性中正确设置。"
                    .to_string(),
            );
        }

        // Check for missing ground connections
        if has_components && !has_junctions && !has_wires {
            builder.add_warning(
                "未检测到连接节点 (junctions) 或导线。\
                 这可能表示原理图仅包含元件定义而未完成布线。"
                    .to_string(),
            );
        }

        // Check for components with no net connections
        let unconnected_count = self
            .components
            .values()
            .filter(|c| c.connected_nets.is_empty())
            .count();
        if unconnected_count > 0 && has_components {
            builder.add_warning(format!(
                "有 {} 个元件未检测到网络连接。\
                 这可能是因为缺少导线连接或引脚未正确绑定到网络。",
                unconnected_count
            ));
        }
    }

    /// Extract power domains
    fn extract_power_domains(&self, builder: &mut TopologySummaryBuilder) {
        for (net_name, pins) in &self.net_components {
            let net_kind = classify_net(net_name);
            if net_kind != NetKind::Power {
                continue;
            }

            let voltage = extract_voltage(net_name);
            let mut consumers = Vec::new();
            let mut sources = Vec::new();

            for pin in pins {
                if let Some(comp_info) = self.components.get(&pin.component) {
                    // Power components are sources, others are consumers
                    if comp_info.kind == ComponentKind::Power {
                        sources.push(pin.component.clone());
                    } else {
                        consumers.push(pin.component.clone());
                    }
                }
            }

            // Deduplicate
            consumers.sort();
            consumers.dedup();
            sources.sort();
            sources.dedup();

            builder.add_power_domain(PowerDomain {
                name: net_name.clone(),
                voltage,
                consumers,
                sources,
            });
        }
    }

    /// Extract ground nets
    fn extract_ground_nets(&self, builder: &mut TopologySummaryBuilder) {
        for (net_name, _) in &self.net_components {
            let net_kind = classify_net(net_name);
            if net_kind == NetKind::Ground {
                builder.add_ground_net(net_name.clone());
            }
        }
    }

    /// Extract signal paths from labels
    fn extract_signal_paths(&self, builder: &mut TopologySummaryBuilder) {
        // Track seen net names to avoid duplicates
        let mut seen_nets: HashSet<String> = HashSet::new();

        // Extract from global labels and hierarchical labels
        for label in &self.schematic.labels {
            let net_name = label.text.clone();
            // Skip if already seen this net
            if !seen_nets.insert(net_name.clone()) {
                continue;
            }

            // Determine direction from label shape
            let direction = match label.shape.as_str() {
                "input" => "input",
                "output" => "output",
                "bidirectional" | "tri_state" => "bidirectional",
                _ => "passive",
            };

            // Find connected components and determine from/to based on signal direction
            let (from, to, via) = self.determine_signal_endpoints(&net_name, direction);

            builder.add_signal_path(SignalPath {
                name: net_name.clone(),
                direction: direction.to_string(),
                from,
                to,
                via,
                pullup: None, // Will be filled by module detection
                series: Vec::new(),
            });
        }

        // Also extract signal paths from named nets (non-power, non-ground)
        for (net_name, pins) in &self.net_components {
            let net_kind = classify_net(net_name);
            if net_kind != NetKind::Signal {
                continue;
            }

            // Skip if already seen this net
            if !seen_nets.insert(net_name.clone()) {
                continue;
            }

            // Create a signal path for this net
            let from = pins.first().map(|p| format!("{}.{}", p.component, p.pin));
            let to = if pins.len() > 1 {
                pins.last().map(|p| format!("{}.{}", p.component, p.pin))
            } else {
                None
            };

            builder.add_signal_path(SignalPath {
                name: net_name.clone(),
                direction: "internal".to_string(),
                from,
                to,
                via: Vec::new(),
                pullup: None,
                series: Vec::new(),
            });
        }
    }

    /// Identify functional modules
    fn identify_modules(&self, builder: &mut TopologySummaryBuilder) {
        // Identify decoupling capacitors
        self.identify_decoupling_capacitors(builder);

        // Identify LED indicators
        self.identify_led_indicators(builder);

        // Identify I2C pull-up resistors
        self.identify_i2c_pullups(builder);

        // Identify crystal oscillator circuits
        self.identify_crystal_oscillators(builder);

        // Identify reset circuits
        self.identify_reset_circuits(builder);
    }

    /// Identify decoupling capacitors (capacitors connected to power/ground)
    fn identify_decoupling_capacitors(&self, builder: &mut TopologySummaryBuilder) {
        // Group capacitors by nearby ICs
        let mut ic_capacitors: HashMap<String, Vec<String>> = HashMap::new();

        for (ref_name, comp_info) in &self.components {
            if comp_info.kind != ComponentKind::Capacitor {
                continue;
            }

            // Check if this capacitor is connected to both power and ground
            let mut connected_to_power = false;
            let mut connected_to_ground = false;
            let mut connected_ics = Vec::new();

            for (_pin, net_name) in &comp_info.connected_nets {
                let net_kind = classify_net(net_name);
                match net_kind {
                    NetKind::Power => connected_to_power = true,
                    NetKind::Ground => connected_to_ground = true,
                    _ => {}
                }

                // Find connected ICs through this net
                if let Some(pins) = self.net_components.get(net_name) {
                    for pin in pins {
                        if let Some(other_comp) = self.components.get(&pin.component) {
                            if other_comp.kind == ComponentKind::Ic
                                && pin.component != *ref_name
                            {
                                connected_ics.push(pin.component.clone());
                            }
                        }
                    }
                }
            }

            // If connected to both power and ground, it's likely a decoupling cap
            if connected_to_power && connected_to_ground {
                // Deduplicate connected ICs
                connected_ics.sort();
                connected_ics.dedup();

                for ic in connected_ics {
                    ic_capacitors
                        .entry(ic)
                        .or_default()
                        .push(ref_name.clone());
                }
            }
        }

        // Create modules for ICs with decoupling capacitors
        for (ic, caps) in ic_capacitors {
            builder.add_module(FunctionalModule {
                module_type: "power_decoupling".to_string(),
                purpose: format!("电源去耦 ({} 个电容)", caps.len()),
                components: caps,
                target: Some(ic),
                metadata: HashMap::new(),
            });
        }
    }

    /// Identify LED indicator circuits
    fn identify_led_indicators(&self, builder: &mut TopologySummaryBuilder) {
        for (ref_name, comp_info) in &self.components {
            if comp_info.kind != ComponentKind::Diode {
                continue;
            }

            // Check if it's an LED (usually has "LED" in lib_id or value)
            let is_led = comp_info.lib_id.to_lowercase().contains("led")
                || comp_info
                    .value
                    .as_ref()
                    .map(|v| v.to_lowercase().contains("led"))
                    .unwrap_or(false);

            if !is_led {
                continue;
            }

            // Find series resistor (connected to same net)
            let mut series_resistor = None;
            for (_pin, net_name) in &comp_info.connected_nets {
                if classify_net(net_name) != NetKind::Signal {
                    continue;
                }
                if let Some(pins) = self.net_components.get(net_name) {
                    for pin in pins {
                        if let Some(other_comp) = self.components.get(&pin.component) {
                            if other_comp.kind == ComponentKind::Resistor
                                && pin.component != *ref_name
                            {
                                series_resistor = Some(pin.component.clone());
                                break;
                            }
                        }
                    }
                }
                if series_resistor.is_some() {
                    break;
                }
            }

            let mut components = vec![ref_name.clone()];
            if let Some(r) = &series_resistor {
                components.push(r.clone());
            }

            builder.add_module(FunctionalModule {
                module_type: "led_indicator".to_string(),
                purpose: "LED 指示灯".to_string(),
                components,
                target: None,
                metadata: HashMap::new(),
            });
        }
    }

    /// Determine signal endpoints based on direction
    /// For input signals: from is external, to is internal (IC)
    /// For output signals: from is internal (IC), to is external
    /// For bidirectional: both could be ICs
    fn determine_signal_endpoints(
        &self,
        net_name: &str,
        direction: &str,
    ) -> (Option<String>, Option<String>, Vec<String>) {
        let pins = match self.net_components.get(net_name) {
            Some(p) => p,
            None => return (None, None, Vec::new()),
        };

        if pins.is_empty() {
            return (None, None, Vec::new());
        }

        // Classify connected components
        let mut ic_pins: Vec<&PinConnection> = Vec::new();
        let mut passive_pins: Vec<&PinConnection> = Vec::new();
        let mut connector_pins: Vec<&PinConnection> = Vec::new();

        for pin in pins {
            if let Some(comp_info) = self.components.get(&pin.component) {
                match comp_info.kind {
                    ComponentKind::Ic => ic_pins.push(pin),
                    ComponentKind::Connector => connector_pins.push(pin),
                    _ => passive_pins.push(pin),
                }
            }
        }

        match direction {
            "input" => {
                // Input signal: external -> internal (IC)
                // from: connector or passive component
                // to: IC
                let from_pin = connector_pins
                    .first()
                    .or_else(|| passive_pins.first());

                let to_pin = ic_pins.first();

                let from = from_pin.map(|p| format!("{}.{}", p.component, p.pin));
                let to = to_pin.map(|p| format!("{}.{}", p.component, p.pin));

                // via: passive components excluding the 'from' component
                let from_comp = from_pin.map(|p| p.component.as_str());
                let via: Vec<String> = passive_pins
                    .iter()
                    .filter(|p| from_comp.map_or(true, |fc| fc != p.component.as_str()))
                    .map(|p| p.component.clone())
                    .collect();

                (from, to, via)
            }
            "output" => {
                // Output signal: internal (IC) -> external
                // from: IC
                // to: connector or passive component
                let from_pin = ic_pins.first();
                let to_pin = connector_pins
                    .first()
                    .or_else(|| passive_pins.first());

                let from = from_pin.map(|p| format!("{}.{}", p.component, p.pin));
                let to = to_pin.map(|p| format!("{}.{}", p.component, p.pin));

                // via: passive components excluding the 'to' component
                let to_comp = to_pin.map(|p| p.component.as_str());
                let via: Vec<String> = passive_pins
                    .iter()
                    .filter(|p| to_comp.map_or(true, |tc| tc != p.component.as_str()))
                    .map(|p| p.component.clone())
                    .collect();

                (from, to, via)
            }
            "bidirectional" => {
                // Bidirectional: could be between two ICs
                let from = pins.first().map(|p| format!("{}.{}", p.component, p.pin));
                let to = if pins.len() > 1 {
                    pins.last().map(|p| format!("{}.{}", p.component, p.pin))
                } else {
                    None
                };
                let via: Vec<String> = pins
                    .iter()
                    .skip(1)
                    .take(pins.len().saturating_sub(2))
                    .map(|p| p.component.clone())
                    .collect();

                (from, to, via)
            }
            _ => {
                // Passive or unknown: use simple ordering
                let from = pins.first().map(|p| format!("{}.{}", p.component, p.pin));
                let to = if pins.len() > 1 {
                    pins.last().map(|p| format!("{}.{}", p.component, p.pin))
                } else {
                    None
                };
                let via: Vec<String> = pins
                    .iter()
                    .skip(1)
                    .take(pins.len().saturating_sub(2))
                    .map(|p| p.component.clone())
                    .collect();

                (from, to, via)
            }
        }
    }

    /// Identify I2C pull-up resistors
    fn identify_i2c_pullups(&self, builder: &mut TopologySummaryBuilder) {
        // Look for SDA and SCL nets
        let i2c_signals = ["SDA", "SCL", "I2C_SDA", "I2C_SCL"];

        for signal in i2c_signals {
            // Find resistors connected to this signal and power
            for (ref_name, comp_info) in &self.components {
                if comp_info.kind != ComponentKind::Resistor {
                    continue;
                }

                let mut connected_to_signal = false;
                let mut connected_to_power = false;

                for (_pin, net_name) in &comp_info.connected_nets {
                    if net_name.to_uppercase().contains(signal) {
                        connected_to_signal = true;
                    }
                    if classify_net(net_name) == NetKind::Power {
                        connected_to_power = true;
                    }
                }

                if connected_to_signal && connected_to_power {
                    builder.add_module(FunctionalModule {
                        module_type: "i2c_pullup".to_string(),
                        purpose: format!("I2C {} 上拉电阻", signal),
                        components: vec![ref_name.clone()],
                        target: None,
                        metadata: [(signal.to_string(), "true".to_string())]
                            .into_iter()
                            .collect(),
                    });
                }
            }
        }
    }

    /// Identify crystal oscillator circuits (Crystal + load capacitors to ground)
    /// Pattern: Crystal connected to IC with two load capacitors to ground
    fn identify_crystal_oscillators(&self, builder: &mut TopologySummaryBuilder) {
        // Find all crystal components
        for (xtal_ref, xtal_info) in &self.components {
            if xtal_info.kind != ComponentKind::Crystal {
                continue;
            }

            let mut load_capacitors: Vec<String> = Vec::new();
            let mut connected_ic: Option<String> = None;
            let mut frequency: Option<String> = None;

            // Try to extract frequency from value (e.g., "8MHz", "16.000")
            if let Some(value) = &xtal_info.value {
                frequency = Some(value.clone());
            }

            // Find capacitors connected to the same nets as the crystal
            // Crystal typically has two pins connected to OSC_IN and OSC_OUT
            // Each pin should have a capacitor to ground
            for (_xtal_pin, net_name) in &xtal_info.connected_nets {
                let net_kind = classify_net(net_name);
                if net_kind == NetKind::Ground {
                    continue;
                }

                // Find connected IC through this net
                if let Some(pins) = self.net_components.get(net_name) {
                    for pin in pins {
                        if pin.component == *xtal_ref {
                            continue;
                        }
                        if let Some(other_comp) = self.components.get(&pin.component) {
                            if other_comp.kind == ComponentKind::Ic {
                                connected_ic = Some(pin.component.clone());
                            }
                        }
                    }
                }

                // Find capacitors that connect this net to ground
                if let Some(pins) = self.net_components.get(net_name) {
                    for pin in pins {
                        if pin.component == *xtal_ref {
                            continue;
                        }
                        if let Some(other_comp) = self.components.get(&pin.component) {
                            if other_comp.kind == ComponentKind::Capacitor {
                                // Check if this capacitor is also connected to ground
                                for (_cap_pin, cap_net) in &other_comp.connected_nets {
                                    if classify_net(cap_net) == NetKind::Ground {
                                        load_capacitors.push(pin.component.clone());
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Deduplicate capacitors
            load_capacitors.sort();
            load_capacitors.dedup();

            // Only identify as crystal oscillator if we have the crystal and at least one load cap
            if !load_capacitors.is_empty() {
                let mut components = vec![xtal_ref.clone()];
                components.extend(load_capacitors.clone());

                let purpose = if let Some(freq) = &frequency {
                    format!("晶振电路 ({}), {} 个负载电容", freq, load_capacitors.len())
                } else {
                    format!("晶振电路, {} 个负载电容", load_capacitors.len())
                };

                builder.add_module(FunctionalModule {
                    module_type: "crystal_oscillator".to_string(),
                    purpose,
                    components,
                    target: connected_ic.clone(),
                    metadata: if let Some(freq) = frequency {
                        [("frequency".to_string(), freq)].into_iter().collect()
                    } else {
                        HashMap::new()
                    },
                });
            }
        }
    }

    /// Identify reset circuits (RC circuit with pullup resistor and capacitor)
    /// Pattern: RC network connected to RESET/NRST pin of IC
    fn identify_reset_circuits(&self, builder: &mut TopologySummaryBuilder) {
        // Look for nets with reset-related names
        let reset_keywords = ["RESET", "NRST", "RST", "NRESET", "~RESET", "/RST"];

        for (net_name, pins) in &self.net_components {
            let net_upper = net_name.to_uppercase();
            let is_reset_net = reset_keywords
                .iter()
                .any(|kw| net_upper.contains(kw));

            if !is_reset_net {
                continue;
            }

            let mut resistors: Vec<String> = Vec::new();
            let mut capacitors: Vec<String> = Vec::new();
            let mut connected_ic: Option<String> = None;

            for pin in pins {
                if let Some(comp_info) = self.components.get(&pin.component) {
                    match comp_info.kind {
                        ComponentKind::Resistor => {
                            // Check if this resistor is also connected to power (pullup)
                            let mut is_pullup = false;
                            for (_r_pin, r_net) in &comp_info.connected_nets {
                                if classify_net(r_net) == NetKind::Power {
                                    is_pullup = true;
                                    break;
                                }
                            }
                            if is_pullup {
                                resistors.push(pin.component.clone());
                            }
                        }
                        ComponentKind::Capacitor => {
                            // Check if this capacitor is also connected to ground
                            let mut is_ground_cap = false;
                            for (_c_pin, c_net) in &comp_info.connected_nets {
                                if classify_net(c_net) == NetKind::Ground {
                                    is_ground_cap = true;
                                    break;
                                }
                            }
                            if is_ground_cap {
                                capacitors.push(pin.component.clone());
                            }
                        }
                        ComponentKind::Ic => {
                            // This is the IC with the reset pin
                            connected_ic = Some(pin.component.clone());
                        }
                        _ => {}
                    }
                }
            }

            // Deduplicate
            resistors.sort();
            resistors.dedup();
            capacitors.sort();
            capacitors.dedup();

            // Only create module if we have at least one resistor or capacitor
            if !resistors.is_empty() || !capacitors.is_empty() {
                let mut components = Vec::new();
                components.extend(resistors.clone());
                components.extend(capacitors.clone());

                let purpose = match (resistors.len(), capacitors.len()) {
                    (1, 1) => "复位电路 (RC 网络)".to_string(),
                    (1, 0) => "复位电路 (上拉电阻)".to_string(),
                    (0, 1) => "复位电路 (滤波电容)".to_string(),
                    (r, c) if r > 0 && c > 0 => {
                        format!("复位电路 ({} 个电阻, {} 个电容)", r, c)
                    }
                    (r, 0) if r > 1 => format!("复位电路 ({} 个电阻)", r),
                    (0, c) if c > 1 => format!("复位电路 ({} 个电容)", c),
                    _ => "复位电路".to_string(),
                };

                builder.add_module(FunctionalModule {
                    module_type: "reset_circuit".to_string(),
                    purpose,
                    components,
                    target: connected_ic.clone(),
                    metadata: [("net".to_string(), net_name.clone())]
                        .into_iter()
                        .collect(),
                });
            }
        }
    }

    /// Build component summary statistics
    fn build_component_summary(&self) -> ComponentSummary {
        let mut summary = ComponentSummary::default();
        summary.total = self.components.len();

        let mut by_type: HashMap<String, usize> = HashMap::new();
        let mut by_type_refs: HashMap<String, Vec<String>> = HashMap::new();

        for (ref_name, comp_info) in &self.components {
            let kind_name = comp_info.kind.name().to_string();
            *by_type.entry(kind_name.clone()).or_default() += 1;
            by_type_refs
                .entry(kind_name)
                .or_default()
                .push(ref_name.clone());
        }

        summary.by_type = by_type;
        summary.by_type_refs = by_type_refs;
        summary
    }
}

/// Convenience function to extract topology from a schematic
pub fn extract_topology(schematic: &Schematic) -> TopologySummary {
    let mut extractor = TopologyExtractor::new(schematic);
    extractor.extract()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_schematic() {
        let schematic = Schematic::new();
        let summary = extract_topology(&schematic);
        assert!(summary.is_empty());
    }
}
