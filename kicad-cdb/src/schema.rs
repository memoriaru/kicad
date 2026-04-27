/// Schema DDL for all tables in the component database.

pub const SCHEMA_SQL: &str = r#"
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
