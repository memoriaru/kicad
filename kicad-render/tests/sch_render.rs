//! Integration test: render real .kicad_sch files from example_sch/ to SVG

use std::path::PathBuf;
use kicad_json5::{Lexer, Parser};
use kicad_render::renderer::Renderer;
use kicad_render::render_core::Matrix;
use kicad_render::schematic_renderer::SchematicRenderer;
use kicad_render::renderer::SvgRenderer;

fn example_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("example_sch")
}

fn render_to_svg(name: &str) -> String {
    let path = example_dir().join(name);
    assert!(path.exists(), "Fixture not found: {}", path.display());
    let source = std::fs::read_to_string(&path).unwrap();

    let lexer = Lexer::new(&source);
    let mut parser = Parser::new(lexer);
    let schematic = parser.parse().unwrap_or_else(|e| panic!("Failed to parse {}: {}", name, e));

    let file_name = name.to_string();
    let sch_renderer = SchematicRenderer::new(&schematic)
        .with_file_name(file_name);
    let bbox = sch_renderer.bbox();
    let (paper_w, paper_h) = sch_renderer.paper_size();

    assert!(paper_w > 0.0, "{}: paper width should be positive", name);
    assert!(paper_h > 0.0, "{}: paper height should be positive", name);
    assert!(!bbox.is_empty(), "{}: bounding box should not be empty", name);

    let scale = 3.0;
    let scale_matrix = Matrix::new([scale, 0.0, 0.0, scale, 0.0, 0.0]);
    let mut svg_renderer = SvgRenderer::new();
    svg_renderer.set_transform(&scale_matrix);
    sch_renderer.render(&mut svg_renderer);

    let output = svg_renderer.output();
    assert!(!output.is_empty(), "{}: SVG output should not be empty", name);
    assert!(output.contains("<circle") || output.contains("<polyline") || output.contains("<text"),
        "{}: SVG should contain graphical elements", name);

    output
}

#[test]
fn test_render_voltage_selection() {
    render_to_svg("Voltage-selection.kicad_sch");
}

#[test]
fn test_render_target_mcu() {
    render_to_svg("Target-MCU.kicad_sch");
}

#[test]
fn test_render_usb() {
    render_to_svg("USB.kicad_sch");
}

#[test]
fn test_render_wch_linke() {
    render_to_svg("WCH-LinkE-R0-1v3.kicad_sch");
}

#[test]
fn test_render_tiny_scarab() {
    render_to_svg("tiny-scarab.kicad_sch");
}
