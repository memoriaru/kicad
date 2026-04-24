//! Layer system for schematic rendering
//!
//! Layers control visibility and rendering order
//! and provide visual grouping of elements.

use crate::render_core::{Color, BoundingBox, Point};
use crate::render_core::graphics::{Circle, Arc, Polyline, Polygon, Bezier};
use crate::renderer::Renderer;

/// Layer identifier
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LayerId {
    /// Unique identifier for the layer
    pub id: String,
    /// Display name
    pub name: String,
    /// Z-index for rendering order (higher = rendered on top)
    pub z_index: i32,
}

impl LayerId {
    pub fn new(id: impl Into<String>, name: impl Into<String>, z_index: i32) -> Self {
        Self { id: id.into(), name: name.into(), z_index }
    }

    pub fn interactive() -> Self {
        Self::new("Interactive", "Interactive", 100)
    }

    pub fn wire() -> Self {
        Self::new("Wire", "Wire", 10)
    }

    pub fn symbol_background() -> Self {
        Self::new("Symbol.Background", "Symbol Background", 5)
    }

    pub fn symbol_foreground() -> Self {
        Self::new("Symbol.Foreground", "Symbol Foreground", 25)
    }

    pub fn symbol_pin() -> Self {
        Self::new("Symbol.Pin", "Symbol Pin", 20)
    }

    pub fn bus() -> Self {
        Self::new("Bus", "Bus", 11)
    }

    pub fn notes() -> Self {
        Self::new("Notes", "Notes", 2)
    }

    pub fn labels() -> Self {
        Self::new("Labels", "Labels", 35)
    }

    pub fn junctions() -> Self {
        Self::new("Junctions", "Junctions", 30)
    }

    pub fn drawing_sheet() -> Self {
        Self::new("DrawingSheet", "Drawing Sheet", 1)
    }

    pub fn grid() -> Self {
        Self::new("Grid", "Grid", 0)
    }
}

impl std::fmt::Display for LayerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LayerId({})", self.id)
    }
}

/// Layer element type
#[derive(Debug, Clone)]
pub enum LayerElementType {
    /// Circle shape
    Circle(Circle),
    /// Arc shape
    Arc(Arc),
    /// Polyline shape
    Polyline(Polyline),
    /// Polygon shape
    Polygon(Polygon),
    /// Bezier curve
    Bezier(Bezier),
    /// Text element
    Text {
        position: Point,
        text: String,
        font_size: f64,
        color: Color,
        bold: bool,
        /// Rotation angle in degrees (0 = horizontal, 90 = vertical)
        rotation: f64,
        /// SVG text-anchor: "start" (default), "middle", "end"
        text_anchor: String,
        /// SVG dominant-baseline: "" (auto), "central", "hanging"
        dominant_baseline: String,
    },
}

/// Element in a layer
#[derive(Debug, Clone)]
pub struct LayerElement {
    /// Bounding box
    pub bbox: BoundingBox,
    /// Element type
    pub element_type: LayerElementType,
}

impl LayerElement {
    /// Create a new layer element
    pub fn new(element_type: LayerElementType) -> Self {
        let bbox = match &element_type {
            LayerElementType::Circle(c) => c.bbox(),
            LayerElementType::Arc(a) => a.bbox(),
            LayerElementType::Polyline(p) => p.bbox(),
            LayerElementType::Polygon(p) => p.bbox(),
            LayerElementType::Bezier(b) => b.bbox(),
            LayerElementType::Text { position, text, font_size, rotation: _, text_anchor: _, dominant_baseline: _, .. } => {
                // Approximate text bbox
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
    /// Layer identifier
    pub id: LayerId,
    /// Bounding box of this layer's content
    pub bbox: BoundingBox,
    /// Whether this layer is visible
    pub visible: bool,
    /// Elements in this layer
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

    /// Add an element to the layer
    pub fn add_element(&mut self, element: LayerElement) {
        self.bbox.expand(&element.bbox);
        self.elements.push(element);
    }

    /// Clear all elements
    pub fn clear(&mut self) {
        self.elements.clear();
        self.bbox = BoundingBox::empty();
    }
}

/// Layer set - collection of layers
#[derive(Debug, Clone)]
pub struct LayerSet {
    /// All layers
    pub layers: Vec<Layer>,
}

impl LayerSet {
    /// Create empty layer set
    pub fn new() -> Self {
        Self { layers: Vec::new() }
    }

    /// Add a new layer
    pub fn add_layer(&mut self, id: LayerId) {
        let layer = Layer::new(id);
        self.layers.push(layer);
    }

    /// Get a layer by ID
    pub fn get_layer(&self, id: &LayerId) -> Option<&Layer> {
        self.layers.iter().find(|l| l.id == *id)
    }

    /// Get mutable layer by ID
    pub fn get_layer_mut(&mut self, id: &LayerId) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|l| l.id == *id)
    }

    /// Render all visible layers in order
    pub fn render(&self, renderer: &mut dyn Renderer) {
        // Sort layers by z-index
        let mut layers: Vec<&Layer> = self.layers.iter().filter(|l| l.visible).collect();
        layers.sort_by(|a, b| a.id.z_index.cmp(&b.id.z_index));

        // Render each layer
        for layer in layers {
            for element in &layer.elements {
                self.render_element(renderer, element);
            }
        }
    }

    /// Render a single element
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
        let mut set = Self::new();
        // Add default layers in z-order (matching JS layer ordering)
        // Grid(0) < DrawingSheet(1) < Notes(2) < Symbol.Background(5) < Wire(10)
        // < Bus(11) < Symbol.Pin(20) < Symbol.Foreground(25) < Junctions(30)
        // < Labels(35) < Interactive(100)
        set.add_layer(LayerId::grid());
        set.add_layer(LayerId::drawing_sheet());
        set.add_layer(LayerId::notes());
        set.add_layer(LayerId::symbol_background());
        set.add_layer(LayerId::wire());
        set.add_layer(LayerId::bus());
        set.add_layer(LayerId::symbol_pin());
        set.add_layer(LayerId::symbol_foreground());
        set.add_layer(LayerId::junctions());
        set.add_layer(LayerId::labels());
        set.add_layer(LayerId::interactive());
        set
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_layer_set_creation() {
        let set = LayerSet::new();
        assert!(set.get_layer(&LayerId::wire()).is_none());

        let default_set = LayerSet::default();
        assert!(default_set.get_layer(&LayerId::wire()).is_some());
    }

    #[test]
    fn test_layer_z_order() {
        let set = LayerSet::default();
        let ids: Vec<_> = set.layers.iter().map(|l| &l.id).collect();

        // Verify z-index order: Grid(0) < DSheet(1) < Notes(2) < Sym.BG(5) < Wire(10)
        // < Bus(11) < Sym.Pin(20) < Sym.FG(25) < Junction(30) < Labels(35) < Interactive(100)
        let expected_z: Vec<i32> = ids.iter().map(|id| id.z_index).collect();
        for i in 1..expected_z.len() {
            assert!(expected_z[i] >= expected_z[i - 1], "Layer z-order mismatch at index {}", i);
        }
    }
}
