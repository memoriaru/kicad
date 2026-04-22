//! Graphic element definitions for symbol rendering

/// A graphic element in a symbol definition
#[derive(Debug, Clone)]
pub enum GraphicElement {
    Polyline(Polyline),
    Rectangle(Rectangle),
    Circle(Circle),
    Arc(Arc),
    Text(Text),
    Pin(PinGraphic),
}

/// Polyline (multi-segment line)
#[derive(Debug, Clone)]
pub struct Polyline {
    pub points: Vec<(f64, f64)>,
    pub stroke: Stroke,
    pub fill: Fill,
}

impl Polyline {
    pub fn new() -> Self {
        Self {
            points: Vec::new(),
            stroke: Stroke::default(),
            fill: Fill::none(),
        }
    }
}

/// Rectangle
#[derive(Debug, Clone)]
pub struct Rectangle {
    pub start: (f64, f64),
    pub end: (f64, f64),
    pub stroke: Stroke,
    pub fill: Fill,
}

impl Rectangle {
    pub fn new(start: (f64, f64), end: (f64, f64)) -> Self {
        Self {
            start,
            end,
            stroke: Stroke::default(),
            fill: Fill::none(),
        }
    }
}

/// Circle
#[derive(Debug, Clone)]
pub struct Circle {
    pub center: (f64, f64),
    pub radius: f64,
    pub stroke: Stroke,
    pub fill: Fill,
}

impl Circle {
    pub fn new(center: (f64, f64), radius: f64) -> Self {
        Self {
            center,
            radius,
            stroke: Stroke::default(),
            fill: Fill::none(),
        }
    }
}

/// Arc
#[derive(Debug, Clone)]
pub struct Arc {
    pub start: (f64, f64),
    pub mid: (f64, f64),
    pub end: (f64, f64),
    pub stroke: Stroke,
    pub fill: Fill,
}

impl Arc {
    pub fn new(start: (f64, f64), mid: (f64, f64), end: (f64, f64)) -> Self {
        Self {
            start,
            mid,
            end,
            stroke: Stroke::default(),
            fill: Fill::none(),
        }
    }

    /// Calculate arc center and radius from three points
    /// Returns (center_x, center_y, radius, start_angle, end_angle)
    pub fn calculate_arc_params(&self) -> Option<(f64, f64, f64, f64, f64)> {
        let (x1, y1) = self.start;
        let (x2, y2) = self.mid;
        let (x3, y3) = self.end;

        // Calculate the perpendicular bisectors of two chords
        let ma = x2 - x1;
        let mb = y2 - y1;
        let mc = x3 - x2;
        let md = y3 - y2;

        // Check if points are collinear
        let det = ma * md - mb * mc;
        if det.abs() < 1e-10 {
            return None;
        }

        // Calculate center using the intersection of perpendicular bisectors
        let x1_sq = x1 * x1 + y1 * y1;
        let x2_sq = x2 * x2 + y2 * y2;
        let x3_sq = x3 * x3 + y3 * y3;

        let cx = (x1_sq * (y2 - y3) + x2_sq * (y3 - y1) + x3_sq * (y1 - y2)) / (2.0 * det);
        let cy = (x1_sq * (x3 - x2) + x2_sq * (x1 - x3) + x3_sq * (x2 - x1)) / (2.0 * det);

        // Calculate radius
        let radius = ((cx - x1).powi(2) + (cy - y1).powi(2)).sqrt();

        // Calculate angles
        let start_angle = (y1 - cy).atan2(x1 - cx);
        let _mid_angle = (y2 - cy).atan2(x2 - cx);
        let end_angle = (y3 - cy).atan2(x3 - cx);

        Some((cx, cy, radius, start_angle, end_angle))
    }
}

/// Text element
#[derive(Debug, Clone)]
pub struct Text {
    pub text: String,
    pub position: (f64, f64, f64), // x, y, rotation
    pub effects: TextEffects,
}

impl Text {
    pub fn new(text: impl Into<String>, position: (f64, f64, f64)) -> Self {
        Self {
            text: text.into(),
            position,
            effects: TextEffects::default(),
        }
    }
}

/// Text effects (font, justify, etc.)
#[derive(Debug, Clone)]
pub struct TextEffects {
    pub font: Font,
    pub justify: Justify,
    pub hide: bool,
}

impl Default for TextEffects {
    fn default() -> Self {
        Self {
            font: Font::default(),
            justify: Justify::default(),
            hide: false,
        }
    }
}

/// Font settings
#[derive(Debug, Clone)]
pub struct Font {
    pub size: (f64, f64), // width, height
    pub bold: bool,
    pub italic: bool,
}

impl Default for Font {
    fn default() -> Self {
        Self {
            size: (1.27, 1.27), // Default KiCad font size
            bold: false,
            italic: false,
        }
    }
}

/// Text justification
#[derive(Debug, Clone)]
pub struct Justify {
    pub horizontal: HorizontalAlign,
    pub vertical: VerticalAlign,
    pub mirror: bool,
}

