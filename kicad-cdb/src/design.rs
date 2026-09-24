use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};

use kicad_json5::codegen::{KicadVersion, SexprConfig, SexprGenerator};
use kicad_json5::ir::{Net, PinInstance, RenderHint, Schematic, Symbol, SymbolInstance};
use kicad_json5::layout;

use crate::composition::{Composition, ModuleInstance};
use crate::ic_template::{self, IcCoreTemplate, Peripheral};
use crate::topology::{load_builtin_template, ComponentSlot, TopologyTemplate};
use crate::ComponentDb;

/// Generate a schematic from a topology template and design requirements.
pub fn generate_schematic(
    db: &ComponentDb,
    template_name: &str,
    vin: f64,
    vout: f64,
    iout: f64,
) -> Result<String> {
    let template = load_builtin_template(template_name)?;

    // Build input map for parameter resolution
    let mut inputs = HashMap::new();
    inputs.insert("vin".to_string(), vin);
    inputs.insert("vout".to_string(), vout);
    inputs.insert("iout".to_string(), iout);
    inputs.insert("iout_max".to_string(), iout);

    // Build nets
    let mut net_id_map = HashMap::new();
    let mut nets = Vec::new();
    for (i, conn) in template.connections.iter().enumerate() {
        let id = i as u32;
        net_id_map.insert(conn.net.clone(), id);
        let render = match conn.net.as_str() {
            "GND" => RenderHint::Power,
            n if n.starts_with("V") || n == "VIN" => RenderHint::Power,
            _ => RenderHint::Wire,
        };
        nets.push(Net {
            id,
            name: conn.net.clone(),
            net_type: None,
            render,
        });
    }

    // Build pin-to-net mapping from connections
    let mut pin_net_map: HashMap<(String, String), u32> = HashMap::new();
    for conn in &template.connections {
        let net_id = *net_id_map
            .get(&conn.net)
            .context(format!("Net '{}' not found", conn.net))?;
        for pin_spec in &conn.pins {
            let parts: Vec<&str> = pin_spec.split('.').collect();
            if parts.len() == 2 {
                pin_net_map.insert((parts[0].to_string(), parts[1].to_string()), net_id);
            }
        }
    }

    // Build components and collect lib_ids
    let mut components = Vec::new();
    let mut ref_counter: HashMap<String, usize> = HashMap::new();
    let mut used_lib_ids: HashSet<String> = HashSet::new();

    for slot in &template.components {
        let comp = build_component(slot, &template, &inputs, &pin_net_map, &mut ref_counter, db)?;
        used_lib_ids.insert(comp.lib_id.clone());
        components.push(comp);
    }

    // Build lib_symbols for auto-wiring to work
    let lib_symbols: Vec<Symbol> = used_lib_ids
        .iter()
        .map(|lib_id| build_lib_symbol(lib_id, &pin_net_map))
        .collect();

    // Assemble schematic
    let mut schematic = Schematic::new();
    schematic.metadata.title_block.title = Some(format!(
        "{} circuit ({}V -> {}V, {}A)",
        template.name.to_uppercase(),
        vin,
        vout,
        iout
    ));
    schematic.nets = nets;
    schematic.components = components;
    schematic.lib_symbols = lib_symbols;

    // Generate .kicad_sch
    let config = SexprConfig {
        kicad_version: Some(KicadVersion::V8),
        ..Default::default()
    };
    let mut gen = SexprGenerator::with_config(config);
    let output = gen.generate(&schematic)?;

    Ok(output)
}

/// Build a minimal Symbol definition so SexprGenerator can compute pin positions for auto-wiring.
fn build_lib_symbol(lib_id: &str, pin_net_map: &HashMap<(String, String), u32>) -> Symbol {
    let mut sym = Symbol::new(lib_id);
    let prefix = match lib_id {
        "Device:R" => "R",
        "Device:C" => "C",
        "Device:L" => "L",
        "Device:D" | "Device:LED" => "D",
        _ => "U",
    };
    sym.reference = prefix.to_string();

    // Collect pins for this lib_id from the role-based pin_net_map.
    // Since pin_net_map keys are (role, pin_name) and lib_id comes from slot.lib,
    // we need a role-to-lib_id mapping. Instead, collect pins for the component
    // that uses this lib_id by scanning all roles that map to it.
    let mut pin_idx = 0;
    let mut seen_pins: HashSet<String> = HashSet::new();
    for key in pin_net_map.keys() {
        let pin_name = &key.1;
        if seen_pins.contains(pin_name) {
            continue;
        }
        let pin_number = if lib_id.starts_with("Device:") {
            pin_idx += 1;
            pin_idx.to_string()
        } else {
            pin_name.clone()
        };
        sym.pins.push(kicad_json5::ir::Pin {
            number: pin_number,
            name: pin_name.clone(),
            pin_type: "passive".to_string(),
        });
        seen_pins.insert(pin_name.clone());
        pin_idx += 1;
    }

    sym
}

fn build_component(
    slot: &ComponentSlot,
    template: &TopologyTemplate,
    _inputs: &HashMap<String, f64>,
    pin_net_map: &HashMap<(String, String), u32>,
    ref_counter: &mut HashMap<String, usize>,
    _db: &ComponentDb,
) -> Result<SymbolInstance> {
    // Determine lib_id
    let lib_id = if slot.lib.is_empty() {
        "custom:IC".to_string()
    } else {
        slot.lib.clone()
    };

    // Generate reference designator
    let prefix = match lib_id.as_str() {
        "Device:R" => "R",
        "Device:C" => "C",
        "Device:L" => "L",
        "Device:D" | "Device:LED" => "D",
        _ => "U",
    };
    let count = ref_counter.entry(prefix.to_string()).or_insert(0);
    *count += 1;
    let reference = format!("{}{}", prefix, *count);

    // Resolve value
    let value = slot.value.clone();

    // Position from layout
    let pos = template.layout.get(&slot.role);
    let (x, y) = pos.map(|p| (p.x, p.y)).unwrap_or((50.8, 50.8));

    // Build pins based on pin_net_map for this role
    let mut pins = Vec::new();
    let mut pin_idx = 0;
    for (key, &net_id) in pin_net_map {
        if key.0 == slot.role {
            let pin_name = key.1.clone();
            let pin_number = if is_standard_device(&lib_id) {
                pin_idx += 1;
                pin_idx.to_string()
            } else {
                pin_name.clone()
            };
            pins.push(PinInstance {
                number: pin_number,
                name: pin_name,
                pin_type: "passive".to_string(),
                net_id: Some(net_id),
                net_name: None,
                nc: false,
            });
        }
    }

    // Sort pins by number for consistent output
    pins.sort_by(|a, b| a.number.cmp(&b.number));

    let mut comp = SymbolInstance::new(lib_id, reference);
    comp.value = value;
    comp.position = (x, y, 0.0);
    comp.pins = pins;

    Ok(comp)
}

fn is_standard_device(lib_id: &str) -> bool {
    lib_id.starts_with("Device:")
}

/// Generate a schematic from an IC core template.
/// `user_params` are user-supplied parameters (e.g. vout=3.3).
/// `net_map` maps interface port names to actual net names in the design.
pub fn generate_ic_schematic(
    _db: &ComponentDb,
    template_name: &str,
    user_params: &HashMap<String, f64>,
    net_map: &HashMap<String, String>,
) -> Result<String> {
    let template = ic_template::load_builtin_template(template_name)?;
    let resolved = ic_template::resolve_params(&template, user_params)?;

    // Build nets from interface ports + internal nets
    let mut net_id_map = HashMap::new();
    let mut nets = Vec::new();
    let mut next_id: u32 = 0;

    // Add interface nets
    for (port_name, port_def) in &template.interface {
        let net_name = net_map
            .get(port_name)
            .cloned()
            .unwrap_or_else(|| port_name.clone());
        net_id_map.insert(port_name.clone(), next_id);
        let render = match port_def.port_type.as_str() {
            "power" => RenderHint::Power,
            _ => RenderHint::Wire,
        };
        nets.push(Net {
            id: next_id,
            name: net_name,
            net_type: None,
            render,
        });
        next_id += 1;
    }

    // Build IC component
    let mut components = Vec::new();
    let mut ref_counter: HashMap<String, usize> = HashMap::new();
    let mut used_lib_ids: HashSet<String> = HashSet::new();

    // IC position
    let ic_pos = template
        .layout
        .get("ic")
        .map(|p| (p.x, p.y))
        .unwrap_or((50.8, 50.8));
    let ic_ref = alloc_ref(&template.ic.mpn, &mut ref_counter);
    let mut ic_pins = Vec::new();
    for pin in &template.ic.pins {
        if let Some(&net_id) = net_id_map.get(&pin.name) {
            ic_pins.push(PinInstance {
                number: pin.number.clone(),
                name: pin.name.clone(),
                pin_type: pin.pin_type.clone(),
                net_id: Some(net_id),
                net_name: None,
                nc: false,
            });
        } else if pin.name == "NC" {
            ic_pins.push(PinInstance {
                number: pin.number.clone(),
                name: pin.name.clone(),
                pin_type: pin.pin_type.clone(),
                net_id: None,
                net_name: None,
                nc: true,
            });
        }
    }
    ic_pins.sort_by(|a, b| a.number.cmp(&b.number));
    let ic_lib_id = template.ic.mpn.clone();
    let mut ic_comp = SymbolInstance::new(ic_lib_id.clone(), ic_ref);
    ic_comp.value = template.ic.mpn.clone();
    ic_comp.position = (ic_pos.0, ic_pos.1, 0.0);
    ic_comp.footprint = if template.ic.footprint.is_empty() {
        None
    } else {
        Some(template.ic.footprint.clone())
    };
    ic_comp.pins = ic_pins;
    used_lib_ids.insert(ic_lib_id.clone());
    components.push(ic_comp);

    // Build peripheral components
    for periph in &template.peripherals {
        let periph_comp = build_peripheral_component(
            periph,
            &template,
            &resolved,
            &mut net_id_map,
            &mut next_id,
            &mut nets,
            &mut ref_counter,
        )?;
        used_lib_ids.insert(periph_comp.lib_id.clone());
        components.push(periph_comp);
    }

    // Build lib_symbols
    let lib_symbols: Vec<Symbol> = used_lib_ids
        .iter()
        .map(|lib_id| build_ic_lib_symbol(lib_id, &template))
        .collect();

    let mut schematic = Schematic::new();
    schematic.metadata.title_block.title =
        Some(format!("{} circuit ({})", template.name, template.ic.mpn));
    schematic.nets = nets;
    schematic.components = components;
    schematic.lib_symbols = lib_symbols;

    let config = SexprConfig {
        kicad_version: Some(KicadVersion::V8),
        ..Default::default()
    };
    let mut gen = SexprGenerator::with_config(config);
    let output = gen.generate(&schematic)?;

    Ok(output)
}

