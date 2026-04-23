use anyhow::{Result, Context};
use clap::{Parser, Subcommand};
use component_db::ComponentDb;

#[derive(Parser)]
#[command(name = "cdb", about = "Component Database CLI for AI-assisted circuit design")]
struct Cli {
    /// Database file path (use :memory: for in-memory)
    #[arg(long, default_value = "components.db")]
    db: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Import components from a JSON file
    Import {
        /// Path to JSON file (single object or array)
        path: String,
    },

    /// Import a SPICE/IBIS model for an existing component
    ImportModel {
        /// Component MPN
        #[arg(long)]
        mpn: String,

        /// Model type: spice, ibis, verilog_ams, sparameter
        #[arg(long)]
        model_type: String,

        /// Path to model file
        path: String,

        /// Model format (e.g. spice3, ltspice, ibis_v2.1)
        #[arg(long, default_value = "spice3")]
        format: String,
    },

    /// Query components with filters
    Query {
        /// Category name filter
        #[arg(long)]
        category: Option<String>,

        /// Parameter filter: name>=min (e.g. "capacitance>=1e-7")
        #[arg(long)]
        param: Option<String>,

        /// Package filter
        #[arg(long)]
        package: Option<String>,

        /// Full-text search query
        #[arg(short, long)]
        search: Option<String>,

        /// Show only in-stock components
        #[arg(long)]
        in_stock: bool,
    },

    /// Show component details
    Show {
        /// Component MPN
        mpn: String,
    },

    /// List all categories
    Categories,

    /// Apply a design rule with given parameters
    Check {
        /// Rule name
        #[arg(long)]
        rule: String,

        /// Parameters as key=value pairs (comma-separated)
        #[arg(long)]
        params: String,

        /// Candidate component value to check (name=value)
        #[arg(long)]
        candidate: Option<String>,
    },

