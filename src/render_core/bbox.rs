//! Bounding Box (matching JS `ne` class)

use super::Point;

/// 2D Bounding Box
#[derive(Debug, Clone, Copy)]
pub struct BoundingBox {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Default for BoundingBox {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
        }
    }
}

impl BoundingBox {
    /// Create a new bounding box
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }

    /// Create empty bounding box for accumulating points
    pub fn empty() -> Self {
        Self {
            x: f64::NAN,
            y: f64::NAN,
            w: f64::NAN,
            h: f64::NAN,
        }
    }

    /// Check if this is the empty sentinel (not initialized)
    fn is_uninitialized(&self) -> bool {
        self.x.is_nan() || self.y.is_nan()
    }

    /// Create from min/max coordinates
    pub fn from_min_max(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Self {
        Self {
            x: min_x,
            y: min_y,
            w: if max_x > min_x { max_x - min_x } else { 0.0 },
            h: if max_y > min_y { max_y - min_y } else { 0.0 },
        }
    }

    /// Create from two corner points
    pub fn from_points(p1: &Point, p2: &Point) -> Self {
        Self {
            x: p1.x.min(p2.x),
            y: p1.y.min(p2.y),
            w: (p1.x - p2.x).abs(),
            h: (p1.y - p2.y).abs(),
        }
    }

    /// Check if box is empty
    pub fn is_empty(&self) -> bool {
        self.w <= 0.0 || self.h <= 0.0
    }

    /// Expand the bounding box to include a point
    pub fn expand_point(&mut self, px: f64, py: f64) {
        if self.is_uninitialized() {
            // First point - create zero-area bbox at this point
            self.x = px;
            self.y = py;
            self.w = 0.0;
            self.h = 0.0;
            return;
        }

        // Expand to include the new point
        let new_min_x = self.x.min(px);
        let new_min_y = self.y.min(py);
        let new_max_x = (self.x + self.w).max(px);
        let new_max_y = (self.y + self.h).max(py);

        self.x = new_min_x;
        self.y = new_min_y;
        self.w = new_max_x - new_min_x;
        self.h = new_max_y - new_min_y;
    }

    /// Expand to include another bounding box
    pub fn expand(&mut self, other: &BoundingBox) {
        if other.is_uninitialized() || other.is_empty() {
            return;
        }
        if self.is_uninitialized() {
            *self = other.clone();
            return;
        }
        if self.is_empty() {
            *self = other.clone();
            return;
        }

        let self_max_x = self.x + self.w;
        let self_max_y = self.y + self.h;
        let other_max_x = other.x + other.w;
        let other_max_y = other.y + other.h;

        let new_min_x = self.x.min(other.x);
        let new_min_y = self.y.min(other.y);
        let new_max_x = self_max_x.max(other_max_x);
        let new_max_y = self_max_y.max(other_max_y);

        self.x = new_min_x;
        self.y = new_min_y;
        self.w = new_max_x - new_min_x;
        self.h = new_max_y - new_min_y;
    }

    /// Get min x coordinate
    pub fn min_x(&self) -> f64 {
        self.x
    }

    /// Get min y coordinate
    pub fn min_y(&self) -> f64 {
        self.y
    }

    /// Get max x coordinate
    pub fn max_x(&self) -> f64 {
        self.x + self.w
    }

    /// Get max y coordinate
    pub fn max_y(&self) -> f64 {
        self.y + self.h
    }

    /// Get center point
    pub fn center(&self) -> Point {
        Point::new(self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    /// Get width
    pub fn width(&self) -> f64 {
        self.w
    }

    /// Get height
    pub fn height(&self) -> f64 {
        self.h
    }

    /// Add padding around the box
    pub fn with_padding(&self, padding: f64) -> Self {
        Self {
            x: self.x - padding,
            y: self.y - padding,
            w: self.w + 2.0 * padding,
            h: self.h + 2.0 * padding,
        }
    }
}