fn alloc_ref(lib_id: &str, counter: &mut HashMap<String, usize>) -> String {
    let prefix = if lib_id.starts_with("Device:R") {
        "R"
    } else if lib_id.starts_with("Device:C") {
        "C"
    } else if lib_id.starts_with("Device:L") {
        "L"
    } else if lib_id.starts_with("Device:D") {
        "D"
    } else if lib_id.starts_with("Device:Q") || lib_id.starts_with("AO") {
        "Q"
    } else {
        "U"
    };
    let count = counter.entry(prefix.to_string()).or_insert(0);
    *count += 1;
    format!("{}{}", prefix, *count)
}

fn build_peripheral_component(
    periph: &Peripheral,
    template: &IcCoreTemplate,
    resolved: &HashMap<String, f64>,
    net_id_map: &mut HashMap<String, u32>,
    next_id: &mut u32,
    nets: &mut Vec<Net>,
    ref_counter: &mut HashMap<String, usize>,
) -> Result<SymbolInstance> {
    let lib_id = if periph.lib.is_empty() {
        "custom:IC".to_string()
    } else {
        periph.lib.clone()
    };
    let reference = alloc_ref(&lib_id, ref_counter);

    // Resolve value — replace "computed" with actual calculated value
    let value = if periph.value == "computed" {
        // Try to find a matching param (e.g., role "r_fb1" → param "r_fb1")
        if let Some(&val) = resolved.get(&periph.role) {
            format_resistance(val)
        } else {
            "computed".to_string()
        }
    } else {
        periph.value.clone()
    };

    // Position from layout
    let pos = template.layout.get(&periph.role);
    let (x, y) = pos.map(|p| (p.x, p.y)).unwrap_or((76.2, 50.8));

    // Build pins — resolve pin targets to net IDs
    let mut pins = Vec::new();
    for (pin_num, target) in &periph.pins {
        let net_id = ensure_net(target, net_id_map, next_id, nets);
        pins.push(PinInstance {
            number: pin_num.clone(),
            name: pin_num.clone(),
            pin_type: "passive".to_string(),
            net_id: Some(net_id),
            net_name: None,
            nc: false,
        });
    }
    pins.sort_by(|a, b| a.number.cmp(&b.number));

    let mut comp = SymbolInstance::new(lib_id, reference);
    comp.value = value;
    comp.position = (x, y, 0.0);
    if !periph.footprint.is_empty() {
        comp.footprint = Some(periph.footprint.clone());
    }
    comp.pins = pins;
    Ok(comp)
}

/// Get or create a net ID for a target name
fn ensure_net(
    target: &str,
    net_id_map: &mut HashMap<String, u32>,
    next_id: &mut u32,
    nets: &mut Vec<Net>,
) -> u32 {
    if let Some(&id) = net_id_map.get(target) {
        return id;
    }
    let id = *next_id;
    *next_id += 1;
    let render = if target == "GND" || target.starts_with("V") || target.contains("PWR") {
        RenderHint::Power
    } else {
        RenderHint::Wire
    };
    nets.push(Net {
        id,
        name: target.to_string(),
        net_type: None,
        render,
    });
    net_id_map.insert(target.to_string(), id);
    id
}

/// Format a resistance value in a human-readable way
fn format_resistance(ohms: f64) -> String {
    if ohms >= 1_000_000.0 {
        format!("{:.1}M", ohms / 1_000_000.0)
    } else if ohms >= 1000.0 {
        format!("{:.1}k", ohms / 1000.0)
    } else if ohms >= 1.0 {
        format!("{:.0}", ohms)
    } else {
        format!("{:.3}", ohms)
    }
}

fn format_component_value(lib_id: &str, val: f64) -> String {
    if lib_id.starts_with("Device:R") {
        format_resistance(val)
    } else if lib_id.starts_with("Device:C") {
        if val >= 1e-6 {
            format!("{:.1}uF", val * 1e6)
        } else if val >= 1e-9 {
            format!("{:.1}nF", val * 1e9)
        } else {
            format!("{:.1}pF", val * 1e12)
        }
    } else if lib_id.starts_with("Device:L") {
        if val >= 1e-6 {
            format!("{:.1}uH", val * 1e6)
        } else {
            format!("{:.1}nH", val * 1e9)
        }
    } else {
        format!("{:.3}", val)
    }
}

fn build_ic_lib_symbol(lib_id: &str, template: &IcCoreTemplate) -> Symbol {
    let mut sym = Symbol::new(lib_id);

    if lib_id == template.ic.mpn {
        sym.reference = "U".to_string();
        for pin in &template.ic.pins {
            sym.pins.push(kicad_json5::ir::Pin {
                number: pin.number.clone(),
                name: pin.name.clone(),
                pin_type: pin.pin_type.clone(),
            });
        }
    } else {
        let prefix = match lib_id {
            "Device:R" => "R",
            "Device:C" => "C",
            "Device:L" => "L",
            "Device:D" => "D",
            _ => "U",
        };
        sym.reference = prefix.to_string();
        sym.pins.push(kicad_json5::ir::Pin {
            number: "1".to_string(),
            name: "1".to_string(),
            pin_type: "passive".to_string(),
        });
        sym.pins.push(kicad_json5::ir::Pin {
            number: "2".to_string(),
            name: "2".to_string(),
            pin_type: "passive".to_string(),
        });
    }

    sym
}

/// Generate a schematic by composing multiple module instances.
pub fn generate_composed_schematic(_db: &ComponentDb, composition: &Composition) -> Result<String> {
    let mut all_nets: Vec<Net> = Vec::new();
    let mut all_components: Vec<SymbolInstance> = Vec::new();
    let mut all_lib_ids: HashSet<String> = HashSet::new();
    let mut net_name_to_id: HashMap<String, u32> = HashMap::new();
    let mut next_net_id: u32 = 0;
    let mut ref_counter: HashMap<String, usize> = HashMap::new();
    let mut y_cursor: f64 = 0.0;

    // Pre-register global nets
    for gnet in &composition.global_nets {
        let render = match gnet.net_type.as_deref() {
            Some("power") => RenderHint::Power,
            _ => RenderHint::Wire,
        };
        all_nets.push(Net {
            id: next_net_id,
            name: gnet.name.clone(),
            net_type: None,
            render,
        });
        net_name_to_id.insert(gnet.name.clone(), next_net_id);
        next_net_id += 1;
    }

    // Process each module instance
    for module in &composition.modules {
        let y_off = module.y_offset.unwrap_or(y_cursor);

        match module.template_type.as_str() {
            "ic-core" => {
                compose_ic_module(
                    module,
                    y_off,
                    &mut net_name_to_id,
                    &mut next_net_id,
                    &mut all_nets,
                    &mut all_components,
                    &mut all_lib_ids,
                    &mut ref_counter,
                )?;
            }
            "topology" => {
                compose_topology_module(
                    module,
                    y_off,
                    &mut net_name_to_id,
                    &mut next_net_id,
                    &mut all_nets,
                    &mut all_components,
                    &mut all_lib_ids,
                    &mut ref_counter,
                )?;
            }
            _ => {
                anyhow::bail!(
                    "Unsupported template_type '{}' in module '{}'",
                    module.template_type,
                    module.id
                );
            }
        }

        y_cursor = y_off + 40.0;
    }

    // Build lib_symbols
    let lib_symbols: Vec<Symbol> = all_lib_ids
        .iter()
        .map(|lib_id| {
            if let Ok(tmpl) = ic_template::load_builtin_template(lib_id) {
                build_ic_lib_symbol(lib_id, &tmpl)
            } else if lib_id.starts_with("Device:") {
                build_simple_device_symbol(lib_id)
            } else {
                let mut sym = Symbol::new(lib_id);
                sym.reference = "U".to_string();
                sym
            }
        })
        .collect();

    let mut schematic = Schematic::new();
    schematic.metadata.title_block.title = Some(composition.name.clone());
    schematic.nets = all_nets;
    schematic.components = all_components;
    schematic.lib_symbols = lib_symbols;

    let config = SexprConfig {
        kicad_version: Some(KicadVersion::V8),
        ..Default::default()
    };
    let mut gen = SexprGenerator::with_config(config);
    let output = gen.generate(&schematic)?;

    Ok(output)
}

fn compose_topology_module(
    module: &ModuleInstance,
    y_offset: f64,
    net_name_to_id: &mut HashMap<String, u32>,
    next_net_id: &mut u32,
    all_nets: &mut Vec<Net>,
    all_components: &mut Vec<SymbolInstance>,
    all_lib_ids: &mut HashSet<String>,
    ref_counter: &mut HashMap<String, usize>,
) -> Result<()> {
    let template = load_builtin_template(&module.template)?;
    let inputs = module
        .topology_inputs
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Missing topology_inputs for module '{}'", module.id))?;

    let _input_map = {
        let mut m = HashMap::new();
        m.insert("vin".to_string(), inputs.vin);
        m.insert("vout".to_string(), inputs.vout);
        m.insert("iout".to_string(), inputs.iout);
        m.insert("iout_max".to_string(), inputs.iout);
        m
    };

    // Map template net names → global net IDs with overrides from module.nets
    let mut net_id_map: HashMap<String, u32> = HashMap::new();
    for conn in &template.connections {
        let actual_name = module
            .nets
            .get(&conn.net)
            .cloned()
            .unwrap_or_else(|| conn.net.clone());
        let net_id =
            ensure_global_net(&actual_name, net_name_to_id, next_net_id, all_nets, "power");
        net_id_map.insert(conn.net.clone(), net_id);
    }

    // Build pin-to-net mapping
    let mut pin_net_map: HashMap<(String, String), u32> = HashMap::new();
    for conn in &template.connections {
        let net_id = *net_id_map
            .get(&conn.net)
            .context(format!("Net '{}' not found", conn.net))?;
        for pin_spec in &conn.pins {
            let parts: Vec<&str> = pin_spec.split('.').collect();
            if parts.len() == 2 {
                pin_net_map.insert((parts[0].to_string(), parts[1].to_string()), net_id);
            }
        }
    }

    // Build components with y_offset
    for slot in &template.components {
        let lib_id = if slot.lib.is_empty() {
            "custom:IC".to_string()
        } else {
            slot.lib.clone()
        };

        let prefix = match lib_id.as_str() {
            "Device:R" => "R",
            "Device:C" => "C",
            "Device:L" => "L",
            "Device:D" | "Device:LED" => "D",
            _ => "U",
        };
        let count = ref_counter.entry(prefix.to_string()).or_insert(0);
        *count += 1;
        let reference = format!("{}{}", prefix, *count);

        let pos = template.layout.get(&slot.role);
        let (x, y) = pos
            .map(|p| (p.x, p.y + y_offset))
            .unwrap_or((50.8, 50.8 + y_offset));

        let mut pins = Vec::new();
        let mut pin_idx = 0;
        for (key, &net_id) in &pin_net_map {
            if key.0 == slot.role {
                let pin_number = if lib_id.starts_with("Device:") {
                    pin_idx += 1;
                    pin_idx.to_string()
                } else {
                    key.1.clone()
                };
                pins.push(PinInstance {
                    number: pin_number,
                    name: key.1.clone(),
                    pin_type: "passive".to_string(),
                    net_id: Some(net_id),
                    net_name: None,
                    nc: false,
                });
            }
        }
        pins.sort_by(|a, b| a.number.cmp(&b.number));

        let mut comp = SymbolInstance::new(lib_id.clone(), reference);
        // Resolve "computed" values from pipeline outputs
        let value = if slot.value == "computed" {
            if let Some(&val) = module.computed_values.get(&slot.role) {
                format_component_value(&lib_id, val)
            } else {
                "computed".to_string()
            }
        } else {
            slot.value.clone()
        };
        comp.value = value;
        comp.position = (x, y, 0.0);
        comp.pins = pins;
        all_lib_ids.insert(lib_id);
        all_components.push(comp);
    }

    Ok(())
}

