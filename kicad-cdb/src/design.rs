use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};

use kicad_json5::codegen::{KicadVersion, SexprConfig, SexprGenerator};
use kicad_json5::ir::{PinInstance, RenderHint, Schematic, Symbol, SymbolInstance, Net};

use crate::topology::{load_builtin_template, ComponentSlot, TopologyTemplate};
use crate::ic_template::{self, IcCoreTemplate, Peripheral};
use crate::composition::{Composition, ModuleInstance};
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
        let net_id = *net_id_map.get(&conn.net).context(format!("Net '{}' not found", conn.net))?;
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
        vin, vout, iout
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
    for (key, _) in pin_net_map {
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
    db: &ComponentDb,
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
        let net_name = net_map.get(port_name)
            .cloned()
            .unwrap_or_else(|| port_name.clone());
        net_id_map.insert(port_name.clone(), next_id);
        let render = match port_def.port_type.as_str() {
            "power" => RenderHint::Power,
            _ => RenderHint::Wire,
        };
        nets.push(Net { id: next_id, name: net_name, net_type: None, render });
        next_id += 1;
    }

    // Build IC component
    let mut components = Vec::new();
    let mut ref_counter: HashMap<String, usize> = HashMap::new();
    let mut used_lib_ids: HashSet<String> = HashSet::new();

    // IC position
    let ic_pos = template.layout.get("ic").map(|p| (p.x, p.y)).unwrap_or((50.8, 50.8));
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
    ic_comp.footprint = if template.ic.footprint.is_empty() { None } else { Some(template.ic.footprint.clone()) };
    ic_comp.pins = ic_pins;
    used_lib_ids.insert(ic_lib_id.clone());
    components.push(ic_comp);

    // Build peripheral components
    for periph in &template.peripherals {
        let periph_comp = build_peripheral_component(
            periph, &template, &resolved, &mut net_id_map, &mut next_id,
            &mut nets, &mut ref_counter,
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
    schematic.metadata.title_block.title = Some(format!(
        "{} circuit ({})", template.name, template.ic.mpn
    ));
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
    let prefix = if lib_id.starts_with("Device:R") { "R" }
        else if lib_id.starts_with("Device:C") { "C" }
        else if lib_id.starts_with("Device:L") { "L" }
        else if lib_id.starts_with("Device:D") { "D" }
        else if lib_id.starts_with("Device:Q") || lib_id.starts_with("AO") { "Q" }
        else { "U" };
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
    let lib_id = if periph.lib.is_empty() { "custom:IC".to_string() } else { periph.lib.clone() };
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
fn ensure_net(target: &str, net_id_map: &mut HashMap<String, u32>, next_id: &mut u32, nets: &mut Vec<Net>) -> u32 {
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
    nets.push(Net { id, name: target.to_string(), net_type: None, render });
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
pub fn generate_composed_schematic(
    db: &ComponentDb,
    composition: &Composition,
) -> Result<String> {
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
        all_nets.push(Net { id: next_net_id, name: gnet.name.clone(), net_type: None, render });
        net_name_to_id.insert(gnet.name.clone(), next_net_id);
        next_net_id += 1;
    }

    // Process each module instance
    for module in &composition.modules {
        let y_off = module.y_offset.unwrap_or(y_cursor);

        match module.template_type.as_str() {
            "ic-core" => {
                compose_ic_module(
                    module, y_off, &mut net_name_to_id, &mut next_net_id,
                    &mut all_nets, &mut all_components, &mut all_lib_ids,
                    &mut ref_counter,
                )?;
            }
            _ => {
                anyhow::bail!("Unsupported template_type '{}' in module '{}'", module.template_type, module.id);
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
        let actual_name = module.nets.get(port_name)
            .cloned()
            .unwrap_or_else(|| port_name.clone());
        let net_id = ensure_global_net(&actual_name, net_name_to_id, next_net_id, all_nets, &port_def.port_type);
        local_net_map.insert(port_name.clone(), net_id);
    }

    // IC component
    let ic_ref = alloc_ref(&template.ic.mpn, ref_counter);
    let ic_pos = template.layout.get("ic").map(|p| (p.x, p.y + y_offset)).unwrap_or((50.8, 50.8 + y_offset));

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
    ic_comp.footprint = if template.ic.footprint.is_empty() { None } else { Some(template.ic.footprint.clone()) };
    ic_comp.pins = ic_pins;
    all_lib_ids.insert(ic_lib_id);
    all_components.push(ic_comp);

    // Peripherals
    for periph in &template.peripherals {
        let lib_id = if periph.lib.is_empty() { "custom:IC".to_string() } else { periph.lib.clone() };
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
        let (x, y) = pos.map(|p| (p.x, p.y + y_offset)).unwrap_or((76.2, 50.8 + y_offset));

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

fn ensure_global_net(name: &str, map: &mut HashMap<String, u32>, next_id: &mut u32, nets: &mut Vec<Net>, port_type: &str) -> u32 {
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
    nets.push(Net { id, name: name.to_string(), net_type: None, render });
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
        number: "1".to_string(), name: "1".to_string(), pin_type: "passive".to_string(),
    });
    sym.pins.push(kicad_json5::ir::Pin {
        number: "2".to_string(), name: "2".to_string(), pin_type: "passive".to_string(),
    });
    sym
}
