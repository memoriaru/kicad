//! Core rendering types matching KiCanvas JS implementation
//!
//! This module provides 1:1 port of the core rendering types from ecad-viewer.pc.js

mod types;
mod matrix;
mod color;
mod bbox;
mod transform;
pub mod graphics;

pub use types::*;
pub use matrix::Matrix;
pub use color::Color;
pub use bbox::BoundingBox;
pub use transform::{Camera2, Transform2D};
pub use graphics::{Circle, Arc, Polyline, Polygon, Rectangle, Bezier, Stroke, Fill, StrokeStyle};
