//! KiCad Text Markup Parser
//!
//! Parses KiCad text markup syntax:
//! - `^{...}` - Superscript
//! - `_{...}` - Subscript
//! - `~{...}` - Overline (bar above text)
//!
//! Example: "V_{CC}" renders as V with CC subscript
//!          "R^{2}" renders as R with 2 superscript
//!          "~{RESET}" renders as RESET with overline

use std::fmt;

/// Text style for markup
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextStyle {
    /// Normal text
    Normal,
    /// Superscript (raised, smaller)
    Superscript,
    /// Subscript (lowered, smaller)
    Subscript,
    /// Overline (bar above text)
    Overline,
}

/// A segment of parsed text with its style
#[derive(Debug, Clone)]
pub struct TextSegment {
    /// The text content
    pub text: String,
    /// The style of this segment
    pub style: TextStyle,
}

/// Parsed markup result
#[derive(Debug, Clone)]
pub struct ParsedMarkup {
    /// All text segments
    pub segments: Vec<TextSegment>,
}

impl ParsedMarkup {
    /// Create empty markup
    pub fn empty() -> Self {
        Self { segments: Vec::new() }
    }

    /// Create from a single normal text
    pub fn from_text(text: &str) -> Self {
        Self {
            segments: vec![TextSegment {
                text: text.to_string(),
                style: TextStyle::Normal,
            }],
        }
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty() || self.segments.iter().all(|s| s.text.is_empty())
    }

    /// Get plain text (without markup)
    pub fn plain_text(&self) -> String {
        self.segments.iter().map(|s| s.text.as_str()).collect()
    }
}

/// Markup parser state
#[derive(Debug, Clone, Copy)]
enum ParserState {
    /// Normal text
    Normal,
    /// After escape character (^, _, ~)
    AfterEscape(char),
    /// Inside braces
    InBraces { escape_char: char, depth: usize },
}

/// Parse KiCad markup text
pub fn parse_markup(text: &str) -> ParsedMarkup {
    let mut segments: Vec<TextSegment> = Vec::new();
    let mut current_text = String::new();
    let mut state = ParserState::Normal;
    let mut brace_content = String::new();

    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];

        match state {
            ParserState::Normal => {
                match c {
                    '^' | '_' | '~' => {
                        // Check if next char is '{'
                        if i + 1 < chars.len() && chars[i + 1] == '{' {
                            // Save current text if any
                            if !current_text.is_empty() {
                                segments.push(TextSegment {
                                    text: current_text.clone(),
                                    style: TextStyle::Normal,
                                });
                                current_text.clear();
                            }
                            state = ParserState::InBraces {
                                escape_char: c,
                                depth: 1,
                            };
                            i += 2; // Skip escape char and '{'
                            continue;
                        } else {
                            current_text.push(c);
                        }
                    }
                    '\\' => {
                        // Escape next character
                        if i + 1 < chars.len() {
                            current_text.push(chars[i + 1]);
                            i += 2;
                            continue;
                        } else {
                            current_text.push(c);
                        }
                    }
                    _ => {
                        current_text.push(c);
                    }
                }
            }
            ParserState::AfterEscape(escape) => {
                if c == '{' {
                    state = ParserState::InBraces {
                        escape_char: escape,
                        depth: 1,
                    };
                } else {
                    // Not a markup, treat as normal text
                    current_text.push(escape);
                    current_text.push(c);
                    state = ParserState::Normal;
                }
            }
            ParserState::InBraces { escape_char, depth } => {
                match c {
                    '{' => {
                        brace_content.push(c);
                        state = ParserState::InBraces {
                            escape_char,
                            depth: depth + 1,
                        };
                    }
                    '}' => {
                        let new_depth = depth - 1;
                        if new_depth == 0 {
                            // End of markup
                            let style = match escape_char {
                                '^' => TextStyle::Superscript,
                                '_' => TextStyle::Subscript,
                                '~' => TextStyle::Overline,
                                _ => TextStyle::Normal,
                            };
                            segments.push(TextSegment {
                                text: brace_content.clone(),
                                style,
                            });
                            brace_content.clear();
                            state = ParserState::Normal;
                        } else {
                            brace_content.push(c);
                            state = ParserState::InBraces {
                                escape_char,
                                depth: new_depth,
                            };
                        }
                    }
                    _ => {
                        brace_content.push(c);
                    }
                }
            }
        }
        i += 1;
    }

    // Handle remaining content
    match state {
        ParserState::Normal => {
            if !current_text.is_empty() {
                segments.push(TextSegment {
                    text: current_text,
                    style: TextStyle::Normal,
                });
            }
        }
        ParserState::AfterEscape(escape) => {
            // Unterminated escape
            segments.push(TextSegment {
                text: format!("{}{}", escape, current_text),
                style: TextStyle::Normal,
            });
        }
        ParserState::InBraces { escape_char, .. } => {
            // Unterminated braces - treat as literal
            segments.push(TextSegment {
                text: format!("{}{}{}", escape_char, '{', brace_content),
                style: TextStyle::Normal,
            });
        }
    }

    ParsedMarkup { segments }
}

