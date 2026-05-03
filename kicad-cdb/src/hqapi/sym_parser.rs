/// Simple S-expression parser for extracting pin data from .kicad_sym files.
///
/// KiCad symbol files use S-expression format:
/// ```text
/// (pin input (at 0 0) (length 2.54)
///   (name "VIN" (effects (font (size 1.27 1.27))))
///   (number "1" (effects (font (size 1.27 1.27))))
/// )
/// ```

#[derive(Debug, Clone)]
pub struct ExtractedPin {
    pub number: String,
    pub name: String,
    pub pin_type: String,
}

/// KiCad pin electrical types mapping
const PIN_TYPES: &[(&str, &str)] = &[
    ("input", "I"),
    ("output", "O"),
    ("bidirectional", "B"),
    ("tri_state", "T"),
    ("passive", "P"),
    ("unspecified", "U"),
    ("power_in", "W"),
    ("power_out", "w"),
    ("open_collector", "C"),
    ("open_emitter", "E"),
    ("no_connect", "N"),
    ("free", "F"),
    ("inverted", "I"),
];

fn pin_type_kicad(sym_type: &str) -> String {
    for &(name, code) in PIN_TYPES {
        if name == sym_type {
            return code.to_string();
        }
    }
    "U".to_string() // unspecified fallback
}

/// Extract all pins from a .kicad_sym S-expression string.
pub fn extract_pins(sym_text: &str) -> Vec<ExtractedPin> {
    let mut pins = Vec::new();

    // Find all (pin ...) top-level forms within symbol definitions
    // We do a simple token-based scan rather than full recursive parsing
    for pin_expr in find_top_level_exprs(sym_text, "pin") {
        if let Some(pin) = parse_pin_expr(&pin_expr) {
            pins.push(pin);
        }
    }

    pins
}

/// Find all top-level expressions starting with `(tag_name`
fn find_top_level_exprs(text: &str, tag_name: &str) -> Vec<String> {
    let mut results = Vec::new();
    let pattern = format!("({}", tag_name);
    let bytes = text.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        // Search for pattern
        if i + pattern.len() <= bytes.len() && &text[i..i + pattern.len()] == pattern {
            // Check that char before is whitespace or '(' (not inside a word)
            if i > 0 && !bytes[i - 1].is_ascii_whitespace() && bytes[i - 1] != b'(' {
                i += 1;
                continue;
            }
            // Check the char after the tag is whitespace (not "pins" vs "pin")
            let after_end = i + pattern.len();
            if after_end < bytes.len()
                && !bytes[after_end].is_ascii_whitespace()
                && bytes[after_end] != b'('
            {
                i += 1;
                continue;
            }

            // Find matching close paren
            if let Some(end) = find_matching_paren(text, i) {
                results.push(text[i..end].to_string());
                i = end;
            } else {
                i += 1;
            }
        } else {
            i += 1;
        }
    }

    results
}

/// Find the matching closing paren for the opening paren at position `start`
fn find_matching_paren(text: &str, start: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    if bytes[start] != b'(' {
        return None;
    }

    let mut depth = 0;
    let mut in_string = false;
    let mut i = start;

    while i < bytes.len() {
        let ch = bytes[i];

        if in_string {
            if ch == b'"' && (i == 0 || bytes[i - 1] != b'\\') {
                in_string = false;
            }
        } else if ch == b'"' {
            in_string = true;
        } else if ch == b'(' {
            depth += 1;
        } else if ch == b')' {
            depth -= 1;
            if depth == 0 {
                return Some(i + 1);
            }
        }

        i += 1;
    }

    None
}

/// Parse a single `(pin ...)` expression into ExtractedPin
fn parse_pin_expr(expr: &str) -> Option<ExtractedPin> {
    // Pattern: (pin <type> [inverted] (at ...) (length ...) (name "..." ...) (number "..." ...))
    let tokens = tokenize(expr);

    if tokens.len() < 2 || tokens[0] != "pin" {
        return None;
    }

    // Second token is the pin type
    let pin_type = &tokens[1];

    // Find name and number
    let mut name = String::new();
    let mut number = String::new();
    let mut i = 2;

    while i < tokens.len() {
        if tokens[i] == "name" && i + 1 < tokens.len() {
            name = tokens[i + 1].clone();
            i += 2;
        } else if tokens[i] == "number" && i + 1 < tokens.len() {
            number = tokens[i + 1].clone();
            i += 2;
        } else {
            i += 1;
        }
    }

    if number.is_empty() && name.is_empty() {
        return None;
    }

    Some(ExtractedPin {
        number: if number.is_empty() {
            "?".to_string()
        } else {
            number
        },
        name,
        pin_type: pin_type_kicad(pin_type),
    })
}

