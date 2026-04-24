//! Graphic primitives (matching JS `Circle`, `Arc`, `Polyline`, `Polygon` classes)
//!
//! These are the core drawing primitives used by Painters to render schematic elements.

use super::{Color, Point, Matrix, BoundingBox};
use super::types::AngleExt;

/// Stroke style for lines
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrokeStyle {
    Solid,
    Dash,
    Dot,
    DashDot,
    DashDotDot,
}

impl Default for StrokeStyle {
    fn default() -> Self {
        StrokeStyle::Solid
    }
}

impl StrokeStyle {
    /// Convert to SVG dash array string
    pub fn to_svg_dash_array(&self, width: f64) -> Option<String> {
        match self {
            StrokeStyle::Solid => None,
            StrokeStyle::Dash => Some(format!("{}", width * 4.0)),
            StrokeStyle::Dot => Some(format!("{} {}", width, width)),
            StrokeStyle::DashDot => Some(format!("{} {} {} {}", width * 4.0, width, width, width)),
            StrokeStyle::DashDotDot => Some(format!("{} {} {} {} {} {}", width * 4.0, width, width, width, width, width)),
        }
    }
}

/// Stroke configuration
#[derive(Debug, Clone)]
pub struct Stroke {
    pub width: f64,
    pub color: Color,
    pub style: StrokeStyle,
}

impl Stroke {
    pub fn new(width: f64, color: Color) -> Self {
        Self {
            width,
            color,
            style: StrokeStyle::Solid,
        }
    }

    pub fn with_style(mut self, style: StrokeStyle) -> Self {
        self.style = style;
        self
    }
}

/// Fill configuration
#[derive(Debug, Clone)]
pub struct Fill {
    pub color: Option<Color>,
}

impl Fill {
    pub fn none() -> Self {
        Self { color: None }
    }

    pub fn solid(color: Color) -> Self {
        Self { color: Some(color) }
    }
}

/// Circle primitive (matching JS `Circle` class)
#[derive(Debug, Clone)]
pub struct Circle {
    pub center: Point,
    pub radius: f64,
    pub fill: Fill,
    pub stroke: Option<Stroke>,
}

impl Circle {
    pub fn new(center: Point, radius: f64) -> Self {
        Self {
            center,
            radius,
            fill: Fill::none(),
            stroke: None,
        }
    }

    pub fn with_fill(mut self, fill: Fill) -> Self {
        self.fill = fill;
        self
    }

    pub fn with_stroke(mut self, stroke: Stroke) -> Self {
        self.stroke = Some(stroke);
        self
    }

    /// Transform the circle by a matrix
    pub fn transform(&self, matrix: &Matrix) -> Self {
        let new_center = matrix.transform(&self.center);
        // For uniform scaling, use the x-scale factor for radius
        let scale = matrix.scale_factor();
        Self {
            center: new_center,
            radius: self.radius * scale,
            fill: self.fill.clone(),
            stroke: self.stroke.clone(),
        }
    }

    /// Get bounding box
    pub fn bbox(&self) -> BoundingBox {
        BoundingBox::from_min_max(
            self.center.x - self.radius,
            self.center.y - self.radius,
            self.center.x + self.radius,
            self.center.y + self.radius,
        )
    }
}

/// Arc primitive (matching JS `Arc` class)
#[derive(Debug, Clone)]
pub struct Arc {
    pub center: Point,
    pub radius: f64,
    pub start_angle: f64,  // in radians
    pub end_angle: f64,    // in radians
    pub stroke: Stroke,
    pub fill: Fill,
}

impl Arc {
    pub fn new(center: Point, radius: f64, start_angle: f64, end_angle: f64, stroke: Stroke) -> Self {
        Self {
            center,
            radius,
            start_angle,
            end_angle,
            stroke,
            fill: Fill::none(),
        }
    }

    pub fn with_fill(mut self, fill: Fill) -> Self {
        self.fill = fill;
        self
    }

    /// Transform the arc by a matrix
    pub fn transform(&self, matrix: &Matrix) -> Self {
        let new_center = matrix.transform(&self.center);
        let scale = matrix.scale_factor();

        // Transform angles based on matrix rotation
        let rotation = matrix.rotation_angle();
        let new_start = self.start_angle + rotation;
        let new_end = self.end_angle + rotation;

        Self {
            center: new_center,
            radius: self.radius * scale,
            start_angle: new_start,
            end_angle: new_end,
            stroke: self.stroke.clone(),
            fill: self.fill.clone(),
        }
    }

