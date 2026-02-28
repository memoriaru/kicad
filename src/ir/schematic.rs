//! Main schematic IR structure

use std::collections::HashMap;

use crate::error::{Error, Result};
use crate::parser::SExpr;

use super::{Junction, Label, Net, Symbol, SymbolInstance, Wire};

/// Paper size definition
#[derive(Debug, Clone)]
pub struct Paper {
    pub size: String,
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub portrait: bool,
}

impl Default for Paper {
    fn default() -> Self {
        Self {
            size: "A4".to_string(),
            width: None,
            height: None,
            portrait: false,
        }
    }
}

/// Title block information
#[derive(Debug, Clone, Default)]
pub struct TitleBlock {
    pub title: Option<String>,
    pub date: Option<String>,
    pub rev: Option<String>,
    pub company: Option<String>,
    pub comments: Vec<String>,
}

/// Metadata for the schematic
#[derive(Debug, Clone)]
pub struct Metadata {
    pub uuid: String,
    pub version: String,
    pub generator: String,
    pub paper: Paper,
    pub title_block: TitleBlock,
}

impl Default for Metadata {
    fn default() -> Self {
        Self {
            uuid: String::new(),
            version: "20211123".to_string(),
            generator: "eeschema".to_string(),
            paper: Paper::default(),
            title_block: TitleBlock::default(),
        }
    }
}

/// The main schematic structure
#[derive(Debug, Clone, Default)]
pub struct Schematic {
    pub metadata: Metadata,
    pub lib_symbols: Vec<Symbol>,
    pub nets: Vec<Net>,
    pub components: Vec<SymbolInstance>,
    pub wires: Vec<Wire>,
    pub labels: Vec<Label>,
    pub junctions: Vec<Junction>,
    /// Net ID to Net name mapping
    net_map: HashMap<u32, String>,
}

impl Schematic {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get net name by ID
    pub fn get_net_name(&self, id: u32) -> Option<&str> {
        self.net_map.get(&id).map(|s| s.as_str())
    }

    /// Parse from S-expression AST
    pub fn from_sexpr(sexpr: &SExpr) -> Result<Self> {
        let mut schematic = Schematic::new();

        // Expect (kicad_sch ...)
        let items = match sexpr {
            SExpr::List(items) => items,
            _ => return Err(Error::InvalidSExpr("Expected kicad_sch list".to_string())),
        };

        if items.is_empty() || !items[0].is_ident("kicad_sch") {
            return Err(Error::InvalidSExpr(
                "Expected kicad_sch as first element".to_string(),
            ));
        }

        // Process each element
        for item in &items[1..] {
            let list = match item {
                SExpr::List(items) => items,
                _ => continue,
            };

            if list.is_empty() {
                continue;
            }

            let head = match &list[0] {
                SExpr::Atom(crate::parser::ast::Atom::Identifier(s)) => s.as_str(),
                _ => continue,
            };

            match head {
                "version" => {
                    if list.len() >= 2 {
                        schematic.metadata.version = get_string_or_ident(&list[1]);
                    }
                }
                "generator" => {
                    if list.len() >= 2 {
                        schematic.metadata.generator = get_string_or_ident(&list[1]);
                    }
                }
                "uuid" => {
                    if list.len() >= 2 {
                        schematic.metadata.uuid = get_string_or_ident(&list[1]);
                    }
                }
                "paper" => {
                    schematic.metadata.paper = Self::parse_paper(list)?;
                }
                "title_block" => {
                    schematic.metadata.title_block = Self::parse_title_block(list)?;
                }
                "lib_symbols" => {
                    schematic.lib_symbols = Self::parse_lib_symbols(list)?;
                }
                "net" => {
                    if let Some(net) = Self::parse_net(list) {
                        schematic.net_map.insert(net.id, net.name.clone());
                        schematic.nets.push(net);
                    }
                }
                "symbol" => {
                    if let Some(component) = Self::parse_symbol_instance(list, &schematic.net_map) {
                        schematic.components.push(component);
                    }
                }
                "wire" => {
                    if let Some(wire) = Self::parse_wire(list) {
                        schematic.wires.push(wire);
                    }
                }
                "label" | "global_label" | "hierarchical_label" => {
                    if let Some(label) = Self::parse_label(list, head) {
                        schematic.labels.push(label);
                    }
                }
                "junction" => {
                    if let Some(junction) = Self::parse_junction(list) {
                        schematic.junctions.push(junction);
                    }
                }
                _ => {}
            }
        }

        Ok(schematic)
    }

