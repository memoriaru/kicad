use serde::{Deserialize, Serialize};
use std::fmt;

// ── Electrical Type ──────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ElectricalType {
    Input,
    Output,
    Bidirectional,
    TriState,
    Passive,
    Free,
    Unspecified,
    PowerIn,
    PowerOut,
    OpenCollector,
    OpenEmitter,
    NoConnect,
}

impl ElectricalType {
    pub fn from_str_lossy(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "input" => Self::Input,
            "output" => Self::Output,
            "bidirectional" | "bi_directional" | "bidir" => Self::Bidirectional,
            "tri_state" | "tristate" => Self::TriState,
            "passive" => Self::Passive,
            "free" => Self::Free,
            "unspecified" => Self::Unspecified,
            "power_in" | "powerin" | "wacc" => Self::PowerIn,
            "power_out" | "powerout" | "wsrc" => Self::PowerOut,
            "open_collector" | "opencollector" => Self::OpenCollector,
            "open_emitter" | "openemitter" => Self::OpenEmitter,
            "no_connect" | "noconnect" | "nc" => Self::NoConnect,
            _ => Self::Passive,
        }
    }

    /// KiCad S-expression pin type keyword
    pub fn to_kicad_keyword(&self) -> &str {
        match self {
            Self::Input => "input",
            Self::Output => "output",
            Self::Bidirectional => "bidirectional",
            Self::TriState => "tri_state",
            Self::Passive => "passive",
            Self::Free => "free",
            Self::Unspecified => "unspecified",
            Self::PowerIn => "power_in",
            Self::PowerOut => "power_out",
            Self::OpenCollector => "open_collector",
            Self::OpenEmitter => "open_emitter",
            Self::NoConnect => "no_connect",
        }
    }

    pub fn is_power(&self) -> bool {
        matches!(self, Self::PowerIn | Self::PowerOut)
    }
}

impl fmt::Display for ElectricalType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_kicad_keyword())
    }
}

// ── Pin Side (layout position) ──────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinSide {
    Left,
    Right,
    Top,
    Bottom,
}

impl PinSide {
    /// KiCad rotation angle for pins on this side
    pub fn rotation(&self) -> f64 {
        match self {
            Self::Left => 0.0,
            Self::Right => 180.0,
            Self::Top => 270.0,
            Self::Bottom => 90.0,
        }
    }
}

// ── Pin Position ────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
pub struct PinPosition {
    pub x: f64,
    pub y: f64,
    pub side: PinSide,
}

// ── Symbol Pin ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolPin {
    pub number: String,
    pub name: String,
    #[serde(default = "default_electrical_type")]
    pub electrical_type: ElectricalType,
    #[serde(default)]
    pub pin_group: Option<String>,
    #[serde(default)]
    pub alt_functions: Option<Vec<String>>,
}

fn default_electrical_type() -> ElectricalType {
    ElectricalType::Passive
}

// ── Symbol Spec ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolSpec {
    pub mpn: String,
    #[serde(default = "default_lib_name")]
    pub lib_name: String,
    #[serde(default)]
    pub reference_prefix: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub datasheet_url: Option<String>,
    #[serde(default)]
    pub footprint: Option<String>,
    #[serde(default)]
    pub manufacturer: Option<String>,
    #[serde(default)]
    pub package: Option<String>,
    #[serde(default)]
    pub pins: Vec<SymbolPin>,
}

fn default_lib_name() -> String {
    "custom".to_string()
}

impl SymbolSpec {
    /// Full lib_id with library prefix
    pub fn lib_id(&self) -> String {
        format!("{}:{}", self.lib_name, self.mpn)
    }

    pub fn reference_prefix(&self) -> &str {
        self.reference_prefix.as_deref().unwrap_or("U")
    }
}