    /// Get start point of the arc
    pub fn start_point(&self) -> Point {
        Point::new(
            self.center.x + self.radius * self.start_angle.cos(),
            self.center.y + self.radius * self.start_angle.sin(),
        )
    }

    /// Get end point of the arc
    pub fn end_point(&self) -> Point {
        Point::new(
            self.center.x + self.radius * self.end_angle.cos(),
            self.center.y + self.radius * self.end_angle.sin(),
        )
    }

    /// Get bounding box (approximate - uses axis-aligned bbox of arc endpoints and center)
    pub fn bbox(&self) -> BoundingBox {
        let mut bbox = BoundingBox::empty();

        // Include center (for full circles)
        bbox.expand_point(self.center.x, self.center.y);

        // Include arc endpoints
        let start = self.start_point();
        let end = self.end_point();
        bbox.expand_point(start.x, start.y);
        bbox.expand_point(end.x, end.y);

        // Include arc extrema if they fall within the arc
        let angles = [0.0, std::f64::consts::FRAC_PI_2, std::f64::consts::PI, std::f64::consts::TAU * 0.75];
        for &angle in &angles {
            if self.angle_in_arc(angle) {
                bbox.expand_point(
                    self.center.x + self.radius * angle.cos(),
                    self.center.y + self.radius * angle.sin(),
                );
            }
        }

        bbox
    }

    /// Check if an angle falls within the arc
    fn angle_in_arc(&self, angle: f64) -> bool {
        let angle = angle.normalize_angle();
        let start = self.start_angle.normalize_angle();
        let end = self.end_angle.normalize_angle();

        if start <= end {
            angle >= start && angle <= end
        } else {
            angle >= start || angle <= end
        }
    }
}

/// Polyline primitive (matching JS `Polyline` class)
#[derive(Debug, Clone)]
pub struct Polyline {
    pub points: Vec<Point>,
    pub stroke: Stroke,
}

impl Polyline {
    pub fn new(points: Vec<Point>, stroke: Stroke) -> Self {
        Self { points, stroke }
    }

    pub fn from_points(points: &[(f64, f64)], stroke: Stroke) -> Self {
        Self {
            points: points.iter().map(|(x, y)| Point::new(*x, *y)).collect(),
            stroke,
        }
    }

    /// Transform the polyline by a matrix
    pub fn transform(&self, matrix: &Matrix) -> Self {
        Self {
            points: self.points.iter().map(|p| matrix.transform(p)).collect(),
            stroke: self.stroke.clone(),
        }
    }

    /// Get bounding box
    pub fn bbox(&self) -> BoundingBox {
        let mut bbox = BoundingBox::empty();
        for point in &self.points {
            bbox.expand_point(point.x, point.y);
        }
        bbox
    }

    /// Get total length of the polyline
    pub fn length(&self) -> f64 {
        let mut total = 0.0;
        for i in 1..self.points.len() {
            let dx = self.points[i].x - self.points[i - 1].x;
            let dy = self.points[i].y - self.points[i - 1].y;
            total += (dx * dx + dy * dy).sqrt();
        }
        total
    }
}

/// Polygon primitive (matching JS `Polygon` class)
#[derive(Debug, Clone)]
pub struct Polygon {
    pub points: Vec<Point>,
    pub fill: Fill,
    pub stroke: Option<Stroke>,
}

impl Polygon {
    pub fn new(points: Vec<Point>) -> Self {
        Self {
            points,
            fill: Fill::none(),
            stroke: None,
        }
    }

    pub fn from_points(points: &[(f64, f64)]) -> Self {
        Self {
            points: points.iter().map(|(x, y)| Point::new(*x, *y)).collect(),
            fill: Fill::none(),
            stroke: None,
        }
    }

    pub fn with_fill(mut self, fill: Fill) -> Self {
        self.fill = fill;
        self
    }

    pub fn with_stroke(mut self, stroke: Stroke) -> Self {
        self.stroke = Some(stroke);
        self
    }

    /// Transform the polygon by a matrix
    pub fn transform(&self, matrix: &Matrix) -> Self {
        Self {
            points: self.points.iter().map(|p| matrix.transform(p)).collect(),
            fill: self.fill.clone(),
            stroke: self.stroke.clone(),
        }
    }

    /// Get bounding box
    pub fn bbox(&self) -> BoundingBox {
        let mut bbox = BoundingBox::empty();
        for point in &self.points {
            bbox.expand_point(point.x, point.y);
        }
        bbox
    }