    fn parse_paper(list: &[SExpr]) -> Result<Paper> {
        let mut paper = Paper::default();
        if list.len() >= 2 {
            paper.size = get_string_or_ident(&list[1]);

            if list.len() >= 4 {
                paper.width = get_number(&list[2]);
                paper.height = get_number(&list[3]);
            }
            if list.len() >= 5 {
                paper.portrait = is_ident(&list[4], "portrait");
            }
        }
        Ok(paper)
    }

    fn parse_title_block(list: &[SExpr]) -> Result<TitleBlock> {
        let mut tb = TitleBlock::default();

        for item in &list[1..] {
            if let SExpr::List(sub_list) = item {
                if sub_list.len() < 2 {
                    continue;
                }

                let key = match &sub_list[0] {
                    SExpr::Atom(crate::parser::ast::Atom::Identifier(s)) => s.as_str(),
                    _ => continue,
                };

                let value = get_string_or_ident(&sub_list[1]);

                match key {
                    "title" => tb.title = Some(value),
                    "date" => tb.date = Some(value),
                    "rev" => tb.rev = Some(value),
                    "company" => tb.company = Some(value),
                    "comment" => {
                        if sub_list.len() >= 3 {
                            if let SExpr::Atom(crate::parser::ast::Atom::String(s)) = &sub_list[2] {
                                tb.comments.push(s.clone());
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        Ok(tb)
    }

    fn parse_lib_symbols(list: &[SExpr]) -> Result<Vec<Symbol>> {
        let mut symbols = Vec::new();

        for item in &list[1..] {
            if let SExpr::List(sub_list) = item {
                if sub_list.len() >= 2 && sub_list[0].is_ident("symbol") {
                    if let Some(symbol) = Self::parse_symbol_def(sub_list) {
                        symbols.push(symbol);
                    }
                }
            }
        }

        Ok(symbols)
    }

    fn parse_symbol_def(list: &[SExpr]) -> Option<Symbol> {
        let lib_id = get_string_or_ident(list.get(1)?);
        let mut symbol = Symbol::new(&lib_id);

        for item in &list[2..] {
            if let SExpr::List(sub_list) = item {
                if sub_list.is_empty() {
                    continue;
                }

                let key = match &sub_list[0] {
                    SExpr::Atom(crate::parser::ast::Atom::Identifier(s)) => s.as_str(),
                    _ => continue,
                };

                match key {
                    "pin_numbers" => {
                        symbol.pin_numbers_hidden = sub_list.iter().any(|s| s.is_ident("hide"));
                    }
                    "pin_names" => {
                        symbol.pin_names_hidden = sub_list.iter().any(|s| s.is_ident("hide"));
                    }
                    "power" => {
                        symbol.is_power = true;
                    }
                    "in_bom" => {
                        symbol.in_bom = sub_list.get(1).and_then(get_bool).unwrap_or(true);
                    }
                    "on_board" => {
                        symbol.on_board = sub_list.get(1).and_then(get_bool).unwrap_or(true);
                    }
                    _ => {}
                }
            }
        }

        Some(symbol)
    }

    fn parse_net(list: &[SExpr]) -> Option<Net> {
        if list.len() < 3 {
            return None;
        }

        let id = get_number(&list[1])? as u32;
        let name = get_string_or_ident(&list[2]);

        let net_type = if name.starts_with('+') || name.starts_with('-') || name == "GND" {
            Some("power".to_string())
        } else {
            None
        };

        Some(Net {
            id,
            name,
            net_type,
        })
    }

    fn parse_symbol_instance(list: &[SExpr], net_map: &HashMap<u32, String>) -> Option<SymbolInstance> {
        if list.len() < 2 {
            return None;
        }

        let lib_id = get_string_or_ident(&list[1]);
        let mut component = SymbolInstance::new(&lib_id, "");

        for item in &list[2..] {
            if let SExpr::List(sub_list) = item {
                if sub_list.is_empty() {
                    continue;
                }

                let key = match &sub_list[0] {
                    SExpr::Atom(crate::parser::ast::Atom::Identifier(s)) => s.as_str(),
                    _ => continue,
                };

                match key {
                    "reference" => {
                        component.reference = sub_list.get(1).and_then(get_string).unwrap_or_default();
                    }
                    "value" => {
                        component.value = sub_list.get(1).and_then(get_string).unwrap_or_default();
                    }
                    "footprint" => {
                        component.footprint = sub_list.get(1).and_then(get_string);
                    }
                    "at" => {
                        let x = sub_list.get(1).and_then(get_number).unwrap_or(0.0);
                        let y = sub_list.get(2).and_then(get_number).unwrap_or(0.0);
                        let rot = sub_list.get(3).and_then(get_number).unwrap_or(0.0);
                        component.position = (x, y, rot);
                    }
                    "uuid" => {
                        component.uuid = sub_list.get(1).and_then(get_string);
                    }
                    "unit" => {
                        component.unit = sub_list.get(1).and_then(get_number).unwrap_or(1.0) as u32;
                    }
                    "pin" => {
                        if let Some(pin) = Self::parse_pin_instance(sub_list, net_map) {
                            component.pins.push(pin);
                        }
                    }
                    "property" => {
                        if sub_list.len() >= 3 {
                            let prop_name = get_string_or_ident(&sub_list[1]);
                            let prop_value = get_string_or_ident(&sub_list[2]);
                            component.properties.insert(prop_name, prop_value);
                        }
                    }
                    _ => {}
                }
            }
        }

        if component.reference.is_empty() {
            return None;
        }

        Some(component)
    }

    fn parse_pin_instance(list: &[SExpr], net_map: &HashMap<u32, String>) -> Option<super::PinInstance> {
        let number = get_string_or_ident(list.get(1)?);

        let mut pin = super::PinInstance {
            number: number.clone(),
            name: String::new(),
            pin_type: "passive".to_string(),
            net_id: None,
            net_name: None,
        };

        for item in &list[2..] {
            if let SExpr::List(sub_list) = item {
                if sub_list.is_empty() {
                    continue;
                }

                let key = match &sub_list[0] {
                    SExpr::Atom(crate::parser::ast::Atom::Identifier(s)) => s.as_str(),
                    _ => continue,
                };

                match key {
                    "name" => {
                        pin.name = sub_list.get(1).and_then(get_string).unwrap_or_default();
                    }
                    "type" => {
                        pin.pin_type = sub_list.get(1).and_then(get_ident).unwrap_or("passive").to_string();
                    }
                    "node" => {
                        let net_id = sub_list.get(1).and_then(get_number).unwrap_or(0.0) as u32;
                        pin.net_id = Some(net_id);
                        pin.net_name = net_map.get(&net_id).cloned();
                    }
                    _ => {}
                }
            }
        }

        Some(pin)
    }

    fn parse_wire(list: &[SExpr]) -> Option<Wire> {
        let mut wire = Wire::new((0.0, 0.0), (0.0, 0.0));

        for item in &list[1..] {
            if let SExpr::List(sub_list) = item {
                if sub_list.is_empty() {
                    continue;
                }

                let key = match &sub_list[0] {
                    SExpr::Atom(crate::parser::ast::Atom::Identifier(s)) => s.as_str(),
                    _ => continue,
                };

                match key {
                    "pts" => {
                        let points: Vec<(f64, f64)> = sub_list[1..]
                            .iter()
                            .filter_map(|pt| {
                                if let SExpr::List(pt_list) = pt {
                                    if pt_list.len() >= 3 && pt_list[0].is_ident("xy") {
                                        let x = get_number(&pt_list[1])?;
                                        let y = get_number(&pt_list[2])?;
                                        return Some((x, y));
                                    }
                                }
                                None
                            })
                            .collect();

                        if points.len() >= 2 {
                            wire.start = points[0];
                            wire.end = points[1];
                        }
                    }
                    "stroke" => {
                        for stroke_item in &sub_list[1..] {
                            if let SExpr::List(stroke_list) = stroke_item {
                                if stroke_list[0].is_ident("width") {
                                    wire.stroke.width = stroke_list.get(1).and_then(get_number).unwrap_or(0.0);
                                } else if stroke_list[0].is_ident("type") {
                                    wire.stroke.stroke_type = stroke_list.get(1).and_then(get_ident).unwrap_or("default").to_string();
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        Some(wire)
    }

    fn parse_label(list: &[SExpr], label_type: &str) -> Option<Label> {
        let text = get_string_or_ident(list.get(1)?);

        let mut label = Label {
            text,
            position: (0.0, 0.0, 0.0),
            label_type: label_type.to_string(),
            net_name: None,
        };

        for item in &list[2..] {
            if let SExpr::List(sub_list) = item {
                if sub_list.is_empty() {
                    continue;
                }

                if sub_list[0].is_ident("at") {
                    let x = sub_list.get(1).and_then(get_number).unwrap_or(0.0);
                    let y = sub_list.get(2).and_then(get_number).unwrap_or(0.0);
                    let rot = sub_list.get(3).and_then(get_number).unwrap_or(0.0);
                    label.position = (x, y, rot);
                }
            }
        }

        Some(label)
    }

    fn parse_junction(list: &[SExpr]) -> Option<Junction> {
        let mut junction = Junction {
            position: (0.0, 0.0),
            diameter: 1.27,
        };

        for item in &list[1..] {
            if let SExpr::List(sub_list) = item {
                if sub_list.is_empty() {
                    continue;
                }

                if sub_list[0].is_ident("at") {
                    let x = sub_list.get(1).and_then(get_number).unwrap_or(0.0);
                    let y = sub_list.get(2).and_then(get_number).unwrap_or(0.0);
                    junction.position = (x, y);
                } else if sub_list[0].is_ident("diameter") {
                    junction.diameter = sub_list.get(1).and_then(get_number).unwrap_or(1.27);
                }
            }
        }

        Some(junction)
    }
}

// Helper functions
fn get_string_or_ident(expr: &SExpr) -> String {
    match expr {
        SExpr::Atom(crate::parser::ast::Atom::String(s)) => s.clone(),
        SExpr::Atom(crate::parser::ast::Atom::Identifier(s)) => s.clone(),
        _ => String::new(),
    }
}

fn get_string(expr: &SExpr) -> Option<String> {
    match expr {
        SExpr::Atom(crate::parser::ast::Atom::String(s)) => Some(s.clone()),
        _ => None,
    }
}

fn get_ident(expr: &SExpr) -> Option<&str> {
    match expr {
        SExpr::Atom(crate::parser::ast::Atom::Identifier(s)) => Some(s.as_str()),
        _ => None,
    }
}

fn get_number(expr: &SExpr) -> Option<f64> {
    match expr {
        SExpr::Atom(crate::parser::ast::Atom::Number(n)) => Some(*n),
        _ => None,
    }
}

fn get_bool(expr: &SExpr) -> Option<bool> {
    match expr {
        SExpr::Atom(crate::parser::ast::Atom::Bool(b)) => Some(*b),
        _ => None,
    }
}

fn is_ident(expr: &SExpr, name: &str) -> bool {
    match expr {
        SExpr::Atom(crate::parser::ast::Atom::Identifier(s)) => s == name,
        _ => false,
    }
}
