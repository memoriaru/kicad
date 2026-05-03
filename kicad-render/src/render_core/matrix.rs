//! 2D Transformation Matrix (matching JS `fe` class)
//!
//! Matrix is stored in row-major format as [a, b, c, d, e, f]
//! corresponding to:
//! ```text
//! | a  c  e |
//! | b  d  f |
//! | 0  0  1 |
//! ```

use super::Point;

/// 2D Transformation Matrix
#[derive(Debug, Clone)]
pub struct Matrix {
    /// Matrix elements [a, b, c, d, e, f]
    /// where the matrix is:
    /// | a  c  e |
    /// | b  d  f |
    /// | 0  0  1 |
    pub elements: [f64; 6],
}

impl Default for Matrix {
    fn default() -> Self {
        Self::identity()
    }
}

impl Matrix {
    /// Create identity matrix
    pub fn identity() -> Self {
        Self {
            elements: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        }
    }

    /// Create from elements [a, b, c, d, e, f]
    pub fn new(elements: [f64; 6]) -> Self {
        Self { elements }
    }

    /// Create translation matrix
    pub fn translation(tx: f64, ty: f64) -> Self {
        Self {
            elements: [1.0, 0.0, 0.0, 1.0, tx, ty],
        }
    }

    /// Create scaling matrix
    pub fn scaling(sx: f64, sy: f64) -> Self {
        Self {
            elements: [sx, 0.0, 0.0, sy, 0.0, 0.0],
        }
    }

    /// Create uniform scaling matrix
    pub fn uniform_scaling(s: f64) -> Self {
        Self::scaling(s, s)
    }

    /// Create rotation matrix (angle in radians)
    pub fn rotation(angle: f64) -> Self {
        let cos_a = angle.cos();
        let sin_a = angle.sin();
        Self {
            elements: [cos_a, sin_a, -sin_a, cos_a, 0.0, 0.0],
        }
    }

    /// Create rotation matrix around a point
    pub fn rotation_around(angle: f64, center: &Point) -> Self {
        let cos_a = angle.cos();
        let sin_a = angle.sin();
        let tx = center.x - center.x * cos_a + center.y * sin_a;
        let ty = center.y - center.x * sin_a - center.y * cos_a;
        Self {
            elements: [cos_a, sin_a, -sin_a, cos_a, tx, ty],
        }
    }

    /// Copy matrix
    pub fn copy(&self) -> Self {
        Self {
            elements: self.elements,
        }
    }

    /// Transform a point
    pub fn transform(&self, point: &Point) -> Point {
        Point::new(
            self.elements[0] * point.x + self.elements[2] * point.y + self.elements[4],
            self.elements[1] * point.x + self.elements[3] * point.y + self.elements[5],
        )
    }

    /// Transform multiple points
    pub fn transform_all<'a>(&'a self, points: &'a [Point]) -> impl Iterator<Item = Point> + 'a {
        points.iter().map(|p| self.transform(p))
    }

    /// Multiply this matrix by another (self * other)
    pub fn multiply(&self, other: &Matrix) -> Self {
        let a1 = self.elements[0];
        let b1 = self.elements[1];
        let c1 = self.elements[2];
        let d1 = self.elements[3];
        let e1 = self.elements[4];
        let f1 = self.elements[5];

        let a2 = other.elements[0];
        let b2 = other.elements[1];
        let c2 = other.elements[2];
        let d2 = other.elements[3];
        let e2 = other.elements[4];
        let f2 = other.elements[5];

        Self {
            elements: [
                a1 * a2 + c1 * b2,
                b1 * a2 + d1 * b2,
                a1 * c2 + c1 * d2,
                b1 * c2 + d1 * d2,
                a1 * e2 + c1 * f2 + e1,
                b1 * e2 + d1 * f2 + f1,
            ],
        }
    }

    /// Multiply self by another matrix in place (self = self * other)
    pub fn multiply_self(&mut self, other: &Matrix) {
        *self = self.multiply(other);
    }

    /// Pre-multiply by another matrix (self = other * self)
    pub fn pre_multiply(&mut self, other: &Matrix) {
        *self = other.multiply(self);
    }

    /// Calculate determinant
    pub fn determinant(&self) -> f64 {
        self.elements[0] * self.elements[3] - self.elements[2] * self.elements[1]
    }