fn compose_ic_module(
    module: &ModuleInstance,
    y_offset: f64,
    net_name_to_id: &mut HashMap<String, u32>,
    next_net_id: &mut u32,
    all_nets: &mut Vec<Net>,
    all_components: &mut Vec<SymbolInstance>,
    all_lib_ids: &mut HashSet<String>,
    ref_counter: &mut HashMap<String, usize>,
) -> Result<()> {
    let template = ic_template::load_builtin_template(&module.template)?;
    let resolved = ic_template::resolve_params(&template, &module.params)?;

    let mut local_net_map: HashMap<String, u32> = HashMap::new();

    // Map interface ports → actual net names
    for (port_name, port_def) in &template.interface {
        let actual_name = module
            .nets
            .get(port_name)
            .cloned()
            .unwrap_or_else(|| port_name.clone());
        let net_id = ensure_global_net(
            &actual_name,
            net_name_to_id,
            next_net_id,
            all_nets,
            &port_def.port_type,
        );
        local_net_map.insert(port_name.clone(), net_id);
    }

    // IC component
    let ic_ref = alloc_ref(&template.ic.mpn, ref_counter);
    let ic_pos = template
        .layout
        .get("ic")
        .map(|p| (p.x, p.y + y_offset))
        .unwrap_or((50.8, 50.8 + y_offset));

    let mut ic_pins = Vec::new();
    for pin in &template.ic.pins {
        if let Some(&net_id) = local_net_map.get(&pin.name) {
            ic_pins.push(PinInstance {
                number: pin.number.clone(),
                name: pin.name.clone(),
                pin_type: pin.pin_type.clone(),
                net_id: Some(net_id),
                net_name: None,
                nc: false,
            });
        } else if pin.name == "NC" {
            ic_pins.push(PinInstance {
                number: pin.number.clone(),
                name: pin.name.clone(),
                pin_type: pin.pin_type.clone(),
                net_id: None,
                net_name: None,
                nc: true,
            });
        }
    }
    ic_pins.sort_by(|a, b| a.number.cmp(&b.number));

    let ic_lib_id = template.ic.mpn.clone();
    let mut ic_comp = SymbolInstance::new(ic_lib_id.clone(), ic_ref);
    ic_comp.value = template.ic.mpn.clone();
    ic_comp.position = (ic_pos.0, ic_pos.1, 0.0);
    ic_comp.footprint = if template.ic.footprint.is_empty() {
        None
    } else {
        Some(template.ic.footprint.clone())
    };
    ic_comp.pins = ic_pins;
    all_lib_ids.insert(ic_lib_id);
    all_components.push(ic_comp);

    // Peripherals
    for periph in &template.peripherals {
        let lib_id = if periph.lib.is_empty() {
            "custom:IC".to_string()
        } else {
            periph.lib.clone()
        };
        let reference = alloc_ref(&lib_id, ref_counter);

        let value = if periph.value == "computed" {
            if let Some(&val) = resolved.get(&periph.role) {
                format_resistance(val)
            } else {
                "computed".to_string()
            }
        } else {
            periph.value.clone()
        };

        let pos = template.layout.get(&periph.role);
        let (x, y) = pos
            .map(|p| (p.x, p.y + y_offset))
            .unwrap_or((76.2, 50.8 + y_offset));

        let mut pins = Vec::new();
        for (pin_num, target) in &periph.pins {
            let net_id = if let Some(&id) = local_net_map.get(target) {
                id
            } else {
                ensure_global_net(target, net_name_to_id, next_net_id, all_nets, "signal")
            };
            pins.push(PinInstance {
                number: pin_num.clone(),
                name: pin_num.clone(),
                pin_type: "passive".to_string(),
                net_id: Some(net_id),
                net_name: None,
                nc: false,
            });
        }
        pins.sort_by(|a, b| a.number.cmp(&b.number));

        let mut comp = SymbolInstance::new(lib_id.clone(), reference);
        comp.value = value;
        comp.position = (x, y, 0.0);
        if !periph.footprint.is_empty() {
            comp.footprint = Some(periph.footprint.clone());
        }
        comp.pins = pins;
        all_lib_ids.insert(lib_id);
        all_components.push(comp);
    }

    Ok(())
}

fn ensure_global_net(
    name: &str,
    map: &mut HashMap<String, u32>,
    next_id: &mut u32,
    nets: &mut Vec<Net>,
    port_type: &str,
) -> u32 {
    if let Some(&id) = map.get(name) {
        return id;
    }
    let id = *next_id;
    *next_id += 1;
    let render = if name == "GND" || name.starts_with("V") || port_type == "power" {
        RenderHint::Power
    } else {
        RenderHint::Wire
    };
    nets.push(Net {
        id,
        name: name.to_string(),
        net_type: None,
        render,
    });
    map.insert(name.to_string(), id);
    id
}

fn build_simple_device_symbol(lib_id: &str) -> Symbol {
    let mut sym = Symbol::new(lib_id);
    let prefix = match lib_id {
        "Device:R" => "R",
        "Device:C" => "C",
        "Device:L" => "L",
        "Device:D" => "D",
        _ => "U",
    };
    sym.reference = prefix.to_string();
    sym.pins.push(kicad_json5::ir::Pin {
        number: "1".to_string(),
        name: "1".to_string(),
        pin_type: "passive".to_string(),
    });
    sym.pins.push(kicad_json5::ir::Pin {
        number: "2".to_string(),
        name: "2".to_string(),
        pin_type: "passive".to_string(),
    });
    sym
}

/// Apply board-level auto-layout to a Schematic, assigning positions to all components.
pub fn apply_board_layout(schematic: &mut Schematic) {
    layout::apply_to(schematic);
}

/// Auto-route power and ground nets using simple L-shaped Manhattan routing.
/// Uses NetClass-aware trace widths from directives when available.
pub fn auto_route_power_nets(
    board: &mut kicad_json5::ir::Board,
    directives: &crate::layout_directives::LayoutDirectives,
) {
    // Collect pad positions grouped by net
    let mut pads_by_net: HashMap<u32, Vec<(f64, f64, String)>> = HashMap::new();
    for fp in &board.footprints {
        for pad in &fp.pads {
            if let Some(net_id) = pad.net {
                let (fx, fy, _) = fp.position;
                pads_by_net
                    .entry(net_id)
                    .or_default()
                    .push((fx, fy, fp.reference.clone()));
            }
        }
    }

    // Route each power/ground net (sorted by net_id for determinism)
    let mut sorted_pbn: Vec<_> = pads_by_net.iter().collect();
    sorted_pbn.sort_by_key(|(id, _)| *id);
    for (net_id, pads) in &sorted_pbn {
        if pads.len() < 2 {
            continue;
        }

        // Determine trace width from net name
        let net_name = board
            .nets
            .iter()
            .find(|n| n.id == **net_id)
            .map(|n| n.name.as_str())
            .unwrap_or("");
        let trace_width = directives.trace_width_for(net_name);
        // Skip signal nets (no auto-routing)
        let upper = net_name.to_uppercase();
        if upper != "GND" && !upper.contains("V") && !upper.contains("5V") && !upper.contains("3V3")
        {
            continue;
        }

        // Nearest-neighbor chain routing with 45° mitered corners
        let mut visited = vec![false; pads.len()];
        visited[0] = true;

        for _ in 1..pads.len() {
            let current_idx = visited.iter().position(|v| *v).unwrap_or(0);
            let (cx, cy, _) = pads[current_idx];

            let mut best_dist = f64::MAX;
            let mut best_idx = 1;

            for (j, (px, py, _)) in pads.iter().enumerate() {
                if visited[j] {
                    continue;
                }
                let dist = (cx - px).abs() + (cy - py).abs();
                if dist < best_dist {
                    best_dist = dist;
                    best_idx = j;
                }
            }

            if best_idx < pads.len() {
                let (tx, ty, _) = pads[best_idx];
                route_mitered_lshape(
                    &mut board.segments,
                    (cx, cy),
                    (tx, ty),
                    trace_width,
                    "F.Cu",
                    **net_id,
                );
                visited[best_idx] = true;
            }
        }
    }
}

/// Route a connection between two points using 45° mitered L-shape.
/// Instead of a sharp 90° corner, inserts a 45° diagonal segment at the bend.
///
/// ```text
/// (sx,sy) ──────╲         ← horizontal + 45° diagonal + vertical
///                ╲
///                 ╲
///                  │
///              (ex,ey)
/// ```
fn route_mitered_lshape(
    segments: &mut Vec<kicad_json5::ir::board::Segment>,
    start: (f64, f64),
    end: (f64, f64),
    width: f64,
    layer: &str,
    net: u32,
) {
    use kicad_json5::ir::board::Segment;
    let (sx, sy) = start;
    let (ex, ey) = end;
    let dx = ex - sx;
    let dy = ey - sy;

    // If nearly horizontal or vertical, just one segment
    if dy.abs() < 0.01 {
        segments.push(Segment {
            start: (sx, sy),
            end: (ex, ey),
            width,
            layer: layer.into(),
            net,
        });
        return;
    }
    if dx.abs() < 0.01 {
        segments.push(Segment {
            start: (sx, sy),
            end: (ex, ey),
            width,
            layer: layer.into(),
            net,
        });
        return;
    }

    // Miter length: the diagonal segment at the 45° corner.
    // Use the smaller of |dx|, |dy| to keep the diagonal within the L shape.
    let miter = dx.abs().min(dy.abs()) * 0.5; // half of the shorter leg
    let miter = miter.min(1.0); // cap at 1mm for aesthetic

    // Direction signs
    let sign_x = dx.signum();
    let sign_y = dy.signum();

    // Corner point: horizontal first, then 45° diagonal, then vertical
    // Horizontal endpoint: travel (|dx| - miter) in x
    let h_end_x = sx + sign_x * (dx.abs() - miter);
    let _h_end_y = sy;

    // 45° diagonal endpoint: travel miter in both x and y
    let diag_end_x = h_end_x + sign_x * miter;
    let diag_end_y = sy + sign_y * miter;

    // Segment 1: horizontal (sx,sy) → (h_end_x, sy)
    if (h_end_x - sx).abs() > 0.01 {
        segments.push(Segment {
            start: (sx, sy),
            end: (h_end_x, sy),
            width,
            layer: layer.into(),
            net,
        });
    }

    // Segment 2: 45° diagonal (h_end_x, sy) → (diag_end_x, diag_end_y)
    if miter > 0.01 {
        segments.push(Segment {
            start: (h_end_x, sy),
            end: (diag_end_x, diag_end_y),
            width,
            layer: layer.into(),
            net,
        });
    }

    // Segment 3: vertical (diag_end_x, diag_end_y) → (ex, ey)
    if (ey - diag_end_y).abs() > 0.01 {
        segments.push(Segment {
            start: (diag_end_x, diag_end_y),
            end: (ex, ey),
            width,
            layer: layer.into(),
            net,
        });
    }
}