    /// Check if the polygon is closed (first and last points are the same)
    pub fn is_closed(&self) -> bool {
        if self.points.len() < 2 {
            return false;
        }
        let first = &self.points[0];
        let last = &self.points[self.points.len() - 1];
        (first.x - last.x).abs() < 1e-6 && (first.y - last.y).abs() < 1e-6
    }

    /// Helper to optionally add stroke
    pub fn with_stroke_opt(mut self, stroke: Option<Stroke>) -> Self {
        self.stroke = stroke;
        self
    }
}

/// Rectangle primitive (convenience wrapper around Polygon)
#[derive(Debug, Clone)]
pub struct Rectangle {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub fill: Fill,
    pub stroke: Option<Stroke>,
}

impl Rectangle {
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
            fill: Fill::none(),
            stroke: None,
        }
    }

    pub fn with_fill(mut self, fill: Fill) -> Self {
        self.fill = fill;
        self
    }

    pub fn with_stroke(mut self, stroke: Stroke) -> Self {
        self.stroke = Some(stroke);
        self
    }

    /// Convert to polygon
    pub fn to_polygon(&self) -> Polygon {
        Polygon::from_points(&[
            (self.x, self.y),
            (self.x + self.width, self.y),
            (self.x + self.width, self.y + self.height),
            (self.x, self.y + self.height),
        ])
        .with_fill(self.fill.clone())
        .with_stroke_opt(self.stroke.clone())
    }

    /// Get bounding box
    pub fn bbox(&self) -> BoundingBox {
        BoundingBox::from_min_max(
            self.x,
            self.y,
            self.x + self.width,
            self.y + self.height,
        )
    }
}

/// Bezier curve primitive (matching JS `Bezier` class)
#[derive(Debug, Clone)]
pub struct Bezier {
    pub start: Point,
    pub control1: Point,
    pub control2: Point,
    pub end: Point,
    pub stroke: Stroke,
}

impl Bezier {
    pub fn new(start: Point, control1: Point, control2: Point, end: Point, stroke: Stroke) -> Self {
        Self {
            start,
            control1,
            control2,
            end,
            stroke,
        }
    }

    /// Transform the bezier curve by a matrix
    pub fn transform(&self, matrix: &Matrix) -> Self {
        Self {
            start: matrix.transform(&self.start),
            control1: matrix.transform(&self.control1),
            control2: matrix.transform(&self.control2),
            end: matrix.transform(&self.end),
            stroke: self.stroke.clone(),
        }
    }

    /// Get approximate bounding box using control points
    pub fn bbox(&self) -> BoundingBox {
        let mut bbox = BoundingBox::empty();
        bbox.expand_point(self.start.x, self.start.y);
        bbox.expand_point(self.control1.x, self.control1.y);
        bbox.expand_point(self.control2.x, self.control2.y);
        bbox.expand_point(self.end.x, self.end.y);
        bbox
    }

    /// Convert to SVG path data string
    pub fn to_svg_path(&self) -> String {
        format!(
            "M {} {} C {} {} {} {} {} {}",
            self.start.x, self.start.y,
            self.control1.x, self.control1.y,
            self.control2.x, self.control2.y,
            self.end.x, self.end.y
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_circle_transform() {
        let circle = Circle::new(Point::new(10.0, 20.0), 5.0);
        let matrix = Matrix::translation(100.0, 50.0);
        let transformed = circle.transform(&matrix);

        assert_eq!(transformed.center.x, 110.0);
        assert_eq!(transformed.center.y, 70.0);
        assert_eq!(transformed.radius, 5.0);
    }

    #[test]
    fn test_polyline_bbox() {
        let polyline = Polyline::from_points(&[(0.0, 0.0), (10.0, 20.0), (30.0, 10.0)], Stroke::new(1.0, Color::black()));
        let bbox = polyline.bbox();

        assert_eq!(bbox.min_x(), 0.0);
        assert_eq!(bbox.min_y(), 0.0);
        assert_eq!(bbox.max_x(), 30.0);
        assert_eq!(bbox.max_y(), 20.0);
    }

    #[test]
    fn test_polygon_closed() {
        let closed = Polygon::from_points(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0), (0.0, 0.0)]);
        assert!(closed.is_closed());

        let open = Polygon::from_points(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)]);
        assert!(!open.is_closed());
    }

    #[test]
    fn test_stroke_dash_pattern() {
        let solid = StrokeStyle::Solid;
        assert!(solid.to_svg_dash_array(1.0).is_none());

        let dash = StrokeStyle::Dash;
        assert_eq!(dash.to_svg_dash_array(1.0), Some("4".to_string()));
    }
}
