//! Declarative graphical constraint-based PCB layout engine.
//!
//! PCB layout is a graphical constraint problem, not a netlist problem.
//! Components are boxes with spatial relationships (like CSS box model).
//! Constraints are declarative rules solved by an iterative solver.

use kicad_json5::ir::board::{DrillDef, PadShape, PadType};
use kicad_json5::ir::{Schematic, SymbolInstance};
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};

use crate::layout_directives::LayoutDirectives;

// ---------------------------------------------------------------------------
// Box Model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct LayoutBox {
    pub id: String,
    pub lib_id: String,
    pub reference: String,
    pub width: f64,
    pub height: f64,
    pub rotation: f64,
    pub margin: f64,
    pub fixed: bool,
    pub tags: HashSet<String>,
    pub nets: HashSet<String>,
    pub anchor_ic: Option<String>,
    pub ic_priority: IcPriority,
    pub pins: Vec<PinInfo>,
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone)]
pub struct PinInfo {
    pub name: String,
    pub dx: f64,
    pub dy: f64,
    pub net: Option<String>,
}

// ---------------------------------------------------------------------------
// Constraints
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Constraint {
    Anchor(AnchorType),
    Near(String, Direction, f64),
    Align(Alignment, Vec<String>),
    MinSpacing(Vec<String>, f64),
    MaxSpacing(Vec<String>, f64),
    Row(Vec<String>, f64),
    Column(Vec<String>, f64),
    InZone(String),
    /// P2-1: signal-flow ordering — components must appear left-to-right in
    /// this order (input connector → series elements → output connector).
    Flow(Vec<String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AnchorType {
    TopEdge,
    BottomEdge,
    LeftEdge,
    RightEdge,
    Center,
}

/// v35（M2）：锚边名解析——top/bottom/left/right/center（大小写不敏感，
/// 可带 Edge 后缀如 "NorthEdge"？不收方位词，只收四边+中心，避免歧义）。
pub fn parse_anchor_type(s: &str) -> Option<AnchorType> {
    let t = s.trim().to_lowercase().trim_end_matches("edge").to_string();
    match t.as_str() {
        "top" => Some(AnchorType::TopEdge),
        "bottom" => Some(AnchorType::BottomEdge),
        "left" => Some(AnchorType::LeftEdge),
        "right" => Some(AnchorType::RightEdge),
        "center" => Some(AnchorType::Center),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Above,
    Below,
    Nearest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum IcPriority {
    GenericIc = 0,
    SupportIc = 1,
    HighSpeedIc = 2,
    MainIc = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alignment {
    Left,
    Right,
    Top,
    Bottom,
    CenterX,
    CenterY,
}

// ---------------------------------------------------------------------------
// Zones
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Zone {
    pub name: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

pub struct BoardZones {
    pub top: Zone,
    pub bottom: Zone,
    pub left: Zone,
    pub right: Zone,
    pub center: Zone,
}

impl BoardZones {
    pub fn new(board_w: f64, board_h: f64, margin: f64) -> Self {
        let strip = 8.0;
        Self {
            top: Zone {
                name: "top".into(),
                x: margin,
                y: margin,
                w: board_w - 2.0 * margin,
                h: strip,
            },
            bottom: Zone {
                name: "bottom".into(),
                x: margin,
                y: board_h - margin - strip,
                w: board_w - 2.0 * margin,
                h: strip,
            },
            left: Zone {
                name: "left".into(),
                x: margin,
                y: margin + strip,
                w: strip,
                h: board_h - 2.0 * margin - 2.0 * strip,
            },
            right: Zone {
                name: "right".into(),
                x: board_w - margin - strip,
                y: margin + strip,
                w: strip,
                h: board_h - 2.0 * margin - 2.0 * strip,
            },
            center: Zone {
                name: "center".into(),
                x: margin + strip,
                y: margin + strip,
                w: board_w - 2.0 * margin - 2.0 * strip,
                h: board_h - 2.0 * margin - 2.0 * strip,
            },
        }
    }

    pub fn get(&self, name: &str) -> &Zone {
        match name {
            "top" => &self.top,
            "bottom" => &self.bottom,
            "left" => &self.left,
            "right" => &self.right,
            _ => &self.center,
        }
    }
}

// ---------------------------------------------------------------------------
// Role Classification
// ---------------------------------------------------------------------------

fn classify_role(comp: &SymbolInstance) -> HashSet<String> {
    let mut tags = HashSet::new();
    let lib = comp.lib_id.to_uppercase();
    let val = comp.value.to_uppercase();
    let ref_prefix = comp.reference.chars().next().unwrap_or('X');

    match ref_prefix {
        'U' | 'Y' => {
            tags.insert("ic".into());
        }
        'J' | 'P' => {
            // Screw/mounting holes are NOT connectors
            if !lib.contains("SCREW")
                && !lib.contains("MOUNTING")
                && !lib.contains("MOUNT_HOLE")
                && !val.contains("MOUNT")
            {
                tags.insert("connector".into());
            }
        }
        _ => {}
    }

    // Mounting hole detection
    if lib.contains("SCREW") || lib.contains("MOUNTING") || lib.contains("MOUNT_HOLE") {
        tags.insert("mounting_hole".into());
    }

    if lib.contains("CRYSTAL") || val.contains("MHZ") {
        tags.insert("crystal".into());
    }
    if lib.contains("CONNECTOR") {
        tags.insert("connector".into());
    }

    // LED
    if lib.contains("LED") {
        tags.insert("led".into());
    }
    // Test point
    if comp.reference.starts_with("TP") {
        tags.insert("test_point".into());
    }
    // Ferrite bead
    if lib.contains("FERRITE") || lib.contains("FB_") {
        tags.insert("ferrite_bead".into());
    }
    // TVS diode
    if lib.contains("TVS") || lib.contains("D_TVS") {
        tags.insert("tvs".into());
    }
    // Op-amp
    if lib.contains("OPAMP")
        || lib.contains("OPA")
        || lib.contains("LM358")
        || lib.contains("TL072")
    {
        tags.insert("opamp".into());
        tags.insert("ic".into());
    }
    // Sensor
    if lib.contains("SENSOR") || lib.contains("NTC") || lib.contains("RTD") || lib.contains("THERM")
    {
        tags.insert("sensor".into());
    }
    // MOSFET
    if lib.contains("MOSFET") || lib.contains("FET") || lib.contains(":Q_") {
        tags.insert("mosfet".into());
        tags.insert("power_passive".into());
    }

    // Check pin nets for more specific roles
    let mut has_power_pin = false;
    let mut has_gnd_pin = false;
    let mut has_fb_pin = false;
    for pin in &comp.pins {
        if let Some(ref net) = pin.net_name {
            let upper = net.to_uppercase();
            if upper == "GND" || upper == "VSS" {
                has_gnd_pin = true;
            }
            if upper.starts_with("VCC") || upper.starts_with("VDD") || upper == "VIN" {
                has_power_pin = true;
            }
            if upper.contains("FB") || upper.contains("COMP") {
                has_fb_pin = true;
            }
        }
    }

    // Decoupling: capacitor with power and GND pins
    if ref_prefix == 'C' && has_power_pin && has_gnd_pin {
        tags.insert("decoupling".into());
    }
    // Feedback: resistor connected to FB net
    if ref_prefix == 'R' && has_fb_pin {
        tags.insert("feedback".into());
    }
    // Pull-up/pull-down: resistor with one pin on power/GND
    if ref_prefix == 'R' && (has_power_pin || has_gnd_pin) && !has_fb_pin {
        tags.insert("pull_resistor".into());
    }

    // Power passives: inductors, diodes (not TVS or LED)
    if (lib.contains("DEVICE:L") || lib.contains("DEVICE:D"))
        && !tags.contains("tvs")
        && !tags.contains("led")
    {
        tags.insert("power_passive".into());
    }

    if tags.is_empty() {
        tags.insert("passive".into());
    }

    tags
}

/// Classify IC priority based on pin count, library ID, and connected nets.
fn classify_ic_priority(comp: &SymbolInstance, tags: &HashSet<String>) -> IcPriority {
    if !tags.contains("ic") {
        return IcPriority::GenericIc;
    }

    let lib = comp.lib_id.to_uppercase();
    let pin_count = comp.pins.len();

    // Main IC: high pin count or known MCU/CPU/SoC/FPGA patterns
    if pin_count >= 48
        || lib.contains("MCU")
        || lib.contains("CPU")
        || lib.contains("MPU")
        || lib.contains("SOC")
        || lib.contains("FPGA")
        || lib.contains("PROCESSOR")
        || lib.contains("STM32")
        || lib.contains("ESP32")
        || lib.contains("RP2040")
        || lib.contains("IMX")
        || lib.contains("CYCLONE")
        || lib.contains("ARTIX")
    {
        return IcPriority::MainIc;
    }

    // High-speed IC: moderate pin count with DDR/SPI/USB/MIPI nets
    let has_hs_net = comp.pins.iter().any(|p| {
        p.net_name.as_ref().is_some_and(|n| {
            let n = n.to_uppercase();
            n.contains("DDR")
                || n.contains("SPI")
                || n.contains("MOSI")
                || n.contains("MISO")
                || n.contains("MIPI")
                || n.contains("USB")
                || n.contains("DATA[")
                || n.contains("CLK")
                || n.contains("HS_")
                || n.contains("DIFF")
                || n.contains("LVDS")
                || n.contains("SDIO")
        })
    });
    if pin_count >= 16 && has_hs_net {
        return IcPriority::HighSpeedIc;
    }

    // Support IC: low pin count (opamps, LDOs, level shifters, etc.)
    if pin_count < 16 {
        return IcPriority::SupportIc;
    }

    IcPriority::GenericIc
}

/// Extract dimensions (width x height in mm) from footprint name patterns.
/// E.g. "QFN-56_7X7" → (7.0, 7.0), "CP_ELEC_6.3X7.7" → (6.3, 7.7)
fn extract_footprint_dims(lib_upper: &str) -> Option<(f64, f64)> {
    // Find pattern "WxH" where W and H are decimal numbers
    let bytes = lib_upper.as_bytes();
    let len = bytes.len();
    for i in 0..len {
        if bytes[i] == b'X' {
            // Try to parse number before 'X' and after 'X'
            let mut w_end = i;
            while w_end > 0 && (bytes[w_end - 1].is_ascii_digit() || bytes[w_end - 1] == b'.') {
                w_end -= 1;
            }
            let w_str = &lib_upper[w_end..i];
            if w_str.is_empty() || w_str == "." {
                continue;
            }

            let mut h_end = i + 1;
            while h_end < len && (bytes[h_end].is_ascii_digit() || bytes[h_end] == b'.') {
                h_end += 1;
            }
            let h_str = &lib_upper[i + 1..h_end];
            if h_str.is_empty() || h_str == "." {
                continue;
            }

            if let (Ok(w), Ok(h)) = (w_str.parse::<f64>(), h_str.parse::<f64>()) {
                if (0.5..=100.0).contains(&w) && (0.5..=100.0).contains(&h) {
                    return Some((w, h));
                }
            }
        }
    }
    None
}

pub fn infer_body_size(lib_id: &str, pin_count: usize) -> (f64, f64) {
    let lib = lib_id.to_uppercase();

    // KAF-09001 CCD (CERDIP-60 leaf-blade variant): two rows of 30 blades,
    // 51.0mm row spacing (body width, spec 51.00±0.51), 1.778mm pitch within
    // a row (span 51.56mm). SMD leaf pads extend OUTWARD — see infer_pad_params.
    if lib.contains("KAF-09001") || lib.contains("KAF09001") || lib.contains("CERDIP-60") {
        return (58.0, 66.0);
    }

    // Electrolytic capacitors: cylindrical, use diameter for both dimensions
    // CP_Elec_DxH → body is circular with diameter D (H is vertical height, irrelevant for layout)
    if lib.contains("CP_ELEC") || lib.contains("CAP_ELEC") {
        if let Some((d, _h)) = extract_footprint_dims(&lib) {
            return (d, d);
        }
        return (6.3, 6.3);
    }

    // Connectors — must be checked BEFORE extract_footprint_dims to avoid
    // NxM pin patterns (like _2X10) being misinterpreted as dimension specs
    if lib.contains("USB") {
        return (9.0, 7.5);
    }
    let is_connector = lib.contains("CONNECTOR")
        || lib.contains("PINHEADER")
        || lib.contains("01X")
        || lib.contains("IDC")
        || (lib.contains("CONN_") && pin_count > 4)
        || lib.contains("FPC_");
    if is_connector {
        let pitch = if lib.contains("P2.00MM") || lib.contains("P2.0MM") {
            2.0
        } else if lib.contains("P1.27MM") {
            1.27
        } else if lib.contains("P1.00MM") {
            1.0
        } else if lib.contains("P0.50MM") {
            0.5
        } else {
            2.54
        };
        // Detect columns: 2xN pattern in various forms
        let cols = if lib.contains("_2X")
            || lib.contains("02X")
            || lib.contains("_2X")
            || lib.ends_with("02X")
        {
            2
        } else {
            1
        };
        let rows = pin_count.div_ceil(cols).max(1);
        // FPC connectors: wide and thin, almost always 0.5mm pitch
        if lib.contains("FPC")
            || lib.contains("FFC")
            || lib.contains("FH12")
            || lib.contains("HIROSE")
        {
            let fpc_pitch = if lib.contains("P1.00MM") { 1.0 } else { 0.5 };
            return (fpc_pitch * pin_count as f64, 3.0);
        }
        return (pitch * cols as f64, pitch * rows as f64);
    }

    // Try to extract dimensions from footprint name (e.g. "QFN-56_7x7", "SOT-23-6")
    if let Some((w, h)) = extract_footprint_dims(&lib) {
        return (w, h);
    }

    // 2-pin passives (0805 default)
    if lib.contains(":R_") || lib.contains(":C_") || lib.contains(":L_") {
        return if lib.contains("0402") {
            (1.0, 0.5)
        } else if lib.contains("0603") {
            (1.6, 0.8)
        } else if lib.contains("1206") {
            (3.2, 1.6)
        } else {
            (2.0, 1.2)
        };
    }

    // Ferrite beads (same as passives)
    if lib.contains("FERRITE") || lib.contains("FB_") {
        return (2.0, 1.2);
    }

    // LEDs and diodes
    if lib.contains(":LED") {
        return (2.0, 1.2);
    }
    if lib.contains(":D") && !lib.contains("D_TVS") {
        return (2.0, 1.2);
    }

    // TVS
    if lib.contains("TVS") || lib.contains("D_TVS") {
        return (2.0, 1.2);
    }

    // Test points
    if lib.contains("TESTPOINT") {
        return (2.0, 2.0);
    }

    // Crystal
    if lib.contains("CRYSTAL") {
        return (4.0, 2.0);
    }

    // TO packages
    if lib.contains("TO-220") {
        return (10.0, 8.0);
    }
    if lib.contains("TO-252") || lib.contains("DPAK") {
        return (6.5, 5.5);
    }
    if lib.contains("TO-263") || lib.contains("D2PAK") {
        return (10.0, 9.0);
    }

    // SOT variants — order matters: most specific first
    if lib.contains("SOT-363") || lib.contains("SOT363") {
        return (2.0, 1.25);
    }
    if lib.contains("SOT-323") || lib.contains("SOT323") {
        return (2.0, 1.25);
    }
    if lib.contains("SOT-23-6") || lib.contains("SOT23-6") {
        return (2.9, 1.6);
    }
    if lib.contains("SOT-23") || lib.contains("SOT23") {
        return (3.0, 1.6);
    }
    if lib.contains("SOT") {
        return (3.0, 2.0);
    }

    // MSOP / TSSOP
    if lib.contains("MSOP") {
        return (3.0, 3.0);
    }
    if lib.contains("TSSOP") {
        let h = 5.0 + pin_count as f64 * 0.15;
        return (4.4, h);
    }

    // SOIC
    if lib.contains("SOIC") {
        return (6.0, 4.0);
    }

    // QFP with pin-count scaling
    if lib.contains("QFP") {
        let side_pins = pin_count.div_ceil(4).max(4);
        let pitch = if pin_count <= 48 { 0.5 } else { 0.4 };
        let side = side_pins as f64 * pitch + 1.0;
        return (side, side);
    }

    // QFN / DFN with pin-count scaling
    if lib.contains("QFN") || lib.contains("DFN") {
        let side_pins = pin_count.div_ceil(4).max(4);
        let pitch = if pin_count <= 32 { 0.5 } else { 0.4 };
        let side = side_pins as f64 * pitch + 0.5;
        return (side, side);
    }

    // BGA
    if lib.contains("BGA") {
        return (10.0, 10.0);
    }

    // DIP / DIP Socket
    if lib.contains("DIP") {
        let cols = 2;
        let rows = pin_count.div_ceil(cols);
        return (2.54 * cols as f64 + 1.0, 2.54 * rows as f64);
    }

    (4.0, 4.0)
}

// ---------------------------------------------------------------------------
// Pin Offsets & Rotation Inference
// ---------------------------------------------------------------------------

/// Build pin offset positions relative to component center.
pub fn build_pin_offsets(
    lib_id: &str,
    pins: &[kicad_json5::ir::PinInstance],
    w: f64,
    h: f64,
) -> Vec<PinInfo> {
    let lib = lib_id.to_uppercase();

    // 2-pin passives: pin1 left, pin2 right
    if pins.len() == 2
        && (lib.contains(":R")
            || lib.contains(":C")
            || lib.contains(":L")
            || lib.contains("FERRITE")
            || lib.contains("LED")
            || lib.contains(":D"))
    {
        return vec![
            PinInfo {
                name: pins[0].number.clone(),
                dx: -w / 2.0,
                dy: 0.0,
                net: pins[0].net_name.clone(),
            },
            PinInfo {
                name: pins[1].number.clone(),
                dx: w / 2.0,
                dy: 0.0,
                net: pins[1].net_name.clone(),
            },
        ];
    }

    // 3-pin SOT: pin1 left, pin2/3 right
    // Official KiCad Package_TO_SOT_SMD pad coordinates (fetched from kicad-footprints):
    //   SOT-23:  pads (-0.9375,-0.95) (-0.9375,0.95) (0.9375,0)
    //   SOT-323: pads (-0.8875,-0.65) (-0.8875,0.65) (0.8875,0)
    // (the old ±w/2,∓h/4 body-size heuristic produced 0.625mm row pitch on
    //  SOT-323 — below the 0.65mm pin pitch, pads nearly touching)
    if pins.len() == 3
        && (lib.contains("SOT-323")
            || lib.contains("SOT323")
            || lib.contains("SOT-353")
            || lib.contains("SC-70"))
    {
        return vec![
            PinInfo {
                name: pins[0].number.clone(),
                dx: -0.8875,
                dy: -0.65,
                net: pins[0].net_name.clone(),
            },
            PinInfo {
                name: pins[1].number.clone(),
                dx: -0.8875,
                dy: 0.65,
                net: pins[1].net_name.clone(),
            },
            PinInfo {
                name: pins[2].number.clone(),
                dx: 0.8875,
                dy: 0.0,
                net: pins[2].net_name.clone(),
            },
        ];
    }
    if pins.len() == 3 && (lib.contains("SOT") || lib.contains("MOSFET") || lib.contains("FET")) {
        return vec![
            PinInfo {
                name: pins[0].number.clone(),
                dx: -0.9375,
                dy: -0.95,
                net: pins[0].net_name.clone(),
            },
            PinInfo {
                name: pins[1].number.clone(),
                dx: -0.9375,
                dy: 0.95,
                net: pins[1].net_name.clone(),
            },
            PinInfo {
                name: pins[2].number.clone(),
                dx: 0.9375,
                dy: 0.0,
                net: pins[2].net_name.clone(),
            },
        ];
    }

    // Through-hole header connectors (Conn_01xNN / Conn_02xNN / PinHeader):
    // pitch grid — the generic 4-side IC distribution put 10-pin headers on a
    // rectangle with 1.69mm pitch (pads nearly touching), and footprint-style
    // lib_ids ("PINHEADER_2X13_P2.54MM_...") fell through entirely (26 pads
    // crammed at 0.73mm along one edge).
    // "_1X" 一并匹配: "PINHEADER_1X08" 不含 "01X"(1 前是 _), 单列排针曾
    // 跌入通用 IC 四边分布 → pad 环形乱布(DRC 短路)。
    if (lib.contains("CONN_") || lib.contains("PINHEADER") || lib.contains("CONNECTOR"))
        && (lib.contains("01X")
            || lib.contains("02X")
            || lib.contains("_1X")
            || lib.contains("_2X"))
    {
        let n = pins.len();
        let cols = if lib.contains("02X") || lib.contains("_2X") {
            2
        } else {
            1
        };
        let rows = n.div_ceil(cols);
        let pitch = parse_named_pitch(&lib).unwrap_or(2.54);
        let cx = (cols as f64 - 1.0) * pitch / 2.0;
        let cy = (rows as f64 - 1.0) * pitch / 2.0;
        let mut result = Vec::with_capacity(n);
        for (i, pin) in pins.iter().enumerate() {
            let col = (i % cols) as f64;
            let row = (i / cols) as f64;
            // Zigzag (row-major): pin1 top-left, pin2 top-right, pin3 next row —
            // matches Conn_02xNN_Odd_Even symbols and KiCad PinHeader_2xNN pads.
            result.push(PinInfo {
                name: pin.number.clone(),
                dx: col * pitch - cx,
                dy: row * pitch - cy,
                net: pin.net_name.clone(),
            });
        }
        return result;
    }

    // KAF-09001 CCD leaf-blade package: 30+30 blades, 51.0mm row spacing,
    // 1.778mm pitch per row. Pin 1-30 down the LEFT column, 31-60 UP the
    // right column (DIP counterclockwise, pin60=VSUB top-right next to pin1).
    if lib.contains("KAF-09001") || lib.contains("KAF09001") || lib.contains("CERDIP-60") {
        let pitch = 1.778;
        let row_span = 29.0 * pitch; // 51.562mm, matches spec span 51.56±0.15
        let col_x = 25.5 + 2.0; // body half-width + pad center offset outward
        let mut result = Vec::with_capacity(pins.len());
        let mut pins_sorted: Vec<kicad_json5::ir::PinInstance> = pins.to_vec();
        pins_sorted.sort_by_key(|p| p.number.parse::<u32>().unwrap_or(0));
        for pin in &pins_sorted {
            let num: u32 = pin.number.parse().unwrap_or(0);
            let (dx, dy) = if (1..=30).contains(&num) {
                (-col_x, -row_span / 2.0 + (num - 1) as f64 * pitch)
            } else {
                (col_x, row_span / 2.0 - (num - 31) as f64 * pitch)
            };
            result.push(PinInfo {
                name: pin.number.clone(),
                dx,
                dy,
                net: pin.net_name.clone(),
            });
        }
        return result;
    }

    // FPC/FFC connectors (Hirose FH12 etc.): single row of fine-pitch pads,
    // pitch from footprint name (P0.50MM default for FH12).
    if lib.contains("FH12") || lib.contains("FPC") || lib.contains("FFC") {
        let pitch = parse_named_pitch(&lib).unwrap_or(0.5);
        let n = pins.len();
        let mut result = Vec::with_capacity(n);
        let mut pins_sorted: Vec<kicad_json5::ir::PinInstance> = pins.to_vec();
        pins_sorted.sort_by_key(|p| p.number.parse::<u32>().unwrap_or(0));
        for (i, pin) in pins_sorted.iter().enumerate() {
            result.push(PinInfo {
                name: pin.number.clone(),
                dx: i as f64 * pitch - (n as f64 - 1.0) * pitch / 2.0,
                dy: 0.0,
                net: pin.net_name.clone(),
            });
        }
        return result;
    }

    // SOT-23-5 / SOT-23-6 family: 3+2 or 3+3 grid, pitch 0.95, row spacing 1.5
    // (pads must not overlap: previous side-distribution put 1.0x0.6 pads at
    //  (0.75, 0.4) offsets which overlap at the corners)
    if lib.contains("SOT-23-5")
        || lib.contains("SOT23-5")
        || lib.contains("SOT-353")
        || lib.contains("SOT23-6")
        || lib.contains("SOT-23-6")
        || lib.contains("SOT-363")
    {
        let per_row = 3;
        let row_y = 0.75;
        let xs = [-0.95f64, 0.0, 0.95];
        let n = pins.len();
        let mut result = Vec::with_capacity(n);
        // Bottom row (pins 1..3 left→right), top row (remaining pins right→left)
        for (i, pin) in pins.iter().enumerate() {
            let (dx, dy) = if i < per_row {
                (xs[i], row_y)
            } else {
                let j = i - per_row;
                (xs[per_row - 1 - j.min(per_row - 1)], -row_y)
            };
            result.push(PinInfo {
                name: pin.number.clone(),
                dx,
                dy,
                net: pin.net_name.clone(),
            });
        }
        return result;
    }

    // Pin slots are assigned by pin NUMBER; the json5 instance pin map may list
    // pins in arbitrary (insertion) order.
    let mut pins_sorted: Vec<kicad_json5::ir::PinInstance> = pins.to_vec();
    if pins_sorted.iter().all(|p| p.number.parse::<u32>().is_ok()) {
        pins_sorted.sort_by_key(|p| p.number.parse::<u32>().unwrap_or(0));
    }
    let pins: &Vec<kicad_json5::ir::PinInstance> = &pins_sorted;

    // BGA: grid layout (rows x cols). Supports lettered balls (A1..K10, I skipped)
    // and numeric pins. Pitch from footprint name (e.g. _P0.65MM_ or _0.65MM_).
    if lib.contains("BGA") && pins.len() >= 4 {
        let pitch = parse_named_pitch(&lib).unwrap_or(1.0);
        let lettered = pins.iter().any(|p| {
            p.number
                .as_bytes()
                .first()
                .map(|c| c.is_ascii_uppercase())
                .unwrap_or(false)
        });
        let n = pins.len();
        let cols = if lettered {
            pins.iter()
                .map(|p| {
                    p.number
                        .chars()
                        .skip(1)
                        .take_while(|c| c.is_ascii_digit())
                        .collect::<String>()
                        .parse::<usize>()
                        .unwrap_or(1)
                })
                .max()
                .unwrap_or(1)
        } else if lib.contains("BGA-80") {
            // VFBGA-80 (e.g. TC358778XBG): 9x9 grid, center ball (E5) depopulated
            // per Toshiba Figure 3.2 — 81-1=80. Pin 1..80 row-major, skipping E5.
            9
        } else {
            let c = (n as f64).sqrt().ceil() as usize;
            c.max(1)
        };
        let rows = if lettered {
            pins.iter()
                .map(|p| letter_row_index(&p.number))
                .max()
                .unwrap_or(1)
                + 1
        } else {
            n.div_ceil(cols)
        };
        let cx = (cols as f64 - 1.0) * pitch / 2.0;
        let cy = (rows as f64 - 1.0) * pitch / 2.0;
        let mut result = Vec::with_capacity(n);
        for pin in pins {
            let (col, row) = if lettered {
                let c: usize = pin
                    .number
                    .chars()
                    .skip(1)
                    .take_while(|c| c.is_ascii_digit())
                    .collect::<String>()
                    .parse()
                    .unwrap_or(1);
                (c.saturating_sub(1), letter_row_index(&pin.number))
            } else {
                let idx: usize = pin.number.parse().unwrap_or(1);
                let mut lin = idx - 1;
                // VFBGA-80: numeric pinout is row-major with E5 depopulated —
                // pins after the hole shift by one grid position
                if !lettered && lib.contains("BGA-80") && lin >= 40 {
                    lin += 1;
                }
                (lin % cols, lin / cols)
            };
            result.push(PinInfo {
                name: pin.number.clone(),
                dx: col as f64 * pitch - cx,
                dy: row as f64 * pitch - cy,
                net: pin.net_name.clone(),
            });
        }
        return result;
    }

    // Quad packages (QFN/QFP) with named pitch: 4 sides at the REAL pitch,
    // pin span centered per side (KiCad QFN/QFP numbering: 1 top-left, down
    // the left side, across the bottom, up the right, across the top).
    // len % 4 == 1 → -1EP part (thermal pad) centered at (0,0).
    if pins.len() >= 4 && pins.len() % 4 <= 1 && (lib.contains("QFN") || lib.contains("QFP")) {
        if let Some(pitch) = parse_named_pitch(&lib) {
            let n = pins.len();
            let per_side = n / 4;
            let span = (per_side as f64 - 1.0) * pitch;
            let mut result = Vec::with_capacity(n);
            for (i, pin) in pins.iter().enumerate() {
                let (dx, dy) = if i >= per_side * 4 {
                    // exposed pad
                    (0.0, 0.0)
                } else if i < per_side {
                    // Left side, top to bottom
                    (-w / 2.0, -span / 2.0 + i as f64 * pitch)
                } else if i < 2 * per_side {
                    // Bottom side, left to right
                    let j = i - per_side;
                    (-span / 2.0 + j as f64 * pitch, h / 2.0)
                } else if i < 3 * per_side {
                    // Right side, bottom to top
                    let j = i - 2 * per_side;
                    (w / 2.0, span / 2.0 - j as f64 * pitch)
                } else {
                    // Top side, right to left
                    let j = i - 3 * per_side;
                    (span / 2.0 - j as f64 * pitch, -h / 2.0)
                };
                result.push(PinInfo {
                    name: pin.number.clone(),
                    dx,
                    dy,
                    net: pin.net_name.clone(),
                });
            }
            return result;
        }
    }

    // Dual-row packages (SOIC/SOP/TSSOP/SSOP/MSOP/DFN): pins 1..n/2 down the
    // left column, n/2+1..n up the right column, real pitch from the name.
    // NOTE: must come after the quad branch - "Package_DFN_QFN:QFN-..." also
    // contains "DFN" and would otherwise be misrouted to the dual layout.
    if pins.len() >= 4
        && pins.len().is_multiple_of(2)
        && (lib.contains("SOIC")
            || lib.contains("SOP")
            || lib.contains("TSSOP")
            || lib.contains("SSOP")
            || lib.contains("MSOP")
            || lib.contains("QSOP")
            || lib.contains("TVSOP")
            || lib.contains("DFN"))
    {
        let n = pins.len();
        let per = n / 2;
        if let Some(pitch) = parse_named_pitch(&lib) {
            let span = (per as f64 - 1.0) * pitch;
            let half_row = w / 2.0;
            let mut result = Vec::with_capacity(n);
            for (i, pin) in pins.iter().enumerate() {
                let (dx, dy) = if i < per {
                    // Left column, top to bottom (pin 1 at top-left)
                    (-half_row, -span / 2.0 + i as f64 * pitch)
                } else {
                    // Right column, bottom to top
                    let j = i - per;
                    (half_row, span / 2.0 - j as f64 * pitch)
                };
                result.push(PinInfo {
                    name: pin.number.clone(),
                    dx,
                    dy,
                    net: pin.net_name.clone(),
                });
            }
            return result;
        }
    }

    // Multi-pin ICs: distribute around 4 sides
    if pins.len() >= 4 {
        let n = pins.len();
        let side_pins = n.div_ceil(4);
        let mut result = Vec::with_capacity(n);
        for (i, pin) in pins.iter().enumerate() {
            let (dx, dy) = if i < side_pins {
                // Left side, top to bottom
                (-w / 2.0, -h / 2.0 + (i as f64 + 0.5) * h / side_pins as f64)
            } else if i < 2 * side_pins {
                // Bottom side, left to right
                let j = i - side_pins;
                (-w / 2.0 + (j as f64 + 0.5) * w / side_pins as f64, h / 2.0)
            } else if i < 3 * side_pins {
                // Right side, bottom to top
                let j = i - 2 * side_pins;
                (w / 2.0, h / 2.0 - (j as f64 + 0.5) * h / side_pins as f64)
            } else {
                // Top side, right to left
                let j = i - 3 * side_pins;
                (
                    w / 2.0 - (j as f64 + 0.5) * w / (n - 3 * side_pins).max(1) as f64,
                    -h / 2.0,
                )
            };
            result.push(PinInfo {
                name: pin.number.clone(),
                dx,
                dy,
                net: pin.net_name.clone(),
            });
        }
        return result;
    }

    // Fallback: generic 2-pin
    pins.iter()
        .enumerate()
        .map(|(i, p)| PinInfo {
            name: p.number.clone(),
            dx: if i == 0 { -w / 2.0 } else { w / 2.0 },
            dy: 0.0,
            net: p.net_name.clone(),
        })
        .collect()
}

/// Parse a pitch value from a footprint/lib name: "..._P0.4MM_..." or "..._0.65MM_...".
/// Requires an explicit MM suffix so body dims like "_7X7MM" don't match.
pub fn parse_named_pitch(lib: &str) -> Option<f64> {
    let b = lib.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let candidate_start =
            if b[i] == b'P' && i + 1 < b.len() && (b[i + 1].is_ascii_digit() || b[i + 1] == b'.') {
                Some(i + 1)
            } else if b[i] == b'_' && i + 1 < b.len() && b[i + 1].is_ascii_digit() {
                Some(i + 1)
            } else {
                None
            };
        if let Some(start) = candidate_start {
            let mut j = start;
            while j < b.len() && (b[j].is_ascii_digit() || b[j] == b'.') {
                j += 1;
            }
            if j + 1 < b.len() && b[j] == b'M' && b[j + 1] == b'M' {
                if let Ok(v) = lib[start..j].parse::<f64>() {
                    if (0.2..=3.0).contains(&v) {
                        return Some(v);
                    }
                }
            }
        }
        i += 1;
    }
    None
}

/// Ball-row letter -> index (0-based), with the I row skipped per JEDEC convention.
fn letter_row_index(pin_name: &str) -> usize {
    let letter = pin_name.chars().next().unwrap_or('A');
    let mut idx = (letter.to_ascii_uppercase() as u8).saturating_sub(b'A') as usize;
    if letter.to_ascii_uppercase() > 'I' {
        idx = idx.saturating_sub(1);
    }
    idx
}

/// Compute absolute pin position given component position and rotation.
fn pin_absolute_pos(pin: &PinInfo, x: f64, y: f64, rotation: f64) -> (f64, f64) {
    let rad = rotation.to_radians();
    let cos_r = rad.cos();
    let sin_r = rad.sin();
    (
        x + pin.dx * cos_r - pin.dy * sin_r,
        y + pin.dx * sin_r + pin.dy * cos_r,
    )
}

/// Infer rotation for all components based on pin connectivity to their anchor ICs.
fn infer_all_rotations(boxes: &mut [LayoutBox]) {
    // Build index: component id → index
    let id_index: HashMap<String, usize> = boxes
        .iter()
        .enumerate()
        .map(|(i, b)| (b.id.clone(), i))
        .collect();

    // For each component with an anchor_ic, try 4 rotations and pick the best
    let anchor_ids: Vec<(usize, String)> = boxes
        .iter()
        .enumerate()
        .filter(|(_, b)| b.anchor_ic.is_some() && !b.fixed && !b.tags.contains("ic"))
        .map(|(i, b)| (i, b.anchor_ic.clone().unwrap()))
        .collect();

    for (comp_idx, anchor_id) in &anchor_ids {
        let anchor_idx = match id_index.get(anchor_id) {
            Some(&idx) => idx,
            None => continue,
        };

        // Find shared nets between component and anchor
        let shared_nets: Vec<String> = {
            let comp = &boxes[*comp_idx];
            let anchor = &boxes[anchor_idx];
            comp.nets.intersection(&anchor.nets).cloned().collect()
        };
        if shared_nets.is_empty() {
            continue;
        }

        // Try 4 rotations, score by pin-to-pin Manhattan distance
        let mut best_rot = 0.0_f64;
        let mut best_score = f64::MAX;

        let anchor = &boxes[anchor_idx];
        let anchor_pins: Vec<&PinInfo> = anchor
            .pins
            .iter()
            .filter(|p| p.net.as_ref().is_some_and(|n| shared_nets.contains(n)))
            .collect();

        for rot_deg in [0.0, 90.0, 180.0, 270.0] {
            let comp = &boxes[*comp_idx];
            let comp_pins: Vec<&PinInfo> = comp
                .pins
                .iter()
                .filter(|p| p.net.as_ref().is_some_and(|n| shared_nets.contains(n)))
                .collect();

            let mut score = 0.0;
            for cp in &comp_pins {
                let (cpx, cpy) = pin_absolute_pos(cp, 0.0, 0.0, rot_deg);
                // Find closest anchor pin on same net
                if let Some(closest) =
                    anchor_pins
                        .iter()
                        .filter(|ap| ap.net == cp.net)
                        .min_by_key(|ap| {
                            let (apx, apy) =
                                pin_absolute_pos(ap, anchor.x, anchor.y, anchor.rotation);
                            let d = ((cpx - apx).abs() + (cpy - apy).abs()) as i64;
                            std::cmp::Reverse(d)
                        })
                {
                    let (apx, apy) = pin_absolute_pos(closest, anchor.x, anchor.y, anchor.rotation);
                    score += (cpx - apx).abs() + (cpy - apy).abs();
                }
            }

            if score < best_score {
                best_score = score;
                best_rot = rot_deg;
            }
        }

        boxes[*comp_idx].rotation = best_rot;
    }
}

// ---------------------------------------------------------------------------
// Constraint Generation
// ---------------------------------------------------------------------------

fn generate_constraints(boxes: &mut [LayoutBox], directives: &LayoutDirectives) -> Vec<Constraint> {
    let mut constraints = Vec::new();

    // Collect IC info as owned data to avoid borrow conflict
    let ics: Vec<IcInfo> = boxes
        .iter()
        .filter(|b| b.tags.contains("ic"))
        .map(|b| IcInfo {
            id: b.id.clone(),
            nets: b.nets.clone(),
            ic_priority: b.ic_priority,
        })
        .collect();
    let connectors: Vec<IcInfo> = boxes
        .iter()
        .filter(|b| b.tags.contains("connector"))
        .map(|b| IcInfo {
            id: b.id.clone(),
            nets: b.nets.clone(),
            ic_priority: b.ic_priority,
        })
        .collect();

    for b in boxes.iter_mut() {
        // Connectors → anchor to board edge with proper spacing
        if b.tags.contains("connector") {
            // v35（M2）：directives 锚定覆盖优先——排线自由度=连接器位置可任选
            let anchor = directives
                .anchor_overrides
                .get(&b.reference)
                .copied()
                .unwrap_or_else(|| infer_connector_edge(&b.nets));
            constraints.push(Constraint::Anchor(anchor));
            b.fixed = true; // Keep connectors on their assigned edge
            b.margin = 3.0; // Extra margin around connectors
        }

        // Decoupling → near its anchor IC (EMC-aware distance)
        if b.tags.contains("decoupling") {
            if let Some(ic) = find_anchor_ic(&b.nets, &ics) {
                b.anchor_ic = Some(ic.id.clone());
                let emc_dist = directives.emc_default_distance();
                constraints.push(Constraint::Near(ic.id.clone(), Direction::Right, emc_dist));
            }
        }

        // Crystal → near anchor IC
        if b.tags.contains("crystal") {
            if let Some(ic) = find_anchor_ic(&b.nets, &ics) {
                b.anchor_ic = Some(ic.id.clone());
                constraints.push(Constraint::Near(ic.id.clone(), Direction::Right, 3.0));
            }
        }

        // Feedback → near anchor IC
        if b.tags.contains("feedback") {
            if let Some(ic) = find_anchor_ic(&b.nets, &ics) {
                b.anchor_ic = Some(ic.id.clone());
                constraints.push(Constraint::Near(ic.id.clone(), Direction::Nearest, 2.0));
            }
        }

        // Power passives → near anchor IC with thermal-aware spacing
        if b.tags.contains("power_passive") {
            if let Some(ic) = find_anchor_ic(&b.nets, &ics) {
                b.anchor_ic = Some(ic.id.clone());
                constraints.push(Constraint::Near(ic.id.clone(), Direction::Nearest, 3.0));
                b.margin = directives.thermal_margin_for(&b.reference, 2.5);
            }
        }

        // Pull resistors → near connected component
        if b.tags.contains("pull_resistor") {
            if let Some(ic) = find_anchor_ic(&b.nets, &ics) {
                b.anchor_ic = Some(ic.id.clone());
                constraints.push(Constraint::Near(ic.id.clone(), Direction::Nearest, 1.5));
            }
        }

        // ICs → center zone with spacing
        if b.tags.contains("ic") {
            constraints.push(Constraint::InZone("center".into()));
        }

        // LED → near anchor IC
        if b.tags.contains("led") {
            if let Some(ic) = find_anchor_ic(&b.nets, &ics) {
                b.anchor_ic = Some(ic.id.clone());
                constraints.push(Constraint::Near(ic.id.clone(), Direction::Nearest, 2.0));
            }
        }

        // Test point → near its connected net's source
        if b.tags.contains("test_point") {
            if let Some(ic) = find_anchor_ic(&b.nets, &ics) {
                b.anchor_ic = Some(ic.id.clone());
                constraints.push(Constraint::Near(ic.id.clone(), Direction::Nearest, 3.0));
            }
        }

        // Ferrite bead → near anchor IC (power input side)
        if b.tags.contains("ferrite_bead") {
            if let Some(ic) = find_anchor_ic(&b.nets, &ics) {
                b.anchor_ic = Some(ic.id.clone());
                constraints.push(Constraint::Near(ic.id.clone(), Direction::Nearest, 3.0));
            }
        }

        // TVS → near connector (protection at interface)
        if b.tags.contains("tvs") {
            if let Some(conn) = find_anchor_ic(&b.nets, &connectors) {
                b.anchor_ic = Some(conn.id.clone());
                constraints.push(Constraint::Near(conn.id.clone(), Direction::Nearest, 2.0));
            } else if let Some(ic) = find_anchor_ic(&b.nets, &ics) {
                b.anchor_ic = Some(ic.id.clone());
                constraints.push(Constraint::Near(ic.id.clone(), Direction::Nearest, 2.0));
            }
        }

        // MOSFET → near driving IC with thermal-aware spacing
        if b.tags.contains("mosfet") {
            if let Some(ic) = find_anchor_ic(&b.nets, &ics) {
                b.anchor_ic = Some(ic.id.clone());
                let mosfet_dist = directives.thermal_margin_for(&b.reference, 4.0);
                constraints.push(Constraint::Near(
                    ic.id.clone(),
                    Direction::Nearest,
                    mosfet_dist,
                ));
            }
        }
    }

    // IC minimum spacing
    let ic_ids: Vec<String> = ics.iter().map(|ic| ic.id.clone()).collect();
    if ic_ids.len() > 1 {
        constraints.push(Constraint::MinSpacing(ic_ids.clone(), 8.0));
    }

    // IC maximum spacing for tightly connected pairs
    if ics.len() >= 2 {
        let ic_nets: Vec<HashSet<String>> = ics
            .iter()
            .map(|ic| {
                ic.nets
                    .iter()
                    .filter(|n| !is_power_ground_net(n))
                    .cloned()
                    .collect()
            })
            .collect();
        for i in 0..ics.len() {
            for j in (i + 1)..ics.len() {
                let shared = ic_nets[i].intersection(&ic_nets[j]).count();
                if shared >= 2 {
                    constraints.push(Constraint::MaxSpacing(
                        vec![ics[i].id.clone(), ics[j].id.clone()],
                        20.0,
                    ));
                }
            }
        }
    }

    // Group components by anchor IC for alignment
    let mut ic_decoupling: HashMap<String, Vec<String>> = HashMap::new();
    let mut ic_pull: HashMap<String, Vec<String>> = HashMap::new();
    for b in boxes.iter() {
        if let Some(ref anchor) = b.anchor_ic {
            if b.tags.contains("decoupling") {
                ic_decoupling
                    .entry(anchor.clone())
                    .or_default()
                    .push(b.id.clone());
            }
            if b.tags.contains("pull_resistor") {
                ic_pull
                    .entry(anchor.clone())
                    .or_default()
                    .push(b.id.clone());
            }
        }
    }

    // Decoupling caps: align vertically, or row if 3+
    let mut sorted_decoupling: Vec<_> = ic_decoupling.into_iter().collect();
    sorted_decoupling.sort_by_key(|(ic_id, _)| ic_id.clone());
    for (ic_id, caps) in sorted_decoupling {
        let cap_spacing = boxes
            .iter()
            .find(|b| b.id == ic_id)
            .map(|ic| (ic.width / (caps.len() as f64 + 1.0)).min(2.54))
            .unwrap_or(2.54);
        if caps.len() >= 3 {
            constraints.push(Constraint::Row(caps, cap_spacing));
        } else if caps.len() == 2 {
            constraints.push(Constraint::Align(Alignment::CenterX, caps));
        }
    }

    // Pull resistors: row
    let mut sorted_pull: Vec<_> = ic_pull.into_iter().collect();
    sorted_pull.sort_by_key(|(ic_id, _)| ic_id.clone());
    for (_ic_id, pulls) in sorted_pull {
        if pulls.len() >= 2 {
            constraints.push(Constraint::Row(pulls.clone(), 2.54));
        }
    }

    // Connectors on same edge: align
    let conn_ids: Vec<String> = boxes
        .iter()
        .filter(|b| b.tags.contains("connector"))
        .map(|b| b.id.clone())
        .collect();
    if conn_ids.len() >= 2 {
        let anchor_counts = constraints
            .iter()
            .filter(|c| matches!(c, Constraint::Anchor(_)))
            .count();
        if anchor_counts > 0 {
            let align = if anchor_counts == 1 {
                Alignment::CenterX
            } else {
                Alignment::CenterX
            };
            constraints.push(Constraint::Align(align, conn_ids));
        }
    }

    // Crystal load caps: align symmetrically around crystal (Phase 4: Clock)
    if !directives.clock.is_empty() {
        let crystal_ids: Vec<usize> = boxes
            .iter()
            .enumerate()
            .filter(|(_, b)| b.tags.contains("crystal"))
            .map(|(i, _)| i)
            .collect();

        for &ci in &crystal_ids {
            let crystal_nets: Vec<String> = boxes[ci].nets.iter().cloned().collect();
            let load_caps: Vec<String> = boxes
                .iter()
                .filter(|b| b.pins.len() == 2 && b.id != boxes[ci].id)
                .filter(|b| b.nets.iter().any(|n| crystal_nets.contains(n)))
                .map(|b| b.id.clone())
                .collect();

            if load_caps.len() == 2 {
                let mut sym_group = load_caps.clone();
                sym_group.push(boxes[ci].id.clone());
                constraints.push(Constraint::Align(Alignment::CenterY, sym_group));
            }
        }
    }

    // P2-1: signal-flow chain — order the power path from the input connector
    // through series elements to the output connector, left to right.
    if let Some(chain) = detect_flow_chain(boxes) {
        eprintln!("[flow] power flow chain: {}", chain.join(" → "));
        constraints.push(Constraint::Flow(chain));
    }

    constraints
}

/// P2-1: detect the main power flow chain via a series-component net graph.
/// Nodes are non-GND nets; a component with ≥2 distinct pad nets creates edges
/// between every pair. Preferred path: input connector → series elements →
/// **main IC** → series elements → output connector (two-leg BFS), falling back
/// to the plain shortest path when no main IC stands out. GND nets are excluded
/// — power flow never routes through ground.
fn detect_flow_chain(boxes: &[LayoutBox]) -> Option<Vec<String>> {
    let is_gnd_net = |n: &str| {
        let u = n.to_uppercase();
        u == "GND"
            || u.ends_with("_GND")
            || u.starts_with("GND_")
            || u == "AGND"
            || u == "DGND"
            || u == "PGND"
            || u == "SGND"
    };
    let is_input_net = |n: &str| {
        let u = n.to_uppercase();
        u.contains("VIN")
            || u.contains("VBAT")
            || u.contains("VBUS")
            || u.contains("INPUT")
            || (u.contains("VCC") && u.contains("IN"))
    };
    let is_output_net = |n: &str| {
        let u = n.to_uppercase();
        u.contains("VOUT")
            || u.contains("5V")
            || u.contains("3V3")
            || u.contains("3.3V")
            || u.contains("12V")
            || u.contains("OUTPUT")
    };
    // P2-1b: power-domain net — voltage-style names only. BFS must stay inside
    // the power topology; letting signal nets (SPI/LVDS/TG/RGB/...) join the
    // queue produced fake chains that linearized the whole placement.
    let is_power_net = |n: &str| {
        let u = n.to_uppercase();
        u.starts_with('+') || u.starts_with('-')
            || u.contains("VBAT") || u.contains("VCI") || u == "VOUT"
            || u.contains("5V") || u.contains("3V3") || u.contains("3V_")
            || u.contains("1V2") || u.contains("1V8") || u.contains("2V8")
            || u.contains("3.3V") || u.contains("12V") || u.contains("1.2V")
            // buck 拓扑桥接网（SW/BST/FB NODE）：非电压命名但是功率路径本体；
            // 信号网（SPI/LVDS/TG/RGB…）无 _NODE 命名，P2-1b 过滤意图不变。
            || u.ends_with("_NODE")
    };

    // P2-1: score-based endpoint pick — a connector may carry both input and
    // output rails (J2 passes VBAT_7V4 through to the CCD board), so a hard
    // "has input ⇒ not output" rule misclassifies. Pick by net-type balance.
    let mut best_in: Option<(i32, i32, String, Vec<String>)> = None;
    let mut best_out: Option<(i32, i32, String, Vec<String>)> = None;
    for b in boxes.iter().filter(|b| b.tags.contains("connector")) {
        let power_nets: Vec<String> = b.nets.iter().filter(|n| !is_gnd_net(n)).cloned().collect();
        let i_hits = power_nets.iter().filter(|n| is_input_net(n)).count() as i32;
        let o_hits = power_nets.iter().filter(|n| is_output_net(n)).count() as i32;
        if i_hits > 0
            && (best_in.is_none()
                || (i_hits - o_hits, i_hits)
                    > (best_in.as_ref().unwrap().0, best_in.as_ref().unwrap().1))
        {
            best_in = Some((i_hits - o_hits, i_hits, b.id.clone(), power_nets.clone()));
        }
        if o_hits > 0
            && (best_out.is_none()
                || (o_hits - i_hits, o_hits)
                    > (best_out.as_ref().unwrap().0, best_out.as_ref().unwrap().1))
        {
            best_out = Some((o_hits - i_hits, o_hits, b.id.clone(), power_nets.clone()));
        }
    }
    let (_, _, in_id, in_nets) = best_in?;
    let (_, _, out_id, out_nets) = best_out?;
    if in_id == out_id {
        return None;
    }

    // component pad-net lists (non-GND only)
    let comp_nets: Vec<(&String, Vec<String>)> = boxes
        .iter()
        .map(|b| {
            let mut nets: Vec<String> = Vec::new();
            for p in &b.pins {
                if let Some(n) = &p.net {
                    if !is_gnd_net(n) && is_power_net(n) && !nets.contains(n) {
                        nets.push(n.clone());
                    }
                }
            }
            (&b.id, nets)
        })
        .collect();

    // BFS over the net graph; expansion goes through one component per hop.
    // Returns (end_net, components in path order).
    let bfs = |starts: &[String],
               targets: &dyn Fn(&str) -> bool,
               blocked: &std::collections::HashSet<String>|
     -> Option<(String, Vec<String>, std::collections::HashSet<String>)> {
        let mut prev: std::collections::HashMap<String, (String, String)> = Default::default();
        let mut visited: std::collections::HashSet<String> = starts.iter().cloned().collect();
        visited.extend(blocked.iter().cloned());
        let mut queue: std::collections::VecDeque<String> = starts.iter().cloned().collect();
        while let Some(cur) = queue.pop_front() {
            for (cid, nets) in &comp_nets {
                if !nets.iter().any(|n| n == &cur) {
                    continue;
                }
                for other in nets {
                    if other == &cur || visited.contains(other) {
                        continue;
                    }
                    visited.insert(other.clone());
                    prev.insert(other.clone(), (cid.to_string(), cur.clone()));
                    if targets(other) {
                        // reconstruct
                        let mut comps: Vec<String> = Vec::new();
                        let mut walk = other.clone();
                        while let Some((cid2, from)) = prev.get(&walk) {
                            if !comps.iter().any(|c| c == cid2) {
                                comps.push(cid2.clone());
                            }
                            if starts.contains(from) {
                                break;
                            }
                            walk = from.clone();
                        }
                        comps.reverse();
                        return Some((other.clone(), comps, visited));
                    }
                    queue.push_back(other.clone());
                }
            }
        }
        None
    };

    // Preferred: route through a conversion IC so the chain doesn't take a
    // shorter bypass branch (battery: U1 buck path, not the Q2 switch branch).
    // Candidates tried in priority order; among valid two-leg chains the one
    // exiting into the most-connected rail wins (5V_BUCK with its caps beats
    // 3V3_STBY), breaking ties by longer path.
    let rail_pads = |net: &str| -> usize {
        comp_nets
            .iter()
            .filter(|(_, nets)| nets.iter().any(|n| n == net))
            .count()
    };
    let mut candidates: Vec<(&String, Vec<String>, usize)> = boxes
        .iter()
        .filter(|b| b.tags.contains("ic") && b.ic_priority as usize > 0)
        .map(|b| {
            let mut v: Vec<String> = b
                .nets
                .iter()
                .filter(|n| !is_gnd_net(n) && is_power_net(n))
                .cloned()
                .collect();
            v.sort();
            (&b.id, v, b.ic_priority as usize)
        })
        .collect();
    candidates.sort_by(|a, b| b.2.cmp(&a.2).then(b.1.len().cmp(&a.1.len())));

    struct ChainPick {
        mid: String,
        comps: Vec<String>,
        exit_pads: usize,
        len: usize,
    }
    let mut best: Option<ChainPick> = None;
    for (ic_id, ic_nets, _) in &candidates {
        let Some((enter_net, c1, visited1)) = bfs(
            &in_nets,
            &|n: &str| ic_nets.iter().any(|m| m == n),
            &std::collections::HashSet::new(),
        ) else {
            continue;
        };
        let starts2: Vec<String> = ic_nets
            .iter()
            .filter(|n| *n != &enter_net)
            .cloned()
            .collect();
        let Some((exit_net, c2, _)) = bfs(
            &starts2,
            &|n: &str| out_nets.iter().any(|m| m == n),
            &visited1,
        ) else {
            continue;
        };
        let path_len = c1.len() + c2.len();
        let cand = ChainPick {
            mid: ic_id.to_string(),
            comps: [c1, c2].concat(),
            exit_pads: rail_pads(&exit_net),
            len: path_len,
        };
        let better = match &best {
            None => true,
            Some(b) => (cand.exit_pads, cand.len) > (b.exit_pads, b.len),
        };
        if better {
            best = Some(cand);
        }
    }

    let (mid_id, comps) = match best {
        Some(p) => (Some(p.mid), p.comps),
        None => {
            // no IC formed a two-leg chain — plain shortest path
            match bfs(
                &in_nets,
                &|n: &str| out_nets.iter().any(|m| m == n),
                &std::collections::HashSet::new(),
            ) {
                Some((_, c, _)) => (None, c),
                None => return None,
            }
        }
    };

    let mut chain = vec![in_id.clone()];
    for c in comps {
        if c != in_id && c != out_id && !chain.contains(&c) {
            chain.push(c);
        }
    }
    if let Some(mid) = mid_id {
        if !chain.contains(&mid) {
            chain.push(mid);
        } else {
            // main IC already on the path — keep natural order
        }
    }
    chain.push(out_id.clone());
    if chain.len() < 3 {
        return None;
    }
    Some(chain)
}

struct IcInfo {
    id: String,
    nets: HashSet<String>,
    ic_priority: IcPriority,
}

fn infer_connector_edge(nets: &HashSet<String>) -> AnchorType {
    // P2-1: same net patterns as the flow detector, scored — a connector can
    // carry both directions (J2 passes VBAT_7V4 through next to 5V/3V3 outs),
    // the dominant side wins the edge.
    let is_input = |u: &str| {
        u.contains("VIN")
            || u.contains("VBAT")
            || u.contains("VBUS")
            || u.contains("INPUT")
            || u.contains("USB")
            || (u.contains("VCC") && u.contains("IN"))
    };
    let is_output = |u: &str| {
        u.contains("VOUT")
            || u.contains("5V")
            || u.contains("3V3")
            || u.contains("3.3V")
            || u.contains("12V")
            || u.contains("OUTPUT")
            || u.contains("TX")
    };
    let mut i_hits = 0i32;
    let mut o_hits = 0i32;
    for net in nets {
        let upper = net.to_uppercase();
        if is_input(&upper) {
            i_hits += 1;
        }
        if is_output(&upper) {
            o_hits += 1;
        }
    }
    if i_hits > 0 && o_hits > 0 {
        if i_hits > o_hits {
            AnchorType::LeftEdge
        } else if o_hits > i_hits {
            AnchorType::RightEdge
        } else {
            AnchorType::TopEdge
        }
    } else if i_hits > 0 {
        AnchorType::LeftEdge
    } else if o_hits > 0 {
        AnchorType::RightEdge
    } else {
        AnchorType::BottomEdge
    }
}

fn find_anchor_ic<'a>(box_nets: &HashSet<String>, ics: &'a [IcInfo]) -> Option<&'a IcInfo> {
    let mut best_ic = None;
    let mut best_score = 0;
    for ic in ics {
        let shared = box_nets.intersection(&ic.nets).count();
        // Composite score: shared nets * 10 + priority weight
        let score = shared * 10 + ic.ic_priority as usize;
        if score > best_score {
            best_score = score;
            best_ic = Some(ic);
        }
    }
    if best_ic.is_none() && !ics.is_empty() {
        best_ic = Some(&ics[0]);
    }
    best_ic
}

// ---------------------------------------------------------------------------
// Sequence Pair Floorplanner + Simulated Annealing
// ---------------------------------------------------------------------------

/// Simple xorshift64 PRNG — avoids adding rand crate dependency.
#[derive(Clone)]
struct Rng {
    state: u64,
}

impl Rng {
    fn new(seed: u64) -> Self {
        Rng {
            state: if seed == 0 { 1 } else { seed },
        }
    }
    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }
    fn next_usize(&mut self, n: usize) -> usize {
        (self.next_u64() as usize) % n
    }
    fn next_f64(&mut self) -> f64 {
        (self.next_u64() as f64) / (u64::MAX as f64)
    }
}

/// Sequence Pair encoding for rectangle packing.
/// Two permutations (pos_seq, neg_seq) encode relative positions of all rectangles.
/// Decode rule: pos[i] before pos[j] AND neg[i] before neg[j] → i left of j.
///              pos[i] before pos[j] AND neg[i] after neg[j] → i above j.
#[derive(Clone)]
struct SequencePair {
    pos_seq: Vec<usize>,
    neg_seq: Vec<usize>,
    rotations: Vec<bool>,
}

impl SequencePair {
    fn new(n: usize) -> Self {
        SequencePair {
            pos_seq: (0..n).collect(),
            neg_seq: (0..n).collect(),
            rotations: vec![false; n],
        }
    }

    /// Build position lookup: element → index in sequence
    fn build_pos_map(seq: &[usize]) -> Vec<usize> {
        let mut m = vec![0usize; seq.len()];
        for (idx, &elem) in seq.iter().enumerate() {
            m[elem] = idx;
        }
        m
    }

    /// Decode to (x, y) positions using O(n²) longest-path on constraint graph.
    /// Each rectangle gets width/height considering rotation.
    fn decode(&self, widths: &[f64], heights: &[f64], spacing: f64) -> Vec<(f64, f64)> {
        let n = self.pos_seq.len();
        let pos_map = Self::build_pos_map(&self.pos_seq);
        let neg_map = Self::build_pos_map(&self.neg_seq);

        // Effective sizes after rotation
        let eff_w: Vec<f64> = (0..n)
            .map(|i| {
                if self.rotations[i] {
                    heights[i]
                } else {
                    widths[i]
                }
            })
            .collect();
        let eff_h: Vec<f64> = (0..n)
            .map(|i| {
                if self.rotations[i] {
                    widths[i]
                } else {
                    heights[i]
                }
            })
            .collect();

        // Horizontal constraint graph (left-of relations):
        // i is left of j iff pos[i] < pos[j] AND neg[i] < neg[j]
        // x[j] >= x[i] + w[i] + spacing
        let mut x = vec![0.0f64; n];
        let pos_order = &self.pos_seq;
        // Process in pos_seq order (topological for horizontal)
        for &j in pos_order.iter() {
            for &i in pos_order.iter() {
                if i == j {
                    continue;
                }
                if pos_map[i] < pos_map[j] && neg_map[i] < neg_map[j] {
                    let candidate = x[i] + eff_w[i] + spacing;
                    if candidate > x[j] {
                        x[j] = candidate;
                    }
                }
            }
        }

        // Vertical constraint graph (above relations):
        // i is above j iff pos[i] < pos[j] AND neg[i] > neg[j]
        let mut y = vec![0.0f64; n];
        for &j in pos_order.iter() {
            for &i in pos_order.iter() {
                if i == j {
                    continue;
                }
                if pos_map[i] < pos_map[j] && neg_map[i] > neg_map[j] {
                    let candidate = y[i] + eff_h[i] + spacing;
                    if candidate > y[j] {
                        y[j] = candidate;
                    }
                }
            }
        }

        (0..n).map(|i| (x[i], y[i])).collect()
    }
}

/// P2: Precomputed net→pin mapping for zero-alloc SA cost evaluation.
struct SaNetIndex {
    /// For each net (by sequential ID): list of (box_idx, pin_idx)
    net_pins: Vec<Vec<(usize, usize)>>,
    /// Number of nets
    num_nets: usize,
    /// anchor_ic pre-resolved: box_idx → Option<anchor_box_idx>
    anchor_map: Vec<Option<usize>>,
}

impl SaNetIndex {
    fn build(boxes: &[LayoutBox]) -> Self {
        // Assign sequential net IDs
        let mut net_id_map: HashMap<String, usize> = HashMap::new();
        let mut next_id = 0usize;
        for b in boxes {
            for pin in &b.pins {
                if let Some(ref net) = pin.net {
                    if !net_id_map.contains_key(net) {
                        net_id_map.insert(net.clone(), next_id);
                        next_id += 1;
                    }
                }
            }
        }
        let num_nets = next_id;

        // Build net → [(box_idx, pin_idx)]
        let mut net_pins: Vec<Vec<(usize, usize)>> = vec![Vec::new(); num_nets];
        for (i, b) in boxes.iter().enumerate() {
            for (pi, pin) in b.pins.iter().enumerate() {
                if let Some(ref net) = pin.net {
                    let &nid = net_id_map.get(net).unwrap();
                    net_pins[nid].push((i, pi));
                }
            }
        }

        // Resolve anchor_ic references
        let ref_index: HashMap<&str, usize> = boxes
            .iter()
            .enumerate()
            .map(|(i, b)| (b.reference.as_str(), i))
            .collect();
        let anchor_map: Vec<Option<usize>> = boxes
            .iter()
            .map(|b| {
                b.anchor_ic
                    .as_ref()
                    .and_then(|a| ref_index.get(a.as_str()).copied())
            })
            .collect();

        SaNetIndex {
            net_pins,
            num_nets,
            anchor_map,
        }
    }
}

/// P2: Zero-allocation cost function for SA inner loop.
/// Uses precomputed net index instead of building HashMap each iteration.
fn evaluate_cost_fast(
    boxes: &[LayoutBox],
    positions: &[(f64, f64)],
    rotations: &[bool],
    widths: &[f64],
    heights: &[f64],
    zones: &BoardZones,
    weights: &crate::layout_directives::SaCostWeights,
    net_idx: &SaNetIndex,
) -> f64 {
    let n = boxes.len();
    let board_w = zones.right.x + zones.right.w;
    let board_h = zones.bottom.y + zones.bottom.h;

    // Precompute effective sizes
    let mut eff_w = [0.0f64; 256];
    let mut eff_h = [0.0f64; 256];
    // For n > 256 fall back to heap — unlikely for PCB layout
    let mut _eff_w_heap: Vec<f64> = Vec::new();
    let mut _eff_h_heap: Vec<f64> = Vec::new();
    let (ew, eh): (&mut [f64], &mut [f64]) = if n <= 256 {
        for i in 0..n {
            eff_w[i] = if rotations[i] { heights[i] } else { widths[i] };
            eff_h[i] = if rotations[i] { widths[i] } else { heights[i] };
        }
        (&mut eff_w[..n], &mut eff_h[..n])
    } else {
        _eff_w_heap = (0..n)
            .map(|i| if rotations[i] { heights[i] } else { widths[i] })
            .collect();
        _eff_h_heap = (0..n)
            .map(|i| if rotations[i] { widths[i] } else { heights[i] })
            .collect();
        (&mut _eff_w_heap, &mut _eff_h_heap)
    };

    // 1. Wire length — iterate precomputed net→pin list
    // Also collect net bboxes for congestion estimation
    let mut wire_len = 0.0;
    let mut net_bboxes: Vec<(f64, f64, f64, f64)> = Vec::with_capacity(net_idx.num_nets);
    // Reusable per-net pin position buffer
    let mut pin_buf: Vec<(f64, f64)> = Vec::with_capacity(32);
    for nid in 0..net_idx.num_nets {
        let pins = &net_idx.net_pins[nid];
        if pins.len() < 2 {
            continue;
        }
        pin_buf.clear();
        for &(bi, pi) in pins {
            let (px, py) = positions[bi];
            let pin = &boxes[bi].pins[pi];
            let rot = if rotations[bi] { 90.0 } else { 0.0 } + boxes[bi].rotation;
            let (ppx, ppy) = pin_absolute_pos(pin, px, py, rot);
            pin_buf.push((ppx, ppy));
        }
        let mut min_x = f64::MAX;
        let mut max_x = f64::MIN;
        let mut min_y = f64::MAX;
        let mut max_y = f64::MIN;
        for &(px, py) in &pin_buf {
            if px < min_x {
                min_x = px;
            }
            if px > max_x {
                max_x = px;
            }
            if py < min_y {
                min_y = py;
            }
            if py > max_y {
                max_y = py;
            }
        }
        wire_len += (max_x - min_x) + (max_y - min_y);
        net_bboxes.push((min_x, min_y, max_x, max_y));
    }

    // 2. Board area
    let mut max_x = 0.0f64;
    let mut max_y = 0.0f64;
    for i in 0..n {
        let right = positions[i].0 + ew[i];
        let bottom = positions[i].1 + eh[i];
        if right > max_x {
            max_x = right;
        }
        if bottom > max_y {
            max_y = bottom;
        }
    }
    let board_area = max_x * max_y;

    // 3. OOB penalty
    let mut oob = 0.0;
    for i in 0..n {
        let (x, y) = positions[i];
        let w = ew[i];
        let h = eh[i];
        if x + w > board_w {
            oob += (x + w - board_w) * 1000.0;
        }
        if y + h > board_h {
            oob += (y + h - board_h) * 1000.0;
        }
        if x < 0.0 {
            oob += (-x) * 1000.0;
        }
        if y < 0.0 {
            oob += (-y) * 1000.0;
        }
    }

    // 4. Anchor penalty
    let mut anchor_pen = 0.0;
    for i in 0..n {
        if let Some(j) = net_idx.anchor_map[i] {
            let dx = positions[i].0 - positions[j].0;
            let dy = positions[i].1 - positions[j].1;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist > 15.0 {
                anchor_pen += (dist - 15.0) * 5.0;
            }
        }
    }

    // 5. Overlap penalty + routing channel congestion
    let mut overlap = 0.0;
    let mut channel_penalty = 0.0;
    let rc_w = weights.routing_congestion;
    for i in 0..n {
        let ix = positions[i].0;
        let iy = positions[i].1;
        let iw = ew[i];
        let ih = eh[i];
        let im = boxes[i].margin;
        for j in (i + 1)..n {
            let jx = positions[j].0;
            let jy = positions[j].1;
            let jw = ew[j];
            let jh = eh[j];
            let jm = boxes[j].margin;
            let min_dx = (iw + jw) / 2.0 + im.max(jm);
            let min_dy = (ih + jh) / 2.0 + im.max(jm);
            let dx = (ix - jx).abs();
            let dy = (iy - jy).abs();
            if dx < min_dx && dy < min_dy {
                overlap += (min_dx - dx) * (min_dy - dy) * weights.overlap;
            }
            // Routing channel: penalize if gap between non-overlapping pairs < 1mm
            if rc_w > 0.0 {
                let gap_x = dx - (iw + jw) / 2.0;
                let gap_y = dy - (ih + jh) / 2.0;
                let min_gap = gap_x.min(gap_y);
                if min_gap < 1.0 && min_gap > -0.5 {
                    // Tight corridor — harder to route between these components
                    let severity = if min_gap < 0.0 { 3.0 } else { 1.0 - min_gap };
                    channel_penalty += severity;
                }
            }
        }
    }

    // 6. Net bbox overlap congestion — penalize nets whose bounding boxes overlap heavily
    let mut congestion_cost = 0.0;
    let nn = net_bboxes.len();
    if nn > 1 && weights.congestion > 0.0 {
        for i in 0..nn {
            let (ix0, iy0, ix1, iy1) = net_bboxes[i];
            for j in (i + 1)..nn {
                let (jx0, jy0, jx1, jy1) = net_bboxes[j];
                // Check bbox intersection
                if ix0 < jx1 && ix1 > jx0 && iy0 < jy1 && iy1 > jy0 {
                    let ow = ix1.min(jx1) - ix0.max(jx0);
                    let oh = iy1.min(jy1) - iy0.max(jy0);
                    congestion_cost += ow * oh;
                }
            }
        }
    }

    wire_len * weights.wire_length
        + board_area * weights.board_area
        + oob
        + anchor_pen
        + overlap
        + congestion_cost * weights.congestion
        + channel_penalty * rc_w
}

/// Perturb a SequencePair for SA neighborhood search.
fn perturb(sp: &SequencePair, rng: &mut Rng) -> SequencePair {
    let n = sp.pos_seq.len();
    let mut new_sp = sp.clone();

    let op = rng.next_usize(4);
    match op {
        0 => {
            // Swap two elements in pos_seq
            let a = rng.next_usize(n);
            let b = rng.next_usize(n);
            new_sp.pos_seq.swap(a, b);
        }
        1 => {
            // Swap two elements in neg_seq
            let a = rng.next_usize(n);
            let b = rng.next_usize(n);
            new_sp.neg_seq.swap(a, b);
        }
        2 => {
            // Toggle rotation of one element
            let a = rng.next_usize(n);
            new_sp.rotations[a] = !new_sp.rotations[a];
        }
        _ => {
            // Swap same pair in both sequences
            let a = rng.next_usize(n);
            let b = rng.next_usize(n);
            new_sp.pos_seq.swap(a, b);
            new_sp.neg_seq.swap(a, b);
        }
    }
    new_sp
}

/// Single-threaded SA run with adaptive cooling (P1) + fast cost evaluation (P2).
/// Returns (best SequencePair, best cost).
/// v35（M2）：把锚定连接器预置到目标边（等距分布 + 长边平行板边）。
/// 旋转时同步交换 width/height，保证 SA 代价模型看到真实占地。
/// 仅 run_sa_seeds 路径使用；主线 solve() 有自己的边放置 pass。
fn place_anchored_connectors(
    boxes: &mut [LayoutBox],
    constraints: &[Constraint],
    bw: f64,
    bh: f64,
) {
    let anchors: Vec<AnchorType> = constraints
        .iter()
        .filter_map(|c| {
            if let Constraint::Anchor(at) = c {
                Some(*at)
            } else {
                None
            }
        })
        .collect();
    let conn_idx: Vec<usize> = boxes
        .iter()
        .enumerate()
        .filter(|(_, b)| b.tags.contains("connector"))
        .map(|(i, _)| i)
        .collect();
    if conn_idx.is_empty() {
        return;
    }
    let gap = 5.0;
    let inset = 3.0;
    // 固定枚举顺序保证确定性（HashMap 迭代序会抖动布局）
    for at in [
        AnchorType::TopEdge,
        AnchorType::BottomEdge,
        AnchorType::LeftEdge,
        AnchorType::RightEdge,
        AnchorType::Center,
    ] {
        let idxs: Vec<usize> = conn_idx
            .iter()
            .copied()
            .enumerate()
            .filter(|(ci, _)| anchors.get(*ci).copied().unwrap_or(AnchorType::BottomEdge) == at)
            .map(|(_, i)| i)
            .collect();
        if idxs.is_empty() {
            continue;
        }
        let horizontal = matches!(at, AnchorType::TopEdge | AnchorType::BottomEdge);
        for &i in &idxs {
            let b = &mut boxes[i];
            let rotated = if horizontal {
                b.height > b.width * 1.2
            } else {
                b.width > b.height * 1.2
            };
            if rotated {
                b.rotation = (b.rotation + 90.0) % 360.0;
                std::mem::swap(&mut b.width, &mut b.height);
            }
        }
        let sizes: Vec<f64> = idxs
            .iter()
            .map(|&i| {
                if horizontal {
                    boxes[i].width
                } else {
                    boxes[i].height
                }
            })
            .collect();
        let total: f64 = sizes.iter().sum::<f64>() + gap * (idxs.len().saturating_sub(1)) as f64;
        let span = if horizontal { bw } else { bh };
        let mut cursor = ((span - total) / 2.0).max(inset);
        for (&i, &sz) in idxs.iter().zip(sizes.iter()) {
            let b = &mut boxes[i];
            let along = cursor + sz / 2.0;
            let perp_half = (if horizontal { b.height } else { b.width }) / 2.0;
            let (x, y) = match at {
                AnchorType::TopEdge => (along, inset + perp_half),
                AnchorType::BottomEdge => (along, bh - inset - perp_half),
                AnchorType::LeftEdge => (inset + perp_half, along),
                AnchorType::RightEdge => (bw - inset - perp_half, along),
                AnchorType::Center => (bw / 2.0, bh / 2.0),
            };
            b.x = x;
            b.y = y;
            cursor += sz + gap;
        }
    }
}

fn sa_floorplan_single(
    boxes: &[LayoutBox],
    widths: &[f64],
    heights: &[f64],
    spacing: f64,
    zones: &BoardZones,
    weights: &crate::layout_directives::SaCostWeights,
    seed: u64,
    thread_id: usize,
    pin_fixed: bool,
) -> (SequencePair, f64) {
    let n = boxes.len();

    // v35（M2）：锚定连接器钉扎——位置取输入 boxes 的预置值，旋转固定，
    // SA 只优化其余件（固定件作为常数参与线长/拥塞代价，产生牵引）。
    // legacy 调用方（sa_floorplan）传 false 保持原行为。
    let pinned: Vec<(usize, f64, f64)> = if pin_fixed {
        boxes
            .iter()
            .enumerate()
            .filter(|(_, b)| b.fixed)
            .map(|(i, b)| (i, b.x, b.y))
            .collect()
    } else {
        Vec::new()
    };
    let pin_positions = |pos: &mut Vec<(f64, f64)>| {
        for &(i, x, y) in &pinned {
            pos[i] = (x, y);
        }
    };
    let pin_rotations = |sp: &mut SequencePair| {
        for &(i, _, _) in &pinned {
            sp.rotations[i] = false;
        }
    };

    // Build initial SP from connectivity order
    let mut sp = SequencePair::new(n);
    pin_rotations(&mut sp);

    // Pre-rotate so long edge aligns with board's long axis
    let board_w = zones.center.x + zones.center.w + zones.right.w + zones.right.x;
    let board_h = zones.center.y + zones.center.h + zones.bottom.h + zones.bottom.y;
    if board_w >= board_h {
        for (i, b) in boxes.iter().enumerate() {
            if b.height > b.width * 1.2 {
                sp.rotations[i] = true;
            }
        }
    } else {
        for (i, b) in boxes.iter().enumerate() {
            if b.width > b.height * 1.2 {
                sp.rotations[i] = true;
            }
        }
    }

    // Connectivity-based initial ordering
    {
        let ic_indices: Vec<usize> = boxes
            .iter()
            .enumerate()
            .filter(|(_, b)| b.tags.contains("ic"))
            .map(|(i, _)| i)
            .collect();
        let ordered = connectivity_order(boxes, &ic_indices);
        let mut sorted = ordered;
        sorted.sort_by(|&a, &b| {
            boxes[b]
                .ic_priority
                .cmp(&boxes[a].ic_priority)
                .then_with(|| a.cmp(&b))
        });
        let mut seq: Vec<usize> = sorted;
        for i in 0..n {
            if !boxes[i].tags.contains("ic") {
                seq.push(i);
            }
        }
        if seq.len() == n {
            sp.pos_seq = seq.clone();
            sp.neg_seq = seq;
        }
    }

    let mut rng = Rng::new(seed);

    // P2: Build precomputed net index once
    let net_idx = SaNetIndex::build(boxes);

    let positions = sp.decode(widths, heights, spacing);
    let mut positions = positions;
    pin_positions(&mut positions);
    let mut current_cost = evaluate_cost_fast(
        boxes,
        &positions,
        &sp.rotations,
        widths,
        heights,
        zones,
        weights,
        &net_idx,
    );

    if thread_id == 0 {
        eprintln!("[SA:{}] init cost={:.2}", thread_id, current_cost);
    }
    let mut best_sp = sp.clone();
    let mut best_cost = current_cost;

    // P1: Adaptive SA parameters
    let t_initial = (current_cost / n as f64).max(1.0);
    let t_final = 0.001;
    let mut cooling: f64;
    let iters = if n < 20 { 5 * n } else { 10 * n };

    let mut temp = t_initial;
    // SA runs to natural convergence (temp ≤ t_final) — deterministic.
    // No step cap: truncating changed layouts and exposed unverified paths.
    // The 2×11s on battery-class boards is not the bottleneck (router is).
    while temp > t_final {
        let mut accepted = 0usize;
        for _ in 0..iters {
            let mut neighbor = perturb(&sp, &mut rng);
            pin_rotations(&mut neighbor);
            let mut new_positions = neighbor.decode(widths, heights, spacing);
            pin_positions(&mut new_positions);
            let new_cost = evaluate_cost_fast(
                boxes,
                &new_positions,
                &neighbor.rotations,
                widths,
                heights,
                zones,
                weights,
                &net_idx,
            );

            let delta = new_cost - current_cost;
            if delta < 0.0 || rng.next_f64() < (-delta / temp).exp() {
                sp = neighbor;
                current_cost = new_cost;
                accepted += 1;

                if current_cost < best_cost {
                    best_sp = sp.clone();
                    best_cost = current_cost;
                }
            }
        }

        let accept_rate = accepted as f64 / iters as f64;

        // P1: Adaptive cooling — accelerate when acceptance is low
        if accept_rate < 0.05 {
            cooling = 0.85;
        } else if accept_rate < 0.15 {
            cooling = if n < 20 { 0.90 } else { 0.93 };
        } else {
            cooling = if n < 20 { 0.95 } else { 0.97 };
        }

        if thread_id == 0 {
            eprintln!(
                "[SA:{}] temp={:.4} cost={:.2} best={:.2} acc={:.0}% cool={:.2}",
                thread_id,
                temp,
                current_cost,
                best_cost,
                accept_rate * 100.0,
                cooling
            );
        }
        temp *= cooling;
    }

    (best_sp, best_cost)
}

/// Simulated Annealing floorplanner — parallel multi-start with adaptive cooling.
/// Runs K independent SA instances via rayon, picks the global best.
fn sa_floorplan(
    boxes: &mut [LayoutBox],
    zones: &BoardZones,
    weights: &crate::layout_directives::SaCostWeights,
) {
    let n = boxes.len();
    if n < 2 {
        return;
    }

    let widths: Vec<f64> = boxes.iter().map(|b| b.width + b.margin * 2.0).collect();
    let heights: Vec<f64> = boxes.iter().map(|b| b.height + b.margin * 2.0).collect();
    let spacing = if n > 80 {
        0.5 + (n as f64 / 25.0).min(3.5)
    } else {
        0.5 + (n as f64 / 30.0).min(2.0)
    };

    // P0: Parallel multi-start SA
    let num_threads = rayon::current_num_threads().clamp(1, 8);
    eprintln!(
        "[SA] starting {} parallel instances ({} components)",
        num_threads, n
    );

    let results: Vec<(SequencePair, f64)> = (0..num_threads)
        .into_par_iter()
        .map(|i| {
            sa_floorplan_single(
                boxes,
                &widths,
                &heights,
                spacing,
                zones,
                weights,
                42 + i as u64 * 7919, // distinct seeds
                i,
                false,
            )
        })
        .collect();

    // Pick global best
    let (best_sp, best_cost) = results
        .into_iter()
        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
        .unwrap();

    eprintln!("[SA] global best cost={:.2}", best_cost);

    // Apply best solution to boxes
    let best_positions = best_sp.decode(&widths, &heights, spacing);
    for (i, b) in boxes.iter_mut().enumerate() {
        b.x = best_positions[i].0;
        b.y = best_positions[i].1;
        if best_sp.rotations[i] {
            b.rotation = (b.rotation + 90.0) % 360.0;
        }
    }
}

// ---------------------------------------------------------------------------
// Solver
// ---------------------------------------------------------------------------

fn solve(boxes: &mut [LayoutBox], constraints: &[Constraint], zones: &BoardZones) {
    // Phase 0: Rotation inference
    infer_all_rotations(boxes);

    // Phase 1: Simulated Annealing floorplanning (Sequence Pair)
    let default_weights = crate::layout_directives::SaCostWeights::default();
    sa_floorplan(boxes, zones, &default_weights);

    // Phase 2: Apply connector anchors to board edges (fixed positions)
    let (mut bw, mut bh) = (
        zones.right.x + zones.right.w,
        zones.bottom.y + zones.bottom.h,
    );
    let mut connector_anchors: Vec<AnchorType> = Vec::new();
    for c in constraints {
        if let Constraint::Anchor(at) = c {
            connector_anchors.push(*at);
        }
    }
    let connectors: Vec<usize> = boxes
        .iter()
        .enumerate()
        .filter(|(_, b)| b.tags.contains("connector"))
        .map(|(i, _)| i)
        .collect();

    let mut edge_groups: HashMap<String, Vec<usize>> = HashMap::new();
    for (ci, &idx) in connectors.iter().enumerate() {
        let at = connector_anchors
            .get(ci)
            .copied()
            .unwrap_or(AnchorType::BottomEdge);
        let edge_key = format!("{:?}", at);
        edge_groups.entry(edge_key).or_default().push(idx);
    }

    // Rebalance: distribute connectors across all 4 edges to avoid overflow
    // Use rotation-aware size: if connector is tall, it gets rotated on horizontal edges
    let conn_gap = 5.0;
    let edge_list = ["TopEdge", "BottomEdge", "LeftEdge", "RightEdge"];

    // Helper: compute connector's size along the edge (accounting for rotation)
    let conn_edge_size = |idx: usize, edge_key: &str| -> f64 {
        let b = &boxes[idx];
        let is_h = edge_key == "TopEdge" || edge_key == "BottomEdge";
        let rotated = if is_h {
            b.height > b.width * 1.2
        } else {
            b.width > b.height * 1.2
        };
        if is_h {
            if rotated {
                b.height
            } else {
                b.width
            }
        } else {
            if rotated {
                b.width
            } else {
                b.height
            }
        }
    };

    // First pass: collect overflow from each edge
    let mut overflow: Vec<usize> = Vec::new();
    for &edge_key in &edge_list {
        let is_h = edge_key == "TopEdge" || edge_key == "BottomEdge";
        let span = if is_h { bw } else { bh };
        // 弹出超额元素直至该边剩余 span 容纳所有焊盘(或边空)
        while let Some(indices) = edge_groups.get(edge_key) {
            if indices.is_empty() {
                break;
            }
            let total: f64 = indices
                .iter()
                .map(|&idx| conn_edge_size(idx, edge_key) + conn_gap)
                .sum::<f64>();
            if total <= span {
                break;
            }
            let overflow_idx = *indices.last().unwrap();
            edge_groups.get_mut(edge_key).unwrap().pop();
            overflow.push(overflow_idx);
        }
    }

    // Second pass: redistribute overflow to least-full edges
    for &idx in &overflow {
        let mut best_edge = "BottomEdge";
        let mut best_fill = f64::MAX;
        for &edge_key in &edge_list {
            let is_h = edge_key == "TopEdge" || edge_key == "BottomEdge";
            let span = if is_h { bw } else { bh };
            let indices = edge_groups.get(edge_key).cloned().unwrap_or_default();
            let total: f64 = indices
                .iter()
                .map(|&i| conn_edge_size(i, edge_key) + conn_gap)
                .sum::<f64>();
            let fill_ratio = total / span;
            if fill_ratio < best_fill {
                best_fill = fill_ratio;
                best_edge = edge_key;
            }
        }
        edge_groups
            .entry(best_edge.to_string())
            .or_default()
            .push(idx);
    }

    // Third pass: if any edge still overflows, expand board to fit
    for &edge_key in &edge_list {
        let is_h = edge_key == "TopEdge" || edge_key == "BottomEdge";
        let indices = edge_groups.get(edge_key).cloned().unwrap_or_default();
        if indices.is_empty() {
            continue;
        }
        let total: f64 = indices
            .iter()
            .map(|&idx| conn_edge_size(idx, edge_key) + conn_gap)
            .sum::<f64>();
        let span = if is_h { bw } else { bh };
        if total > span {
            let extra = (total - span) + 6.0;
            if is_h {
                bw += extra;
            } else {
                bh += extra;
            }
        }
    }

    for edge_key in ["TopEdge", "BottomEdge", "LeftEdge", "RightEdge", "Center"] {
        let indices = match edge_groups.get(edge_key) {
            Some(v) => v,
            None => continue,
        };
        let at = match edge_key {
            "TopEdge" => AnchorType::TopEdge,
            "BottomEdge" => AnchorType::BottomEdge,
            "LeftEdge" => AnchorType::LeftEdge,
            "RightEdge" => AnchorType::RightEdge,
            _ => AnchorType::Center,
        };
        let conn_gap = 5.0;
        let total_size: f64 = match at {
            AnchorType::TopEdge | AnchorType::BottomEdge => {
                indices.iter().map(|&idx| boxes[idx].width + conn_gap).sum()
            }
            AnchorType::LeftEdge | AnchorType::RightEdge => indices
                .iter()
                .map(|&idx| boxes[idx].height + conn_gap)
                .sum(),
            AnchorType::Center => indices.iter().map(|&idx| boxes[idx].width + conn_gap).sum(),
        };
        let (span, is_horizontal) = match at {
            AnchorType::TopEdge | AnchorType::BottomEdge => (bw, true),
            AnchorType::LeftEdge | AnchorType::RightEdge => (bh, false),
            AnchorType::Center => (bw.min(bh), true),
        };
        let start = (span - total_size).max(0.0) / 2.0;
        let mut offset = start + 3.0;
        for &idx in indices.iter() {
            let b = &mut boxes[idx];

            // Rotate connector so long edge is parallel to the board edge
            let rotated = match at {
                AnchorType::TopEdge | AnchorType::BottomEdge => b.height > b.width * 1.2,
                AnchorType::LeftEdge | AnchorType::RightEdge => b.width > b.height * 1.2,
                AnchorType::Center => false,
            };
            if rotated {
                b.rotation = (b.rotation + 90.0) % 360.0;
            }

            let comp_size = if is_horizontal {
                if rotated {
                    b.height + conn_gap
                } else {
                    b.width + conn_gap
                }
            } else {
                if rotated {
                    b.width + conn_gap
                } else {
                    b.height + conn_gap
                }
            };
            let center = offset
                .min(span - comp_size / 2.0 - 3.0)
                .max(comp_size / 2.0 + 3.0);
            match at {
                AnchorType::TopEdge => {
                    b.x = center;
                    b.y = 5.0;
                    b.fixed = true;
                }
                AnchorType::BottomEdge => {
                    b.x = center;
                    b.y = bh - 5.0;
                    b.fixed = true;
                }
                AnchorType::LeftEdge => {
                    b.x = 5.0;
                    b.y = center;
                    b.fixed = true;
                }
                AnchorType::RightEdge => {
                    b.x = bw - 5.0;
                    b.y = center;
                    b.fixed = true;
                }
                AnchorType::Center => {
                    b.x = bw / 2.0;
                    b.y = bh / 2.0;
                }
            }
            offset += comp_size;
        }
    }

    // Phase 3: Apply proximity and alignment constraints
    // First apply non-Near constraints
    for c in constraints.iter() {
        match c {
            Constraint::Align(alignment, ids) => apply_align(boxes, *alignment, ids),
            Constraint::Row(ids, spacing) => apply_row(boxes, ids, *spacing),
            Constraint::Column(ids, spacing) => apply_column(boxes, ids, *spacing),
            _ => {}
        }
    }
    // Group Near constraints by target, rotate directions to avoid stacking
    let near_groups: Vec<(&str, Vec<(Direction, f64)>)> = {
        let mut groups: HashMap<&str, Vec<(Direction, f64)>> = HashMap::new();
        for c in constraints.iter() {
            if let Constraint::Near(target_id, dir, dist) = c {
                groups
                    .entry(target_id.as_str())
                    .or_default()
                    .push((*dir, *dist));
            }
        }
        let mut sorted: Vec<_> = groups.into_iter().collect();
        sorted.sort_by_key(|(id, _)| *id);
        sorted
    };
    let rotation_dirs = [
        Direction::Right,
        Direction::Below,
        Direction::Left,
        Direction::Above,
    ];
    for (target_id, nears) in &near_groups {
        if nears.len() > 6 {
            // For very crowded anchors (>6), use circular distribution
            apply_near_circular(boxes, target_id, nears);
        } else {
            for (i, (dir, dist)) in nears.iter().enumerate() {
                let effective_dir = if nears.len() > 1 && *dir == Direction::Nearest {
                    rotation_dirs[i % rotation_dirs.len()]
                } else {
                    *dir
                };
                // Increase spacing when multiple components anchor to same target
                let effective_dist = if nears.len() > 1 {
                    *dist + (i as f64 * 0.8).max(0.3)
                } else {
                    *dist
                };
                apply_near(boxes, target_id, effective_dir, effective_dist);
            }
        }
    }

    // Phase 4: Final collision resolution (pin-aware, boundary-aware)
    let (board_w, board_h) = (
        zones.right.x + zones.right.w,
        zones.bottom.y + zones.bottom.h,
    );
    for _ in 0..120 {
        if !resolve_collisions_bounded(boxes, board_w, board_h) {
            break;
        }
    }

    // Phase 4.5 (P2-1): enforce Flow chain ordering deterministically. The SA
    // floorplanner optimizes wire length only — signal-flow order must be
    // imposed after connectors are edge-anchored. Middle members are laid out
    // left-to-right in chain order between the two connector endpoints, then
    // y-staggered apart if the reassignment creates overlaps.
    for c in constraints {
        if let Constraint::Flow(ids) = c {
            if ids.len() < 3 {
                continue;
            }
            let in_box = boxes.iter().find(|b| b.id == ids[0]);
            let out_box = boxes.iter().find(|b| b.id == *ids.last().unwrap());
            let (Some(ib), Some(ob)) = (in_box, out_box) else {
                continue;
            };
            let x0 = ib.x + ib.width / 2.0 + ib.margin + 1.0;
            let x1 = ob.x - ob.width / 2.0 - ob.margin - 1.0;
            if x1 - x0 < 4.0 {
                continue;
            }

            let mids: Vec<usize> = ids[1..ids.len() - 1]
                .iter()
                .filter_map(|id| boxes.iter().position(|b| b.id == *id))
                .collect();
            if mids.is_empty() {
                continue;
            }

            // even spread with min gaps; leftover space distributed evenly
            let min_gap = 1.0;
            let total_w: f64 = mids
                .iter()
                .map(|&i| boxes[i].width + 2.0 * boxes[i].margin)
                .sum();
            let avail = ((x1 - x0) - total_w - min_gap * (mids.len() as f64 - 1.0)).max(0.0);
            let step_extra = avail / mids.len() as f64;
            // chain slots never overlap horizontally (≥ width + min_gap), so
            // all members can share one y — the connector centerline — giving
            // a clean horizontal flow line; collisions with outsiders are
            // settled by the pinned pass below.
            let band_y = (ib.y + ob.y) / 2.0;
            let mut x = x0;
            for &i in &mids {
                let half = boxes[i].width / 2.0 + boxes[i].margin;
                boxes[i].x = x + half;
                boxes[i].y = band_y;
                x += boxes[i].width + 2.0 * boxes[i].margin + min_gap + step_extra;
            }
        }
    }

    // Settle collisions with chain members pinned — the resolver only shoves
    // non-fixed boxes, so the imposed ordering survives (P2-1).
    let mut pinned: Vec<(usize, bool)> = Vec::new();
    for c in constraints {
        if let Constraint::Flow(ids) = c {
            for id in &ids[1..ids.len().saturating_sub(1)] {
                if let Some(i) = boxes.iter().position(|b| b.id == *id) {
                    pinned.push((i, boxes[i].fixed));
                    boxes[i].fixed = true;
                }
            }
        }
    }
    for _ in 0..60 {
        if !resolve_collisions_bounded(boxes, board_w, board_h) {
            break;
        }
    }
    for (i, was) in pinned {
        boxes[i].fixed = was;
    }

    // Phase 5: Place test points and mounting holes in free areas along board edges
    place_peripherals(boxes, board_w, board_h);
}

/// Place test points and mounting holes along the board edges in free spots.
fn place_peripherals(boxes: &mut [LayoutBox], board_w: f64, board_h: f64) {
    let margin = 3.0;
    let mut peripheral_indices: Vec<usize> = boxes
        .iter()
        .enumerate()
        .filter(|(_, b)| b.tags.contains("test_point") || b.tags.contains("mounting_hole"))
        .map(|(i, _)| i)
        .collect();
    if peripheral_indices.is_empty() {
        return;
    }

    // Collect occupied rectangles from non-peripheral, non-fixed components
    let occupied: Vec<(f64, f64, f64, f64)> = boxes
        .iter()
        .enumerate()
        .filter(|(i, b)| {
            !peripheral_indices.contains(i)
                && !b.tags.contains("test_point")
                && !b.tags.contains("mounting_hole")
        })
        .map(|(_, b)| {
            let (hw, hh) = pin_aware_half_extent(b);
            (b.x - hw, b.y - hh, b.x + hw, b.y + hh)
        })
        .collect();

    // Collect fixed component rects too
    let fixed_rects: Vec<(f64, f64, f64, f64)> = boxes
        .iter()
        .filter(|b| b.fixed)
        .map(|b| {
            let (hw, hh) = pin_aware_half_extent(b);
            (b.x - hw, b.y - hh, b.x + hw, b.y + hh)
        })
        .collect();

    let all_obs: Vec<(f64, f64, f64, f64)> =
        occupied.iter().chain(fixed_rects.iter()).cloned().collect();

    // Candidate positions along board edges and center
    let mut candidates: Vec<(f64, f64)> = Vec::new();
    let step = 4.0;

    // Bottom edge
    let y = board_h - margin;
    let mut x = margin;
    while x < board_w - margin {
        candidates.push((x, y));
        x += step;
    }
    // Top edge
    let y = margin;
    let mut x = margin;
    while x < board_w - margin {
        candidates.push((x, y));
        x += step;
    }
    // Left edge
    let x = margin;
    let mut y = margin;
    while y < board_h - margin {
        candidates.push((x, y));
        y += step;
    }
    // Right edge
    let x = board_w - margin;
    let mut y = margin;
    while y < board_h - margin {
        candidates.push((x, y));
        y += step;
    }

    // Sort peripheral indices by id for determinism
    peripheral_indices.sort_by_key(|&i| boxes[i].id.clone());

    let clearance = 3.0;
    for &idx in &peripheral_indices {
        let (pw, ph) = (boxes[idx].width / 2.0, boxes[idx].height / 2.0);
        let mut best_pos: Option<(f64, f64)> = None;
        let mut best_score = f64::MAX;

        for &(cx, cy) in &candidates {
            // Check clearance against all occupied rects
            let rect = (
                cx - pw - clearance,
                cy - ph - clearance,
                cx + pw + clearance,
                cy + ph + clearance,
            );
            let mut overlaps = false;
            for &(ox1, oy1, ox2, oy2) in &all_obs {
                if rect.2 > ox1 && ox2 > rect.0 && rect.3 > oy1 && oy2 > rect.1 {
                    overlaps = true;
                    break;
                }
            }
            if overlaps {
                continue;
            }
            // Check clearance against already-placed peripherals
            for &pidx in &peripheral_indices {
                if pidx == idx {
                    continue;
                }
                let (phw, phh) = pin_aware_half_extent(&boxes[pidx]);
                let dx = (cx - boxes[pidx].x).abs();
                let dy = (cy - boxes[pidx].y).abs();
                if dx < pw + phw + clearance && dy < ph + phh + clearance {
                    overlaps = true;
                    break;
                }
            }
            if overlaps {
                continue;
            }

            // Score: prefer edge positions (lower y = top edge, higher y = bottom edge)
            let score = 0.0; // all candidates are already on edges
            if score < best_score {
                best_score = score;
                best_pos = Some((cx, cy));
            }
        }

        if let Some((px, py)) = best_pos {
            boxes[idx].x = px;
            boxes[idx].y = py;
        }
        // If no candidate found, leave at current position (collision resolution will handle it)
    }
}

/// Check if a net name is power/ground (should be excluded from signal connectivity).
fn is_power_ground_net(net: &str) -> bool {
    let upper = net.to_uppercase();
    // Match patterns used in router power net filtering
    upper.contains("GND")
        || upper.contains("VCC")
        || upper.contains("VDD")
        || upper.contains("VSS")
        || upper.starts_with('+')
        || upper.starts_with("-")
        || upper.contains("VIN")
        || upper.contains("VOUT")
        || upper == "3V3"
        || upper == "5V"
        || upper == "1V8"
        || upper == "12V"
        || upper == "3.3V"
        || upper == "5.0V"
        || upper == "1.8V"
}

/// Check if a net name indicates a high-speed signal (weighted 3x in connectivity).
fn is_high_speed_net(net: &str) -> bool {
    let n = net.to_uppercase();
    n.contains("DDR")
        || n.contains("SPI")
        || n.contains("MOSI")
        || n.contains("MISO")
        || n.contains("MIPI")
        || n.contains("USB")
        || n.contains("DATA[")
        || n.contains("DATAA")
        || n.contains("DATAB")
        || n.contains("CLK")
        || n.contains("HS_")
        || n.contains("DIFF")
        || n.contains("LVDS")
        || n.contains("SDIO")
        || n.contains("SDRAM")
}

/// Order ICs by signal connectivity — ICs sharing many signal nets are placed adjacent.
/// High-speed nets are weighted 3x to prioritize high-speed IC proximity.
fn connectivity_order(boxes: &[LayoutBox], ic_indices: &[usize]) -> Vec<usize> {
    if ic_indices.len() <= 2 {
        return ic_indices.to_vec();
    }

    // Collect signal nets (exclude power/ground) for each IC
    let ic_signals: Vec<HashSet<String>> = ic_indices
        .iter()
        .map(|&idx| {
            boxes[idx]
                .nets
                .iter()
                .filter(|n| !is_power_ground_net(n))
                .cloned()
                .collect()
        })
        .collect();

    // Compute weighted shared signal net counts between all pairs
    let n = ic_indices.len();
    let mut shared_count: Vec<Vec<usize>> = vec![vec![0; n]; n];
    for i in 0..n {
        for j in (i + 1)..n {
            let count: usize = ic_signals[i]
                .intersection(&ic_signals[j])
                .map(|net| if is_high_speed_net(net) { 3 } else { 1 })
                .sum();
            shared_count[i][j] = count;
            shared_count[j][i] = count;
        }
    }

    // Greedy nearest-neighbor ordering: start from the most connected pair
    let mut best_start = 0;
    let mut best_start_conn = 0;
    for i in 0..n {
        let total: usize = shared_count[i].iter().sum();
        if total > best_start_conn {
            best_start_conn = total;
            best_start = i;
        }
    }

    let mut visited = vec![false; n];
    let mut order = Vec::with_capacity(n);
    let mut current = best_start;
    visited[current] = true;
    order.push(ic_indices[current]);

    while order.len() < n {
        let mut best_next = None;
        let mut best_conn = 0;
        for j in 0..n {
            if visited[j] {
                continue;
            }
            if shared_count[current][j] > best_conn {
                best_conn = shared_count[current][j];
                best_next = Some(j);
            }
        }
        let next = best_next.unwrap_or_else(|| (0..n).find(|&j| !visited[j]).unwrap());
        visited[next] = true;
        order.push(ic_indices[next]);
        current = next;
    }

    order
}

/// Align a group of components along the specified axis.
fn apply_align(boxes: &mut [LayoutBox], alignment: Alignment, target_ids: &[String]) {
    if target_ids.len() < 2 {
        return;
    }

    let id_set: HashSet<&str> = target_ids.iter().map(|s| s.as_str()).collect();
    let positions: Vec<(usize, f64, f64)> = boxes
        .iter()
        .enumerate()
        .filter(|(_, b)| id_set.contains(b.id.as_str()))
        .map(|(i, b)| (i, b.x, b.y))
        .collect();

    if positions.len() < 2 {
        return;
    }

    let target = match alignment {
        Alignment::CenterX => {
            let avg_x: f64 =
                positions.iter().map(|(_, x, _)| x).sum::<f64>() / positions.len() as f64;
            Some(('x', avg_x))
        }
        Alignment::CenterY => {
            let avg_y: f64 =
                positions.iter().map(|(_, _, y)| y).sum::<f64>() / positions.len() as f64;
            Some(('y', avg_y))
        }
        Alignment::Left => {
            let min_x = positions
                .iter()
                .map(|(_, x, _)| x)
                .copied()
                .fold(f64::MAX, f64::min);
            Some(('x', min_x))
        }
        Alignment::Right => {
            let max_x = positions
                .iter()
                .map(|(_, x, _)| x)
                .copied()
                .fold(f64::MIN, f64::max);
            Some(('x', max_x))
        }
        Alignment::Top => {
            let min_y = positions
                .iter()
                .map(|(_, _, y)| y)
                .copied()
                .fold(f64::MAX, f64::min);
            Some(('y', min_y))
        }
        Alignment::Bottom => {
            let max_y = positions
                .iter()
                .map(|(_, _, y)| y)
                .copied()
                .fold(f64::MIN, f64::max);
            Some(('y', max_y))
        }
    };

    if let Some((axis, val)) = target {
        for (idx, _, _) in &positions {
            if !boxes[*idx].fixed {
                match axis {
                    'x' => boxes[*idx].x = val,
                    _ => boxes[*idx].y = val,
                }
            }
        }
    }
}

/// Place components in a horizontal row with equal spacing.
fn apply_row(boxes: &mut [LayoutBox], target_ids: &[String], spacing: f64) {
    if target_ids.len() < 2 {
        return;
    }

    let id_set: HashSet<&str> = target_ids.iter().map(|s| s.as_str()).collect();
    let indices: Vec<usize> = boxes
        .iter()
        .enumerate()
        .filter(|(_, b)| id_set.contains(b.id.as_str()) && !b.fixed)
        .map(|(i, _)| i)
        .collect();

    if indices.is_empty() {
        return;
    }

    // Use first placed component as anchor
    let anchor_y = boxes[indices[0]].y;
    let anchor_x = boxes[indices[0]].x;
    let total_width = (indices.len() - 1) as f64 * spacing;
    let start_x = anchor_x - total_width / 2.0;

    for (i, &idx) in indices.iter().enumerate() {
        boxes[idx].x = start_x + i as f64 * spacing;
        boxes[idx].y = anchor_y;
    }
}

/// Place components in a vertical column with equal spacing.
fn apply_column(boxes: &mut [LayoutBox], target_ids: &[String], spacing: f64) {
    if target_ids.len() < 2 {
        return;
    }

    let id_set: HashSet<&str> = target_ids.iter().map(|s| s.as_str()).collect();
    let indices: Vec<usize> = boxes
        .iter()
        .enumerate()
        .filter(|(_, b)| id_set.contains(b.id.as_str()) && !b.fixed)
        .map(|(i, _)| i)
        .collect();

    if indices.is_empty() {
        return;
    }

    let anchor_x = boxes[indices[0]].x;
    let anchor_y = boxes[indices[0]].y;
    let total_height = (indices.len() - 1) as f64 * spacing;
    let start_y = anchor_y - total_height / 2.0;

    for (i, &idx) in indices.iter().enumerate() {
        boxes[idx].x = anchor_x;
        boxes[idx].y = start_y + i as f64 * spacing;
    }
}

fn apply_near(boxes: &mut [LayoutBox], target_id: &str, dir: Direction, dist: f64) {
    // Collect target info
    let target_info = boxes
        .iter()
        .find(|b| b.id == target_id)
        .map(|b| (b.x, b.y, b.width, b.height, b.rotation, b.pins.clone()));

    let Some((tx, ty, tw, th, tr, target_pins)) = target_info else {
        return;
    };

    let (ew, eh) = {
        let rot = tr % 360.0;
        if (rot - 90.0).abs() < 1.0 || (rot - 270.0).abs() < 1.0 {
            (th, tw) // swap for 90/270
        } else {
            (tw, th)
        }
    };
    // Add pin overhang to target effective half-sizes
    let mut t_half_w = ew / 2.0;
    let mut t_half_h = eh / 2.0;
    for p in &target_pins {
        t_half_w = t_half_w.max(p.dx.abs() + 0.4);
        t_half_h = t_half_h.max(p.dy.abs() + 0.4);
    }

    for b in boxes.iter_mut() {
        if b.id == target_id {
            continue;
        }
        if b.fixed {
            continue;
        }
        if b.anchor_ic.as_deref() != Some(target_id) {
            continue;
        }

        // Try pin-aware placement first
        if !b.pins.is_empty() && !target_pins.is_empty() {
            if let Some((px, py)) = compute_pin_aware_position(b, tx, ty, &target_pins, dir, dist) {
                b.x = px;
                b.y = py;
                continue;
            }
        }

        // Pin-aware half-extent for this component
        let (bhw, bhh) = pin_aware_half_extent(b);

        // Fallback: center-based placement with pin-aware clearance
        match dir {
            Direction::Right => {
                b.x = tx + t_half_w + bhw + dist;
                b.y = ty;
            }
            Direction::Left => {
                b.x = tx - t_half_w - bhw - dist;
                b.y = ty;
            }
            Direction::Above => {
                b.x = tx;
                b.y = ty - t_half_h - bhh - dist;
            }
            Direction::Below => {
                b.x = tx;
                b.y = ty + t_half_h + bhh + dist;
            }
            Direction::Nearest => {
                let cur_x = b.x;
                let cur_y = b.y;
                let offsets = [
                    (tx + t_half_w + bhw + dist, ty),
                    (tx - t_half_w - bhw - dist, ty),
                    (tx, ty + t_half_h + bhh + dist),
                    (tx, ty - t_half_h - bhh - dist),
                ];
                let best = offsets.iter().min_by(|a, c| {
                    let da = (a.0 - cur_x).abs() + (a.1 - cur_y).abs();
                    let dc = (c.0 - cur_x).abs() + (c.1 - cur_y).abs();
                    da.partial_cmp(&dc).unwrap_or(std::cmp::Ordering::Equal)
                });
                if let Some(&(nx, ny)) = best {
                    b.x = nx;
                    b.y = ny;
                }
            }
        }
    }
}

/// Circular distribution for many components anchored to the same target.
/// Places components evenly around the target at increasing radii.
fn apply_near_circular(boxes: &mut [LayoutBox], target_id: &str, _nears: &[(Direction, f64)]) {
    let target = boxes.iter().find(|b| b.id == target_id);
    let Some(target) = target else { return };
    let (tx, ty) = (target.x, target.y);
    let (t_half_w, t_half_h) = pin_aware_half_extent(target);

    // Collect anchored components sorted by id for determinism
    let mut anchored: Vec<usize> = boxes
        .iter()
        .enumerate()
        .filter(|(_, b)| !b.fixed && b.anchor_ic.as_deref() == Some(target_id))
        .map(|(i, _)| i)
        .collect();
    anchored.sort_by_key(|&i| boxes[i].id.clone());

    let n = anchored.len();
    if n == 0 {
        return;
    }

    // Base radius: target half-diagonal + max component size
    let max_comp: f64 = anchored
        .iter()
        .map(|&idx| boxes[idx].width.max(boxes[idx].height))
        .fold(0.0_f64, f64::max);
    let base_radius = (t_half_w + t_half_h) + max_comp + 2.0;

    let angle_step = 2.0 * std::f64::consts::PI / n as f64;
    let radius_step = max_comp * 0.3; // spiral outward for very large groups

    for (slot, &idx) in anchored.iter().enumerate() {
        let angle = slot as f64 * angle_step;
        let radius = base_radius + (slot as f64 / 8.0).floor() * radius_step;
        boxes[idx].x = tx + radius * angle.cos();
        boxes[idx].y = ty + radius * angle.sin();
    }
}

/// Pin-aware placement: position component so its closest shared-net pin
/// is at `dist` mm from the target's corresponding pin, in the given direction.
fn compute_pin_aware_position(
    component: &LayoutBox,
    target_x: f64,
    target_y: f64,
    target_pins: &[PinInfo],
    dir: Direction,
    dist: f64,
) -> Option<(f64, f64)> {
    // Find shared nets
    let shared_nets: HashSet<String> = component
        .pins
        .iter()
        .filter_map(|p| p.net.clone())
        .filter(|n| target_pins.iter().any(|tp| tp.net.as_deref() == Some(n)))
        .collect();

    if shared_nets.is_empty() {
        return None;
    }

    // Pick the first shared net deterministically (sorted)
    let mut sorted_nets: Vec<&String> = shared_nets.iter().collect();
    sorted_nets.sort();
    let net_name = sorted_nets.into_iter().next()?;
    let comp_pin = component
        .pins
        .iter()
        .find(|p| p.net.as_deref() == Some(net_name))?;
    let tgt_pin = target_pins
        .iter()
        .find(|p| p.net.as_deref() == Some(net_name))?;

    // Target pin absolute position
    let (tpx, tpy) = pin_absolute_pos(tgt_pin, target_x, target_y, 0.0); // target rotation already factored into stored position

    // Component pin offset (already rotated by infer_all_rotations)
    let (cpx, cpy) = (comp_pin.dx, comp_pin.dy);

    // Place component so the pin-to-pin vector matches the direction
    match dir {
        Direction::Right => {
            // Component pin should be to the left of target pin
            Some((tpx - cpx - dist, tpy - cpy))
        }
        Direction::Left => Some((tpx - cpx + dist, tpy - cpy)),
        Direction::Above => Some((tpx - cpx, tpy - cpy + dist)),
        Direction::Below => Some((tpx - cpx, tpy - cpy - dist)),
        Direction::Nearest => {
            // Place at minimum distance: offset along the shortest axis
            let dx = tpx - cpx - component.x;
            let dy = tpy - cpy - component.y;
            if dx.abs() >= dy.abs() {
                Some((
                    tpx - cpx + if component.x <= target_x { -dist } else { dist },
                    tpy - cpy,
                ))
            } else {
                Some((
                    tpx - cpx,
                    tpy - cpy + if component.y <= target_y { -dist } else { dist },
                ))
            }
        }
    }
}

/// Get effective (width, height) accounting for 90/270 rotation.
fn effective_size(b: &LayoutBox) -> (f64, f64) {
    let rot = b.rotation % 360.0;
    if (rot - 90.0).abs() < 1.0 || (rot - 270.0).abs() < 1.0 {
        (b.height, b.width) // swap for 90/270
    } else {
        (b.width, b.height)
    }
}

/// Compute effective half-extent of a component including pin overhang.
/// Returns (half_w, half_h) accounting for rotation and max pin offset.
fn pin_aware_half_extent(b: &LayoutBox) -> (f64, f64) {
    let (bw, bh) = effective_size(b);
    let mut half_w = bw / 2.0;
    let mut half_h = bh / 2.0;
    for p in &b.pins {
        // Pin offsets are in un-rotated frame; effective_size already swaps for 90/270
        half_w = half_w.max(p.dx.abs() + 0.4);
        half_h = half_h.max(p.dy.abs() + 0.4);
    }
    (half_w, half_h)
}

/// Collision resolution with boundary awareness.
/// Prevents components from being pushed outside board bounds.
fn resolve_collisions_bounded(boxes: &mut [LayoutBox], board_w: f64, board_h: f64) -> bool {
    let mut any_collision = false;
    let base_margin = 1.0;
    let extents: Vec<(f64, f64)> = boxes.iter().map(pin_aware_half_extent).collect();

    // Compute max pad half-size per box for pad-aware collision
    let pad_half_sizes: Vec<f64> = boxes
        .iter()
        .map(|b| {
            b.pins
                .iter()
                .map(|p| (p.dx.abs().max(p.dy.abs())).max(b.width.max(b.height) * 0.3))
                .fold(0.0_f64, f64::max)
                .max(0.6) // minimum 0.6mm pad half-size (typical 0402 pad)
        })
        .collect();

    for i in 0..boxes.len() {
        for j in (i + 1)..boxes.len() {
            let (ax, ay, am) = (boxes[i].x, boxes[i].y, boxes[i].margin);
            let (bx, by, bm) = (boxes[j].x, boxes[j].y, boxes[j].margin);
            let (ahw, ahh) = extents[i];
            let (bhw, bhh) = extents[j];

            // Add pad-aware extra margin so pads don't violate DRC
            let pad_extra = (pad_half_sizes[i] + pad_half_sizes[j]) * 0.5;
            let min_dist = ahw + bhw + am.max(bm) + pad_extra;
            let min_dist_y = ahh + bhh + am.max(bm) + pad_extra;

            let dx = (ax - bx).abs();
            let dy = (ay - by).abs();

            if dx < min_dist && dy < min_dist_y {
                any_collision = true;
                let overlap_x = min_dist - dx;
                let overlap_y = min_dist_y - dy;

                // Push along primary axis, fallback to secondary if blocked by boundary
                let (px, py) = if dx / min_dist.max(0.01) < dy / min_dist_y.max(0.01) {
                    let sign = if ax < bx { -1.0 } else { 1.0 };
                    (sign * overlap_x / 2.0 * 1.05, 0.0)
                } else {
                    let sign = if ay < by { -1.0 } else { 1.0 };
                    (0.0, sign * overlap_y / 2.0 * 1.05)
                };

                let clamp = |x: f64, hw: f64, w: f64| -> f64 {
                    let lo = base_margin + hw;
                    let hi = (w - base_margin - hw).max(lo);
                    x.max(lo).min(hi)
                };

                if !boxes[i].fixed {
                    let (new_x, new_y) = (boxes[i].x + px, boxes[i].y + py);
                    let cx = clamp(new_x, extents[i].0, board_w);
                    let cy = clamp(new_y, extents[i].1, board_h);
                    // If primary axis was clamped, try pushing along secondary axis
                    if (cx - new_x).abs() > 0.01 || (cy - new_y).abs() > 0.01 {
                        let (sx, sy) = if px.abs() > 0.01 {
                            let sign = if ay < by { -1.0 } else { 1.0 };
                            (0.0, sign * overlap_y / 2.0 * 1.05)
                        } else {
                            let sign = if ax < bx { -1.0 } else { 1.0 };
                            (sign * overlap_x / 2.0 * 1.05, 0.0)
                        };
                        boxes[i].x = clamp(boxes[i].x + sx, extents[i].0, board_w);
                        boxes[i].y = clamp(boxes[i].y + sy, extents[i].1, board_h);
                    } else {
                        boxes[i].x = cx;
                        boxes[i].y = cy;
                    }
                }
                if !boxes[j].fixed {
                    let (new_x, new_y) = (boxes[j].x - px, boxes[j].y - py);
                    let cx = clamp(new_x, extents[j].0, board_w);
                    let cy = clamp(new_y, extents[j].1, board_h);
                    if (cx - new_x).abs() > 0.01 || (cy - new_y).abs() > 0.01 {
                        let (sx, sy) = if px.abs() > 0.01 {
                            let sign = if ay > by { -1.0 } else { 1.0 };
                            (0.0, sign * overlap_y / 2.0 * 1.05)
                        } else {
                            let sign = if ax > bx { -1.0 } else { 1.0 };
                            (sign * overlap_x / 2.0 * 1.05, 0.0)
                        };
                        boxes[j].x = clamp(boxes[j].x + sx, extents[j].0, board_w);
                        boxes[j].y = clamp(boxes[j].y + sy, extents[j].1, board_h);
                    } else {
                        boxes[j].x = cx;
                        boxes[j].y = cy;
                    }
                }
            }
        }
    }
    any_collision
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Run the full auto-layout engine on a schematic.
/// Returns (component_index, x, y, rotation) for each component.
pub fn auto_layout(schematic: &Schematic) -> Vec<(usize, f64, f64, f64)> {
    if schematic.components.is_empty() {
        return Vec::new();
    }

    // Build boxes from schematic components
    let mut boxes = build_boxes(schematic);

    // Compute board size (density-aware)
    let total_area: f64 = boxes.iter().map(|b| b.width * b.height).sum();
    let density_factor = 1.0 + (boxes.len() as f64 / 50.0).min(2.0);
    let board_side = (total_area * 2.5 * density_factor).sqrt().max(30.0);
    let board_w = board_side * 1.1;
    let board_h = board_side;

    // Generate constraints
    let constraints = generate_constraints(&mut boxes, &LayoutDirectives::default());

    // Build zones
    let zones = BoardZones::new(board_w, board_h, 3.0);

    // Initialize positions (center spread)
    initialize_positions(&mut boxes, &zones);

    // Solve
    solve(&mut boxes, &constraints, &zones);

    // Collect results
    boxes
        .iter()
        .map(|b| (b.index(), b.x, b.y, b.rotation))
        .collect()
}

fn build_boxes(schematic: &Schematic) -> Vec<LayoutBox> {
    schematic
        .components
        .iter()
        .map(|comp| {
            let tags = classify_role(comp);
            let pin_count = comp.pins.len();
            let size_key = comp.footprint.as_deref().unwrap_or(&comp.lib_id);
            let (w, h) = infer_body_size(size_key, pin_count);
            let nets: HashSet<String> = comp
                .pins
                .iter()
                .filter_map(|p| p.net_name.clone())
                .collect();

            let margin = if tags.contains("mosfet") || tags.contains("power_passive") {
                2.5
            } else if tags.contains("ic") {
                1.0
            } else if tags.contains("connector") {
                0.8
            } else {
                0.5
            };

            let pins = build_pin_offsets(&comp.lib_id, &comp.pins, w, h);
            let ic_priority = classify_ic_priority(comp, &tags);

            LayoutBox {
                id: comp.reference.clone(),
                lib_id: comp.lib_id.clone(),
                reference: comp.reference.clone(),
                width: w,
                height: h,
                rotation: 0.0,
                margin,
                fixed: false,
                tags,
                nets,
                anchor_ic: None,
                ic_priority,
                pins,
                x: 0.0,
                y: 0.0,
            }
        })
        .collect()
}

/// Classify component role from a PCB Footprint (mirrors classify_role for SymbolInstance).
fn classify_role_from_footprint(
    fp: &kicad_json5::ir::board::Footprint,
    net_names: &HashSet<String>,
) -> HashSet<String> {
    let mut tags = HashSet::new();
    let lib = fp.lib_id.to_uppercase();
    let ref_prefix = fp.reference.chars().next().unwrap_or('X');

    match ref_prefix {
        'U' | 'Y' => {
            tags.insert("ic".into());
        }
        'J' | 'P' => {
            if !lib.contains("SCREW") && !lib.contains("MOUNTING") && !lib.contains("MOUNT_HOLE") {
                tags.insert("connector".into());
            }
        }
        _ => {}
    }

    if lib.contains("SCREW") || lib.contains("MOUNTING") || lib.contains("MOUNT_HOLE") {
        tags.insert("mounting_hole".into());
    }

    if lib.contains("CRYSTAL") {
        tags.insert("crystal".into());
    }
    if lib.contains("CONNECTOR") {
        tags.insert("connector".into());
    }
    if lib.contains("LED") {
        tags.insert("led".into());
    }
    if fp.reference.starts_with("TP") {
        tags.insert("test_point".into());
    }
    if lib.contains("FERRITE") || lib.contains("FB_") {
        tags.insert("ferrite_bead".into());
    }
    if lib.contains("TVS") || lib.contains("D_TVS") {
        tags.insert("tvs".into());
    }
    if lib.contains("OPAMP")
        || lib.contains("OPA")
        || lib.contains("LM358")
        || lib.contains("TL072")
    {
        tags.insert("opamp".into());
        tags.insert("ic".into());
    }
    if lib.contains("SENSOR") || lib.contains("NTC") || lib.contains("THERM") {
        tags.insert("sensor".into());
    }
    if lib.contains("MOSFET") || lib.contains("FET") || lib.contains(":Q_") {
        tags.insert("mosfet".into());
        tags.insert("power_passive".into());
    }

    let mut has_power = false;
    let mut has_gnd = false;
    let mut has_fb = false;
    for name in net_names {
        let upper = name.to_uppercase();
        if upper == "GND" || upper == "VSS" {
            has_gnd = true;
        }
        if upper.starts_with("VCC") || upper.starts_with("VDD") || upper == "VIN" {
            has_power = true;
        }
        if upper.contains("FB") || upper.contains("COMP") {
            has_fb = true;
        }
    }

    if ref_prefix == 'C' && has_power && has_gnd {
        tags.insert("decoupling".into());
    }
    if ref_prefix == 'R' && has_fb {
        tags.insert("feedback".into());
    }
    if ref_prefix == 'R' && (has_power || has_gnd) && !has_fb {
        tags.insert("pull_resistor".into());
    }
    if (lib.contains("DEVICE:L") || lib.contains("DEVICE:D"))
        && !tags.contains("tvs")
        && !tags.contains("led")
    {
        tags.insert("power_passive".into());
    }
    if tags.is_empty() {
        tags.insert("passive".into());
    }

    tags
}

/// Build LayoutBox list from a PCB Board (for re-layout on existing PCBs).
fn build_boxes_from_board(board: &kicad_json5::ir::board::Board) -> Vec<LayoutBox> {
    let net_id_to_name: HashMap<u32, &str> = board
        .nets
        .iter()
        .filter(|n| !n.name.is_empty())
        .map(|n| (n.id, n.name.as_str()))
        .collect();

    board
        .footprints
        .iter()
        .map(|fp| {
            let pin_count = fp.pads.len();
            let (w, h) = infer_body_size(&fp.lib_id, pin_count);

            let net_names: HashSet<String> = fp
                .pads
                .iter()
                .filter_map(|pad| {
                    pad.net
                        .and_then(|id| net_id_to_name.get(&id).map(|s| s.to_string()))
                })
                .collect();

            let tags = classify_role_from_footprint(fp, &net_names);
            let ic_priority = classify_ic_priority_from_footprint(fp, &tags, &net_names);

            let margin = if tags.contains("mosfet") || tags.contains("power_passive") {
                2.5
            } else if tags.contains("ic") {
                1.0
            } else if tags.contains("connector") {
                0.8
            } else {
                0.5
            };

            let pins: Vec<PinInfo> = fp
                .pads
                .iter()
                .map(|pad| {
                    let net_name = pad
                        .net
                        .and_then(|id| net_id_to_name.get(&id).map(|s| s.to_string()));
                    PinInfo {
                        name: pad.number.clone(),
                        dx: pad.position.0,
                        dy: pad.position.1,
                        net: net_name,
                    }
                })
                .collect();

            LayoutBox {
                id: fp.reference.clone(),
                lib_id: fp.lib_id.clone(),
                reference: fp.reference.clone(),
                width: w,
                height: h,
                rotation: 0.0,
                margin,
                fixed: false,
                tags,
                nets: net_names,
                anchor_ic: None,
                ic_priority,
                pins,
                x: 0.0,
                y: 0.0,
            }
        })
        .collect()
}

/// Classify IC priority from Footprint data.
fn classify_ic_priority_from_footprint(
    fp: &kicad_json5::ir::board::Footprint,
    tags: &HashSet<String>,
    net_names: &HashSet<String>,
) -> IcPriority {
    if !tags.contains("ic") {
        return IcPriority::GenericIc;
    }

    let pin_count = fp.pads.len();
    let lib = fp.lib_id.to_uppercase();

    if pin_count >= 48
        || lib.contains("MCU")
        || lib.contains("STM32")
        || lib.contains("ESP32")
        || lib.contains("FPGA")
        || lib.contains("CPU")
        || lib.contains("SOC")
    {
        return IcPriority::MainIc;
    }

    if pin_count >= 16 {
        if net_names.iter().any(|n| is_high_speed_net(n)) {
            return IcPriority::HighSpeedIc;
        }
        return IcPriority::SupportIc;
    }

    IcPriority::GenericIc
}

/// Auto-layout from a PCB Board (for re-layout on existing .kicad_pcb files).
/// Extract board outline origin (min_x, min_y) from Edge.Cuts graphics.
/// Returns (0, 0) if no Edge.Cuts found.
fn board_outline_origin(board: &kicad_json5::ir::board::Board) -> (f64, f64) {
    use kicad_json5::ir::board::BoardGraphicKind;

    let edge: Vec<_> = board
        .graphics
        .iter()
        .filter(|g| g.layer == "Edge.Cuts")
        .collect();

    if edge.is_empty() {
        return (0.0, 0.0);
    }

    let mut min_x = f64::MAX;
    let mut min_y = f64::MAX;
    for gr in &edge {
        match &gr.kind {
            BoardGraphicKind::Rect { start, end } => {
                min_x = min_x.min(start.0).min(end.0);
                min_y = min_y.min(start.1).min(end.1);
            }
            BoardGraphicKind::Line { start, end } => {
                min_x = min_x.min(start.0).min(end.0);
                min_y = min_y.min(start.1).min(end.1);
            }
            BoardGraphicKind::Circle { center, end: e } => {
                let r = ((e.0 - center.0).powi(2) + (e.1 - center.1).powi(2)).sqrt();
                min_x = min_x.min(center.0 - r);
                min_y = min_y.min(center.1 - r);
            }
            BoardGraphicKind::Poly { points } => {
                for (px, py) in points {
                    min_x = min_x.min(*px);
                    min_y = min_y.min(*py);
                }
            }
            _ => {}
        }
    }
    if min_x < f64::MAX {
        (min_x, min_y)
    } else {
        (0.0, 0.0)
    }
}

/// Compute the half-extent clearance needed for a component based on pin offsets.
/// Returns (half_width, half_height) using max(|dx|,|dy|) for both axes since
/// rotation can swap local x/y offsets into either world direction.
fn pin_clearance(b: &LayoutBox) -> (f64, f64) {
    let mut extent = b.width.max(b.height) / 2.0;
    for p in &b.pins {
        extent = extent.max(p.dx.abs()).max(p.dy.abs());
    }
    let clearance = extent + 0.8;
    (clearance, clearance)
}

/// Extract board dimensions from Edge.Cuts graphics.
/// Returns (width, height). Falls back to estimated size from component area.
fn board_outline_size(board: &kicad_json5::ir::board::Board) -> (f64, f64) {
    use kicad_json5::ir::board::BoardGraphicKind;

    let edge_graphics: Vec<_> = board
        .graphics
        .iter()
        .filter(|g| g.layer == "Edge.Cuts")
        .collect();

    if !edge_graphics.is_empty() {
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
                _ => {}
            }
        }
        if min_x < max_x && min_y < max_y {
            return (max_x - min_x, max_y - min_y);
        }
    }

    // Fallback: estimate from total component area
    let total_area: f64 = board
        .footprints
        .iter()
        .filter_map(|fp| {
            let pads = &fp.pads;
            if pads.is_empty() {
                return None;
            }
            let mut px_min = f64::MAX;
            let mut py_min = f64::MAX;
            let mut px_max = f64::MIN;
            let mut py_max = f64::MIN;
            for p in pads {
                let (sx, sy) = p.size;
                px_min = px_min.min(p.position.0 - sx / 2.0);
                py_min = py_min.min(p.position.1 - sy / 2.0);
                px_max = px_max.max(p.position.0 + sx / 2.0);
                py_max = py_max.max(p.position.1 + sy / 2.0);
            }
            Some((px_max - px_min) * (py_max - py_min))
        })
        .sum();
    let side = (total_area * 2.5).sqrt().max(30.0);
    (side * 1.1, side)
}

pub fn auto_layout_board(
    board: &kicad_json5::ir::board::Board,
) -> HashMap<String, (f64, f64, f64)> {
    if board.footprints.is_empty() {
        return HashMap::new();
    }

    let mut boxes = build_boxes_from_board(board);

    // Use Edge.Cuts dimensions if available, otherwise estimate from component area
    let (board_w, board_h) = board_outline_size(board);
    let total_area: f64 = boxes.iter().map(|b| b.width * b.height).sum();
    eprintln!("[layout] Board size: {:.1}x{:.1}mm, components: {}, total area: {:.0}mm², board area: {:.0}mm²",
        board_w, board_h, boxes.len(), total_area, board_w * board_h);

    let constraints = generate_constraints(&mut boxes, &LayoutDirectives::default());
    let zones = BoardZones::new(board_w, board_h, 3.0);
    initialize_positions(&mut boxes, &zones);
    solve(&mut boxes, &constraints, &zones);

    // Clamp positions so no pin extends beyond board outline.
    // Compute per-component clearance from pin offsets.
    let base_margin = 0.5;
    for b in &mut boxes {
        let (half_w, half_h) = pin_clearance(b);
        let x_min = base_margin + half_w;
        let x_max = (board_w - base_margin - half_w).max(x_min);
        let y_min = base_margin + half_h;
        let y_max = (board_h - base_margin - half_h).max(y_min);
        b.x = b.x.max(x_min).min(x_max);
        b.y = b.y.max(y_min).min(y_max);
    }

    // Offset positions to align with Edge.Cuts origin (not (0,0))
    let (ox, oy) = board_outline_origin(board);

    boxes
        .iter()
        .map(|b| (b.reference.clone(), (b.x + ox, b.y + oy, b.rotation)))
        .collect()
}

impl LayoutBox {
    fn index(&self) -> usize {
        // reference like "R1" → extract number, but we don't store index
        // We'll match by reference when applying
        0 // placeholder, caller maps by reference
    }
}

fn initialize_positions(boxes: &mut [LayoutBox], zones: &BoardZones) {
    let zone = &zones.center;
    let non_fixed: Vec<usize> = boxes
        .iter()
        .enumerate()
        .filter(|(_, b)| !b.fixed && !b.tags.contains("ic"))
        .map(|(i, _)| i)
        .collect();

    let count = non_fixed.len();
    if count == 0 {
        return;
    }

    let cols = (count as f64).sqrt().ceil() as usize;
    let x_spacing = zone.w / (cols + 1) as f64;
    let y_spacing = zone.h / (count.div_ceil(cols) + 1) as f64;

    for (i, &idx) in non_fixed.iter().enumerate() {
        let col = i % cols;
        let row = i / cols;
        boxes[idx].x = zone.x + (col + 1) as f64 * x_spacing;
        boxes[idx].y = zone.y + (row + 1) as f64 * y_spacing;
    }
}

/// Public wrapper: auto_layout returns positions keyed by reference.
pub fn auto_layout_refs(schematic: &Schematic) -> HashMap<String, (f64, f64, f64)> {
    auto_layout_refs_with_directives(schematic, &LayoutDirectives::default())
}

/// Auto-layout with design rule directives for thermal, EMC, and SI-aware placement.
/// If `fixed_board_size` is provided, the board outline is locked to those dimensions.
pub fn auto_layout_refs_with_directives(
    schematic: &Schematic,
    directives: &LayoutDirectives,
) -> HashMap<String, (f64, f64, f64)> {
    auto_layout_refs_with_directives_fixed(schematic, directives, None)
}

/// Auto-layout with optional fixed board dimensions.
pub fn auto_layout_refs_with_directives_fixed(
    schematic: &Schematic,
    directives: &LayoutDirectives,
    fixed_board_size: Option<(f64, f64)>,
) -> HashMap<String, (f64, f64, f64)> {
    if schematic.components.is_empty() {
        return HashMap::new();
    }

    let mut boxes = build_boxes(schematic);

    let total_area: f64 = boxes.iter().map(|b| b.width * b.height).sum();
    let (mut board_w, mut board_h) = if let Some((fw, fh)) = fixed_board_size {
        eprintln!("[layout] Fixed board: {:.1}x{:.1}mm", fw, fh);
        (fw, fh)
    } else {
        let density_factor = 1.0 + (boxes.len() as f64 / 40.0).min(2.5);
        let board_side = (total_area * 2.5 * density_factor).sqrt().max(30.0);
        (board_side * 1.4, board_side)
    };

    // Generate constraints early to know connector edge assignments
    let constraints = generate_constraints(&mut boxes, directives);

    // Ensure board is large enough for all connectors on each edge
    let conn_gap = 5.0;
    let connector_indices: Vec<usize> = boxes
        .iter()
        .enumerate()
        .filter(|(_, b)| b.tags.contains("connector"))
        .map(|(i, _)| i)
        .collect();
    let connector_anchors: Vec<AnchorType> = constraints
        .iter()
        .filter_map(|c| {
            if let Constraint::Anchor(at) = c {
                Some(*at)
            } else {
                None
            }
        })
        .collect();

    // Compute total connector width per edge
    let mut edge_total: HashMap<String, f64> = HashMap::new();
    for (ci, &idx) in connector_indices.iter().enumerate() {
        let at = connector_anchors
            .get(ci)
            .copied()
            .unwrap_or(AnchorType::BottomEdge);
        let key = format!("{:?}", at);
        let size = boxes[idx].width.max(boxes[idx].height) + conn_gap;
        *edge_total.entry(key).or_default() += size;
    }
    // Expand board if connectors overflow any edge.
    // Fixed-size mode: user owns the floorplan (post-processing places
    // connectors on exact edges), so never silently grow the board.
    let is_fixed = fixed_board_size.is_some();
    if !is_fixed {
        let needed_w = edge_total
            .get("TopEdge")
            .or(edge_total.get("BottomEdge"))
            .map(|&t| t + 10.0)
            .unwrap_or(board_w)
            .max(board_w);
        let needed_h = edge_total
            .get("LeftEdge")
            .or(edge_total.get("RightEdge"))
            .map(|&t| t + 10.0)
            .unwrap_or(board_h)
            .max(board_h);
        board_w = board_w.max(needed_w);
        board_h = board_h.max(needed_h);
        // Also check summed connector heights/widths as before
        let max_conn_dim: f64 = connector_indices
            .iter()
            .map(|&idx| boxes[idx].width.max(boxes[idx].height) + 2.0)
            .sum();
        if max_conn_dim > board_h * 0.8 {
            board_h = max_conn_dim * 1.3;
        }
        if max_conn_dim > board_w * 0.8 {
            board_w = max_conn_dim * 1.3;
        }
    }

    let zones = BoardZones::new(board_w, board_h, 3.0);
    initialize_positions(&mut boxes, &zones);
    solve(&mut boxes, &constraints, &zones);

    // Clamp positions so no pin extends beyond board bounds
    let base_margin = 0.5;
    for b in &mut boxes {
        let (half_w, half_h) = pin_clearance(b);
        let x_min = base_margin + half_w;
        let x_max = (board_w - base_margin - half_w).max(x_min);
        let y_min = base_margin + half_h;
        let y_max = (board_h - base_margin - half_h).max(y_min);
        b.x = b.x.max(x_min).min(x_max);
        b.y = b.y.max(y_min).min(y_max);
    }

    boxes
        .iter()
        .map(|b| (b.reference.clone(), (b.x, b.y, b.rotation)))
        .collect()
}

// ---------------------------------------------------------------------------
// Layout Quality Scoring
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct LayoutScore {
    pub total: f64,
    pub constraint_satisfaction: f64,
    pub wire_length: f64,
    pub alignment_quality: f64,
    pub overlap_count: usize,
    pub density: f64,
}

/// Evaluate layout quality. Higher total = better layout (0-100).
pub fn score_layout(
    boxes: &[LayoutBox],
    constraints: &[Constraint],
    zones: &BoardZones,
) -> LayoutScore {
    let n = boxes.len();
    if n == 0 {
        return LayoutScore {
            total: 100.0,
            constraint_satisfaction: 100.0,
            wire_length: 0.0,
            alignment_quality: 100.0,
            overlap_count: 0,
            density: 0.0,
        };
    }

    // 1. Constraint satisfaction (0-100). P2-1: each Flow hop scores
    // independently so SA gets a gradient proportional to how many hops are
    // out of order, plus a direct penalty term below.
    let mut total_constraints = constraints.len();
    let mut satisfied = 0;
    let mut flow_violations = 0usize;
    for c in constraints {
        match c {
            Constraint::Flow(ids) => {
                for w in ids.windows(2) {
                    total_constraints += 1;
                    let ax = boxes.iter().find(|b| b.id == w[0]).map(|b| b.x);
                    let bx = boxes.iter().find(|b| b.id == w[1]).map(|b| b.x);
                    match (ax, bx) {
                        (Some(a), Some(b)) if a >= b => flow_violations += 1,
                        _ => satisfied += 1,
                    }
                }
            }
            _ => {
                if check_constraint(boxes, c) {
                    satisfied += 1;
                }
            }
        }
    }
    let total_constraints = total_constraints.max(1);
    let constraint_pct = satisfied as f64 / total_constraints as f64 * 100.0;

    // 2. Wire length estimate
    let wire_len = compute_wire_length(boxes);

    // 3. Overlap count
    let overlaps = count_overlaps(boxes);

    // 4. Alignment quality: variance of positions for grouped components
    let align_quality = compute_alignment_quality(boxes);

    // 5. Density
    let comp_area: f64 = boxes.iter().map(|b| b.width * b.height).sum();
    let board_area = zones.center.w * zones.center.h * 4.0; // rough board area
    let density = if board_area > 0.0 {
        comp_area / board_area * 100.0
    } else {
        0.0
    };

    // Composite score
    let overlap_penalty = overlaps as f64 * 10.0;
    let wire_penalty = wire_len * 0.1;
    // P2-1: 3 points per out-of-order hop — moving a series element ~7mm costs
    // ~0.7 wire points, so fixing the ordering always wins.
    let flow_penalty = flow_violations as f64 * 3.0;
    let total =
        (constraint_pct * 0.4 + align_quality * 0.3 + (100.0 - density.min(100.0)) * 0.1 + 20.0)
            - overlap_penalty
            - wire_penalty
            - flow_penalty;
    let total = total.clamp(0.0, 100.0);

    LayoutScore {
        total,
        constraint_satisfaction: constraint_pct,
        wire_length: wire_len,
        alignment_quality: align_quality,
        overlap_count: overlaps,
        density,
    }
}

fn check_constraint(boxes: &[LayoutBox], c: &Constraint) -> bool {
    match c {
        Constraint::Anchor(at) => {
            // Check if any connector is near the expected edge
            boxes
                .iter()
                .filter(|b| b.tags.contains("connector"))
                .any(|b| match at {
                    AnchorType::TopEdge => b.y < 10.0,
                    AnchorType::BottomEdge => b.y > 40.0,
                    AnchorType::LeftEdge => b.x < 10.0,
                    AnchorType::RightEdge => b.x > 40.0,
                    AnchorType::Center => true,
                })
        }
        Constraint::InZone(zone_name) => {
            // Check if ICs are in center-ish area
            let _ = zone_name;
            true // InZone is always applied in Phase 1
        }
        Constraint::Flow(ids) => {
            // every consecutive pair must be ordered left-to-right
            for w in ids.windows(2) {
                let ax = boxes.iter().find(|b| b.id == w[0]).map(|b| b.x);
                let bx = boxes.iter().find(|b| b.id == w[1]).map(|b| b.x);
                match (ax, bx) {
                    (Some(a), Some(b)) if a >= b => return false,
                    _ => {}
                }
            }
            true
        }
        Constraint::Near(target_id, _dir, dist) => {
            let target = boxes.iter().find(|b| b.id == *target_id);
            if let Some(t) = target {
                boxes
                    .iter()
                    .filter(|b| b.anchor_ic.as_deref() == Some(target_id))
                    .all(|b| {
                        let d = ((b.x - t.x).powi(2) + (b.y - t.y).powi(2)).sqrt();
                        d < dist * 3.0 // tolerance
                    })
            } else {
                true
            }
        }
        Constraint::Align(alignment, ids) => {
            let positions: Vec<(f64, f64)> = boxes
                .iter()
                .filter(|b| ids.contains(&b.id))
                .map(|b| (b.x, b.y))
                .collect();
            if positions.len() < 2 {
                return true;
            }
            let tolerance = 2.0;
            match alignment {
                Alignment::CenterX | Alignment::Left | Alignment::Right => {
                    let avg_x =
                        positions.iter().map(|(x, _)| x).sum::<f64>() / positions.len() as f64;
                    positions.iter().all(|(x, _)| (x - avg_x).abs() < tolerance)
                }
                Alignment::CenterY | Alignment::Top | Alignment::Bottom => {
                    let avg_y =
                        positions.iter().map(|(_, y)| y).sum::<f64>() / positions.len() as f64;
                    positions.iter().all(|(_, y)| (y - avg_y).abs() < tolerance)
                }
            }
        }
        Constraint::MinSpacing(ids, min_dist) => {
            let positions: Vec<(f64, f64)> = boxes
                .iter()
                .filter(|b| ids.contains(&b.id))
                .map(|b| (b.x, b.y))
                .collect();
            positions.iter().enumerate().all(|(i, (ax, ay))| {
                positions
                    .iter()
                    .enumerate()
                    .filter(|(j, _)| *j != i)
                    .all(|(_, (bx, by))| {
                        ((ax - bx).powi(2) + (ay - by).powi(2)).sqrt() >= *min_dist * 0.8
                    })
            })
        }
        Constraint::Row(ids, spacing) => {
            let positions: Vec<f64> = boxes
                .iter()
                .filter(|b| ids.contains(&b.id))
                .map(|b| b.x)
                .collect();
            if positions.len() < 2 {
                return true;
            }
            let mut sorted = positions.clone();
            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
            sorted
                .windows(2)
                .all(|w| (w[1] - w[0] - spacing).abs() < 3.0)
        }
        Constraint::Column(ids, spacing) => {
            let positions: Vec<f64> = boxes
                .iter()
                .filter(|b| ids.contains(&b.id))
                .map(|b| b.y)
                .collect();
            if positions.len() < 2 {
                return true;
            }
            let mut sorted = positions.clone();
            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
            sorted
                .windows(2)
                .all(|w| (w[1] - w[0] - spacing).abs() < 3.0)
        }
        Constraint::MaxSpacing(ids, max_dist) => {
            let positions: Vec<(f64, f64)> = boxes
                .iter()
                .filter(|b| ids.contains(&b.id))
                .map(|b| (b.x, b.y))
                .collect();
            if positions.len() < 2 {
                return true;
            }
            let (ax, ay) = positions[0];
            let (bx, by) = positions[1];
            ((ax - bx).powi(2) + (ay - by).powi(2)).sqrt() <= *max_dist * 1.2
        }
    }
}

/// Estimate total wire length using pin positions (Manhattan distance).
fn compute_wire_length(boxes: &[LayoutBox]) -> f64 {
    // Build net → list of (box_idx, pin_idx) index
    let mut net_pins: HashMap<String, Vec<(usize, usize)>> = HashMap::new();
    for (bi, b) in boxes.iter().enumerate() {
        for (pi, pin) in b.pins.iter().enumerate() {
            if let Some(ref net) = pin.net {
                net_pins.entry(net.clone()).or_default().push((bi, pi));
            }
        }
    }

    let mut total = 0.0;
    for pins in net_pins.values() {
        if pins.len() < 2 {
            continue;
        }
        // Simple: sum of Manhattan distances between consecutive pin pairs
        let positions: Vec<(f64, f64)> = pins
            .iter()
            .map(|(bi, pi)| {
                let b = &boxes[*bi];
                pin_absolute_pos(&b.pins[*pi], b.x, b.y, b.rotation)
            })
            .collect();

        // Sort by x for a rough MST
        let mut sorted = positions.clone();
        sorted.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

        for w in sorted.windows(2) {
            total += (w[0].0 - w[1].0).abs() + (w[0].1 - w[1].1).abs();
        }
    }
    total
}

fn count_overlaps(boxes: &[LayoutBox]) -> usize {
    let sizes: Vec<(f64, f64)> = boxes.iter().map(effective_size).collect();
    let mut count = 0;
    for i in 0..boxes.len() {
        for j in (i + 1)..boxes.len() {
            let (aw, ah) = sizes[i];
            let (bw, bh) = sizes[j];
            let min_x = (aw + bw) / 2.0 + boxes[i].margin.max(boxes[j].margin);
            let min_y = (ah + bh) / 2.0 + boxes[i].margin.max(boxes[j].margin);
            let dx = (boxes[i].x - boxes[j].x).abs();
            let dy = (boxes[i].y - boxes[j].y).abs();
            if dx < min_x && dy < min_y {
                count += 1;
            }
        }
    }
    count
}

fn compute_alignment_quality(boxes: &[LayoutBox]) -> f64 {
    // Group by anchor_ic, measure alignment variance
    let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, b) in boxes.iter().enumerate() {
        if let Some(ref anchor) = b.anchor_ic {
            groups.entry(anchor.clone()).or_default().push(i);
        }
    }

    if groups.is_empty() {
        return 100.0;
    }

    let mut total_variance = 0.0;
    let mut group_count = 0;

    for indices in groups.values() {
        if indices.len() < 2 {
            continue;
        }
        let xs: Vec<f64> = indices.iter().map(|&i| boxes[i].x).collect();
        let ys: Vec<f64> = indices.iter().map(|&i| boxes[i].y).collect();
        let avg_x = xs.iter().sum::<f64>() / xs.len() as f64;
        let avg_y = ys.iter().sum::<f64>() / ys.len() as f64;
        let var_x = xs.iter().map(|x| (x - avg_x).powi(2)).sum::<f64>() / xs.len() as f64;
        let var_y = ys.iter().map(|y| (y - avg_y).powi(2)).sum::<f64>() / ys.len() as f64;
        total_variance += var_x.min(var_y); // use the smaller axis (aligned axis)
        group_count += 1;
    }

    if group_count == 0 {
        return 100.0;
    }
    let avg_var = total_variance / group_count as f64;
    // Lower variance = better alignment. Map to 0-100.
    (100.0 / (1.0 + avg_var * 0.1)).min(100.0)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_ic() {
        let comp = SymbolInstance {
            lib_id: "custom:MCU".into(),
            reference: "U1".into(),
            value: "STM32".into(),
            footprint: None,
            position: (0.0, 0.0, 0.0),
            mirror: kicad_json5::ir::Mirror::None,
            pins: vec![],
            properties: Default::default(),
            properties_ext: vec![],
            uuid: None,
            unit: 0,
            exclude_from_sim: false,
            in_bom: true,
            on_board: true,
            dnp: false,
            fields_autoplaced: false,
            instances: kicad_json5::ir::Instances::default(),
        };
        let tags = classify_role(&comp);
        assert!(tags.contains("ic"));
        assert!(!tags.contains("connector"));
    }

    #[test]
    fn test_classify_decoupling() {
        let comp = SymbolInstance {
            lib_id: "Device:C".into(),
            reference: "C1".into(),
            value: "100nF".into(),
            footprint: None,
            position: (0.0, 0.0, 0.0),
            mirror: kicad_json5::ir::Mirror::None,
            pins: vec![
                kicad_json5::ir::PinInstance {
                    number: "1".into(),
                    name: "".into(),
                    pin_type: "passive".into(),
                    net_id: Some(1),
                    net_name: Some("VCC".into()),
                    nc: false,
                },
                kicad_json5::ir::PinInstance {
                    number: "2".into(),
                    name: "".into(),
                    pin_type: "passive".into(),
                    net_id: Some(2),
                    net_name: Some("GND".into()),
                    nc: false,
                },
            ],
            properties: Default::default(),
            properties_ext: vec![],
            uuid: None,
            unit: 0,
            exclude_from_sim: false,
            in_bom: true,
            on_board: true,
            dnp: false,
            fields_autoplaced: false,
            instances: kicad_json5::ir::Instances::default(),
        };
        let tags = classify_role(&comp);
        assert!(tags.contains("decoupling"));
    }

    #[test]
    fn test_classify_crystal() {
        let comp = SymbolInstance {
            lib_id: "Device:Crystal".into(),
            reference: "Y1".into(),
            value: "8MHz".into(),
            footprint: None,
            position: (0.0, 0.0, 0.0),
            mirror: kicad_json5::ir::Mirror::None,
            pins: vec![],
            properties: Default::default(),
            properties_ext: vec![],
            uuid: None,
            unit: 0,
            exclude_from_sim: false,
            in_bom: true,
            on_board: true,
            dnp: false,
            fields_autoplaced: false,
            instances: kicad_json5::ir::Instances::default(),
        };
        let tags = classify_role(&comp);
        assert!(tags.contains("crystal"));
    }

    #[test]
    fn test_infer_body_size() {
        let (w, h) = infer_body_size("Resistor_SMD:R_0805", 2);
        assert!(w > 0.0 && h > 0.0);
        let (w, _h) = infer_body_size("Package_QFP:LQFP-48", 48);
        assert!(w > 5.0);
        let (w, _h) = infer_body_size("Package_TO-220:TO-220-3", 3);
        assert_eq!(w, 10.0);
        let (w, _h) = infer_body_size("Package_QFN:QFN-32", 32);
        assert!(w > 3.0 && w < 7.0);
    }

    #[test]
    fn test_extended_roles() {
        // LED
        let comp = SymbolInstance {
            lib_id: "Device:LED".into(),
            reference: "D1".into(),
            value: "Red".into(),
            footprint: None,
            position: (0.0, 0.0, 0.0),
            mirror: kicad_json5::ir::Mirror::None,
            pins: vec![],
            properties: Default::default(),
            properties_ext: vec![],
            uuid: None,
            unit: 0,
            exclude_from_sim: false,
            in_bom: true,
            on_board: true,
            dnp: false,
            fields_autoplaced: false,
            instances: kicad_json5::ir::Instances::default(),
        };
        let tags = classify_role(&comp);
        assert!(tags.contains("led"));

        // MOSFET
        let comp = SymbolInstance {
            lib_id: "Device:Q_NMOS_MOSFET".into(),
            reference: "Q1".into(),
            value: "AO3400".into(),
            footprint: None,
            position: (0.0, 0.0, 0.0),
            mirror: kicad_json5::ir::Mirror::None,
            pins: vec![],
            properties: Default::default(),
            properties_ext: vec![],
            uuid: None,
            unit: 0,
            exclude_from_sim: false,
            in_bom: true,
            on_board: true,
            dnp: false,
            fields_autoplaced: false,
            instances: kicad_json5::ir::Instances::default(),
        };
        let tags = classify_role(&comp);
        assert!(tags.contains("mosfet"));
        assert!(tags.contains("power_passive"));
    }

    #[test]
    fn test_solver_no_overlap() {
        let mut boxes = vec![
            LayoutBox {
                id: "U1".into(),
                lib_id: "Package_QFP:LQFP-48".into(),
                reference: "U1".into(),
                width: 7.0,
                height: 7.0,
                rotation: 0.0,
                margin: 1.0,
                fixed: false,
                tags: HashSet::from(["ic".into()]),
                nets: HashSet::new(),
                anchor_ic: None,
                ic_priority: IcPriority::GenericIc,
                pins: vec![],
                x: 0.0,
                y: 0.0,
            },
            LayoutBox {
                id: "U2".into(),
                lib_id: "Package_SOIC:SOIC-8".into(),
                reference: "U2".into(),
                width: 5.0,
                height: 4.0,
                rotation: 0.0,
                margin: 1.0,
                fixed: false,
                tags: HashSet::from(["ic".into()]),
                nets: HashSet::new(),
                anchor_ic: None,
                ic_priority: IcPriority::GenericIc,
                pins: vec![],
                x: 0.0,
                y: 0.0,
            },
        ];
        // Initially overlapping at origin
        for _ in 0..20 {
            if !resolve_collisions_bounded(&mut boxes, 200.0, 200.0) {
                break;
            }
        }
        let dx = (boxes[0].x - boxes[1].x).abs();
        let dy = (boxes[0].y - boxes[1].y).abs();
        let min_x = (boxes[0].width + boxes[1].width) / 2.0 + 1.0;
        let min_y = (boxes[0].height + boxes[1].height) / 2.0 + 1.0;
        assert!(
            dx >= min_x || dy >= min_y,
            "Boxes should not overlap after resolution"
        );
    }

    #[test]
    fn test_build_pin_offsets_2pin() {
        let pins = vec![
            kicad_json5::ir::PinInstance {
                number: "1".into(),
                name: "".into(),
                pin_type: "passive".into(),
                net_id: Some(1),
                net_name: Some("VCC".into()),
                nc: false,
            },
            kicad_json5::ir::PinInstance {
                number: "2".into(),
                name: "".into(),
                pin_type: "passive".into(),
                net_id: Some(2),
                net_name: Some("GND".into()),
                nc: false,
            },
        ];
        let offsets = build_pin_offsets("Device:C", &pins, 2.0, 1.2);
        assert_eq!(offsets.len(), 2);
        assert!(offsets[0].dx < 0.0, "pin1 should be on the left");
        assert!(offsets[1].dx > 0.0, "pin2 should be on the right");
    }

    #[test]
    fn test_build_pin_offsets_pinheader_2x13_footprint_id() {
        // gen-pcb passes the FOOTPRINT string, not the symbol lib_id —
        // "PINHEADER_2X13_P2.54MM_..." has "_2X" (not "02X") and previously
        // fell through to the generic perimeter distribution (0.73mm cram).
        let mk = |i: u32| kicad_json5::ir::PinInstance {
            number: i.to_string(),
            name: "".into(),
            pin_type: "passive".into(),
            net_id: None,
            net_name: None,
            nc: false,
        };
        let pins: Vec<_> = (1..=26).map(mk).collect();
        let lib = "Connector_PinHeader_2.54mm:PinHeader_2x13_P2.54mm_Horizontal";
        let offsets = build_pin_offsets(lib, &pins, 5.08, 33.02);
        assert_eq!(offsets.len(), 26);
        let by_name = |n: &str| offsets.iter().find(|p| p.name == n).unwrap();
        // KiCad PinHeader_2x13 pads: ±1.27mm columns, 13 rows at 2.54mm pitch
        let p1 = by_name("1");
        assert!(
            (p1.dx + 1.27).abs() < 1e-6 && (p1.dy + 15.24).abs() < 1e-6,
            "pad1 should be top-left at (-1.27,-15.24), got ({},{})",
            p1.dx,
            p1.dy
        );
        let p2 = by_name("2");
        assert!(
            (p2.dx - 1.27).abs() < 1e-6 && (p2.dy + 15.24).abs() < 1e-6,
            "pad2 should be top-right at (1.27,-15.24) — zigzag, got ({},{})",
            p2.dx,
            p2.dy
        );
        // Min pitch between any two pads ≥ 2.54 - epsilon (no crammed edges)
        for a in &offsets {
            for b in &offsets {
                if a.name != b.name {
                    let d = ((a.dx - b.dx).powi(2) + (a.dy - b.dy).powi(2)).sqrt();
                    assert!(
                        d > 2.0,
                        "pads {} and {} only {d:.3}mm apart",
                        a.name,
                        b.name
                    );
                }
            }
        }
    }

    #[test]
    fn test_build_pin_offsets_pinheader_2mm_pitch() {
        let mk = |i: u32| kicad_json5::ir::PinInstance {
            number: i.to_string(),
            name: "".into(),
            pin_type: "passive".into(),
            net_id: None,
            net_name: None,
            nc: false,
        };
        let pins: Vec<_> = (1..=10).map(mk).collect();
        let lib = "Connector_PinHeader_2.00mm:PinHeader_2x05_P2.00mm_Vertical";
        let offsets = build_pin_offsets(lib, &pins, 4.0, 10.0);
        let p1 = offsets.iter().find(|p| p.name == "1").unwrap();
        let p2 = offsets.iter().find(|p| p.name == "2").unwrap();
        assert!(
            (p2.dx - p1.dx - 2.0).abs() < 1e-6,
            "2.0mm column pitch from name, got {} vs {}",
            p1.dx,
            p2.dx
        );
    }

    #[test]
    fn test_infer_rotation_decoupling() {
        // IC at (10, 10) with VCC pin on right side
        let ic = LayoutBox {
            id: "U1".into(),
            lib_id: "MCU".into(),
            reference: "U1".into(),
            width: 7.0,
            height: 7.0,
            rotation: 0.0,
            margin: 1.0,
            fixed: true,
            tags: HashSet::from(["ic".into()]),
            nets: HashSet::from(["VCC".into(), "GND".into()]),
            anchor_ic: None,
            ic_priority: IcPriority::GenericIc,
            pins: vec![
                PinInfo {
                    name: "VCC".into(),
                    dx: 3.5,
                    dy: 0.0,
                    net: Some("VCC".into()),
                },
                PinInfo {
                    name: "GND".into(),
                    dx: -3.5,
                    dy: 0.0,
                    net: Some("GND".into()),
                },
            ],
            x: 10.0,
            y: 10.0,
        };

        // Decoupling cap with pin1=VCC, pin2=GND
        let cap = LayoutBox {
            id: "C1".into(),
            lib_id: "Device:C".into(),
            reference: "C1".into(),
            width: 2.0,
            height: 1.2,
            rotation: 0.0,
            margin: 0.5,
            fixed: false,
            tags: HashSet::from(["decoupling".into()]),
            nets: HashSet::from(["VCC".into(), "GND".into()]),
            anchor_ic: Some("U1".into()),
            ic_priority: IcPriority::GenericIc,
            pins: vec![
                PinInfo {
                    name: "1".into(),
                    dx: -1.0,
                    dy: 0.0,
                    net: Some("VCC".into()),
                },
                PinInfo {
                    name: "2".into(),
                    dx: 1.0,
                    dy: 0.0,
                    net: Some("GND".into()),
                },
            ],
            x: 15.0,
            y: 10.0,
        };

        let mut boxes = vec![ic, cap];
        infer_all_rotations(&mut boxes);
        // Cap should rotate 180° so VCC pin faces right toward IC's VCC pin (which is at right side)
        // Actually, with cap to the right of IC, VCC pin at dx=-1.0 (left) should face IC
        // So rotation=0 means pin1(VCC) points left toward IC — that's correct already
        assert!(boxes[1].rotation == 0.0 || boxes[1].rotation == 180.0);
    }

    #[test]
    fn test_effective_size_rotation() {
        let b = LayoutBox {
            id: "R1".into(),
            lib_id: "Device:R".into(),
            reference: "R1".into(),
            width: 2.0,
            height: 1.2,
            rotation: 90.0,
            margin: 0.5,
            fixed: false,
            tags: HashSet::new(),
            nets: HashSet::new(),
            anchor_ic: None,
            ic_priority: IcPriority::GenericIc,
            pins: vec![],
            x: 0.0,
            y: 0.0,
        };
        let (ew, eh) = effective_size(&b);
        assert_eq!(ew, 1.2); // swapped
        assert_eq!(eh, 2.0);
    }

    #[test]
    fn test_align_constraints() {
        let mut boxes = vec![
            LayoutBox {
                id: "C1".into(),
                lib_id: "Device:C".into(),
                reference: "C1".into(),
                width: 2.0,
                height: 1.2,
                rotation: 0.0,
                margin: 0.5,
                fixed: false,
                tags: HashSet::from(["decoupling".into()]),
                nets: HashSet::new(),
                anchor_ic: Some("U1".into()),
                ic_priority: IcPriority::GenericIc,
                pins: vec![],
                x: 20.0,
                y: 10.0,
            },
            LayoutBox {
                id: "C2".into(),
                lib_id: "Device:C".into(),
                reference: "C2".into(),
                width: 2.0,
                height: 1.2,
                rotation: 0.0,
                margin: 0.5,
                fixed: false,
                tags: HashSet::from(["decoupling".into()]),
                nets: HashSet::new(),
                anchor_ic: Some("U1".into()),
                ic_priority: IcPriority::GenericIc,
                pins: vec![],
                x: 25.0,
                y: 15.0,
            },
        ];
        apply_align(&mut boxes, Alignment::CenterX, &["C1".into(), "C2".into()]);
        let dx = (boxes[0].x - boxes[1].x).abs();
        assert!(dx < 0.01, "Aligned components should share x coordinate");
    }

    #[test]
    fn test_row_constraints() {
        let mut boxes = vec![
            LayoutBox {
                id: "C1".into(),
                lib_id: "Device:C".into(),
                reference: "C1".into(),
                width: 2.0,
                height: 1.2,
                rotation: 0.0,
                margin: 0.5,
                fixed: false,
                tags: HashSet::new(),
                nets: HashSet::new(),
                anchor_ic: None,
                ic_priority: IcPriority::GenericIc,
                pins: vec![],
                x: 20.0,
                y: 10.0,
            },
            LayoutBox {
                id: "C2".into(),
                lib_id: "Device:C".into(),
                reference: "C2".into(),
                width: 2.0,
                height: 1.2,
                rotation: 0.0,
                margin: 0.5,
                fixed: false,
                tags: HashSet::new(),
                nets: HashSet::new(),
                anchor_ic: None,
                ic_priority: IcPriority::GenericIc,
                pins: vec![],
                x: 20.0,
                y: 10.0,
            },
            LayoutBox {
                id: "C3".into(),
                lib_id: "Device:C".into(),
                reference: "C3".into(),
                width: 2.0,
                height: 1.2,
                rotation: 0.0,
                margin: 0.5,
                fixed: false,
                tags: HashSet::new(),
                nets: HashSet::new(),
                anchor_ic: None,
                ic_priority: IcPriority::GenericIc,
                pins: vec![],
                x: 20.0,
                y: 10.0,
            },
        ];
        apply_row(&mut boxes, &["C1".into(), "C2".into(), "C3".into()], 2.54);
        let d01 = (boxes[1].x - boxes[0].x).abs();
        let d12 = (boxes[2].x - boxes[1].x).abs();
        assert!((d01 - 2.54).abs() < 0.01, "Row spacing should be 2.54");
        assert!((d12 - 2.54).abs() < 0.01, "Row spacing should be 2.54");
    }

    #[test]
    fn test_score_layout() {
        let zones = BoardZones::new(60.0, 40.0, 3.0);
        let boxes = vec![
            LayoutBox {
                id: "U1".into(),
                lib_id: "MCU".into(),
                reference: "U1".into(),
                width: 7.0,
                height: 7.0,
                rotation: 0.0,
                margin: 1.0,
                fixed: true,
                tags: HashSet::from(["ic".into()]),
                nets: HashSet::new(),
                anchor_ic: None,
                ic_priority: IcPriority::GenericIc,
                pins: vec![],
                x: 30.0,
                y: 20.0,
            },
            LayoutBox {
                id: "C1".into(),
                lib_id: "Device:C".into(),
                reference: "C1".into(),
                width: 2.0,
                height: 1.2,
                rotation: 0.0,
                margin: 0.5,
                fixed: false,
                tags: HashSet::from(["decoupling".into()]),
                nets: HashSet::new(),
                anchor_ic: Some("U1".into()),
                ic_priority: IcPriority::GenericIc,
                pins: vec![],
                x: 38.0,
                y: 20.0,
            },
        ];
        let constraints = vec![
            Constraint::InZone("center".into()),
            Constraint::Near("U1".into(), Direction::Right, 1.5),
        ];
        let score = score_layout(&boxes, &constraints, &zones);
        assert!(score.total > 0.0, "Score should be positive");
        assert!(score.overlap_count == 0, "No overlaps expected");
    }
}

