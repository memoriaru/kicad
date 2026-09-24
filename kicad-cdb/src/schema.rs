use anyhow::Result;
use rusqlite::Connection;

/// Current schema version — increment when adding migrations
pub const SCHEMA_VERSION: u32 = 7;

/// v1: initial schema
const SCHEMA_SQL_V1: &str = r#"
PRAGMA foreign_keys = ON;
PRAGMA journal_mode = WAL;

-- 1. Element categories (tree structure)
CREATE TABLE IF NOT EXISTS categories (
    id          INTEGER PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE,
    parent_id   INTEGER REFERENCES categories(id),
    description TEXT
);

-- 2. Components (main table)
CREATE TABLE IF NOT EXISTS components (
    id          INTEGER PRIMARY KEY,
    mpn         TEXT NOT NULL,
    manufacturer TEXT NOT NULL,
    category_id INTEGER NOT NULL REFERENCES categories(id),
    description TEXT,
    package     TEXT,
    lifecycle   TEXT DEFAULT 'active',
    datasheet_url TEXT,
    kicad_symbol   TEXT,
    kicad_footprint TEXT,
    created_at  TEXT DEFAULT (datetime('now')),
    updated_at  TEXT DEFAULT (datetime('now')),
    UNIQUE(mpn, manufacturer)
);

-- 3. Pin definitions
CREATE TABLE IF NOT EXISTS pins (
    id            INTEGER PRIMARY KEY,
    component_id  INTEGER NOT NULL REFERENCES components(id) ON DELETE CASCADE,
    pin_number    TEXT NOT NULL,
    pin_name      TEXT NOT NULL,
    pin_group     TEXT,
    electrical_type TEXT,
    alt_functions TEXT,
    description   TEXT,
    UNIQUE(component_id, pin_number)
);

-- 4. Electrical parameters (EAV pattern)
CREATE TABLE IF NOT EXISTS parameters (
    id            INTEGER PRIMARY KEY,
    component_id  INTEGER NOT NULL REFERENCES components(id) ON DELETE CASCADE,
    name          TEXT NOT NULL,
    value_numeric REAL,
    value_text    TEXT,
    unit          TEXT,
    typical       INTEGER DEFAULT 0,
    condition     TEXT,
    source_page   TEXT,
    UNIQUE(component_id, name, typical)
);

-- 5. Simulation models (large text fields)
CREATE TABLE IF NOT EXISTS simulation_models (
    id            INTEGER PRIMARY KEY,
    component_id  INTEGER NOT NULL REFERENCES components(id) ON DELETE CASCADE,
    model_type    TEXT NOT NULL,
    model_subcategory TEXT,
    model_text    TEXT NOT NULL,
    format        TEXT,
    port_mapping  TEXT,
    verified      INTEGER DEFAULT 0,
    source        TEXT,
    notes         TEXT,
    UNIQUE(component_id, model_type, model_subcategory)
);

-- 6. Design rules / constraint templates
CREATE TABLE IF NOT EXISTS design_rules (
    id            INTEGER PRIMARY KEY,
    name          TEXT NOT NULL UNIQUE,
    category_id   INTEGER REFERENCES categories(id),
    description   TEXT,
    condition_expr TEXT,
    formula_expr  TEXT,
    check_expr    TEXT,
    parameters    TEXT,
    output_params TEXT,
    source        TEXT
);

-- 7. Supply chain info
CREATE TABLE IF NOT EXISTS supply_info (
    id            INTEGER PRIMARY KEY,
    component_id  INTEGER NOT NULL REFERENCES components(id) ON DELETE CASCADE,
    supplier      TEXT NOT NULL,
    sku           TEXT,
    price_breaks  TEXT,
    stock         INTEGER,
    lead_time_days INTEGER,
    moq           INTEGER,
    UNIQUE(component_id, supplier)
);

-- 8. Reference circuits / application circuits
CREATE TABLE IF NOT EXISTS reference_circuits (
    id            INTEGER PRIMARY KEY,
    component_id  INTEGER NOT NULL REFERENCES components(id) ON DELETE CASCADE,
    name          TEXT NOT NULL,
    description   TEXT,
    topology      TEXT,
    circuit_json  TEXT,
    notes         TEXT,
    UNIQUE(component_id, name)
);

-- Indexes
CREATE INDEX IF NOT EXISTS idx_components_category ON components(category_id);
CREATE INDEX IF NOT EXISTS idx_components_mpn ON components(mpn);
CREATE INDEX IF NOT EXISTS idx_components_package ON components(package);
CREATE INDEX IF NOT EXISTS idx_pins_component ON pins(component_id);
CREATE INDEX IF NOT EXISTS idx_params_component ON parameters(component_id);
CREATE INDEX IF NOT EXISTS idx_params_name ON parameters(name);
CREATE INDEX IF NOT EXISTS idx_params_value ON parameters(name, value_numeric);
CREATE INDEX IF NOT EXISTS idx_sim_component ON simulation_models(component_id);
CREATE INDEX IF NOT EXISTS idx_rules_category ON design_rules(category_id);
CREATE INDEX IF NOT EXISTS idx_supply_component ON supply_info(component_id);
"#;