/// Generate copper zones for GND and power nets.
/// - GND: full-board zone on B.Cu (ground plane)
/// - Power nets (VIN, 5V, 3V3, etc.): zone on F.Cu covering component area
///   Extract board outline from Edge.Cuts graphics.
///   Falls back to footprint bounding box + 5mm margin if no Edge.Cuts found.
pub fn extract_board_outline(board: &kicad_json5::ir::Board) -> Vec<(f64, f64)> {
    use kicad_json5::ir::board::BoardGraphicKind;

    // Try to find Edge.Cuts graphics
    let edge_graphics: Vec<_> = board
        .graphics
        .iter()
        .filter(|g| g.layer == "Edge.Cuts")
        .collect();

    if !edge_graphics.is_empty() {
        // Compute bbox from Edge.Cuts graphics
        let mut min_x = f64::MAX;
        let mut min_y = f64::MAX;
        let mut max_x = f64::MIN;
        let mut max_y = f64::MIN;
        for gr in &edge_graphics {
            match &gr.kind {
                BoardGraphicKind::Rect { start, end } => {
                    min_x = min_x.min(start.0).min(end.0);
                    min_y = min_y.min(start.1).min(end.1);
                    max_x = max_x.max(start.0).max(end.0);
                    max_y = max_y.max(start.1).max(end.1);
                }
                BoardGraphicKind::Line { start, end } => {
                    min_x = min_x.min(start.0).min(end.0);
                    min_y = min_y.min(start.1).min(end.1);
                    max_x = max_x.max(start.0).max(end.0);
                    max_y = max_y.max(start.1).max(end.1);
                }
                BoardGraphicKind::Circle { center, end } => {
                    let r = ((end.0 - center.0).powi(2) + (end.1 - center.1).powi(2)).sqrt();
                    min_x = min_x.min(center.0 - r);
                    min_y = min_y.min(center.1 - r);
                    max_x = max_x.max(center.0 + r);
                    max_y = max_y.max(center.1 + r);
                }
                BoardGraphicKind::Poly { points } => {
                    for (px, py) in points {
                        min_x = min_x.min(*px);
                        min_y = min_y.min(*py);
                        max_x = max_x.max(*px);
                        max_y = max_y.max(*py);
                    }
                }
                BoardGraphicKind::Arc { start, mid, end } => {
                    for (x, y) in &[start, mid, end] {
                        min_x = min_x.min(*x);
                        min_y = min_y.min(*y);
                        max_x = max_x.max(*x);
                        max_y = max_y.max(*y);
                    }
                }
                BoardGraphicKind::Text { position, .. } => {
                    min_x = min_x.min(position.0);
                    min_y = min_y.min(position.1);
                    max_x = max_x.max(position.0);
                    max_y = max_y.max(position.1);
                }
            }
        }
        if min_x < max_x && min_y < max_y {
            return vec![
                (min_x, min_y),
                (max_x, min_y),
                (max_x, max_y),
                (min_x, max_y),
            ];
        }
    }

    // Fallback: footprint bbox + 5mm margin
    let mut min_x = f64::MAX;
    let mut min_y = f64::MAX;
    let mut max_x = f64::MIN;
    let mut max_y = f64::MIN;
    for fp in &board.footprints {
        let (fx, fy, _) = fp.position;
        min_x = min_x.min(fx - 5.0);
        min_y = min_y.min(fy - 5.0);
        max_x = max_x.max(fx + 5.0);
        max_y = max_y.max(fy + 5.0);
    }
    vec![
        (min_x, min_y),
        (max_x, min_y),
        (max_x, max_y),
        (min_x, max_y),
    ]
}

pub fn generate_copper_zones(
    board: &mut kicad_json5::ir::Board,
    layer_config: &crate::layer_config::BoardLayerConfig,
) {
    use kicad_json5::ir::board::*;

    if board.footprints.is_empty() {
        return;
    }

    // Extract board outline from Edge.Cuts graphics; fallback to footprint bbox
    let outline = extract_board_outline(board);

    // Determine target layers for zones
    let gnd_zone_layer = layer_config.ground_plane_layer().unwrap_or("B.Cu");
    let pwr_zone_layer = layer_config.power_plane_layer().unwrap_or("F.Cu");

    // 1. GND zone(s) — P2-2 for grounds: ONE zone per layer. Stacking AGND/DGND/
    //    GND_STAR zones full-outline on the same layer leaves them mutually
    //    clearance-cut at refill (no fill wins, pads starve). The GND net with
    //    the most pads takes the ground plane layer; the next ones get B.Cu.
    let is_gnd = |name: &str| {
        let n = name.to_uppercase();
        n == "GND"
            || n == "DGND"
            || n == "AGND"
            || n.starts_with("GND_")
            || n.ends_with("_GND")
            || n == "PGND"
            || n == "SGND"
            || n == "EGND"
            || n == "CHASSIS_GND"
    };
    let pad_count_on_net = |net_id: u32| -> usize {
        board
            .footprints
            .iter()
            .flat_map(|fp| fp.pads.iter())
            .filter(|p| p.net == Some(net_id))
            .count()
    };
    let mut gnd_nets: Vec<(u32, String, usize)> = board
        .nets
        .iter()
        .filter(|n| is_gnd(&n.name))
        .map(|n| (n.id, n.name.clone(), pad_count_on_net(n.id)))
        .collect();
    gnd_nets.sort_by_key(|(_, _, c)| std::cmp::Reverse(*c));
    {
        let mut zone_layers: Vec<(&str, Option<(u32, String)>)> = vec![
            (
                gnd_zone_layer,
                gnd_nets.first().map(|(i, n, _)| (*i, n.clone())),
            ),
            ("B.Cu", gnd_nets.get(1).map(|(i, n, _)| (*i, n.clone()))),
        ];
        if let (Some(_), Some(_)) = (gnd_nets.first(), gnd_nets.get(1)) {
            eprintln!(
                "[pdn] GND planes: {:?} on {} / {:?} on B.Cu / {:?} on F.Cu",
                gnd_nets.first().map(|(_, n, _)| n),
                gnd_zone_layer,
                gnd_nets.get(1).map(|(_, n, _)| n),
                gnd_nets.get(2).map(|(_, n, _)| n)
            );
        }
        for (layer, net) in zone_layers.drain(..) {
            if let Some((net_id, net_name)) = net {
                board.zones.push(Zone {
                    net: net_id,
                    net_name,
                    layer: layer.into(),
                    hatch_style: "edge".into(),
                    hatch_pitch: 0.508,
                    pad_connect: "thermal".into(),
                    connect_pads_clearance: 0.3,
                    min_thickness: 0.254,
                    fill: true,
                    thermal_gap: 0.508,
                    thermal_bridge_width: 0.508,
                    island_removal_mode: 0,
                    island_area: 10.0,
                    outline: outline.clone(),
                    filled_polygons: Vec::new(),
                    keepout: None,
                });
            }
        }
    }

    // 2. Power zones — P2-2: at most ONE per layer. Multiple full-outline power
    //    zones stacked on the same layer are an instant short the moment zones
    //    refill (battery board: 5V/3V3/VBAT zones all on F.Cu). The rail with the
    //    most pads wins the pour; the rest ship as wide traces (net classes).
    let power_nets: Vec<(u32, String)> = board
        .nets
        .iter()
        .filter(|n| {
            let name = n.name.to_uppercase();
            let is_gnd_net = name == "GND"
                || name == "DGND"
                || name == "AGND"
                || name.starts_with("GND_")
                || name.ends_with("_GND")
                || name == "PGND"
                || name == "SGND";
            !n.name.is_empty()
                && !is_gnd_net
                && (name.contains("VIN")
                    || name.contains("VCC")
                    || name.contains("VDD")
                    || name.contains("5V")
                    || name.contains("3V3")
                    || name.contains("3.3V")
                    || name.contains("12V")
                    || name.contains("9V")
                    || name.contains("VOUT")
                    || name.contains("+2V")
                    || name.contains("+4V")
                    || name.contains("+15V"))
        })
        .map(|n| (n.id, n.name.clone()))
        .collect();

    let pad_count_on = |net_id: u32| -> usize {
        board
            .footprints
            .iter()
            .flat_map(|fp| fp.pads.iter())
            .filter(|p| p.net == Some(net_id))
            .count()
    };
    let primary_power = power_nets
        .iter()
        .map(|(id, name)| (*id, name.clone(), pad_count_on(*id)))
        .filter(|(_, _, c)| *c >= 2)
        .max_by_key(|(_, _, c)| *c);

    match primary_power {
        Some((net_id, net_name, _)) => {
            if power_nets.len() > 1 {
                let others: Vec<&String> = power_nets
                    .iter()
                    .map(|(_, n)| n)
                    .filter(|n| *n != &net_name)
                    .collect();
                eprintln!("[pdn] 2L template: '{}' wins the {} pour; rails {:?} ship as wide traces (net classes)",
                    net_name, pwr_zone_layer, others);
            }
            board.zones.push(Zone {
                net: net_id,
                net_name: net_name.clone(),
                layer: pwr_zone_layer.into(),
                hatch_style: "edge".into(),
                hatch_pitch: 0.508,
                pad_connect: "thermal".into(),
                connect_pads_clearance: 0.2,
                min_thickness: 0.254,
                fill: true,
                thermal_gap: 0.508,
                thermal_bridge_width: 0.508,
                island_removal_mode: 0,
                island_area: 10.0,
                outline: outline.clone(),
                filled_polygons: Vec::new(),
                keepout: None,
            });
        }
        None => {
            eprintln!("[pdn] no multi-pad power rail found — power shipped as traces only");
        }
    }
}
/// Generate thermal vias for power/GND pads to connect to inner planes on 4-layer boards.
pub fn generate_plane_thermal_vias(
    board: &mut kicad_json5::ir::Board,
    layer_config: &crate::layer_config::BoardLayerConfig,
) {
    use kicad_json5::ir::board::Via;

    let (first_layer, last_layer) = layer_config.via_layer_names();

    // Get board outline bounds for clipping
    let outline = extract_board_outline(board);
    let (bx_min, by_min, bx_max, by_max) = if outline.len() >= 2 {
        let xs: Vec<f64> = outline.iter().map(|p| p.0).collect();
        let ys: Vec<f64> = outline.iter().map(|p| p.1).collect();
        (
            xs.iter().cloned().fold(f64::MAX, f64::min),
            ys.iter().cloned().fold(f64::MAX, f64::min),
            xs.iter().cloned().fold(f64::MIN, f64::max),
            ys.iter().cloned().fold(f64::MIN, f64::max),
        )
    } else {
        (f64::MIN, f64::MIN, f64::MAX, f64::MAX)
    };

    // P1: stitch zone nets to their plane layer. Only SMD pads whose layer set
    // lacks the zone layer need a via — an F.Cu zone feeds F.Cu pads directly.
    let zone_layer: std::collections::HashMap<u32, String> = board
        .zones
        .iter()
        .map(|z| (z.net, z.layer.clone()))
        .collect();
    if zone_layer.is_empty() {
        return;
    }

    // World-space pad census for clearance checks: (x, y, half_diagonal, net)
    let pad_census: Vec<(f64, f64, f64, u32)> = board
        .footprints
        .iter()
        .flat_map(|fp| {
            let (fx, fy, _) = fp.position;
            fp.pads.iter().filter_map(move |p| {
                let id = p.net?;
                let (ox, oy) = fp.pad_rotated_offset(p);
                let hd = (p.size.0.hypot(p.size.1)) / 2.0;
                Some((fx + ox, fy + oy, hd, id))
            })
        })
        .collect();
    // Through-hole drills for hole-to-hole checks: (x, y, drill_diameter, net)
    let th_drills: Vec<(f64, f64, f64, u32)> = board
        .footprints
        .iter()
        .flat_map(|fp| {
            let (fx, fy, _) = fp.position;
            fp.pads.iter().filter_map(move |p| {
                let d = p.drill.as_ref()?.diameter;
                if d <= 0.0 {
                    return None;
                }
                let id = p.net.unwrap_or(0);
                let (ox, oy) = fp.pad_rotated_offset(p);
                Some((fx + ox, fy + oy, d, id))
            })
        })
        .collect();

    const VIA_R: f64 = 0.3; // via copper radius (size 0.6)
    const CLR: f64 = 0.2; // copper clearance to dissimilar nets
    const H2H: f64 = 0.25; // hole-to-hole edge margin (KiCad hole_to_hole)
    const EDGE: f64 = 0.5; // keep-out from board outline

    let clear_at = |x: f64, y: f64, net_id: u32, placed: &[(f64, f64, u32)]| -> bool {
        if pad_census.iter().any(|&(px, py, hd, pn)| {
            pn != net_id && (px - x).hypot(py - y) < VIA_R + hd + CLR - 0.01
        }) {
            return false;
        }
        if th_drills.iter().any(|&(px, py, d, pn)| {
            pn != net_id && (px - x).hypot(py - y) < d / 2.0 + VIA_R + H2H - 0.01
        }) {
            return false;
        }
        if placed
            .iter()
            .any(|&(px, py, pn)| pn == net_id && (px - x).hypot(py - y) < VIA_R + H2H)
        {
            return false;
        }
        x >= bx_min + EDGE && x <= bx_max - EDGE && y >= by_min + EDGE && y <= by_max - EDGE
    };

    let mut via_count = 0usize;
    let mut skipped = 0usize;
    for fp in &board.footprints {
        let (fx, fy, _) = fp.position;
        for pad in &fp.pads {
            let net_id = match pad.net {
                Some(id) if zone_layer.contains_key(&id) => id,
                _ => continue,
            };
            let zlayer = &zone_layer[&net_id];
            if pad.pad_type != kicad_json5::ir::board::PadType::Smd {
                continue;
            }
            if pad.layers.iter().any(|l| l == zlayer) {
                continue;
            }

            let (ox, oy) = fp.pad_rotated_offset(pad);
            let wx = fx + ox;
            let wy = fy + oy;

            // Candidate spots: on-pad (only when the via fits inside the pad
            // with mask margin — QFN 0.4mm pads never qualify), then outward
            // offsets (center→pad direction = perpendicular escape from IC
            // edges), then lateral alternates.
            let plen = pad.size.0.hypot(pad.size.1);
            let mut candidates: Vec<(f64, f64)> = Vec::new();
            if plen / 2.0 >= VIA_R + 0.05 {
                candidates.push((wx, wy));
            }
            let len = ox.hypot(oy);
            let (ux, uy) = if len > 1e-6 {
                (ox / len, oy / len)
            } else {
                (1.0, 0.0)
            };
            for &d in &[0.45, 0.65, 0.85, 1.1, 1.4] {
                candidates.push((wx + ux * d, wy + uy * d));
            }
            for &d in &[0.5, 0.9] {
                candidates.push((wx - uy * d, wy + ux * d));
                candidates.push((wx + uy * d, wy - ux * d));
            }

            let placed_nearby: Vec<(f64, f64, u32)> = board
                .vias
                .iter()
                .filter(|v| v.net == net_id)
                .map(|v| (v.at.0, v.at.1, v.net))
                .collect();
            let mut chosen: Option<(f64, f64)> = None;
            for (cx, cy) in candidates {
                if clear_at(cx, cy, net_id, &placed_nearby) {
                    chosen = Some((cx, cy));
                    break;
                }
            }
            let Some((vx, vy)) = chosen else {
                skipped += 1;
                continue;
            };
            board.vias.push(Via {
                at: (vx, vy),
                size: 0.6,
                drill: 0.3,
                layers: vec![first_layer.clone(), last_layer.clone()],
                net: net_id,
            });
            via_count += 1;
        }
    }

    eprintln!(
        "[5b] Plane thermal vias: {} placed, {} skipped (no clear spot) for {} zone nets",
        via_count,
        skipped,
        zone_layer.len()
    );
}

