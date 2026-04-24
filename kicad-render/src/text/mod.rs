//! Text rendering module
//!
//! Handles KiCad text markup and rendering

mod markup;

pub use markup::{parse_markup, markup_to_svg_tspans, ParsedMarkup, TextSegment, TextStyle};
