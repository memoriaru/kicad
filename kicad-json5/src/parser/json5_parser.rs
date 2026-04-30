//! JSON5 to IR parser
//!
//! Parses JSON5 text (as produced by `Json5Generator`) back into the
//! intermediate representation (`Schematic`), enabling round-trip conversion.

use std::collections::HashMap;

use serde_json::Value;

use crate::error::{Error, Result};
use crate::ir::{
    Junction, Label, Mirror, Net, Paper, Pin, PinInstance, PinType, Property, RenderHint,
    Schematic, Sheet, SheetPin, SheetProperty, Stroke, StrokeType, Symbol, SymbolInstance,
    TextEffects, TitleBlock, Wire,
};

/// Parse a JSON5 string into a Schematic IR.
pub fn parse_json5(source: &str) -> Result<Schematic> {
    let value: Value = serde_json5::from_str(source)
        .map_err(|e| Error::Json5Parse(format!("{}", e)))?;
    value_to_schematic(&value)
}

fn err(msg: impl Into<String>) -> Error {
    Error::Json5Parse(msg.into())
}

// ── Top-level ──────────────────────────────────────────────────────

fn value_to_schematic(value: &Value) -> Result<Schematic> {
    let obj = value
        .as_object()
        .ok_or_else(|| err("JSON5 root must be an object"))?;

    let mut schematic = Schematic::new();

    // Version & generator (top level)
    schematic.metadata.version = get_str(obj, "version").unwrap_or_default();
    schematic.metadata.generator = get_str(obj, "generator").unwrap_or_else(|| "kicad-json5".into());
    schematic.metadata.generator_version = get_str(obj, "generator_version");

    // Metadata block
    if let Some(meta) = obj.get("metadata").and_then(|v| v.as_object()) {
        schematic.metadata.uuid = get_str(meta, "uuid").unwrap_or_default();
        schematic.metadata.title_block = TitleBlock {
            title: get_str(meta, "title"),
            date: get_str(meta, "date"),
            rev: get_str(meta, "rev"),
            company: get_str(meta, "company"),
            comments: meta
                .get("comments")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default(),
        };
        schematic.metadata.paper = Paper {
            size: get_str(meta, "paper").unwrap_or_else(|| "A4".into()),
            width: None,
            height: None,
            portrait: false,
        };
    }

    // Build net map first (needed for pin net_name resolution)
    let mut net_map: HashMap<u32, String> = HashMap::new();
    if let Some(nets) = obj.get("nets").and_then(|v| v.as_array()) {
        for nv in nets {
            if let Some(net) = value_to_net(nv) {
                if let Some(existing) = net_map.get(&net.id) {
                    eprintln!(
                        "ERROR: duplicate net id {} — \"{}\" conflicts with \"{}\"",
                        net.id, net.name, existing
                    );
                    std::process::exit(1);
                }
                net_map.insert(net.id, net.name.clone());
                schematic.nets.push(net);
            }
        }
    }

    // lib_symbols
    if let Some(syms) = obj.get("lib_symbols").and_then(|v| v.as_array()) {
        for sv in syms {
            schematic.lib_symbols.push(value_to_symbol(sv));
        }
    }

    // components
    if let Some(comps) = obj.get("components").and_then(|v| v.as_array()) {
        for cv in comps {
            schematic
                .components
                .push(value_to_component(cv, &net_map));
        }
    }

    // wires
    if let Some(wires) = obj.get("wires").and_then(|v| v.as_array()) {
        for wv in wires {
            schematic.wires.push(value_to_wire(wv));
        }
    }

    // labels
    if let Some(labels) = obj.get("labels").and_then(|v| v.as_array()) {
        for lv in labels {
            schematic.labels.push(value_to_label(lv));
        }
    }

    // junctions
    if let Some(juncs) = obj.get("junctions").and_then(|v| v.as_array()) {
        for jv in juncs {
            schematic.junctions.push(value_to_junction(jv));
        }
    }

    // no_connects (may not exist in older JSON5 output)
    if let Some(ncs) = obj.get("no_connects").and_then(|v| v.as_array()) {
        for nv in ncs {
            schematic.no_connects.push(value_to_no_connect(nv));
        }
    }

    // buses
    if let Some(buses) = obj.get("buses").and_then(|v| v.as_array()) {
        for bv in buses {
            schematic.buses.push(value_to_bus(bv));
        }
    }

    // bus_entries
    if let Some(entries) = obj.get("bus_entries").and_then(|v| v.as_array()) {
        for ev in entries {
            schematic.bus_entries.push(value_to_bus_entry(ev));
        }
    }

    // sheets (hierarchical)
    if let Some(sheets) = obj.get("sheets").and_then(|v| v.as_array()) {
        for sv in sheets {
            schematic.sheets.push(value_to_sheet(sv));
        }
    }

    // Backfill lib_symbol pins from component instances.
    // JSON5 lib_symbols often lack pin definitions, but component instances have them.
    // For IC-type symbols, we need pin info to generate correct lib_symbol graphics and
    // compute pin world positions for auto-labels.
    {
        // Collect max pin count per lib_id from components
        let mut max_pins_by_lib: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for comp in &schematic.components {
            let count = comp.pins.len();
            let entry = max_pins_by_lib.entry(comp.lib_id.clone()).or_insert(0);
            *entry = (*entry).max(count);
        }

        for symbol in &mut schematic.lib_symbols {
            if symbol.pins.is_empty() {
                if let Some(&max_pins) = max_pins_by_lib.get(&symbol.lib_id) {
                    // Only backfill if components have more than 2 pins (IC-type).
                    // 2-pin symbols (R, C, etc.) are handled by template detection.
                    if max_pins > 2 {
                        // Find a component with the most pins to get pin names/types
                        let best_comp = schematic.components.iter()
                            .filter(|c| c.lib_id == symbol.lib_id)
                            .max_by_key(|c| c.pins.len());

                        if let Some(comp) = best_comp {
                            for cp in &comp.pins {
                                symbol.pins.push(crate::ir::Pin {
                                    number: cp.number.clone(),
                                    name: cp.name.clone(),
                                    pin_type: cp.pin_type.clone(),
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(schematic)
}

// ── IR type converters ─────────────────────────────────────────────

fn value_to_symbol(value: &Value) -> Symbol {
    let obj = value.as_object();
    let lib_id = obj
        .and_then(|o| o.get("lib_id"))
        .or_else(|| obj.and_then(|o| o.get("id")))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let mut symbol = Symbol::new(&lib_id);

    if let Some(o) = obj {
        symbol.reference = get_str(o, "reference").unwrap_or_default();
        symbol.pin_numbers_hidden = o
            .get("pin_numbers")
            .and_then(|v| v.as_str())
            .map(|s| s == "hide")
            .unwrap_or(false);
        symbol.pin_names_hidden = o
            .get("pin_names")
            .and_then(|v| v.as_str())
            .map(|s| s == "hide")
            .unwrap_or(false);
        symbol.is_power = o.get("power").and_then(|v| v.as_bool()).unwrap_or(false);
        symbol.in_bom = o.get("in_bom").and_then(|v| v.as_bool()).unwrap_or(true);
        symbol.on_board = o.get("on_board").and_then(|v| v.as_bool()).unwrap_or(true);
        symbol.exclude_from_sim = o.get("exclude_from_sim").and_then(|v| v.as_bool()).unwrap_or(false);
        symbol.in_pos_files = o.get("in_pos_files").and_then(|v| v.as_bool()).unwrap_or(true);
        symbol.duplicate_pin_numbers_are_jumpers = o.get("duplicate_pin_numbers_are_jumpers").and_then(|v| v.as_bool()).unwrap_or(false);

        // properties
        if let Some(props) = o.get("properties").and_then(|v| v.as_object()) {
            for (key, val) in props {
                if let Some(v) = val.as_str() {
                    use crate::ir::Property;
                    symbol.properties.push(Property::new(key, v));
                }
            }
        }

        // pins
        if let Some(pins) = o.get("pins").and_then(|v| v.as_array()) {
            for pv in pins {
                symbol.pins.push(value_to_pin(pv));
            }
        }
    }

    symbol
}

fn value_to_pin(value: &Value) -> Pin {
    let o = value.as_object();
    Pin {
        number: o.and_then(|o| get_str(o, "number")).unwrap_or_default(),
        name: o.and_then(|o| get_str(o, "name")).unwrap_or_default(),
        pin_type: o
            .and_then(|o| get_str(o, "type"))
            .unwrap_or_else(|| "passive".into()),
    }
}

fn value_to_net(value: &Value) -> Option<Net> {
    let o = value.as_object()?;
    let id = o.get("id").and_then(|v| v.as_u64())? as u32;
    let name = get_str(o, "name")?;
    let net_type = get_str(o, "type");
    let render = get_str(o, "render")
        .as_deref()
        .and_then(RenderHint::from_str)
        .unwrap_or_default();
    Some(Net {
        id,
        name,
        net_type,
        render,
    })
}

fn value_to_component(value: &Value, net_map: &HashMap<u32, String>) -> SymbolInstance {
    let o = value.as_object();
    let lib_id = o
        .and_then(|o| get_str(o, "lib_id"))
        .unwrap_or_default();
    let reference = o
        .and_then(|o| get_str(o, "ref"))
        .unwrap_or_default();

    let mut comp = SymbolInstance::new(&lib_id, &reference);

    if let Some(o) = o {
        comp.value = get_str(o, "value").unwrap_or_default();
        comp.footprint = get_str(o, "footprint");

        // position: {x, y, rotation}
        if let Some(pos) = o.get("position").and_then(|v| v.as_object()) {
            comp.position = (
                pos.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0),
                pos.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0),
                pos.get("rotation").and_then(|v| v.as_f64()).unwrap_or(0.0),
            );
        }

        // mirror
        if let Some(m) = get_str(o, "mirror") {
            comp.mirror = match m.as_str() {
                "x" => Mirror::X,
                "y" => Mirror::Y,
                _ => Mirror::None,
            };
        }

        comp.uuid = get_str(o, "uuid");
        comp.unit = o
            .get("unit")
            .and_then(|v| v.as_u64())
            .unwrap_or(1) as u32;

        // KiCad 8+ flags
        comp.exclude_from_sim = o.get("exclude_from_sim").and_then(|v| v.as_bool()).unwrap_or(false);
        comp.in_bom = o.get("in_bom").and_then(|v| v.as_bool()).unwrap_or(true);
        comp.on_board = o.get("on_board").and_then(|v| v.as_bool()).unwrap_or(true);
        comp.dnp = o.get("dnp").and_then(|v| v.as_bool()).unwrap_or(false);

        // instances
        if let Some(insts) = o.get("instances").and_then(|v| v.as_array()) {
            use crate::ir::{InstancePath, InstanceProject};
            for inst_val in insts {
                if let Some(proj_obj) = inst_val.as_object() {
                    let proj_name = get_str(proj_obj, "project").unwrap_or_default();
                    let mut project = InstanceProject {
                        name: proj_name,
                        paths: Vec::new(),
                    };
                    if let Some(paths) = proj_obj.get("paths").and_then(|v| v.as_array()) {
                        for path_val in paths {
                            if let Some(path_obj) = path_val.as_object() {
                                project.paths.push(InstancePath {
                                    path: get_str(path_obj, "path").unwrap_or_default(),
                                    reference: get_str(path_obj, "reference").unwrap_or_default(),
                                    unit: path_obj.get("unit").and_then(|v| v.as_u64()).unwrap_or(1) as u32,
                                });
                            }
                        }
                    }
                    comp.instances.projects.push(project);
                }
            }
        }

        // properties (simple map)
        if let Some(props) = o.get("properties").and_then(|v| v.as_object()) {
            for (k, v) in props {
                if let Some(val) = v.as_str() {
                    comp.properties.insert(k.clone(), val.to_string());

                    // Populate properties_ext
                    let prop = Property::new(k, val);
                    // Also set well-known fields on component
                    match k.as_str() {
                        "Reference" => comp.reference = val.to_string(),
                        "Value" => comp.value = val.to_string(),
                        "Footprint" => comp.footprint = Some(val.to_string()),
                        _ => {}
                    }
                    comp.properties_ext.push(prop);
                }
            }
        }

        // pins: object keyed by pin number
        if let Some(pins) = o.get("pins").and_then(|v| v.as_object()) {
            for (num, pv) in pins {
                let po = pv.as_object();
                let net_id = po
                    .and_then(|o| o.get("net"))
                    .and_then(|v| v.as_u64())
                    .map(|n| n as u32);
                let net_name = net_id
                    .as_ref()
                    .and_then(|id| net_map.get(id))
                    .cloned()
                    .or_else(|| {
                        po.and_then(|o| get_str(o, "net_name"))
                    });

                comp.pins.push(PinInstance {
                    number: num.clone(),
                    name: po.and_then(|o| get_str(o, "name")).unwrap_or_default(),
                    pin_type: po
                        .and_then(|o| get_str(o, "type"))
                        .unwrap_or_else(|| "passive".into()),
                    net_id,
                    net_name,
                    nc: po.and_then(|o| o.get("nc")).and_then(|v| v.as_bool()).unwrap_or(false),
                });
            }
        }
    }

    comp
}

fn value_to_wire(value: &Value) -> Wire {
    let o = value.as_object();
    let start = o
        .and_then(|o| o.get("start"))
        .and_then(|v| v.as_array())
        .and_then(|arr| array_to_point(arr))
        .unwrap_or((0.0, 0.0));
    let end = o
        .and_then(|o| o.get("end"))
        .and_then(|v| v.as_array())
        .and_then(|arr| array_to_point(arr))
        .unwrap_or((0.0, 0.0));

    let mut wire = Wire::new(start, end);

    if let Some(o) = o {
        wire.net_id = o.get("net").and_then(|v| v.as_u64()).map(|n| n as u32);
        if let Some(sv) = o.get("stroke").and_then(|v| v.as_object()) {
            wire.stroke = value_to_stroke(sv);
        }
    }

    wire
}

fn value_to_label(value: &Value) -> Label {
    let o = value.as_object();
    let text = o
        .and_then(|o| get_str(o, "text"))
        .unwrap_or_default();
    let label_type = o
        .and_then(|o| get_str(o, "type"))
        .unwrap_or_else(|| "label".into());

    let mut label = Label::new(text, label_type);

    if let Some(o) = o {
        if let Some(pos) = o.get("position").and_then(|v| v.as_object()) {
            label.position = (
                pos.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0),
                pos.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0),
                pos.get("rotation").and_then(|v| v.as_f64()).unwrap_or(0.0),
            );
        }
        label.shape = get_str(o, "shape").unwrap_or_else(|| "passive".into());
    }

    label
}

fn value_to_junction(value: &Value) -> Junction {
    let o = value.as_object();
    Junction {
        position: o
            .and_then(|o| o.get("position"))
            .and_then(|v| v.as_array())
            .and_then(|arr| array_to_point(arr))
            .unwrap_or((0.0, 0.0)),
        diameter: o
            .and_then(|o| o.get("diameter"))
            .and_then(|v| v.as_f64())
            .unwrap_or(1.27),
    }
}

fn value_to_no_connect(value: &Value) -> crate::ir::NoConnect {
    let o = value.as_object();
    let pos = o
        .and_then(|o| o.get("position"))
        .and_then(|v| v.as_array())
        .and_then(|arr| array_to_point(arr))
        .unwrap_or((0.0, 0.0));
    let mut nc = crate::ir::NoConnect::new(pos);
    nc.uuid = o.and_then(|o| get_str(o, "uuid"));
    nc
}

fn value_to_bus(value: &Value) -> crate::ir::Bus {
    let mut bus = crate::ir::Bus::new();
    let o = value.as_object();
    if let Some(o) = o {
        if let Some(pts) = o.get("points").and_then(|v| v.as_array()) {
            for pt in pts {
                if let Some(arr) = pt.as_array() {
                    if let Some(p) = array_to_point(arr) {
                        bus.points.push(p);
                    }
                }
            }
        }
        if let Some(sv) = o.get("stroke").and_then(|v| v.as_object()) {
            bus.stroke = value_to_stroke(sv);
        }
    }
    bus
}

fn value_to_bus_entry(value: &Value) -> crate::ir::BusEntry {
    let o = value.as_object();
    let pos = o
        .as_ref()
        .and_then(|o| o.get("position"))
        .and_then(|v| v.as_array())
        .and_then(|arr| array_to_point(arr))
        .unwrap_or((0.0, 0.0));

    let size = o
        .as_ref()
        .and_then(|o| o.get("size"))
        .and_then(|v| v.as_array())
        .and_then(|arr| array_to_point(arr))
        .unwrap_or((2.54, 2.54));

    let mut entry = crate::ir::BusEntry::new(pos, size);
    if let Some(o) = o {
        if let Some(sv) = o.get("stroke").and_then(|v| v.as_object()) {
            entry.stroke = value_to_stroke(sv);
        }
    }
    entry
}

fn value_to_sheet(value: &Value) -> Sheet {
    let o = value.as_object();
    let mut sheet = Sheet {
        position: (0.0, 0.0),
        size: (50.0, 30.0),
        stroke: Stroke::default(),
        fill: crate::ir::Fill::none(),
        sheet_name: SheetProperty::default(),
        sheet_file: SheetProperty::default(),
        pins: Vec::new(),
    };

    if let Some(o) = o {
        // position
        if let Some(pos) = o.get("position").and_then(|v| v.as_array()) {
            if pos.len() >= 2 {
                sheet.position = (
                    pos[0].as_f64().unwrap_or(0.0),
                    pos[1].as_f64().unwrap_or(0.0),
                );
            }
        }
        // size
        if let Some(sz) = o.get("size").and_then(|v| v.as_array()) {
            if sz.len() >= 2 {
                sheet.size = (
                    sz[0].as_f64().unwrap_or(50.0),
                    sz[1].as_f64().unwrap_or(30.0),
                );
            }
        }
        // sheet_name
        if let Some(name) = get_str(o, "sheet_name") {
            sheet.sheet_name.value = name;
        }
        // sheet_file
        if let Some(file) = get_str(o, "sheet_file") {
            sheet.sheet_file.value = file;
        }
        // pins
        if let Some(pins) = o.get("pins").and_then(|v| v.as_array()) {
            for pv in pins {
                sheet.pins.push(value_to_sheet_pin(pv));
            }
        }
    }

    sheet
}

fn value_to_sheet_pin(value: &Value) -> SheetPin {
    let o = value.as_object();
    let pin_type = o
        .and_then(|o| get_str(o, "type"))
        .map(|s| PinType::from_str(&s))
        .unwrap_or(PinType::Passive);

    let mut pin = SheetPin {
        name: o.and_then(|o| get_str(o, "name")).unwrap_or_default(),
        pin_type,
        position: (0.0, 0.0, 0.0),
        effects: TextEffects::default(),
    };

    // position override if provided
    if let Some(o) = value.as_object() {
        if let Some(pos) = o.get("position").and_then(|v| v.as_object()) {
            pin.position = (
                pos.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0),
                pos.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0),
                pos.get("rotation").and_then(|v| v.as_f64()).unwrap_or(0.0),
            );
        } else if let Some(pos) = o.get("position").and_then(|v| v.as_array()) {
            if pos.len() >= 2 {
                pin.position = (
                    pos[0].as_f64().unwrap_or(0.0),
                    pos[1].as_f64().unwrap_or(0.0),
                    pos.get(2).and_then(|v| v.as_f64()).unwrap_or(0.0),
                );
            }
        }
        // side: auto-compute position from side if no explicit position
        if let Some(side) = get_str(o, "side") {
            // Only override if position is still default (0,0,0)
            if pin.position == (0.0, 0.0, 0.0) {
                pin.position = match side.as_str() {
                    "left" => (0.0, 0.0, 0.0),
                    "right" => (0.0, 0.0, 180.0),
                    "top" => (0.0, 0.0, 270.0),
                    "bottom" => (0.0, 0.0, 90.0),
                    _ => (0.0, 0.0, 0.0),
                };
            }
        }
    }

    pin
}

// ── Helpers ────────────────────────────────────────────────────────

fn value_to_stroke(o: &serde_json::Map<String, Value>) -> Stroke {
    Stroke {
        width: o.get("width").and_then(|v| v.as_f64()).unwrap_or(0.0),
        stroke_type: o
            .get("type")
            .and_then(|v| v.as_str())
            .map(|s| StrokeType::from_str(s))
            .unwrap_or(StrokeType::Default),
    }
}

fn array_to_point(arr: &[Value]) -> Option<(f64, f64)> {
    if arr.len() >= 2 {
        let x = arr[0].as_f64()?;
        let y = arr[1].as_f64()?;
        Some((x, y))
    } else {
        None
    }
}

fn get_str(obj: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
    obj.get(key).and_then(|v| v.as_str()).map(|s| s.to_string())
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_empty_schematic() {
        let json5 = "{ version: \"20231120\", generator: \"test\" }";
        let sch = parse_json5(json5).unwrap();
        assert_eq!(sch.metadata.version, "20231120");
        assert_eq!(sch.metadata.generator, "test");
        assert!(sch.components.is_empty());
        assert!(sch.nets.is_empty());
    }

    #[test]
    fn test_parse_metadata() {
        let json5 = r#"{
            version: "20231120",
            generator: "eeschema",
            metadata: {
                uuid: "test-uuid",
                title: "Test Schematic",
                date: "2024-01-01",
                rev: "1.0",
                company: "Test Co",
                paper: "A3",
                comments: ["Hello", "World"]
            }
        }"#;
        let sch = parse_json5(json5).unwrap();
        assert_eq!(sch.metadata.uuid, "test-uuid");
        assert_eq!(sch.metadata.title_block.title, Some("Test Schematic".into()));
        assert_eq!(sch.metadata.paper.size, "A3");
        assert_eq!(sch.metadata.title_block.comments, vec!["Hello", "World"]);
    }

    #[test]
    fn test_parse_nets() {
        let json5 = r#"{
            nets: [
                { id: 0, name: "GND", type: "power" },
                { id: 1, name: "VCC" }
            ]
        }"#;
        let sch = parse_json5(json5).unwrap();
        assert_eq!(sch.nets.len(), 2);
        assert_eq!(sch.nets[0].name, "GND");
        assert_eq!(sch.nets[0].net_type, Some("power".into()));
        assert_eq!(sch.nets[1].net_type, None);
    }

    #[test]
    fn test_parse_component() {
        let json5 = r#"{
            nets: [{ id: 1, name: "VCC" }],
            components: [{
                ref: "R1",
                lib_id: "Device:R",
                value: "10k",
                footprint: "R_0805",
                position: { x: 10.5, y: 20.0, rotation: 90.0 },
                mirror: "x",
                uuid: "comp-uuid-1",
                properties: { Reference: "R1", Value: "10k" },
                pins: {
                    "1": { name: "", net: 1, net_name: "VCC" },
                    "2": { name: "" }
                }
            }]
        }"#;
        let sch = parse_json5(json5).unwrap();
        assert_eq!(sch.components.len(), 1);
        let c = &sch.components[0];
        assert_eq!(c.reference, "R1");
        assert_eq!(c.lib_id, "Device:R");
        assert_eq!(c.value, "10k");
        assert_eq!(c.footprint, Some("R_0805".into()));
        assert_eq!(c.position, (10.5, 20.0, 90.0));
        assert_eq!(c.mirror, Mirror::X);
        assert_eq!(c.uuid, Some("comp-uuid-1".into()));
        assert_eq!(c.pins.len(), 2);
    }

    #[test]
    fn test_parse_wires() {
        let json5 = r#"{
            wires: [{
                start: [10.0, 20.0],
                end: [30.0, 40.0],
                net: 1,
                stroke: { width: 0.5, type: "dash" }
            }]
        }"#;
        let sch = parse_json5(json5).unwrap();
        assert_eq!(sch.wires.len(), 1);
        let w = &sch.wires[0];
        assert_eq!(w.start, (10.0, 20.0));
        assert_eq!(w.end, (30.0, 40.0));
        assert_eq!(w.net_id, Some(1));
        assert_eq!(w.stroke.width, 0.5);
    }

    #[test]
    fn test_parse_labels() {
        let json5 = r#"{
            labels: [{
                text: "VCC",
                type: "global_label",
                position: { x: 50.0, y: 60.0, rotation: 0.0 }
            }]
        }"#;
        let sch = parse_json5(json5).unwrap();
        assert_eq!(sch.labels.len(), 1);
        assert_eq!(sch.labels[0].text, "VCC");
        assert_eq!(sch.labels[0].label_type, "global_label");
        assert_eq!(sch.labels[0].position, (50.0, 60.0, 0.0));
    }

    #[test]
    fn test_parse_junctions() {
        let json5 = r#"{
            junctions: [{ position: [10.0, 20.0], diameter: 1.5 }]
        }"#;
        let sch = parse_json5(json5).unwrap();
        assert_eq!(sch.junctions.len(), 1);
        assert_eq!(sch.junctions[0].position, (10.0, 20.0));
        assert!((sch.junctions[0].diameter - 1.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_parse_lib_symbols() {
        let json5 = r#"{
            lib_symbols: [{
                id: "Device:R",
                reference: "R",
                in_bom: true,
                on_board: true,
                pins: [
                    { number: "1", name: "", type: "passive" },
                    { number: "2", name: "", type: "passive" }
                ]
            }]
        }"#;
        let sch = parse_json5(json5).unwrap();
        assert_eq!(sch.lib_symbols.len(), 1);
        assert_eq!(sch.lib_symbols[0].lib_id, "Device:R");
        assert_eq!(sch.lib_symbols[0].pins.len(), 2);
    }
}
