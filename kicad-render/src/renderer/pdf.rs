//! PDF Renderer — renders schematic graphics directly to PDF via printpdf

use printpdf::path::{PaintMode, WindingOrder};
use printpdf::*;

use crate::render_core::graphics::{Arc, Bezier, Circle, Polygon, Polyline, Stroke};
use crate::render_core::{BoundingBox, Color, Matrix, Point};
use crate::renderer::{RenderContext, Renderer};

const MM_TO_PT: f64 = 72.0 / 25.4;

pub struct PdfRenderer {
    doc: PdfDocumentReference,
    #[allow(dead_code)]
    page: PdfPageIndex,
    layer: PdfLayerReference,
    font: IndirectFontRef,
    #[allow(dead_code)]
    page_w_mm: f64,
    page_h_mm: f64,
    context: RenderContext,
    transform_stack: Vec<Matrix>,
}

impl PdfRenderer {
    pub fn new(page_w_mm: f64, page_h_mm: f64) -> Self {
        let (doc, page, layer_idx) = PdfDocument::new(
            "Schematic",
            Mm(page_w_mm as f32),
            Mm(page_h_mm as f32),
            "Schematic",
        );
        let font = doc.add_builtin_font(BuiltinFont::Helvetica).unwrap();
        let page_ref = doc.get_page(page);
        let layer = page_ref.get_layer(layer_idx);

        Self {
            doc,
            page,
            layer,
            font,
            page_w_mm,
            page_h_mm,
            context: RenderContext::new(BoundingBox::empty(), 1.0),
            transform_stack: vec![Matrix::identity()],
        }
    }

    pub fn save_to_bytes(self) -> Vec<u8> {
        self.doc.save_to_bytes().unwrap_or_default()
    }

    fn current_transform(&self) -> &Matrix {
        self.transform_stack.last().unwrap()
    }

    /// Transform point from schematic coords (mm, Y-down) to PDF coords (mm, Y-up).
    fn transform_point(&self, x: f64, y: f64) -> (f64, f64) {
        let e = &self.current_transform().elements;
        let tx = e[0] * x + e[2] * y + e[4];
        let ty = e[1] * x + e[3] * y + e[5];
        (tx, self.page_h_mm - ty)
    }

    fn transform_scale(&self) -> f64 {
        let e = &self.current_transform().elements;
        ((e[0] * e[0] + e[1] * e[1]).sqrt() + (e[2] * e[2] + e[3] * e[3]).sqrt()) / 2.0
    }

    fn to_rgb(color: &Color) -> printpdf::Rgb {
        printpdf::Rgb::new(color.r as f32, color.g as f32, color.b as f32, None)
    }

    fn set_stroke(&self, stroke: &Stroke) {
        let scale = self.transform_scale();
        self.layer
            .set_outline_color(printpdf::Color::Rgb(Self::to_rgb(&stroke.color)));
        self.layer
            .set_outline_thickness((stroke.width * scale * MM_TO_PT) as f32);
    }

    fn set_fill(&self, color: &Color) {
        self.layer
            .set_fill_color(printpdf::Color::Rgb(Self::to_rgb(color)));
    }

    fn mm_point(x: f64, y: f64) -> printpdf::Point {
        printpdf::Point::new(Mm(x as f32), Mm(y as f32))
    }
}

impl Renderer for PdfRenderer {
    fn context(&self) -> &RenderContext {
        &self.context
    }

    fn save(&mut self) {
        self.layer.save_graphics_state();
        let current = self.current_transform().clone();
        self.transform_stack.push(current);
    }

    fn restore(&mut self) {
        self.transform_stack.pop();
        self.layer.restore_graphics_state();
    }

    fn set_transform(&mut self, transform: &Matrix) {
        self.transform_stack.push(transform.clone());
        // No CTM — all coordinate transforms done in transform_point()
    }

    fn draw_circle(&mut self, circle: &Circle) {
        let (cx, cy) = self.transform_point(circle.center.x, circle.center.y);
        let r = circle.radius * self.transform_scale();

        let k = 0.5522847498;
        let pts: Vec<(printpdf::Point, bool)> = vec![
            (Self::mm_point(cx - r, cy), true),
            (Self::mm_point(cx - r, cy - r * k), false),
            (Self::mm_point(cx - r * k, cy - r), false),
            (Self::mm_point(cx, cy - r), true),
            (Self::mm_point(cx + r * k, cy - r), false),
            (Self::mm_point(cx + r, cy - r * k), false),
            (Self::mm_point(cx + r, cy), true),
            (Self::mm_point(cx + r, cy + r * k), false),
            (Self::mm_point(cx + r * k, cy + r), false),
            (Self::mm_point(cx, cy + r), true),
            (Self::mm_point(cx - r * k, cy + r), false),
            (Self::mm_point(cx - r, cy + r * k), false),
            (Self::mm_point(cx - r, cy), true),
        ];

        if let Some(ref fill_color) = circle.fill.color {
            self.set_fill(fill_color);
            let poly = printpdf::Polygon {
                rings: vec![pts.clone()],
                mode: PaintMode::Fill,
                winding_order: WindingOrder::NonZero,
            };
            self.layer.add_polygon(poly);
        }

        if let Some(ref stroke) = circle.stroke {
            self.set_stroke(stroke);
            let line = Line {
                points: pts,
                is_closed: true,
            };
            self.layer.add_line(line);
        }
    }