// ---------------------------------------------------------------------------
// Pad Parameter Inference
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct PadParams {
    pub size: (f64, f64),
    pub pad_type: PadType,
    pub shape: PadShape,
    pub drill: Option<DrillDef>,
    pub roundrect_rratio: Option<f64>,
    pub layers: Vec<String>,
}

pub fn infer_pad_params(lib_id: &str, pin_count: usize) -> PadParams {
    let lib = lib_id.to_uppercase();

    // KAF-09001 CCD leaf blades: 0.46mm-wide blades soldered directly,
    // 1.5mm-wide pad absorbs ±0.5mm row-spacing tolerance, 4.0mm long
    // extending outward from the ceramic body edge.
    if lib.contains("KAF-09001") || lib.contains("KAF09001") || lib.contains("CERDIP-60") {
        return PadParams {
            size: (4.0, 1.5),
            pad_type: PadType::Smd,
            shape: PadShape::Rect,
            drill: None,
            roundrect_rratio: None,
            layers: vec!["F.Cu".into(), "F.Paste".into(), "F.Mask".into()],
        };
    }

    // FPC / Flex connectors — fine-pitch SMD pads
    if lib.contains("FPC") || lib.contains("FLEX") || lib.contains("FH12") || lib.contains("FFC") {
        return PadParams {
            size: (0.3, 1.2),
            pad_type: PadType::Smd,
            shape: PadShape::Rect,
            drill: None,
            roundrect_rratio: None,
            layers: vec!["F.Cu".into(), "F.Paste".into(), "F.Mask".into()],
        };
    }

    // Through-hole header connectors — KiCad PinHeader standard: 1.7mm pad, 1.0mm drill
    if lib.contains("PINHEADER")
        || lib.contains("01X")
        || lib.contains("02X")
        || lib.contains("USB")
        || lib.contains("IDC")
        || lib.contains("CONN_")
        || (lib.contains("CONNECTOR") && !lib.contains("SMD"))
    {
        return PadParams {
            size: (1.7, 1.7),
            pad_type: PadType::ThruHole,
            shape: PadShape::Circle,
            drill: Some(DrillDef {
                diameter: 1.0,
                width: None,
                offset: None,
            }),
            roundrect_rratio: None,
            layers: vec!["*.Cu".into(), "*.Mask".into()],
        };
    }

    // BGA
    if lib.contains("BGA") {
        return PadParams {
            size: (0.3, 0.3),
            pad_type: PadType::Smd,
            shape: PadShape::Circle,
            drill: None,
            roundrect_rratio: None,
            layers: vec!["F.Cu".into(), "F.Paste".into(), "F.Mask".into()],
        };
    }

    // QFP / QFN / DFN — fine-pitch IC pads; width scaled by named pitch
    // (0.4mm pitch leaves only pitch-0.3=0.1mm gap with the old 0.3mm pad)
    if lib.contains("QFP") || lib.contains("QFN") || lib.contains("DFN") {
        let (w, h) = match parse_named_pitch(&lib) {
            Some(p) if p <= 0.4 => (0.2, 0.9),
            Some(p) if p <= 0.5 => (0.3, 1.0),
            _ => (0.6, 0.3),
        };
        return PadParams {
            size: (w, h),
            pad_type: PadType::Smd,
            shape: PadShape::RoundRect,
            drill: None,
            roundrect_rratio: Some(0.25),
            layers: vec!["F.Cu".into(), "F.Paste".into(), "F.Mask".into()],
        };
    }

    // SOIC / TSSOP / MSOP
    if lib.contains("SOIC") || lib.contains("TSSOP") || lib.contains("MSOP") {
        return PadParams {
            size: (0.6, 0.3),
            pad_type: PadType::Smd,
            shape: PadShape::RoundRect,
            drill: None,
            roundrect_rratio: Some(0.25),
            layers: vec!["F.Cu".into(), "F.Paste".into(), "F.Mask".into()],
        };
    }

    // SOT-23-6 / SOT-363
    if lib.contains("SOT-23-6")
        || lib.contains("SOT23-6")
        || lib.contains("SOT-363")
        || lib.contains("SOT363")
    {
        return PadParams {
            size: (0.7, 0.6),
            pad_type: PadType::Smd,
            shape: PadShape::RoundRect,
            drill: None,
            roundrect_rratio: Some(0.25),
            layers: vec!["F.Cu".into(), "F.Paste".into(), "F.Mask".into()],
        };
    }

    // SOT-23-5 / SOT-353 — 0.95 pitch needs ≤0.6 pad width
    if lib.contains("SOT-23-5")
        || lib.contains("SOT23-5")
        || lib.contains("SOT-353")
        || lib.contains("SOT353")
    {
        return PadParams {
            size: (0.6, 0.7),
            pad_type: PadType::Smd,
            shape: PadShape::RoundRect,
            drill: None,
            roundrect_rratio: Some(0.25),
            layers: vec!["F.Cu".into(), "F.Paste".into(), "F.Mask".into()],
        };
    }

    // SOT-323 / SC-70 — 0.65mm pitch, official pad 0.6x0.7
    if lib.contains("SOT-323")
        || lib.contains("SOT323")
        || lib.contains("SOT-353")
        || lib.contains("SOT353")
    {
        return PadParams {
            size: (0.6, 0.7),
            pad_type: PadType::Smd,
            shape: PadShape::RoundRect,
            drill: None,
            roundrect_rratio: Some(0.25),
            layers: vec!["F.Cu".into(), "F.Paste".into(), "F.Mask".into()],
        };
    }

    // SOT-23 (3-pin) — official pad 0.6x0.7, pitch 1.9 span 1.875
    if lib.contains("SOT-23") || lib.contains("SOT23") {
        return PadParams {
            size: (0.6, 0.7),
            pad_type: PadType::Smd,
            shape: PadShape::RoundRect,
            drill: None,
            roundrect_rratio: Some(0.25),
            layers: vec!["F.Cu".into(), "F.Paste".into(), "F.Mask".into()],
        };
    }

    // SOT (other)
    if lib.contains("SOT") {
        return PadParams {
            size: (1.0, 0.6),
            pad_type: PadType::Smd,
            shape: PadShape::Rect,
            drill: None,
            roundrect_rratio: None,
            layers: vec!["F.Cu".into(), "F.Paste".into(), "F.Mask".into()],
        };
    }

    // TO packages — larger SMD pads
    if lib.contains("TO-220")
        || lib.contains("TO-252")
        || lib.contains("TO-263")
        || lib.contains("DPAK")
        || lib.contains("D2PAK")
    {
        return PadParams {
            size: (1.5, 1.0),
            pad_type: PadType::Smd,
            shape: PadShape::Rect,
            drill: None,
            roundrect_rratio: None,
            layers: vec!["F.Cu".into(), "F.Paste".into(), "F.Mask".into()],
        };
    }

    // TVS
    if lib.contains("TVS") || lib.contains("D_TVS") {
        return PadParams {
            size: (1.0, 0.6),
            pad_type: PadType::Smd,
            shape: PadShape::Rect,
            drill: None,
            roundrect_rratio: None,
            layers: vec!["F.Cu".into(), "F.Paste".into(), "F.Mask".into()],
        };
    }

    // Multi-pin ICs (≥4 pins) without specific package match — assume QFN-like
    if pin_count >= 4 {
        return PadParams {
            size: (0.6, 0.3),
            pad_type: PadType::Smd,
            shape: PadShape::RoundRect,
            drill: None,
            roundrect_rratio: Some(0.25),
            layers: vec!["F.Cu".into(), "F.Paste".into(), "F.Mask".into()],
        };
    }

    // 2-pin passives and LEDs/Diodes — default SMD
    // 0402 body is 1.0mm wide (pin offsets ±0.5): a 1.0mm pad would make the
    // two pads touch — real KiCad C_0402 pad is 0.55mm wide
    if lib.contains("0402") {
        return PadParams {
            size: (0.55, 0.6),
            pad_type: PadType::Smd,
            shape: PadShape::Rect,
            drill: None,
            roundrect_rratio: None,
            layers: vec!["F.Cu".into(), "F.Paste".into(), "F.Mask".into()],
        };
    }
    PadParams {
        size: (1.0, 0.6),
        pad_type: PadType::Smd,
        shape: PadShape::Rect,
        drill: None,
        roundrect_rratio: None,
        layers: vec!["F.Cu".into(), "F.Paste".into(), "F.Mask".into()],
    }
}