/// Generate thermal management features: copper zones and thermal via arrays
/// for power-dissipating components based on thermal directives.
fn generate_thermal_features(
    board: &mut kicad_json5::ir::Board,
    directives: &crate::layout_directives::LayoutDirectives,
) {
    use kicad_json5::ir::board::*;

    if directives.thermal.is_empty() {
        return;
    }

    let is_gnd = |name: &str| {
        let n = name.to_uppercase();
        n == "GND"
            || n == "DGND"
            || n == "AGND"
            || n.starts_with("GND_")
            || n.ends_with("_GND")
            || n == "PGND"
            || n == "SGND"
            || n == "EGND"
            || n == "CHASSIS_GND"
    };
    let gnd_net_id = board.nets.iter().find(|n| is_gnd(&n.name)).map(|n| n.id);

    for td in &directives.thermal {
        if td.copper_area_mm2 < 1.0 {
            continue;
        }

        // Find the target component — match by tag if ref is empty
        let target_fp = if !td.component_ref.is_empty() {
            board
                .footprints
                .iter()
                .find(|fp| fp.reference == td.component_ref)
        } else {
            // Match any power-dissipating component (IC, MOSFET, power passive)
            board.footprints.iter().find(|fp| {
                let lib = fp.lib_id.to_uppercase();
                lib.contains("QFP")
                    || lib.contains("QFN")
                    || lib.contains("SOIC")
                    || lib.contains("TSSOP")
                    || lib.contains("MSOP")
                    || lib.contains("BGA")
                    || lib.contains("TO-220")
                    || lib.contains("TO-252")
                    || lib.contains("DPAK")
            })
        };

        let (cx, cy, _) = match target_fp {
            Some(fp) => fp.position,
            None => continue,
        };

        // Copper zone centered on component
        let side = td.copper_area_mm2.sqrt();
        let zone_outline = vec![
            (cx - side / 2.0, cy - side / 2.0),
            (cx + side / 2.0, cy - side / 2.0),
            (cx + side / 2.0, cy + side / 2.0),
            (cx - side / 2.0, cy + side / 2.0),
        ];

        // Thermal copper zone on F.Cu connected to GND (or no net)
        if let Some(gnd_id) = gnd_net_id {
            board.zones.push(Zone {
                net: gnd_id,
                net_name: "GND".into(),
                layer: "F.Cu".into(),
                hatch_style: "edge".into(),
                hatch_pitch: 0.508,
                pad_connect: "thermal".into(),
                connect_pads_clearance: 0.3,
                min_thickness: 0.254,
                fill: true,
                thermal_gap: 0.508,
                thermal_bridge_width: 0.508,
                island_removal_mode: 0,
                island_area: 10.0,
                outline: zone_outline,
                filled_polygons: Vec::new(),
                keepout: None,
            });
        }

        // Thermal via array under component
        if td.via_count > 0 {
            let spacing = td.via_grid_spacing_mm;
            let half_side = side / 2.0;
            let gnd_id = gnd_net_id.unwrap_or(0);

            // Grid of vias
            let count_per_side = (td.via_count as f64).sqrt().ceil() as usize;
            for ix in 0..count_per_side {
                for iy in 0..count_per_side {
                    let vx = cx - half_side + (ix as f64 + 0.5) * spacing;
                    let vy = cy - half_side + (iy as f64 + 0.5) * spacing;
                    // Stay within component bounds
                    if vx < cx - half_side || vx > cx + half_side {
                        continue;
                    }
                    if vy < cy - half_side || vy > cy + half_side {
                        continue;
                    }
                    board.vias.push(Via {
                        at: (vx, vy),
                        size: 0.6,
                        drill: 0.3,
                        layers: vec!["F.Cu".into(), "B.Cu".into()],
                        net: gnd_id,
                    });
                }
            }
        }
    }
}

/// Bake the footprint's own rotation into its pad/graphic geometry, then zero
/// out the rotation angle.
///
/// KiCad 10 rotates a footprint's pad positions by the footprint angle but
/// keeps pad sizes axis-aligned (W = board X, H = board Y). A 90/270°
/// footprint therefore ends up with swapped pad dimensions — adjacent
/// fine-pitch pads overlap and DRC reports shorts between neighbouring pins.
/// KiCad 9 rotated positions and sizes together. Baking the rotation into the
/// geometry (positions pre-rotated, sizes swapped for 90/270°) is correct
/// under both semantics and version-independent.
pub fn bake_footprint_rotation(fp: &mut kicad_json5::ir::board::Footprint) {
    let pr = ((fp.position.2 % 360.0) + 360.0) % 360.0;
    if pr == 0.0 {
        return;
    }
    let rot = |p: (f64, f64)| -> (f64, f64) {
        match pr {
            90.0 => (p.1, -p.0),
            180.0 => (-p.0, -p.1),
            270.0 => (-p.1, p.0),
            other => {
                let r = other.to_radians();
                (p.0 * r.cos() - p.1 * r.sin(), p.0 * r.sin() + p.1 * r.cos())
            }
        }
    };
    let swap_size = matches!(pr, 90.0 | 270.0);
    for pad in &mut fp.pads {
        let (x, y) = rot((pad.position.0, pad.position.1));
        pad.position.0 = x;
        pad.position.1 = y;
        if swap_size
            && !matches!(
                pad.shape,
                kicad_json5::ir::board::PadShape::Circle | kicad_json5::ir::board::PadShape::Custom
            )
        {
            pad.size = (pad.size.1, pad.size.0);
        }
    }
    for l in &mut fp.fp_lines {
        l.start = rot(l.start);
        l.end = rot(l.end);
    }
    for c in &mut fp.fp_circles {
        c.center = rot(c.center);
        c.end = rot(c.end);
    }
    for r in &mut fp.fp_rects {
        r.start = rot(r.start);
        r.end = rot(r.end);
    }
    for a in &mut fp.fp_arcs {
        a.start = rot(a.start);
        a.mid = rot(a.mid);
        a.end = rot(a.end);
    }
    for poly in &mut fp.fp_polys {
        for pt in poly.points.iter_mut() {
            *pt = rot(*pt);
        }
    }
    for t in &mut fp.fp_texts {
        let (x, y) = rot((t.position.0, t.position.1));
        t.position.0 = x;
        t.position.1 = y;
    }
    fp.position.2 = 0.0;
}

