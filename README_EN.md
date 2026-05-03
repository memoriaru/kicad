# kicad-rs

Rust toolchain for AI-assisted KiCad circuit design workflows.

## Projects

| Project | Description | Version |
|---------|-------------|---------|
| [kicad-json5](kicad-json5/) | KiCad S-expression ↔ JSON5 bidirectional compiler + topology extraction | 0.8.5 |
| [kicad-render](kicad-render/) | KiCad schematic SVG renderer (1:1 port from KiCanvas) | 0.2.3 |

## kicad-json5

Bidirectional conversion between KiCad `.kicad_sch` and JSON5, providing a clean, token-efficient intermediate representation for LLMs.

```bash
# S-expression → JSON5
kicad-json5 schematic.kicad_sch -o schematic.json5

# JSON5 → KiCad (auto-insert PWR_FLAG)
kicad-json5 schematic.json5 -o schematic.kicad_sch --power-flags

# Extract circuit topology
kicad-json5 schematic.kicad_sch -f topology
```

Key features: Standard Device library embedding, auto wire/label generation, PWR_FLAG insertion, No-Connect markers, topology extraction.

See [kicad-json5/README_EN.md](kicad-json5/README_EN.md) | [中文](kicad-json5/README.md)

## kicad-render

Renders `.kicad_sch` files into high-fidelity SVG vector graphics. Rendering logic ported 1:1 from KiCanvas JS to Rust.

```bash
kicad-render schematic.kicad_sch -o output.svg
```

Also usable as a library, with optional WASM compilation for in-browser rendering.

See [kicad-render/README_EN.md](kicad-render/README_EN.md) | [中文](kicad-render/README.md)

## Pipeline

```
.kicad_sch → kicad-json5 → JSON5 (AI-readable) → kicad-json5 → .kicad_sch
                          ↓
                     kicad-render → SVG visualization
```

## Acknowledgments

- [KiCanvas](https://github.com/theacodes/kicanvas) — Source of kicad-render's Painter architecture and rendering algorithms
- [ecad-viewer](https://github.com/AbijahKecadan/ecad-viewer) — Online KiCad file viewer based on KiCanvas

## License

MIT