#[cfg(test)]
mod flow_tests {
    use super::*;

    fn box_with(id: &str, lib: &str, tags: &[&str], nets: &[&str]) -> LayoutBox {
        let mut b = LayoutBox {
            id: id.into(),
            lib_id: lib.into(),
            reference: id.into(),
            width: 4.0,
            height: 2.0,
            rotation: 0.0,
            margin: 0.5,
            fixed: false,
            tags: tags.iter().map(|s| s.to_string()).collect(),
            nets: nets.iter().map(|s| s.to_string()).collect(),
            anchor_ic: None,
            ic_priority: IcPriority::GenericIc,
            pins: Vec::new(),
            x: 0.0,
            y: 0.0,
        };
        for (i, n) in nets.iter().enumerate() {
            if *n == "GND" {
                continue;
            }
            b.pins.push(PinInfo {
                name: format!("p{}", i),
                dx: 0.0,
                dy: 0.0,
                net: Some(n.to_string()),
            });
        }
        b
    }

    #[test]
    fn test_detect_flow_chain_battery() {
        let boxes = vec![
            box_with(
                "J1",
                "Connector:PinHeader_1x02",
                &["connector"],
                &["VBAT_IN", "GND"],
            ),
            box_with(
                "F1",
                "Fuse:Fuse_1206",
                &["power_passive"],
                &["VBAT_IN", "VBAT_FUSED"],
            ),
            box_with(
                "Q1",
                "Package_TO_SOT_SMD:SOT-23",
                &["ic"],
                &["VBAT_FUSED", "VBAT_PROT", "GATE_Q1"],
            ),
            box_with(
                "U1",
                "Converter:SC59",
                &["ic"],
                &["VBAT_PROT", "GND", "SW_NODE", "BST_NODE", "FB_NODE"],
            ),
            box_with(
                "L1",
                "Inductor:L_4020",
                &["power_passive"],
                &["SW_NODE", "5V_BUCK"],
            ),
            box_with("C4", "Capacitor:0805", &["passive"], &["5V_BUCK", "GND"]),
            box_with(
                "J2",
                "Connector:PinHeader_2x05",
                &["connector"],
                &["5V_BUCK", "3V3_STBY", "CCD_PWR_EN", "GND"],
            ),
        ];
        let chain = detect_flow_chain(&boxes).expect("chain must be detected");
        assert_eq!(chain.first().map(|s| s.as_str()), Some("J1"));
        assert_eq!(chain.last().map(|s| s.as_str()), Some("J2"));
        // series elements in path order between the connectors
        assert!(chain.contains(&"F1".to_string()));
        assert!(chain.contains(&"Q1".to_string()));
        assert!(chain.contains(&"U1".to_string()));
        assert!(chain.contains(&"L1".to_string()));
        // order preserved: J1 < F1 < Q1 < U1 < L1 < J2
        let idx = |s: &str| chain.iter().position(|c| c == s).unwrap();
        assert!(
            idx("J1") < idx("F1")
                && idx("F1") < idx("Q1")
                && idx("Q1") < idx("U1")
                && idx("U1") < idx("L1")
                && idx("L1") < idx("J2")
        );
    }

