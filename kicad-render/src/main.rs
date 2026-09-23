//! kicad-render CLI — render KiCad schematics or PCB layouts to SVG, PDF, or PNG

use std::path::{Path, PathBuf};
use std::process;

use anyhow::{Context, Result};
use kicad_json5::{Lexer, Parser as SExprParser};

use kicad_render::pcb_renderer::{PcbRenderer, RenderMode};
use kicad_render::render_core::Matrix;
use kicad_render::renderer::{PdfRenderer, Renderer, SvgRenderer};
use kicad_render::schematic_renderer::SchematicRenderer;

const USAGE: &str = "Usage: kicad-render <input.kicad_sch|.kicad_pcb> [-o out.svg|pdf|png] \
[--interactive] [--format pdf|svg|png] [--mode assembly|fab|copper] [--scale N]";

fn arg_value(args: &[String], flag: &str) -> Option<String> {
    // Matches both "--flag value" and "--flag=value"
    args.iter()
        .position(|a| a == flag)
        .and_then(|pos| args.get(pos + 1).cloned())
        .or_else(|| {
            args.iter()
                .find_map(|a| a.strip_prefix(&format!("{}=", flag)).map(|v| v.to_string()))
        })
}

fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|a| a == flag)
}

/// Resolve output format: explicit --format wins, else the -o extension, else svg.
fn resolve_format(args: &[String], output: &Path) -> String {
    if let Some(f) = arg_value(args, "--format") {
        return f.to_lowercase();
    }
    match output.extension().and_then(|e| e.to_str()) {
        Some("png") => "png".into(),
        Some("pdf") => "pdf".into(),
        Some("svg") => "svg".into(),
        _ => "svg".into(),
    }
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("{}", USAGE);
        process::exit(1);
    }

    let input_path = PathBuf::from(&args[1]);
    let interactive = has_flag(&args, "--interactive");

    let format_hint = arg_value(&args, "--format").map(|f| format!("{}.tmp_placeholder", f));
    let output_path = if let Some(pos) = args.iter().position(|a| a == "-o") {
        if pos + 1 < args.len() {
            PathBuf::from(&args[pos + 1])
        } else {
            eprintln!("Error: -o requires a path argument");
            process::exit(1);
        }
    } else {
        // No -o: derive from input + format hint (default svg)
        let ext = format_hint
            .as_deref()
            .and_then(|h| h.split('.').next_back())
            .unwrap_or("svg");
        input_path.with_extension(ext)
    };

    let format = resolve_format(&args, &output_path);
    let scale: f64 = arg_value(&args, "--scale")
        .and_then(|s| s.parse().ok())
        .unwrap_or(3.0);

    let is_pcb = input_path.extension().and_then(|e| e.to_str()) == Some("kicad_pcb");

    if is_pcb {
        render_pcb(&input_path, &output_path, &format, scale, &args)
    } else {
        render_schematic(&input_path, &output_path, &format, scale, interactive)
    }
}

// ── PCB rendering ──────────────────────────────────────────────

fn parse_render_mode(args: &[String]) -> Result<RenderMode> {
    match arg_value(args, "--mode").as_deref() {
        None => Ok(RenderMode::Assembly),
        Some("assembly") | Some("all") => Ok(RenderMode::Assembly),
        Some("fab") | Some("fabrication") => Ok(RenderMode::Fabrication),
        Some("copper") | Some("copper-only") => Ok(RenderMode::CopperOnly),
        Some(other) => Err(anyhow::anyhow!(
            "Unknown --mode '{}'. Valid: assembly | fab | copper",
            other
        )),
    }
}