    fn draw_arc(&mut self, arc: &Arc) {
        let (cx, cy) = self.transform_point(arc.center.x, arc.center.y);
        let r = arc.radius * self.transform_scale();

        self.set_stroke(&arc.stroke);

        let steps = 16;
        let mut pts = Vec::with_capacity(steps + 1);
        for i in 0..=steps {
            let t = i as f64 / steps as f64;
            let angle = arc.start_angle + (arc.end_angle - arc.start_angle) * t;
            let x = cx + r * angle.cos();
            let y = cy + r * angle.sin();
            pts.push((Self::mm_point(x, y), false));
        }

        if let Some(ref fill_color) = arc.fill.color {
            self.set_fill(fill_color);
            let poly = printpdf::Polygon {
                rings: vec![pts.clone()],
                mode: PaintMode::Fill,
                winding_order: WindingOrder::NonZero,
            };
            self.layer.add_polygon(poly);
        }

        let line = Line {
            points: pts,
            is_closed: false,
        };
        self.layer.add_line(line);
    }

    fn draw_polyline(&mut self, polyline: &Polyline) {
        if polyline.points.len() < 2 {
            return;
        }
        self.set_stroke(&polyline.stroke);

        let pts: Vec<(printpdf::Point, bool)> = polyline
            .points
            .iter()
            .map(|p| {
                let (x, y) = self.transform_point(p.x, p.y);
                (Self::mm_point(x, y), false)
            })
            .collect();

        let line = Line {
            points: pts,
            is_closed: false,
        };
        self.layer.add_line(line);
    }

    fn draw_polygon(&mut self, polygon: &Polygon) {
        if polygon.points.len() < 2 {
            return;
        }

        let pts: Vec<(printpdf::Point, bool)> = polygon
            .points
            .iter()
            .map(|p| {
                let (x, y) = self.transform_point(p.x, p.y);
                (Self::mm_point(x, y), false)
            })
            .collect();

        if let Some(ref fill_color) = polygon.fill.color {
            self.set_fill(fill_color);
            let poly = printpdf::Polygon {
                rings: vec![pts.clone()],
                mode: PaintMode::Fill,
                winding_order: WindingOrder::NonZero,
            };
            self.layer.add_polygon(poly);
        }

        if let Some(ref stroke) = polygon.stroke {
            self.set_stroke(stroke);
            let line = Line {
                points: pts,
                is_closed: true,
            };
            self.layer.add_line(line);
        }
    }

    fn draw_bezier(&mut self, bezier: &Bezier) {
        self.set_stroke(&bezier.stroke);

        let steps = 16;
        let pts: Vec<(printpdf::Point, bool)> = (0..=steps)
            .map(|i| {
                let t = i as f64 / steps as f64;
                let mt = 1.0 - t;
                let t2 = t * t;
                let t3 = t2 * t;

                let x = mt * mt * mt * bezier.start.x
                    + 3.0 * mt * mt * t * bezier.control1.x
                    + 3.0 * mt * t2 * bezier.control2.x
                    + t3 * bezier.end.x;
                let y = mt * mt * mt * bezier.start.y
                    + 3.0 * mt * mt * t * bezier.control1.y
                    + 3.0 * mt * t2 * bezier.control2.y
                    + t3 * bezier.end.y;

                let (tx, ty) = self.transform_point(x, y);
                (Self::mm_point(tx, ty), false)
            })
            .collect();

        let line = Line {
            points: pts,
            is_closed: false,
        };
        self.layer.add_line(line);
    }

    fn draw_text(
        &mut self,
        position: &Point,
        text: &str,
        font_size: f64,
        color: &Color,
        _bold: bool,
        _rotation: f64,
        _text_anchor: &str,
        _dominant_baseline: &str,
    ) {
        let (x, y) = self.transform_point(position.x, position.y);
        let scale = self.transform_scale();
        let size_pt = (font_size * scale * MM_TO_PT) as f32;

        self.layer
            .set_fill_color(printpdf::Color::Rgb(Self::to_rgb(color)));

        let safe_text: String = text.chars().filter(|c| *c as u32 <= 255).collect();

        if !safe_text.is_empty() {
            self.layer
                .use_text(safe_text, size_pt, Mm(x as f32), Mm(y as f32), &self.font);
        }
    }
}