/// Maps components → footprints, nets → board nets, adds board outline.
pub fn schematic_to_board(schematic: &Schematic) -> Result<kicad_json5::ir::Board> {
    schematic_to_board_with_directives(
        schematic,
        &crate::layout_directives::LayoutDirectives::default(),
    )
}

/// Board generation with design rule directives for thermal, EMC, and SI features.
pub fn schematic_to_board_with_directives(
    schematic: &Schematic,
    directives: &crate::layout_directives::LayoutDirectives,
) -> Result<kicad_json5::ir::Board> {
    schematic_to_board_with_config(
        schematic,
        directives,
        &crate::layer_config::BoardLayerConfig::two_layer(),
    )
}

/// Board generation with explicit layer configuration.
pub fn schematic_to_board_with_config(
    schematic: &Schematic,
    directives: &crate::layout_directives::LayoutDirectives,
    layer_config: &crate::layer_config::BoardLayerConfig,
) -> Result<kicad_json5::ir::Board> {
    schematic_to_board_with_config_fixed(schematic, directives, layer_config, None)
}

pub fn schematic_to_board_with_config_fixed(
    schematic: &Schematic,
    directives: &crate::layout_directives::LayoutDirectives,
    layer_config: &crate::layer_config::BoardLayerConfig,
    fixed_board_size: Option<(f64, f64)>,
) -> Result<kicad_json5::ir::Board> {
    schematic_to_board_with_config_fixed_skip_route(
        schematic,
        directives,
        layer_config,
        fixed_board_size,
        false,
    )
}

pub fn schematic_to_board_with_config_fixed_skip_route(
    schematic: &Schematic,
    directives: &crate::layout_directives::LayoutDirectives,
    layer_config: &crate::layer_config::BoardLayerConfig,
    fixed_board_size: Option<(f64, f64)>,
    skip_signal_route: bool,
) -> Result<kicad_json5::ir::Board> {
    use kicad_json5::ir::board::*;

    let mut board = Board::new();
    board.generator = "kicad-designer".into();

    // Set layer stackup from config
    board.layers = layer_config.to_layer_defs();

    // 1. Map nets: schematic net → board net
    let mut net_map: HashMap<u32, u32> = HashMap::new();
    for net in &schematic.nets {
        if net.name.is_empty() {
            net_map.insert(net.id, 0);
            continue;
        }
        let board_net_id = board.find_or_add_net(&net.name);
        net_map.insert(net.id, board_net_id);
    }

    // 2. Run constraint-based auto-layout engine with directives
    let layout_positions = crate::layout_engine::auto_layout_refs_with_directives_fixed(
        schematic,
        directives,
        fixed_board_size,
    );

    // 3. Map components → footprints
    for comp in &schematic.components {
        let footprint_id = comp.footprint.as_deref().unwrap_or({
            // Default footprint assignment based on lib_id
            match comp.lib_id.as_str() {
                "Device:R" => "Resistor_SMD:R_0805_2012Metric",
                "Device:C" => "Capacitor_SMD:C_0805_2012Metric",
                "Device:L" => "Inductor_SMD:L_0805_2012Metric",
                "Device:D" | "Device:LED" => "Diode_SMD:D_0805_2012Metric",
                _ => "custom:Default",
            }
        });

        // Use layout engine position, fallback to schematic position
        let (px, py, pr) = layout_positions
            .get(&comp.reference)
            .copied()
            .unwrap_or(comp.position);

        let mut fp = Footprint::new(footprint_id, &comp.reference, &comp.value);
        fp.position = (px, py, pr);
        fp.layer = "F.Cu".into();

        // Compute body size and pin offsets for proper pad placement
        let pin_count = comp.pins.len();
        let (body_w, body_h) = crate::layout_engine::infer_body_size(footprint_id, pin_count);
        let pin_offsets =
            crate::layout_engine::build_pin_offsets(footprint_id, &comp.pins, body_w, body_h);
        let pad_params = crate::layout_engine::infer_pad_params(footprint_id, pin_count);

        let _offset_map: HashMap<&str, (f64, f64)> = pin_offsets
            .iter()
            .map(|p| (p.name.as_str(), (p.dx, p.dy)))
            .collect();

        // Map pins → pads with proper positions and sizes.
        // Fine-pitch quad packages (QFN/QFP at ≤0.5mm): pad aspect must follow
        // the side — long axis OUT of the body, short (0.2mm at P0.4) along
        // the pitch direction, else adjacent pads violate clearance.
        let quad_fine = (footprint_id.to_uppercase().contains("QFN")
            || footprint_id.to_uppercase().contains("QFP"))
            && crate::layout_engine::parse_named_pitch(&footprint_id.to_uppercase())
                .map(|p| p <= 0.5)
                .unwrap_or(false);
        let offset_map: HashMap<&str, (f64, f64)> = pin_offsets
            .iter()
            .map(|p| (p.name.as_str(), (p.dx, p.dy)))
            .collect();

        for pin in &comp.pins {
            let pad_net = pin.net_id.and_then(|nid| net_map.get(&nid)).copied();
            let (dx, dy) = offset_map
                .get(pin.number.as_str())
                .copied()
                .unwrap_or((0.0, 0.0));

            let mut pad_size = pad_params.size;
            if quad_fine {
                pad_size = if dx.abs() >= dy.abs() {
                    (0.875, 0.2)
                } else {
                    (0.2, 0.875)
                };
            }
            let pad = Pad {
                number: pin.number.clone(),
                pad_type: pad_params.pad_type,
                shape: pad_params.shape,
                position: (dx, dy, 0.0),
                size: pad_size,
                layers: pad_params.layers.clone(),
                drill: pad_params.drill.clone(),
                net: pad_net,
                net_name: None,
                pin_function: None,
                pin_type: None,
                roundrect_rratio: pad_params.roundrect_rratio,
                solder_mask_margin: None,
                thermal_bridge_width: None,
                thermal_bridge_angle: None,
                thermal_gap: None,
                clearance: None,
                zone_connect: None,
                remove_unused_layers: None,
                options: None,
                primitives: Vec::new(),
            };
            fp.pads.push(pad);
        }

        // QFN/QFP exposed pad: name carries "EP<w>x<h>" (e.g. EP5.6x5.6mm).
        // Pin "57" carries its net when the schematic defines one.
        {
            let up = footprint_id.to_uppercase();
            let ep_pos = up
                .match_indices("EP")
                .find(|(i, _)| {
                    up[i + 2..]
                        .chars()
                        .next()
                        .map(|c| c.is_ascii_digit())
                        .unwrap_or(false)
                })
                .map(|(i, _)| i);
            if let Some(pos) = ep_pos {
                let rest = &up[pos + 2..];
                let dims: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == 'X')
                    .collect();
                let parts: Vec<&str> = dims.split('X').collect();
                if parts.len() == 2 {
                    if let (Ok(ew), Ok(eh)) = (parts[0].parse::<f64>(), parts[1].parse::<f64>()) {
                        if ew > 1.0 && eh > 1.0 {
                            let ep_net =
                                comp.pins.iter().find(|p| p.number == "57").and_then(|p| {
                                    p.net_id.and_then(|nid| net_map.get(&nid)).copied()
                                });
                            fp.pads.push(Pad {
                                number: "57".into(),
                                pad_type: kicad_json5::ir::board::PadType::Smd,
                                shape: kicad_json5::ir::board::PadShape::RoundRect,
                                position: (0.0, 0.0, 0.0),
                                size: (ew, eh),
                                layers: pad_params.layers.clone(),
                                drill: None,
                                net: ep_net,
                                net_name: None,
                                pin_function: None,
                                pin_type: None,
                                roundrect_rratio: Some(0.25),
                                solder_mask_margin: None,
                                thermal_bridge_width: None,
                                thermal_bridge_angle: None,
                                thermal_gap: None,
                                clearance: None,
                                zone_connect: None,
                                remove_unused_layers: None,
                                options: None,
                                primitives: Vec::new(),
                            });
                        }
                    }
                }
            }
        }

        bake_footprint_rotation(&mut fp);
        board.footprints.push(fp);
    }

    // 3. Generate board outline (rectangular)
    if !board.footprints.is_empty() {
        let (outline_min, outline_max) = if let Some((fw, fh)) = fixed_board_size {
            // Fixed-size mode: the user owns the floorplan — the outline is
            // exactly the requested size, origin at (0,0). Post-processing
            // (place_post.py) owns the final placement margins.
            ((0.0, 0.0), (fw, fh))
        } else {
            let mut min_x = f64::MAX;
            let mut min_y = f64::MAX;
            let mut max_x = f64::MIN;
            let mut max_y = f64::MIN;

            for fp in &board.footprints {
                let (fx, fy, _) = fp.position;
                let (body_w, body_h) =
                    crate::layout_engine::infer_body_size(&fp.lib_id, fp.pads.len());
                min_x = min_x.min(fx - body_w / 2.0);
                min_y = min_y.min(fy - body_h / 2.0);
                max_x = max_x.max(fx + body_w / 2.0);
                max_y = max_y.max(fy + body_h / 2.0);
            }

            // Add margin
            let margin = 3.0;
            (
                (min_x - margin, min_y - margin),
                (max_x + margin, max_y + margin),
            )
        };

        // Board outline as gr_rect on Edge.Cuts
        board.graphics.push(BoardGraphic {
            kind: BoardGraphicKind::Rect {
                start: outline_min,
                end: outline_max,
            },
            layer: "Edge.Cuts".into(),
            stroke_width: 0.15,
            fill: false,
        });

        // Update paper size based on board dimensions
        let board_w = outline_max.0 - outline_min.0;
        let board_h = outline_max.1 - outline_min.1;
        if board_w > 0.0 && board_h > 0.0 {
            board.paper = format!("User {} {}", board_w + 20.0, board_h + 20.0);
        }
    }

    // 4. Auto-route power/ground nets (always — on 4-layer boards, inner planes supplement)
    auto_route_power_nets(&mut board, directives);

    // 4b-6. Route signals, add zones and thermal features
    if !skip_signal_route {
        route_board(&mut board, directives, layer_config);
    }

    // 7. Trim board outline to actual content bounding box.
    // Fixed-size mode: the outline is the user's contract — trimming would
    // silently shrink it to the content bbox (see AFE 50x40 → 87x61 regression
    // via the growth path, and the symmetric shrink path here).
    if fixed_board_size.is_none() {
        trim_board_outline(&mut board);
    }

    // 8. Normalize coordinates so all positions are positive
    normalize_board_coordinates(&mut board);

    Ok(board)
}

