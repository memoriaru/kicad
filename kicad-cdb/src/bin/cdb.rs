use anyhow::Result;
use clap::{Parser, Subcommand};
use kicad_cdb::ComponentDb;
use serde::Serialize;

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

        /// Parameter filter: name:min:max (e.g. "capacitance:1e-7:1e-6", "capacitance:1e-7:")
        #[arg(long)]
        param: Option<String>,

        /// Manufacturer filter
        #[arg(long)]
        manufacturer: Option<String>,

        /// Package filter
        #[arg(long)]
        package: Option<String>,

        /// Full-text search query
        #[arg(short, long)]
        search: Option<String>,

        /// Show only in-stock components
        #[arg(long)]
        in_stock: bool,

        /// Limit number of results
        #[arg(short, long)]
        limit: Option<usize>,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Show component details
    Show {
        /// Component MPN
        mpn: String,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// List all categories
    Categories {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

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

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Export component(s) as KiCad .kicad_sym or JSON5 spec
    Export {
        /// MPN to export
        #[arg(long)]
        mpn: Option<String>,

        /// Category to export (all components in category)
        #[arg(long)]
        category: Option<String>,

        /// Output format: kicad_sym (default) or spec (JSON5 for symgen)
        #[arg(long, default_value = "kicad_sym")]
        format: String,

        /// Output file path
        #[arg(short, long)]
        output: String,
    },

    /// Fetch component from HuaQiu EDA and import into database
    Fetch {
        /// Manufacturer Part Number to search
        #[arg(long)]
        mpn: String,

        /// Manufacturer ID (optional, auto-detected from search)
        #[arg(long)]
        mfg_id: Option<String>,
    },

    /// Search HuaQiu online component library
    HqSearch {
        /// Search keyword (e.g. "STM32", "100nF 0805")
        keyword: String,

        /// Max results
        #[arg(short, long, default_value = "20")]
        limit: usize,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Generate a schematic from a topology template
    Design {
        /// Topology template name (ldo, buck, led)
        #[arg(long)]
        template: String,

        /// Input voltage (V)
        #[arg(long)]
        vin: f64,

        /// Output voltage (V)
        #[arg(long)]
        vout: f64,

        /// Output current (A)
        #[arg(long)]
        iout: f64,

        /// Output .kicad_sch file path
        #[arg(short, long)]
        output: String,
    },

    /// Suggest suitable power topology based on requirements
    Suggest {
        /// Input voltage (V)
        #[arg(long)]
        vin: f64,

        /// Output voltage (V)
        #[arg(long)]
        vout: f64,

        /// Output current (A)
        #[arg(long)]
        iout: f64,

        /// Require galvanic isolation
        #[arg(long, default_value = "false")]
        isolated: bool,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Compose multiple modules into a single schematic
    Compose {
        /// Path to composition JSON file
        #[arg(long)]
        file: String,

        /// Output .kicad_sch file path
        #[arg(short, long)]
        output: String,
    },

    /// Generate a schematic from an IC core template
    IcDesign {
        /// IC template name (e.g. RT9193-ADJ, EL7156)
        #[arg(long)]
        template: String,

        /// Parameters as key=value pairs (comma-separated, e.g. "vout=3.3")
        #[arg(long)]
        params: String,

        /// Net name overrides as key=value pairs (comma-separated, e.g. "VIN=+5V,GND=DGND")
        #[arg(long)]
        nets: Option<String>,

        /// Output .kicad_sch file path
        #[arg(short, long)]
        output: String,
    },

    /// Fetch pin list from HuaQiu API for an MPN (for building IC templates)
    TemplatePins {
        /// Manufacturer Part Number to search
        mpn: String,

        /// Output as JSON fragment (for pasting into template)
        #[arg(long)]
        json: bool,
    },

    /// Run a design pipeline (chained rule execution with decision tracing)
    Pipeline {
        /// Pipeline name (buck, boost, ldo, led) or --list to show available
        name: Option<String>,

        /// List available pipelines
        #[arg(long)]
        list: bool,

        /// Parameters as key=value pairs (comma-separated)
        #[arg(long)]
        params: Option<String>,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// List or manage design rules (Skills)
    Rules {
        /// Seed default rules into database
        #[arg(long)]
        seed: bool,

        /// Apply a rule with parameters
        #[arg(long)]
        apply: Option<String>,

        /// Parameters as key=value pairs (comma-separated)
        #[arg(long)]
        params: Option<String>,

        /// Candidate component value to check (name=value)
        #[arg(long)]
        candidate: Option<String>,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// List parameter names available in the database
    ListParams {
        /// Filter by category name
        #[arg(long)]
        category: Option<String>,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Start MCP server for AI integration (stdio transport)
    Serve,
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
        Commands::Query { category, param, manufacturer, package, search, in_stock, limit, json } => {
            cmd_query(&db, category.as_deref(), param.as_deref(), manufacturer.as_deref(), package.as_deref(), search.as_deref(), in_stock, limit, json)
        }
        Commands::Show { mpn, json } => cmd_show(&db, &mpn, json),
        Commands::Categories { json } => cmd_categories(&db, json),
        Commands::Check { rule, params, candidate, json } => {
            cmd_check(&db, &rule, &params, candidate.as_deref(), json)
        }
        Commands::Export { mpn, category, format, output } => {
            cmd_export(&db, mpn.as_deref(), category.as_deref(), &format, &output)
        }
        Commands::Fetch { mpn, mfg_id } => {
            cmd_fetch(&db, &mpn, mfg_id.as_deref())
        }
        Commands::Design { template, vin, vout, iout, output } => {
            cmd_design(&db, &template, vin, vout, iout, &output)
        }
        Commands::Compose { file, output } => {
            cmd_compose(&db, &file, &output)
        }
        Commands::IcDesign { template, params, nets, output } => {
            cmd_ic_design(&db, &template, &params, nets.as_deref(), &output)
        }
        Commands::Suggest { vin, vout, iout, isolated, json } => {
            cmd_suggest(vin, vout, iout, isolated, json)
        }
        Commands::HqSearch { keyword, limit, json } => {
            cmd_hqsearch(&keyword, limit, json)
        }
        Commands::Rules { seed, apply, params, candidate, json } => {
            cmd_rules(&db, seed, apply.as_deref(), params.as_deref(), candidate.as_deref(), json)
        }
        Commands::TemplatePins { mpn, json } => {
            cmd_template_pins(&mpn, json)
        }
        Commands::Pipeline { name, list, params, json } => {
            cmd_pipeline(&db, name.as_deref(), list, params.as_deref(), json)
        }
        Commands::ListParams { category, json } => {
            cmd_list_params(&db, category.as_deref(), json)
        }
        Commands::Serve => kicad_cdb::mcp::serve(&db),
    }
}

// ---------------------------------------------------------------------------
// JSON output helpers
// ---------------------------------------------------------------------------

fn print_json(value: &impl Serialize) {
    println!("{}", serde_json::to_string_pretty(value).unwrap_or_else(|e| format!("{{\"error\":\"{}\"}}", e)));
}

// ---------------------------------------------------------------------------
// Command handlers
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// query
// ---------------------------------------------------------------------------

fn cmd_query(
    db: &ComponentDb,
    category: Option<&str>,
    param: Option<&str>,
    manufacturer: Option<&str>,
    package: Option<&str>,
    search: Option<&str>,
    in_stock: bool,
    limit: Option<usize>,
    json: bool,
) -> Result<()> {
    let param_filter = match param {
        Some(p) => {
            let (name, min, max) = parse_param_filter(p)?;
            Some((name, min, max))
        }
        None => None,
    };

    let results = kicad_cdb::service::query_filtered(
        db, search, category, manufacturer, package, param_filter, in_stock, limit,
    )?;

    if json {
        #[derive(Serialize)]
        struct Out { count: usize, components: Vec<kicad_cdb::Component> }
        print_json(&Out { count: results.len(), components: results });
    } else if results.is_empty() {
        println!("No components found");
    } else {
        println!("Found {} components:", results.len());
        for c in &results {
            println!("  {} | {} | {} | {}",
                c.mpn, c.manufacturer,
                c.package.as_deref().unwrap_or("-"),
                c.description.as_deref().unwrap_or("-"));
        }
    }
    Ok(())
}

/// Parse param filter: "name:min:max" or "name>=min" (legacy)
fn parse_param_filter(filter: &str) -> Result<(&str, Option<f64>, Option<f64>)> {
    // Legacy format: name>=value
    if let Some(pos) = filter.find(">=") {
        let name = filter[..pos].trim();
        let val: f64 = filter[pos+2..].parse()?;
        return Ok((name, Some(val), None));
    }
    // Three-part format: name:min:max
    let parts: Vec<&str> = filter.splitn(3, ':').collect();
    if parts.len() >= 2 && !parts[0].is_empty() {
        let name = parts[0].trim();
        let min = if parts.len() > 1 && !parts[1].is_empty() {
            Some(parts[1].parse()?)
        } else { None };
        let max = if parts.len() > 2 && !parts[2].is_empty() {
            Some(parts[2].parse()?)
        } else { None };
        return Ok((name, min, max));
    }
    anyhow::bail!("Parameter filter must be 'name:min:max' or 'name>=value', got: {}", filter);
}

// ---------------------------------------------------------------------------
// show
// ---------------------------------------------------------------------------

fn cmd_show(db: &ComponentDb, mpn: &str, json: bool) -> Result<()> {
    let comp = db.get_component_by_mpn_any(mpn)?
        .ok_or_else(|| anyhow::anyhow!("Component '{}' not found", mpn))?;

    let id = comp.id.unwrap();
    let pins = db.get_pins(id)?;
    let params = db.get_parameters(id)?;
    let models = db.get_simulation_models(id)?;
    let supply = db.get_supply_info(id)?;

    if json {
        #[derive(Serialize)]
        struct Out {
            component: kicad_cdb::Component,
            pins: Vec<kicad_cdb::Pin>,
            parameters: Vec<kicad_cdb::Parameter>,
            models: Vec<kicad_cdb::SimulationModel>,
            supply: Vec<kicad_cdb::SupplyInfo>,
        }
        print_json(&Out { component: comp, pins, parameters: params, models, supply });
    } else {
        println!("MPN:          {}", comp.mpn);
        println!("Manufacturer: {}", comp.manufacturer);
        println!("Package:      {}", comp.package.as_deref().unwrap_or("-"));
        println!("Lifecycle:    {}", comp.lifecycle);
        if let Some(ref desc) = comp.description { println!("Description:  {}", desc); }
        if let Some(ref url) = comp.datasheet_url { println!("Datasheet:    {}", url); }
        if let Some(ref sym) = comp.kicad_symbol { println!("KiCad Symbol: {}", sym); }
        if let Some(ref fp) = comp.kicad_footprint { println!("KiCad Footprint: {}", fp); }

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

        if !models.is_empty() {
            println!("\n--- Simulation Models ({}) ---", models.len());
            for m in &models {
                println!("  {} ({}) - {} chars", m.model_type,
                    m.format.as_deref().unwrap_or("?"), m.model_text.len());
            }
        }

        if !supply.is_empty() {
            println!("\n--- Supply ({}) ---", supply.len());
            for s in &supply {
                println!("  {} | SKU: {} | Stock: {}",
                    s.supplier, s.sku.as_deref().unwrap_or("-"),
                    s.stock.map(|n| n.to_string()).unwrap_or("-".to_string()));
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// categories
// ---------------------------------------------------------------------------

fn cmd_categories(db: &ComponentDb, json: bool) -> Result<()> {
    let cats: Vec<kicad_cdb::Category> = db.conn
        .prepare("SELECT id, name, parent_id, description FROM categories ORDER BY name")?
        .query_map([], |row| Ok(kicad_cdb::Category {
            id: Some(row.get(0)?), name: row.get(1)?, parent_id: row.get(2)?, description: row.get(3)?,
        }))?.filter_map(|r| r.ok()).collect();

    if json {
        #[derive(Serialize)]
        struct Out { categories: Vec<kicad_cdb::Category> }
        print_json(&Out { categories: cats });
    } else {
        for cat in &cats {
            let indent = if cat.parent_id.is_some() { "  " } else { "" };
            println!("{}{} (id={})", indent, cat.name, cat.id.unwrap());
        }
        println!("\nTotal: {} categories", cats.len());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// check (single rule)
// ---------------------------------------------------------------------------

fn cmd_check(db: &ComponentDb, rule_name: &str, params_str: &str, candidate: Option<&str>, json: bool) -> Result<()> {
    let (rule, result) = kicad_cdb::service::apply_rule_with_str_params(db, rule_name, params_str, candidate)?;

    if json {
        #[derive(Serialize)]
        struct Out {
            rule: String,
            description: Option<String>,
            outputs: std::collections::HashMap<String, f64>,
            check_expr: String,
            pass: bool,
        }
        print_json(&Out {
            rule: rule.name.clone(),
            description: rule.description.clone(),
            outputs: result.outputs,
            check_expr: result.check_expression,
            pass: result.pass,
        });
    } else {
        println!("Rule: {}", rule_name);
        if let Some(desc) = &rule.description { println!("  {}", desc); }
        for (name, val) in &result.outputs {
            println!("  {} = {:.6e}", name, val);
        }
        println!("Check: {} => {}", result.check_expression, if result.pass { "PASS" } else { "FAIL" });
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// export
// ---------------------------------------------------------------------------

fn cmd_export(db: &ComponentDb, mpn: Option<&str>, category: Option<&str>, format: &str, output: &str) -> Result<()> {
    let components = match (mpn, category) {
        (Some(mpn), _) => {
            let comp = db.get_component_by_mpn_any(mpn)?;
            vec![comp].into_iter().flatten().collect()
        }
        (None, Some(cat)) => db.query_components_by_category(cat)?,
        _ => anyhow::bail!("Specify --mpn or --category for export"),
    };

    if components.is_empty() {
        anyhow::bail!("No components found to export");
    }

    match format {
        "spec" => cmd_export_spec(db, &components, output),
        "kicad_sym" | _ => cmd_export_kicad_sym(db, &components, output),
    }
}

fn cmd_export_spec(db: &ComponentDb, components: &[kicad_cdb::Component], output: &str) -> Result<()> {
    #[derive(Serialize)]
    struct SpecOutput {
        mpn: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        lib_name: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        reference_prefix: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        datasheet_url: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        footprint: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        manufacturer: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        package: Option<String>,
        pins: Vec<SpecPin>,
    }

    #[derive(Serialize)]
    struct SpecPin {
        number: String,
        name: String,
        #[serde(rename = "type")]
        pin_type: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        group: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        alt_functions: Option<Vec<String>>,
    }

    let mut out = String::new();
    for comp in components {
        let id = comp.id.unwrap();
        let db_pins = db.get_pins(id)?;

        let pins: Vec<SpecPin> = db_pins.into_iter().map(|p| SpecPin {
            number: p.pin_number,
            name: p.pin_name,
            pin_type: p.electrical_type.unwrap_or_else(|| "passive".into()),
            group: p.pin_group,
            alt_functions: p.alt_functions,
        }).collect();

        let spec = SpecOutput {
            mpn: comp.mpn.clone(),
            lib_name: comp.kicad_symbol.as_ref()
                .and_then(|s| s.split(':').next())
                .map(|s| s.to_string()),
            reference_prefix: comp.kicad_symbol.as_ref()
                .and_then(|s| s.split(':').last())
                .map(|s| infer_ref_prefix(s).into()),
            description: comp.description.clone(),
            datasheet_url: comp.datasheet_url.clone(),
            footprint: comp.kicad_footprint.clone(),
            manufacturer: Some(comp.manufacturer.clone()),
            package: comp.package.clone(),
            pins,
        };

        out.push_str(&serde_json::to_string_pretty(&spec)?);
        out.push('\n');
    }

    std::fs::write(output, &out)?;
    println!("Exported {} spec(s) → {}", components.len(), output);
    Ok(())
}

fn infer_ref_prefix(symbol_name: &str) -> String {
    let upper = symbol_name.to_uppercase();
    if upper.starts_with("R") && !upper.starts_with("REG") && !upper.contains("RELAY") { return "R".into(); }
    if upper.starts_with("C") && !upper.starts_with("CONN") && !upper.starts_with("CRYSTAL") { return "C".into(); }
    if upper.starts_with("L") && !upper.starts_with("LED") && !upper.starts_with("LCD") { return "L".into(); }
    if upper.starts_with("LED") { return "D".into(); }
    if upper.starts_with("D") && !upper.starts_with("DIP") { return "D".into(); }
    if upper.starts_with("CONN") || upper.starts_with("J") { return "J".into(); }
    if upper.starts_with("SW") { return "SW".into(); }
    if upper.starts_with("CRYSTAL") || upper.starts_with("XTAL") { return "Y".into(); }
    "U".into()
}

fn cmd_export_kicad_sym(db: &ComponentDb, components: &[kicad_cdb::Component], output: &str) -> Result<()> {
    let mut out = String::new();
    out.push_str("(kicad_symbol_lib\n");
    out.push_str("  (version \"20231120\")\n");
    out.push_str("  (generator \"component-db\")\n");

    for comp in components {
        let id = comp.id.unwrap();
        let pins = db.get_pins(id)?;
        let name = comp.mpn.replace('.', "_");

        out.push_str(&format!("  (symbol \"{}\"\n", name));
        out.push_str("    (in_bom yes)\n");
        out.push_str("    (on_board yes)\n");

        out.push_str("    (property \"Reference\" \"U?\" (at 0 1.27 0)\n");
        out.push_str("      (effects (font (size 1.27 1.27))))\n");
        out.push_str(&format!("    (property \"Value\" \"{}\" (at 0 -1.27 0)\n", comp.mpn));
        out.push_str("      (effects (font (size 1.27 1.27))))\n");
        if let Some(ref fp) = comp.kicad_footprint {
            out.push_str(&format!("    (property \"Footprint\" \"{}\" (at 0 -2.54 0)\n", fp));
            out.push_str("      (effects (font (size 1.27 1.27)) hide))\n");
        }

        let pin_count = pins.len() as f64;
        let left_pins = (pin_count / 2.0).ceil() as i32;
        let body_half_h = left_pins as f64 * 1.27;
        out.push_str(&format!("    (symbol \"{}_0_1\"\n", name));
        out.push_str(&format!("      (rectangle (start -5.08 {}) (end 5.08 {})\n",
            body_half_h + 1.27, -(body_half_h + 1.27)));
        out.push_str("        (stroke (width 0.254) (type default))\n");
        out.push_str("        (fill (type background)))\n");
        out.push_str("    )\n");

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

// ---------------------------------------------------------------------------
// fetch / hq-search
// ---------------------------------------------------------------------------

fn cmd_fetch(db: &ComponentDb, mpn: &str, mfg_id: Option<&str>) -> Result<()> {
    println!("Fetching '{}' from HuaQiu EDA...", mpn);
    let id = kicad_cdb::hqapi::fetch_and_import(db, mpn, mfg_id)?;

    let comp = db.get_component(id)?.unwrap();
    println!("\nImported: {} - {} (id={})", comp.mpn, comp.manufacturer, id);
    if let Some(ref desc) = comp.description { println!("  {}", desc); }
    if let Some(ref pkg) = comp.package { println!("  Package: {}", pkg); }

    let pins = db.get_pins(id)?;
    let params = db.get_parameters(id)?;
    println!("  Pins: {} | Parameters: {}", pins.len(), params.len());
    Ok(())
}

fn cmd_hqsearch(keyword: &str, limit: usize, json: bool) -> Result<()> {
    let client = kicad_cdb::hqapi::HqClient::new()?;
    let results = client.search(keyword, limit)?;

    if json {
        #[derive(Serialize)]
        struct Out<'a> { count: usize, results: &'a [kicad_cdb::hqapi::SearchResult] }
        print_json(&Out { count: results.len(), results: &results });
    } else if results.is_empty() {
        println!("No results for '{}'", keyword);
    } else {
        println!("Found {} results for '{}':\n", results.len(), keyword);
        println!("{:<30} {:<20} {:<10} {}", "MPN", "Manufacturer", "Package", "Description");
        println!("{}", "-".repeat(90));
        for r in &results {
            let desc = r.description.chars().take(40).collect::<String>();
            println!("{:<30} {:<20} {:<10} {}", r.mpn, r.manufacturer, r.package, desc);
        }
        println!("\nUse: cdb --db <db> fetch --mpn <MPN> --mfg-id <ID>  to import");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// design / compose / ic-design
// ---------------------------------------------------------------------------

fn cmd_design(db: &ComponentDb, template: &str, vin: f64, vout: f64, iout: f64, output: &str) -> Result<()> {
    println!("Generating {} schematic: {}V -> {}V, {}A", template, vin, vout, iout);
    let sch_text = kicad_cdb::design::generate_schematic(db, template, vin, vout, iout)?;
    std::fs::write(output, &sch_text)?;
    println!("Written to {}", output);
    Ok(())
}

fn cmd_suggest(vin: f64, vout: f64, iout: f64, isolated: bool, json: bool) -> Result<()> {
    let candidates = kicad_cdb::skills::suggest_topologies(vin, vout, iout, isolated);

    if json {
        #[derive(Serialize)]
        struct Out {
            requirements: serde_json::Value,
            recommendations: Vec<kicad_cdb::skills::TopologyCandidate>,
        }
        print_json(&Out {
            requirements: serde_json::json!({ "vin": vin, "vout": vout, "iout": iout, "isolated": isolated }),
            recommendations: candidates,
        });
    } else {
        println!("Power topology suggestions for {}V -> {}V @ {}A{}\n",
            vin, vout, iout, if isolated { " [isolated]" } else { "" });
        println!("{:<15} {:>8} {:>8}  {}", "Topology", "Eff%", "Score", "Reason");
        println!("{}", "-".repeat(80));
        for c in &candidates {
            println!("{:<15} {:>7.0}% {:>7.2}  {}",
                c.topology, c.estimated_efficiency * 100.0, c.score, c.reason);
        }
        if let Some(best) = candidates.first() {
            println!("\nRecommended: {} (score {:.2})", best.topology, best.score);
        }
    }
    Ok(())
}

fn cmd_compose(db: &ComponentDb, file: &str, output: &str) -> Result<()> {
    let composition = kicad_cdb::composition::load_composition(std::path::Path::new(file))?;
    println!("Composing '{}' — {} modules", composition.name, composition.modules.len());
    for m in &composition.modules {
        println!("  {} [{}] ({})", m.id, m.template, m.template_type);
    }
    let sch_text = kicad_cdb::design::generate_composed_schematic(db, &composition)?;
    std::fs::write(output, &sch_text)?;
    println!("Written to {}", output);
    Ok(())
}

fn cmd_ic_design(
    db: &ComponentDb,
    template: &str,
    params_str: &str,
    nets_str: Option<&str>,
    output: &str,
) -> Result<()> {
    let user_params = kicad_cdb::service::parse_kv_f64(params_str)?;

    let mut net_map = std::collections::HashMap::new();
    if let Some(nets) = nets_str {
        for pair in nets.split(',') {
            let kv: Vec<&str> = pair.splitn(2, '=').collect();
            if kv.len() == 2 {
                net_map.insert(kv[0].trim().to_string(), kv[1].trim().to_string());
            }
        }
    }

    println!("Generating {} IC circuit...", template);
    let sch_text = kicad_cdb::design::generate_ic_schematic(db, template, &user_params, &net_map)?;
    std::fs::write(output, &sch_text)?;
    println!("Written to {}", output);
    Ok(())
}

fn cmd_template_pins(mpn: &str, json: bool) -> Result<()> {
    println!("Fetching pins for '{}' from HuaQiu API...", mpn);
    let pins = kicad_cdb::ic_template::fetch_pins_from_hqapi(mpn)?;

    if pins.is_empty() {
        println!("No pins found. The component may not have a symbol available on HuaQiu.");
        return Ok(());
    }

    if json {
        println!("\"pins\": [");
        for (i, pin) in pins.iter().enumerate() {
            let comma = if i + 1 < pins.len() { "," } else { "" };
            println!("  {{ \"number\": \"{}\", \"name\": \"{}\", \"type\": \"{}\" }}{}",
                pin.number, pin.name, pin.pin_type, comma);
        }
        println!("]");
    } else {
        println!("Found {} pins:", pins.len());
        for pin in &pins {
            println!("  Pin {}: {} ({})", pin.number, pin.name, pin.pin_type);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// pipeline
// ---------------------------------------------------------------------------

fn cmd_pipeline(
    db: &ComponentDb,
    name: Option<&str>,
    list: bool,
    params_str: Option<&str>,
    json: bool,
) -> Result<()> {
    let pipelines = kicad_cdb::pipeline::builtin_pipelines();

    if list || name.is_none() {
        if json {
            #[derive(Serialize)]
            struct PipelineInfo {
                name: String,
                description: String,
                user_inputs: Vec<String>,
                steps: Vec<String>,
            }
            let infos: Vec<PipelineInfo> = pipelines.iter().map(|p| PipelineInfo {
                name: p.name.clone(),
                description: p.description.clone(),
                user_inputs: p.user_inputs.clone(),
                steps: p.steps.iter().map(|s| s.rule_name.clone()).collect(),
            }).collect();
            #[derive(Serialize)]
            struct Out { pipelines: Vec<PipelineInfo> }
            print_json(&Out { pipelines: infos });
        } else {
            println!("Available design pipelines:\n");
            for p in &pipelines {
                println!("  {} — {}", p.name, p.description);
                println!("    Required inputs: {}", p.user_inputs.join(", "));
                println!("    Steps: {}", p.steps.iter().map(|s| s.rule_name.as_str()).collect::<Vec<_>>().join(" → "));
                println!();
            }
        }
        return Ok(());
    }

    let pipeline_name = name.unwrap();
    let pipeline = kicad_cdb::pipeline::get_builtin_pipeline(pipeline_name)
        .ok_or_else(|| anyhow::anyhow!("Unknown pipeline '{}'. Use --list to see available.", pipeline_name))?;

    let user_params = match params_str {
        Some(s) => kicad_cdb::service::parse_kv_f64(s)?,
        None => std::collections::HashMap::new(),
    };

    let log = kicad_cdb::pipeline::run_pipeline(db, &pipeline, &user_params)?;

    if json {
        print_json(&log);
        return Ok(());
    }

    println!("Pipeline: {} ({})", log.pipeline_name, pipeline.description);
    print!("Inputs: ");
    let input_strs: Vec<String> = log.user_inputs.iter()
        .map(|(k, v)| format!("{}={}", k, v)).collect();
    println!("{}", input_strs.join(", "));
    println!();

    for step in &log.steps {
        if step.skipped {
            println!("Step {}: {} — SKIPPED", step.seq, step.rule_name);
            if let Some(reason) = &step.skip_reason { println!("  Reason: {}", reason); }
        } else {
            println!("Step {}: {}", step.seq, step.rule_name);
            if !step.description.is_empty() { println!("  {}", step.description); }
            if !step.formula.is_empty() { println!("  Formula: {}", step.formula); }
            if !step.outputs.is_empty() {
                for (name, val) in &step.outputs { println!("  {} = {:.6e}", name, val); }
            }
            if !step.check_expr.is_empty() {
                println!("  Check: {} => {}", step.check_expr, if step.passed { "PASS" } else { "FAIL" });
            }
        }
        println!();
    }

    println!("Summary: {} passed, {} skipped, {} failed", log.passed, log.skipped, log.failed);
    Ok(())
}

// ---------------------------------------------------------------------------
// rules
// ---------------------------------------------------------------------------

fn cmd_rules(
    db: &ComponentDb,
    seed: bool,
    apply: Option<&str>,
    params: Option<&str>,
    candidate: Option<&str>,
    json: bool,
) -> Result<()> {
    if seed {
        let count = db.seed_default_rules()?;
        if count > 0 {
            println!("Seeded {} new rules", count);
        } else {
            println!("All default rules already exist");
        }
        return Ok(());
    }

    if let Some(rule_name) = apply {
        let params_str = params.unwrap_or("");
        let (rule, result) = kicad_cdb::service::apply_rule_with_str_params(db, rule_name, params_str, candidate)?;

        if json {
            #[derive(Serialize)]
            struct Out {
                rule: String,
                description: Option<String>,
                outputs: std::collections::HashMap<String, f64>,
                check_expr: String,
                pass: bool,
            }
            print_json(&Out {
                rule: rule.name.clone(),
                description: rule.description.clone(),
                outputs: result.outputs,
                check_expr: result.check_expression,
                pass: result.pass,
            });
        } else {
            println!("Rule: {}", rule.name);
            if let Some(desc) = &rule.description { println!("  {}", desc); }
            for (name, val) in &result.outputs { println!("  {} = {:.6e}", name, val); }
            println!("Check: {} => {}", result.check_expression, if result.pass { "PASS" } else { "FAIL" });
        }
        return Ok(());
    }

    // Default: list all rules
    let rules = db.get_all_design_rules()?;
    if json {
        #[derive(Serialize)]
        struct Out { rules: Vec<kicad_cdb::DesignRule> }
        print_json(&Out { rules });
    } else if rules.is_empty() {
        println!("No rules. Use 'cdb rules --seed' to add default rules.");
    } else {
        println!("Design Rules ({}):\n", rules.len());
        for r in &rules {
            println!("  {}", r.name);
            if let Some(desc) = &r.description { println!("    {}", desc); }
            if let Some(formula) = &r.formula_expr { println!("    Formula: {}", formula); }
            if let Some(check) = &r.check_expr { println!("    Check:   {}", check); }
            println!();
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// list-params
// ---------------------------------------------------------------------------

fn cmd_list_params(db: &ComponentDb, category: Option<&str>, json: bool) -> Result<()> {
    let names = db.list_parameter_names(category)?;
    if json {
        #[derive(Serialize)]
        struct Out { parameters: Vec<String> }
        print_json(&Out { parameters: names });
    } else if names.is_empty() {
        println!("No parameters found");
    } else {
        println!("Parameters ({}):", names.len());
        for n in &names { println!("  {}", n); }
    }
    Ok(())
}
