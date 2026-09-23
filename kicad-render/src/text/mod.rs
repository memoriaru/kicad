//! Text rendering module
//!
//! Handles KiCad text markup and rendering

mod markup;

pub use markup::{markup_to_svg_tspans, parse_markup, ParsedMarkup, TextSegment, TextStyle};