fn render_pcb(
    input: &Path,
    output: &Path,
    format: &str,
    scale: f64,
    args: &[String],
) -> Result<()> {
    let mode = parse_render_mode(args)?;

    let source = std::fs::read_to_string(input)
        .with_context(|| format!("Failed to read {}", input.display()))?;

    let board = kicad_json5::parse_board(&source)
        .map_err(|e| anyhow::anyhow!("Parsing PCB failed: {}", e))?;

    eprintln!(
        "Parsed PCB: {} footprints, {} segments, {} vias, {} zones",
        board.footprints.len(),
        board.segments.len(),
        board.vias.len(),
        board.zones.len(),
    );

    let svg = PcbRenderer::new(&board)
        .with_scale(scale)
        .with_mode(mode)
        .render_to_string();

    match format {
        "svg" => {
            std::fs::write(output, &svg)
                .with_context(|| format!("Failed to write {}", output.display()))?;
            eprintln!("Written (SVG): {} ({} bytes)", output.display(), svg.len());
        }
        "png" => {
            write_png(&svg, output)?;
            eprintln!("Written (PNG): {}", output.display());
        }
        "pdf" => anyhow::bail!("PDF output is not supported for PCB; use svg or png"),
        other => anyhow::bail!("Unknown output format '{}'; use svg, png, or pdf", other),
    }
    Ok(())
}

/// Rasterize an SVG string to PNG via resvg. System fonts are loaded so
/// text elements (silkscreen refdes, title block) render with real glyphs.
fn write_png(svg: &str, output: &Path) -> Result<()> {
    let mut opts = resvg::usvg::Options::default();
    let mut fontdb = resvg::usvg::fontdb::Database::new();
    fontdb.load_system_fonts();
    opts.fontdb = std::sync::Arc::new(fontdb);

    let tree = resvg::usvg::Tree::from_str(svg, &opts)
        .map_err(|e| anyhow::anyhow!("SVG parse failed for rasterization: {:?}", e))?;

    let size = tree.size();
    let width = size.width().ceil() as u32;
    let height = size.height().ceil() as u32;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)
        .ok_or_else(|| anyhow::anyhow!("Failed to allocate {}x{} pixmap", width, height))?;
    pixmap.fill(resvg::tiny_skia::Color::from_rgba8(255, 255, 255, 255));

    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
    );

    pixmap
        .save_png(output)
        .map_err(|e| anyhow::anyhow!("Failed to write {}: {:?}", output.display(), e))?;
    Ok(())
}

// ── Schematic rendering (unchanged path) ───────────────────────

