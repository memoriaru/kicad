//! Camera and Transform utilities (matching JS `Camera2` and `Transform` classes)

use super::{Matrix, Point};

/// Camera for viewport management (matching JS `Camera2` class)
#[derive(Debug, Clone)]
pub struct Camera2 {
    /// Viewport size in pixels
    pub viewport_size: Point,
    /// Center of the view in world coordinates
    pub center: Point,
    /// Zoom level
    pub zoom: f64,
    /// Rotation angle
    pub rotation: f64,
}

impl Default for Camera2 {
    fn default() -> Self {
        Self {
            viewport_size: Point::zero(),
            center: Point::zero(),
            zoom: 1.0,
            rotation: 0.0,
        }
    }
}

impl Camera2 {
    pub fn new(viewport_size: Point, center: Point, zoom: f64) -> Self {
        Self {
            viewport_size,
            center,
            zoom,
            rotation: 0.0,
        }
    }

    /// Translate the camera by a delta
    pub fn translate(&mut self, delta: &Point) {
        self.center = self.center.add(delta);
    }

    /// Rotate the camera by an angle (radians)
    pub fn rotate(&mut self, angle: f64) {
        self.rotation += angle;
    }

    /// Get the transformation matrix for this camera
    pub fn matrix(&self) -> Matrix {
        let tx = self.viewport_size.x / 2.0;
        let ty = self.viewport_size.y / 2.0;
        let r = self.center.x - self.center.x * self.zoom;
        let i = self.center.y - self.center.y * self.zoom;
        let s = -(self.center.x - tx) + r;
        let o = -(self.center.y - ty) + i;

        // Build matrix: translate -> rotate -> scale
        Matrix::translation(s, o)
            .multiply(&Matrix::rotation(self.rotation))
            .multiply(&Matrix::uniform_scaling(self.zoom))
    }

    /// Get the bounding box in world coordinates
    pub fn bbox(&self) -> super::BoundingBox {
        let inv = self.matrix().inverse().unwrap_or(Matrix::identity());
        let p1 = inv.transform(&Point::new(0.0, 1.0));
        let p2 = inv.transform(&Point::new(self.viewport_size.x, self.viewport_size.y));
        super::BoundingBox::from_points(&p1, &p2)
    }

    /// Set bounding box
    pub fn set_bbox(&mut self, bbox: &super::BoundingBox) {
        let ex = self.viewport_size.x / bbox.w;
        let ey = self.viewport_size.y / bbox.h;
        let cx = bbox.x + bbox.w / 2.0;
        let cy = bbox.y + bbox.h / 2.0;

        self.zoom = ex.min(ey);
        self.center = Point::new(cx, cy);
    }

    /// Get the top edge of viewport in world coordinates
    pub fn top(&self) -> f64 {
        self.bbox().y
    }

    /// Get the bottom edge of viewport in world coordinates
    pub fn bottom(&self) -> f64 {
        self.bbox().y + self.bbox().h
    }

    /// Get the left edge of viewport in world coordinates
    pub fn left(&self) -> f64 {
        self.bbox().x
    }

    /// Get the right edge of viewport in world coordinates
    pub fn right(&self) -> f64 {
        self.bbox().x + self.bbox().w
    }

    /// Apply camera transformation to a Canvas context
    #[cfg(feature = "wasm")]
    pub fn apply_to_canvas(&self, ctx: &web_sys::CanvasRenderingContext2d) {
        self.viewport_size.set(ctx.canvas.width() as f64, ctx.canvas.height() as f64);

        let current_transform = Matrix::from_dom_matrix(&ctx.get_transform());
        current_transform.multiply_self(&self.matrix());

        ctx.set_transform(&current_transform.to_dom_matrix());
    }

    /// Convert screen coordinates to world coordinates
    pub fn screen_to_world(&self, screen: &Point) -> Point {
        match self.matrix().inverse() {
            Some(matrix) => matrix.transform(screen),
            None => Point::zero(),
        }
    }

    /// Convert world coordinates to screen coordinates
    pub fn world_to_screen(&self, world: &Point) -> Point {
        self.matrix().transform(world)
    }
}

/// 2D Transform utility (matching JS transform functions)
pub struct Transform2D {
    /// Scale factor (pixels per KiCad unit)
    pub scale: f64,
    /// X offset for SVG viewBox
    pub offset_x: f64,
    /// Y offset for SVG viewBox (after flip)
    pub offset_y: f64,
    /// Width of the SVG viewport
    pub width: f64,
    /// Height of the SVG viewport (for Y flip)
    pub height: f64,
    /// Min Y value (for Y-flip calculation)
    pub min_y: f64,
    /// Max Y value (for Y-flip calculation)
    pub max_y: f64,
}

impl Default for Transform2D {
    fn default() -> Self {
        Self {
            scale: 1.0,
            offset_x: 0.0,
            offset_y: 0.0,
            width: 0.0,
            height: 0.0,
            min_y: 0.0,
            max_y: 0.0,
        }
    }
}

impl Transform2D {
    /// Create a new transform
    pub fn new(scale: f64, min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Self {
        let width = (max_x - min_x) * scale;
        let height = (max_y - min_y) * scale;
        Self {
            scale,
            offset_x: -min_x * scale,
            offset_y: -min_y * scale,
            width,
            height,
            min_y,
            max_y,
        }
    }

    /// Transform a point from KiCad to SVG coordinates (no Y-flip)
    pub fn transform_point(&self, x: f64, y: f64) -> (f64, f64) {
        let svg_x = x * self.scale + self.offset_x;
        let svg_y = y * self.scale + self.offset_y;
        (svg_x, svg_y)
    }

    /// Transform a point with Y-flip (for wires, labels, etc.)
    pub fn transform_point_with_flip(&self, x: f64, y: f64) -> (f64, f64) {
        let svg_x = x * self.scale + self.offset_x;
        // Y-flip: invert Y relative to the bounding box
        let svg_y = (self.max_y - y) * self.scale + self.offset_y;
        (svg_x, svg_y)
    }

    /// Transform a distance (scale only)
    pub fn transform_distance(&self, d: f64) -> f64 {
        d * self.scale
    }

    /// Transform a size (width, height)
    pub fn transform_size(&self, width: f64, height: f64) -> (f64, f64) {
        (width * self.scale, height * self.scale)
    }

    /// Transform an angle for SVG (accounting for Y flip)
    pub fn transform_angle(&self, angle: f64) -> f64 {
        -angle
    }

    /// Get the viewBox string for SVG
    pub fn view_box(&self, width: f64, height: f64) -> String {
        format!("0 0 {} {}", width, height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transform_point_with_flip() {
        let t = Transform2D::new(1.0, 0.0, 0.0, 100.0, 100.0);

        // Point at (0, 100) should flip to (0, 0)
        let (x, y) = t.transform_point_with_flip(0.0, 100.0);
        assert_eq!(x, 0.0);
        assert_eq!(y, 0.0);

        // Point at (0, 0) should flip to (0, 100)
        let (x, y) = t.transform_point_with_flip(0.0, 0.0);
        assert_eq!(x, 0.0);
        assert_eq!(y, 100.0);
    }

    #[test]
    fn test_camera_matrix() {
        let camera = Camera2::new(
            Point::new(100.0, 100.0),
            Point::new(50.0, 50.0),
            2.0,
        );

        let matrix = camera.matrix();
        // Matrix should scale by 2.0
        assert_eq!(matrix.elements[0], 2.0); // a = scale
        assert_eq!(matrix.elements[3], 2.0); // d = scale
    }
}
