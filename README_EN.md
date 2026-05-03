# kicad-render

KiCad schematic SVG renderer, ported 1:1 from the KiCanvas JS schematic renderer to Rust. Renders `.kicad_sch` files into high-quality SVG vector graphics.

## Features

- **High-fidelity rendering** — 1:1 reproduction of KiCanvas JS rendering logic, visually consistent with KiCad
- **Complete element support** — Wires, component symbols, pins, junctions, labels, text, no-connect markers, title blocks
- **KiCad theme colors** — Uses KiCad's official default color scheme
- **Accurate dimensions** — All measurements match KiCad internal units (mm)
- **WASM support** — Compilable to WebAssembly for in-browser rendering

## Installation

```bash
cargo build --release
# Binary: target/release/kicad-render
```

## CLI Usage

```bash
# Basic usage — outputs to same-name .svg file
kicad-render schematic.kicad_sch

# Specify output path
kicad-render schematic.kicad_sch -o output.svg
```

The CLI parses the schematic file, prints statistics (wire count, component count, junction count, etc.), and renders to SVG with correct viewBox and white background (3x scale).

## Library Usage

```rust
use kicad_render::SchematicRenderer;
use kicad_json5::parser::schematic::SchematicParser;

// Parse KiCad schematic
let schematic = SchematicParser::parse_file("schematic.kicad_sch")?;

// Render to SVG
let renderer = SchematicRenderer::new(&schematic);
let svg = renderer.render_to_svg()?;

// Save
std::fs::write("output.svg", svg)?;
```

## Architecture

```
kicad-render/
├── src/
│   ├── lib.rs                  # Library entry point
│   ├── main.rs                 # CLI entry point
│   ├── render_core/            # Base types and primitives
│   │   ├── point.rs            # Point, Matrix
│   │   └── color.rs            # Color definitions
│   ├── renderer/               # Renderer trait and SVG implementation
│   │   ├── mod.rs              # Renderer trait
│   │   └── svg_renderer.rs     # SVG backend
│   ├── schematic_renderer.rs   # Rendering orchestrator
│   ├── painter/                # Specialized painters
│   │   ├── pin_painter.rs      # Pin rendering
│   │   ├── wire_painter.rs     # Wire/bus rendering
│   │   ├── symbol_painter.rs   # Component symbol rendering
│   │   ├── label_painter.rs    # Label rendering (local/global/hierarchical)
│   │   ├── junction_painter.rs # Junction rendering
│   │   └── sheet_painter.rs    # Sheet symbol rendering
│   ├── layer/                  # Layer management
│   ├── text.rs                 # Text rendering and markup processing
│   ├── bridge.rs               # kicad-json5 IR → rendering type conversion
│   └── constants.rs            # Rendering constants and KiCad theme colors
├── tests/
│   ├── sch_render.rs           # Real .kicad_sch rendering integration tests
│   └── svg_render_test.rs      # SVG rendering unit tests
└── MODULE_REVIEW.md            # JS→Rust port comparison audit
```

### Rendering Pipeline

```
.kicad_sch → kicad-json5 Parser → IR → Bridge → SchematicRenderer
                                                  ├── WirePainter
                                                  ├── SymbolPainter
                                                  ├── PinPainter
                                                  ├── LabelPainter
                                                  ├── JunctionPainter
                                                  └── SheetPainter
                                              → SVG Output
```

## Features

| Feature | Default | Description |
|---------|---------|-------------|
| `svg` | Enabled | SVG rendering backend |
| `wasm` | Disabled | WebAssembly + Canvas backend |

Enable WASM:

```bash
cargo build --features wasm --target wasm32-unknown-unknown
```

## Dependencies

- `kicad-json5` — KiCad S-expression parser (local dependency)
- `thiserror` / `anyhow` — Error handling
- `wasm-bindgen` / `web-sys` / `js-sys` — WASM support (optional)

## Relationship with kicad-json5

`kicad-render` uses `kicad-json5` as its parsing engine. `kicad-json5` handles S-expression → IR conversion, while `kicad-render`'s `bridge` module converts IR to rendering types, then draws via Painters.

```
kicad-json5 (parse/compile)  →  kicad-render (render/visualize)
```

## Acknowledgments

This project is based on rendering logic from the following open-source projects:

- [**KiCanvas**](https://github.com/theacodes/kicanvas) — KiCad schematic/PCB browser renderer. This project's Painter layer architecture and rendering algorithms are ported 1:1 from its TypeScript/JS code
- [**ecad-viewer**](https://github.com/AbijahKecadan/ecad-viewer) — Online KiCad file viewer based on KiCanvas

## License

MIT