    #[test]
    fn test_flow_constraint_check() {
        let mut a = box_with("A", "T:A", &[], &["N1"]);
        let mut b = box_with("B", "T:B", &[], &["N1", "N2"]);
        let mut c = box_with("C", "T:C", &[], &["N2"]);
        a.x = 10.0;
        b.x = 20.0;
        c.x = 30.0;
        let mut boxes = vec![c, a, b];
        assert!(check_constraint(
            &boxes,
            &Constraint::Flow(vec!["A".into(), "B".into(), "C".into()])
        ));
        boxes[0].x = 5.0; // C out of order
        assert!(!check_constraint(
            &boxes,
            &Constraint::Flow(vec!["A".into(), "B".into(), "C".into()])
        ));
    }
}

// ---------------------------------------------------------------------------
// Phase B: 远程多 seed SA（kroute-server LayoutOptimize RPC 消费）
// ---------------------------------------------------------------------------

/// 一个 seed 的解：refs 与 positions 一一对应（reference, x, y, rot）。
#[derive(Debug, Clone)]
pub struct LayoutSeedSolution {
    pub seed: u64,
    pub cost: f64,
    pub refs: Vec<String>,
    pub positions: Vec<(f64, f64, f64)>,
}

/// 从 schematic 构建 SA 问题并跑指定 seed 集合，按 cost 升序返回。
///
/// 问题构建与 [`auto_layout_refs_with_directives_fixed`] 前半段保持一致
/// （复制式维护：主线活跃期零回归优先）。与 `solve` 的差异：seed 池显式
/// 传入（不受本机 rayon 8 线程上限约束），供远程算力节点聚合多 seed。
/// 权重与 `solve` 相同取 `SaCostWeights::default()`。
pub fn run_sa_seeds(
    schematic: &Schematic,
    directives: &LayoutDirectives,
    fixed_board_size: Option<(f64, f64)>,
    seeds: &[u64],
) -> Vec<LayoutSeedSolution> {
    run_sa_seeds_with_dims(
        schematic,
        directives,
        fixed_board_size,
        seeds,
        &HashMap::new(),
    )
}