/// Build board from schematic up to (but NOT including) signal routing.
/// Returns board with layout, outline, and power routing done — ready for signal routing.
pub fn schematic_to_board_without_signal_route(
    schematic: &Schematic,
    directives: &crate::layout_directives::LayoutDirectives,
    layer_config: &crate::layer_config::BoardLayerConfig,
) -> Result<kicad_json5::ir::Board> {
    schematic_to_board_with_config_fixed_skip_route(schematic, directives, layer_config, None, true)
}

/// Route all nets on an existing board: signal routing + copper zones + thermal features.
/// Used by both gen-pcb and relayout/reroute.
pub fn route_board(
    board: &mut kicad_json5::ir::Board,
    directives: &crate::layout_directives::LayoutDirectives,
    layer_config: &crate::layer_config::BoardLayerConfig,
) {
    // P1: zones first — the router's zone-aware power filtering only excludes
    // rails that a zone actually covers; unzoned rails must route as signals.
    generate_copper_zones(board, layer_config);

    let routing_result =
        crate::router::auto_route_signal_nets_with_config(board, directives, layer_config);
    eprintln!(
        "[route] Signal routing: {}/{} nets routed ({} segments, {} vias)",
        routing_result.routed_nets,
        routing_result.total_nets,
        routing_result.total_segments,
        routing_result.total_vias
    );
    if !routing_result.failed_nets.is_empty() {
        eprintln!("[route] Unrouted nets: {:?}", routing_result.failed_nets);
    }

    let has_plane =
        layer_config.ground_zone_layer.is_some() || layer_config.power_zone_layer.is_some();
    if has_plane || !board.zones.is_empty() {
        generate_plane_thermal_vias(board, layer_config);
    }

    generate_thermal_features(board, directives);

    // Clip traces and vias to Edge.Cuts bounds (must run after all via/trace generation)
    clip_traces_to_outline(board);
}

/// Trim the Edge.Cuts board outline to tightly fit all actual board content
/// (footprints, segments, vias) after routing is complete.
fn trim_board_outline(board: &mut kicad_json5::ir::Board) {
    let mut min_x = f64::MAX;
    let mut min_y = f64::MAX;
    let mut max_x = f64::MIN;
    let mut max_y = f64::MIN;

    // Footprints: use pad positions (actual copper) not just body center
    for fp in &board.footprints {
        let (fx, fy, fr) = fp.position;
        let cos_r = fr.to_radians().cos();
        let sin_r = fr.to_radians().sin();
        for pad in &fp.pads {
            let (px, py, _) = pad.position;
            let wx = fx + px * cos_r - py * sin_r;
            let wy = fy + px * sin_r + py * cos_r;
            let (sw, sh) = pad.size;
            min_x = min_x.min(wx - sw / 2.0);
            min_y = min_y.min(wy - sh / 2.0);
            max_x = max_x.max(wx + sw / 2.0);
            max_y = max_y.max(wy + sh / 2.0);
        }
        // Also include courtyard from body size
        let (body_w, body_h) = crate::layout_engine::infer_body_size(&fp.lib_id, fp.pads.len());
        min_x = min_x.min(fx - body_w / 2.0);
        min_y = min_y.min(fy - body_h / 2.0);
        max_x = max_x.max(fx + body_w / 2.0);
        max_y = max_y.max(fy + body_h / 2.0);
    }

    // Segments
    for seg in &board.segments {
        min_x = min_x.min(seg.start.0).min(seg.end.0);
        min_y = min_y.min(seg.start.1).min(seg.end.1);
        max_x = max_x.max(seg.start.0).max(seg.end.0);
        max_y = max_y.max(seg.start.1).max(seg.end.1);
    }

    // Vias
    for via in &board.vias {
        let r = via.size / 2.0;
        min_x = min_x.min(via.at.0 - r);
        min_y = min_y.min(via.at.1 - r);
        max_x = max_x.max(via.at.0 + r);
        max_y = max_y.max(via.at.1 + r);
    }

    if min_x >= max_x || min_y >= max_y {
        return;
    }

    let margin = 1.5; // tighter margin than initial 3mm
    min_x -= margin;
    min_y -= margin;
    max_x += margin;
    max_y += margin;

    // Replace the old Edge.Cuts rect with the trimmed one
    use kicad_json5::ir::board::{BoardGraphic, BoardGraphicKind};
    board.graphics.retain(|g| g.layer != "Edge.Cuts");
    board.graphics.push(BoardGraphic {
        kind: BoardGraphicKind::Rect {
            start: (min_x, min_y),
            end: (max_x, max_y),
        },
        layer: "Edge.Cuts".into(),
        stroke_width: 0.15,
        fill: false,
    });

    // Update zones outlines to match new board bounds
    for zone in &mut board.zones {
        let zmin_x = zone.outline.iter().map(|p| p.0).fold(f64::MAX, f64::min);
        let zmin_y = zone.outline.iter().map(|p| p.1).fold(f64::MAX, f64::min);
        let _zmax_x = zone.outline.iter().map(|p| p.0).fold(f64::MIN, f64::max);
        let _zmax_y = zone.outline.iter().map(|p| p.1).fold(f64::MIN, f64::max);
        // If zone was board-sized, shrink it too
        if zmin_x <= min_x + 2.0 && zmin_y <= min_y + 2.0 {
            zone.outline = vec![
                (min_x + 0.5, min_y + 0.5),
                (max_x - 0.5, min_y + 0.5),
                (max_x - 0.5, max_y - 0.5),
                (min_x + 0.5, max_y - 0.5),
            ];
        }
    }

    // Update paper size
    let board_w = max_x - min_x;
    let board_h = max_y - min_y;
    if board_w > 0.0 && board_h > 0.0 {
        board.paper = format!("User {} {}", board_w + 20.0, board_h + 20.0);
    }

    eprintln!(
        "[trim] Board outline trimmed: ({:.2},{:.2})-({:.2},{:.2}) = {:.1}x{:.1}mm",
        min_x, min_y, max_x, max_y, board_w, board_h
    );
}

/// Remove trace segments and vias that are outside the Edge.Cuts board outline.
fn clip_traces_to_outline(board: &mut kicad_json5::ir::Board) {
    let outline = extract_board_outline(board);
    if outline.len() < 2 {
        return;
    }

    let xs: Vec<f64> = outline.iter().map(|p| p.0).collect();
    let ys: Vec<f64> = outline.iter().map(|p| p.1).collect();
    let (bx_min, bx_max) = (
        xs.iter().cloned().fold(f64::MAX, f64::min),
        xs.iter().cloned().fold(f64::MIN, f64::max),
    );
    let (by_min, by_max) = (
        ys.iter().cloned().fold(f64::MAX, f64::min),
        ys.iter().cloned().fold(f64::MIN, f64::max),
    );

    eprintln!(
        "[clip] Edge.Cuts bounds: ({:.2},{:.2})-({:.2},{:.2}), {} segments, {} vias before clip",
        bx_min,
        by_min,
        bx_max,
        by_max,
        board.segments.len(),
        board.vias.len()
    );

    let margin = 0.15; // trace width / 2
    let before = board.segments.len();
    board.segments.retain(|seg| {
        let (x1, y1) = seg.start;
        let (x2, y2) = seg.end;
        x1 >= bx_min - margin
            && x1 <= bx_max + margin
            && y1 >= by_min - margin
            && y1 <= by_max + margin
            && x2 >= bx_min - margin
            && x2 <= bx_max + margin
            && y2 >= by_min - margin
            && y2 <= by_max + margin
    });
    let removed_segs = before - board.segments.len();

    let via_margin = 0.5; // via radius + padding
    let before_vias = board.vias.len();
    board.vias.retain(|via| {
        let (vx, vy) = via.at;
        vx >= bx_min - via_margin
            && vx <= bx_max + via_margin
            && vy >= by_min - via_margin
            && vy <= by_max + via_margin
    });
    let removed_vias = before_vias - board.vias.len();

    eprintln!(
        "[clip] Removed {} segments, {} vias outside Edge.Cuts",
        removed_segs, removed_vias
    );
}

/// Shift all board coordinates so that the minimum x,y are at (margin, margin).
/// Ensures SVG rendering displays everything correctly.
fn normalize_board_coordinates(board: &mut kicad_json5::ir::Board) {
    use kicad_json5::ir::board::BoardGraphicKind;

    let margin = 5.0;
    let mut min_x = f64::MAX;
    let mut min_y = f64::MAX;

    // Collect min coordinates from all elements
    for fp in &board.footprints {
        min_x = min_x.min(fp.position.0);
        min_y = min_y.min(fp.position.1);
    }
    for seg in &board.segments {
        min_x = min_x.min(seg.start.0).min(seg.end.0);
        min_y = min_y.min(seg.start.1).min(seg.end.1);
    }
    for via in &board.vias {
        min_x = min_x.min(via.at.0);
        min_y = min_y.min(via.at.1);
    }
    for gr in &board.graphics {
        match &gr.kind {
            BoardGraphicKind::Rect { start, end } => {
                min_x = min_x.min(start.0).min(end.0);
                min_y = min_y.min(start.1).min(end.1);
            }
            BoardGraphicKind::Line { start, end } => {
                min_x = min_x.min(start.0).min(end.0);
                min_y = min_y.min(start.1).min(end.1);
            }
            BoardGraphicKind::Circle { center, .. } => {
                min_x = min_x.min(center.0);
                min_y = min_y.min(center.1);
            }
            BoardGraphicKind::Poly { points } => {
                for (px, py) in points {
                    min_x = min_x.min(*px);
                    min_y = min_y.min(*py);
                }
            }
            BoardGraphicKind::Arc { start, mid, end } => {
                min_x = min_x.min(start.0).min(mid.0).min(end.0);
                min_y = min_y.min(start.1).min(mid.1).min(end.1);
            }
            BoardGraphicKind::Text { position, .. } => {
                min_x = min_x.min(position.0);
                min_y = min_y.min(position.1);
            }
        }
    }
    for zone in &board.zones {
        for (px, py) in &zone.outline {
            min_x = min_x.min(*px);
            min_y = min_y.min(*py);
        }
    }

    // Only shift if coordinates go below margin
    let dx = if min_x < margin { margin - min_x } else { 0.0 };
    let dy = if min_y < margin { margin - min_y } else { 0.0 };
    if dx == 0.0 && dy == 0.0 {
        return;
    }

    eprintln!("[normalize] Shifting board by ({:.2}, {:.2})", dx, dy);

    for fp in &mut board.footprints {
        fp.position.0 += dx;
        fp.position.1 += dy;
    }
    for seg in &mut board.segments {
        seg.start.0 += dx;
        seg.start.1 += dy;
        seg.end.0 += dx;
        seg.end.1 += dy;
    }
    for via in &mut board.vias {
        via.at.0 += dx;
        via.at.1 += dy;
    }
    for gr in &mut board.graphics {
        match &mut gr.kind {
            BoardGraphicKind::Rect { start, end } => {
                start.0 += dx;
                start.1 += dy;
                end.0 += dx;
                end.1 += dy;
            }
            BoardGraphicKind::Line { start, end } => {
                start.0 += dx;
                start.1 += dy;
                end.0 += dx;
                end.1 += dy;
            }
            BoardGraphicKind::Circle { center, end } => {
                center.0 += dx;
                center.1 += dy;
                end.0 += dx;
                end.1 += dy;
            }
            BoardGraphicKind::Poly { points } => {
                for (px, py) in points.iter_mut() {
                    *px += dx;
                    *py += dy;
                }
            }
            BoardGraphicKind::Arc { start, mid, end } => {
                start.0 += dx;
                start.1 += dy;
                mid.0 += dx;
                mid.1 += dy;
                end.0 += dx;
                end.1 += dy;
            }
            BoardGraphicKind::Text { position, .. } => {
                position.0 += dx;
                position.1 += dy;
            }
        }
    }
    for zone in &mut board.zones {
        for (px, py) in &mut zone.outline {
            *px += dx;
            *py += dy;
        }
    }
}