// ── Footprint ───────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageType {
    Dip,
    Sip,
    Soic,
    Tssop,
    Sop,
    MsoP,
    Qfp,
    Lqfp,
    Tqfp,
    Qfn,
    Dfn,
    Sot23,
    Sot223,
    Sot89,
    Sot353,
    Sot363,
    Bga,
    PinHeader,
    DipSocket,
}

impl PackageType {
    /// Parse from package string like "DIP-8", "SOIC-16", "SOT-23-5", "DIP-Socket-60"
    pub fn from_package_str(s: &str) -> Option<Self> {
        let upper = s.to_uppercase().replace(['-', '_', ' '], "");
        match upper.as_str() {
            s if s.starts_with("DIPSOCKET") => Some(Self::DipSocket),
            s if s.starts_with("DIP") => Some(Self::Dip),
            s if s.starts_with("SIP") => Some(Self::Sip),
            s if s.starts_with("TSSOP") => Some(Self::Tssop),
            s if s.starts_with("SOIC") => Some(Self::Soic),
            s if s.starts_with("MSOP") => Some(Self::MsoP),
            s if s.starts_with("SOP") => Some(Self::Sop),
            s if s.starts_with("LQFP") => Some(Self::Lqfp),
            s if s.starts_with("TQFP") => Some(Self::Tqfp),
            s if s.starts_with("QFP") => Some(Self::Qfp),
            s if s.starts_with("QFN") => Some(Self::Qfn),
            s if s.starts_with("DFN") || s.starts_with("MLF") => Some(Self::Dfn),
            s if s.starts_with("SOT23") => Some(Self::Sot23),
            s if s.starts_with("SOT223") => Some(Self::Sot223),
            s if s.starts_with("SOT89") => Some(Self::Sot89),
            s if s.starts_with("SOT353") => Some(Self::Sot353),
            s if s.starts_with("SOT363") => Some(Self::Sot363),
            s if s.starts_with("BGA") => Some(Self::Bga),
            s if s.contains("PINHEADER") || s.contains("PINHDR") => Some(Self::PinHeader),
            _ => None,
        }
    }

    pub fn is_through_hole(&self) -> bool {
        matches!(self, Self::Dip | Self::Sip | Self::PinHeader | Self::DipSocket)
    }
}

/// Extract pin count from a package string like "DIP-8" → 8, "SOT-23-5" → 5
pub fn extract_pin_count(s: &str) -> Option<u32> {
    let upper = s.to_uppercase().replace([' ', '_'], "-");
    for part in upper.split('-').rev() {
        if let Ok(n) = part.parse::<u32>() {
            return Some(n);
        }
    }
    None
}

#[derive(Debug, Clone)]
pub struct FootprintSpec {
    pub name: String,
    pub package_type: PackageType,
    pub pin_count: u32,
    pub pitch: f64,
    pub row_spacing: Option<f64>,
    pub options: FootprintOptions,
}

#[derive(Debug, Clone)]
pub struct FootprintOptions {
    pub pad_size: Option<(f64, f64)>,
    pub drill_size: Option<f64>,
    pub courtyard_margin: f64,
}

impl Default for FootprintOptions {
    fn default() -> Self {
        Self {
            pad_size: None,
            drill_size: None,
            courtyard_margin: 0.5,
        }
    }
}

// ── KiCad Version ───────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum KicadVersion {
    V7,
    #[default]
    V8,
    V9,
    V10,
}

impl KicadVersion {
    pub fn from_u8(v: u8) -> Self {
        match v {
            7 => Self::V7,
            8 => Self::V8,
            9 => Self::V9,
            10 => Self::V10,
            _ => Self::V8,
        }
    }

    pub fn sym_lib_version(&self) -> &str {
        match self {
            Self::V7 => "20220914",
            Self::V8 => "20231120",
            Self::V9 => "20240110",
            Self::V10 => "20250110",
        }
    }

    pub fn footprint_version(&self) -> &str {
        match self {
            Self::V7 => "20211014",
            _ => "20221018",
        }
    }
}