/// M3 第二片：从板量测每个 footprint 的真实物理包围盒（fp 本地系 pad 外扩 + margin）。
/// SA 盒尺寸用实测值替代启发式估计——启发式偏小是回填后 footprint 物理重叠
/// （carrier s3 未布线已含 shorting 144）的根因。
pub fn measure_footprint_extents(
    board: &kicad_json5::Board,
    margin_mm: f64,
) -> HashMap<String, (f64, f64)> {
    let mut out = HashMap::new();
    for fp in &board.footprints {
        let mut ext_x: f64 = 0.5; // 退化下限：至少 0.5mm 半宽
        let mut ext_y: f64 = 0.5;
        for pad in &fp.pads {
            let (px, py, _) = pad.position; // fp 本地系（未含 fp 旋转）
            let (sw, sh) = pad.size;
            ext_x = ext_x.max(px.abs() + sw / 2.0);
            ext_y = ext_y.max(py.abs() + sh / 2.0);
        }
        out.insert(
            fp.reference.clone(),
            ((ext_x + margin_mm) * 2.0, (ext_y + margin_mm) * 2.0),
        );
    }
    out
}

/// run_sa_seeds 带盒尺寸覆盖：ref → (宽, 高) 实测值优先于启发式估计。
pub fn run_sa_seeds_with_dims(
    schematic: &Schematic,
    directives: &LayoutDirectives,
    fixed_board_size: Option<(f64, f64)>,
    seeds: &[u64],
    dim_overrides: &HashMap<String, (f64, f64)>,
) -> Vec<LayoutSeedSolution> {
    if schematic.components.is_empty() || seeds.is_empty() {
        return Vec::new();
    }
    let mut boxes = build_boxes(schematic);
    for b in boxes.iter_mut() {
        if let Some((w, h)) = dim_overrides.get(&b.reference) {
            b.width = *w;
            b.height = *h;
        }
    }

    let total_area: f64 = boxes.iter().map(|b| b.width * b.height).sum();
    let (mut board_w, mut board_h) = if let Some((fw, fh)) = fixed_board_size {
        eprintln!("[layout] Fixed board: {:.1}x{:.1}mm", fw, fh);
        (fw, fh)
    } else {
        let density_factor = 1.0 + (boxes.len() as f64 / 40.0).min(2.5);
        let board_side = (total_area * 2.5 * density_factor).sqrt().max(30.0);
        (board_side * 1.4, board_side)
    };

    let constraints = generate_constraints(&mut boxes, directives);

    let conn_gap = 5.0;
    let connector_indices: Vec<usize> = boxes
        .iter()
        .enumerate()
        .filter(|(_, b)| b.tags.contains("connector"))
        .map(|(i, _)| i)
        .collect();
    let connector_anchors: Vec<AnchorType> = constraints
        .iter()
        .filter_map(|c| {
            if let Constraint::Anchor(at) = c {
                Some(*at)
            } else {
                None
            }
        })
        .collect();

    let mut edge_total: HashMap<String, f64> = HashMap::new();
    for (ci, &idx) in connector_indices.iter().enumerate() {
        let at = connector_anchors
            .get(ci)
            .copied()
            .unwrap_or(AnchorType::BottomEdge);
        let key = format!("{:?}", at);
        let size = boxes[idx].width.max(boxes[idx].height) + conn_gap;
        *edge_total.entry(key).or_default() += size;
    }
    let is_fixed = fixed_board_size.is_some();
    if !is_fixed {
        let needed_w = edge_total
            .get("TopEdge")
            .or(edge_total.get("BottomEdge"))
            .map(|&t| t + 10.0)
            .unwrap_or(board_w)
            .max(board_w);
        let needed_h = edge_total
            .get("LeftEdge")
            .or(edge_total.get("RightEdge"))
            .map(|&t| t + 10.0)
            .unwrap_or(board_h)
            .max(board_h);
        board_w = board_w.max(needed_w);
        board_h = board_h.max(needed_h);
        let max_conn_dim: f64 = connector_indices
            .iter()
            .map(|&idx| boxes[idx].width.max(boxes[idx].height) + 2.0)
            .sum();
        if max_conn_dim > board_h * 0.8 {
            board_h = max_conn_dim * 1.3;
        }
        if max_conn_dim > board_w * 0.8 {
            board_w = max_conn_dim * 1.3;
        }
    }

    let zones = BoardZones::new(board_w, board_h, 3.0);
    initialize_positions(&mut boxes, &zones);
    // v35（M2）：锚定连接器预置到目标边——随后 SA 钉扎这些 fixed 件不动
    place_anchored_connectors(&mut boxes, &constraints, board_w, board_h);

    // M3：权重走 directives（此前硬编码 Default，RPC 传的 overlap_weight 被静默忽略）
    let weights = directives.sa_weights.clone();
    let widths: Vec<f64> = boxes.iter().map(|b| b.width + b.margin * 2.0).collect();
    let heights: Vec<f64> = boxes.iter().map(|b| b.height + b.margin * 2.0).collect();
    let spacing = if boxes.len() > 80 {
        0.5 + (boxes.len() as f64 / 25.0).min(3.5)
    } else {
        0.5 + (boxes.len() as f64 / 30.0).min(2.0)
    };

    let refs: Vec<String> = boxes.iter().map(|b| b.reference.clone()).collect();

    use rayon::prelude::*;
    let mut solutions: Vec<LayoutSeedSolution> = seeds
        .par_iter()
        .map(|&seed| {
            let (sp, cost) = sa_floorplan_single(
                &boxes, &widths, &heights, spacing, &zones, &weights, seed, 0, true,
            );
            let mut positions = sp.decode(&widths, &heights, spacing);
            // v36 修复: 自由件板界 clamp（SA 移动分支有 clamp 但初始摆位无;
            // 无 clamp 时 L1/J_PWR 曾出界 12-15mm 且随 seed 漂移）
            for (i, (x, y)) in positions.iter_mut().enumerate() {
                if !boxes[i].fixed {
                    let hw = boxes[i].width / 2.0 + boxes[i].margin;
                    let hh = boxes[i].height / 2.0 + boxes[i].margin;
                    let lo_x = hw.min(board_w / 2.0);
                    let hi_x = (board_w - hw).max(lo_x);
                    let lo_y = hh.min(board_h / 2.0);
                    let hi_y = (board_h - hh).max(lo_y);
                    *x = x.max(lo_x).min(hi_x);
                    *y = y.max(lo_y).min(hi_y);
                }
            }
            // 钉扎件终态回写（与 sa_floorplan_single 内部代价口径一致）
            for (i, b) in boxes.iter().enumerate() {
                if b.fixed {
                    positions[i] = (b.x, b.y);
                }
            }
            let positions = positions
                .iter()
                .enumerate()
                .map(|(i, (x, y))| {
                    let rot = if sp.rotations[i] {
                        (boxes[i].rotation + 90.0) % 360.0
                    } else {
                        boxes[i].rotation
                    };
                    (*x, *y, rot)
                })
                .collect();
            LayoutSeedSolution {
                seed,
                cost,
                refs: refs.clone(),
                positions,
            }
        })
        .collect();
    solutions.sort_by(|a, b| {
        a.cost
            .partial_cmp(&b.cost)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    solutions
}