    /// Calculate inverse matrix
    pub fn inverse(&self) -> Option<Self> {
        let det = self.determinant();
        if det.abs() < 1e-10 {
            return None;
        }

        let a = self.elements[0];
        let b = self.elements[1];
        let c = self.elements[2];
        let d = self.elements[3];
        let e = self.elements[4];
        let f = self.elements[5];

        let inv_det = 1.0 / det;
        Some(Self {
            elements: [
                d * inv_det,
                -b * inv_det,
                -c * inv_det,
                a * inv_det,
                (c * f - d * e) * inv_det,
                (b * e - a * f) * inv_det,
            ],
        })
    }

    /// Translate in place
    pub fn translate_self(&mut self, tx: f64, ty: f64) {
        self.elements[4] += self.elements[0] * tx + self.elements[2] * ty;
        self.elements[5] += self.elements[1] * tx + self.elements[3] * ty;
    }

    /// Scale in place
    pub fn scale_self(&mut self, sx: f64, sy: f64) {
        self.elements[0] *= sx;
        self.elements[1] *= sx;
        self.elements[2] *= sy;
        self.elements[3] *= sy;
    }

    /// Rotate in place (angle in radians)
    pub fn rotate_self(&mut self, angle: f64) {
        let rot = Self::rotation(angle);
        self.multiply_self(&rot);
    }

    /// Convert to SVG matrix string "matrix(a, b, c, d, e, f)"
    pub fn to_svg_matrix(&self) -> String {
        format!(
            "matrix({:.6}, {:.6}, {:.6}, {:.6}, {:.6}, {:.6})",
            self.elements[0],
            self.elements[1],
            self.elements[2],
            self.elements[3],
            self.elements[4],
            self.elements[5]
        )
    }

    /// Convert to DOMMatrix format for Canvas
    pub fn to_dom_matrix(&self) -> [f64; 6] {
        self.elements
    }

    /// Create from DOMMatrix
    pub fn from_dom_matrix(elements: [f64; 6]) -> Self {
        Self { elements }
    }

    /// Get the rotation angle from the matrix (in radians)
    pub fn get_rotation(&self) -> f64 {
        self.elements[1].atan2(self.elements[0])
    }

    /// Get the scale factors (sx, sy)
    pub fn get_scale(&self) -> (f64, f64) {
        let sx = (self.elements[0] * self.elements[0] + self.elements[1] * self.elements[1]).sqrt();
        let sy = (self.elements[2] * self.elements[2] + self.elements[3] * self.elements[3]).sqrt();
        (sx, sy)
    }

    /// Get the translation (tx, ty)
    pub fn get_translation(&self) -> (f64, f64) {
        (self.elements[4], self.elements[5])
    }

    /// Get the uniform scale factor (assumes uniform scaling)
    pub fn scale_factor(&self) -> f64 {
        (self.elements[0] * self.elements[0] + self.elements[1] * self.elements[1]).sqrt()
    }

    /// Get the rotation angle in radians
    pub fn rotation_angle(&self) -> f64 {
        self.elements[1].atan2(self.elements[0])
    }

    /// Check if the matrix is identity
    pub fn is_identity(&self) -> bool {
        let e = &self.elements;
        (e[0] - 1.0).abs() < 1e-10 &&
        e[1].abs() < 1e-10 &&
        e[2].abs() < 1e-10 &&
        (e[3] - 1.0).abs() < 1e-10 &&
        e[4].abs() < 1e-10 &&
        e[5].abs() < 1e-10
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identity() {
        let m = Matrix::identity();
        let p = Point::new(5.0, 10.0);
        let result = m.transform(&p);
        assert_eq!(result.x, 5.0);
        assert_eq!(result.y, 10.0);
    }

    #[test]
    fn test_translation() {
        let m = Matrix::translation(10.0, 20.0);
        let p = Point::new(5.0, 10.0);
        let result = m.transform(&p);
        assert_eq!(result.x, 15.0);
        assert_eq!(result.y, 30.0);
    }

    #[test]
    fn test_scaling() {
        let m = Matrix::scaling(2.0, 3.0);
        let p = Point::new(5.0, 10.0);
        let result = m.transform(&p);
        assert_eq!(result.x, 10.0);
        assert_eq!(result.y, 30.0);
    }

    #[test]
    fn test_rotation() {
        let m = Matrix::rotation(std::f64::consts::FRAC_PI_2);
        let p = Point::new(1.0, 0.0);
        let result = m.transform(&p);
        assert!((result.x - 0.0).abs() < 1e-10);
        assert!((result.y - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_inverse() {
        let m = Matrix::rotation(0.5).multiply(&Matrix::translation(10.0, 20.0));
        let inv = m.inverse().unwrap();
        let identity = m.multiply(&inv);
        assert!((identity.elements[0] - 1.0).abs() < 1e-10);
        assert!((identity.elements[3] - 1.0).abs() < 1e-10);
    }
}
