//! SVG Renderer implementation

use crate::render_core::{Color, Point, Matrix, BoundingBox};
use crate::render_core::graphics::{Circle, Arc, Polyline, Polygon, Bezier, Stroke, StrokeStyle};
use crate::constants;
use super::{RenderContext, Renderer};

/// SVG Renderer - generates SVG markup
pub struct SvgRenderer {
    /// Output buffer
    output: String,
    /// Current context
    context: RenderContext,
    /// Indentation level
    indent: usize,
    /// Current transform stack
    transform_stack: Vec<Matrix>,
    /// Style state stack (for save/restore)
    state_stack: Vec<RenderState>,
    /// Current stroke style
    stroke: Option<Stroke>,
    /// Current fill color
    fill: Option<Color>,
}

/// Render state for save/restore
#[derive(Debug, Clone)]
struct RenderState {
    transform: Matrix,
    stroke: Option<Stroke>,
    fill: Option<Color>,
}

impl SvgRenderer {
    /// Create a new SVG renderer
    pub fn new() -> Self {
        Self {
            output: String::new(),
            context: RenderContext::new(BoundingBox::empty(), 1.0),
            indent: 0,
            transform_stack: vec![Matrix::identity()],
            state_stack: Vec::new(),
            stroke: None,
            fill: None,
        }
    }

    /// Create with initial context
    pub fn with_context(context: RenderContext) -> Self {
        Self {
            output: String::new(),
            context,
            indent: 0,
            transform_stack: vec![Matrix::identity()],
            state_stack: Vec::new(),
            stroke: None,
            fill: None,
        }
    }

    /// Get the final SVG output
    pub fn output(&self) -> String {
        self.output.clone()
    }

    /// Get current indentation string
    fn indent(&self) -> String {
        "  ".repeat(self.indent)
    }

    /// Convert color to SVG color string
    fn color_to_svg(&self, color: &Color) -> String {
        color.to_css()
    }

    /// Get the scale factor from the current transform
    fn current_scale(&self) -> f64 {
        self.current_transform().scale_factor()
    }