/// Re-layout an existing PCB: run SA floorplanner, optionally re-route.
pub fn relayout_board(
    board: &mut kicad_json5::ir::Board,
    directives: &crate::layout_directives::LayoutDirectives,
    layer_config: &crate::layer_config::BoardLayerConfig,
    do_route: bool,
) {
    eprintln!(
        "[relayout] {} footprints, {} nets",
        board.footprints.len(),
        board.nets.len()
    );

    let positions = crate::layout_engine::auto_layout_board(board);
    eprintln!(
        "[relayout] SA floorplanner computed {} positions",
        positions.len()
    );

    // Apply new positions to footprints
    for fp in &mut board.footprints {
        if let Some((x, y, rot)) = positions.get(&fp.reference) {
            fp.position = (*x, *y, *rot);
        }
    }

    // Clear existing traces and zones (positions changed, old data is invalid)
    board.segments.clear();
    board.vias.clear();
    board.zones.clear();

    if do_route {
        auto_route_power_nets(board, directives);
        route_board(board, directives, layer_config);
    }

    // Normalize coordinates so all positions are positive
    normalize_board_coordinates(board);

    eprintln!("[relayout] Done");
}

/// Build board from schematic with optional routing skip.
/// When do_route is false, skips signal routing / zones / thermal features.
pub fn schematic_to_board_phases(
    schematic: &Schematic,
    directives: &crate::layout_directives::LayoutDirectives,
    layer_config: &crate::layer_config::BoardLayerConfig,
    do_route: bool,
) -> Result<kicad_json5::ir::Board> {
    schematic_to_board_phases_fixed(schematic, directives, layer_config, do_route, None)
}

pub fn schematic_to_board_phases_fixed(
    schematic: &Schematic,
    directives: &crate::layout_directives::LayoutDirectives,
    layer_config: &crate::layer_config::BoardLayerConfig,
    do_route: bool,
    fixed_board_size: Option<(f64, f64)>,
) -> Result<kicad_json5::ir::Board> {
    use kicad_json5::ir::board::*;

    let mut board = Board::new();
    board.generator = "kicad-designer".into();
    board.layers = layer_config.to_layer_defs();

    // Map nets
    let mut net_map: HashMap<u32, u32> = HashMap::new();
    for net in &schematic.nets {
        if net.name.is_empty() {
            net_map.insert(net.id, 0);
            continue;
        }
        let board_net_id = board.find_or_add_net(&net.name);
        net_map.insert(net.id, board_net_id);
    }

    // Run auto-layout
    let layout_positions = crate::layout_engine::auto_layout_refs_with_directives_fixed(
        schematic,
        directives,
        fixed_board_size,
    );

    // Map components → footprints
    for comp in &schematic.components {
        let footprint_id = comp
            .footprint
            .as_deref()
            .unwrap_or(match comp.lib_id.as_str() {
                "Device:R" => "Resistor_SMD:R_0805_2012Metric",
                "Device:C" => "Capacitor_SMD:C_0805_2012Metric",
                "Device:L" => "Inductor_SMD:L_0805_2012Metric",
                "Device:D" | "Device:LED" => "Diode_SMD:D_0805_2012Metric",
                _ => "custom:Default",
            });

        let (px, py, pr) = layout_positions
            .get(&comp.reference)
            .copied()
            .unwrap_or(comp.position);

        let mut fp = Footprint::new(footprint_id, &comp.reference, &comp.value);
        fp.position = (px, py, pr);
        fp.layer = "F.Cu".into();

        // Full footprint synthesis (same as with_config path): body size,
        // pin offsets, pad params — the naive (0,0) 1x1 pads here broke
        // FreeRouting DSN export (all pads stacked at origin).
        let pin_count = comp.pins.len();
        let (body_w, body_h) = crate::layout_engine::infer_body_size(footprint_id, pin_count);
        let _ = (body_w, body_h);
        let pin_offsets =
            crate::layout_engine::build_pin_offsets(footprint_id, &comp.pins, body_w, body_h);
        let pad_params = crate::layout_engine::infer_pad_params(footprint_id, pin_count);
        let offset_map: HashMap<&str, (f64, f64)> = pin_offsets
            .iter()
            .map(|p| (p.name.as_str(), (p.dx, p.dy)))
            .collect();
        let quad_fine = (footprint_id.to_uppercase().contains("QFN")
            || footprint_id.to_uppercase().contains("QFP"))
            && crate::layout_engine::parse_named_pitch(&footprint_id.to_uppercase())
                .map(|p| p <= 0.5)
                .unwrap_or(false);

        for pin in &comp.pins {
            let board_net = if pin.nc {
                None
            } else {
                pin.net_name
                    .as_ref()
                    .and_then(|nn| board.nets.iter().find(|n| n.name == *nn).map(|n| n.id))
            };
            let (dx, dy) = offset_map
                .get(pin.number.as_str())
                .copied()
                .unwrap_or((0.0, 0.0));
            let mut pad_size = pad_params.size;
            if quad_fine {
                pad_size = if dx.abs() >= dy.abs() {
                    (0.875, 0.2)
                } else {
                    (0.2, 0.875)
                };
            }
            // NC pads: no-net copper bridging neighbours reads as a short in
            // DRC (conductive chain through unconnected copper) — slim it.
            if pin.nc {
                pad_size = (pad_size.0 * 0.6, pad_size.1 * 0.6);
            }
            fp.pads.push(Pad {
                number: pin.number.clone(),
                pad_type: pad_params.pad_type,
                shape: pad_params.shape,
                position: (dx, dy, 0.0),
                size: pad_size,
                layers: pad_params.layers.clone(),
                drill: pad_params.drill.clone(),
                net: board_net,
                net_name: None,
                pin_function: None,
                pin_type: None,
                roundrect_rratio: pad_params.roundrect_rratio,
                solder_mask_margin: None,
                thermal_bridge_width: None,
                thermal_bridge_angle: None,
                thermal_gap: None,
                clearance: None,
                zone_connect: None,
                remove_unused_layers: None,
                options: None,
                primitives: Vec::new(),
            });
        }
        bake_footprint_rotation(&mut fp);
        board.footprints.push(fp);
    }

    // Board outline
    let (outline_min, outline_max) = if let Some((fw, fh)) = fixed_board_size {
        // Fixed-size mode: user owns the floorplan, outline is exact.
        ((0.0, 0.0), (fw, fh))
    } else {
        let (mut min_x, mut min_y) = (f64::MAX, f64::MAX);
        let (mut max_x, mut max_y) = (f64::MIN, f64::MIN);
        for fp in &board.footprints {
            let (x, y, _) = fp.position;
            min_x = min_x.min(x - 8.0);
            min_y = min_y.min(y - 8.0);
            max_x = max_x.max(x + 8.0);
            max_y = max_y.max(y + 8.0);
        }
        ((min_x, min_y), (max_x, max_y))
    };
    if outline_max.0 > outline_min.0 && outline_max.1 > outline_min.1 {
        board.graphics.push(BoardGraphic {
            kind: kicad_json5::ir::board::BoardGraphicKind::Rect {
                start: outline_min,
                end: outline_max,
            },
            stroke_width: 0.1,
            layer: "Edge.Cuts".into(),
            fill: false,
        });
    }

    if do_route {
        auto_route_power_nets(&mut board, directives);
        route_board(&mut board, directives, layer_config);
    }

    Ok(board)
}

/// Generate a .kicad_pcb string from a Board IR.
pub fn generate_kicad_pcb(board: &kicad_json5::ir::Board) -> Result<String> {
    use kicad_json5::codegen::{BoardSexprConfig, BoardSexprGenerator};
    // 产线权威链是华秋 fork: net 内联方言, 显式指定(库默认已翻转为 Official)
    let hq = BoardSexprConfig {
        dialect: kicad_json5::dialect::VendorDialect::Huaqiu,
        ..Default::default()
    };
    let mut gen = BoardSexprGenerator::with_config(hq);
    gen.generate(board)
        .map_err(|e| anyhow::anyhow!("PCB generation failed: {}", e))
}

/// Export Gerber files from a .kicad_pcb using kicad-cli.
/// Returns a list of generated file paths.
pub fn export_gerber(
    kicad_cli_path: &str,
    pcb_path: &str,
    output_dir: &str,
) -> Result<Vec<String>> {
    let out = output_dir.trim_end_matches('/');
    let output = std::process::Command::new(kicad_cli_path)
        .args([
            "pcb",
            "export",
            "gerbers",
            pcb_path,
            "-o",
            &format!("{out}/"),
        ])
        .output()
        .with_context(|| format!("Failed to execute '{}' for gerber export", kicad_cli_path))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("kicad-cli gerber export failed: {}", stderr);
    }

    // List generated files
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(out) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                let lower = name.to_lowercase();
                if lower.ends_with(".gbr")
                    || lower.ends_with(".gbo")
                    || lower.ends_with(".gbs")
                    || lower.ends_with(".gbl")
                    || lower.ends_with(".gtl")
                    || lower.ends_with(".gts")
                    || lower.ends_with(".gto")
                    || lower.ends_with(".gtp")
                    || lower.ends_with(".drl")
                    || lower.ends_with(".gko")
                {
                    files.push(format!("{}/{}", out, name));
                }
            }
        }
    }
    files.sort();
    Ok(files)
}

/// Export drill files from a .kicad_pcb using kicad-cli.
pub fn export_drill(
    kicad_cli_path: &str,
    pcb_path: &str,
    output_dir: &str,
) -> Result<(String, String)> {
    // kicad-cli 的 drill 导出：-o 带尾斜杠才是目录，否则按输出文件名前缀处理
    let out = output_dir.trim_end_matches('/');
    let output = std::process::Command::new(kicad_cli_path)
        .args(["pcb", "export", "drill", pcb_path, "-o", &format!("{out}/")])
        .output()
        .with_context(|| format!("Failed to execute '{}' for drill export", kicad_cli_path))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("kicad-cli drill export failed: {}", stderr);
    }

    let pth = format!(
        "{}/{}.drl",
        out,
        std::path::Path::new(pcb_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("board")
    );
    let npth = format!(
        "{}/{}-NPTH.drl",
        out,
        std::path::Path::new(pcb_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("board")
    );
    Ok((pth, npth))
}