impl Default for Justify {
    fn default() -> Self {
        Self {
            horizontal: HorizontalAlign::Left,
            vertical: VerticalAlign::Bottom,
            mirror: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HorizontalAlign {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VerticalAlign {
    Top,
    Center,
    Bottom,
}

/// Pin graphic element
#[derive(Debug, Clone)]
pub struct PinGraphic {
    pub pin_type: PinType,
    pub shape: PinShape,
    pub name: String,
    pub number: String,
    pub position: (f64, f64, f64), // x, y, rotation
    pub length: f64,
    pub name_effects: TextEffects,
    pub number_effects: TextEffects,
}

impl PinGraphic {
    pub fn new(number: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            pin_type: PinType::Passive,
            shape: PinShape::Line,
            name: name.into(),
            number: number.into(),
            position: (0.0, 0.0, 0.0),
            length: 2.54, // Default pin length in KiCad
            name_effects: TextEffects::default(),
            number_effects: TextEffects::default(),
        }
    }
}

/// Pin electrical type
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PinType {
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

impl PinType {
    pub fn from_str(s: &str) -> Self {
        match s {
            "input" => PinType::Input,
            "output" => PinType::Output,
            "bidirectional" => PinType::Bidirectional,
            "tri_state" => PinType::TriState,
            "passive" => PinType::Passive,
            "free" => PinType::Free,
            "unspecified" => PinType::Unspecified,
            "power_in" => PinType::PowerIn,
            "power_out" => PinType::PowerOut,
            "open_collector" => PinType::OpenCollector,
            "open_emitter" => PinType::OpenEmitter,
            "no_connect" => PinType::NoConnect,
            _ => PinType::Passive,
        }
    }

    /// Get the default shape for this pin type
    pub fn default_shape(&self) -> PinShape {
        match self {
            PinType::Input => PinShape::Line,
            PinType::Output => PinShape::Triangle,
            PinType::Bidirectional => PinShape::Line,
            PinType::PowerIn | PinType::PowerOut => PinShape::Line,
            _ => PinShape::Line,
        }
    }
}

/// Pin graphical shape
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PinShape {
    Line,
    Inverted,
    Clock,
    InvertedClock,
    InputLow,
    ClockLow,
    OutputLow,
    EdgeClockHigh,
    NonLogic,
    Triangle,
}

impl PinShape {
    pub fn from_str(s: &str) -> Self {
        match s {
            "line" => PinShape::Line,
            "inverted" => PinShape::Inverted,
            "clock" => PinShape::Clock,
            "inverted_clock" => PinShape::InvertedClock,
            "input_low" => PinShape::InputLow,
            "clock_low" => PinShape::ClockLow,
            "output_low" => PinShape::OutputLow,
            "edge_clock_high" => PinShape::EdgeClockHigh,
            "non_logic" => PinShape::NonLogic,
            _ => PinShape::Line,
        }
    }
}

/// Stroke style for lines
#[derive(Debug, Clone)]
pub struct Stroke {
    pub width: f64,
    pub stroke_type: StrokeType,
}

impl Default for Stroke {
    fn default() -> Self {
        Self {
            width: 0.0, // Use default
            stroke_type: StrokeType::Default,
        }
    }
}

/// Stroke type
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StrokeType {
    Default,
    Dash,
    Dot,
    DashDot,
    DashDotDot,
    Solid,
}

impl StrokeType {
    pub fn from_str(s: &str) -> Self {
        match s {
            "default" => StrokeType::Default,
            "dash" => StrokeType::Dash,
            "dot" => StrokeType::Dot,
            "dash_dot" => StrokeType::DashDot,
            "dash_dot_dot" => StrokeType::DashDotDot,
            "solid" => StrokeType::Solid,
            _ => StrokeType::Default,
        }
    }
}

impl std::fmt::Display for StrokeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StrokeType::Default => write!(f, "default"),
            StrokeType::Dash => write!(f, "dash"),
            StrokeType::Dot => write!(f, "dot"),
            StrokeType::DashDot => write!(f, "dash_dot"),
            StrokeType::DashDotDot => write!(f, "dash_dot_dot"),
            StrokeType::Solid => write!(f, "solid"),
        }
    }
}

/// Fill style
#[derive(Debug, Clone)]
pub struct Fill {
    pub fill_type: FillType,
    pub color: Option<(u8, u8, u8, u8)>, // RGBA (optional)
}

impl Fill {
    pub fn none() -> Self {
        Self {
            fill_type: FillType::None,
            color: None,
        }
    }

    pub fn outline() -> Self {
        Self {
            fill_type: FillType::Outline,
            color: None,
        }
    }

    pub fn background() -> Self {
        Self {
            fill_type: FillType::Background,
            color: None,
        }
    }
}

/// Fill type
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FillType {
    None,
    Outline,
    Background,
    Color,
}

impl FillType {
    pub fn from_str(s: &str) -> Self {
        match s {
            "none" => FillType::None,
            "outline" => FillType::Outline,
            "background" => FillType::Background,
            "color" => FillType::Color,
            _ => FillType::None,
        }
    }
}

/// Symbol unit (for multi-unit symbols like gates)
#[derive(Debug, Clone)]
pub struct SymbolUnit {
    pub unit_id: u32,
    pub style_id: u32,
    /// Original name from KiCad file (e.g., "C_0_1", "0603WAF1002T5E_1_1")
    pub name: String,
    pub graphics: Vec<GraphicElement>,
}

impl SymbolUnit {
    pub fn new(unit_id: u32, style_id: u32) -> Self {
        Self {
            unit_id,
            style_id,
            name: String::new(),
            graphics: Vec::new(),
        }
    }
}
