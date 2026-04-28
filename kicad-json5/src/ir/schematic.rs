//! Main schematic IR structure

use std::collections::HashMap;

use crate::error::{Error, Result};
use crate::parser::SExpr;

use super::graphic::{
    Arc, Circle, Fill, FillType, Font, GraphicElement, HorizontalAlign, Justify, PinGraphic,
    PinType, Polyline, Rectangle, Stroke, StrokeType, SymbolUnit, Text, TextEffects, VerticalAlign,
};
use super::{Bus, BusEntry, Junction, Label, Net, NoConnect, Property, Symbol, SymbolInstance, Wire};
use super::component::{InstancePath, InstanceProject};

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
    pub generator_version: Option<String>,
    pub paper: Paper,
    pub title_block: TitleBlock,
}

impl Default for Metadata {
    fn default() -> Self {
        Self {
            uuid: String::new(),
            version: String::new(),
            generator: "kicad-json5".to_string(),
            generator_version: None,
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
    pub no_connects: Vec<NoConnect>,
    pub buses: Vec<Bus>,
    pub bus_entries: Vec<BusEntry>,
    /// Text notes placed on the schematic
    pub text_items: Vec<TextItem>,
    /// Schematic-level graphic polylines (section dividers, etc.)
    pub polylines: Vec<Polyline>,
    /// Hierarchical sheet instances
    pub sheets: Vec<Sheet>,
    /// Net ID to Net name mapping
    net_map: HashMap<u32, String>,
}

/// A text note placed directly on the schematic
#[derive(Debug, Clone)]
pub struct TextItem {
    pub text: String,
    pub position: (f64, f64, f64), // x, y, rotation
    pub effects: TextEffects,
}

/// A hierarchical sheet instance
#[derive(Debug, Clone)]
pub struct Sheet {
    /// Position of the sheet box top-left corner (x, y)
    pub position: (f64, f64),
    /// Size of the sheet box (width, height)
    pub size: (f64, f64),
    /// Border stroke
    pub stroke: Stroke,
    /// Fill (typically blue with alpha)
    pub fill: Fill,
    /// Sheet name property
    pub sheet_name: SheetProperty,
    /// Sheet file property
    pub sheet_file: SheetProperty,
    /// Sheet pins
    pub pins: Vec<SheetPin>,
}

/// A property on a sheet (Sheetname or Sheetfile)
#[derive(Debug, Clone)]
pub struct SheetProperty {
    pub value: String,
    pub position: (f64, f64, f64),
    pub effects: TextEffects,
}

impl Default for SheetProperty {
    fn default() -> Self {
        Self {
            value: String::new(),
            position: (0.0, 0.0, 0.0),
            effects: TextEffects::default(),
        }
    }
}

/// A pin on a hierarchical sheet
#[derive(Debug, Clone)]
pub struct SheetPin {
    /// Pin name
    pub name: String,
    /// Pin electrical type
    pub pin_type: PinType,
    /// Position (x, y, rotation). Rotation: 0=right, 90=bottom, 180=left, 270=top
    pub position: (f64, f64, f64),
    /// Text effects (includes custom color for the pin name)
    pub effects: TextEffects,
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
                "generator_version" => {
                    if list.len() >= 2 {
                        schematic.metadata.generator_version = Some(get_string_or_ident(&list[1]));
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
                "no_connect" => {
                    if let Some(no_connect) = Self::parse_no_connect(list) {
                        schematic.no_connects.push(no_connect);
                    }
                }
                "bus" => {
                    if let Some(bus) = Self::parse_bus(list) {
                        schematic.buses.push(bus);
                    }
                }
                "bus_entry" => {
                    if let Some(bus_entry) = Self::parse_bus_entry(list) {
                        schematic.bus_entries.push(bus_entry);
                    }
                }
                "text" => {
                    if let Some(text) = Self::parse_text_item(list) {
                        schematic.text_items.push(text);
                    }
                }
                "polyline" => {
                    if let Some(polyline) = Self::parse_polyline(list) {
                        schematic.polylines.push(polyline);
                    }
                }
                "sheet" => {
                    if let Some(sheet) = Self::parse_sheet(list) {
                        schematic.sheets.push(sheet);
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
                        // v8: (pin_numbers hide) / v9: (pin_numbers (hide yes))
                        symbol.pin_numbers_hidden = sub_list.iter().any(|s| {
                            s.is_ident("hide") ||
                            (s.is_list() && s.as_list().unwrap().first().map_or(false, |f| f.is_ident("hide")))
                        });
                    }
                    "pin_names" => {
                        // v8: (pin_names hide) / v9: (pin_names (offset ...) (hide yes))
                        symbol.pin_names_hidden = sub_list.iter().any(|s| {
                            s.is_ident("hide") ||
                            (s.is_list() && s.as_list().unwrap().first().map_or(false, |f| f.is_ident("hide")))
                        });
                        // Check for offset
                        for sub_item in &sub_list[1..] {
                            if let SExpr::List(offset_list) = sub_item {
                                if offset_list.first().map_or(false, |f| f.is_ident("offset")) {
                                    symbol.pin_name_offset =
                                        offset_list.get(1).and_then(get_number).unwrap_or(0.254);
                                }
                            }
                        }
                    }
                    "power" => {
                        symbol.is_power = true;
                    }
                    "exclude_from_sim" => {
                        symbol.exclude_from_sim = sub_list.get(1).and_then(get_bool).unwrap_or(false);
                    }
                    "in_bom" => {
                        symbol.in_bom = sub_list.get(1).and_then(get_bool).unwrap_or(true);
                    }
                    "on_board" => {
                        symbol.on_board = sub_list.get(1).and_then(get_bool).unwrap_or(true);
                    }
                    "in_pos_files" => {
                        symbol.in_pos_files = sub_list.get(1).and_then(get_bool).unwrap_or(true);
                    }
                    "duplicate_pin_numbers_are_jumpers" => {
                        symbol.duplicate_pin_numbers_are_jumpers = sub_list.get(1).and_then(get_bool).unwrap_or(false);
                    }
                    // Parse graphic elements
                    "polyline" => {
                        if let Some(polyline) = Self::parse_polyline(sub_list) {
                            symbol.graphics.push(GraphicElement::Polyline(polyline));
                        }
                    }
                    "rectangle" => {
                        if let Some(rect) = Self::parse_rectangle(sub_list) {
                            symbol.graphics.push(GraphicElement::Rectangle(rect));
                        }
                    }
                    "circle" => {
                        if let Some(circle) = Self::parse_circle(sub_list) {
                            symbol.graphics.push(GraphicElement::Circle(circle));
                        }
                    }
                    "arc" => {
                        if let Some(arc) = Self::parse_arc(sub_list) {
                            symbol.graphics.push(GraphicElement::Arc(arc));
                        }
                    }
                    "text" => {
                        if let Some(text) = Self::parse_text(sub_list) {
                            symbol.graphics.push(GraphicElement::Text(text));
                        }
                    }
                    "pin" => {
                        if let Some(pin) = Self::parse_pin_graphic(sub_list) {
                            symbol.graphics.push(GraphicElement::Pin(pin));
                        }
                    }
                    "property" => {
                        if sub_list.len() >= 3 {
                            let prop_name = get_string_or_ident(&sub_list[1]);
                            let prop_value = get_string_or_ident(&sub_list[2]);
                            let mut prop = Property::new(&prop_name, &prop_value);
                            for prop_item in &sub_list[3..] {
                                if let SExpr::List(prop_sub) = prop_item {
                                    if prop_sub.is_empty() { continue; }
                                    match get_ident(&prop_sub[0]) {
                                        Some("at") => {
                                            prop.position = (
                                                prop_sub.get(1).and_then(get_number).unwrap_or(0.0),
                                                prop_sub.get(2).and_then(get_number).unwrap_or(0.0),
                                                prop_sub.get(3).and_then(get_number).unwrap_or(0.0),
                                            );
                                        }
                                        Some("effects") => {
                                            prop.effects = Self::parse_effects(prop_sub);
                                        }
                                        Some("show_name") => {
                                            prop.show_name = prop_sub.get(1).and_then(get_bool).unwrap_or(false);
                                        }
                                        Some("do_not_autoplace") => {
                                            prop.do_not_autoplace = prop_sub.get(1).and_then(get_bool).unwrap_or(false);
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            match prop_name.as_str() {
                                "Reference" => symbol.reference = prop_value,
                                "Value" => symbol.value = Some(prop_value),
                                _ => {}
                            }
                            symbol.properties.push(prop);
                        }
                    }
                    // Parse symbol units (e.g., "symbol_0_1", "symbol_1_1")
                    // In KiCad 8, symbol units are nested symbols like (symbol "C_0_1" ...)
                    "symbol" => {
                        // Check if this is a unit definition (has a name like "C_0_1")
                        if sub_list.len() >= 2 {
                            let unit_name = get_string_or_ident(&sub_list[1]);
                            // Unit names typically contain underscores like "C_0_1"
                            if unit_name.contains('_') {
                                if let Some(unit) = Self::parse_symbol_unit(sub_list, &unit_name) {
                                    symbol.units.push(unit);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        Some(symbol)
    }

    fn parse_polyline(list: &[SExpr]) -> Option<Polyline> {
        let mut polyline = Polyline::new();

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
                        for pt in &sub_list[1..] {
                            if let SExpr::List(pt_list) = pt {
                                if pt_list.len() >= 3 && pt_list[0].is_ident("xy") {
                                    let x = get_number(&pt_list[1])?;
                                    let y = get_number(&pt_list[2])?;
                                    polyline.points.push((x, y));
                                }
                            }
                        }
                    }
                    "stroke" => {
                        polyline.stroke = Self::parse_stroke(sub_list);
                    }
                    "fill" => {
                        polyline.fill = Self::parse_fill(sub_list);
                    }
                    _ => {}
                }
            }
        }

        if polyline.points.is_empty() {
            return None;
        }
        Some(polyline)
    }

    fn parse_rectangle(list: &[SExpr]) -> Option<Rectangle> {
        let mut start: Option<(f64, f64)> = None;
        let mut end: Option<(f64, f64)> = None;
        let mut stroke = Stroke::default();
        let mut fill = Fill::none();

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
                    "start" => {
                        let x = get_number(sub_list.get(1)?)?;
                        let y = get_number(sub_list.get(2)?)?;
                        start = Some((x, y));
                    }
                    "end" => {
                        let x = get_number(sub_list.get(1)?)?;
                        let y = get_number(sub_list.get(2)?)?;
                        end = Some((x, y));
                    }
                    "stroke" => {
                        stroke = Self::parse_stroke(sub_list);
                    }
                    "fill" => {
                        fill = Self::parse_fill(sub_list);
                    }
                    _ => {}
                }
            }
        }

        Some(Rectangle {
            start: start?,
            end: end?,
            stroke,
            fill,
        })
    }

    fn parse_circle(list: &[SExpr]) -> Option<Circle> {
        let mut center: Option<(f64, f64)> = None;
        let mut radius: Option<f64> = None;
        let mut stroke = Stroke::default();
        let mut fill = Fill::none();

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
                    "center" => {
                        let x = get_number(sub_list.get(1)?)?;
                        let y = get_number(sub_list.get(2)?)?;
                        center = Some((x, y));
                    }
                    "radius" => {
                        radius = get_number(sub_list.get(1)?);
                    }
                    "stroke" => {
                        stroke = Self::parse_stroke(sub_list);
                    }
                    "fill" => {
                        fill = Self::parse_fill(sub_list);
                    }
                    _ => {}
                }
            }
        }

        Some(Circle {
            center: center?,
            radius: radius?,
            stroke,
            fill,
        })
    }

    fn parse_arc(list: &[SExpr]) -> Option<Arc> {
        let mut start: Option<(f64, f64)> = None;
        let mut mid: Option<(f64, f64)> = None;
        let mut end: Option<(f64, f64)> = None;
        let mut stroke = Stroke::default();
        let mut fill = Fill::none();

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
                    "start" => {
                        let x = get_number(sub_list.get(1)?)?;
                        let y = get_number(sub_list.get(2)?)?;
                        start = Some((x, y));
                    }
                    "mid" => {
                        let x = get_number(sub_list.get(1)?)?;
                        let y = get_number(sub_list.get(2)?)?;
                        mid = Some((x, y));
                    }
                    "end" => {
                        let x = get_number(sub_list.get(1)?)?;
                        let y = get_number(sub_list.get(2)?)?;
                        end = Some((x, y));
                    }
                    "stroke" => {
                        stroke = Self::parse_stroke(sub_list);
                    }
                    "fill" => {
                        fill = Self::parse_fill(sub_list);
                    }
                    _ => {}
                }
            }
        }