    /// Export component(s) as KiCad .kicad_sym
    Export {
        /// MPN to export
        #[arg(long)]
        mpn: Option<String>,

        /// Category to export (all components in category)
        #[arg(long)]
        category: Option<String>,

        /// Output file path
        #[arg(short, long)]
        output: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.db == ":memory:" {
        eprintln!("Note: using in-memory database, data will not persist");
    }
    let db = ComponentDb::open(&cli.db)?;

    match cli.command {
        Commands::Import { path } => cmd_import(&db, &path),
        Commands::ImportModel { mpn, model_type, path, format } => {
            cmd_import_model(&db, &mpn, &model_type, &path, &format)
        }
        Commands::Query { category, param, package, search, in_stock } => {
            cmd_query(&db, category.as_deref(), param.as_deref(), package.as_deref(), search.as_deref(), in_stock)
        }
        Commands::Show { mpn } => cmd_show(&db, &mpn),
        Commands::Categories => cmd_categories(&db),
        Commands::Check { rule, params, candidate } => {
            cmd_check(&db, &rule, &params, candidate.as_deref())
        }
        Commands::Export { mpn, category, output } => {
            cmd_export(&db, mpn.as_deref(), category.as_deref(), &output)
        }
    }
}

fn cmd_import(db: &ComponentDb, path: &str) -> Result<()> {
    let content = std::fs::read_to_string(path)?;
    let trimmed = content.trim();

    if trimmed.starts_with('[') {
        let ids = db.import_batch_from_json(trimmed)?;
        println!("Imported {} components", ids.len());
        for id in &ids {
            if let Some(comp) = db.get_component(*id)? {
                println!("  {} - {} ({})", comp.mpn, comp.manufacturer, comp.package.as_deref().unwrap_or("?"));
            }
        }
    } else {
        let id = db.import_from_json(trimmed)?;
        let comp = db.get_component(id)?.unwrap();
        println!("Imported: {} - {} (id={})", comp.mpn, comp.manufacturer, id);
    }
    Ok(())
}

fn cmd_import_model(db: &ComponentDb, mpn: &str, model_type: &str, path: &str, format: &str) -> Result<()> {
    // Find component by MPN (any manufacturer)
    let comp = db.conn.query_row(
        "SELECT id FROM components WHERE mpn = ?1 LIMIT 1",
        rusqlite::params![mpn],
        |row| row.get::<_, i64>(0),
    ).map_err(|_| anyhow::anyhow!("Component '{}' not found", mpn))?;

    let model_text = std::fs::read_to_string(path)?;
    let model_id = db.import_simulation_model(comp, model_type, None, &model_text, format)?;
    println!("Imported {} model for {} (id={})", model_type, mpn, model_id);
    Ok(())
}

fn cmd_query(
    db: &ComponentDb,
    category: Option<&str>,
    param: Option<&str>,
    package: Option<&str>,
    search: Option<&str>,
    in_stock: bool,
) -> Result<()> {
    let mut results = Vec::new();

    if let Some(cat) = category {
        results = db.query_components_by_category(cat)?;
    }

    if let Some(query) = search {
        results = db.search(query)?;
    }

    if in_stock {
        let stocked = db.query_in_stock()?;
        if results.is_empty() {
            results = stocked;
        } else {
            let stocked_ids: std::collections::HashSet<i64> = stocked.iter()
                .filter_map(|c| c.id).collect();
            results.retain(|c| c.id.map(|id| stocked_ids.contains(&id)).unwrap_or(false));
        }
    }

    if let Some(pkg) = package {
        results.retain(|c| c.package.as_deref() == Some(pkg));
    }

    if let Some(param_filter) = param {
        let (name, min) = parse_param_filter(param_filter)?;
        let filtered = db.query_by_parameter_range(name, Some(min), None)?;
        if results.is_empty() {
            results = filtered;
        } else {
            let filtered_ids: std::collections::HashSet<i64> = filtered.iter()
                .filter_map(|c| c.id).collect();
            results.retain(|c| c.id.map(|id| filtered_ids.contains(&id)).unwrap_or(false));
        }
    }

    if results.is_empty() {
        println!("No components found");
    } else {
        println!("Found {} components:", results.len());
        for c in &results {
            println!("  {} | {} | {} | {}",
                c.mpn,
                c.manufacturer,
                c.package.as_deref().unwrap_or("-"),
                c.description.as_deref().unwrap_or("-")
            );
        }
    }
    Ok(())
}

fn parse_param_filter(filter: &str) -> Result<(&str, f64)> {
    if let Some(pos) = filter.find(">=") {
        let name = filter[..pos].trim();
        let val: f64 = filter[pos+2..].parse()?;
        return Ok((name, val));
    }
    anyhow::bail!("Parameter filter must be 'name>=value', got: {}", filter);
}

fn cmd_show(db: &ComponentDb, mpn: &str) -> Result<()> {
    let comp = db.conn.query_row(
        "SELECT id, mpn, manufacturer, category_id, description, package, lifecycle, datasheet_url, kicad_symbol, kicad_footprint
         FROM components WHERE mpn = ?1 LIMIT 1",
        rusqlite::params![mpn],
        |row| Ok(component_db::Component {
            id: Some(row.get(0)?), mpn: row.get(1)?, manufacturer: row.get(2)?,
            category_id: row.get(3)?, description: row.get(4)?, package: row.get(5)?,
            lifecycle: row.get(6)?, datasheet_url: row.get(7)?,
            kicad_symbol: row.get(8)?, kicad_footprint: row.get(9)?,
        }),
    ).map_err(|_| anyhow::anyhow!("Component '{}' not found", mpn))?;

    let id = comp.id.unwrap();

    println!("MPN:          {}", comp.mpn);
    println!("Manufacturer: {}", comp.manufacturer);
    println!("Package:      {}", comp.package.as_deref().unwrap_or("-"));
    println!("Lifecycle:    {}", comp.lifecycle);
    if let Some(ref desc) = comp.description { println!("Description:  {}", desc); }
    if let Some(ref url) = comp.datasheet_url { println!("Datasheet:    {}", url); }
    if let Some(ref sym) = comp.kicad_symbol { println!("KiCad Symbol: {}", sym); }
    if let Some(ref fp) = comp.kicad_footprint { println!("KiCad Footprint: {}", fp); }

    // Pins
    let pins = db.get_pins(id)?;
    if !pins.is_empty() {
        println!("\n--- Pins ({}) ---", pins.len());
        for p in &pins {
            let alts = p.alt_functions.as_ref()
                .map(|a| format!(" [{}]", a.join(", ")))
                .unwrap_or_default();
            println!("  {:>4} {:<12} {:<15}{}{}", p.pin_number, p.pin_name,
                p.electrical_type.as_deref().unwrap_or("-"), alts,
                p.description.as_ref().map(|d| format!(" - {}", d)).unwrap_or_default());
        }
    }

    // Parameters
    let params = db.get_parameters(id)?;
    if !params.is_empty() {
        println!("\n--- Parameters ({}) ---", params.len());
        for p in &params {
            let val = match (p.value_numeric, &p.value_text) {
                (Some(n), _) => format!("{:.6e}", n),
                (_, Some(t)) => t.clone(),
                _ => "-".to_string(),
            };
            let typ = if p.typical { "typ" } else { "" };
            println!("  {:<20} {} {} {}", p.name, val, p.unit.as_deref().unwrap_or(""), typ);
        }
    }

    // Simulation models
    let models = db.get_simulation_models(id)?;
    if !models.is_empty() {
        println!("\n--- Simulation Models ({}) ---", models.len());
        for m in &models {
            println!("  {} ({}) - {} chars", m.model_type,
                m.format.as_deref().unwrap_or("?"), m.model_text.len());
        }
    }

    // Supply info
    let supply = db.get_supply_info(id)?;
    if !supply.is_empty() {
        println!("\n--- Supply ({}) ---", supply.len());
        for s in &supply {
            println!("  {} | SKU: {} | Stock: {}",
                s.supplier, s.sku.as_deref().unwrap_or("-"),
                s.stock.map(|n| n.to_string()).unwrap_or("-".to_string()));
        }
    }

    Ok(())
}

fn cmd_categories(db: &ComponentDb) -> Result<()> {
    let cats: Vec<component_db::Category> = db.conn
        .prepare("SELECT id, name, parent_id, description FROM categories ORDER BY name")?
        .query_map([], |row| Ok(component_db::Category {
            id: Some(row.get(0)?), name: row.get(1)?, parent_id: row.get(2)?, description: row.get(3)?,
        }))?.filter_map(|r| r.ok()).collect();

    for cat in &cats {
        let indent = if cat.parent_id.is_some() { "  " } else { "" };
        println!("{}{} (id={})", indent, cat.name, cat.id.unwrap());
    }
    println!("\nTotal: {} categories", cats.len());
    Ok(())
}

fn cmd_check(db: &ComponentDb, rule_name: &str, params_str: &str, candidate: Option<&str>) -> Result<()> {
    let rule = db.conn.query_row(
        "SELECT id, name, category_id, description, condition_expr, formula_expr, check_expr, parameters, output_params, source
         FROM design_rules WHERE name = ?1",
        rusqlite::params![rule_name],
        |row| Ok(component_db::DesignRule {
            id: Some(row.get(0)?), name: row.get(1)?, category_id: row.get(2)?,
            description: row.get(3)?, condition_expr: row.get(4)?, formula_expr: row.get(5)?,
            check_expr: row.get(6)?, parameters: row.get(7)?, output_params: row.get(8)?,
            source: row.get(9)?,
        }),
    ).map_err(|_| anyhow::anyhow!("Rule '{}' not found", rule_name))?;

    // Parse params
    let mut inputs = serde_json::Map::new();
    for pair in params_str.split(',') {
        let kv: Vec<&str> = pair.splitn(2, '=').collect();
        if kv.len() == 2 {
            let val: f64 = kv[1].parse().context(format!("Invalid number: {}", kv[1]))?;
            inputs.insert(kv[0].trim().to_string(), serde_json::Value::from(val));
        }
    }

    // Parse candidate
    let (cand_name, cand_val) = match candidate {
        Some(c) => {
            let kv: Vec<&str> = c.splitn(2, '=').collect();
            if kv.len() == 2 {
                (Some(kv[0].trim().to_string()), Some(kv[1].parse::<f64>()?))
            } else {
                (None, None)
            }
        }
        None => (None, None),
    };

    let result = db.apply_rule(&rule, &serde_json::Value::Object(inputs), cand_name.as_deref(), cand_val)?;

    println!("Rule: {}", rule_name);
    if let Some(desc) = &rule.description { println!("  {}", desc); }
    for (name, val) in &result.outputs {
        println!("  {} = {:.6e}", name, val);
    }
    println!("Check: {} => {}", result.check_expression, if result.pass { "PASS" } else { "FAIL" });

    Ok(())
}

fn cmd_export(db: &ComponentDb, mpn: Option<&str>, category: Option<&str>, output: &str) -> Result<()> {
    let components = match (mpn, category) {
        (Some(mpn), _) => {
            let comp = db.get_component_by_mpn(mpn, "")?;
            // Try without manufacturer
            if comp.is_none() {
                let c = db.conn.query_row(
                    "SELECT id, mpn, manufacturer, category_id, description, package, lifecycle, datasheet_url, kicad_symbol, kicad_footprint
                     FROM components WHERE mpn = ?1 LIMIT 1",
                    rusqlite::params![mpn],
                    |row| Ok(component_db::Component {
                        id: Some(row.get(0)?), mpn: row.get(1)?, manufacturer: row.get(2)?,
                        category_id: row.get(3)?, description: row.get(4)?, package: row.get(5)?,
                        lifecycle: row.get(6)?, datasheet_url: row.get(7)?,
                        kicad_symbol: row.get(8)?, kicad_footprint: row.get(9)?,
                    }),
                ).ok();
                vec![c].into_iter().flatten().collect()
            } else {
                vec![comp.unwrap()]
            }
        }
        (None, Some(cat)) => db.query_components_by_category(cat)?,
        _ => anyhow::bail!("Specify --mpn or --category for export"),
    };

    if components.is_empty() {
        anyhow::bail!("No components found to export");
    }

    // Generate .kicad_sym
    let mut out = String::new();
    out.push_str("(kicad_symbol_lib\n");
    out.push_str("  (version \"20231120\")\n");
    out.push_str("  (generator \"component-db\")\n");

    for comp in &components {
        let id = comp.id.unwrap();
        let pins = db.get_pins(id)?;
        let name = comp.mpn.replace('.', "_");

        out.push_str(&format!("  (symbol \"{}\"\n", name));
        out.push_str("    (in_bom yes)\n");
        out.push_str("    (on_board yes)\n");

        // Properties
        out.push_str(&format!("    (property \"Reference\" \"U?\" (at 0 1.27 0)\n"));
        out.push_str("      (effects (font (size 1.27 1.27))))\n");
        out.push_str(&format!("    (property \"Value\" \"{}\" (at 0 -1.27 0)\n", comp.mpn));
        out.push_str("      (effects (font (size 1.27 1.27))))\n");
        if let Some(ref fp) = comp.kicad_footprint {
            out.push_str(&format!("    (property \"Footprint\" \"{}\" (at 0 -2.54 0)\n", fp));
            out.push_str("      (effects (font (size 1.27 1.27)) hide))\n");
        }

        // Body rectangle (_0_1)
        let pin_count = pins.len() as f64;
        let left_pins = (pin_count / 2.0).ceil() as i32;
        let body_half_h = left_pins as f64 * 1.27;
        out.push_str(&format!("    (symbol \"{}_0_1\"\n", name));
        out.push_str(&format!("      (rectangle (start -5.08 {}) (end 5.08 {})\n",
            body_half_h + 1.27, -(body_half_h + 1.27)));
        out.push_str("        (stroke (width 0.254) (type default))\n");
        out.push_str("        (fill (type background)))\n");
        out.push_str("    )\n");

        // Pins (_1_1)
        if !pins.is_empty() {
            out.push_str(&format!("    (symbol \"{}_1_1\"\n", name));
            let mut y_left = body_half_h;
            let mut y_right = body_half_h;
            for (i, pin) in pins.iter().enumerate() {
                let etype = pin.electrical_type.as_deref().unwrap_or("passive");
                if i < left_pins as usize {
                    out.push_str(&format!("      (pin {} line (at -7.62 {} 0) (length 2.54)\n", etype, y_left));
                    y_left -= 2.54;
                } else {
                    out.push_str(&format!("      (pin {} line (at 7.62 {} 180) (length 2.54)\n", etype, y_right));
                    y_right -= 2.54;
                }
                out.push_str(&format!("        (name \"{}\" (effects (font (size 1.27 1.27))))\n", pin.pin_name));
                out.push_str(&format!("        (number \"{}\" (effects (font (size 1.27 1.27))))\n", pin.pin_number));
                out.push_str("      )\n");
            }
            out.push_str("    )\n");
        }

        out.push_str("  )\n");
    }

    out.push_str(")\n");

    std::fs::write(output, &out)?;
    println!("Exported {} components to {}", components.len(), output);
    Ok(())
}