/// Migration descriptors: (name, SQL to execute)
const MIGRATIONS: &[(&str, &str)] = &[
    ("v1_initial", SCHEMA_SQL_V1),
    ("v2_add_lib_paths", SCHEMA_SQL_V2),
    ("v3_add_rule_metadata", SCHEMA_SQL_V3),
    ("v4_add_model_3d_path", SCHEMA_SQL_V4),
    ("v5_add_template_tables", SCHEMA_SQL_V5),
    ("v6_add_reference_designs", SCHEMA_SQL_V6),
    ("v7_add_design_snapshots", SCHEMA_SQL_V7),
    ("v8_snapshot_composite_pk", SCHEMA_SQL_V8),
    ("v9_footprint_metadata", SCHEMA_SQL_V9),
];

const SCHEMA_SQL_V2: &str = r#"
ALTER TABLE components ADD COLUMN symbol_lib_path TEXT;
ALTER TABLE components ADD COLUMN footprint_lib_path TEXT;
"#;

const SCHEMA_SQL_V3: &str = r#"
ALTER TABLE design_rules ADD COLUMN domain TEXT;
ALTER TABLE design_rules ADD COLUMN tags TEXT;
"#;

const SCHEMA_SQL_V4: &str = r#"
ALTER TABLE components ADD COLUMN model_3d_path TEXT;
"#;

const SCHEMA_SQL_V5: &str = r#"
-- IC core templates (auto-imported from ic-templates/*.json)
CREATE TABLE IF NOT EXISTS ic_templates (
    name        TEXT PRIMARY KEY,
    description TEXT,
    json_data   TEXT NOT NULL,
    updated_at  TEXT DEFAULT (datetime('now'))
);

-- Topology templates (auto-imported from templates/*.json)
CREATE TABLE IF NOT EXISTS topology_templates (
    name        TEXT PRIMARY KEY,
    description TEXT,
    json_data   TEXT NOT NULL,
    updated_at  TEXT DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_ic_templates_name ON ic_templates(name);
CREATE INDEX IF NOT EXISTS idx_topology_templates_name ON topology_templates(name);
"#;

const SCHEMA_SQL_V6: &str = r#"
-- Reference designs (complete board-level designs for reuse)
CREATE TABLE IF NOT EXISTS reference_designs (
    id            INTEGER PRIMARY KEY,
    name          TEXT NOT NULL UNIQUE,
    description   TEXT,
    tags          TEXT,
    topology      TEXT,
    requirements  TEXT,
    schematic     TEXT,
    parameters    TEXT,
    verified      INTEGER DEFAULT 0,
    created_at    TEXT DEFAULT (datetime('now')),
    updated_at    TEXT DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_ref_designs_name ON reference_designs(name);
"#;

const SCHEMA_SQL_V7: &str = r#"
-- Design snapshots for iteration support
CREATE TABLE IF NOT EXISTS design_snapshots (
    id                TEXT PRIMARY KEY,
    name              TEXT NOT NULL,
    version           INTEGER NOT NULL DEFAULT 1,
    spec_json         TEXT NOT NULL,
    composition_json  TEXT NOT NULL,
    power_tree_json   TEXT,
    pipeline_cache_json TEXT,
    schematic_json    TEXT,
    created_at        TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at        TEXT NOT NULL DEFAULT (datetime('now'))
);
"#;

const SCHEMA_SQL_V8: &str = r#"
-- H1: composite PK (id, version) to retain version history.
-- SQLite cannot ALTER a PRIMARY KEY, so DROP+RECREATE. The snapshots table
-- holds design iterations (small row count in dev), data loss is acceptable.
-- This fixes the version-overwrite bug where INSERT OR REPLACE on the single
-- `id` PK silently discarded all prior versions on every update_design call.
DROP TABLE IF EXISTS design_snapshots;
CREATE TABLE design_snapshots (
    id                TEXT NOT NULL,
    name              TEXT NOT NULL,
    version           INTEGER NOT NULL DEFAULT 1,
    spec_json         TEXT NOT NULL,
    composition_json  TEXT NOT NULL,
    power_tree_json   TEXT,
    pipeline_cache_json TEXT,
    schematic_json    TEXT,
    created_at        TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at        TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (id, version)
);
CREATE INDEX IF NOT EXISTS idx_snapshots_name ON design_snapshots(name);
"#;

const SCHEMA_SQL_V9: &str = r#"
-- H3: Footprint metadata from KiCad system libraries
CREATE TABLE IF NOT EXISTS footprint_metadata (
    lib_id       TEXT PRIMARY KEY,
    pad_count    INTEGER NOT NULL,
    pitch        REAL NOT NULL,
    row_spacing  REAL,
    body_w       REAL,
    body_h       REAL,
    drill_size   REAL,
    pad_w        REAL,
    pad_h        REAL,
    source_lib   TEXT NOT NULL
);
"#;

/// Run all pending schema migrations.
/// Returns the final schema version after migration.
pub fn run_migrations(conn: &Connection) -> Result<u32> {
    let current: u32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;

    for (i, (_name, sql)) in MIGRATIONS.iter().enumerate() {
        let target = (i + 1) as u32;
        if current < target {
            conn.execute_batch(sql)?;
            conn.pragma_update(None, "user_version", target)?;
        }
    }

    let version: u32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    Ok(version)
}

/// Legacy constant kept for backward compat with tests that reference SCHEMA_SQL directly.
pub const SCHEMA_SQL: &str = SCHEMA_SQL_V1;
