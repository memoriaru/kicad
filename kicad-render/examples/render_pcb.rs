use kicad_json5::ir::board::Board;
use kicad_json5::Lexer;
use kicad_json5::Parser as SExprParser;
use kicad_render::pcb_renderer::PcbRenderer;

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: render_pcb <file.kicad_pcb>");
    let source = std::fs::read_to_string(&path).unwrap();

    let lexer = Lexer::new(&source);
    let mut parser = SExprParser::new(lexer);
    let sexpr = parser.parse_sexpr().expect("parse failed");
    let board = Board::from_sexpr(&sexpr).expect("board IR failed");

    eprintln!(
        "Board: {} footprints, {} segments, {} vias, {} zones, {} graphics",
        board.footprints.len(),
        board.segments.len(),
        board.vias.len(),
        board.zones.len(),
        board.graphics.len()
    );
    for fp in &board.footprints {
        eprintln!(
            "  FP {} ({}): {} pads, {} lines, {} circles, {} arcs, {} rects, {} polys",
            fp.reference,
            fp.lib_id,
            fp.pads.len(),
            fp.fp_lines.len(),
            fp.fp_circles.len(),
            fp.fp_arcs.len(),
            fp.fp_rects.len(),
            fp.fp_polys.len()
        );
    }

    let svg = PcbRenderer::new(&board).with_dpi(150.0).render_to_string();

    let out_path = path.replace(".kicad_pcb", ".pcb.svg");
    std::fs::write(&out_path, &svg).unwrap();
    eprintln!("Written: {} ({} bytes)", out_path, svg.len());
}
