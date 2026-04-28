//! Layer system for schematic rendering
//!
//! Layers control visibility and rendering order
//! and provide visual grouping of elements.

use crate::render_core::{Color, BoundingBox, Point};
use crate::render_core::graphics::{Circle, Arc, Polyline, Polygon, Bezier};
use crate::renderer::Renderer;

/// Layer identifier (enum — zero allocation, Copy semantics)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LayerId {
    Grid,
    DrawingSheet,
    Notes,
    SymbolBackground,
    Wire,
    Bus,
    SymbolPin,
    SymbolForeground,
    Junctions,
    Labels,
    Interactive,
}

impl LayerId {
    /// Z-index for rendering order (higher = rendered on top)
    pub fn z_index(self) -> i32 {
        match self {
            Self::Grid => 0,
            Self::DrawingSheet => 1,
            Self::Notes => 2,
            Self::SymbolBackground => 5,
            Self::Wire => 10,
            Self::Bus => 11,
            Self::SymbolPin => 20,
            Self::SymbolForeground => 25,
            Self::Junctions => 30,
            Self::Labels => 35,
            Self::Interactive => 100,
        }
    }
}

impl std::fmt::Display for LayerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::Grid => "Grid",
            Self::DrawingSheet => "DrawingSheet",
            Self::Notes => "Notes",
            Self::SymbolBackground => "Symbol.Background",
            Self::Wire => "Wire",
            Self::Bus => "Bus",
            Self::SymbolPin => "Symbol.Pin",
            Self::SymbolForeground => "Symbol.Foreground",
            Self::Junctions => "Junctions",
            Self::Labels => "Labels",
            Self::Interactive => "Interactive",
        };
        write!(f, "LayerId({})", name)
    }
}

/// Layer element type
#[derive(Debug, Clone)]
pub enum LayerElementType {
    Circle(Circle),
    Arc(Arc),
    Polyline(Polyline),
    Polygon(Polygon),
    Bezier(Bezier),
    Text {
        position: Point,
        text: String,
        font_size: f64,
        color: Color,
        bold: bool,
        rotation: f64,
        text_anchor: &'static str,
        dominant_baseline: &'static str,
    },
}

/// Element in a layer
#[derive(Debug, Clone)]
pub struct LayerElement {
    pub bbox: BoundingBox,
    pub element_type: LayerElementType,
}

impl LayerElement {
    pub fn new(element_type: LayerElementType) -> Self {
        let bbox = match &element_type {
            LayerElementType::Circle(c) => c.bbox(),
            LayerElementType::Arc(a) => a.bbox(),
            LayerElementType::Polyline(p) => p.bbox(),
            LayerElementType::Polygon(p) => p.bbox(),
            LayerElementType::Bezier(b) => b.bbox(),
            LayerElementType::Text { position, text, font_size, .. } => {
                BoundingBox::from_min_max(
                    position.x,
                    position.y,
                    position.x + text.len() as f64 * font_size * 0.5,
                    position.y + font_size,
                )
            }
        };
        Self { bbox, element_type }
    }
}

/// Layer containing rendered elements
#[derive(Debug, Clone)]
pub struct Layer {
    pub id: LayerId,
    pub bbox: BoundingBox,
    pub visible: bool,
    pub elements: Vec<LayerElement>,
}

impl Layer {
    pub fn new(id: LayerId) -> Self {
        Self {
            id,
            bbox: BoundingBox::empty(),
            visible: true,
            elements: Vec::new(),
        }
    }

    pub fn add_element(&mut self, element: LayerElement) {
        self.bbox.expand(&element.bbox);
        self.elements.push(element);
    }

    pub fn clear(&mut self) {
        self.elements.clear();
        self.bbox = BoundingBox::empty();
    }
}

/// Layer set - collection of layers
#[derive(Debug, Clone)]
pub struct LayerSet {
    pub layers: Vec<Layer>,
}

impl LayerSet {
    pub fn new() -> Self {
        Self { layers: Vec::new() }
    }

    pub fn add_layer(&mut self, id: LayerId) {
        self.layers.push(Layer::new(id));
    }

    pub fn get_layer(&self, id: LayerId) -> Option<&Layer> {
        self.layers.iter().find(|l| l.id == id)
    }

    pub fn get_layer_mut(&mut self, id: LayerId) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|l| l.id == id)
    }

    /// Render all visible layers sorted by z-index
    pub fn render(&self, renderer: &mut dyn Renderer) {
        let mut layers: Vec<&Layer> = self.layers.iter().filter(|l| l.visible).collect();
        layers.sort_by_key(|l| l.id.z_index());

        for layer in layers {
            for element in &layer.elements {
                self.render_element(renderer, element);
            }
        }
    }

    fn render_element(&self, renderer: &mut dyn Renderer, element: &LayerElement) {
        match &element.element_type {
            LayerElementType::Circle(c) => renderer.draw_circle(c),
            LayerElementType::Arc(a) => renderer.draw_arc(a),
            LayerElementType::Polyline(p) => renderer.draw_polyline(p),
            LayerElementType::Polygon(p) => renderer.draw_polygon(p),
            LayerElementType::Bezier(b) => renderer.draw_bezier(b),
            LayerElementType::Text { position, text, font_size, color, bold, rotation, text_anchor, dominant_baseline } => {
                renderer.draw_text(position, text, *font_size, color, *bold, *rotation, text_anchor, dominant_baseline);
            }
        }
    }
}

impl Default for LayerSet {
    fn default() -> Self {
        let mut set = Self { layers: Vec::with_capacity(11) };
        set.add_layer(LayerId::Grid);
        set.add_layer(LayerId::DrawingSheet);
        set.add_layer(LayerId::Notes);
        set.add_layer(LayerId::SymbolBackground);
        set.add_layer(LayerId::Wire);
        set.add_layer(LayerId::Bus);
        set.add_layer(LayerId::SymbolPin);
        set.add_layer(LayerId::SymbolForeground);
        set.add_layer(LayerId::Junctions);
        set.add_layer(LayerId::Labels);
        set.add_layer(LayerId::Interactive);
        set
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_layer_set_creation() {
        let set = LayerSet::new();
        assert!(set.get_layer(LayerId::Wire).is_none());

        let default_set = LayerSet::default();
        assert!(default_set.get_layer(LayerId::Wire).is_some());
    }

    #[test]
    fn test_layer_z_order() {
        let set = LayerSet::default();
        let z_indices: Vec<i32> = set.layers.iter().map(|l| l.id.z_index()).collect();
        for i in 1..z_indices.len() {
            assert!(z_indices[i] >= z_indices[i - 1], "Layer z-order mismatch at index {}", i);
        }
    }
}
