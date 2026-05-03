# kicad-json5

A bidirectional compiler between KiCad schematic S-expressions and JSON5, with circuit topology extraction. Provides a human-friendly intermediate representation for AI-assisted circuit design.

## Why JSON5?

- **Human-readable** — Comments, unquoted keys, trailing commas
- **Token-efficient** — 40-50% fewer tokens than S-expressions
- **AI-friendly** — Clean structure, LLMs can directly understand and generate
- **Bidirectional** — Full `.kicad_sch` ↔ `.json5` round-trip

## Installation

```bash
cargo build --release
# Binary: target/release/kicad-json5
```

## CLI Usage

### Basic Conversion

```bash
# S-expression → JSON5
kicad-json5 schematic.kicad_sch -o schematic.json5

# JSON5 → S-expression
kicad-json5 schematic.json5 -o schematic.kicad_sch

# Output to stdout
kicad-json5 schematic.kicad_sch
```

### All Options

| Option | Description |
|--------|-------------|
| `-o, --output <FILE>` | Output file path |
| `-f, --format <FORMAT>` | Output format: `json5`, `sexpr`, `topology` |
| `-i, --indent <N>` | Indentation spaces (default: 2) |
| `--kicad-version <V>` | Target KiCad version: 7, 8, 9, 10+ |
| `--no-comments` | JSON5 output without comments |
| `--validate` | Validate only, no output |
| `--debug-ast` | Print AST for debugging |
| `--power-flags` | Auto-insert PWR_FLAG when generating S-expressions |
| `-v, --verbose` | Verbose output |

### Examples

```bash
# JSON5 → KiCad with auto PWR_FLAG and NC markers
kicad-json5 carrier-board.json5 -o carrier-board.kicad_sch --power-flags

# Extract circuit topology
kicad-json5 schematic.kicad_sch -f topology -o topology.json

# Validate only
kicad-json5 schematic.kicad_sch --validate
```

## JSON5 Input Format

```json5
{
  // Schematic metadata
  version: "20241129",
  generator: "kicad-json5",

  title_block: {
    title: "CCD Carrier Board",
    date: "2026-04-01",
    paper: "A3",
  },

  // Component instances
  components: [
    {
      ref: "U1",
      lib_id: "Device:R",
      value: "10k",
      position: { x: 100, y: 50, rotation: 0 },
      pins: {
        "1": { net_id: 1, net_name: "VCC" },
        "2": { net_id: 5, net_name: "SDA" },
      },
    },
    {
      ref: "U3",
      lib_id: "custom:ME2802",
      value: "ME2802",
      position: { x: 39.37, y: 110.49, rotation: 0 },
      pins: {
        "1": { net_name: "GND" },
        "2": { net_name: "VOUT_5V" },
        "3": { net_name: "VBAT" },
        "4": { nc: true },   // No-Connect marker
      },
    },
  ],

  // Net definitions
  nets: [
    { id: 0, name: "GND" },
    { id: 1, name: "VCC" },
    { id: 5, name: "SDA" },
  ],
}
```

## Key Features

### Standard Device Library Embedding

Built-in KiCad standard Device library symbols (R, C, L, D, LED, NTC). When generating `.kicad_sch`, lib_symbol definitions are automatically embedded — no external library files needed to pass ERC.

### Auto Label Generation

When converting JSON5→S-expression, `global_label` elements are automatically generated for each net connection point, placed at the pin's world coordinates to ensure KiCad correctly recognizes net connectivity.

### PWR_FLAG Auto-Insertion

With the `--power-flags` option, detects nets that need power markers (have `power_in` pins but no `power_out` driver) and inserts PWR_FLAG symbols conforming to KiCad's official `power.kicad_sym` specification.

> **Known limitation (hierarchical schematics):** `--power-flags` only works for single-file (flat) schematics without sub-sheets. In hierarchical schematics with `sheets`, multiple PWR_FLAGs on the same global power net trigger KiCad ERC `pin_to_pin` conflicts. Sub-sheets in hierarchical designs require manual PWR_FLAG placement in KiCad, or implicit power net driving via power symbols (e.g., VCC/GND symbols with `power_out` pins).

### No-Connect Markers

Pins declared with `"nc": true` generate `(no_connect ...)` elements at the corresponding pin position, eliminating KiCad ERC `pin_not_connected` warnings.

### Topology Extraction

The `-f topology` mode extracts circuit semantic information:
- Power domain and ground net identification
- Signal path tracing
- Functional module recognition (pull-up resistors, decoupling capacitors, LED indicators, etc.)

## Project Structure

```
kicad-json5/
├── src/
│   ├── lexer/              # S-expression tokenizer
│   │   ├── mod.rs
│   │   ├── token.rs
│   │   └── scanner.rs
│   ├── parser/             # Parsers
│   │   ├── mod.rs
│   │   ├── ast.rs
│   │   ├── schematic.rs    # S-expression → IR
│   │   ├── json5_parser.rs # JSON5 → IR
│   │   └── s_expr_parser.rs
│   ├── ir/                 # Intermediate representation
│   │   ├── schematic.rs    # Schematic structure
│   │   ├── component.rs    # Component/pin instances
│   │   ├── net.rs          # Nets/labels/NC
│   │   └── symbol.rs       # Library symbol definitions
│   ├── codegen/            # Code generation
│   │   ├── json5_gen.rs    # IR → JSON5
│   │   ├── sexpr_gen/      # IR → S-expression
│   │   │   ├── mod.rs
│   │   │   ├── auto.rs     # Auto wire/label/PWR_FLAG generation
│   │   │   ├── element.rs  # Component instance generation
│   │   │   ├── graphic.rs  # Graphic element generation
│   │   │   └── symbol.rs   # Library symbol embedding
│   │   └── standard_symbols.rs  # Embedded standard symbols
│   ├── topology/           # Topology analysis
│   │   ├── extractor.rs    # Topology extraction
│   │   ├── classify.rs     # Component classification
│   │   ├── connectivity.rs # Connectivity analysis
│   │   ├── patterns.rs     # Circuit pattern recognition
│   │   ├── summary.rs      # Topology summary
│   │   └── types.rs        # Topology data types
│   ├── lib.rs
│   ├── main.rs             # CLI entry point
│   └── error.rs
├── docs/                   # Design documents
└── tests/                  # Integration tests
    └── sch_roundtrip.rs    # Real .kicad_sch roundtrip tests
```

## Dependencies

- `clap` — CLI argument parsing
- `serde` / `serde_json` / `serde_json5` — Serialization
- `uuid` — UUID generation
- `thiserror` / `anyhow` — Error handling

## License

MIT
