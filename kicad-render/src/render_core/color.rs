//! Color representation (matching JS `N` class)

/// RGBA Color (all values in 0.0-1.0 range)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

impl Default for Color {
    fn default() -> Self {
        Self::black()
    }
}

impl Color {
    /// Create from RGB values (0-1 range), alpha defaults to 1.0
    pub fn new(r: f64, g: f64, b: f64) -> Self {
        Self { r, g, b, a: 1.0 }
    }

    /// Create with alpha
    pub fn with_alpha(r: f64, g: f64, b: f64, a: f64) -> Self {
        Self { r, g, b, a }
    }

    /// Create from RGB u8 values (0-255 range)
    pub fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        Self::new(r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0)
    }

    /// Create from RGBA u8 values (0-255 range)
    pub fn from_rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self::with_alpha(
            r as f64 / 255.0,
            g as f64 / 255.0,
            b as f64 / 255.0,
            a as f64 / 255.0,
        )
    }

    /// Black color
    pub fn black() -> Self {
        Self::new(0.0, 0.0, 0.0)
    }

    /// White color
    pub fn white() -> Self {
        Self::new(1.0, 1.0, 1.0)
    }

    /// Red color
    pub fn red() -> Self {
        Self::new(1.0, 0.0, 0.0)
    }

    /// Green color
    pub fn green() -> Self {
        Self::new(0.0, 1.0, 0.0)
    }

    /// Dark green (wire color in KiCad)
    pub fn dark_green() -> Self {
        Self::new(0.0, 0.5, 0.0)
    }

    /// Cyan color (used for Reference/Value)
    pub fn cyan() -> Self {
        Self::from_rgb(81, 255, 159)
    }

    /// Light blue color
    pub fn light_blue() -> Self {
        Self::from_rgb(172, 230, 255)
    }

    /// Blue color
    pub fn blue() -> Self {
        Self::new(0.0, 0.0, 1.0)
    }

    /// Yellow color
    pub fn yellow() -> Self {
        Self::new(1.0, 1.0, 0.0)
    }

    /// Gray color
    pub fn gray() -> Self {
        Self::from_rgb(200, 200, 200)
    }

    /// Dark gray color
    pub fn dark_gray() -> Self {
        Self::from_rgb(100, 100, 100)
    }

    /// Light gray (transparent)
    pub fn light_gray() -> Self {
        Self::with_alpha(0.5, 0.5, 0.5, 0.2)
    }

    /// Transparent black
    pub fn transparent_black() -> Self {
        Self::with_alpha(0.0, 0.0, 0.0, 0.0)
    }

    /// Parse from CSS color string
    pub fn from_css(css: &str) -> Self {
        let css = css.trim();

        // Handle hex format
        if css.starts_with('#') {
            return Self::from_hex(&css[1..]);
        }

        // Handle rgb/rgba format
        if css.starts_with("rgb") {
            return Self::from_rgb_string(css);
        }

        // Handle named colors
        Self::from_named_color(css)
    }

    /// Parse hex color string
    fn from_hex(hex: &str) -> Self {
        let hex = hex.trim();

        let (r, g, b, a) = match hex.len() {
            3 => {
                // #RGB -> #RRGGBB
                let r = u8::from_str_radix(&format!("{}{}", &hex[0..1], &hex[0..1]), 16).unwrap_or(0);
                let g = u8::from_str_radix(&format!("{}{}", &hex[1..2], &hex[1..2]), 16).unwrap_or(0);
                let b = u8::from_str_radix(&format!("{}{}", &hex[2..3], &hex[2..3]), 16).unwrap_or(0);
                (r, g, b, 255u8)
            }
            6 => {
                // #RRGGBB
                let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
                let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
                let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
                (r, g, b, 255u8)
            }
            8 => {
                // #RRGGBBAA
                let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
                let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
                let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
                let a = u8::from_str_radix(&hex[6..8], 16).unwrap_or(255);
                (r, g, b, a)
            }
            _ => return Self::black(),
        };

        Self::from_rgba(r, g, b, a)
    }

    /// Parse rgb/rgba string
    fn from_rgb_string(css: &str) -> Self {
        let is_rgba = css.starts_with("rgba");
        let content = if is_rgba {
            css.trim_start_matches("rgba")
        } else {
            css.trim_start_matches("rgb")
        };

        let content = content.trim_start_matches('(').trim_end_matches(')').trim();
        let parts: Vec<&str> = content.split(',').map(|s| s.trim()).collect();

        if parts.len() < 3 {
            return Self::black();
        }

        let parse_val = |s: &str, max: f64| -> f64 {
            let s = s.trim();
            if s.ends_with('%') {
                s.trim_end_matches('%').parse::<f64>().unwrap_or(0.0) / 100.0
            } else {
                s.parse::<f64>().unwrap_or(0.0) / max
            }
        };

        let r = parse_val(parts[0], 255.0);
        let g = parse_val(parts[1], 255.0);
        let b = parse_val(parts[2], 255.0);
        let a = if is_rgba && parts.len() > 3 {
            parse_val(parts[3], 1.0)
        } else {
            1.0
        };

        Self::with_alpha(r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0), a.clamp(0.0, 1.0))
    }

    /// Parse named color
    fn from_named_color(name: &str) -> Self {
        match name.to_lowercase().as_str() {
            "black" => Self::black(),
            "white" => Self::white(),
            "red" => Self::red(),
            "green" => Self::green(),
            "blue" => Self::blue(),
            "yellow" => Self::yellow(),
            "cyan" => Self::cyan(),
            "gray" | "grey" => Self::gray(),
            "none" | "transparent" => Self::transparent_black(),
            _ => Self::black(),
        }
    }

    /// Convert to CSS color string
    pub fn to_css(&self) -> String {
        if self.a < 1.0 {
            format!(
                "rgba({},{},{},{:.2})",
                self.r_255(),
                self.g_255(),
                self.b_255(),
                self.a
            )
        } else {
            format!("#{:02x}{:02x}{:02x}", self.r_255(), self.g_255(), self.b_255())
        }
    }

    /// Create a copy
    pub fn copy(&self) -> Self {
        *self
    }

    /// Get grayscale version
    pub fn grayscale(&self) -> Self {
        let gray = self.r * 0.299 + self.g * 0.587 + self.b * 0.114;
        Self::with_alpha(gray, gray, gray, self.a)
    }

    /// Mix with another color
    pub fn mix(&self, other: &Color, ratio: f64) -> Self {
        let ratio = ratio.clamp(0.0, 1.0);
        Self::with_alpha(
            other.r * (1.0 - ratio) + self.r * ratio,
            other.g * (1.0 - ratio) + self.g * ratio,
            other.b * (1.0 - ratio) + self.b * ratio,
            self.a,
        )
    }

    /// Create with modified alpha
    pub fn set_alpha(&self, a: f64) -> Self {
        Self {
            r: self.r,
            g: self.g,
            b: self.b,
            a,
        }
    }

    /// Desaturate (remove color saturation)
    pub fn desaturate(&self) -> Self {
        if (self.r - self.g).abs() < 1e-6 && (self.r - self.b).abs() < 1e-6 {
            return *self;
        }
        // Simple desaturation: average RGB and use that for all channels
        let avg = (self.r + self.g + self.b) / 3.0;
        Self::with_alpha(avg, avg, avg, self.a)
    }

    /// Check if transparent black
    pub fn is_transparent_black(&self) -> bool {
        self.r == 0.0 && self.g == 0.0 && self.b == 0.0 && self.a == 0.0
    }

    /// Get red component as 0-255
    pub fn r_255(&self) -> u8 {
        (self.r * 255.0).round() as u8
    }

    /// Get green component as 0-255
    pub fn g_255(&self) -> u8 {
        (self.g * 255.0).round() as u8
    }

    /// Get blue component as 0-255
    pub fn b_255(&self) -> u8 {
        (self.b * 255.0).round() as u8
    }

    /// Get alpha component as 0-255
    pub fn a_255(&self) -> u8 {
        (self.a * 255.0).round() as u8
    }

    /// Check if fully opaque
    pub fn is_opaque(&self) -> bool {
        self.a >= 1.0
    }

    /// Check if fully transparent
    pub fn is_transparent(&self) -> bool {
        self.a <= 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_color_from_hex() {
        let color = Color::from_css("#ff0000");
        assert_eq!(color.r_255(), 255);
        assert_eq!(color.g_255(), 0);
        assert_eq!(color.b_255(), 0);

        let color = Color::from_css("#00ff00");
        assert_eq!(color.r_255(), 0);
        assert_eq!(color.g_255(), 255);
        assert_eq!(color.b_255(), 0);

        let color = Color::from_css("#0000ff");
        assert_eq!(color.r_255(), 0);
        assert_eq!(color.g_255(), 0);
        assert_eq!(color.b_255(), 255);
    }

    #[test]
    fn test_color_to_css() {
        let color = Color::new(1.0, 0.5, 0.25);
        assert_eq!(color.to_css(), "#ff8040");

        let color = Color::with_alpha(1.0, 0.5, 0.25, 0.5);
        assert!(color.to_css().contains("rgba"));
    }

    #[test]
    fn test_color_grayscale() {
        let color = Color::red();
        let gray = color.grayscale();
        // Red should convert to grayscale
        assert!((gray.r - gray.g).abs() < 0.01);
        assert!((gray.g - gray.b).abs() < 0.01);
    }

    #[test]
    fn test_color_from_rgb() {
        let color = Color::from_rgb(255, 128, 64);
        assert_eq!(color.r_255(), 255);
        assert_eq!(color.g_255(), 128);
        assert_eq!(color.b_255(), 64);
        assert!(color.is_opaque());
    }
}
