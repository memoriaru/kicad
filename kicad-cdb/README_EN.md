# kicad-cdb

Core engine library for AI-assisted circuit design: SQLite component database +
placement/routing/DRC engines + design knowledge base.

> For the CLI entry point see [kicad-designer](https://github.com/memoriaru/kicad/tree/main/kicad-designer)
> (`kdesign`); this crate is the library form, for use by kdesign and your own code.

## Feature modules

| Module | What it does |
| --- | --- |
| **Component DB** (`db`/`schema`) | MPN/manufacturer/category/package/pins/parameters/supply chain, hierarchical categories, range queries, full-text search |
| **Autorouter** (`router`) | Wavefront grid routing, 3D Dijkstra, net-aware rip-up retry, differential pairs & SerDes length matching |
| **GPU routing** (`gpu_router`, optional) | wgpu compute-shader wavefront expansion (`--features gpu`, on by default; separate `cuda` feature) |
| **Placement engine** (`layout_engine`) | SA Sequence Pair global placement + power-flow chain detection + connector/peripheral grouping |
| **DRC/ERC** (`drc`/`erc`) | Full design-rule checking (clearance/width/drill…) + electrical rule checking |
| **Rule engine** (`rules`) | Math expression evaluation and parameter constraint checks |
| **Design skills** (`skills`) | 93 executable design rules + component recommendation + pipeline orchestration |
| **BOM / power tree** (`bom`/`power_tree`) | BOM generation and power topology derivation |
| **Composition** (`composition`/`design`) | Sub-circuit composition into boards, IC-level schematic generation |
| **HQ API** (`hqapi`) | HQ EDA (华秋) component library online fetch (pins/parameters/datasheets) |
| **Symbol/footprint generation** (`symgen`/`footprint`) | KiCad symbol library export, footprint metadata extraction |

## Database lifecycle

**Schema is the template**: migrations are built into the library; opening any
path for the first time creates all tables. The repository ships **no `.db`
files** — every project instantiates its own database:

```rust
use kicad_cdb::db::Db;

// Path from the CDB_PATH environment variable (toolchain convention), or any path
let db = Db::open("myproject.db")?;   // first open creates the schema
```

Populate via the HQ API fetch, CSV/JSON import (`hqapi`/`csv_import` modules),
or open a `:memory:` database for tests.

Bundled template assets (loaded relative to cwd, or imported into the DB):
- `ic-templates/` — 27 IC core templates (CH340/LM358/INA219 etc.)
- `templates/` — 9 power topology templates (boost/buck/SEPIC…)

## Library usage example

```rust
use kicad_cdb::db::Db;

let db = Db::open_in_memory()?;

db.insert_category(&kicad_cdb::models::Category {
    name: "passive/capacitor".into(),
    ..Default::default()
})?;

// Range query: capacitors rated ≥ 25 V
let hits = db.query_components("category:passive/capacitor voltage>=25", 10)?;
```

## Toolchain

| crate | What it does |
| --- | --- |
| [kicad-json5](https://github.com/memoriaru/kicad/tree/main/kicad-json5) | Bidirectional .kicad_sch/.kicad_pcb ↔ JSON5 compiler |
| [kicad-symgen](https://github.com/memoriaru/kicad/tree/main/kicad-symgen) | Parameterized symbol/footprint generation |
| [kicad-render](https://github.com/memoriaru/kicad/tree/main/kicad-render) | Schematic & PCB SVG rendering (KiCanvas port) |
| [kicad-cdb](https://github.com/memoriaru/kicad/tree/main/kicad-cdb) | This crate: component DB + placement/routing/DRC engines |

## License

MIT
