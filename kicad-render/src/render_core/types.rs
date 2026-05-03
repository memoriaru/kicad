//! Basic geometric types matching JS implementation

use std::ops::{Add, Sub, Mul, Neg};

/// Extension trait for f64 angle operations
pub trait AngleExt {
    /// Normalize angle to [0, 2π)
    fn normalize_angle(self) -> f64;
}

impl AngleExt for f64 {
    fn normalize_angle(self) -> f64 {
        let two_pi = 2.0 * std::f64::consts::PI;
        let mut angle = self % two_pi;
        if angle < 0.0 {
            angle += two_pi;
        }
        angle
    }
}

/// 2D Point / Vector (matching JS `v` class)
#[derive(Debug, Clone, Copy, Default)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    pub fn zero() -> Self {
        Self { x: 0.0, y: 0.0 }
    }

    pub fn set(&mut self, x: f64, y: f64) {
        self.x = x;
        self.y = y;
    }

    pub fn copy(&self) -> Self {
        *self
    }

    /// Add vector
    pub fn add(&self, other: &Point) -> Self {
        Self::new(self.x + other.x, self.y + other.y)
    }

    /// Subtract vector
    pub fn sub(&self, other: &Point) -> Self {
        Self::new(self.x - other.x, self.y - other.y)
    }

    /// Multiply by scalar
    pub fn mul(&self, scalar: f64) -> Self {
        Self::new(self.x * scalar, self.y * scalar)
    }

    /// Element-wise multiply
    pub fn multiply(&self, other: &Point) -> Self {
        Self::new(self.x * other.x, self.y * other.y)
    }

    /// Dot product
    pub fn dot(&self, other: &Point) -> f64 {
        self.x * other.x + self.y * other.y
    }

    /// Length squared
    pub fn length_sq(&self) -> f64 {
        self.x * self.x + self.y * self.y
    }

    /// Length
    pub fn length(&self) -> f64 {
        self.length_sq().sqrt()
    }

    /// Normalize
    pub fn normalize(&self) -> Self {
        let len = self.length();
        if len > 0.0 {
            self.mul(1.0 / len)
        } else {
            *self
        }
    }

    /// Rotate by angle (radians)
    pub fn rotate(&self, angle: f64) -> Self {
        let cos_a = angle.cos();
        let sin_a = angle.sin();
        Self::new(
            self.x * cos_a - self.y * sin_a,
            self.x * sin_a + self.y * cos_a,
        )
    }

    /// Rotate around a center point
    pub fn rotate_around(&self, center: &Point, angle: f64) -> Self {
        let dx = self.x - center.x;
        let dy = self.y - center.y;
        let cos_a = angle.cos();
        let sin_a = angle.sin();
        Point::new(
            center.x + dx * cos_a - dy * sin_a,
            center.y + dx * sin_a + dy * cos_a,
        )
    }

    /// Distance to another point
    pub fn distance_to(&self, other: &Point) -> f64 {
        self.sub(other).length()
    }
}

impl Add for Point {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y)
    }
}

impl Sub for Point {
    type Output = Self;
    fn sub(self, other: Self) -> Self {
        Self::new(self.x - other.x, self.y - other.y)
    }
}

impl Mul<f64> for Point {
    type Output = Self;
    fn mul(self, scalar: f64) -> Self {
        Self::new(self.x * scalar, self.y * scalar)
    }
}

impl Neg for Point {
    type Output = Self;
    fn neg(self) -> Self {
        Self::new(-self.x, -self.y)
    }
}

/// Angle in radians (matching JS `re` class)
#[derive(Debug, Clone, Copy, Default)]
pub struct Angle {
    pub radians: f64,
}

impl Angle {
    pub fn new(radians: f64) -> Self {
        Self { radians }
    }

    pub fn from_degrees(degrees: f64) -> Self {
        Self { radians: degrees.to_radians() }
    }

    pub fn degrees(&self) -> f64 {
        self.radians.to_degrees()
    }

    pub fn is_horizontal(&self) -> bool {
        let deg = self.degrees().abs() % 180.0;
        deg < 45.0 || deg > 135.0
    }

    /// Normalize angle to [-PI, PI]
    pub fn normalize(&self) -> Self {
        let mut r = self.radians;
        while r > std::f64::consts::PI {
            r -= 2.0 * std::f64::consts::PI;
        }
        while r < -std::f64::consts::PI {
            r += 2.0 * std::f64::consts::PI;
        }
        Self { radians: r }
    }

    /// Rotate a point around origin
    pub fn rotate_point(&self, point: &Point) -> Point {
        point.rotate(self.radians)
    }

    /// Rotate a point around a center
    pub fn rotate_point_around(&self, point: &Point, center: &Point) -> Point {
        point.rotate_around(center, self.radians)
    }

    pub fn add(&self, other: &Angle) -> Self {
        Self { radians: self.radians + other.radians }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_point_add() {
        let p1 = Point::new(1.0, 2.0);
        let p2 = Point::new(3.0, 4.0);
        let result = p1 + p2;
        assert_eq!(result.x, 4.0);
        assert_eq!(result.y, 6.0);
    }

    #[test]
    fn test_point_rotate() {
        let p = Point::new(1.0, 0.0);
        let rotated = p.rotate(std::f64::consts::FRAC_PI_2);
        assert!((rotated.x - 0.0).abs() < 1e-10);
        assert!((rotated.y - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_angle_from_degrees() {
        let angle = Angle::from_degrees(90.0);
        assert!((angle.radians - std::f64::consts::FRAC_PI_2).abs() < 1e-10);
    }
}
