//! Core rendering types matching KiCanvas JS implementation
//!
//! This module provides 1:1 port of the core rendering types from ecad-viewer.pc.js

mod bbox;
mod color;
pub mod graphics;
mod matrix;
mod transform;
mod types;

pub use bbox::BoundingBox;
pub use color::Color;
pub use graphics::{Arc, Bezier, Circle, Fill, Polygon, Polyline, Rectangle, Stroke, StrokeStyle};
pub use matrix::Matrix;
pub use transform::{Camera2, Transform2D};
pub use types::*;
