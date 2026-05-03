//! SVG Rendering Integration Test
//!
//! Tests the complete rendering pipeline from schematic elements to SVG output

use kicad_render::{
    Point, Color, BoundingBox, Matrix,
    Circle, Arc, Polyline, Polygon, Stroke, Fill,
    Renderer, RenderContext, SvgRenderer,
    LayerSet, LayerId, LayerElement, LayerElementType,
};
use kicad_render::painter::{
    Painter, PinPainter, PinGraphic, PinType,
    WirePainter, WireSegment,
    JunctionPainter, Junction,
    LabelPainter, Label, LabelType, LabelShape,
};
use std::fs;
use std::path::PathBuf;

/// Create a simple test schematic with various elements
fn create_test_schematic_layers() -> LayerSet {
    let mut layers = LayerSet::default();

    // 1. Draw some wires (using wire color: dark green)
    let wire_color = Color::from_rgb(0, 132, 132); // KiCad wire color
    let wire_segments = vec![
        WireSegment::new(Point::new(0.0, 0.0), Point::new(50.0, 0.0)),
        WireSegment::new(Point::new(50.0, 0.0), Point::new(50.0, 30.0)),
        WireSegment::new(Point::new(50.0, 30.0), Point::new(100.0, 30.0)),
    ];
    let wire_painter = WirePainter::new(wire_segments, wire_color);
    wire_painter.paint(&mut layers);

    // 2. Add junctions at the corners
    let junctions = vec![
        Junction::new(Point::new(50.0, 0.0)),
        Junction::new(Point::new(50.0, 30.0)),
    ];
    let junction_painter = JunctionPainter::new(junctions, wire_color);
    junction_painter.paint(&mut layers);

    // 3. Add a pin
    let mut pin = PinGraphic::new(Point::new(100.0, 30.0), 0, 10.0);
    pin.name = "OUT".to_string();
    pin.pin_type = PinType::Output;
    let pin_painter = PinPainter::new(
        pin,
        Matrix::identity(),
        wire_color,
        Color::black(),
    );
    pin_painter.paint(&mut layers);

    // 4. Add labels
    let label = Label {
        label_type: LabelType::Local,
        position: Point::new(0.0, 0.0),
        rotation: 0,
        text: "VIN".to_string(),
        shape: LabelShape::Passive,
        font_size: 1.27,
        custom_color: None,
    };
    let label_painter = LabelPainter::new(label, wire_color);
    label_painter.paint(&mut layers);

    // 5. Add some direct graphics primitives
    // Add a rectangle (symbol body)
    let layer = layers.get_layer_mut(LayerId::SymbolBackground).unwrap();
    let rect_points = vec![
        Point::new(60.0, 20.0),
        Point::new(90.0, 20.0),
        Point::new(90.0, 40.0),
        Point::new(60.0, 40.0),
    ];
    let rect = Polygon::new(rect_points.clone())
        .with_fill(Fill::solid(Color::from_rgb(255, 255, 220)))
        .with_stroke(Stroke::new(0.1524, Color::black()));
    layer.add_element(LayerElement::new(LayerElementType::Polygon(rect)));

    // Add a circle inside
    let layer = layers.get_layer_mut(LayerId::SymbolForeground).unwrap();
    let circle = Circle::new(Point::new(75.0, 30.0), 5.0)
        .with_fill(Fill::solid(Color::red()));
    layer.add_element(LayerElement::new(LayerElementType::Circle(circle)));

    layers
}

/// Test basic SVG rendering
#[test]
fn test_basic_svg_render() {
    let layers = create_test_schematic_layers();

    // Calculate bounding box from all layers
    let mut bbox = BoundingBox::empty();
    for layer in &layers.layers {
        bbox.expand(&layer.bbox);
    }

    // Add some padding
    let padding = 10.0;
    let bounds = BoundingBox::new(
        bbox.x - padding,
        bbox.y - padding,
        bbox.w + padding * 2.0,
        bbox.h + padding * 2.0,
    );

    // Create renderer with context
    let context = RenderContext::for_svg(bounds, 1.0);
    let mut renderer = SvgRenderer::with_context(context);

    // Render all layers
    layers.render(&mut renderer);

    // Get output
    let output = renderer.output();

    // Verify output contains expected elements
    assert!(output.contains("<polyline"), "Should contain polyline elements");
    assert!(output.contains("<circle"), "Should contain circle elements");
    assert!(output.contains("<polygon"), "Should contain polygon elements");
    assert!(output.contains("<text"), "Should contain text elements");

    println!("SVG Output:\n{}", output);
}

