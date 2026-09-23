# kicad

Rust toolchain for programmatic KiCad design: parse, generate, and render.

| Crate | What it does |
| --- | --- |
| [kicad-json5](kicad-json5/) | KiCad S-expression ↔ JSON5 bidirectional compiler for schematics and PCBs (KiCad 7–10), with topology extraction and board-level auto-layout |
| [kicad-render](kicad-render/) | Schematic & PCB SVG renderer — a 1:1 Rust port of the [KiCanvas](https://github.com/theacodes/kicanvas) painters, with PDF/PNG output |

Both crates are MIT licensed. `kicad-render` derives from KiCanvas
(© 2022 Alethea Katherine Flowers) and ecad-viewer
(© 2024 深圳华秋电子有限公司), both MIT — see its [LICENSE](kicad-render/LICENSE).

## Quick start

```sh
cargo build --release

# Convert a KiCad board to JSON5 and back
./target/release/kicad-json5 input.kicad_pcb -o board.json5
./target/release/kicad-json5 board.json5 -o roundtrip.kicad_pcb

# Render a schematic to SVG
./target/release/kicad-render input.kicad_sch -o schematic.svg
```

## Development

```sh
cargo test --workspace          # runs on committed fixtures, no external files needed
cargo clippy --workspace --all-targets -- -D warnings
```

Real-world boards/schematics from the HQ EDA (华秋 EDA) online viewer demo set
are exercised by opt-in tests: `KICAD_DEMO_DIR=/path/to/demo cargo test -- --ignored`
(see `*/tests/fixtures/README.md`).