/// Render parsed markup to SVG tspans
pub fn markup_to_svg_tspans(
    markup: &ParsedMarkup,
    base_font_size: f64,
    _base_y: f64,
    color: &str,
) -> String {
    let mut result = String::new();
    let small_size = base_font_size * 0.7;

    for segment in &markup.segments {
        let (font_size, dy) = match segment.style {
            TextStyle::Normal => (base_font_size, 0.0),
            TextStyle::Superscript => (small_size, -base_font_size * 0.4),
            TextStyle::Subscript => (small_size, base_font_size * 0.2),
            TextStyle::Overline => (base_font_size, 0.0),
        };

        // Escape XML special characters
        let escaped = segment.text
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&apos;");

        let style = match segment.style {
            TextStyle::Overline => {
                format!(r#"font-size="{:.1}px" fill="{}" text-decoration="overline""#, font_size, color)
            }
            _ => {
                format!(r#"font-size="{:.1}px" fill="{}""#, font_size, color)
            }
        };

        if dy != 0.0 {
            result.push_str(&format!(
                r#"<tspan {} dy="{:.2}">{}</tspan>"#,
                style, dy, escaped
            ));
        } else {
            result.push_str(&format!(
                r#"<tspan {}>{}</tspan>"#,
                style, escaped
            ));
        }
    }

    result
}

impl fmt::Display for ParsedMarkup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for segment in &self.segments {
            let prefix = match segment.style {
                TextStyle::Normal => "",
                TextStyle::Superscript => "^{",
                TextStyle::Subscript => "_{",
                TextStyle::Overline => "~{",
            };
            let suffix = match segment.style {
                TextStyle::Normal => "",
                _ => "}",
            };
            write!(f, "{}{}{}", prefix, segment.text, suffix)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_normal_text() {
        let markup = parse_markup("Hello World");
        assert_eq!(markup.segments.len(), 1);
        assert_eq!(markup.segments[0].text, "Hello World");
        assert_eq!(markup.segments[0].style, TextStyle::Normal);
    }

    #[test]
    fn test_parse_superscript() {
        let markup = parse_markup("R^{2}");
        assert_eq!(markup.segments.len(), 2);
        assert_eq!(markup.segments[0].text, "R");
        assert_eq!(markup.segments[0].style, TextStyle::Normal);
        assert_eq!(markup.segments[1].text, "2");
        assert_eq!(markup.segments[1].style, TextStyle::Superscript);
    }

    #[test]
    fn test_parse_subscript() {
        let markup = parse_markup("V_{CC}");
        assert_eq!(markup.segments.len(), 2);
        assert_eq!(markup.segments[0].text, "V");
        assert_eq!(markup.segments[0].style, TextStyle::Normal);
        assert_eq!(markup.segments[1].text, "CC");
        assert_eq!(markup.segments[1].style, TextStyle::Subscript);
    }

    #[test]
    fn test_parse_overline() {
        let markup = parse_markup("~{RESET}");
        assert_eq!(markup.segments.len(), 1);
        assert_eq!(markup.segments[0].text, "RESET");
        assert_eq!(markup.segments[0].style, TextStyle::Overline);
    }

    #[test]
    fn test_parse_mixed() {
        let markup = parse_markup("V_{CC}^{2}");
        assert_eq!(markup.segments.len(), 3);
        assert_eq!(markup.segments[0].text, "V");
        assert_eq!(markup.segments[0].style, TextStyle::Normal);
        assert_eq!(markup.segments[1].text, "CC");
        assert_eq!(markup.segments[1].style, TextStyle::Subscript);
        assert_eq!(markup.segments[2].text, "2");
        assert_eq!(markup.segments[2].style, TextStyle::Superscript);
    }

    #[test]
    fn test_parse_nested_braces() {
        let markup = parse_markup("V_{C{C}}");
        // Should handle nested braces correctly
        assert_eq!(markup.segments.len(), 2);
        assert_eq!(markup.segments[1].text, "C{C}");
        assert_eq!(markup.segments[1].style, TextStyle::Subscript);
    }

    #[test]
    fn test_plain_text() {
        let markup = parse_markup("V_{CC} = 5V");
        assert_eq!(markup.plain_text(), "VCC = 5V");
    }

    #[test]
    fn test_svg_output() {
        let markup = parse_markup("V_{CC}");
        let svg = markup_to_svg_tspans(&markup, 1.27, 0.0, "#000000");
        assert!(svg.contains("<tspan"));
        assert!(svg.contains("CC"));
        assert!(svg.contains("font-size"));
    }
}