/// Test SVG output to file
#[test]
fn test_svg_file_output() {
    let layers = create_test_schematic_layers();

    // Calculate bounding box
    let mut bbox = BoundingBox::empty();
    for layer in &layers.layers {
        bbox.expand(&layer.bbox);
    }

    let padding = 10.0;
    let bounds = BoundingBox::new(
        bbox.x - padding,
        bbox.y - padding,
        bbox.w + padding * 2.0,
        bbox.h + padding * 2.0,
    );

    // Create renderer
    let context = RenderContext::for_svg(bounds, 1.0);
    let mut renderer = SvgRenderer::with_context(context);

    // Render
    layers.render(&mut renderer);

    // Build complete SVG document
    let svg_content = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="{:.2} {:.2} {:.2} {:.2}" width="800" height="600">
<style>
  polyline {{ fill: none; }}
  text {{ font-family: monospace; dominant-baseline: middle; }}
</style>
{}
</svg>"#,
        bounds.x, bounds.y, bounds.w, bounds.h,
        renderer.output()
    );

    // Write to test output file
    let output_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("test_output")
        .join("test_schematic.svg");

    fs::create_dir_all(output_path.parent().unwrap()).unwrap();
    fs::write(&output_path, &svg_content).unwrap();

    println!("SVG written to: {:?}", output_path);

    // Verify file was created
    assert!(output_path.exists(), "SVG file should be created");

    // Read back and verify
    let content = fs::read_to_string(&output_path).unwrap();
    assert!(content.starts_with("<svg"), "Should be valid SVG");
    assert!(content.ends_with("</svg>"), "Should have closing tag");
}

/// Test individual graphic primitives
#[test]
fn test_primitive_rendering() {
    let context = RenderContext::new(BoundingBox::new(0.0, 0.0, 100.0, 100.0), 1.0);
    let mut renderer = SvgRenderer::with_context(context);

    // Test circle
    let circle = Circle::new(Point::new(25.0, 25.0), 10.0)
        .with_fill(Fill::solid(Color::red()));
    renderer.draw_circle(&circle);

    // Test polyline
    let polyline = Polyline::from_points(
        &[(0.0, 50.0), (30.0, 50.0), (30.0, 80.0)],
        Stroke::new(1.0, Color::blue())
    );
    renderer.draw_polyline(&polyline);

    // Test polygon
    let polygon = Polygon::new(vec![
        Point::new(50.0, 50.0),
        Point::new(70.0, 50.0),
        Point::new(60.0, 70.0),
    ]).with_fill(Fill::solid(Color::green()));
    renderer.draw_polygon(&polygon);

    // Test arc (Arc::new requires stroke in constructor)
    let arc = Arc::new(
        Point::new(80.0, 25.0),
        10.0,
        0.0,
        std::f64::consts::PI,
        Stroke::new(1.0, Color::black())
    );
    renderer.draw_arc(&arc);

    // Test text
    renderer.draw_text(&Point::new(10.0, 90.0), "Test Label", 12.0, &Color::black(), false, 0.0, "", "");

    let output = renderer.output();

    // Verify all elements are present
    assert!(output.contains("<circle"), "Circle should be rendered");
    assert!(output.contains("<polyline"), "Polyline should be rendered");
    assert!(output.contains("<polygon"), "Polygon should be rendered");
    assert!(output.contains("<path"), "Arc (as path) should be rendered");
    assert!(output.contains("<text"), "Text should be rendered");
    assert!(output.contains("Test Label"), "Text content should be present");

    println!("Primitive output:\n{}", output);
}

/// Test layer ordering
#[test]
fn test_layer_z_order() {
    let _layers = LayerSet::default();

    // Verify z-order
    let bg_z = LayerId::SymbolBackground.z_index();
    let fg_z = LayerId::SymbolForeground.z_index();
    assert!(bg_z < fg_z, "Background should have lower z-index than foreground");

    println!("Z-order verified: background={}, foreground={}", bg_z, fg_z);
}