fn render_schematic(
    input: &Path,
    output: &Path,
    format: &str,
    scale: f64,
    interactive: bool,
) -> Result<()> {
    let input_path = input.to_path_buf();
    let output_path = output.to_path_buf();

    // Parse .kicad_sch → Schematic IR
    let source = std::fs::read_to_string(&input_path)
        .with_context(|| format!("Failed to read {}", input_path.display()))?;

    let lexer = Lexer::new(&source);
    let mut parser = SExprParser::new(lexer);
    let schematic = parser.parse().with_context(|| "Parsing failed")?;

    eprintln!(
        "Parsed: {} wires, {} components, {} junctions, {} labels, {} text_items, {} sheets",
        schematic.wires.len(),
        schematic.components.len(),
        schematic.junctions.len(),
        schematic.labels.len(),
        schematic.text_items.len(),
        schematic.sheets.len(),
    );

    // Render
    let file_name = input_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let sch_renderer = SchematicRenderer::new(&schematic).with_file_name(file_name);
    let bbox = sch_renderer.bbox();
    let (paper_w, paper_h) = sch_renderer.paper_size();
    eprintln!("Paper size: {:.1} x {:.1} mm", paper_w, paper_h);
    eprintln!(
        "Bounding box: ({:.1}, {:.1}) - ({:.1}, {:.1})",
        bbox.x,
        bbox.y,
        bbox.x + bbox.w,
        bbox.y + bbox.h
    );

    let scale_matrix = Matrix::new([scale, 0.0, 0.0, scale, 0.0, 0.0]);

    // PDF output path — no extra scale, schematic coords are already in mm
    if format == "pdf" {
        let mut pdf = PdfRenderer::new(paper_w, paper_h);
        sch_renderer.render(&mut pdf);
        let bytes = pdf.save_to_bytes();
        std::fs::write(&output_path, &bytes)
            .with_context(|| format!("Failed to write {}", output_path.display()))?;
        eprintln!(
            "Written (PDF): {} ({} bytes)",
            output_path.display(),
            bytes.len()
        );
        return Ok(());
    }

    if format == "png" {
        let mut svg_renderer = SvgRenderer::new();
        svg_renderer.set_transform(&scale_matrix);
        sch_renderer.render(&mut svg_renderer);

        let pad = 2.0;
        let svg_content = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" viewBox=\"{:.2} {:.2} {:.2} {:.2}\">\n\
             <rect x=\"{:.2}\" y=\"{:.2}\" width=\"{:.2}\" height=\"{:.2}\" fill=\"white\"/>\n\
             {}\n</svg>",
            -pad, -pad, paper_w * scale + pad * 2.0, paper_h * scale + pad * 2.0,
            -pad, -pad, paper_w * scale + pad * 2.0, paper_h * scale + pad * 2.0,
            svg_renderer.output(),
        );
        write_png(&svg_content, &output_path)?;
        eprintln!("Written (PNG): {}", output_path.display());
        return Ok(());
    }

    let mut svg_renderer = SvgRenderer::new();
    svg_renderer.set_transform(&scale_matrix);
    sch_renderer.render(&mut svg_renderer);

    // ViewBox covers the full paper area with padding so outer border stroke is not clipped
    let pad = 2.0;
    let view_x = -pad;
    let view_y = -pad;
    let view_w = paper_w * scale + pad * 2.0;
    let view_h = paper_h * scale + pad * 2.0;

    // CSS styles for interactive mode
    let interactive_css = if interactive && !schematic.sheets.is_empty() {
        r#"<style>
.sheet-link rect { fill: transparent; stroke: none; cursor: pointer; }
.sheet-link:hover rect { fill: rgba(0,100,255,0.08); stroke: #0064ff; stroke-width: 0.5; }
</style>
"#
    } else {
        ""
    };

    let mut svg_content = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" viewBox=\"{:.2} {:.2} {:.2} {:.2}\">\n\
         {}<rect x=\"{:.2}\" y=\"{:.2}\" width=\"{:.2}\" height=\"{:.2}\" fill=\"white\"/>\n\
         {}\n</svg>",
        view_x, view_y, view_w, view_h,
        interactive_css,
        view_x, view_y, view_w, view_h,
        svg_renderer.output(),
    );

    // Hierarchical: embed sub-schematics inside sheet boxes
    let has_subs = !schematic.sheets.is_empty();
    if has_subs {
        let base_dir = input_path.parent().unwrap_or(std::path::Path::new("."));
        let mut sub_embeds = Vec::new();
        let mut sub_links = Vec::new();
        let output_stem = output_path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "schematic".into());

        for sheet in &schematic.sheets {
            let sub_path = base_dir.join(&sheet.sheet_file.value);
            if !sub_path.exists() {
                continue;
            }

            let sub_source = match std::fs::read_to_string(&sub_path) {
                Ok(s) => s,
                Err(_) => continue,
            };

            let sub_lexer = Lexer::new(&sub_source);
            let mut sub_parser = SExprParser::new(sub_lexer);
            let sub_schematic = match sub_parser.parse() {
                Ok(s) => s,
                Err(_) => continue,
            };

            let sub_renderer = SchematicRenderer::new(&sub_schematic).skip_drawing_sheet();
            let sub_bbox = sub_renderer.bbox();
            if sub_bbox.is_empty() {
                continue;
            }

            // Embed sub-schematic content in the main SVG
            let mut sub_svg = SvgRenderer::new();
            sub_renderer.render(&mut sub_svg);
            let sub_content = sub_svg.output();

            let (sx, sy) = sheet.position;
            let (sw, sh) = sheet.size;
            let inset = 2.0;

            sub_embeds.push(format!(
                "<svg x=\"{:.2}\" y=\"{:.2}\" width=\"{:.2}\" height=\"{:.2}\" viewBox=\"{:.2} {:.2} {:.2} {:.2}\" style=\"overflow:hidden\">\n{}\n</svg>",
                (sx + inset) * scale,
                (sy + inset) * scale,
                (sw - 2.0 * inset) * scale,
                (sh - 2.0 * inset) * scale,
                sub_bbox.x - inset, sub_bbox.y - inset,
                sub_bbox.w + 2.0 * inset, sub_bbox.h + 2.0 * inset,
                sub_content
            ));

            // Interactive: generate separate sub-SVG + clickable link overlay
            if interactive {
                let safe_name = sheet.sheet_name.value.replace([' ', '/', '\\'], "_");
                let sub_svg_name = format!("{}-{}.svg", output_stem, safe_name);
                let sub_svg_path = output_path
                    .parent()
                    .unwrap_or(std::path::Path::new("."))
                    .join(&sub_svg_name);

                // Render sub-schematic as standalone SVG (with drawing sheet)
                let sub_full_renderer = SchematicRenderer::new(&sub_schematic)
                    .with_file_name(sheet.sheet_file.value.clone());
                let sub_paper = sub_full_renderer.paper_size();
                let mut sub_full_svg = SvgRenderer::new();
                sub_full_svg.set_transform(&scale_matrix);
                sub_full_renderer.render(&mut sub_full_svg);

                let sub_pad = 2.0;
                let sub_view_w = sub_paper.0 * scale + sub_pad * 2.0;
                let sub_view_h = sub_paper.1 * scale + sub_pad * 2.0;

                // Breadcrumb link back to parent
                let parent_name = output_path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "parent.svg".into());

                let sub_svg_content = format!(
                    "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" viewBox=\"{:.2} {:.2} {:.2} {:.2}\">\n\
                     <rect x=\"{:.2}\" y=\"{:.2}\" width=\"{:.2}\" height=\"{:.2}\" fill=\"white\"/>\n\
                     <a xlink:href=\"{}\" style=\"cursor:pointer\">\
                     <rect x=\"0\" y=\"0\" width=\"60\" height=\"14\" fill=\"#f0f0f0\" rx=\"2\" stroke=\"#999\" stroke-width=\"0.3\"/>\
                     <text x=\"4\" y=\"11\" font-size=\"8\" fill=\"#333\" font-family=\"sans-serif\">← {}</text>\n\
                     </a>\n\
                     <text x=\"70\" y=\"11\" font-size=\"8\" fill=\"#666\" font-family=\"sans-serif\">{}</text>\n\
                     {}\n</svg>",
                    -sub_pad, -sub_pad, sub_view_w, sub_view_h,
                    -sub_pad, -sub_pad, sub_view_w, sub_view_h,
                    parent_name,
                    parent_name.strip_suffix(".svg").unwrap_or(&parent_name),
                    sheet.sheet_name.value,
                    sub_full_svg.output(),
                );

                std::fs::write(&sub_svg_path, &sub_svg_content)
                    .with_context(|| format!("Failed to write {}", sub_svg_path.display()))?;
                eprintln!("Sub-SVG: {}", sub_svg_path.display());

                // Clickable overlay on sheet box in main SVG
                sub_links.push(format!(
                    "<a xlink:href=\"{}\" class=\"sheet-link\"><rect x=\"{:.2}\" y=\"{:.2}\" width=\"{:.2}\" height=\"{:.2}\"/></a>",
                    sub_svg_name,
                    sx * scale, sy * scale,
                    sw * scale, sh * scale,
                ));
            }
        }

        if !sub_embeds.is_empty() {
            svg_content = svg_content.replace(
                "</svg>",
                &format!(
                    "{}\n{}\n</svg>",
                    sub_embeds.join("\n"),
                    sub_links.join("\n")
                ),
            );
            eprintln!("Embedded {} sub-schematic(s)", sub_embeds.len());
        }
    }

    std::fs::write(&output_path, &svg_content)
        .with_context(|| format!("Failed to write {}", output_path.display()))?;

    eprintln!("Written: {}", output_path.display());
    Ok(())
}
