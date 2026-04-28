//! Sheet Painter - renders hierarchical sheet instances to graphics primitives

use crate::render_core::{Point, Color, BoundingBox};
use crate::render_core::graphics::{Polygon, Polyline, Stroke, Fill};
use crate::layer::{LayerSet, LayerId, LayerElement, LayerElementType};
use crate::constants;
use super::Painter;

/// Sheet property text (Sheetname or Sheetfile)
#[derive(Debug, Clone)]
pub struct SheetPropertyRender {
    pub value: String,
    pub position: Point,
    pub font_size: f64,
    pub rotation: f64,
    pub text_anchor: &'static str,
    pub dominant_baseline: &'static str,
}

/// Sheet pin render data
#[derive(Debug, Clone)]
pub struct SheetPinRender {
    pub name: String,
    pub position: Point,
    pub rotation: i32,
    pub font_size: f64,
    pub color: Color,
    pub text_anchor: &'static str,
}

/// Sheet instance render data
#[derive(Debug, Clone)]
pub struct SheetInstance {
    pub position: Point,
    pub size: (f64, f64),
    pub stroke: Stroke,
    pub fill_color: Color,
    pub sheet_name: SheetPropertyRender,
    pub sheet_file: SheetPropertyRender,
    pub pins: Vec<SheetPinRender>,
}

pub struct SheetPainter {
    pub sheet: SheetInstance,
}

impl SheetPainter {
    pub fn new(sheet: SheetInstance) -> Self {
        Self { sheet }
    }

    fn paint_box(&self, layers: &mut LayerSet) {
        let layer = layers.get_layer_mut(LayerId::SheetBackground).unwrap();
        let (x, y) = (self.sheet.position.x, self.sheet.position.y);
        let (w, h) = self.sheet.size;

        let polygon = Polygon::from_points(&[
            (x, y), (x + w, y), (x + w, y + h), (x, y + h), (x, y),
        ])
        .with_fill(Fill::solid(self.sheet.fill_color))
        .with_stroke(self.sheet.stroke.clone());

        layer.add_element(LayerElement::new(LayerElementType::Polygon(polygon)));
    }

    fn paint_properties(&self, layers: &mut LayerSet) {
        let layer = layers.get_layer_mut(LayerId::SheetBackground).unwrap();
        let color = Color::black();

        if !self.sheet.sheet_name.value.is_empty() {
            layer.add_element(LayerElement::new(LayerElementType::Text {
                position: self.sheet.sheet_name.position,
                text: self.sheet.sheet_name.value.clone(),
                font_size: self.sheet.sheet_name.font_size,
                color,
                bold: false,
                rotation: self.sheet.sheet_name.rotation,
                text_anchor: self.sheet.sheet_name.text_anchor,
                dominant_baseline: self.sheet.sheet_name.dominant_baseline,
            }));
        }

        if !self.sheet.sheet_file.value.is_empty() {
            layer.add_element(LayerElement::new(LayerElementType::Text {
                position: self.sheet.sheet_file.position,
                text: self.sheet.sheet_file.value.clone(),
                font_size: self.sheet.sheet_file.font_size,
                color,
                bold: false,
                rotation: self.sheet.sheet_file.rotation,
                text_anchor: self.sheet.sheet_file.text_anchor,
                dominant_baseline: self.sheet.sheet_file.dominant_baseline,
            }));
        }
    }

    fn paint_pins(&self, layers: &mut LayerSet) {
        let layer = layers.get_layer_mut(LayerId::SheetPin).unwrap();
        let pin_stroke = Stroke::new(constants::LINE_WIDTH, constants::wire_color());

        for pin in &self.sheet.pins {
            let pin_pos = pin.position;
            let connection_length = 2.54; // ~100 mil

            let (dx, dy) = match pin.rotation % 360 {
                0 => (connection_length, 0.0),
                90 => (0.0, connection_length),
                180 => (-connection_length, 0.0),
                270 => (0.0, -connection_length),
                _ => (connection_length, 0.0),
            };

            let edge_point = Point::new(pin_pos.x - dx, pin_pos.y - dy);

            let line = Polyline::from_points(
                &[(edge_point.x, edge_point.y), (pin_pos.x, pin_pos.y)],
                pin_stroke.clone(),
            );
            layer.add_element(LayerElement::new(LayerElementType::Polyline(line)));

            if !pin.name.is_empty() {
                layer.add_element(LayerElement::new(LayerElementType::Text {
                    position: pin_pos,
                    text: pin.name.clone(),
                    font_size: pin.font_size,
                    color: pin.color,
                    bold: false,
                    rotation: 0.0,
                    text_anchor: pin.text_anchor,
                    dominant_baseline: "central",
                }));
            }
        }
    }
}

impl Painter for SheetPainter {
    fn bbox(&self) -> BoundingBox {
        let (x, y) = (self.sheet.position.x, self.sheet.position.y);
        let (w, h) = self.sheet.size;
        BoundingBox::from_min_max(x, y, x + w, y + h)
    }

    fn paint(&self, layers: &mut LayerSet) {
        self.paint_box(layers);
        self.paint_properties(layers);
        self.paint_pins(layers);
    }
}