/// Test transformation matrices
#[test]
fn test_transform_rendering() {
    let context = RenderContext::new(BoundingBox::new(0.0, 0.0, 100.0, 100.0), 1.0);
    let mut renderer = SvgRenderer::with_context(context);

    // Draw circle at origin
    let circle1 = Circle::new(Point::new(10.0, 10.0), 5.0)
        .with_fill(Fill::solid(Color::black()));
    renderer.draw_circle(&circle1);

    // Apply translation
    let translate = Matrix::translation(30.0, 0.0);
    renderer.set_transform(&translate);

    // Draw same circle (should appear translated)
    let circle2 = Circle::new(Point::new(10.0, 10.0), 5.0)
        .with_fill(Fill::solid(Color::red()));
    renderer.draw_circle(&circle2);

    // Restore and draw again
    renderer.restore();
    let circle3 = Circle::new(Point::new(10.0, 30.0), 5.0)
        .with_fill(Fill::solid(Color::blue()));
    renderer.draw_circle(&circle3);

    let output = renderer.output();

    // Should have three circles
    let circle_count = output.matches("<circle").count();
    assert_eq!(circle_count, 3, "Should have 3 circles");

    println!("Transform output:\n{}", output);
}

/// Test wire painter
#[test]
fn test_wire_painter() {
    let mut layers = LayerSet::default();

    let segments = vec![
        WireSegment::new(Point::new(0.0, 0.0), Point::new(50.0, 0.0)),
        WireSegment::new(Point::new(50.0, 0.0), Point::new(50.0, 50.0)),
    ];
    let painter = WirePainter::new(segments, Color::blue());
    painter.paint(&mut layers);

    let wire_layer = layers.get_layer(LayerId::Wire).unwrap();
    assert_eq!(wire_layer.elements.len(), 2, "Should have 2 wire segments");

    // Check bounding box - coordinates should encompass the wire path
    let bbox = painter.bbox();
    assert_eq!(bbox.min_x(), 0.0, "min_x should be 0.0");
    assert_eq!(bbox.max_x(), 50.0, "max_x should be 50.0");
    // Y coordinates span from 0.0 to 50.0
    assert!(bbox.min_y() <= 0.0, "min_y should be <= 0.0, got {}", bbox.min_y());
    assert!(bbox.max_y() >= 50.0, "max_y should be >= 50.0, got {}", bbox.max_y());
}

/// Test junction painter
#[test]
fn test_junction_painter() {
    let mut layers = LayerSet::default();

    let junctions = vec![
        Junction::new(Point::new(25.0, 25.0)),
        Junction::with_diameter(Point::new(50.0, 50.0), 2.0),
    ];
    let painter = JunctionPainter::new(junctions, Color::red());
    painter.paint(&mut layers);

    let junction_layer = layers.get_layer(LayerId::Junctions).unwrap();
    assert_eq!(junction_layer.elements.len(), 2, "Should have 2 junctions");
}

/// Test pin painter
#[test]
fn test_pin_painter() {
    let mut layers = LayerSet::default();

    let pin = PinGraphic::new(Point::new(0.0, 0.0), 0, 10.0);
    let painter = PinPainter::new(
        pin,
        Matrix::identity(),
        Color::blue(),
        Color::black(),
    );
    painter.paint(&mut layers);

    let pin_layer = layers.get_layer(LayerId::SymbolPin).unwrap();
    // Pin should have at least the body (polyline)
    assert!(!pin_layer.elements.is_empty(), "Pin should have elements");
}

/// Test label painter
#[test]
fn test_label_painter() {
    let mut layers = LayerSet::default();

    let label = Label {
        label_type: LabelType::Global,
        position: Point::new(0.0, 0.0),
        rotation: 0,
        text: "VCC".to_string(),
        shape: LabelShape::Input,
        font_size: 1.27,
        custom_color: None,
    };
    let painter = LabelPainter::new(label, Color::red());
    painter.paint(&mut layers);

    let label_layer = layers.get_layer(LayerId::Labels).unwrap();
    // Label should have at least text element
    assert!(!label_layer.elements.is_empty(), "Label should have elements");
}
