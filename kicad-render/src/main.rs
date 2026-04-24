//! kicad-render CLI — render KiCad schematics to SVG

use std::path::PathBuf;
use std::process;

use anyhow::{Context, Result};
use kicad_json5::{Lexer, Parser as SExprParser};

use kicad_render::renderer::{SvgRenderer, Renderer};
use kicad_render::render_core::Matrix;
use kicad_render::schematic_renderer::SchematicRenderer;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: kicad-render <input.kicad_sch> [-o output.svg]");
        process::exit(1);
    }

    let input_path = PathBuf::from(&args[1]);
    let output_path = if let Some(pos) = args.iter().position(|a| a == "-o") {
        if pos + 1 < args.len() {
            PathBuf::from(&args[pos + 1])
        } else {
            eprintln!("Error: -o requires a path argument");
            process::exit(1);
        }
    } else {
        input_path.with_extension("svg")
    };

    // Parse .kicad_sch → Schematic IR
    let source = std::fs::read_to_string(&input_path)
        .with_context(|| format!("Failed to read {}", input_path.display()))?;

    let lexer = Lexer::new(&source);
    let mut parser = SExprParser::new(lexer);
    let schematic = parser.parse()
        .with_context(|| "Parsing failed")?;

    eprintln!("Parsed: {} wires, {} components, {} junctions, {} labels, {} text_items",
        schematic.wires.len(),
        schematic.components.len(),
        schematic.junctions.len(),
        schematic.labels.len(),
        schematic.text_items.len(),
    );

    // Render
    let sch_renderer = SchematicRenderer::new(&schematic);
    let bbox = sch_renderer.bbox();
    let (paper_w, paper_h) = sch_renderer.paper_size();
    eprintln!("Paper size: {:.1} x {:.1} mm", paper_w, paper_h);
    eprintln!("Bounding box: ({:.1}, {:.1}) - ({:.1}, {:.1})",
        bbox.x, bbox.y, bbox.x + bbox.w, bbox.y + bbox.h);

    let scale = 3.0;

    // KiCad schematic coords are Y-down (origin at top-left), same as SVG.
    // No global Y-flip needed — each symbol's transform includes Y-flip
    // internally (library Y-UP → schematic Y-DOWN conversion).
    let scale_matrix = Matrix::new([scale, 0.0, 0.0, scale, 0.0, 0.0]);

    let mut svg_renderer = SvgRenderer::new();
    svg_renderer.set_transform(&scale_matrix);
    sch_renderer.render(&mut svg_renderer);

    // ViewBox covers the full paper area with padding so outer border stroke is not clipped
    let pad = 2.0;
    let view_x = -pad;
    let view_y = -pad;
    let view_w = paper_w * scale + pad * 2.0;
    let view_h = paper_h * scale + pad * 2.0;

    let svg_content = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"{:.2} {:.2} {:.2} {:.2}\">\n\
         <rect x=\"{:.2}\" y=\"{:.2}\" width=\"{:.2}\" height=\"{:.2}\" fill=\"white\"/>\n\
         {}\n</svg>",
        view_x, view_y, view_w, view_h,
        view_x, view_y, view_w, view_h,
        svg_renderer.output(),
    );

    std::fs::write(&output_path, svg_content)
        .with_context(|| format!("Failed to write {}", output_path.display()))?;

    eprintln!("Written: {}", output_path.display());
    Ok(())
}