    /// Convert stroke to SVG attributes, scaling width by current transform's scale.
    fn stroke_to_attrs_scaled(&self, stroke: &Stroke, scale: f64) -> String {
        let width = stroke.width * scale;
        let mut attrs = format!(
            r#"stroke="{}" stroke-width="{:.2}""#,
            self.color_to_svg(&stroke.color),
            width
        );

        if stroke.style != StrokeStyle::Solid {
            if let Some(dash) = stroke.style.to_svg_dash_array(width) {
                attrs.push_str(&format!(r#" stroke-dasharray="{}""#, dash));
            }
        }

        attrs
    }
}

impl Renderer for SvgRenderer {
    fn context(&self) -> &RenderContext {
        &self.context
    }

    fn save(&mut self) {
        let state = RenderState {
            transform: self.current_transform(),
            stroke: self.stroke.clone(),
            fill: self.fill,
        };
        self.state_stack.push(state);
    }

    fn restore(&mut self) {
        if let Some(state) = self.state_stack.pop() {
            self.stroke = state.stroke;
            self.fill = state.fill;
        }
        self.transform_stack.pop();
    }

    fn set_transform(&mut self, transform: &Matrix) {
        self.transform_stack.push(transform.clone());
    }

    fn draw_circle(&mut self, circle: &Circle) {
        let transform = self.current_transform();
        let transformed = circle.transform(&transform);
        let scale = transform.scale_factor();

        let mut attrs = String::new();
        if let Some(ref fill) = &transformed.fill.color {
            attrs.push_str(&format!(r#" fill="{}""#, self.color_to_svg(fill)));
        } else {
            attrs.push_str(r#" fill="none""#);
        }
        if let Some(ref stroke) = &transformed.stroke {
            attrs.push(' ');
            attrs.push_str(&self.stroke_to_attrs_scaled(stroke, scale));
        }

        self.output.push_str(&format!(
            r#"{}<circle cx="{:.2}" cy="{:.2}" r="{:.2}" {}/>"#,
            self.indent(),
            transformed.center.x,
            transformed.center.y,
            transformed.radius,
            attrs.trim()
        ));
    }

    fn draw_arc(&mut self, arc: &Arc) {
        let transform = self.current_transform();
        let transformed = arc.transform(&transform);
        let scale = transform.scale_factor();

        let path = self.arc_to_svg_path(&transformed);
        let attrs = self.stroke_to_attrs_scaled(&transformed.stroke, scale);

        // If the arc has a fill, close the path (pie slice: arc + line to center)
        let (d_attr, fill_attr) = if let Some(ref fill_color) = transformed.fill.color {
            let center = transformed.center;
            (format!("{} L {:.2} {:.2} Z", path, center.x, center.y),
             format!(r#" fill="{}""#, self.color_to_svg(fill_color)))
        } else {
            (path, String::new())
        };

        self.output.push_str(&format!(
            r#"{}<path d="{}"{} {}/>"#,
            self.indent(),
            d_attr,
            fill_attr,
            attrs.trim()
        ));
    }

    fn draw_polyline(&mut self, polyline: &Polyline) {
        let transform = self.current_transform();
        let transformed = polyline.transform(&transform);
        let scale = transform.scale_factor();

        let points: String = transformed.points.iter()
            .map(|p| format!("{:.2},{:.2}", p.x, p.y))
            .collect::<Vec<_>>()
            .join(" ");

        let attrs = self.stroke_to_attrs_scaled(&transformed.stroke, scale);

        self.output.push_str(&format!(
            r#"{}<polyline points="{}" fill="none" {}/>"#,
            self.indent(),
            points,
            attrs.trim()
        ));
    }

    fn draw_polygon(&mut self, polygon: &Polygon) {
        let transform = self.current_transform();
        let transformed = polygon.transform(&transform);
        let scale = transform.scale_factor();

        let points: String = transformed.points.iter()
            .map(|p| format!("{:.2},{:.2}", p.x, p.y))
            .collect::<Vec<_>>()
            .join(" ");

        let mut attrs = String::new();
        if let Some(ref fill) = &transformed.fill.color {
            attrs.push_str(&format!(r#" fill="{}""#, self.color_to_svg(fill)));
        } else {
            attrs.push_str(r#" fill="none""#);
        }
        if let Some(ref stroke) = &transformed.stroke {
            attrs.push(' ');
            attrs.push_str(&self.stroke_to_attrs_scaled(stroke, scale));
        }

        self.output.push_str(&format!(
            r#"{}<polygon points="{}" {}/>"#,
            self.indent(),
            points,
            attrs.trim()
        ));
    }

    fn draw_bezier(&mut self, bezier: &Bezier) {
        let transform = self.current_transform();
        let transformed = bezier.transform(&transform);
        let scale = transform.scale_factor();

        let path = transformed.to_svg_path();
        let attrs = self.stroke_to_attrs_scaled(&transformed.stroke, scale);

        self.output.push_str(&format!(
            r#"{}<path d="{}" {}/>"#,
            self.indent(),
            path,
            attrs.trim()
        ));
    }

    fn draw_text(&mut self, position: &Point, text: &str, font_size: f64, color: &Color, bold: bool, rotation: f64, text_anchor: &str, dominant_baseline: &str) {
        let transform = self.current_transform();
        let transformed_pos = transform.transform(position);
        let scale = transform.scale_factor();
        let scaled_font_size = font_size * scale * constants::SVG_FONT_SCALE;
        let color_str = self.color_to_svg(color);

        // Build optional rotation transform for the text element
        let rotation_attr = if rotation.abs() > 0.01 {
            format!(r#" transform="rotate({:.2},{:.2},{:.2})""#, rotation, transformed_pos.x, transformed_pos.y)
        } else {
            String::new()
        };

        // Build optional alignment attributes
        let mut align_attrs = String::new();
        if !text_anchor.is_empty() && text_anchor != "start" {
            align_attrs.push_str(&format!(r#" text-anchor="{}""#, text_anchor));
        }
        if !dominant_baseline.is_empty() {
            align_attrs.push_str(&format!(r#" dominant-baseline="{}""#, dominant_baseline));
        }

        if text.contains("^{") || text.contains("_{") || text.contains("~{") {
            use crate::text::parse_markup;
            use crate::text::markup_to_svg_tspans;

            let parsed = parse_markup(text);
            let tspans = markup_to_svg_tspans(&parsed, scaled_font_size, transformed_pos.y, &color_str);

            self.output.push_str(&format!(
                r#"{}<text x="{:.2}" y="{:.2}"{}{}>{}</text>"#,
                self.indent(),
                transformed_pos.x,
                transformed_pos.y,
                rotation_attr,
                align_attrs,
                tspans
            ));
        } else {
            let escaped = text
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('"', "&quot;")
                .replace('\'', "&apos;");

            let bold_attr = if bold { r#" font-weight="bold""# } else { "" };
            self.output.push_str(&format!(
                r#"{}<text x="{:.2}" y="{:.2}" fill="{}" font-size="{:.2}px"{}{}{}>{}</text>"#,
                self.indent(),
                transformed_pos.x,
                transformed_pos.y,
                color_str,
                scaled_font_size,
                bold_attr,
                rotation_attr,
                align_attrs,
                escaped
            ));
        }
    }
}

impl SvgRenderer {
    /// Get current combined transform
    fn current_transform(&self) -> Matrix {
        self.transform_stack.iter().cloned().fold(Matrix::identity(), |a, b| a.multiply(&b))
    }

    /// Convert arc to SVG path data using proper SVG arc command.
    fn arc_to_svg_path(&self, arc: &Arc) -> String {
        let start = arc.start_point();
        let end = arc.end_point();

        let mut sweep = arc.end_angle - arc.start_angle;
        while sweep > 2.0 * std::f64::consts::PI { sweep -= 2.0 * std::f64::consts::PI; }
        while sweep < -2.0 * std::f64::consts::PI { sweep += 2.0 * std::f64::consts::PI; }

        let large_arc = sweep.abs() > std::f64::consts::PI;
        let sweep_flag = if sweep >= 0.0 { 1 } else { 0 };
        let large_arc_flag = if large_arc { 1 } else { 0 };

        format!(
            "M {:.2} {:.2} A {:.2} {:.2} 0 {} {} {:.2} {:.2}",
            start.x, start.y,
            arc.radius,
            arc.radius,
            large_arc_flag,
            sweep_flag,
            end.x, end.y
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render_core::graphics::Fill;

    #[test]
    fn test_svg_circle() {
        let mut renderer = SvgRenderer::new();
        let circle = Circle::new(Point::new(10.0, 20.0), 5.0)
            .with_fill(Fill::solid(Color::red()));

        renderer.draw_circle(&circle);
        let output = renderer.output();

        assert!(output.contains("<circle"));
        assert!(output.contains("cx=\"10.00\""));
        assert!(output.contains("cy=\"20.00\""));
        assert!(output.contains("r=\"5.00\""));
        assert!(output.contains("fill=\"#ff0000\""));
    }

    #[test]
    fn test_svg_polyline() {
        let mut renderer = SvgRenderer::new();
        let polyline = Polyline::from_points(&[(0.0, 0.0), (10.0, 20.0)], Stroke::new(1.0, Color::black()));

        renderer.draw_polyline(&polyline);
        let output = renderer.output();

        assert!(output.contains("<polyline"));
        assert!(output.contains("points=\"0.00,0.00 10.00,20.00\""));
    }
}
