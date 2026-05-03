# kicad-cdb

Component database for AI-assisted circuit design, built on SQLite with parametric queries, design rule checking, and KiCad symbol export.

## Features

- **Component management** — MPN, manufacturer, category, package, lifecycle, datasheet links
- **Hierarchical categories** — Tree-structured taxonomy (e.g., `passive > capacitor > ceramic`)
- **Pin definitions** — Pin numbers, names, electrical types, alternate functions
- **Electrical parameters** — Numeric/text parameters + units, with typical value markers
- **Parametric range queries** — Filter by `capacitance>=1e-7`, `voltage>=25`, etc.
- **Full-text search** — Search across descriptions and metadata
- **Simulation models** — SPICE / IBIS / Verilog-AMS / S-parameter model storage
- **Design rule engine** — Mathematical expression evaluation, parameter constraint checking
- **Supply chain info** — Supplier, SKU, price breaks, stock, lead time, MOQ
- **KiCad export** — Generate `.kicad_sym` symbol library files

## Installation

```bash
cargo build --release
# Binary: target/release/cdb
```

## CLI Usage

```bash
cdb [OPTIONS] <COMMAND>
```

### Global Options

| Option | Description |
|--------|-------------|
| `--db <PATH>` | Database path (default: `components.db`, supports `:memory:`) |

### Import Components

```bash
# Import single component (JSON format)
cdb import component.json

# Batch import
cdb import components.json
```

JSON import format example:

```json
{
  "mpn": "LM358",
  "manufacturer": "TI",
  "category": "amplifiers/opamps",
  "description": "Dual Operational Amplifier",
  "package": "SOIC-8",
  "kicad_symbol": "Amplifier_Operational:LM358",
  "pins": [
    { "number": "1", "name": "OUT_A", "electrical_type": "output" },
    { "number": "2", "name": "IN-_A", "electrical_type": "input" },
    { "number": "3", "name": "IN+_A", "electrical_type": "input" },
    { "number": "4", "name": "V-", "electrical_type": "power_in" },
    { "number": "5", "name": "IN+_B", "electrical_type": "input" },
    { "number": "6", "name": "IN-_B", "electrical_type": "input" },
    { "number": "7", "name": "OUT_B", "electrical_type": "output" },
    { "number": "8", "name": "V+", "electrical_type": "power_in" }
  ],
  "parameters": [
    { "name": "supply_voltage_max", "value_numeric": 32.0, "unit": "V" },
    { "name": "gain_bandwidth", "value_numeric": 1.1e6, "unit": "Hz", "typical": true }
  ]
}
```

### Query Components

```bash
# By category
cdb query --category "capacitors"

# By parameter range
cdb query --param "capacitance>=1e-7"
cdb query --param "voltage>=25"

# By package
cdb query --package "SOIC-8"

# Full-text search
cdb query --search "STM32F4"

# In-stock only
cdb query --in-stock

# Combined filters
cdb query --category "capacitors" --param "voltage>=25" --search "ceramic"
```

### View Component Details

```bash
cdb show LM358
```

### Design Rule Checking

```bash
cdb check --rule "power_dissipation" --params "voltage=5,current=0.1" --candidate "resistance=50"
```

### Export KiCad Symbol Library

```bash
# Export single component
cdb export --mpn LM358 --output lm358.kicad_sym

# Export entire category
cdb export --category "opamps" --output opamps.kicad_sym
```

### Other Commands

```bash
# List all categories
cdb categories

# Import simulation model
cdb import-model --mpn LM358 --model-type spice --path lm358.cir --format spice3
```

## Database Schema

SQLite, main tables:

| Table | Description |
|-------|-------------|
| `categories` | Hierarchical categories (id, name, parent_id) |
| `components` | Component master (mpn, manufacturer, package, lifecycle) |
| `pins` | Pin definitions (pin_number, name, electrical_type) |
| `parameters` | Electrical parameters EAV model (name, value_numeric, unit) |
| `simulation_models` | Simulation models (model_type, model_text, format) |
| `design_rules` | Design rules (condition_expr, formula_expr) |
| `supply_info` | Supply chain (supplier, sku, price_breaks, stock) |
| `reference_circuits` | Reference circuits (topology, circuit_json) |

## Project Structure

```
kicad-cdb/
├── src/
│   ├── bin/cdb.rs       # CLI entry point
│   ├── db.rs            # Database operations
│   ├── models.rs        # Data structure definitions
│   ├── schema.rs        # Table schema definitions
│   ├── query.rs         # Query engine
│   ├── rules.rs         # Design rule engine
│   ├── import.rs        # JSON import (single/batch/upsert)
│   └── lib.rs           # Library entry point
├── Cargo.toml
└── tests/               # Integration tests
    ├── schema_test.rs   # Schema and CRUD tests
    └── rules_test.rs    # Rule engine tests
```

## Dependencies

- `rusqlite` — SQLite (bundled)
- `serde` / `serde_json` — Serialization
- `clap` — CLI argument parsing
- `anyhow` — Error handling

## License

MIT
