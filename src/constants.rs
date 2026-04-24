//! KiCad Renderer Constants
//!
//! 1:1 port from KiCanvas JS `DefaultValues` object + KiCad default theme colors.
//! All dimension values in mm. All colors match kicad-default.ts theme.

use crate::render_core::Color;

// ── JS DefaultValues constants (all in mm) ─────────────────

/// Dangling symbol size (mm) — used for unconnected pin indicators
pub const DANGLING_SYMBOL_SIZE: f64 = 0.3048;

/// Unselected end size (mm)
pub const UNSELECTED_END_SIZE: f64 = 0.1016;

/// Default pin length (mm) = 100 mil
pub const PIN_LENGTH: f64 = 2.54;

/// Pin symbol size (mm) — for clock/inversion decorations
pub const PIN_SYMBOL_SIZE: f64 = 0.635;

/// Pin number text size (mm) = 50 mil
pub const PINNUM_SIZE: f64 = 1.27;

/// Pin name text size (mm) = 50 mil
pub const PINNAME_SIZE: f64 = 1.27;

/// Selection highlight thickness (mm) = 3 mil
pub const SELECTION_THICKNESS: f64 = 0.0762;

/// Default line/stroke width (mm) = 6 mil
pub const LINE_WIDTH: f64 = 0.1524;

/// Wire width (mm) = 6 mil
pub const WIRE_WIDTH: f64 = 0.1524;

/// Bus width (mm) = 12 mil
pub const BUS_WIDTH: f64 = 0.3048;

/// No-connect symbol size (mm) = 48 mil
pub const NOCONNECT_SIZE: f64 = 1.2192;

/// Junction diameter (mm) = 36 mil
pub const JUNCTION_DIAMETER: f64 = 0.9144;

/// Target pin radius for selection (mm) = 15 mil
pub const TARGET_PIN_RADIUS: f64 = 0.381;

/// Schematic entry size (mm) = 100 mil
pub const SCH_ENTRY_SIZE: f64 = 2.54;

/// Default text size (mm) = 50 mil
pub const TEXT_SIZE: f64 = 1.27;

/// Text offset ratio for label/pin text spacing
pub const TEXT_OFFSET_RATIO: f64 = 0.15;

/// Label size ratio (used for global label shape margin)
pub const LABEL_SIZE_RATIO: f64 = 0.375;

/// Pin name offset (mm) = 20 mil
pub const PIN_NAME_OFFSET: f64 = 0.508;

/// Pin label text margin base (mm) = 24 mil = 4 × LINE_WIDTH
pub const TEXT_MARGIN: f64 = 4.0 * LINE_WIDTH;

/// Character width/height ratio for KiCad stroke font (JS: StrokeFont.space_width)
pub const CHAR_WIDTH_RATIO: f64 = 0.6;

/// Interline pitch ratio for multi-line text (JS: StrokeFont.interline_pitch_ratio)
pub const INTERLINE_PITCH_RATIO: f64 = 1.62;

/// Drawing sheet grid reference label font size (mm)
pub const SHEET_REF_FONT: f64 = 1.3;

/// Drawing sheet title block text font size (mm)
pub const SHEET_TEXT_FONT: f64 = 1.5;

/// Drawing sheet title font size (mm)
pub const SHEET_TITLE_FONT: f64 = 2.0;

/// Drawing sheet inner/outer border spacing (mm)
pub const SHEET_BORDER_SPACING: f64 = 2.0;

// ── Backward-compatible aliases ────────────────────────────

pub const WIRE_WIDTH_MM: f64 = WIRE_WIDTH;
pub const JUNCTION_DIAMETER_MM: f64 = JUNCTION_DIAMETER;
pub const JUNCTION_RADIUS_MM: f64 = JUNCTION_DIAMETER / 2.0;
pub const PIN_LENGTH_MM: f64 = PIN_LENGTH;
pub const PIN_WIDTH_MM: f64 = LINE_WIDTH;
pub const PIN_DOT_RADIUS_MM: f64 = TARGET_PIN_RADIUS;
pub const DEFAULT_FONT_SIZE_MM: f64 = TEXT_SIZE;
pub const PIN_NUMBER_FONT_SIZE_MM: f64 = PINNUM_SIZE;
pub const PIN_NAME_FONT_SIZE_MM: f64 = PINNAME_SIZE;
pub const DEFAULT_TEXT_HEIGHT_MM: f64 = TEXT_SIZE;
pub const REFERENCE_TEXT_HEIGHT_MM: f64 = TEXT_SIZE;
pub const VALUE_TEXT_HEIGHT_MM: f64 = TEXT_SIZE;
pub const STROKE_WIDTH_MM: f64 = LINE_WIDTH;

// ── Default colors (KiCad Eeschema theme — kicad-default.ts) ──

/// Wire color: green (theme.wire rgb(0,150,0))
pub fn wire_color() -> Color { Color::from_rgb(0, 150, 0) }
/// Junction color: green (theme.junction rgb(0,150,0))
pub fn junction_color() -> Color { Color::from_rgb(0, 150, 0) }
/// Pin body line color (theme.pin rgb(169,0,0))
pub fn pin_color() -> Color { Color::from_rgb(169, 0, 0) }
/// Pin name/number text color (theme.pin_name rgb(169,0,0))
pub fn pin_text_color() -> Color { Color::from_rgb(169, 0, 0) }
/// Component outline/body stroke color (theme.component_outline rgb(132,0,0))
pub fn component_outline_color() -> Color { Color::from_rgb(132, 0, 0) }
/// Component body fill color: pale yellow (theme.body_rgb)
pub fn component_body_fill() -> Color { Color::from_rgb(255, 255, 194) }
/// Reference text color: teal (theme.reference rgb(0,100,100))
pub fn reference_color() -> Color { Color::from_rgb(0, 100, 100) }
/// Value text color: teal (theme.value rgb(0,100,100))
pub fn value_color() -> Color { Color::from_rgb(0, 100, 100) }
/// Local label text color (theme.label_local rgb(15,15,15))
pub fn label_color() -> Color { Color::from_rgb(15, 15, 15) }
/// Global label color (theme.label_global rgb(132,0,0))
pub fn global_label_color() -> Color { Color::from_rgb(132, 0, 0) }
/// Hierarchical label color (theme.label_hier rgb(114,86,0))
pub fn hier_label_color() -> Color { Color::from_rgb(114, 86, 0) }
/// Grid color (theme.grid rgb(181,181,181))
pub fn grid_color() -> Color { Color::from_rgb(181, 181, 181) }
/// Background color (theme.background rgb(245,244,239))
pub fn background_color() -> Color { Color::from_rgb(245, 244, 239) }
/// Default stroke color for graphic elements
pub fn default_stroke_color() -> Color { Color::black() }
/// Default text color for body text
pub fn default_text_color() -> Color { Color::black() }
/// Drawing sheet border color (theme.worksheet rgb(132,0,0))
pub fn sheet_border_color() -> Color { Color::from_rgb(132, 0, 0) }
/// Drawing sheet text color (theme.worksheet rgb(132,0,0))
pub fn sheet_text_color() -> Color { Color::from_rgb(132, 0, 0) }
/// Schematic text note color (theme.note rgb(0,0,194))
pub fn note_color() -> Color { Color::from_rgb(0, 0, 194) }
/// No-connect marker color (theme.no_connect rgb(0,0,132))
pub fn no_connect_color() -> Color { Color::from_rgb(0, 0, 132) }
/// Bus color (theme.bus rgb(0,0,132))
pub fn bus_color() -> Color { Color::from_rgb(0, 0, 132) }