        Some(Arc {
            start: start?,
            mid: mid?,
            end: end?,
            stroke,
            fill,
        })
    }

    fn parse_text(list: &[SExpr]) -> Option<Text> {
        let text = get_string_or_ident(list.get(1)?);
        let mut position = (0.0, 0.0, 0.0);
        let mut effects = TextEffects::default();

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
                    "at" => {
                        let x = sub_list.get(1).and_then(get_number).unwrap_or(0.0);
                        let y = sub_list.get(2).and_then(get_number).unwrap_or(0.0);
                        let rot = sub_list.get(3).and_then(get_number).unwrap_or(0.0);
                        position = (x, y, rot);
                    }
                    "effects" => {
                        effects = Self::parse_effects(sub_list);
                    }
                    _ => {}
                }
            }
        }

        Some(Text { text, position, effects })
    }

    /// Parse a top-level schematic text note — same format as symbol text.
    fn parse_text_item(list: &[SExpr]) -> Option<TextItem> {
        let text = get_string_or_ident(list.get(1)?);
        let mut position = (0.0, 0.0, 0.0);
        let mut effects = TextEffects::default();

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
                    "at" => {
                        let x = sub_list.get(1).and_then(get_number).unwrap_or(0.0);
                        let y = sub_list.get(2).and_then(get_number).unwrap_or(0.0);
                        let rot = sub_list.get(3).and_then(get_number).unwrap_or(0.0);
                        position = (x, y, rot);
                    }
                    "effects" => {
                        effects = Self::parse_effects(sub_list);
                    }
                    _ => {}
                }
            }
        }

        Some(TextItem { text, position, effects })
    }

    fn parse_pin_graphic(list: &[SExpr]) -> Option<PinGraphic> {
        if list.len() < 3 {
            return None;
        }

        let pin_type = PinType::from_str(get_ident(&list[1]).unwrap_or("passive"));
        let mut pin = PinGraphic::new("", "");
        pin.pin_type = pin_type;

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
                        pin.name_effects = Self::parse_effects_from_list(sub_list);
                    }
                    "number" => {
                        pin.number = sub_list.get(1).and_then(get_string).unwrap_or_default();
                        pin.number_effects = Self::parse_effects_from_list(sub_list);
                    }
                    "at" => {
                        let x = sub_list.get(1).and_then(get_number).unwrap_or(0.0);
                        let y = sub_list.get(2).and_then(get_number).unwrap_or(0.0);
                        let rot = sub_list.get(3).and_then(get_number).unwrap_or(0.0);
                        pin.position = (x, y, rot);
                    }
                    "length" => {
                        pin.length = sub_list.get(1).and_then(get_number).unwrap_or(2.54);
                    }
                    _ => {}
                }
            } else if let SExpr::Atom(crate::parser::ast::Atom::Identifier(s)) = item {
                if s == "hide" {
                    pin.hidden = true;
                }
            }
        }

        Some(pin)
    }

    fn parse_symbol_unit(list: &[SExpr], key: &str) -> Option<SymbolUnit> {
        // Parse "C_0_1" format where 0 is unit_id and 1 is style_id
        let parts: Vec<&str> = key.split('_').collect();
        if parts.len() < 3 {
            return None;
        }
        // The format is "Name_Unit_Style", so we need the last two parts
        let style_id = parts.last()?.parse().ok()?;
        let unit_id = parts.get(parts.len() - 2)?.parse().ok()?;

        let mut unit = SymbolUnit::new(unit_id, style_id);
        unit.name = key.to_string();

        // In KiCad 8, format is (symbol "C_0_1" (polyline ...) (pin ...))
        // So we skip list[0] ("symbol") and list[1] ("C_0_1")
        for item in &list[2..] {
            if let SExpr::List(sub_list) = item {
                if sub_list.is_empty() {
                    continue;
                }

                let elem_key = match &sub_list[0] {
                    SExpr::Atom(crate::parser::ast::Atom::Identifier(s)) => s.as_str(),
                    _ => continue,
                };

                match elem_key {
                    "polyline" => {
                        if let Some(polyline) = Self::parse_polyline(sub_list) {
                            unit.graphics.push(GraphicElement::Polyline(polyline));
                        }
                    }
                    "rectangle" => {
                        if let Some(rect) = Self::parse_rectangle(sub_list) {
                            unit.graphics.push(GraphicElement::Rectangle(rect));
                        }
                    }
                    "circle" => {
                        if let Some(circle) = Self::parse_circle(sub_list) {
                            unit.graphics.push(GraphicElement::Circle(circle));
                        }
                    }
                    "arc" => {
                        if let Some(arc) = Self::parse_arc(sub_list) {
                            unit.graphics.push(GraphicElement::Arc(arc));
                        }
                    }
                    "text" => {
                        if let Some(text) = Self::parse_text(sub_list) {
                            unit.graphics.push(GraphicElement::Text(text));
                        }
                    }
                    "pin" => {
                        if let Some(pin) = Self::parse_pin_graphic(sub_list) {
                            unit.graphics.push(GraphicElement::Pin(pin));
                        }
                    }
                    _ => {}
                }
            }
        }

        Some(unit)
    }

    fn parse_stroke(list: &[SExpr]) -> Stroke {
        let mut stroke = Stroke::default();

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
                    "width" => {
                        stroke.width = sub_list.get(1).and_then(get_number).unwrap_or(0.0);
                    }
                    "type" => {
                        if let Some(type_str) = sub_list.get(1).and_then(get_ident) {
                            stroke.stroke_type = StrokeType::from_str(type_str);
                        }
                    }
                    _ => {}
                }
            }
        }

        stroke
    }

    fn parse_fill(list: &[SExpr]) -> Fill {
        let mut fill = Fill::none();

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
                    "type" => {
                        if let Some(type_str) = sub_list.get(1).and_then(get_ident) {
                            fill.fill_type = FillType::from_str(type_str);
                        }
                    }
                    _ => {}
                }
            }
        }

        fill
    }

    /// Parse sheet fill — uses `(color R G B A)` where alpha is 0.0-1.0 float,
    /// unlike font colors which use integer alpha.
    fn parse_sheet_fill(list: &[SExpr]) -> Fill {
        let mut fill = Fill::none();

        for item in &list[1..] {
            if let SExpr::List(sub_list) = item {
                if sub_list.is_empty() {
                    continue;
                }

                let key = match &sub_list[0] {
                    SExpr::Atom(crate::parser::ast::Atom::Identifier(s)) => s.as_str(),
                    _ => continue,
                };

                if key == "color" {
                    let r = sub_list.get(1).and_then(get_number).unwrap_or(0.0) as u8;
                    let g = sub_list.get(2).and_then(get_number).unwrap_or(0.0) as u8;
                    let b = sub_list.get(3).and_then(get_number).unwrap_or(0.0) as u8;
                    let a_float = sub_list.get(4).and_then(get_number).unwrap_or(1.0);
                    let a = (a_float * 255.0).round() as u8;
                    fill.fill_type = FillType::Color;
                    fill.color = Some((r, g, b, a));
                }
            }
        }

        fill
    }

    fn parse_sheet(list: &[SExpr]) -> Option<Sheet> {
        let mut position = (0.0, 0.0);
        let mut size = (0.0, 0.0);
        let mut stroke = Stroke::default();
        let mut fill = Fill::none();
        let mut sheet_name = SheetProperty::default();
        let mut sheet_file = SheetProperty::default();
        let mut pins = Vec::new();

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
                    "at" => {
                        let x = sub_list.get(1).and_then(get_number).unwrap_or(0.0);
                        let y = sub_list.get(2).and_then(get_number).unwrap_or(0.0);
                        position = (x, y);
                    }
                    "size" => {
                        let w = sub_list.get(1).and_then(get_number).unwrap_or(0.0);
                        let h = sub_list.get(2).and_then(get_number).unwrap_or(0.0);
                        size = (w, h);
                    }
                    "stroke" => {
                        stroke = Self::parse_stroke(sub_list);
                    }
                    "fill" => {
                        fill = Self::parse_sheet_fill(sub_list);
                    }
                    "property" => {
                        if sub_list.len() >= 3 {
                            let prop_name = get_string_or_ident(&sub_list[1]);
                            let prop_value = get_string_or_ident(&sub_list[2]);
                            let mut prop_pos = (0.0, 0.0, 0.0);
                            let mut prop_effects = TextEffects::default();

                            for prop_item in &sub_list[3..] {
                                if let SExpr::List(prop_sub) = prop_item {
                                    if prop_sub.is_empty() {
                                        continue;
                                    }
                                    match get_ident(&prop_sub[0]) {
                                        Some("at") => {
                                            prop_pos = (
                                                prop_sub.get(1).and_then(get_number).unwrap_or(0.0),
                                                prop_sub.get(2).and_then(get_number).unwrap_or(0.0),
                                                prop_sub.get(3).and_then(get_number).unwrap_or(0.0),
                                            );
                                        }
                                        Some("effects") => {
                                            prop_effects = Self::parse_effects(prop_sub);
                                        }
                                        _ => {}
                                    }
                                }
                            }

                            match prop_name.as_str() {
                                "Sheetname" => {
                                    sheet_name = SheetProperty {
                                        value: prop_value,
                                        position: prop_pos,
                                        effects: prop_effects,
                                    };
                                }
                                "Sheetfile" => {
                                    sheet_file = SheetProperty {
                                        value: prop_value,
                                        position: prop_pos,
                                        effects: prop_effects,
                                    };
                                }
                                _ => {}
                            }
                        }
                    }
                    "pin" => {
                        // (pin "VDD" input (at x y rot) (effects ...))
                        if sub_list.len() >= 3 {
                            let pin_name = get_string_or_ident(&sub_list[1]);
                            let pin_type_str = get_ident(&sub_list[2]).unwrap_or("passive");
                            let pin_type = PinType::from_str(pin_type_str);
                            let mut pin_pos = (0.0, 0.0, 0.0);
                            let mut pin_effects = TextEffects::default();

                            for pin_item in &sub_list[3..] {
                                if let SExpr::List(pin_sub) = pin_item {
                                    if pin_sub.is_empty() {
                                        continue;
                                    }
                                    match get_ident(&pin_sub[0]) {
                                        Some("at") => {
                                            pin_pos = (
                                                pin_sub.get(1).and_then(get_number).unwrap_or(0.0),
                                                pin_sub.get(2).and_then(get_number).unwrap_or(0.0),
                                                pin_sub.get(3).and_then(get_number).unwrap_or(0.0),
                                            );
                                        }
                                        Some("effects") => {
                                            pin_effects = Self::parse_effects(pin_sub);
                                        }
                                        _ => {}
                                    }
                                }
                            }

                            pins.push(SheetPin {
                                name: pin_name,
                                pin_type,
                                position: pin_pos,
                                effects: pin_effects,
                            });
                        }
                    }
                    _ => {}
                }
            }
        }

        Some(Sheet {
            position,
            size,
            stroke,
            fill,
            sheet_name,
            sheet_file,
            pins,
        })
    }

    fn parse_effects(list: &[SExpr]) -> TextEffects {
        let mut effects = TextEffects::default();

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
                    "font" => {
                        effects.font = Self::parse_font(sub_list);
                    }
                    "justify" => {
                        effects.justify = Self::parse_justify(sub_list);
                    }
                    "hide" => {
                        effects.hide = sub_list.get(1).and_then(get_bool).unwrap_or(true);
                    }
                    _ => {}
                }
            }
        }

        effects
    }

    fn parse_effects_from_list(list: &[SExpr]) -> TextEffects {
        let mut effects = TextEffects::default();

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
                    "font" => {
                        effects.font = Self::parse_font(sub_list);
                    }
                    "justify" => {
                        effects.justify = Self::parse_justify(sub_list);
                    }
                    "hide" => {
                        effects.hide = sub_list.get(1).and_then(get_bool).unwrap_or(true);
                    }
                    _ => {}
                }
            }
        }

        effects
    }

    fn parse_font(list: &[SExpr]) -> Font {
        let mut font = Font::default();

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
                    "size" => {
                        let h = sub_list.get(1).and_then(get_number).unwrap_or(1.27);
                        let w = sub_list.get(2).and_then(get_number).unwrap_or(1.27);
                        font.size = (w, h);
                    }
                    "color" => {
                        let r = sub_list.get(1).and_then(get_number).unwrap_or(0.0) as u8;
                        let g = sub_list.get(2).and_then(get_number).unwrap_or(0.0) as u8;
                        let b = sub_list.get(3).and_then(get_number).unwrap_or(0.0) as u8;
                        let a = sub_list.get(4).and_then(get_number).unwrap_or(1.0) as u8;
                        font.color = Some((r, g, b, a));
                    }
                    _ => {}
                }
            }

            // Check for bold/italic flags: either atom "bold" or list "(bold yes)"
            if item.is_ident("bold") {
                font.bold = true;
            }
            if item.is_ident("italic") {
                font.italic = true;
            }
            if let SExpr::List(sub_list) = item {
                if !sub_list.is_empty() {
                    let key = match &sub_list[0] {
                        SExpr::Atom(crate::parser::ast::Atom::Identifier(s)) => s.as_str(),
                        _ => "",
                    };
                    match key {
                        "bold" => {
                            font.bold = sub_list.get(1).and_then(get_bool).unwrap_or(true);
                        }
                        "italic" => {
                            font.italic = sub_list.get(1).and_then(get_bool).unwrap_or(true);
                        }
                        _ => {}
                    }
                }
            }
        }

        font
    }

    fn parse_justify(list: &[SExpr]) -> Justify {
        let mut justify = Justify::default();

        for item in &list[1..] {
            if item.is_ident("left") {
                justify.horizontal = HorizontalAlign::Left;
            } else if item.is_ident("center") {
                justify.horizontal = HorizontalAlign::Center;
            } else if item.is_ident("right") {
                justify.horizontal = HorizontalAlign::Right;
            } else if item.is_ident("top") {
                justify.vertical = VerticalAlign::Top;
            } else if item.is_ident("bottom") {
                justify.vertical = VerticalAlign::Bottom;
            } else if item.is_ident("mirror") {
                justify.mirror = true;
            }
        }

        justify
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

        // In KiCad 8 format, lib_id is a sub-list: (lib_id "Device:C")
        // Find lib_id by iterating through sub-lists
        let mut lib_id = String::new();
        for item in &list[1..] {
            if let SExpr::List(sub_list) = item {
                if sub_list.first().map_or(false, |f| f.is_ident("lib_id")) {
                    if let Some(id) = sub_list.get(1).and_then(get_string) {
                        lib_id = id;
                        break;
                    }
                }
            }
        }

        if lib_id.is_empty() {
            return None;
        }

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
                    // Note: "reference" and "value" are not separate keys in KiCad 8
                    // They are handled in the "property" branch below
                    "footprint" => {
                        component.footprint = sub_list.get(1).and_then(get_string);
                    }
                    "at" => {
                        let x = sub_list.get(1).and_then(get_number).unwrap_or(0.0);
                        let y = sub_list.get(2).and_then(get_number).unwrap_or(0.0);
                        let rot = sub_list.get(3).and_then(get_number).unwrap_or(0.0);
                        component.position = (x, y, rot);
                    }
                    "mirror" => {
                        if let Some(mirror_str) = sub_list.get(1).and_then(get_ident) {
                            component.mirror = match mirror_str {
                                "x" => super::Mirror::X,
                                "y" => super::Mirror::Y,
                                _ => super::Mirror::None,
                            };
                        }
                    }
                    "uuid" => {
                        component.uuid = sub_list.get(1).and_then(get_string);
                    }
                    "unit" => {
                        component.unit = sub_list.get(1).and_then(get_number).unwrap_or(1.0) as u32;
                    }
                    "exclude_from_sim" => {
                        component.exclude_from_sim = sub_list.get(1).and_then(get_bool).unwrap_or(false);
                    }
                    "in_bom" => {
                        component.in_bom = sub_list.get(1).and_then(get_bool).unwrap_or(true);
                    }
                    "on_board" => {
                        component.on_board = sub_list.get(1).and_then(get_bool).unwrap_or(true);
                    }
                    "dnp" => {
                        component.dnp = sub_list.get(1).and_then(get_bool).unwrap_or(false);
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
                            component.properties.insert(prop_name.clone(), prop_value.clone());

                            // Parse property position and hide status
                            let mut prop = Property::new(&prop_name, &prop_value);
                            for prop_item in &sub_list[3..] {
                                if let SExpr::List(prop_sub) = prop_item {
                                    if prop_sub.is_empty() {
                                        continue;
                                    }
                                    match get_ident(&prop_sub[0]) {
                                        Some("at") => {
                                            prop.position = (
                                                prop_sub.get(1).and_then(get_number).unwrap_or(0.0),
                                                prop_sub.get(2).and_then(get_number).unwrap_or(0.0),
                                                prop_sub.get(3).and_then(get_number).unwrap_or(0.0),
                                            );
                                        }
                                        Some("effects") => {
                                            prop.effects = Self::parse_effects(prop_sub);
                                        }
                                        Some("show_name") => {
                                            prop.show_name = prop_sub.get(1).and_then(get_bool).unwrap_or(false);
                                        }
                                        Some("do_not_autoplace") => {
                                            prop.do_not_autoplace = prop_sub.get(1).and_then(get_bool).unwrap_or(false);
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            component.properties_ext.push(prop);

                            // Also set common properties directly on component
                            match prop_name.as_str() {
                                "Reference" => component.reference = prop_value,
                                "Value" => component.value = prop_value,
                                "Footprint" => component.footprint = Some(prop_value),
                                _ => {}
                            }
                        }
                    }
                    "instances" => {
                        for inst_item in &sub_list[1..] {
                            if let SExpr::List(proj_list) = inst_item {
                                if proj_list.first().map_or(false, |f| f.is_ident("project")) {
                                    let proj_name = proj_list.get(1).and_then(get_string).unwrap_or_default();
                                    let mut project = InstanceProject {
                                        name: proj_name,
                                        paths: Vec::new(),
                                    };
                                    for path_item in &proj_list[2..] {
                                        if let SExpr::List(path_list) = path_item {
                                            if path_list.first().map_or(false, |f| f.is_ident("path")) {
                                                let path_str = path_list.get(1).and_then(get_string).unwrap_or_default();
                                                let mut ref_str = String::new();
                                                let mut unit_val = 1u32;
                                                for path_child in &path_list[2..] {
                                                    if let SExpr::List(child_list) = path_child {
                                                        match get_ident(&child_list[0]) {
                                                            Some("reference") => {
                                                                ref_str = child_list.get(1).map(|v| get_string_or_ident(v)).unwrap_or_default();
                                                            }
                                                            Some("unit") => {
                                                                unit_val = child_list.get(1).and_then(get_number).unwrap_or(1.0) as u32;
                                                            }
                                                            _ => {}
                                                        }
                                                    }
                                                }
                                                project.paths.push(InstancePath {
                                                    path: path_str,
                                                    reference: ref_str,
                                                    unit: unit_val,
                                                });
                                            }
                                        }
                                    }
                                    component.instances.projects.push(project);
                                }
                            }
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
            nc: false,
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
                                    wire.stroke.stroke_type = stroke_list.get(1).and_then(get_ident)
                                        .map(StrokeType::from_str)
                                        .unwrap_or(StrokeType::Default);
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

        let mut label = Label::new(text, label_type);

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
                    "at" => {
                        let x = sub_list.get(1).and_then(get_number).unwrap_or(0.0);
                        let y = sub_list.get(2).and_then(get_number).unwrap_or(0.0);
                        let rot = sub_list.get(3).and_then(get_number).unwrap_or(0.0);
                        label.position = (x, y, rot);
                    }
                    "shape" => {
                        label.shape = sub_list.get(1).and_then(get_ident).unwrap_or("passive").to_string();
                    }
                    "effects" => {
                        label.effects = Self::parse_effects(sub_list);
                    }
                    _ => {}
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

    fn parse_no_connect(list: &[SExpr]) -> Option<NoConnect> {
        let mut no_connect = NoConnect::new((0.0, 0.0));

        for item in &list[1..] {
            if let SExpr::List(sub_list) = item {
                if sub_list.is_empty() {
                    continue;
                }

                if sub_list[0].is_ident("at") {
                    let x = sub_list.get(1).and_then(get_number).unwrap_or(0.0);
                    let y = sub_list.get(2).and_then(get_number).unwrap_or(0.0);
                    no_connect.position = (x, y);
                } else if sub_list[0].is_ident("uuid") {
                    no_connect.uuid = sub_list.get(1).and_then(get_string);
                }
            }
        }

        Some(no_connect)
    }

    fn parse_bus(list: &[SExpr]) -> Option<Bus> {
        let mut bus = Bus::new();

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
                        for pt in &sub_list[1..] {
                            if let SExpr::List(pt_list) = pt {
                                if pt_list.len() >= 3 && pt_list[0].is_ident("xy") {
                                    let x = get_number(&pt_list[1])?;
                                    let y = get_number(&pt_list[2])?;
                                    bus.points.push((x, y));
                                }
                            }
                        }
                    }
                    "stroke" => {
                        bus.stroke = Self::parse_stroke(sub_list);
                    }
                    _ => {}
                }
            }
        }

        if bus.points.len() >= 2 {
            Some(bus)
        } else {
            None
        }
    }

    fn parse_bus_entry(list: &[SExpr]) -> Option<BusEntry> {
        let mut bus_entry = BusEntry::new((0.0, 0.0), (2.54, 2.54));

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
                    "at" => {
                        let x = sub_list.get(1).and_then(get_number).unwrap_or(0.0);
                        let y = sub_list.get(2).and_then(get_number).unwrap_or(0.0);
                        bus_entry.position = (x, y);
                    }
                    "size" => {
                        let dx = sub_list.get(1).and_then(get_number).unwrap_or(2.54);
                        let dy = sub_list.get(2).and_then(get_number).unwrap_or(2.54);
                        bus_entry.size = (dx, dy);
                    }
                    "stroke" => {
                        bus_entry.stroke = Self::parse_stroke(sub_list);
                    }
                    _ => {}
                }
            }
        }

        Some(bus_entry)
    }
}

// Helper functions
fn get_string_or_ident(expr: &SExpr) -> String {
    match expr {
        SExpr::Atom(crate::parser::ast::Atom::String(s)) => s.clone(),
        SExpr::Atom(crate::parser::ast::Atom::Identifier(s)) => s.clone(),
        SExpr::Atom(crate::parser::ast::Atom::Number(n)) => {
            if (*n - n.round()).abs() < 1e-9 {
                format!("{}", *n as i64)
            } else {
                n.to_string()
            }
        }
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
