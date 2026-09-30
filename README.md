# kicad

Rust toolchain for programmatic KiCad design: a data-driven pipeline from
component knowledge to schematics, PCB layout, routing, verification, and
rendering — usable as libraries, a CLI (`kdesign`), an MCP server for AI
agents, and a gRPC routing service.

| Crate | What it does |
| --- | --- |
| [kicad-json5](kicad-json5/) | KiCad S-expression ↔ JSON5 bidirectional compiler for schematics and PCBs (KiCad 7–10), with topology extraction and board-level auto-layout |
| [kicad-symgen](kicad-symgen/) | Parameterized KiCad symbol & footprint generator (IC / connector templates) |
| [kicad-render](kicad-render/) | Schematic & PCB SVG renderer — a 1:1 Rust port of the [KiCanvas](https://github.com/theacodes/kicanvas) painters, with layered output, PDF/PNG export |
| [kicad-cdb](kicad-cdb/) | SQLite component database + placement/routing/DRC engines + rule engine and design skills |
| [kroute-server](kroute-server/) | gRPC autorouting server: wavefront router (CPU / wgpu / CUDA) and freerouting pipeline |
| [kicad-designer](kicad-designer/) | `kdesign` CLI + MCP Server orchestrating the whole toolchain (46 tools) |

Dependency flow: `kicad-json5` ← `kicad-symgen` / `kicad-render` / `kicad-cdb`
← `kroute-server` / `kicad-designer`.

All crates are MIT licensed. `kicad-render` derives from KiCanvas
(© 2022 Alethea Katherine Flowers) and ecad-viewer
(© 2024 深圳华秋电子有限公司), both MIT — see its [LICENSE](kicad-render/LICENSE).

## Quick start

```sh
cargo build --release

# Convert a KiCad board to JSON5 and back (stock kicad-cli loadable)
./target/release/kicad-json5 input.kicad_pcb -o board.json5
./target/release/kicad-json5 board.json5 -o roundtrip.kicad_pcb

# Render a schematic to SVG (add --layers F.Cu,B.Cu for per-layer PCB output)
./target/release/kicad-render input.kicad_sch -o schematic.svg

# Query the component database (auto-creates the schema on first open)
./target/release/kdesign --db myproject.db query --category passive/capacitor

# Start the MCP server for AI agents (stdio JSON-RPC)
./target/release/kdesign --db myproject.db serve

# Start the gRPC routing service
./target/release/kroute-server serve --port 50051
```

## Development

```sh
cargo test --workspace          # runs on committed fixtures, no external files needed
cargo clippy --workspace --all-targets -- -D warnings
```

Real-world boards/schematics from the HQ EDA (华秋 EDA) online viewer demo set
are exercised by opt-in tests: `KICAD_DEMO_DIR=/path/to/demo cargo test -- --ignored`
(see `*/tests/fixtures/README.md`).

CI runs fmt, clippy (`-D warnings`), and the full test suite on
ubuntu + macOS, plus kroute-server in CPU-only mode.
