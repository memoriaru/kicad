# kicad-symgen

KiCad component symbol (.kicad_sym) and footprint (.kicad_mod) generator, optimized for AI-assisted circuit design.

Works with the [kicad-cdb](../kicad-cdb) component database, or accepts JSON5 input independently.

## Installation

```bash
cd kicad-symgen
cargo build --release
# Binary: target/release/symgen
```

## Usage

### Generate Symbols (.kicad_sym)

```bash
# From JSON5 input (recommended for AI workflows)
symgen symbol --input fp6277.json5 --output custom.kicad_sym

# From component database
symgen symbol --db components.db --mpn FP6277 --output custom.kicad_sym

# Batch export entire category
symgen symbol --db components.db --category IC --output custom.kicad_sym
```

JSON5 input example:

```json5
{
  mpn: "FP6277",
  reference_prefix: "U",
  description: "2A synchronous boost converter",
  package: "SOT-23-6",
  kicad_footprint: "Package_TO_SOT_SMD:SOT-23-6",
  pins: [
    { number: "1", name: "LX", electrical_type: "power_out" },
    { number: "2", name: "GND", electrical_type: "power_in" },
    { number: "3", name: "EN", electrical_type: "input" },
    { number: "4", name: "FB", electrical_type: "input" },
    { number: "5", name: "VCC", electrical_type: "power_in" },
    { number: "6", name: "SW", electrical_type: "power_out" },
  ],
}
```

### Generate Footprints (.kicad_mod)

```bash
symgen footprint --package DIP-8 --output DIP-8.kicad_mod
symgen footprint --package SOIC-8 --pitch 1.27 --output SOIC-8.kicad_mod
symgen footprint --package SOT-23-6 --output SOT-23-6.kicad_mod
symgen footprint --package TSSOP-20 --output TSSOP-20.kicad_mod
```

Supported package types: DIP, SIP, SOIC, SOP, MSOP, TSSOP, QFP, LQFP, TQFP, QFN, DFN, SOT-23, SOT-223, Pin Header, DIP Socket.

### Generate Library Tables

```bash
symgen lib-table --dir ./libraries
# Generates sym-lib-table and fp-lib-table
```

### Batch Generation

```bash
symgen batch --db components.db --category IC --output-dir ./libraries --with-footprints
```

## Smart Pin Layout

Symbol generation uses an intelligent pin layout algorithm:

- **Power pins (VCC/VDD)** → Top of symbol body
- **Ground pins (GND/VSS)** → Bottom of symbol body
- **Input pins** → Left side
- **Output pins** → Right side
- **Bidirectional pins** → Assigned by pin_group

Automatically adds `custom:` prefix to lib_id, resolving KiCad ERC `lib_symbol_issues` warnings.

## Architecture

```
kicad-symgen/
  src/
    main.rs                 CLI entry point (clap)
    model.rs                Core data models
    input.rs                Input adapter (DB/JSON5)
    symbol/
      layout.rs             Smart pin layout algorithm
      sexpr.rs              .kicad_sym S-expression output
    footprint/
      pad.rs                Pad position calculation
      outline.rs            Silkscreen/assembly/soldermask outlines
      sexpr.rs              .kicad_mod S-expression output
      templates/
        dip.rs              DIP/SIP through-hole templates
        soic.rs             SOIC/TSSOP/SOP SMD templates
        sot.rs              SOT-23/SOT-223 templates
    lib_table.rs            sym-lib-table / fp-lib-table generation
```

## Toolchain Integration

| Tool | Function |
|------|----------|
| [kicad-cdb](../kicad-cdb) | Component database (SQLite), stores MPN/pin/parameters |
| **kicad-symgen** | Generate symbol and footprint files from DB/JSON5 |
| [kicad-json5](../kicad-json5) | Bidirectional .kicad_sch ↔ JSON5 compiler |

## License

MIT