/// Simple tokenizer for S-expressions — extracts string literals and bare tokens
fn tokenize(expr: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let bytes = expr.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        let ch = bytes[i];

        // Skip whitespace and parens
        if ch.is_ascii_whitespace() || ch == b'(' || ch == b')' {
            i += 1;
            continue;
        }

        // String literal
        if ch == b'"' {
            let start = i + 1;
            i += 1;
            while i < bytes.len() && bytes[i] != b'"' {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    i += 2;
                } else {
                    i += 1;
                }
            }
            tokens.push(expr[start..i].to_string());
            if i < bytes.len() {
                i += 1; // skip closing quote
            }
            continue;
        }

        // Bare token
        let start = i;
        while i < bytes.len()
            && !bytes[i].is_ascii_whitespace()
            && bytes[i] != b'('
            && bytes[i] != b')'
        {
            i += 1;
        }
        tokens.push(expr[start..i].to_string());
    }

    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_matching_paren() {
        let text = "(pin input (name \"A\") (number \"1\"))";
        let end = find_matching_paren(text, 0).unwrap();
        assert_eq!(&text[..end], "(pin input (name \"A\") (number \"1\"))");
    }

    #[test]
    fn test_find_matching_paren_nested() {
        let text = "(symbol (pin input (name \"A\" (effects (font (size 1 1)))) (number \"1\")))";
        // position 8 is '(' before 'pin'
        let end = find_matching_paren(text, 8).unwrap();
        assert!(text[8..end].starts_with("(pin input"));
        assert_eq!(&text[end-1..end], ")");
    }

    #[test]
    fn test_parse_pin_expr() {
        let expr = "(pin input (at -5.08 0 0) (length 2.54)\n  (name \"VIN\" (effects (font (size 1.27 1.27))))\n  (number \"1\" (effects (font (size 1.27 1.27)))))";
        let pin = parse_pin_expr(expr).unwrap();
        assert_eq!(pin.number, "1");
        assert_eq!(pin.name, "VIN");
        assert_eq!(pin.pin_type, "I");
    }

    #[test]
    fn test_parse_power_pin() {
        let expr = "(pin power_in (at 0 5.08 270) (length 2.54)\n  (name \"VCC\" (effects (font (size 1.27 1.27))))\n  (number \"8\" (effects (font (size 1.27 1.27)))))";
        let pin = parse_pin_expr(expr).unwrap();
        assert_eq!(pin.number, "8");
        assert_eq!(pin.name, "VCC");
        assert_eq!(pin.pin_type, "W");
    }

    #[test]
    fn test_extract_pins_full() {
        let sym = r#"(kicad_symbol_lib (version 20211014)
  (symbol "MCP6444" (in_bom yes) (on_board yes)
    (symbol "MCP6444_1_1"
      (pin input (at -5.08 2.54 0) (length 2.54)
        (name "IN+" (effects (font (size 1.27 1.27))))
        (number "1" (effects (font (size 1.27 1.27))))
      )
      (pin output (at 5.08 2.54 180) (length 2.54)
        (name "OUT" (effects (font (size 1.27 1.27))))
        (number "2" (effects (font (size 1.27 1.27))))
      )
      (pin power_in (at 0 5.08 270) (length 2.54)
        (name "VCC" (effects (font (size 1.27 1.27))))
        (number "3" (effects (font (size 1.27 1.27))))
      )
    )
  )
)"#;

        let pins = extract_pins(sym);
        assert_eq!(pins.len(), 3);
        assert_eq!(pins[0].name, "IN+");
        assert_eq!(pins[0].pin_type, "I");
        assert_eq!(pins[1].name, "OUT");
        assert_eq!(pins[1].pin_type, "O");
        assert_eq!(pins[2].name, "VCC");
        assert_eq!(pins[2].pin_type, "W");
    }
}
