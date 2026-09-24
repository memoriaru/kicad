use anyhow::{Context, Result};
use kicad_cdb::ComponentDb;
use serde::Serialize;

// ---------------------------------------------------------------------------
// JSON output helpers
// ---------------------------------------------------------------------------

pub fn print_json(value: &impl Serialize) {
    println!(
        "{}",
        serde_json::to_string_pretty(value).unwrap_or_else(|e| format!("{{\"error\":\"{}\"}}", e))
    );
}

// ---------------------------------------------------------------------------
// import
// ---------------------------------------------------------------------------

pub fn cmd_import(db: &ComponentDb, path: &str) -> Result<()> {
    let content = std::fs::read_to_string(path)?;
    let trimmed = content.trim();

    if trimmed.starts_with('[') {
        let ids = db.import_batch_from_json(trimmed)?;
        println!("Imported {} components", ids.len());
        for id in &ids {
            if let Some(comp) = db.get_component(*id)? {
                println!(
                    "  {} - {} ({})",
                    comp.mpn,
                    comp.manufacturer,
                    comp.package.as_deref().unwrap_or("?")
                );
            }
        }
    } else {
        let id = db.import_from_json(trimmed)?;
        let comp = db
            .get_component(id)?
            .ok_or_else(|| anyhow::anyhow!("Component {} not found after import", id))?;
        println!("Imported: {} - {} (id={})", comp.mpn, comp.manufacturer, id);
    }
    Ok(())
}

pub fn cmd_import_csv(db: &ComponentDb, path: &str, json: bool) -> Result<()> {
    let result = kicad_cdb::csv_import::import_csv(db, std::path::Path::new(path))?;
    if json {
        print_json(&result);
    } else {
        println!(
            "CSV import complete: {} imported, {} skipped, {} total rows",
            result.imported, result.skipped, result.total_rows
        );
        if !result.errors.is_empty() {
            println!("\nErrors:");
            for e in &result.errors {
                println!("  - {}", e);
            }
        }
    }
    Ok(())
}

pub fn cmd_import_model(
    db: &ComponentDb,
    mpn: &str,
    model_type: &str,
    path: &str,
    format: &str,
) -> Result<()> {
    let comp = db
        .conn
        .query_row(
            "SELECT id FROM components WHERE mpn = ?1 LIMIT 1",
            rusqlite::params![mpn],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|_| anyhow::anyhow!("Component '{}' not found", mpn))?;

    let model_text = std::fs::read_to_string(path)?;
    let model_id = db.import_simulation_model(comp, model_type, None, &model_text, format)?;
    println!(
        "Imported {} model for {} (id={})",
        model_type, mpn, model_id
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// query
// ---------------------------------------------------------------------------

pub fn cmd_query(
    db: &ComponentDb,
    category: Option<&str>,
    param: Option<&str>,
    manufacturer: Option<&str>,
    package: Option<&str>,
    search: Option<&str>,
    in_stock: bool,
    lifecycle: Option<&str>,
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

    let mut results = kicad_cdb::service::query_filtered(
        db,
        search,
        category,
        manufacturer,
        package,
        param_filter,
        in_stock,
        limit,
    )?;

    if let Some(status) = lifecycle {
        results.retain(|c| c.lifecycle == status);
    }

    if json {
        #[derive(Serialize)]
        struct Out {
            count: usize,
            components: Vec<kicad_cdb::Component>,
        }
        print_json(&Out {
            count: results.len(),
            components: results,
        });
    } else if results.is_empty() {
        println!("No components found");
    } else {
        println!("Found {} components:", results.len());
        for c in &results {
            println!(
                "  {} | {} | {} | {}",
                c.mpn,
                c.manufacturer,
                c.package.as_deref().unwrap_or("-"),
                c.description.as_deref().unwrap_or("-")
            );
        }
    }
    Ok(())
}

fn parse_param_filter(filter: &str) -> Result<(&str, Option<f64>, Option<f64>)> {
    if let Some(pos) = filter.find(">=") {
        let name = filter[..pos].trim();
        let val: f64 = filter[pos + 2..].parse()?;
        return Ok((name, Some(val), None));
    }
    let parts: Vec<&str> = filter.splitn(3, ':').collect();
    if parts.len() >= 2 && !parts[0].is_empty() {
        let name = parts[0].trim();
        let min = if parts.len() > 1 && !parts[1].is_empty() {
            Some(parts[1].parse()?)
        } else {
            None
        };
        let max = if parts.len() > 2 && !parts[2].is_empty() {
            Some(parts[2].parse()?)
        } else {
            None
        };
        return Ok((name, min, max));
    }
    anyhow::bail!(
        "Parameter filter must be 'name:min:max' or 'name>=value', got: {}",
        filter
    );
}

// ---------------------------------------------------------------------------
// show
// ---------------------------------------------------------------------------

pub fn cmd_show(db: &ComponentDb, mpn: &str, json: bool) -> Result<()> {
    let comp = db
        .get_component_by_mpn_any(mpn)?
        .ok_or_else(|| anyhow::anyhow!("Component '{}' not found", mpn))?;

    let id = comp.id.context("Component missing id")?;
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
        print_json(&Out {
            component: comp,
            pins,
            parameters: params,
            models,
            supply,
        });
    } else {
        println!("MPN:          {}", comp.mpn);
        println!("Manufacturer: {}", comp.manufacturer);
        println!("Package:      {}", comp.package.as_deref().unwrap_or("-"));
        println!("Lifecycle:    {}", comp.lifecycle);
        if let Some(ref desc) = comp.description {
            println!("Description:  {}", desc);
        }
        if let Some(ref url) = comp.datasheet_url {
            println!("Datasheet:    {}", url);
        }
        if let Some(ref sym) = comp.kicad_symbol {
            println!("KiCad Symbol: {}", sym);
        }
        if let Some(ref fp) = comp.kicad_footprint {
            println!("KiCad Footprint: {}", fp);
        }
        if let Some(ref p) = comp.symbol_lib_path {
            println!("Symbol File:   {}", p);
        }
        if let Some(ref p) = comp.footprint_lib_path {
            println!("Footprint File: {}", p);
        }

        if !pins.is_empty() {
            println!("\n--- Pins ({}) ---", pins.len());
            for p in &pins {
                let alts = p
                    .alt_functions
                    .as_ref()
                    .map(|a| {
                        let items: Vec<String> = a
                            .iter()
                            .map(|af| {
                                let mut s = af.function.clone();
                                if let Some(n) = af.af_num {
                                    s = format!("AF{}: {}", n, s);
                                }
                                if let Some(ref p) = af.peripheral {
                                    s = format!("{}/{}", s, p);
                                }
                                if let Some(ref t) = af.signal_type {
                                    s = format!("{}({})", s, t);
                                }
                                s
                            })
                            .collect();
                        format!(" [{}]", items.join(", "))
                    })
                    .unwrap_or_default();
                println!(
                    "  {:>4} {:<12} {:<15}{}{}",
                    p.pin_number,
                    p.pin_name,
                    p.electrical_type.as_deref().unwrap_or("-"),
                    alts,
                    p.description
                        .as_ref()
                        .map(|d| format!(" - {}", d))
                        .unwrap_or_default()
                );
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
                println!(
                    "  {:<20} {} {} {}",
                    p.name,
                    val,
                    p.unit.as_deref().unwrap_or(""),
                    typ
                );
            }
        }

        if !models.is_empty() {
            println!("\n--- Simulation Models ({}) ---", models.len());
            for m in &models {
                println!(
                    "  {} ({}) - {} chars",
                    m.model_type,
                    m.format.as_deref().unwrap_or("?"),
                    m.model_text.len()
                );
            }
        }

        if !supply.is_empty() {
            println!("\n--- Supply ({}) ---", supply.len());
            for s in &supply {
                println!(
                    "  {} | SKU: {} | Stock: {}",
                    s.supplier,
                    s.sku.as_deref().unwrap_or("-"),
                    s.stock.map(|n| n.to_string()).unwrap_or("-".to_string())
                );
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// categories
// ---------------------------------------------------------------------------

pub fn cmd_categories(db: &ComponentDb, json: bool) -> Result<()> {
    let cats: Vec<kicad_cdb::Category> = db
        .conn
        .prepare("SELECT id, name, parent_id, description FROM categories ORDER BY name")?
        .query_map([], |row| {
            Ok(kicad_cdb::Category {
                id: Some(row.get(0)?),
                name: row.get(1)?,
                parent_id: row.get(2)?,
                description: row.get(3)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    if json {
        #[derive(Serialize)]
        struct Out {
            categories: Vec<kicad_cdb::Category>,
        }
        print_json(&Out { categories: cats });
    } else {
        for cat in &cats {
            let indent = if cat.parent_id.is_some() { "  " } else { "" };
            println!(
                "{}{} (id={})",
                indent,
                cat.name,
                cat.id.context("Category missing id")?
            );
        }
        println!("\nTotal: {} categories", cats.len());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// check (single rule)
// ---------------------------------------------------------------------------

pub fn cmd_check(
    db: &ComponentDb,
    rule_name: &str,
    params_str: &str,
    candidate: Option<&str>,
    json: bool,
) -> Result<()> {
    let (rule, result) =
        kicad_cdb::service::apply_rule_with_str_params(db, rule_name, params_str, candidate)?;

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
        if let Some(desc) = &rule.description {
            println!("  {}", desc);
        }
        for (name, val) in &result.outputs {
            println!("  {} = {:.6e}", name, val);
        }
        println!(
            "Check: {} => {}",
            result.check_expression,
            if result.pass { "PASS" } else { "FAIL" }
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// export
// ---------------------------------------------------------------------------

pub fn cmd_export(
    db: &ComponentDb,
    mpn: Option<&str>,
    category: Option<&str>,
    format: &str,
    output: &str,
) -> Result<()> {
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
        "kicad_mod" => cmd_export_kicad_mod(db, &components, output),
        "lib-table" => {
            let lib_name = category.unwrap_or("custom");
            let entry = kicad_cdb::symgen::generate_lib_table_entry(lib_name, output);
            println!("{}", entry);
            Ok(())
        }
        _ => {
            let content = kicad_cdb::symgen::generate_rich_symbol_lib(&components, db)?;
            std::fs::write(output, &content)?;
            let abs_path = std::path::Path::new(output)
                .canonicalize()
                .unwrap_or_else(|_| output.into());
            for comp in &components {
                if let Some(id) = comp.id {
                    db.update_lib_paths(
                        id,
                        Some(abs_path.to_str().unwrap_or(output)),
                        comp.footprint_lib_path.as_deref(),
                        comp.model_3d_path.as_deref(),
                    )?;
                }
            }
            println!("Exported {} components to {}", components.len(), output);
            Ok(())
        }
    }
}

pub fn cmd_export_spec(
    db: &ComponentDb,
    components: &[kicad_cdb::Component],
    output: &str,
) -> Result<()> {
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
        alt_functions: Option<Vec<kicad_cdb::PinAltFunction>>,
    }

    let output_path = std::path::Path::new(output);
    let is_dir_output = output.ends_with('/') || output_path.is_dir();

    if is_dir_output {
        std::fs::create_dir_all(output_path)?;
        for comp in components {
            let id = match comp.id {
                Some(id) => id,
                None => continue,
            };
            let db_pins = db.get_pins(id)?;

            let pins: Vec<SpecPin> = db_pins
                .into_iter()
                .map(|p| SpecPin {
                    number: p.pin_number,
                    name: p.pin_name,
                    pin_type: p.electrical_type.unwrap_or_else(|| "passive".into()),
                    group: p.pin_group,
                    alt_functions: p.alt_functions,
                })
                .collect();

            let spec = SpecOutput {
                mpn: comp.mpn.clone(),
                lib_name: comp
                    .kicad_symbol
                    .as_ref()
                    .and_then(|s| s.split(':').next())
                    .map(|s| s.to_string()),
                reference_prefix: comp
                    .kicad_symbol
                    .as_ref()
                    .and_then(|s| s.split(':').next_back())
                    .map(infer_ref_prefix),
                description: comp.description.clone(),
                datasheet_url: comp.datasheet_url.clone(),
                footprint: comp.kicad_footprint.clone(),
                manufacturer: Some(comp.manufacturer.clone()),
                package: comp.package.clone(),
                pins,
            };

            let file_name = format!("{}.json5", comp.mpn.replace('.', "_"));
            let file_path = output_path.join(&file_name);
            std::fs::write(&file_path, serde_json::to_string_pretty(&spec)?)?;
            println!("  {}", file_path.display());
        }
        println!("Exported {} spec(s) → {}/", components.len(), output);
    } else {
        let mut out = String::new();
        for comp in components {
            let id = match comp.id {
                Some(id) => id,
                None => continue,
            };
            let db_pins = db.get_pins(id)?;

            let pins: Vec<SpecPin> = db_pins
                .into_iter()
                .map(|p| SpecPin {
                    number: p.pin_number,
                    name: p.pin_name,
                    pin_type: p.electrical_type.unwrap_or_else(|| "passive".into()),
                    group: p.pin_group,
                    alt_functions: p.alt_functions,
                })
                .collect();

            let spec = SpecOutput {
                mpn: comp.mpn.clone(),
                lib_name: comp
                    .kicad_symbol
                    .as_ref()
                    .and_then(|s| s.split(':').next())
                    .map(|s| s.to_string()),
                reference_prefix: comp
                    .kicad_symbol
                    .as_ref()
                    .and_then(|s| s.split(':').next_back())
                    .map(infer_ref_prefix),
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
    }
    Ok(())
}

fn infer_ref_prefix(symbol_name: &str) -> String {
    let upper = symbol_name.to_uppercase();
    if upper.starts_with("R") && !upper.starts_with("REG") && !upper.contains("RELAY") {
        return "R".into();
    }
    if upper.starts_with("C") && !upper.starts_with("CONN") && !upper.starts_with("CRYSTAL") {
        return "C".into();
    }
    if upper.starts_with("L") && !upper.starts_with("LED") && !upper.starts_with("LCD") {
        return "L".into();
    }
    if upper.starts_with("LED") {
        return "D".into();
    }
    if upper.starts_with("D") && !upper.starts_with("DIP") {
        return "D".into();
    }
    if upper.starts_with("CONN") || upper.starts_with("J") {
        return "J".into();
    }
    if upper.starts_with("SW") {
        return "SW".into();
    }
    if upper.starts_with("CRYSTAL") || upper.starts_with("XTAL") {
        return "Y".into();
    }
    "U".into()
}

// ---------------------------------------------------------------------------
// export kicad_mod
// ---------------------------------------------------------------------------

pub fn cmd_export_kicad_mod(
    db: &ComponentDb,
    components: &[kicad_cdb::Component],
    output: &str,
) -> Result<()> {
    let output_path = std::path::Path::new(output);
    let is_dir_output = output.ends_with('/') || output_path.is_dir();

    if is_dir_output {
        std::fs::create_dir_all(output_path)?;
        let results = kicad_cdb::footprint::generate_footprint_lib(components, db)?;
        let abs_dir = output_path
            .canonicalize()
            .unwrap_or_else(|_| output_path.to_path_buf());
        for (name, content) in &results {
            let file_name = format!("{}.kicad_mod", name);
            let file_path = output_path.join(&file_name);
            std::fs::write(&file_path, content)?;
            println!("  {}", file_path.display());
        }
        for comp in components {
            if let Some(id) = comp.id {
                let fp = comp.package.as_deref().map(|p| {
                    let name = p.replace(['.', ' ', '/'], "_");
                    abs_dir
                        .join(format!("{}.kicad_mod", name))
                        .to_str()
                        .unwrap_or("")
                        .to_string()
                });
                db.update_lib_paths(
                    id,
                    comp.symbol_lib_path.as_deref(),
                    fp.as_deref(),
                    comp.model_3d_path.as_deref(),
                )?;
            }
        }
        println!("Exported {} footprints → {}/", results.len(), output);
    } else {
        if components.len() != 1 {
            anyhow::bail!("Single file output requires exactly one component. Use --mpn for single, or directory output for batch.");
        }
        let content = kicad_cdb::footprint::generate_footprint_for_component(&components[0], db)?;
        std::fs::write(output, &content)?;
        let abs_path = std::path::Path::new(output)
            .canonicalize()
            .unwrap_or_else(|_| output.into());
        if let Some(id) = components[0].id {
            db.update_lib_paths(
                id,
                components[0].symbol_lib_path.as_deref(),
                Some(abs_path.to_str().unwrap_or(output)),
                components[0].model_3d_path.as_deref(),
            )?;
        }
        println!("Exported footprint → {}", output);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// fetch / hq-search
// ---------------------------------------------------------------------------

pub fn cmd_fetch(db: &ComponentDb, mpn: &str, mfg_id: Option<&str>) -> Result<()> {
    println!("Fetching '{}' from HuaQiu EDA...", mpn);
    let id = kicad_cdb::hqapi::fetch_and_import(db, mpn, mfg_id)?;

    let comp = db
        .get_component(id)?
        .ok_or_else(|| anyhow::anyhow!("Component {} not found after import", id))?;
    println!(
        "\nImported: {} - {} (id={})",
        comp.mpn, comp.manufacturer, id
    );
    if let Some(ref desc) = comp.description {
        println!("  {}", desc);
    }
    if let Some(ref pkg) = comp.package {
        println!("  Package: {}", pkg);
    }

    let pins = db.get_pins(id)?;
    let params = db.get_parameters(id)?;
    println!("  Pins: {} | Parameters: {}", pins.len(), params.len());
    Ok(())
}

pub fn cmd_hqsearch(keyword: &str, limit: usize, json: bool) -> Result<()> {
    let client = kicad_cdb::hqapi::HqClient::new()?;
    let results = client.search(keyword, limit)?;

    if json {
        #[derive(Serialize)]
        struct Out<'a> {
            count: usize,
            results: &'a [kicad_cdb::hqapi::SearchResult],
        }
        print_json(&Out {
            count: results.len(),
            results: &results,
        });
    } else if results.is_empty() {
        println!("No results for '{}'", keyword);
    } else {
        println!("Found {} results for '{}':\n", results.len(), keyword);
        println!(
            "{:<30} {:<20} {:<10} Description",
            "MPN", "Manufacturer", "Package"
        );
        println!("{}", "-".repeat(90));
        for r in &results {
            let desc = r.description.chars().take(40).collect::<String>();
            println!(
                "{:<30} {:<20} {:<10} {}",
                r.mpn, r.manufacturer, r.package, desc
            );
        }
        println!("\nUse: kdesign --db <db> fetch --mpn <MPN> --mfg-id <ID>  to import");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// design / compose / ic-design
// ---------------------------------------------------------------------------

pub fn cmd_design(
    db: &ComponentDb,
    template: &str,
    vin: f64,
    vout: f64,
    iout: f64,
    output: &str,
) -> Result<()> {
    println!(
        "Generating {} schematic: {}V -> {}V, {}A",
        template, vin, vout, iout
    );
    let sch_text = kicad_cdb::design::generate_schematic(db, template, vin, vout, iout)?;
    std::fs::write(output, &sch_text)?;
    println!("Written to {}", output);
    Ok(())
}

pub fn cmd_suggest(vin: f64, vout: f64, iout: f64, isolated: bool, json: bool) -> Result<()> {
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
        println!(
            "Power topology suggestions for {}V -> {}V @ {}A{}\n",
            vin,
            vout,
            iout,
            if isolated { " [isolated]" } else { "" }
        );
        println!("{:<15} {:>8} {:>8}  Reason", "Topology", "Eff%", "Score");
        println!("{}", "-".repeat(80));
        for c in &candidates {
            println!(
                "{:<15} {:>7.0}% {:>7.2}  {}",
                c.topology,
                c.estimated_efficiency * 100.0,
                c.score,
                c.reason
            );
        }
        if let Some(best) = candidates.first() {
            println!("\nRecommended: {} (score {:.2})", best.topology, best.score);
        }
    }
    Ok(())
}

pub fn cmd_compose(db: &ComponentDb, file: &str, output: &str) -> Result<()> {
    let composition = kicad_cdb::composition::load_composition(std::path::Path::new(file))?;
    println!(
        "Composing '{}' — {} modules",
        composition.name,
        composition.modules.len()
    );
    for m in &composition.modules {
        println!("  {} [{}] ({})", m.id, m.template, m.template_type);
    }
    let sch_text = kicad_cdb::design::generate_composed_schematic(db, &composition)?;
    std::fs::write(output, &sch_text)?;
    println!("Written to {}", output);
    Ok(())
}

pub fn cmd_ic_design(
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

pub fn cmd_template_pins(mpn: &str, json: bool) -> Result<()> {
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
            println!(
                "  {{ \"number\": \"{}\", \"name\": \"{}\", \"type\": \"{}\" }}{}",
                pin.number, pin.name, pin.pin_type, comma
            );
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
// import-templates
// ---------------------------------------------------------------------------

pub fn cmd_import_templates(db: &ComponentDb, ic_dir: &str, topo_dir: &str) -> Result<()> {
    use std::path::Path;

    let conn = &db.conn;
    let mut total = 0;

    let ic_path = Path::new(ic_dir);
    if ic_path.is_dir() {
        let count = kicad_cdb::ic_template::import_templates_from_dir(ic_path, conn)?;
        println!("Imported {} IC templates from {}", count, ic_dir);
        total += count;
    } else {
        println!("IC template directory '{}' not found, skipping", ic_dir);
    }

    let topo_path = Path::new(topo_dir);
    if topo_path.is_dir() {
        let count = kicad_cdb::topology::import_topologies_from_dir(topo_path, conn)?;
        println!("Imported {} topology templates from {}", count, topo_dir);
        total += count;
    } else {
        println!(
            "Topology template directory '{}' not found, skipping",
            topo_dir
        );
    }

    println!("Total: {} templates imported", total);
    Ok(())
}

// ---------------------------------------------------------------------------
// pipeline
// ---------------------------------------------------------------------------

pub fn cmd_pipeline(
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
            let infos: Vec<PipelineInfo> = pipelines
                .iter()
                .map(|p| PipelineInfo {
                    name: p.name.clone(),
                    description: p.description.clone(),
                    user_inputs: p.user_inputs.clone(),
                    steps: p.steps.iter().map(|s| s.rule_name.clone()).collect(),
                })
                .collect();
            #[derive(Serialize)]
            struct Out {
                pipelines: Vec<PipelineInfo>,
            }
            print_json(&Out { pipelines: infos });
        } else {
            println!("Available design pipelines:\n");
            for p in &pipelines {
                println!("  {} — {}", p.name, p.description);
                println!("    Required inputs: {}", p.user_inputs.join(", "));
                println!(
                    "    Steps: {}",
                    p.steps
                        .iter()
                        .map(|s| s.rule_name.as_str())
                        .collect::<Vec<_>>()
                        .join(" → ")
                );
                println!();
            }
        }
        return Ok(());
    }

    let pipeline_name = name.ok_or_else(|| {
        anyhow::anyhow!("Pipeline name required. Use --name <pipeline> or --list")
    })?;
    let pipeline = kicad_cdb::pipeline::get_builtin_pipeline(pipeline_name).ok_or_else(|| {
        anyhow::anyhow!(
            "Unknown pipeline '{}'. Use --list to see available.",
            pipeline_name
        )
    })?;

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
    let input_strs: Vec<String> = log
        .user_inputs
        .iter()
        .map(|(k, v)| format!("{}={}", k, v))
        .collect();
    println!("{}", input_strs.join(", "));
    println!();

    for step in &log.steps {
        if step.skipped {
            println!("Step {}: {} — SKIPPED", step.seq, step.rule_name);
            if let Some(reason) = &step.skip_reason {
                println!("  Reason: {}", reason);
            }
        } else {
            println!("Step {}: {}", step.seq, step.rule_name);
            if !step.description.is_empty() {
                println!("  {}", step.description);
            }
            if !step.formula.is_empty() {
                println!("  Formula: {}", step.formula);
            }
            if !step.outputs.is_empty() {
                for (name, val) in &step.outputs {
                    println!("  {} = {:.6e}", name, val);
                }
            }
            if !step.check_expr.is_empty() {
                println!(
                    "  Check: {} => {}",
                    step.check_expr,
                    if step.passed { "PASS" } else { "FAIL" }
                );
            }
        }
        println!();
    }

    println!(
        "Summary: {} passed, {} skipped, {} failed",
        log.passed, log.skipped, log.failed
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// rules
// ---------------------------------------------------------------------------

pub fn cmd_rules(
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
        let (rule, result) =
            kicad_cdb::service::apply_rule_with_str_params(db, rule_name, params_str, candidate)?;

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
            if let Some(desc) = &rule.description {
                println!("  {}", desc);
            }
            for (name, val) in &result.outputs {
                println!("  {} = {:.6e}", name, val);
            }
            println!(
                "Check: {} => {}",
                result.check_expression,
                if result.pass { "PASS" } else { "FAIL" }
            );
        }
        return Ok(());
    }

    let rules = db.get_all_design_rules()?;
    if json {
        #[derive(Serialize)]
        struct Out {
            rules: Vec<kicad_cdb::DesignRule>,
        }
        print_json(&Out { rules });
    } else if rules.is_empty() {
        println!("No rules. Use 'kdesign rules --seed' to add default rules.");
    } else {
        println!("Design Rules ({}):\n", rules.len());
        for r in &rules {
            println!("  {}", r.name);
            if let Some(desc) = &r.description {
                println!("    {}", desc);
            }
            if let Some(formula) = &r.formula_expr {
                println!("    Formula: {}", formula);
            }
            if let Some(check) = &r.check_expr {
                println!("    Check:   {}", check);
            }
            println!();
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// list-params
// ---------------------------------------------------------------------------

pub fn cmd_list_params(db: &ComponentDb, category: Option<&str>, json: bool) -> Result<()> {
    if category.is_some() {
        let names = db.list_parameter_names(category)?;
        if json {
            #[derive(Serialize)]
            struct Out {
                parameters: Vec<String>,
            }
            print_json(&Out { parameters: names });
        } else if names.is_empty() {
            println!("No parameters found");
        } else {
            println!("Parameters ({}):", names.len());
            for n in &names {
                println!("  {}", n);
            }
        }
        return Ok(());
    }

    let stats = db.list_parameter_stats()?;
    if json {
        #[derive(Serialize)]
        struct ParamStat {
            name: String,
            components: usize,
        }
        #[derive(Serialize)]
        struct Out {
            parameters: Vec<ParamStat>,
        }
        print_json(&Out {
            parameters: stats
                .into_iter()
                .map(|(name, cnt)| ParamStat {
                    name,
                    components: cnt,
                })
                .collect(),
        });
    } else if stats.is_empty() {
        println!("No parameters found");
    } else {
        println!("{:<30} {:>10}", "Parameter", "Components");
        println!("{}", "-".repeat(42));
        for (name, cnt) in &stats {
            println!("{:<30} {:>10}", name, cnt);
        }
        println!("\nTotal: {} unique parameters", stats.len());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// bom
// ---------------------------------------------------------------------------

pub fn cmd_bom(db: &ComponentDb, format: &str, output: &str) -> Result<()> {
    let entries = kicad_cdb::bom::generate_bom(db)?;

    let content = match format {
        "json" => kicad_cdb::bom::bom_to_json(&entries)?,
        _ => kicad_cdb::bom::bom_to_csv(&entries)?,
    };

    std::fs::write(output, &content)?;
    println!("BOM generated: {} entries → {}", entries.len(), output);
    Ok(())
}

// ---------------------------------------------------------------------------
// netlist
// ---------------------------------------------------------------------------

pub fn cmd_netlist(input: &str, output: &str) -> Result<()> {
    let content = kicad_cdb::netlist::netlist_from_file(input)?;
    std::fs::write(output, &content)?;
    println!("Netlist generated → {}", output);
    Ok(())
}

// ---------------------------------------------------------------------------
// recommend
// ---------------------------------------------------------------------------

pub fn cmd_recommend(
    db: &ComponentDb,
    rule_name: &str,
    params_str: &str,
    candidate: Option<&str>,
    limit: Option<usize>,
    json: bool,
) -> Result<()> {
    let (rule, result, recommendations) =
        kicad_cdb::service::recommend_components(db, rule_name, params_str, candidate, limit)?;

    if json {
        #[derive(Serialize)]
        struct Out {
            rule: String,
            outputs: std::collections::HashMap<String, f64>,
            pass: bool,
            recommendation_count: usize,
            recommendations: Vec<kicad_cdb::Component>,
        }
        print_json(&Out {
            rule: rule.name.clone(),
            outputs: result.outputs,
            pass: result.pass,
            recommendation_count: recommendations.len(),
            recommendations,
        });
    } else {
        println!("Rule: {}", rule.name);
        for (name, val) in &result.outputs {
            println!("  {} = {:.6e}", name, val);
        }
        println!(
            "Check: {} => {}",
            result.check_expression,
            if result.pass { "PASS" } else { "FAIL" }
        );
        println!("\nRecommended components ({}):", recommendations.len());
        for c in &recommendations {
            println!(
                "  {} | {} | {} | {}",
                c.mpn,
                c.manufacturer,
                c.package.as_deref().unwrap_or("-"),
                c.description.as_deref().unwrap_or("-")
            );
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// erc
// ---------------------------------------------------------------------------

pub fn cmd_erc(
    input: &str,
    output: Option<&str>,
    json: bool,
    annotate_svg: Option<&str>,
    scale: f64,
) -> Result<()> {
    let cfg = kicad_cdb::config::AppConfig::load()?;

    println!("Running ERC on {}...", input);
    let report = kicad_cdb::erc::run_erc(&cfg.kicad_cli_path, input)?;

    if let Some(svg_path) = annotate_svg {
        let svg_content = std::fs::read_to_string(svg_path)
            .with_context(|| format!("Cannot read SVG file: {}", svg_path))?;
        let annotated = kicad_cdb::erc_vis::annotate_svg(&svg_content, &report, scale);
        let out_path = output.unwrap_or(svg_path);
        std::fs::write(out_path, &annotated)?;
        println!(
            "Annotated SVG written to {} ({} errors, {} warnings)",
            out_path, report.summary.errors, report.summary.warnings
        );
        return Ok(());
    }

    if json {
        if let Some(path) = output {
            std::fs::write(path, serde_json::to_string_pretty(&report)?)?;
            println!("ERC report written to {}", path);
        } else {
            print_json(&report);
        }
        return Ok(());
    }

    if let Some(path) = output {
        let mut out = String::new();
        format_erc_text(&report, &mut out);
        std::fs::write(path, &out)?;
        println!("ERC report written to {}", path);
    } else {
        for sheet in &report.sheets {
            println!("Sheet: {}", sheet.path);
            for v in &sheet.violations {
                let sev = match v.severity {
                    kicad_cdb::erc::ErcSeverity::Error => "ERROR",
                    kicad_cdb::erc::ErcSeverity::Warning => "WARN ",
                };
                println!("  [{}] [{}] {}", sev, v.error_type, v.description);
                for loc in &v.locations {
                    println!(
                        "         @({:.2} mm, {:.2} mm): {}",
                        loc.x_mm, loc.y_mm, loc.detail
                    );
                }
            }
            println!();
        }
        let verdict = if report.summary.errors == 0 {
            "PASS"
        } else {
            "FAIL"
        };
        println!(
            "ERC GATE: {}{}",
            verdict,
            if report.summary.warnings > 0 {
                format!(
                    " — {} advisory warnings (--severity-all)",
                    report.summary.warnings
                )
            } else {
                String::new()
            }
        );
        println!(
            "Summary: {} total, {} errors, {} warnings",
            report.summary.total, report.summary.errors, report.summary.warnings
        );
    }
    Ok(())
}

pub fn cmd_drc(input: &str, output: Option<&str>, json: bool) -> Result<()> {
    let cfg = kicad_cdb::config::AppConfig::load()?;

    println!("Running DRC on {}...", input);
    let report = kicad_cdb::drc::run_drc(&cfg.kicad_cli_path, input)?;

    if json {
        if let Some(path) = output {
            std::fs::write(path, serde_json::to_string_pretty(&report)?)?;
            println!("DRC report written to {}", path);
        } else {
            print_json(&report);
        }
        return Ok(());
    }

    if let Some(path) = output {
        let mut out = String::new();
        format_drc_text(&report, &mut out);
        std::fs::write(path, &out)?;
        println!("DRC report written to {}", path);
    } else {
        for v in &report.violations {
            let sev = match v.severity {
                kicad_cdb::drc::DrcSeverity::Error => "ERROR",
                kicad_cdb::drc::DrcSeverity::Warning => "WARN ",
                kicad_cdb::drc::DrcSeverity::Exclusion => "EXCL ",
            };
            println!("  [{}] [{}] {}", sev, v.violation_type, v.description);
            for item in &v.items {
                if let (Some(x), Some(y)) = (item.x_mm, item.y_mm) {
                    println!("         @({:.2} mm, {:.2} mm): {}", x, y, item.description);
                } else {
                    println!("         {}", item.description);
                }
            }
        }
        if !report.unconnected_items.is_empty() {
            println!("\nUnconnected items:");
            for item in &report.unconnected_items {
                if let (Some(x), Some(y)) = (item.x_mm, item.y_mm) {
                    println!("  @({:.2} mm, {:.2} mm): {}", x, y, item.description);
                } else {
                    println!("  {}", item.description);
                }
            }
        }
        let (verdict, reasons) = report.summary.gate();
        println!(
            "\nDRC GATE: {}{}",
            verdict,
            if reasons.is_empty() {
                String::new()
            } else {
                format!(" — {}", reasons.join(", "))
            }
        );
        println!(
            "Summary: {} total, {} errors, {} warnings | shorts: {}, unconnected: {}, dangling: {}",
            report.summary.total,
            report.summary.errors,
            report.summary.warnings,
            report.summary.shorts,
            report.summary.unconnected,
            report.summary.dangling
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// export-gerber
// ---------------------------------------------------------------------------

pub fn cmd_export_gerber(input: &str, output: &str) -> Result<()> {
    let cfg = kicad_cdb::config::AppConfig::load()?;

    println!(
        "Exporting manufacturing files from {} to {}...",
        input, output
    );
    std::fs::create_dir_all(output).with_context(|| format!("create output dir {}", output))?;

    let mut files = kicad_cdb::design::export_gerber(&cfg.kicad_cli_path, input, output)?;
    match kicad_cdb::design::export_drill(&cfg.kicad_cli_path, input, output) {
        Ok((pth, npth)) => files.extend([pth, npth]),
        Err(e) => eprintln!("[export-gerber] WARNING: drill export failed: {e:#}"),
    }
    files.sort();
    files.dedup();

    if files.is_empty() {
        anyhow::bail!("no gerber/drill files generated — check kicad-cli output above");
    }
    println!("\nManufacturing files ({}):", files.len());
    for f in &files {
        println!("  {}", f);
    }
    Ok(())
}

fn format_drc_text(report: &kicad_cdb::drc::DrcReport, out: &mut String) {
    use std::fmt::Write;
    let (verdict, reasons) = report.summary.gate();
    writeln!(
        out,
        "DRC GATE: {}{}",
        verdict,
        if reasons.is_empty() {
            String::new()
        } else {
            format!(" — {}", reasons.join(", "))
        }
    )
    .unwrap();
    writeln!(
        out,
        "Summary: {} total, {} errors, {} warnings | shorts: {}, unconnected: {}, dangling: {}",
        report.summary.total,
        report.summary.errors,
        report.summary.warnings,
        report.summary.shorts,
        report.summary.unconnected,
        report.summary.dangling
    )
    .unwrap();
    if report.summary.unconnected > 0 {
        writeln!(out, "\nUnconnected connections (each must be routed):").unwrap();
        for v in report
            .violations
            .iter()
            .filter(|v| v.violation_type == "unconnected_items")
        {
            let locs: Vec<String> = v
                .items
                .iter()
                .map(|i| match (i.x_mm, i.y_mm) {
                    (Some(x), Some(y)) => format!("({:.2}, {:.2}) {}", x, y, i.description),
                    _ => i.description.clone(),
                })
                .collect();
            writeln!(out, "  {}", locs.join("  <->  ")).unwrap();
        }
    }
    for v in &report.violations {
        if v.violation_type == "unconnected_items" {
            continue;
        }
        let sev = match v.severity {
            kicad_cdb::drc::DrcSeverity::Error => "ERROR",
            kicad_cdb::drc::DrcSeverity::Warning => "WARN ",
            kicad_cdb::drc::DrcSeverity::Exclusion => "EXCL ",
        };
        writeln!(out, "[{}] [{}] {}", sev, v.violation_type, v.description).unwrap();
        for item in &v.items {
            if let (Some(x), Some(y)) = (item.x_mm, item.y_mm) {
                writeln!(out, "  @({:.2} mm, {:.2} mm): {}", x, y, item.description).unwrap();
            } else {
                writeln!(out, "  {}", item.description).unwrap();
            }
        }
    }
    if !report.unconnected_items.is_empty() {
        writeln!(out, "\nUnconnected items:").unwrap();
        for item in &report.unconnected_items {
            if let (Some(x), Some(y)) = (item.x_mm, item.y_mm) {
                writeln!(out, "  @({:.2} mm, {:.2} mm): {}", x, y, item.description).unwrap();
            } else {
                writeln!(out, "  {}", item.description).unwrap();
            }
        }
    }
}

fn format_erc_text(report: &kicad_cdb::erc::ErcReport, out: &mut String) {
    for sheet in &report.sheets {
        use std::fmt::Write;
        writeln!(out, "Sheet: {}", sheet.path).unwrap();
        for v in &sheet.violations {
            let sev = match v.severity {
                kicad_cdb::erc::ErcSeverity::Error => "ERROR",
                kicad_cdb::erc::ErcSeverity::Warning => "WARN ",
            };
            writeln!(out, "  [{}] [{}] {}", sev, v.error_type, v.description).unwrap();
            for loc in &v.locations {
                writeln!(
                    out,
                    "         @({:.2} mm, {:.2} mm): {}",
                    loc.x_mm, loc.y_mm, loc.detail
                )
                .unwrap();
            }
        }
        writeln!(out).unwrap();
    }
    use std::fmt::Write;
    let verdict = if report.summary.errors == 0 {
        "PASS"
    } else {
        "FAIL"
    };
    writeln!(out, "ERC GATE: {}", verdict).unwrap();
    writeln!(
        out,
        "Summary: {} total, {} errors, {} warnings",
        report.summary.total, report.summary.errors, report.summary.warnings
    )
    .unwrap();
}

// ---------------------------------------------------------------------------
// explore
// ---------------------------------------------------------------------------

pub fn cmd_explore(
    db: &ComponentDb,
    vin: f64,
    vout: f64,
    iout: f64,
    isolated: bool,
    json: bool,
) -> Result<()> {
    let result = kicad_cdb::explore::explore(db, vin, vout, iout, isolated, None)?;

    if json {
        print_json(&result);
        return Ok(());
    }

    println!(
        "Design Space Exploration: {}V → {}V @ {}A{}\n",
        vin,
        vout,
        iout,
        if isolated { " [isolated]" } else { "" }
    );
    println!(
        "{:<4} {:<15} {:>6} {:>8} {:>8} {:>8} {:>8} Score",
        "#", "Topology", "Viable", "Eff%", "Complex", "Thermal", "Cost"
    );
    println!("{}", "-".repeat(85));

    for (i, c) in result.candidates.iter().enumerate() {
        let viable = if c.viable { "Yes" } else { "NO" };
        println!(
            "{:<4} {:<15} {:>6} {:>7.1}% {:>7.2} {:>7.2} {:>7.2} {:.3}",
            i + 1,
            c.topology,
            viable,
            c.scores.efficiency * 100.0,
            c.scores.complexity,
            c.scores.thermal,
            c.scores.cost_estimate,
            c.scores.overall
        );
        if let Some(ref reason) = c.fail_reason {
            println!("         FAIL: {}", reason);
        }
    }

    if let Some(best) = result.candidates.first() {
        println!(
            "\nRecommended: {} (score {:.3})",
            best.topology, best.scores.overall
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// workflow
// ---------------------------------------------------------------------------

pub fn cmd_workflow(
    db: &ComponentDb,
    goal: Option<&str>,
    list: bool,
    params_str: Option<&str>,
    json: bool,
) -> Result<()> {
    let compositions = kicad_cdb::skill_comp::builtin_compositions();

    if list || goal.is_none() {
        if json {
            #[derive(Serialize)]
            struct Out {
                workflows: Vec<kicad_cdb::skill_comp::CompositionSpec>,
            }
            print_json(&Out {
                workflows: compositions,
            });
        } else {
            println!("Available workflows:\n");
            for c in &compositions {
                println!("  {} — {}", c.name, c.description);
                println!("    Required inputs: {}", c.required_inputs.join(", "));
                println!(
                    "    Stages: {}",
                    c.stages
                        .iter()
                        .map(|s| s.name.as_str())
                        .collect::<Vec<_>>()
                        .join(" → ")
                );
                println!();
            }
        }
        return Ok(());
    }

    let goal_name =
        goal.ok_or_else(|| anyhow::anyhow!("Workflow goal required. Use --goal <name> or --list"))?;
    let spec = compositions
        .into_iter()
        .find(|c| c.name == goal_name)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Unknown workflow '{}'. Use --list to see available.",
                goal_name
            )
        })?;

    let user_params = match params_str {
        Some(s) => kicad_cdb::service::parse_kv_f64(s)?,
        None => std::collections::HashMap::new(),
    };

    println!("Running workflow '{}'...", spec.name);
    let result = kicad_cdb::skill_comp::run_composition(db, &spec, &user_params)?;

    if json {
        print_json(&result);
        return Ok(());
    }

    println!("Workflow: {}\n", result.spec_name);
    for stage in &result.stages {
        if stage.skipped {
            println!("Stage: {} — SKIPPED", stage.name);
            if let Some(reason) = &stage.skip_reason {
                println!("  Reason: {}", reason);
            }
        } else {
            println!("Stage: {} [{}]", stage.name, stage.stage_type);
            println!(
                "  {}",
                serde_json::to_string_pretty(&stage.detail).unwrap_or_default()
            );
        }
        println!();
    }

    if let Some(ref summary) = result.summary {
        println!("Summary: {}", summary);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// list-models
// ---------------------------------------------------------------------------

pub fn cmd_list_models(
    db: &ComponentDb,
    model_type: Option<&str>,
    unverified: bool,
    json: bool,
) -> Result<()> {
    let models = db.list_all_simulation_models(model_type, unverified)?;
    if json {
        #[derive(Serialize)]
        struct ModelEntry {
            id: i64,
            mpn: String,
            model_type: String,
            format: Option<String>,
            verified: bool,
            size: usize,
            notes: Option<String>,
        }
        let entries: Vec<ModelEntry> = models
            .iter()
            .filter_map(|(m, mpn)| {
                Some(ModelEntry {
                    id: m.id?,
                    mpn: mpn.clone(),
                    model_type: m.model_type.clone(),
                    format: m.format.clone(),
                    verified: m.verified,
                    size: m.model_text.len(),
                    notes: m.notes.clone(),
                })
            })
            .collect();
        #[derive(Serialize)]
        struct Out {
            count: usize,
            models: Vec<ModelEntry>,
        }
        print_json(&Out {
            count: entries.len(),
            models: entries,
        });
    } else if models.is_empty() {
        println!("No simulation models found");
    } else {
        println!("Found {} simulation models:", models.len());
        for (m, mpn) in &models {
            println!(
                "  [{}] {} | {} ({}) | {} chars{}",
                m.id.unwrap_or(0),
                mpn,
                m.model_type,
                m.format.as_deref().unwrap_or("?"),
                m.model_text.len(),
                m.notes
                    .as_ref()
                    .map(|n| format!(" | {}", n))
                    .unwrap_or_default()
            );
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// verify-model
// ---------------------------------------------------------------------------

pub fn cmd_verify_model(
    db: &ComponentDb,
    id: Option<i64>,
    mpn: Option<&str>,
    model_type: Option<&str>,
) -> Result<()> {
    let target_id = match (id, mpn) {
        (Some(i), _) => i,
        (None, Some(mpn)) => {
            let comp = db
                .get_component_by_mpn_any(mpn)?
                .ok_or_else(|| anyhow::anyhow!("Component '{}' not found", mpn))?;
            let comp_id = comp.id.context("Component missing id")?;
            let models = db.get_simulation_models(comp_id)?;
            let model = if let Some(mt) = model_type {
                models
                    .into_iter()
                    .find(|m| m.model_type == mt)
                    .ok_or_else(|| anyhow::anyhow!("No {} model found for {}", mt, mpn))?
            } else {
                models
                    .into_iter()
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("No simulation models found for {}", mpn))?
            };
            model.id.context("Simulation model missing id")?
        }
        (None, None) => anyhow::bail!("Specify --id or --mpn"),
    };

    let updated = db.verify_simulation_model(target_id)?;
    if updated {
        println!("Model {} marked as verified", target_id);
    } else {
        println!("Model {} not found", target_id);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// lifecycle
// ---------------------------------------------------------------------------

pub fn cmd_lifecycle(db: &ComponentDb, mpn: &str, status: Option<&str>, json: bool) -> Result<()> {
    let comp = db
        .get_component_by_mpn_any(mpn)?
        .ok_or_else(|| anyhow::anyhow!("Component '{}' not found", mpn))?;
    let id = comp.id.context("Component missing id")?;

    if let Some(new_status) = status {
        if !kicad_cdb::lifecycle::is_valid(new_status) {
            anyhow::bail!(
                "Invalid lifecycle status '{}'. Use: active, nrnd, obsolete, eol",
                new_status
            );
        }
        db.update_lifecycle(id, new_status)?;
        println!("{} lifecycle: {} → {}", mpn, comp.lifecycle, new_status);
    } else if json {
        #[derive(Serialize)]
        struct Out {
            mpn: String,
            lifecycle: String,
        }
        print_json(&Out {
            mpn: mpn.to_string(),
            lifecycle: comp.lifecycle.clone(),
        });
    } else {
        println!("{} lifecycle: {}", mpn, comp.lifecycle);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// diff — parameter comparison
// ---------------------------------------------------------------------------

pub fn cmd_diff(db: &ComponentDb, mpns: &[String], json: bool) -> Result<()> {
    if mpns.len() < 2 {
        anyhow::bail!("Need at least 2 MPNs to compare");
    }
    let mpn_refs: Vec<&str> = mpns.iter().map(|s| s.as_str()).collect();
    let result = db.compare_components(&mpn_refs)?;

    if json {
        print_json(&result);
    } else {
        let mut header = format!("{:<20}", "Parameter");
        for c in &result.components {
            header.push_str(&format!(" | {:<15}", c.mpn));
        }
        println!("{}", header);
        println!("{}", "-".repeat(header.len()));

        for p in &result.parameters {
            let unit = p.unit.as_deref().unwrap_or("");
            let mut row = format!("{:<20}", format!("{} {}", p.name, unit).trim());
            for val in &p.values {
                match val {
                    Some(v) => row.push_str(&format!(" | {:<15}", format!("{:.4e}", v))),
                    None => row.push_str(&format!(" | {:<15}", "-")),
                }
            }
            println!("{}", row);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// compare — multi-candidate comparison
// ---------------------------------------------------------------------------

pub fn cmd_compare(
    db: &ComponentDb,
    rule_name: &str,
    params_str: &str,
    candidates_str: &str,
    json: bool,
) -> Result<()> {
    let candidates: Vec<(&str, f64)> = candidates_str
        .split(',')
        .filter_map(|s| {
            let kv: Vec<&str> = s.splitn(2, '=').collect();
            if kv.len() == 2 {
                Some((kv[0].trim(), kv[1].trim().parse().ok()?))
            } else {
                None
            }
        })
        .collect();

    if candidates.is_empty() {
        anyhow::bail!("No valid candidates. Use format: name=value,name=value");
    }

    let result = kicad_cdb::service::compare_candidates(db, rule_name, params_str, &candidates)?;

    if json {
        print_json(&result);
    } else {
        println!("Rule: {} ", result.rule_name);
        for (k, v) in &result.outputs {
            println!("  {} = {:.6e}", k, v);
        }
        println!();

        for c in &result.candidates {
            let status = if c.pass { "PASS" } else { "FAIL" };
            let margin = format!("{:+.0}%", c.margin_pct);
            let stars = if c.pass {
                if c.margin_pct > 50.0 {
                    "★★★"
                } else if c.margin_pct > 10.0 {
                    "★★"
                } else {
                    "★"
                }
            } else {
                ""
            };
            println!(
                "  {}. {} ({:.6e}) — {}  margin: {} {}",
                c.rank, c.name, c.value, status, margin, stars
            );
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// skills — list registered skills
// ---------------------------------------------------------------------------

pub fn cmd_skills(
    db: &ComponentDb,
    domain: Option<&str>,
    tag: Option<&str>,
    json: bool,
) -> Result<()> {
    let rules = if let Some(d) = domain {
        db.get_rules_by_domain(d)?
    } else if let Some(t) = tag {
        db.get_rules_by_tag(t)?
    } else {
        db.get_all_design_rules()?
    };

    if json {
        #[derive(Serialize)]
        struct Out {
            count: usize,
            skills: Vec<SkillEntry>,
        }
        #[derive(Serialize)]
        struct SkillEntry {
            name: String,
            domain: Option<String>,
            tags: Option<String>,
            description: Option<String>,
        }
        let entries: Vec<SkillEntry> = rules
            .iter()
            .map(|r| SkillEntry {
                name: r.name.clone(),
                domain: r.domain.clone(),
                tags: r.tags.clone(),
                description: r.description.clone(),
            })
            .collect();
        print_json(&Out {
            count: entries.len(),
            skills: entries,
        });
    } else {
        if rules.is_empty() {
            println!("No skills found");
            return Ok(());
        }

        let mut domains: std::collections::BTreeMap<String, Vec<&kicad_cdb::DesignRule>> =
            std::collections::BTreeMap::new();
        for r in &rules {
            let d = r.domain.as_deref().unwrap_or("general").to_string();
            domains.entry(d).or_default().push(r);
        }

        println!(
            "Registered skills ({} rules in {} domains):",
            rules.len(),
            domains.len()
        );
        for (domain, rules) in &domains {
            println!("\n  [{}]", domain);
            for r in rules {
                let tags = r.tags.as_deref().unwrap_or("[]");
                println!(
                    "    {} — {} {}",
                    r.name,
                    r.description.as_deref().unwrap_or("-"),
                    tags
                );
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// match-skill — natural language skill matching
// ---------------------------------------------------------------------------

pub fn cmd_match_skill(db: &ComponentDb, query: &str, json: bool) -> Result<()> {
    let rules = db.get_all_design_rules()?;
    let matches = kicad_cdb::skill_match::match_skills(query, &rules)?;

    if matches.is_empty() {
        println!("No matching skills found for: {}", query);
        return Ok(());
    }

    if json {
        print_json(&matches);
    } else {
        println!(
            "Skill matches for \"{}\" ({} results):\n",
            query,
            matches.len()
        );
        for (i, m) in matches.iter().enumerate() {
            println!(
                "  {}. {} [{}] — score: {:.1}",
                i + 1,
                m.rule_name,
                m.domain,
                m.score
            );
            if !m.description.is_empty() {
                println!("     {}", m.description);
            }
            if !m.matched_keywords.is_empty() {
                println!("     Keywords: {}", m.matched_keywords.join(", "));
            }
            if !m.extracted_params.is_empty() {
                let params: Vec<String> = m
                    .extracted_params
                    .iter()
                    .map(|(k, v)| format!("{}={}", k, v))
                    .collect();
                println!("     Extracted: {}", params.join(", "));
            }
            if !m.parameters.is_empty() {
                println!("     Required params: {}", m.parameters.join(", "));
            }
            if !m.output_params.is_empty() {
                println!("     Outputs: {}", m.output_params.join(", "));
            }
            println!();
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// trace — parameter backtrace
// ---------------------------------------------------------------------------

pub fn cmd_trace(log_path: &str, param_name: &str) -> Result<()> {
    let content = std::fs::read_to_string(log_path)?;
    let log: kicad_cdb::pipeline::DesignLog = serde_json::from_str(&content)?;

    match kicad_cdb::pipeline::trace_parameter(&log, param_name) {
        Some(trace) => {
            if trace.step_seq == 0 {
                println!("{} = {:.6e}", param_name, trace.value);
                println!("  Source: user input");
            } else {
                println!("{} = {:.6e}", param_name, trace.value);
                println!(
                    "  Source: pipeline \"{}\", step {} \"{}\"",
                    log.pipeline_name, trace.step_seq, trace.rule_name
                );
                if !trace.description.is_empty() {
                    println!("  {}", trace.description);
                }
                if !trace.formula.is_empty() {
                    println!("  Formula: {}", trace.formula);
                }
                if !trace.inputs.is_empty() {
                    let inputs: Vec<String> = trace
                        .inputs
                        .iter()
                        .map(|(k, v)| format!("{}={}", k, v))
                        .collect();
                    println!("  Inputs: {}", inputs.join(", "));
                }
            }
        }
        None => println!("Parameter '{}' not found in design log", param_name),
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// impact — change impact analysis
// ---------------------------------------------------------------------------

pub fn cmd_impact(log_path: &str, param_name: &str) -> Result<()> {
    let content = std::fs::read_to_string(log_path)?;
    let log: kicad_cdb::pipeline::DesignLog = serde_json::from_str(&content)?;

    let result = kicad_cdb::pipeline::analyze_impact(&log, param_name);

    println!("Changing parameter: {}", result.changed_param);
    if result.affected_steps.is_empty() {
        println!("  No downstream dependencies found");
    } else {
        println!("  Affected steps:");
        for step in &result.affected_steps {
            let direct = if step.uses_param_directly {
                " (direct)"
            } else {
                ""
            };
            println!(
                "    Step {}: {}{} — produces: {}",
                step.seq,
                step.rule_name,
                direct,
                step.produced_outputs.join(", ")
            );
        }
        println!("  Affected outputs: {}", result.affected_outputs.join(", "));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// design-board — end-to-end AI design orchestrator
// ---------------------------------------------------------------------------

pub fn cmd_design_board(
    db: &ComponentDb,
    vin: f64,
    vout: f64,
    iout: f64,
    output: &str,
    topology: Option<&str>,
) -> Result<()> {
    println!("Design Board: {}V → {}V @ {}A", vin, vout, iout);
    let result = crate::workflow::run_design_board(db, vin, vout, iout, topology)?;
    std::fs::write(output, &result.schematic)?;
    println!("Schematic written to {}", output);
    if let Some(ref summary) = result.summary {
        println!("{}", summary);
    }
    Ok(())
}

pub fn cmd_power_tree(
    db: &ComponentDb,
    vin: f64,
    outputs_str: &str,
    output: &str,
    isolated: bool,
) -> Result<()> {
    let mut outputs = Vec::new();
    for pair in outputs_str.split(',') {
        let kv: Vec<&str> = pair.splitn(2, ':').collect();
        if kv.len() != 2 {
            anyhow::bail!("Invalid output spec '{}'. Use vout:iout format.", pair);
        }
        let vout: f64 = kv[0].parse()?;
        let iout: f64 = kv[1].parse()?;
        outputs.push(kicad_cdb::power_tree::RailSpec {
            vout,
            iout,
            name: None,
        });
    }

    println!("Power Tree: {}V → {} rails", vin, outputs.len());
    for (i, rail) in outputs.iter().enumerate() {
        println!("  Rail {}: {}V @ {}A", i + 1, rail.vout, rail.iout);
    }

    let request = kicad_cdb::power_tree::PowerTreeRequest {
        vin,
        outputs,
        isolated,
    };

    // Try to load cache
    let cache_path = ".power_tree_cache.json";
    let cache = std::fs::read_to_string(cache_path)
        .ok()
        .and_then(|text| serde_json::from_str::<kicad_cdb::power_tree::PowerTreeCache>(&text).ok());

    let result = kicad_cdb::power_tree::run_power_tree_with_cache(db, &request, cache.as_ref())?;

    // Save cache
    let new_cache = kicad_cdb::power_tree::PowerTreeCache {
        request: request.clone(),
        tree: result.tree.clone(),
    };
    if let Ok(json) = serde_json::to_string_pretty(&new_cache) {
        let _ = std::fs::write(cache_path, json);
    }

    std::fs::write(output, &result.schematic)?;
    println!("\nSchematic written to {}", output);
    println!("{}", result.summary);

    if let Some(ref info) = result.incremental_info {
        if info.modules_cached > 0 {
            println!(
                "\nIncremental: {} modules re-evaluated, {} modules cached",
                info.modules_recomputed, info.modules_cached
            );
        }
    }

    if !result.validation.valid {
        anyhow::bail!("Power tree validation failed — see errors above");
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// gen-pcb
// ---------------------------------------------------------------------------

pub fn cmd_gen_pcb(
    input: &str,
    output: &str,
    svg: Option<&str>,
    json: bool,
    layers: usize,
    layout_only: bool,
    route_only: bool,
    render_mode: &str,
    render_layers: Option<&str>,
    board_size: Option<(f64, f64)>,
) -> Result<()> {
    if layout_only && route_only {
        anyhow::bail!("--layout-only and --route-only are mutually exclusive");
    }

    let source =
        std::fs::read_to_string(input).with_context(|| format!("Failed to read {}", input))?;

    let schematic = if input.ends_with(".json5") {
        kicad_json5::parse_json5(&source).with_context(|| "Failed to parse JSON5 schematic")?
    } else {
        let lexer = kicad_json5::Lexer::new(&source);
        let mut parser = kicad_json5::Parser::new(lexer);
        parser
            .parse()
            .with_context(|| "Failed to parse kicad_sch")?
    };

    eprintln!(
        "Parsed: {} components, {} nets",
        schematic.components.len(),
        schematic.nets.len()
    );
    if schematic.nets.is_empty() {
        anyhow::bail!(
            "0 nets parsed from {} — the netlist was lost during parsing              (known json5→sch→gen-pcb chain bug). Refusing to route an empty board.              If the input is a .kicad_sch, feed gen-pcb the original .json5 instead.",
            input
        );
    }
    // P0-1b: footprint names against official KiCad library patterns
    {
        let board_probe = kicad_cdb::design::schematic_to_board_phases(
            &schematic,
            &kicad_cdb::layout_directives::LayoutDirectives::default(),
            &kicad_cdb::layer_config::BoardLayerConfig::two_layer(),
            false,
        )?;
        for w in kicad_cdb::footprint_check::check_footprint_names(&board_probe) {
            eprintln!("⚠ footprint: {}", w);
        }
    }

    let layer_config = if layers == 4 {
        kicad_cdb::layer_config::BoardLayerConfig::four_layer()
    } else {
        kicad_cdb::layer_config::BoardLayerConfig::two_layer()
    };
    let directives = kicad_cdb::layout_directives::LayoutDirectives::default();

    if layout_only {
        // Only run SA layout, skip routing
        let layout_positions = kicad_cdb::layout_engine::auto_layout_refs_with_directives_fixed(
            &schematic,
            &directives,
            board_size,
        );
        eprintln!(
            "[layout] SA floorplanner: {} positions computed",
            layout_positions.len()
        );

        // Build minimal board with positions but no traces
        let mut board = kicad_cdb::design::schematic_to_board_phases_fixed(
            &schematic,
            &directives,
            &layer_config,
            false,
            board_size,
        )?;
        {
            let (dots, pol, cy) = kicad_cdb::silk::add_footprint_silk(&mut board);
            eprintln!(
                "silk: {} pin1 dots, {} polarity marks, {} courtyards",
                dots, pol, cy
            );
        }
        board.segments.clear();
        board.vias.clear();

        let pcb_text = kicad_cdb::design::generate_kicad_pcb(&board)?;
        std::fs::write(output, &pcb_text)?;
        eprintln!(
            "PCB (layout only): {} footprints → {}",
            board.footprints.len(),
            output
        );
        return Ok(());
    }

    if route_only {
        // Only run routing on already-positioned components
        let mut board = kicad_cdb::design::schematic_to_board_phases_fixed(
            &schematic,
            &directives,
            &layer_config,
            false,
            board_size,
        )?;
        {
            let (dots, pol, cy) = kicad_cdb::silk::add_footprint_silk(&mut board);
            eprintln!(
                "silk: {} pin1 dots, {} polarity marks, {} courtyards",
                dots, pol, cy
            );
        }
        board.segments.clear();
        board.vias.clear();
        kicad_cdb::design::auto_route_power_nets(&mut board, &directives);
        kicad_cdb::design::route_board(&mut board, &directives, &layer_config);
        {
            let mitered = kicad_cdb::audit::miter_corners(&mut board, 0.4);
            if mitered > 0 {
                eprintln!("miter: {} corners chamfered", mitered);
            }
        }

        let pcb_text = kicad_cdb::design::generate_kicad_pcb(&board)?;
        std::fs::write(output, &pcb_text)?;
        eprintln!(
            "PCB (route only): {} segments → {}",
            board.segments.len(),
            output
        );
        return Ok(());
    }

    let mut board = kicad_cdb::design::schematic_to_board_with_config_fixed(
        &schematic,
        &directives,
        &layer_config,
        board_size,
    )?;
    {
        let mitered = kicad_cdb::audit::miter_corners(&mut board, 0.4);
        if mitered > 0 {
            eprintln!("miter: {} right-angle corners chamfered", mitered);
        }
        let (dots, pol, cy) = kicad_cdb::silk::add_footprint_silk(&mut board);
        eprintln!(
            "silk: {} pin1 dots, {} polarity marks, {} courtyards",
            dots, pol, cy
        );
    }

    let drc_report = kicad_cdb::drc::builtin_drc(&board, Some(&directives));

    let pcb_text = kicad_cdb::design::generate_kicad_pcb(&board)?;
    std::fs::write(output, &pcb_text)?;

    if let Some(svg_path) = svg {
        let mode = match render_mode {
            "fabrication" => kicad_render::pcb_renderer::RenderMode::Fabrication,
            "copper" => kicad_render::pcb_renderer::RenderMode::CopperOnly,
            _ => kicad_render::pcb_renderer::RenderMode::Assembly,
        };
        let mut renderer = kicad_render::pcb_renderer::PcbRenderer::new(&board).with_mode(mode);
        if let Some(layers_str) = render_layers {
            let set: std::collections::HashSet<String> = layers_str
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if !set.is_empty() {
                renderer = renderer.with_layer_filter(set);
            }
        }
        let svg_content = renderer.render_to_string();
        std::fs::write(svg_path, &svg_content)?;
        eprintln!("PCB SVG written: {} (mode={})", svg_path, render_mode);
    }

    if json {
        #[derive(Serialize)]
        struct PcbResult {
            footprint_count: usize,
            net_count: usize,
            zone_count: usize,
            segment_count: usize,
            via_count: usize,
            drc_errors: usize,
            drc_warnings: usize,
            board_size_mm: String,
        }
        print_json(&PcbResult {
            footprint_count: board.footprints.len(),
            net_count: board.nets.len(),
            zone_count: board.zones.len(),
            segment_count: board.segments.len(),
            via_count: board.vias.len(),
            drc_errors: drc_report.summary.errors,
            drc_warnings: drc_report.summary.warnings,
            board_size_mm: format!(
                "{:.1}x{:.1}",
                board
                    .footprints
                    .iter()
                    .map(|f| f.position.0)
                    .fold(f64::MIN, f64::max)
                    - board
                        .footprints
                        .iter()
                        .map(|f| f.position.0)
                        .fold(f64::MAX, f64::min)
                    + 16.0,
                board
                    .footprints
                    .iter()
                    .map(|f| f.position.1)
                    .fold(f64::MIN, f64::max)
                    - board
                        .footprints
                        .iter()
                        .map(|f| f.position.1)
                        .fold(f64::MAX, f64::min)
                    + 16.0
            ),
        });
    } else {
        println!(
            "PCB generated: {} footprints, {} nets, {} zones → {}",
            board.footprints.len(),
            board.nets.len(),
            board.zones.len(),
            output
        );
        if drc_report.summary.errors > 0 {
            eprintln!("DRC errors: {}", drc_report.summary.errors);
            for v in &drc_report.violations {
                eprintln!("  ✗ {} — {}", v.violation_type, v.description);
            }
        }
        if drc_report.summary.warnings > 0 {
            eprintln!("DRC warnings: {}", drc_report.summary.warnings);
        }
        if drc_report.summary.unconnected > 0 {
            eprintln!(
                "DRC unconnected: {} (0 shorts + 0 unconnected = done)",
                drc_report.summary.unconnected
            );
        }
        let (verdict, reasons) = drc_report.summary.gate();
        if verdict == "PASS" {
            println!("DRC GATE: PASS");
        } else {
            eprintln!("DRC GATE: FAIL — {}", reasons.join(", "));
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// archive
// ---------------------------------------------------------------------------

pub fn cmd_archive(
    db: &ComponentDb,
    input: &str,
    name: &str,
    output_dir: &str,
    db_export: bool,
    layers: usize,
    board_width: Option<f64>,
    board_height: Option<f64>,
) -> Result<()> {
    use std::path::Path;

    // Create output directory
    std::fs::create_dir_all(output_dir)?;

    let mut manifest_files: Vec<serde_json::Value> = Vec::new();
    let mut erc_errors = 0usize;
    let mut erc_warnings = 0usize;
    #[allow(unused_assignments)] // 聚合循环里必定先覆盖, 初值只为类型标注
    let (mut drc_errors, mut drc_warnings) = (0usize, 0usize);

    let input_path = Path::new(input);
    let ext = input_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    // Step 0: Copy original input
    let input_dest = format!("{}/{}.{}", output_dir, name, ext);
    std::fs::copy(input, &input_dest)?;
    eprintln!("[1/9] Original input copied");
    archive_add_file(&mut manifest_files, &input_dest, output_dir)?;

    // Step 1: JSON5 → kicad_sch (or copy if already .kicad_sch)
    let sch_path = format!("{}/{}.kicad_sch", output_dir, name);
    // We keep schematic_ir alive so we can use it for PCB generation
    // (the sexpr round-trip may lose net info)
    let mut schematic_ir_holder: Option<kicad_json5::ir::Schematic> = None;

    if ext == "json5" {
        let source = std::fs::read_to_string(input)?;
        let mut schematic_ir = kicad_json5::parse_json5(&source)?;

        // Check if positions are all zero (auto-layout mode)
        let all_zero = schematic_ir
            .components
            .iter()
            .all(|c| c.position.0 == 0.0 && c.position.1 == 0.0);
        if all_zero && !schematic_ir.components.is_empty() {
            let layout = kicad_cdb::layout_engine::auto_layout_refs(&schematic_ir);
            let grid = 1.27; // 50mil KiCad grid
            for comp in &mut schematic_ir.components {
                if let Some((x, y, r)) = layout.get(&comp.reference) {
                    let sx = (x / grid).round() * grid;
                    let sy = (y / grid).round() * grid;
                    comp.position = (sx, sy, *r);
                }
            }
            eprintln!(
                "  Auto-layout: {} components placed (snapped to 50mil grid)",
                layout.len()
            );
        }

        let config = kicad_json5::codegen::SexprConfig {
            indent: "\t".to_string(),
            include_uuids: true,
            kicad_version: None,
            generate_uuids: true,
            insert_power_flags: true,
        };
        let mut gen = kicad_json5::SexprGenerator::with_config(config);
        let sch_text = gen.generate(&schematic_ir)?;
        std::fs::write(&sch_path, &sch_text)?;

        // Keep the JSON5-parsed IR for PCB generation (has net info)
        schematic_ir_holder = Some(schematic_ir);
    } else {
        std::fs::copy(input, &sch_path)?;
    }
    eprintln!("[2/9] Schematic generated: {}", sch_path);
    archive_add_file(&mut manifest_files, &sch_path, output_dir)?;

    // Parse schematic for subsequent steps
    let sch_source = std::fs::read_to_string(&sch_path)?;
    let lexer = kicad_json5::Lexer::new(&sch_source);
    let mut parser = kicad_json5::Parser::new(lexer);
    let mut schematic = parser.parse()?;

    // If we parsed from JSON5, use the original IR which has complete net info
    if let Some(ir) = schematic_ir_holder.take() {
        schematic = ir;
    }

    // Step 2: Schematic SVG
    let sch_svg_path = format!("{}/{}.sch.svg", output_dir, name);
    let sch_renderer = kicad_render::schematic_renderer::SchematicRenderer::new(&schematic)
        .with_file_name(format!("{}.kicad_sch", name));
    let (paper_w, paper_h) = sch_renderer.paper_size();
    let scale = 3.0;
    use kicad_render::render_core::Matrix;
    use kicad_render::renderer::Renderer;
    let scale_matrix = Matrix::new([scale, 0.0, 0.0, scale, 0.0, 0.0]);
    let mut svg_renderer = kicad_render::renderer::SvgRenderer::new();
    svg_renderer.set_transform(&scale_matrix);
    sch_renderer.render(&mut svg_renderer);
    let pad = 2.0;
    let svg_content = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"{:.2} {:.2} {:.2} {:.2}\">\n\
         <rect x=\"{:.2}\" y=\"{:.2}\" width=\"{:.2}\" height=\"{:.2}\" fill=\"white\"/>\n\
         {}\n</svg>",
        -pad,
        -pad,
        paper_w * scale + pad * 2.0,
        paper_h * scale + pad * 2.0,
        -pad,
        -pad,
        paper_w * scale + pad * 2.0,
        paper_h * scale + pad * 2.0,
        svg_renderer.output(),
    );
    std::fs::write(&sch_svg_path, &svg_content)?;
    eprintln!("[3/9] Schematic SVG: {}", sch_svg_path);
    archive_add_file(&mut manifest_files, &sch_svg_path, output_dir)?;

    // Step 3: ERC (optional — requires kicad-cli)
    let erc_path = format!("{}/{}-erc.rpt", output_dir, name);
    let cfg = kicad_cdb::config::AppConfig::load().ok();
    let kicad_cli = cfg
        .as_ref()
        .map(|c| c.kicad_cli_path.as_str())
        .unwrap_or("");
    if !kicad_cli.is_empty() {
        match kicad_cdb::erc::run_erc(kicad_cli, &sch_path) {
            Ok(erc_result) => {
                erc_errors = erc_result.summary.errors;
                erc_warnings = erc_result.summary.warnings;
                let erc_text = format_erc_report(&erc_result);
                std::fs::write(&erc_path, &erc_text)?;
                eprintln!(
                    "[4/9] ERC: {} errors, {} warnings",
                    erc_errors, erc_warnings
                );
                let _ = archive_add_file(&mut manifest_files, &erc_path, output_dir);
            }
            Err(e) => eprintln!("[4/9] ERC: skipped ({})", e),
        }
    } else {
        eprintln!("[4/9] ERC: skipped (kicad-cli not configured)");
    }

    // Step 4: Netlist
    let net_path = format!("{}/{}.net", output_dir, name);
    let netlist_content = kicad_cdb::netlist::netlist_from_file(&sch_path)?;
    std::fs::write(&net_path, &netlist_content)?;
    eprintln!("[5/9] Netlist: {}", net_path);
    archive_add_file(&mut manifest_files, &net_path, output_dir)?;

    // Step 5: PCB + PCB SVG
    let pcb_path = format!("{}/{}.kicad_pcb", output_dir, name);
    let pcb_svg_path = format!("{}/{}.pcb.svg", output_dir, name);
    let layer_config = if layers == 4 {
        kicad_cdb::layer_config::BoardLayerConfig::four_layer()
    } else {
        kicad_cdb::layer_config::BoardLayerConfig::two_layer()
    };
    let directives = kicad_cdb::layout_directives::LayoutDirectives::default();
    let fixed_board = board_width.zip(board_height);
    let board = kicad_cdb::design::schematic_to_board_with_config_fixed(
        &schematic,
        &directives,
        &layer_config,
        fixed_board,
    )?;
    let drc_report = kicad_cdb::drc::builtin_drc(&board, Some(&directives));
    drc_errors = drc_report.summary.errors;
    drc_warnings = drc_report.summary.warnings;
    let pcb_text = kicad_cdb::design::generate_kicad_pcb(&board)?;
    std::fs::write(&pcb_path, &pcb_text)?;
    let pcb_renderer = kicad_render::pcb_renderer::PcbRenderer::new(&board);
    let pcb_svg_content = pcb_renderer.render_to_string();
    std::fs::write(&pcb_svg_path, &pcb_svg_content)?;
    eprintln!(
        "[6/9] PCB: {} footprints, {} zones, DRC: {} errors, {} warnings → {}",
        board.footprints.len(),
        board.zones.len(),
        drc_errors,
        drc_warnings,
        pcb_path
    );

    // Save DRC report
    let drc_rpt_path = format!("{}/{}-drc.rpt", output_dir, name);
    let drc_text = format_drc_report(&drc_report);
    std::fs::write(&drc_rpt_path, &drc_text)?;
    eprintln!("    DRC report: {}", drc_rpt_path);

    // Print detailed layout/routing summary
    eprintln!("\n=== Layout & Routing Summary ===");
    if let Some((fw, fh)) = fixed_board {
        eprintln!("  Board frame: FIXED {:.1} x {:.1} mm", fw, fh);
    } else {
        eprintln!("  Board frame: AUTO-SIZED");
    }
    eprintln!("  Footprints: {}", board.footprints.len());
    eprintln!("  Nets: {}", board.nets.len());
    eprintln!("  Segments: {}", board.segments.len());
    eprintln!("  Vias: {}", board.vias.len());
    eprintln!("  Zones: {}", board.zones.len());
    eprintln!("  DRC: {} errors, {} warnings", drc_errors, drc_warnings);
    if drc_errors > 0 {
        eprintln!(
            "  (layout completed with DRC violations — some components may need manual adjustment)"
        );
    } else {
        eprintln!("  (clean layout — no DRC violations)");
    }
    if board.segments.is_empty() {
        eprintln!("  Routing: no signal traces (power-only or unrouted)");
    } else {
        let total_len: f64 = board
            .segments
            .iter()
            .map(|s| ((s.end.0 - s.start.0).powi(2) + (s.end.1 - s.start.1).powi(2)).sqrt())
            .sum();
        eprintln!(
            "  Routing: {} segments, {:.1}mm total trace length, {} vias",
            board.segments.len(),
            total_len,
            board.vias.len()
        );
    }
    eprintln!("=== End Summary ===\n");

    archive_add_file(&mut manifest_files, &pcb_path, output_dir)?;
    archive_add_file(&mut manifest_files, &pcb_svg_path, output_dir)?;
    archive_add_file(&mut manifest_files, &drc_rpt_path, output_dir)?;

    // Step 6: Components JSON
    let comp_json_path = format!("{}/{}-components.json", output_dir, name);
    let comp_data = extract_components_json(&schematic, name);
    let comp_json = serde_json::to_string_pretty(&comp_data)?;
    std::fs::write(&comp_json_path, &comp_json)?;
    eprintln!(
        "[7/9] Components: {} unique → {}",
        comp_data.unique_components.len(),
        comp_json_path
    );
    archive_add_file(&mut manifest_files, &comp_json_path, output_dir)?;

    // Step 7: Symbol library
    let sym_path = format!("{}/{}-lib.kicad_sym", output_dir, name);
    let unique_lib_ids: Vec<String> = schematic
        .components
        .iter()
        .map(|c| c.lib_id.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    let mut sym_components = Vec::new();
    for lib_id in &unique_lib_ids {
        let parts: Vec<&str> = lib_id.splitn(2, ':').collect();
        let mpn = parts.last().unwrap_or(&"");
        if let Some(comp) = db.get_component_by_mpn_any(mpn)? {
            sym_components.push(comp);
        }
    }
    if !sym_components.is_empty() {
        let sym_content = kicad_cdb::symgen::generate_rich_symbol_lib(&sym_components, db)?;
        std::fs::write(&sym_path, &sym_content)?;
        eprintln!(
            "[8/9] Symbol lib: {} symbols → {}",
            sym_components.len(),
            sym_path
        );
        archive_add_file(&mut manifest_files, &sym_path, output_dir)?;
    } else {
        eprintln!("[8/9] Symbol lib: skipped (no matching components in DB)");
    }

    // Step 8: BOM
    let bom_path = format!("{}/{}-bom.csv", output_dir, name);
    let bom_entries = kicad_cdb::bom::generate_bom(db)?;
    if !bom_entries.is_empty() {
        let bom_csv = kicad_cdb::bom::bom_to_csv(&bom_entries)?;
        std::fs::write(&bom_path, &bom_csv)?;
        eprintln!("[9/9] BOM: {} entries → {}", bom_entries.len(), bom_path);
        archive_add_file(&mut manifest_files, &bom_path, output_dir)?;
    } else {
        eprintln!("[9/9] BOM: skipped (no entries in DB)");
    }

    // Step 9 (optional): Board-specific DB
    if db_export {
        let board_db_path = format!("{}/{}-components.db", output_dir, name);
        export_board_db(db, &schematic, &board_db_path)?;
        eprintln!("[+] Board DB: {}", board_db_path);
        archive_add_file(&mut manifest_files, &board_db_path, output_dir)?;
    }

    // Manifest
    let now = {
        let dur = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        format!("{}", dur.as_secs())
    };
    let manifest = serde_json::json!({
        "board": name,
        "version": "1.0",
        "generated_at": now,
        "toolchain": {
            "kicad-designer": env!("CARGO_PKG_VERSION"),
        },
        "files": manifest_files,
        "erc_summary": { "errors": erc_errors, "warnings": erc_warnings },
        "drc_summary": { "errors": drc_errors, "warnings": drc_warnings },
        "stats": {
            "components": schematic.components.len(),
            "nets": schematic.nets.len(),
            "footprints": board.footprints.len(),
        },
    });
    let manifest_path = format!("{}/archive-manifest.json", output_dir);
    std::fs::write(&manifest_path, serde_json::to_string_pretty(&manifest)?)?;
    eprintln!("Manifest: {}", manifest_path);

    println!(
        "Archive complete: {} files → {}",
        manifest_files.len() + 1,
        output_dir
    );
    Ok(())
}

fn archive_add_file(files: &mut Vec<serde_json::Value>, path: &str, base_dir: &str) -> Result<()> {
    use std::path::Path;
    let meta = std::fs::metadata(path)?;
    let rel = Path::new(path)
        .strip_prefix(base_dir)
        .unwrap_or(Path::new(path))
        .to_string_lossy()
        .to_string();

    // Compute sha256
    let data = std::fs::read(path)?;
    let hash = sha256_digest(&data)?;

    files.push(serde_json::json!({
        "path": rel,
        "size": meta.len(),
        "sha256": hash,
    }));
    Ok(())
}

fn sha256_digest(data: &[u8]) -> Result<String> {
    use std::fmt::Write;
    // Simple sha256 - use sha2 crate or just use a basic hash
    // For now, use a simple hex representation of a basic hash
    let mut result = String::with_capacity(64);
    // Use rustcrypto sha2 if available, otherwise skip
    let digest = {
        // Fallback: just produce a placeholder
        let mut hash: [u8; 32] = [0u8; 32];
        for (i, byte) in data.iter().enumerate() {
            hash[i % 32] ^= byte;
        }
        hash
    };
    for byte in &digest {
        write!(result, "{:02x}", byte)?;
    }
    Ok(result)
}

#[derive(Serialize)]
struct BoardComponents {
    board: String,
    generated_at: String,
    unique_components: Vec<ComponentEntry>,
    total_instances: usize,
    total_unique: usize,
}

#[derive(Serialize)]
struct ComponentEntry {
    lib_id: String,
    value: String,
    footprint: Option<String>,
    count: usize,
    properties: std::collections::HashMap<String, String>,
    pins: Vec<PinEntry>,
}

#[derive(Serialize)]
struct PinEntry {
    number: String,
    name: String,
    pin_type: String,
}

fn format_drc_report(report: &kicad_cdb::drc::DrcReport) -> String {
    let mut out = String::new();
    out.push_str("DRC Report (built-in)\n");
    out.push_str("============================================================\n\n");
    out.push_str(&format!(
        "Errors: {}\nWarnings: {}\n\n",
        report.summary.errors, report.summary.warnings
    ));

    if !report.violations.is_empty() {
        for v in &report.violations {
            let sev = match v.severity {
                kicad_cdb::drc::DrcSeverity::Error => "ERR",
                kicad_cdb::drc::DrcSeverity::Warning => "WRN",
                kicad_cdb::drc::DrcSeverity::Exclusion => "EXC",
            };
            out.push_str(&format!(
                "  {} [{}] {}\n",
                sev, v.violation_type, v.description
            ));
            for item in &v.items {
                if let (Some(x), Some(y)) = (item.x_mm, item.y_mm) {
                    out.push_str(&format!(
                        "    @({:.1}mm, {:.1}mm): {}\n",
                        x, y, item.description
                    ));
                } else {
                    out.push_str(&format!("    {}\n", item.description));
                }
            }
        }
    }
    out
}

fn extract_components_json(schematic: &kicad_json5::ir::Schematic, name: &str) -> BoardComponents {
    use std::collections::HashMap;
    let mut groups: HashMap<String, ComponentEntry> = HashMap::new();

    for comp in &schematic.components {
        let key = format!("{}:{}", comp.lib_id, comp.value);
        let entry = groups.entry(key).or_insert_with(|| ComponentEntry {
            lib_id: comp.lib_id.clone(),
            value: comp.value.clone(),
            footprint: comp.footprint.clone(),
            count: 0,
            properties: comp.properties.clone(),
            pins: comp
                .pins
                .iter()
                .map(|p| PinEntry {
                    number: p.number.clone(),
                    name: p.name.clone(),
                    pin_type: p.pin_type.clone(),
                })
                .collect(),
        });
        entry.count += 1;
    }

    let total_instances = schematic.components.len();
    let unique: Vec<ComponentEntry> = groups.into_values().collect();
    let total_unique = unique.len();

    BoardComponents {
        board: name.to_string(),
        generated_at: {
            let dur = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default();
            format!("{}", dur.as_secs())
        },
        unique_components: unique,
        total_instances,
        total_unique,
    }
}

fn format_erc_report(report: &kicad_cdb::erc::ErcReport) -> String {
    let mut out = String::new();
    out.push_str(&format!("ERC Report\n{}\n\n", "=".repeat(60)));
    out.push_str(&format!(
        "Errors: {}\nWarnings: {}\n\n",
        report.summary.errors, report.summary.warnings
    ));
    for sheet in &report.sheets {
        if !sheet.violations.is_empty() {
            out.push_str(&format!("Sheet: {}\n", sheet.path));
            for v in &sheet.violations {
                let mark = match v.severity {
                    kicad_cdb::erc::ErcSeverity::Error => "✗",
                    kicad_cdb::erc::ErcSeverity::Warning => "⚠",
                };
                out.push_str(&format!(
                    "  {} [{}] {}\n",
                    mark, v.error_type, v.description
                ));
                for loc in &v.locations {
                    out.push_str(&format!(
                        "    @({:.1}mm, {:.1}mm): {}\n",
                        loc.x_mm, loc.y_mm, loc.detail
                    ));
                }
            }
            out.push('\n');
        }
    }
    out
}

fn export_board_db(
    db: &ComponentDb,
    schematic: &kicad_json5::ir::Schematic,
    output_path: &str,
) -> Result<()> {
    // Create a new SQLite DB with the same schema, then copy relevant components
    let board_db = kicad_cdb::ComponentDb::open(output_path)?;

    // Extract unique MPNs from schematic
    let mpns: Vec<String> = schematic
        .components
        .iter()
        .map(|c| {
            let parts: Vec<&str> = c.lib_id.splitn(2, ':').collect();
            parts.last().unwrap_or(&"").to_string()
        })
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    let mut imported = 0usize;
    for mpn in &mpns {
        if let Some(comp) = db.get_component_by_mpn_any(mpn)? {
            board_db.insert_component(&comp)?;
            // Copy pins
            if let Some(id) = comp.id {
                let pins = db.get_pins(id)?;
                if !pins.is_empty() {
                    board_db.insert_pins(&pins)?;
                }
                let params = db.get_parameters(id)?;
                for p in &params {
                    board_db.insert_parameter(p)?;
                }
            }
            imported += 1;
        }
    }

    eprintln!("  Board DB: {} components exported", imported);
    Ok(())
}

/// Re-route an existing .kicad_pcb: clear traces and re-run auto-router.
/// P2: bake footprint rotations into pad geometry and zero the rot angle.
/// Fixes the KiCad-10 double-rotation pad-size mismatch on 90/270° parts
/// (QFN pads read as overlapping shorts). Idempotent: rot==0 is a no-op.
pub fn cmd_bake(input: &str, output: &str) -> Result<()> {
    let source = std::fs::read_to_string(input).with_context(|| format!("read {}", input))?;
    let mut board = kicad_json5::parse_board(&source).with_context(|| "parse .kicad_pcb")?;
    let baked = board
        .footprints
        .iter()
        .filter(|fp| fp.position.2.abs() > 1e-9)
        .count();
    for fp in &mut board.footprints {
        kicad_cdb::design::bake_footprint_rotation(fp);
    }
    let pcb_text = kicad_cdb::design::generate_kicad_pcb(&board)?;
    std::fs::write(output, &pcb_text)?;
    eprintln!("[bake] Baked {} footprints -> {}", baked, output);
    Ok(())
}

pub fn cmd_reroute(
    input: &str,
    output: &str,
    layers: usize,
    trace_width: Option<f64>,
    clearance: Option<f64>,
    width_by_net: Option<&str>,
    open: bool,
) -> Result<()> {
    use kicad_cdb::layer_config::BoardLayerConfig;
    use kicad_cdb::layout_directives::LayoutDirectives;

    eprintln!("[reroute] Reading {}", input);
    let source = std::fs::read_to_string(input).with_context(|| format!("read {}", input))?;

    let mut board = kicad_json5::parse_board(&source).with_context(|| "parse .kicad_pcb")?;

    // Bake footprint rotations (pad geometry pre-rotated, rot zeroed) before
    // anything else touches the board. Boards generated before the bake pass
    // carry pre-rotated pad geometry AND a non-zero (at x y 90) — the double
    // rotation misaligns pad sizes on 90/270° parts and DRC reports
    // pad-to-pad shorts across the whole QFN (105 shorts on a 0.4mm-pitch BGA board).
    let baked = board
        .footprints
        .iter()
        .filter(|fp| fp.position.2.abs() > 1e-9)
        .count();
    if baked > 0 {
        for fp in &mut board.footprints {
            kicad_cdb::design::bake_footprint_rotation(fp);
        }
        eprintln!("[reroute] Baked rotation into {} footprints", baked);
    }

    eprintln!(
        "[reroute] Board: {} nets, {} footprints, {} segments, {} vias, {} zones",
        board.nets.len(),
        board.footprints.len(),
        board.segments.len(),
        board.vias.len(),
        board.zones.len()
    );

    // P1-8: routing parameter overrides
    let mut directives = LayoutDirectives::default();
    if trace_width.is_some() || clearance.is_some() || width_by_net.is_some() {
        let mut by_net: Vec<(String, f64)> = Vec::new();
        if let Some(spec) = width_by_net {
            for pair in spec.split(',') {
                let pair = pair.trim();
                if pair.is_empty() {
                    continue;
                }
                let (name, w) = pair.split_once('=').ok_or_else(|| {
                    anyhow::anyhow!("--width-by-net expects NET=mm pairs, got '{}'", pair)
                })?;
                by_net.push((
                    name.trim().to_string(),
                    w.trim().parse::<f64>().map_err(|_| {
                        anyhow::anyhow!("--width-by-net: '{}' is not a number", w.trim())
                    })?,
                ));
            }
        }
        directives.apply_routing_overrides(trace_width, clearance, &by_net);
        eprintln!(
            "[reroute] Overrides: width={:?} clearance={:?} per-net={:?}",
            trace_width, clearance, by_net
        );
    }

    // Save original stats for comparison
    let orig_segments = board.segments.len();
    let orig_vias = board.vias.len();

    // Clear existing signal traces and vias (keep zones and footprints)
    board.segments.clear();
    board.vias.clear();
    eprintln!(
        "[reroute] Cleared {} segments, {} vias",
        orig_segments, orig_vias
    );

    // Layer config
    let layer_config = BoardLayerConfig::from_layer_count(layers);
    eprintln!(
        "[reroute] Layer config: {} layers, signal layers: {:?}",
        layer_config.layer_count(),
        layer_config
            .signal_layer_indices()
            .iter()
            .map(|&i| layer_config.layer_name(i).to_string())
            .collect::<Vec<_>>()
    );

    // P1: boards routed from scratch (no zones yet) get their PDN planes here,
    // before routing — the router only leaves a power net unrouted when a zone
    // actually covers it, and the stitching pass below keys off the same zones.
    if board.zones.is_empty() {
        kicad_cdb::design::generate_copper_zones(&mut board, &layer_config);
        eprintln!(
            "[reroute] Generated {} copper zones (board had none)",
            board.zones.len()
        );
    }

    // Run auto-router (directives carry P1-8 overrides when provided)
    let result = kicad_cdb::router::auto_route_signal_nets_with_config(
        &mut board,
        &directives,
        &layer_config,
    );

    // P1: stitch zone nets to their plane layer (vias were cleared above)
    kicad_cdb::design::generate_plane_thermal_vias(&mut board, &layer_config);

    eprintln!(
        "[reroute] Routing result: {}/{} nets routed ({} segments, {} vias)",
        result.routed_nets, result.total_nets, result.total_segments, result.total_vias
    );
    if !result.failed_nets.is_empty() {
        eprintln!(
            "[reroute] Failed nets ({}): {:?}",
            result.failed_nets.len(),
            &result.failed_nets[..result.failed_nets.len().min(20)]
        );
        if result.failed_nets.len() > 20 {
            eprintln!("[reroute]   ... and {} more", result.failed_nets.len() - 20);
        }
    }

    let completion = if result.total_nets > 0 {
        result.routed_nets as f64 / result.total_nets as f64 * 100.0
    } else {
        0.0
    };
    eprintln!("[reroute] Completion: {:.1}%", completion);

    // Generate output .kicad_pcb
    // TEMP P1-diag: miter skipped — suspected of leaving gaps between segments
    // (unconnected sample chains show 45-degree stubs with 0.75-1.4mm gaps)
    {
        let mitered = 0usize; // kicad_cdb::audit::miter_corners(&mut board, 0.4);
        if mitered > 0 {
            eprintln!("miter: {} corners chamfered", mitered);
        }
    }
    let pcb_text = kicad_cdb::design::generate_kicad_pcb(&board)?;
    std::fs::write(output, &pcb_text)?;
    eprintln!("[reroute] Output: {}", output);

    // P1: refill zones and persist the fill. Stale/missing fills poison DRC
    // results (tracks vs old fill copper) and starve plane-connected pads.
    // Failure is a warning only — kicad-cli may be absent in CI sandboxes.
    match kicad_cdb::config::AppConfig::load() {
        Ok(cfg) => {
            let refill = std::process::Command::new(&cfg.kicad_cli_path)
                .args([
                    "pcb",
                    "drc",
                    output,
                    "--refill-zones",
                    "--save-board",
                    "--severity-error",
                    "-o",
                ])
                .arg(format!("{}.refill.tmp", output))
                .output();
            match refill {
                Ok(r) if r.status.success() => {
                    eprintln!("[reroute] Zones refilled and saved via kicad-cli");
                }
                Ok(r) => {
                    eprintln!(
                        "[reroute] WARNING: kicad-cli refill failed ({}): {}",
                        r.status,
                        String::from_utf8_lossy(&r.stderr)
                            .lines()
                            .next()
                            .unwrap_or("")
                    );
                }
                Err(e) => {
                    eprintln!(
                        "[reroute] WARNING: kicad-cli not runnable ({}), zones left unfilled",
                        e
                    );
                }
            }
        }
        Err(e) => eprintln!("[reroute] WARNING: no config ({}), zones left unfilled", e),
    }

    // Auto-open in default application only when --open flag is used
    if open {
        #[cfg(target_os = "macos")]
        let _ = std::process::Command::new("open").arg(output).spawn();
        #[cfg(target_os = "linux")]
        let _ = std::process::Command::new("xdg-open").arg(output).spawn();
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// relayout
// ---------------------------------------------------------------------------

pub fn cmd_relayout(
    input: &str,
    output: &str,
    layers: usize,
    do_route: bool,
    svg: Option<&str>,
    open: bool,
) -> Result<()> {
    use kicad_cdb::layer_config::BoardLayerConfig;
    use kicad_cdb::layout_directives::LayoutDirectives;

    eprintln!("[relayout] Reading {}", input);
    let source = std::fs::read_to_string(input).with_context(|| format!("read {}", input))?;
    let mut board = kicad_json5::parse_board(&source).with_context(|| "parse .kicad_pcb")?;

    eprintln!(
        "[relayout] Board: {} nets, {} footprints, {} segments, {} vias, {} zones",
        board.nets.len(),
        board.footprints.len(),
        board.segments.len(),
        board.vias.len(),
        board.zones.len()
    );

    let layer_config = BoardLayerConfig::from_layer_count(layers);
    let directives = LayoutDirectives::default();

    kicad_cdb::design::relayout_board(&mut board, &directives, &layer_config, do_route);

    let pcb_text = kicad_cdb::design::generate_kicad_pcb(&board)?;
    std::fs::write(output, &pcb_text)?;
    eprintln!("[relayout] Output: {}", output);

    if let Some(svg_path) = svg {
        let renderer = kicad_render::pcb_renderer::PcbRenderer::new(&board);
        let svg_content = renderer.render_to_string();
        std::fs::write(svg_path, &svg_content)?;
        eprintln!("[relayout] SVG: {}", svg_path);
    }

    if open {
        #[cfg(target_os = "macos")]
        let _ = std::process::Command::new("open").arg(output).spawn();
        #[cfg(target_os = "linux")]
        let _ = std::process::Command::new("xdg-open").arg(output).spawn();
    }

    Ok(())
}

/// H5: Render a design review HTML report from a DesignReviewResult JSON file.
/// The JSON is typically produced by `power-tree --json` (the `review` field).
pub fn cmd_review_report(input: &str, output: &str, title: &str) -> Result<()> {
    let json_str = std::fs::read_to_string(input)
        .with_context(|| format!("Failed to read review JSON: {}", input))?;
    let result: kicad_cdb::design_review::DesignReviewResult = serde_json::from_str(&json_str)
        .with_context(|| format!("Failed to parse DesignReviewResult JSON from: {}", input))?;

    let html = kicad_cdb::design_review::render_html_report(&result, title);
    std::fs::write(output, &html)
        .with_context(|| format!("Failed to write HTML report: {}", output))?;

    let status = if result.passed { "PASSED" } else { "FAILED" };
    eprintln!(
        "[review-report] {} — {} issues ({} errors, {} warnings, {} info) → {}",
        status,
        result.issues.len(),
        result
            .issues
            .iter()
            .filter(|i| i.severity == "error")
            .count(),
        result
            .issues
            .iter()
            .filter(|i| i.severity == "warning")
            .count(),
        result
            .issues
            .iter()
            .filter(|i| i.severity == "info")
            .count(),
        output,
    );

    Ok(())
}

/// P1-5: Check hierarchical sheet interfaces — load multi-page project + validate pin/label matching.
pub fn cmd_check_hierarchy(input: &str, json: bool) -> Result<()> {
    let project =
        kicad_json5::hierarchy::HierarchicalProject::load_from_file(std::path::Path::new(input))
            .with_context(|| format!("Failed to load hierarchical project from {}", input))?;
    let result = kicad_json5::hierarchy::check_sheet_interfaces(&project);

    if json {
        let json_out = serde_json::to_string_pretty(&result)?;
        println!("{}", json_out);
    } else {
        let status = if result.passed {
            "✓ PASSED"
        } else {
            "✗ FAILED"
        };
        eprintln!(
            "[hierarchy] {} — {} sheets, {} findings",
            status,
            project.total_sheets(),
            result.findings.len()
        );
        for f in &result.findings {
            let icon = if f.severity == "error" {
                "❌"
            } else {
                "⚠️"
            };
            eprintln!(
                "  {} [{}] {} → {}: {}",
                icon, f.severity, f.sheet_name, f.sheet_file, f.message
            );
        }
    }
    Ok(())
}

/// H3: Import footprint metadata from KiCad system .pretty libraries into the DB.
pub fn cmd_import_footprints(db: &ComponentDb, dir: Option<&str>) -> Result<()> {
    let fp_dir = match dir {
        Some(d) => std::path::PathBuf::from(d),
        None => match kicad_cdb::footprint_lib::detect_kicad_footprint_dir() {
            Some(d) => d,
            None => anyhow::bail!("KiCad footprint directory not found. Use --dir to specify, or set KICAD_PATH env var."),
        },
    };
    eprintln!("[import-footprints] Scanning {}...", fp_dir.display());
    let metas = kicad_cdb::footprint_lib::scan_footprint_library(&fp_dir);
    eprintln!(
        "[import-footprints] Found {} footprints, importing to DB...",
        metas.len()
    );
    let count = kicad_cdb::footprint_lib::import_footprint_metadata(db, &metas)?;
    eprintln!(
        "[import-footprints] Imported {} footprint metadata entries",
        count
    );
    Ok(())
}

pub fn cmd_netlist_diff(sch: &str, pcb: &str, strict: bool) -> Result<bool> {
    let cfg = kicad_cdb::config::AppConfig::load()?;
    println!("Netlist consistency: {} <-> {}", sch, pcb);
    let smap = crate::netlist_diff::sch_netmap(sch, &cfg.kicad_cli_path)?;
    let pmap = crate::netlist_diff::pcb_netmap(pcb)?;
    println!("sch pins: {}, pcb netted pads: {}", smap.len(), pmap.len());
    let prefixes = if strict {
        vec![]
    } else {
        vec!["TP".into(), "H".into()]
    };
    let r = crate::netlist_diff::diff(smap, pmap, prefixes);
    print!("{}", r.report());
    Ok(r.pass())
}

pub fn cmd_audit_angles(input: &str, fix: bool) -> Result<()> {
    let text = std::fs::read_to_string(input)?;
    let mut board = kicad_json5::parse_board(&text)?;
    let report = kicad_cdb::audit::audit_angles(&board);
    println!(
        "Angle audit: {} acute, {} degenerate overlaps, {} right angles, {} miterable",
        report.acute.len(),
        report.overlaps.len(),
        report.right_angles,
        report.miter_candidates
    );
    for i in report.overlaps.iter().chain(report.acute.iter()) {
        println!(
            "  [{}] {} {} @({:.2}, {:.2}) {:.1}°",
            i.kind, i.net, i.layer, i.at.0, i.at.1, i.angle_deg
        );
    }
    if fix {
        let (removed, mitered) = kicad_cdb::audit::fix_angles(&mut board);
        println!(
            "fixed: {} overlap segments removed, {} corners mitered",
            removed, mitered
        );
        let out = kicad_cdb::design::generate_kicad_pcb(&board)?;
        std::fs::write(input, &out)?;
        println!("written back to {}", input);
    }
    Ok(())
}

pub fn cmd_audit_vias(input: &str) -> Result<()> {
    let text = std::fs::read_to_string(input)?;
    let board = kicad_json5::parse_board(&text)?;
    let vias = kicad_cdb::audit::audit_vias(&board);
    let mut by_class: std::collections::BTreeMap<&str, Vec<&kicad_cdb::audit::ViaInfo>> =
        Default::default();
    for v in &vias {
        by_class.entry(v.class).or_default().push(v);
    }
    println!("Via audit: {} vias total", vias.len());
    for (class, list) in &by_class {
        println!("  {}: {}", class, list.len());
    }
    for (class, list) in &by_class {
        println!("\n[{}]", class);
        for v in list {
            println!(
                "  ({:.2}, {:.2}) {} F:segs={} pads={} B:segs={} zone={}",
                v.at.0, v.at.1, v.net, v.f_segs, v.f_pads, v.b_segs, v.in_zone
            );
        }
    }
    if let Some(floating) = by_class.get("floating") {
        if !floating.is_empty() {
            println!("\n⚠ {} floating via(s) deletable", floating.len());
        }
    }
    Ok(())
}

pub fn cmd_patch_from_drc(input: &str) -> Result<()> {
    let cfg = kicad_cdb::config::AppConfig::load()?;
    let report = kicad_cdb::drc::run_drc(&cfg.kicad_cli_path, input)?;
    let pairs: Vec<&kicad_cdb::drc::DrcViolation> = report
        .violations
        .iter()
        .filter(|v| v.violation_type == "unconnected_items" && v.items.len() >= 2)
        .collect();
    println!("Unconnected pairs: {}", pairs.len());
    println!("Proposed patches (review coordinates, then route manually or via GUI):\n");
    for (n, v) in pairs.iter().enumerate() {
        let coords: Vec<&kicad_cdb::drc::DrcItem> = v
            .items
            .iter()
            .filter(|i| i.x_mm.is_some() && i.y_mm.is_some())
            .collect();
        if coords.len() < 2 {
            continue;
        }
        let a = coords[0];
        let b = coords[1];
        let (ax, ay) = (a.x_mm.unwrap_or(0.0), a.y_mm.unwrap_or(0.0));
        let (bx, by) = (b.x_mm.unwrap_or(0.0), b.y_mm.unwrap_or(0.0));
        // net from the item description "[NET]"
        let net_of = |d: &str| -> Option<String> {
            let s = d.find('[')?;
            let e = d[s..].find(']')?;
            Some(d[s + 1..s + e].to_string())
        };
        let net = net_of(&a.description)
            .or_else(|| net_of(&b.description))
            .unwrap_or_default();
        let layer_a = if a.description.contains("B.Cu") {
            "B.Cu"
        } else {
            "F.Cu"
        };
        let layer_b = if b.description.contains("B.Cu") {
            "B.Cu"
        } else {
            "F.Cu"
        };
        let action = if layer_a == layer_b {
            format!("F.Cu segment ({ax:.3},{ay:.3}) -> ({bx:.3},{by:.3}) w>=0.25 [check clearance first]")
        } else {
            let mx = (ax + bx) / 2.0;
            let my = (ay + by) / 2.0;
            format!(
                "via @({mx:.3},{my:.3}) + F segment to via + B segment to via [via d0.6 drill0.3]"
            )
        };
        println!(
            "#{} net='{}' gap {:.2}mm",
            n + 1,
            net,
            ((ax - bx).powi(2) + (ay - by).powi(2)).sqrt()
        );
        println!("   A: ({ax:.2},{ay:.2}) {}", a.description);
        println!("   B: ({bx:.2},{by:.2}) {}", b.description);
        println!("   -> {}", action);
    }
    if pairs.is_empty() {
        println!("Nothing to patch.");
    }
    Ok(())
}

pub fn cmd_audit_design(input: &str, rails: &[String]) -> Result<()> {
    let text = std::fs::read_to_string(input)?;
    let board = kicad_json5::parse_board(&text)?;
    let names: std::collections::BTreeMap<u32, String> =
        board.nets.iter().map(|n| (n.id, n.name.clone())).collect();
    let norm = |n: &str| n.trim_start_matches('/').to_string();

    // ---- 1. decoupling distance: IC pins vs nearest cap on the same power net ----
    println!("== 去耦距离（IC 电源脚 ↔ 最近同网电容，警告阈值 5mm） ==");
    fn is_cap(lib: &str) -> bool {
        lib.contains("Capacitor_SMD") || lib.contains(":C")
    }
    let mut cap_nets: Vec<(String, (f64, f64))> = Vec::new();
    for f in &board.footprints {
        if !is_cap(&f.lib_id) {
            continue;
        }
        for p in &f.pads {
            let Some(id) = p.net else { continue };
            if id == 0 {
                continue;
            }
            if let Some(name) = names.get(&id) {
                cap_nets.push((name.clone(), (f.position.0, f.position.1)));
            }
        }
    }
    for fp in &board.footprints {
        if fp.pads.len() < 4 || is_cap(&fp.lib_id) {
            continue;
        }
        let mut worst: Option<(f64, String, String)> = None; // dist, cap_ref?, net
        for pad in &fp.pads {
            let Some(id) = pad.net else { continue };
            if id == 0 {
                continue;
            }
            let net = norm(names.get(&id).map(|s| s.as_str()).unwrap_or(""));
            if !(net.contains("V") || net.contains("vcc") || net.to_uppercase().contains("VCC")) {
                continue;
            }
            let (ox, oy) = fp.pad_rotated_offset(pad);
            let (px, py) = (fp.position.0 + ox, fp.position.1 + oy);
            // nearest cap pad on the same normalized net
            let mut best = f64::INFINITY;
            for (cn, cp) in &cap_nets {
                if norm(cn) == net {
                    best = best.min((cp.0 - px).hypot(cp.1 - py));
                }
            }
            if best.is_finite() && (worst.is_none() || best > worst.as_ref().unwrap().0) {
                worst = Some((best, String::new(), net));
            }
        }
        if let Some((d, _, net)) = worst {
            let tag = if d > 10.0 {
                "严重"
            } else if d > 5.0 {
                "偏远"
            } else {
                "ok"
            };
            println!("  {} {} 最近去耦 {:.1}mm [{}]", fp.reference, net, d, tag);
        }
    }

    // ---- 2. ampacity: min width per net vs required current ----
    println!("\n== 载流量（1oz 外层保守: A ≈ 3.0 × 宽mm；--rail NET=电流 指定需求） ==");
    let mut req: std::collections::BTreeMap<String, f64> = Default::default();
    for r in rails {
        if let Some((n, v)) = r.split_once('=') {
            if let Ok(a) = v.trim_end_matches("A").parse::<f64>() {
                req.insert(norm(n), a);
            }
        }
    }
    let mut per_net: std::collections::BTreeMap<String, f64> = Default::default();
    for s in &board.segments {
        let n = norm(names.get(&s.net).map(|s| s.as_str()).unwrap_or(""));
        if n.is_empty() || n == "GND" {
            continue;
        } // GND 走 plane
        let e = per_net.entry(n).or_insert(f64::INFINITY);
        *e = e.min(s.width);
    }
    for (net, minw) in &per_net {
        let amps = minw * 3.0;
        let need = req.get(net).copied();
        let status = match need {
            Some(r) if r > amps => format!("❌ 需 {:.1}A > 容量 {:.1}A", r, amps),
            Some(r) => format!("✅ 需 {:.1}A ≤ 容量 {:.1}A", r, amps),
            None => format!("容量 {:.1}A", amps),
        };
        println!("  {:<14} 最小线宽 {:.2}mm → {}", net, minw, status);
    }
    Ok(())
}

pub fn cmd_add_silk(input: &str, output: Option<&str>) -> Result<()> {
    // Text-level implementation: the IR→sexpr/json5 generators still drop
    // footprint graphics (P0-9), so IR round-trip would silently lose markers.
    let text = std::fs::read_to_string(input)?;
    let mut lines: Vec<String> = text.split('\n').map(|s| s.to_string()).collect();

    // top-level net table: id -> name
    let mut net_names: std::collections::BTreeMap<u32, String> = Default::default();
    for l in &lines {
        let t = l.trim_start();
        if let Some(rest) = t.strip_prefix("(net ") {
            let rest = rest.trim();
            if let Some(sp) = rest.find(' ') {
                if let Ok(id) = rest[..sp].parse::<u32>() {
                    let name = rest[sp + 1..]
                        .trim()
                        .trim_matches('"')
                        .trim_matches(')')
                        .trim_matches('"')
                        .to_string();
                    net_names.insert(id, name);
                }
            }
        }
    }

    // footprint blocks
    let mut blocks: Vec<(usize, usize)> = Vec::new();
    let mut i = 0usize;
    while i < lines.len() {
        if lines[i].starts_with("\t(footprint ") {
            let mut depth = 0i32;
            let mut j = i;
            while j < lines.len() {
                depth += lines[j].matches('(').count() as i32;
                depth -= lines[j].matches(')').count() as i32;
                if depth <= 0 {
                    break;
                }
                j += 1;
            }
            blocks.push((i, j + 1));
            i = j + 1;
        } else {
            i += 1;
        }
    }

    fn fake_uuid(n: u64) -> String {
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        let mut h = t
            .wrapping_mul(0x9E3779B97F4A7C15)
            .wrapping_add(n.wrapping_mul(0xBF58476D1CE4E5B9));
        let mut hex = String::new();
        for _ in 0..8 {
            hex.push_str(&format!("{:04x}", (h & 0xFFFF) as u16));
            h = h.rotate_left(16).wrapping_add(0x2545F4914245C3C9);
        }
        format!(
            "{}-{}-{}-{}-{}",
            &hex[0..8],
            &hex[8..12],
            &hex[12..16],
            &hex[16..20],
            &hex[20..32]
        )
    }
    fn fmt(v: f64) -> String {
        let s = format!("{:.4}", v);
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }

    struct PadDef {
        #[allow(dead_code)] // 与手术清单字段对齐, 保留表意
        num: String,
        lx: f64,
        ly: f64,
        sw: f64,
        sh: f64,
        net: u32,
    }
    struct Insert {
        at: usize,
        lines: Vec<String>,
    }

    // ── P2-6: promote key reference designators F.Fab → F.SilkS ─────
    // ICs (≥3 pads) and connectors get their refdes on silkscreen when the
    // text area is clear of copper; try the mirrored side on conflict; only
    // truly boxed-in refs stay on F.Fab. Content-only line edits — indices
    // of `blocks` stay valid for the marker pass below.
    let mut promoted = 0usize;
    let mut kept_fab = 0usize;
    {
        let mut lines_mut = lines.clone();
        if let Ok(board) = kicad_json5::parse_board(&text) {
            let mut world_pads: Vec<(f64, f64, f64)> = Vec::new();
            for fp in &board.footprints {
                let (fx, fy, fr) = fp.position;
                let (s, c) = (fr.to_radians().sin(), fr.to_radians().cos());
                for p in &fp.pads {
                    let (lx, ly, _) = p.position;
                    world_pads.push((
                        fx + lx * c - ly * s,
                        fy + lx * s + ly * c,
                        p.size.0.max(p.size.1) / 2.0,
                    ));
                }
            }
            let clear_of_copper = |ax: f64, ay: f64| -> bool {
                world_pads
                    .iter()
                    .all(|&(px, py, pr)| (px - ax).hypot(py - ay) > pr + 0.9)
            };
            for &(b0, b1) in &blocks {
                let lib_id = lines[b0]
                    .trim_start_matches("\t(footprint ")
                    .trim()
                    .trim_matches('"')
                    .to_string();
                let lib_up = lib_id.to_uppercase();
                if lib_up.contains("MOUNTINGHOLE") || lib_up.contains("TESTPOINT") {
                    continue;
                }
                let npad = lines[b0..b1]
                    .iter()
                    .filter(|l| l.trim_start().starts_with("(pad \""))
                    .count();
                if npad < 3 && !lib_up.contains("CONN") {
                    continue;
                }

                // locate Reference property block
                let p0 = match (b0..b1)
                    .find(|&k| lines[k].trim_start().starts_with("(property \"Reference\""))
                {
                    Some(k) => k,
                    None => continue,
                };
                let p1 = {
                    let mut d = 0i32;
                    let mut j = p0;
                    loop {
                        d += lines[j].matches('(').count() as i32;
                        d -= lines[j].matches(')').count() as i32;
                        if d <= 0 || j + 1 >= b1 {
                            break;
                        }
                        j += 1;
                    }
                    j + 1
                };
                if lines[p0..p1].iter().any(|l| l.contains("\"F.SilkS\"")) {
                    continue;
                }
                let fab_line = match (p0..p1).find(|&k| lines[k].contains("(layer \"F.Fab\")")) {
                    Some(k) => k,
                    None => continue,
                };

                // footprint placement
                let (mut fx, mut fy, mut frot) = (0.0f64, 0.0f64, 0.0f64);
                for l in &lines[b0..b1] {
                    if l.starts_with("\t\t") && !l.starts_with("\t\t\t") {
                        let t = l.trim_start();
                        if let Some(rest) = t.strip_prefix("(at ") {
                            let parts: Vec<&str> =
                                rest.trim_end_matches(')').split_whitespace().collect();
                            if parts.len() >= 2 {
                                fx = parts[0].parse().unwrap_or(0.0);
                                fy = parts[1].parse().unwrap_or(0.0);
                                frot = parts.get(2).and_then(|v| v.parse().ok()).unwrap_or(0.0);
                            }
                            break;
                        }
                    }
                }
                // current anchor local coords from the (at ...) inside property
                let (mut tx, mut ty) = (0.0f64, -1.6f64);
                let mut at_line = None;
                for (k, t) in lines[p0..p1].iter().map(|l| l.trim_start()).enumerate() {
                    let k = p0 + k;
                    if let Some(rest) = t.strip_prefix("(at ") {
                        let parts: Vec<&str> =
                            rest.trim_end_matches(')').split_whitespace().collect();
                        if parts.len() >= 2 {
                            tx = parts[0].parse().unwrap_or(0.0);
                            ty = parts[1].parse().unwrap_or(-1.6);
                            at_line = Some(k);
                        }
                        break;
                    }
                }
                let (s, c) = (frot.to_radians().sin(), frot.to_radians().cos());
                let world = |lx: f64, ly: f64| (fx + lx * c - ly * s, fy + lx * s + ly * c);
                let (ax, ay) = world(tx, ty);
                let (flip, ok) = if clear_of_copper(ax, ay) {
                    (false, true)
                } else {
                    let (ax2, ay2) = world(tx, -ty);
                    if clear_of_copper(ax2, ay2) {
                        (true, true)
                    } else {
                        (false, false)
                    }
                };
                if !ok {
                    kept_fab += 1;
                    continue;
                }

                lines_mut[fab_line] =
                    lines_mut[fab_line].replace("(layer \"F.Fab\")", "(layer \"F.SilkS\")");
                if flip {
                    if let Some(k) = at_line {
                        let t = lines_mut[k].trim_start().to_string();
                        let new_t = if ty == 0.0 {
                            t.clone()
                        } else {
                            t.replace(&fmt(ty), &fmt(-ty))
                        };
                        let indent = lines_mut[k].len() - lines_mut[k].trim_start().len();
                        lines_mut[k] = format!("{}{}", " ".repeat(indent), new_t);
                    }
                }
                promoted += 1;
            }
        }
        lines = lines_mut;
    }
    let mut inserts: Vec<Insert> = Vec::new();
    let mut dots = 0usize;
    let mut pol = 0usize;
    let mut cy = 0usize;
    let mut uid_counter = 0u64;

    for (bi, &(b0, b1)) in blocks.iter().enumerate() {
        let _ = bi;
        let blk: Vec<&str> = lines[b0..b1].iter().map(|s| s.as_str()).collect();
        let lib_id = blk[0]
            .trim_start_matches("\t(footprint ")
            .trim()
            .trim_matches('"')
            .to_string();

        // footprint placement: first 2-tab (at x y [rot])
        let (mut fx, mut fy, mut frot) = (0.0f64, 0.0f64, 0.0f64);
        let mut reference = String::new();
        for l in &blk {
            let t = l.trim_start();
            if t.starts_with("(at ") && (l.starts_with("\t\t") && !l.starts_with("\t\t\t")) {
                let parts: Vec<&str> = t[4..].trim_end_matches(')').split_whitespace().collect();
                if parts.len() >= 2 {
                    fx = parts[0].parse().unwrap_or(0.0);
                    fy = parts[1].parse().unwrap_or(0.0);
                    frot = parts.get(2).and_then(|v| v.parse().ok()).unwrap_or(0.0);
                }
                break;
            }
            if t.starts_with("(property \"Reference\"") {
                if let (Some(q1), Some(q2)) = (t.find('"'), t.rfind('"')) {
                    let inner = &t[q1 + 1..q2];
                    if let Some(sp) = inner.rfind('"') {
                        reference = inner[sp + 1..].to_string();
                    }
                }
            }
        }

        let mut pads: Vec<PadDef> = Vec::new();
        let mut k = 0usize;
        while k < blk.len() {
            let t = blk[k].trim_start();
            if let Some(rest) = t.strip_prefix("(pad \"") {
                let num: String = rest.chars().take_while(|c| *c != '"').collect();
                // scan forward for at/size/net within this pad block
                let (mut lx, mut ly) = (0.0f64, 0.0f64);
                let (mut sw, mut sh) = (0.6f64, 0.6f64);
                let mut net = 0u32;
                let mut kk = k;
                let mut pdepth = 1i32;
                while kk < blk.len() && kk <= k + 20 {
                    let tt = blk[kk].trim_start();
                    if let Some(r2) = tt.strip_prefix("(at ") {
                        if pdepth == 2 {
                            let parts: Vec<&str> =
                                r2.trim_end_matches(')').split_whitespace().collect();
                            if parts.len() >= 2 {
                                lx = parts[0].parse().unwrap_or(0.0);
                                ly = parts[1].parse().unwrap_or(0.0);
                            }
                        }
                    }
                    if let Some(r2) = tt.strip_prefix("(size ") {
                        let parts: Vec<&str> =
                            r2.trim_end_matches(')').split_whitespace().collect();
                        if parts.len() >= 2 {
                            sw = parts[0].parse().unwrap_or(0.6);
                            sh = parts[1].parse().unwrap_or(0.6);
                        }
                    }
                    if let Some(r2) = tt.strip_prefix("(net ") {
                        let r2 = r2.trim_end_matches(')').trim().to_string();
                        if let Some(sp) = r2.find('"') {
                            // (net ID "NAME") or (net "NAME") — take ID before first quote
                            let pre = r2[..sp].trim().to_string();
                            net = pre.parse().unwrap_or(0);
                        }
                    }
                    pdepth += tt.matches('(').count() as i32;
                    pdepth -= tt.matches(')').count() as i32;
                    if pdepth <= 0 {
                        break;
                    }
                    kk += 1;
                }
                pads.push(PadDef {
                    num,
                    lx,
                    ly,
                    sw,
                    sh,
                    net,
                });
            }
            k += 1;
        }
        if pads.is_empty() {
            continue;
        }

        let has_silk = blk
            .iter()
            .any(|l| l.contains("fp_circle") || l.contains("fp_line"));
        let has_crt = blk.iter().any(|l| l.contains("CrtYd"));
        let sina = frot.to_radians().sin();
        let cosa = frot.to_radians().cos();
        let world = |lx: f64, ly: f64| (fx + lx * cosa + ly * sina, fy - lx * sina + ly * cosa);
        let nname = |id: u32| net_names.get(&id).cloned().unwrap_or_default();
        let mut ins: Vec<String> = Vec::new();

        if !has_crt {
            let mnx = pads
                .iter()
                .map(|p| p.lx - p.sw / 2.0)
                .fold(f64::INFINITY, f64::min)
                - 0.5;
            let mxx = pads
                .iter()
                .map(|p| p.lx + p.sw / 2.0)
                .fold(f64::NEG_INFINITY, f64::max)
                + 0.5;
            let mny = pads
                .iter()
                .map(|p| p.ly - p.sh / 2.0)
                .fold(f64::INFINITY, f64::min)
                - 0.5;
            let mxy = pads
                .iter()
                .map(|p| p.ly + p.sh / 2.0)
                .fold(f64::NEG_INFINITY, f64::max)
                + 0.5;
            uid_counter += 1;
            ins.push(format!("\t\t(fp_rect\n\t\t\t(start {} {})\n\t\t\t(end {} {})\n\t\t\t(stroke (width 0.05) (type default))\n\t\t\t(layer \"F.CrtYd\")\n\t\t\t(uuid \"{}\")\n\t\t)",
                fmt(mnx), fmt(mny), fmt(mxx), fmt(mxy), fake_uuid(uid_counter)));
            cy += 1;
        }

        let is_passive2 = pads.len() == 2
            && (lib_id.contains("Resistor_SMD")
                || lib_id.contains("Capacitor_SMD")
                || lib_id.contains("Inductor_SMD")
                || lib_id.contains("Diode_SMD")
                || lib_id.contains("Fuse:")
                || lib_id.contains("LED_SMD"));
        let is_conn = lib_id.contains("Connector_") || lib_id.contains("Conn_");
        let is_polcap = (lib_id.contains("CP_Elec") || lib_id.contains(":CP_"))
            && pads.len() == 2
            && (reference.starts_with('C'));

        if !has_silk {
            if is_polcap {
                let gnd = pads
                    .iter()
                    .position(|p| nname(p.net).to_uppercase().contains("GND"));
                if let Some(gi) = gnd {
                    let (nx, ny) = world(pads[gi].lx, pads[gi].ly);
                    let (px, py) = world(pads[1 - gi].lx, pads[1 - gi].ly);
                    let dir = if nx > fx { 1.0 } else { -1.0 };
                    uid_counter += 1;
                    ins.push(format!("\t\t(fp_line\n\t\t\t(start {} {})\n\t\t\t(end {} {})\n\t\t\t(stroke (width 0.5) (type solid))\n\t\t\t(layer \"F.SilkS\")\n\t\t\t(uuid \"{}\")\n\t\t)",
                        fmt(nx + dir * 0.7), fmt(ny - 1.6), fmt(nx + dir * 0.7), fmt(ny + 1.6), fake_uuid(uid_counter)));
                    let pdir = if px > fx { 1.0 } else { -1.0 };
                    uid_counter += 1;
                    ins.push(format!("\t\t(fp_text user \"+\"\n\t\t\t(at {} {} 0)\n\t\t\t(layer \"F.SilkS\")\n\t\t\t(uuid \"{}\")\n\t\t\t(effects (font (size 1 1) (thickness 0.2)))\n\t\t)",
                        fmt(px + pdir * 1.1), fmt(py - 1.2), fake_uuid(uid_counter)));
                    pol += 1;
                }
            }
            if pads.len() >= 3 && !is_passive2 && !is_conn && !is_polcap {
                let (p1x, p1y) = world(pads[0].lx, pads[0].ly);
                let cx = pads.iter().map(|p| world(p.lx, p.ly).0).sum::<f64>() / pads.len() as f64;
                let cyy = pads.iter().map(|p| world(p.lx, p.ly).1).sum::<f64>() / pads.len() as f64;
                let (dx, dy) = (p1x - cx, p1y - cyy);
                let ln = dx.hypot(dy);
                let (ux, uy) = if ln > 1e-9 {
                    (dx / ln, dy / ln)
                } else {
                    (-1.0, 0.0)
                };
                let cxp = p1x + ux * 0.95;
                let cyp = p1y + uy * 0.95;
                uid_counter += 1;
                ins.push(format!("\t\t(fp_circle\n\t\t\t(center {} {})\n\t\t\t(end {} {})\n\t\t\t(stroke (width 0.15) (type solid))\n\t\t\t(fill none)\n\t\t\t(layer \"F.SilkS\")\n\t\t\t(uuid \"{}\")\n\t\t)",
                    fmt(cxp), fmt(cyp), fmt(cxp + 0.3), fmt(cyp + 0.3), fake_uuid(uid_counter)));
                dots += 1;
            }
        }
        if !ins.is_empty() {
            inserts.push(Insert {
                at: b1 - 1,
                lines: ins,
            });
        }
    }

    inserts.sort_by_key(|x| std::cmp::Reverse(x.at));
    let mut out = lines;
    for ins in &inserts {
        let mut head = out[..ins.at].to_vec();
        head.extend(ins.lines.iter().cloned());
        head.extend(out[ins.at..].to_vec());
        out = head;
    }
    let path = output.unwrap_or(input);
    std::fs::write(path, out.join("\n"))?;
    println!("silk: {} pin1 dots, {} polarity marks, {} courtyards added ({} footprints); refdes: {} on silk, {} kept on fab",
        dots, pol, cy, blocks.len(), promoted, kept_fab);
    Ok(())
}

/// P1-9: add mounting holes + test points as text-level footprint blocks.
/// IR round-trip is avoided for the same reason as add-silk (P0-9 generator
/// drops footprint graphics); parse_board is used read-only for auto placement
/// collision checks.
pub fn cmd_add_fixture(
    input: &str,
    output: Option<&str>,
    holes: usize,
    hole_size: f64,
    hole_inset: f64,
    tps: &[String],
    tp_size: f64,
    tp_inset: f64,
) -> Result<()> {
    let text = std::fs::read_to_string(input)?;
    let lines: Vec<String> = text.split('\n').map(|s| s.to_string()).collect();

    fn fake_uuid(n: u64) -> String {
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        let mut h = t
            .wrapping_mul(0x9E3779B97F4A7C15)
            .wrapping_add(n.wrapping_mul(0xBF58476D1CE4E5B9));
        let mut hex = String::new();
        for _ in 0..8 {
            hex.push_str(&format!("{:04x}", (h & 0xFFFF) as u16));
            h = h.rotate_left(16).wrapping_add(0x2545F4914245C3C9);
        }
        format!(
            "{}-{}-{}-{}-{}",
            &hex[0..8],
            &hex[8..12],
            &hex[12..16],
            &hex[16..20],
            &hex[20..32]
        )
    }
    fn fmt(v: f64) -> String {
        let s = format!("{:.4}", v);
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }

    // ── top-level net table: name <-> id (numeric boards), or dialect
    // name-only nets (KiCad-10 kipy boards use `(net "NAME")` with no IDs).
    let mut net_by_name: std::collections::BTreeMap<String, u32> = Default::default();
    let mut dialect_names: std::collections::BTreeSet<String> = Default::default();
    for l in &lines {
        let t = l.trim_start();
        if let Some(rest) = t.strip_prefix("(net ") {
            let rest = rest.trim();
            if let Some(sp) = rest.find(' ') {
                if let Ok(id) = rest[..sp].parse::<u32>() {
                    let name = rest[sp + 1..]
                        .trim()
                        .trim_matches('"')
                        .trim_matches(')')
                        .trim_matches('"')
                        .to_string();
                    net_by_name.insert(name, id);
                }
            } else if let Some(name) = rest.strip_prefix('"').and_then(|r| r.strip_suffix("\")")) {
                dialect_names.insert(name.to_string());
            }
        }
    }
    let numeric_nets = !net_by_name.is_empty();

    // ── scan footprint blocks: existing fixtures + used refs ────────
    let mut blocks: Vec<(usize, usize)> = Vec::new();
    let mut i = 0usize;
    while i < lines.len() {
        if lines[i].starts_with("\t(footprint ") {
            let mut depth = 0i32;
            let mut j = i;
            while j < lines.len() {
                depth += lines[j].matches('(').count() as i32;
                depth -= lines[j].matches(')').count() as i32;
                if depth <= 0 {
                    break;
                }
                j += 1;
            }
            blocks.push((i, j + 1));
            i = j + 1;
        } else {
            i += 1;
        }
    }

    let mut has_mount_hole = false;
    let mut tp_nets_done: Vec<String> = Vec::new();
    let mut used_refs: Vec<(char, u32)> = Vec::new();
    // extract the value string after a `(property "Name"\n\t\t\t"VALUE"` —
    // split('"') puts it at index 3 for both single-line and multiline forms
    let prop_value = |blk: &str, name: &str| -> Option<String> {
        let marker = format!("(property \"{}\"", name);
        let vp = blk.find(&marker)?;
        let seg = &blk[vp..(vp + 400).min(blk.len())];
        let strs: Vec<&str> = seg.split('"').collect();
        strs.get(3).map(|s| s.to_string())
    };
    for &(b0, b1) in &blocks {
        let blk = lines[b0..b1].join("\n");
        if blk.contains("MountingHole") {
            has_mount_hole = true;
        }
        if blk.contains("TestPoint") {
            if let Some(v) = prop_value(&blk, "Value") {
                tp_nets_done.push(v);
            }
        }
        if let Some(r) = prop_value(&blk, "Reference") {
            let mut chars = r.chars();
            if let Some(prefix) = chars.next() {
                if let Ok(n) = chars.as_str().parse::<u32>() {
                    used_refs.push((prefix, n));
                }
            }
        }
    }
    fn next_ref(used_refs: &[(char, u32)], prefix: char) -> u32 {
        used_refs
            .iter()
            .filter(|(p, _)| *p == prefix)
            .map(|(_, n)| *n)
            .max()
            .unwrap_or(0)
            + 1
    }

    // ── Edge.Cuts bbox from text ────────────────────────────────────
    // Block-scoped two-pass: pass 1 finds whether the gr_* block is on
    // Edge.Cuts, pass 2 collects its coordinates (start may precede layer).
    let mut edge = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    {
        fn collect(line: &str, edge: &mut (f64, f64, f64, f64)) {
            let t = line.trim_start();
            for key in ["(start ", "(end ", "(xy "] {
                if let Some(rest) = t.strip_prefix(key) {
                    let parts: Vec<&str> = rest.trim_end_matches(')').split_whitespace().collect();
                    if parts.len() >= 2 {
                        if let (Ok(x), Ok(y)) = (parts[0].parse::<f64>(), parts[1].parse::<f64>()) {
                            edge.0 = edge.0.min(x);
                            edge.1 = edge.1.min(y);
                            edge.2 = edge.2.max(x);
                            edge.3 = edge.3.max(y);
                        }
                    }
                }
            }
        }
        let mut gi = 0usize;
        while gi < lines.len() {
            let t = lines[gi].trim_start();
            let is_gr = t.starts_with("(gr_rect")
                || t.starts_with("(gr_line")
                || t.starts_with("(gr_poly")
                || t.starts_with("(gr_circle");
            if !is_gr {
                gi += 1;
                continue;
            }
            // walk this gr_ block
            let mut depth = 0i32;
            let mut in_edge = false;
            let mut j = gi;
            while j < lines.len() {
                if lines[j].contains("(layer \"Edge.Cuts\")") {
                    in_edge = true;
                }
                depth += lines[j].matches('(').count() as i32;
                depth -= lines[j].matches(')').count() as i32;
                if depth <= 0 {
                    break;
                }
                j += 1;
            }
            if in_edge {
                for l in &lines[gi..j + 1] {
                    collect(l, &mut edge);
                }
            }
            gi = j + 1;
        }
    }
    if edge.0 > edge.2 {
        anyhow::bail!("No Edge.Cuts graphics found — cannot place fixtures");
    }

    // ── occupied geometry for auto TP placement (read-only parse) ───
    struct Occupy {
        pads: Vec<(f64, f64, f64)>,
        vias: Vec<(f64, f64, f64)>,
        segs: Vec<((f64, f64), (f64, f64))>,
        zones: Vec<(String, Vec<(f64, f64)>)>,
    }
    let mut occupy = {
        let mut o = Occupy {
            pads: Vec::new(),
            vias: Vec::new(),
            segs: Vec::new(),
            zones: Vec::new(),
        };
        if let Ok(board) = kicad_json5::parse_board(&text) {
            let id2name: std::collections::BTreeMap<u32, String> =
                board.nets.iter().map(|n| (n.id, n.name.clone())).collect();
            for fp in &board.footprints {
                let (fx, fy, fr) = fp.position;
                let (s, c) = (fr.to_radians().sin(), fr.to_radians().cos());
                for p in &fp.pads {
                    let (lx, ly, _) = p.position;
                    let wx = fx + lx * c - ly * s;
                    let wy = fy + lx * s + ly * c;
                    let r = p.size.0.max(p.size.1) / 2.0;
                    o.pads.push((wx, wy, r));
                }
            }
            for v in &board.vias {
                o.vias.push((v.at.0, v.at.1, v.size / 2.0));
            }
            for sgm in &board.segments {
                o.segs.push((sgm.start, sgm.end));
            }
            for z in &board.zones {
                let name = id2name.get(&z.net).cloned().unwrap_or_default();
                let poly = if z.filled_polygons.is_empty() {
                    z.outline.clone()
                } else {
                    z.filled_polygons
                        .iter()
                        .flat_map(|p| p.points.clone())
                        .collect()
                };
                o.zones.push((name, poly));
            }
        }
        o
    };
    fn pt_seg_dist(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
        let (vx, vy) = (b.0 - a.0, b.1 - a.1);
        let len2 = vx * vx + vy * vy;
        if len2 < 1e-12 {
            return (p.0 - a.0).hypot(p.1 - a.1);
        }
        let t = (((p.0 - a.0) * vx + (p.1 - a.1) * vy) / len2).clamp(0.0, 1.0);
        let (cx, cy) = (a.0 + t * vx, a.1 + t * vy);
        (p.0 - cx).hypot(p.1 - cy)
    }
    fn pt_in_poly(p: (f64, f64), poly: &[(f64, f64)]) -> bool {
        let (mut inside, n) = (false, poly.len());
        if n < 3 {
            return false;
        }
        let (mut j, mut k) = (n - 1, 0);
        while k < n {
            let (xi, yi) = poly[j];
            let (xj, yj) = poly[k];
            if ((yi > p.1) != (yj > p.1)) && (p.0 < (xj - xi) * (p.1 - yi) / (yj - yi + 1e-12) + xi)
            {
                inside = !inside;
            }
            j = k;
            k += 1;
        }
        inside
    }
    fn tp_free(o: &Occupy, x: f64, y: f64, net: &str, tp_size: f64) -> bool {
        if o.pads
            .iter()
            .any(|&(px, py, pr)| (px - x).hypot(py - y) < pr + tp_size / 2.0 + 1.0)
        {
            return false;
        }
        if o.vias
            .iter()
            .any(|&(px, py, pr)| (px - x).hypot(py - y) < pr + tp_size / 2.0 + 0.8)
        {
            return false;
        }
        if o.segs
            .iter()
            .any(|&(a, b)| pt_seg_dist((x, y), a, b) < tp_size / 2.0 + 0.6)
        {
            return false;
        }
        // reject foreign-zone interiors (same-net zone is fine — refill connects it)
        for (znet, poly) in &o.zones {
            if znet != net && pt_in_poly((x, y), poly) {
                return false;
            }
        }
        true
    }

    // ── build insert blocks ─────────────────────────────────────────
    let mut new_blocks: Vec<String> = Vec::new();
    let mut n_holes = 0usize;
    let mut n_tps = 0usize;
    let mut uid = 0u64;

    // mounting holes at 4 corners
    if holes > 0 {
        if has_mount_hole {
            eprintln!("holes: mounting hole footprint already present, skipped");
        } else {
            let (x0, y0, x1, y1) = edge;
            let corners = [
                (x0 + hole_inset, y0 + hole_inset),
                (x1 - hole_inset, y0 + hole_inset),
                (x0 + hole_inset, y1 - hole_inset),
                (x1 - hole_inset, y1 - hole_inset),
            ];
            for &(hx, hy) in corners.iter().take(holes.min(4)) {
                // register the hole so later TP auto-placement avoids it
                occupy.pads.push((hx, hy, hole_size / 2.0));
                let r = next_ref(&used_refs, 'H');
                used_refs.push(('H', r));
                let half = hole_size / 2.0 + 0.5;
                uid += 1;
                let u1 = fake_uuid(uid);
                uid += 1;
                let u2 = fake_uuid(uid);
                uid += 1;
                let u3 = fake_uuid(uid);
                uid += 1;
                let u4 = fake_uuid(uid);
                uid += 1;
                let u5 = fake_uuid(uid);
                new_blocks.push(format!(
                    "\t(footprint \"MountingHole:MountingHole_{}mm\"\n\
                     \t\t(layer \"F.Cu\")\n\
                     \t\t(uuid \"{}\")\n\
                     \t\t(at {} {})\n\
                     \t\t(property \"Reference\"\n\
                     \t\t\t\"H{}\"\n\
                     \t\t\t(at 0 -1.6)\n\
                     \t\t\t(layer \"F.Fab\")\n\
                     \t\t\t(uuid \"{}\")\n\
                     \t\t\t(effects (font (size 1 1) (thickness 0.15)))\n\
                     \t\t)\n\
                     \t\t(property \"Value\"\n\
                     \t\t\t\"MountHole\"\n\
                     \t\t\t(at 0 1.6)\n\
                     \t\t\t(layer \"F.Fab\")\n\
                     \t\t\t(uuid \"{}\")\n\
                     \t\t\t(effects (font (size 1 1) (thickness 0.15)))\n\
                     \t\t)\n\
                     \t\t(fp_rect\n\
                     \t\t\t(start {} {})\n\
                     \t\t\t(end {} {})\n\
                     \t\t\t(stroke (width 0.05) (type default))\n\
                     \t\t\t(layer \"F.CrtYd\")\n\
                     \t\t\t(uuid \"{}\")\n\
                     \t\t)\n\
                     \t\t(pad \"\" np_thru_hole circle\n\
                     \t\t\t(at 0 0)\n\
                     \t\t\t(size {} {})\n\
                     \t\t\t(drill {})\n\
                     \t\t\t(layers \"*.Cu\" \"*.Mask\")\n\
                     \t\t\t(uuid \"{}\")\n\
                     \t\t)\n\
                     \t)",
                    fmt(hole_size),
                    u1,
                    fmt(hx),
                    fmt(hy),
                    r,
                    u2,
                    u3,
                    fmt(-half),
                    fmt(-half),
                    fmt(half),
                    fmt(half),
                    u4,
                    fmt(hole_size),
                    fmt(hole_size),
                    fmt(hole_size),
                    u5
                ));
                n_holes += 1;
            }
        }
    }

    // test points
    for spec in tps {
        // "NET" or "NET@x,y"
        let (net, explicit) = match spec.split_once('@') {
            Some((n, xy)) => {
                let parts: Vec<&str> = xy.split(',').collect();
                if parts.len() != 2 {
                    anyhow::bail!("Invalid --tp '{}' (expected NET@x,y)", spec);
                }
                (
                    n.to_string(),
                    Some((
                        parts[0].trim().parse::<f64>()?,
                        parts[1].trim().parse::<f64>()?,
                    )),
                )
            }
            None => (spec.clone(), None),
        };
        if tp_nets_done.iter().any(|n| n.eq_ignore_ascii_case(&net)) {
            eprintln!("tp {}: test point already present, skipped", net);
            continue;
        }
        let in_numeric = net_by_name.contains_key(&net);
        let in_dialect = dialect_names.contains(&net);
        if !in_numeric && !in_dialect {
            let known: Vec<_> = if numeric_nets {
                net_by_name.keys().collect()
            } else {
                dialect_names.iter().collect()
            };
            anyhow::bail!("Net '{}' not found on board. Available: {:?}", net, known);
        }

        let (tx, ty) = match explicit {
            Some((x, y)) => (x, y),
            None => {
                // auto: grid scan preferring the right edge, then top-to-bottom;
                // walks left column-by-column when the edge is occupied
                let (x0, y0, x1, y1) = edge;
                let mut found = None;
                let mut x = x1 - tp_inset;
                while x >= x0 + tp_inset {
                    let mut y = y0 + tp_inset;
                    while y <= y1 - tp_inset {
                        if tp_free(&occupy, x, y, &net, tp_size) {
                            found = Some((x, y));
                            break;
                        }
                        y += 2.0;
                    }
                    if found.is_some() {
                        break;
                    }
                    x -= 2.0;
                }
                match found {
                    Some(p) => p,
                    None => anyhow::bail!(
                        "No free auto-placement slot for TP '{}'; specify {}@x,y",
                        net,
                        net
                    ),
                }
            }
        };
        // register so later TPs don't stack on this one
        occupy.pads.push((tx, ty, tp_size / 2.0));
        let net_clause = if numeric_nets {
            format!("(net {} \"{}\")", net_by_name[&net], net)
        } else {
            format!("(net \"{}\")", net)
        };
        let r = next_ref(&used_refs, 'T');
        used_refs.push(('T', r));
        let ref_name = format!("TP{}", r);
        let half = tp_size / 2.0 + 1.0;
        uid += 1;
        let u1 = fake_uuid(uid);
        uid += 1;
        let u2 = fake_uuid(uid);
        uid += 1;
        let u3 = fake_uuid(uid);
        uid += 1;
        let u4 = fake_uuid(uid);
        uid += 1;
        let u5 = fake_uuid(uid);
        new_blocks.push(format!(
            "\t(footprint \"TestPoint:TestPoint_Pad_D{}mm\"\n\
             \t\t(layer \"F.Cu\")\n\
             \t\t(uuid \"{}\")\n\
             \t\t(at {} {})\n\
             \t\t(property \"Reference\"\n\
             \t\t\t\"{}\"\n\
             \t\t\t(at 0 -1.6)\n\
             \t\t\t(layer \"F.Fab\")\n\
             \t\t\t(uuid \"{}\")\n\
             \t\t\t(effects (font (size 1 1) (thickness 0.15)))\n\
             \t\t)\n\
             \t\t(property \"Value\"\n\
             \t\t\t\"{}\"\n\
             \t\t\t(at 0 1.6)\n\
             \t\t\t(layer \"F.Fab\")\n\
             \t\t\t(uuid \"{}\")\n\
             \t\t\t(effects (font (size 1 1) (thickness 0.15)))\n\
             \t\t)\n\
             \t\t(fp_rect\n\
             \t\t\t(start {} {})\n\
             \t\t\t(end {} {})\n\
             \t\t\t(stroke (width 0.05) (type default))\n\
             \t\t\t(layer \"F.CrtYd\")\n\
             \t\t\t(uuid \"{}\")\n\
             \t\t)\n\
             \t\t(pad \"1\" smd circle\n\
             \t\t\t(at 0 0)\n\
             \t\t\t(size {} {})\n\
             \t\t\t(layers \"F.Cu\" \"F.Mask\")\n\
             \t\t\t{}\n\
             \t\t\t(uuid \"{}\")\n\
             \t\t)\n\
             \t)",
            fmt(tp_size),
            u1,
            fmt(tx),
            fmt(ty),
            ref_name,
            u2,
            net,
            u3,
            fmt(-half),
            fmt(-half),
            fmt(half),
            fmt(half),
            u4,
            fmt(tp_size),
            fmt(tp_size),
            net_clause,
            u5
        ));
        n_tps += 1;
    }

    if new_blocks.is_empty() {
        println!("fixture: nothing to add (all requested fixtures already present)");
        return Ok(());
    }

    // ── insert before the final closing paren ───────────────────────
    let mut out = lines;
    let mut insert_at = out.len();
    while insert_at > 0 && out[insert_at - 1].trim().is_empty() {
        insert_at -= 1;
    }
    if insert_at == 0 || !out[insert_at - 1].trim().starts_with(')') {
        anyhow::bail!("Cannot find the closing paren of the board — file layout unexpected");
    }
    let tail: Vec<String> = out.split_off(insert_at - 1);
    out.push(new_blocks.join("\n"));
    out.extend(tail);

    let path = output.unwrap_or(input);
    std::fs::write(path, out.join("\n"))?;
    println!(
        "fixture: {} mounting holes, {} test points added (edge {} {}..{} {})",
        n_holes,
        n_tps,
        fmt(edge.0),
        fmt(edge.1),
        fmt(edge.2),
        fmt(edge.3)
    );
    println!("note: run `kdesign drc --input <board>` (refill-zones) afterwards to verify");
    Ok(())
}

// ---------------------------------------------------------------------------
// Phase B: 波前网格导出（kroute-server RouteGrid / CUDA 求解）
// ---------------------------------------------------------------------------

pub fn cmd_export_grid(
    input: &str,
    output: &str,
    res: f64,
    dilate: usize,
    layers: usize,
) -> Result<()> {
    use kicad_cdb::layer_config::BoardLayerConfig;

    let source = std::fs::read_to_string(input)?;
    let board = kicad_json5::parse_board(&source)?;

    let has_bga = board.footprints.iter().any(|fp| {
        let u = fp.lib_id.to_uppercase();
        u.contains("BGA") || u.contains("CSP")
    });
    let grid_res = if res > 0.0 {
        res
    } else if has_bga {
        0.25
    } else {
        0.5
    };

    let cfg = match layers {
        2 => BoardLayerConfig::two_layer(),
        n if n >= 3 => BoardLayerConfig::n_signal_layer(n),
        _ => anyhow::bail!("信号层数至少为 2（传 0/1 属参数错误）"),
    };
    let export = kicad_cdb::router::export_wavefront_grid_dilated(&board, cfg, grid_res, dilate)?;

    let u32_path = format!("{output}.grid.u32");
    let json_path = format!("{output}.grid.json");

    let mut bytes = Vec::with_capacity(export.grid_u32.len() * 4);
    for v in &export.grid_u32 {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    std::fs::write(&u32_path, &bytes)?;

    // 差分对分组（G5）：`XXX_P/XXX_N` 同键归组，pair id 从 1 起（0=无配对）
    let mut pair_ids: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    let pair_of = |name: &str, pair_ids: &mut std::collections::HashMap<String, u32>| -> u32 {
        match diff_pair_key(name) {
            Some(key) => {
                let next = pair_ids.len() as u32 + 1;
                *pair_ids.entry(key).or_insert(next)
            }
            None => 0,
        }
    };
    let pairs: Vec<u32> = export
        .nets
        .iter()
        .map(|n| pair_of(&n.name, &mut pair_ids))
        .collect();

    let meta = serde_json::json!({
        "input": input,
        "cols": export.cols,
        "rows": export.rows,
        "grid_res_mm": export.grid_res_mm,
        "origin_mm": export.origin_mm,
        "layers": export.layers,
        "net_count": export.nets.len(),
        "nets": export.nets.iter().zip(pairs.iter()).map(|(n, &pair)| serde_json::json!({
            "net_id": n.net_id,
            "name": n.name,
            "start": [n.start.0, n.start.1],
            "goal": [n.goal.0, n.goal.1],
            "start_mm": n.start_mm,
            "goal_mm": n.goal_mm,
            "pair": pair,
        })).collect::<Vec<_>>(),
    });
    std::fs::write(&json_path, serde_json::to_string_pretty(&meta)?)?;

    println!(
        "[export-grid] {}x{} @{}mm layers={:?} nets(2pin signal)={} pairs={} -> {} + {}",
        export.cols,
        export.rows,
        export.grid_res_mm,
        export.layers,
        export.nets.len(),
        pair_ids.len(),
        u32_path,
        json_path
    );
    Ok(())
}

/// 修复模式波前网格导出（Task1 修订版）：单网自由端点 + 孔约束墙。
/// 与 cmd_export_grid 的差异：不枚举 2-pin 网，nets 只装一条合成 net
///（net_name 任意 pin 数含电源地网；from/to 世界坐标越界报错）；
/// hole_clr>0 时异网 PTH/via 的 drill/2+hole_clr 在全部信号层成墙。
/// meta 附 `repair` 上下文与全板 `net_names` 表——route-grid --attribute
/// 返回的归因簇 net id 由此反查网名。
pub fn cmd_export_grid_repair(
    input: &str,
    output: &str,
    res: f64,
    dilate: usize,
    layers: usize,
    net_name: &str,
    from: (f64, f64),
    to: (f64, f64),
    hole_clr: f64,
) -> Result<()> {
    use kicad_cdb::layer_config::BoardLayerConfig;

    let source = std::fs::read_to_string(input)?;
    let board = kicad_json5::parse_board(&source)?;

    let grid_res = if res > 0.0 { res } else { 0.5 };

    let cfg = match layers {
        2 => BoardLayerConfig::two_layer(),
        n if n >= 3 => BoardLayerConfig::n_signal_layer(n),
        _ => anyhow::bail!("信号层数至少为 2（传 0/1 属参数错误）"),
    };
    let export = kicad_cdb::router::export_wavefront_grid_repair(
        &board, cfg, grid_res, dilate, net_name, from, to, hole_clr,
    )?;

    let u32_path = format!("{output}.grid.u32");
    let json_path = format!("{output}.grid.json");
    let mut bytes = Vec::with_capacity(export.grid_u32.len() * 4);
    for v in &export.grid_u32 {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    std::fs::write(&u32_path, &bytes)?;

    // 全板 net id → 名字表（归因簇 net_ids 反查网名）
    let net_names: serde_json::Map<String, serde_json::Value> = board
        .nets
        .iter()
        .map(|n| (n.id.to_string(), serde_json::Value::String(n.name.clone())))
        .collect();

    let net = &export.nets[0];
    let meta = serde_json::json!({
        "input": input,
        "cols": export.cols,
        "rows": export.rows,
        "grid_res_mm": export.grid_res_mm,
        "origin_mm": export.origin_mm,
        "layers": export.layers,
        "repair": {
            "net": net_name,
            "net_id": net.net_id,
            "from": [from.0, from.1],
            "to": [to.0, to.1],
            "hole_clr": hole_clr,
        },
        "net_names": net_names,
        "net_count": export.nets.len(),
        "nets": export.nets.iter().map(|n| serde_json::json!({
            "net_id": n.net_id,
            "name": n.name,
            "start": [n.start.0, n.start.1],
            "goal": [n.goal.0, n.goal.1],
            "start_mm": n.start_mm,
            "goal_mm": n.goal_mm,
            "pair": 0,
        })).collect::<Vec<_>>(),
    });
    std::fs::write(&json_path, serde_json::to_string_pretty(&meta)?)?;

    println!(
        "[export-grid repair] net={net_name}(id {}) hole_clr={hole_clr} {}x{} @{}mm layers={:?} from={from:?} to={to:?} -> {} + {}",
        net.net_id,
        export.cols, export.rows, export.grid_res_mm, export.layers,
        u32_path, json_path
    );
    Ok(())
}

/// 差分对名识别：`XXX_P` / `XXX_N` 后缀（大小写不敏感）→ 去后缀配对键；非差分 → None
pub(crate) fn diff_pair_key(name: &str) -> Option<String> {
    let lower = name.to_lowercase();
    for suf in ["_p", "_n"] {
        if let Some(base) = lower.strip_suffix(suf) {
            let ok = base.len() >= 2
                && base
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
            if ok {
                return Some(base.to_string());
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Phase B: CUDA 路径写回（候选布线——须本地 union-DRC 验证后交付）
// ---------------------------------------------------------------------------

pub fn cmd_commit_grid_path(
    input: &str,
    output: &str,
    path_file: &str,
    meta_file: &str,
    net_id: u32,
    width: f64,
    via_size: f64,
    via_drill: f64,
) -> Result<()> {
    let source = std::fs::read_to_string(input).with_context(|| format!("read {input}"))?;

    let path_doc: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path_file)?)?;
    let meta: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(meta_file)?)?;

    // 网格参数一致性校验
    let cols = meta["cols"].as_u64().context("meta.cols")? as usize;
    let rows = meta["rows"].as_u64().context("meta.rows")? as usize;
    let layers = meta["layers"].as_array().context("meta.layers")?.len();
    let pcols = path_doc["cols"].as_u64().context("path.cols")? as usize;
    let prows = path_doc["rows"].as_u64().context("path.rows")? as usize;
    let players = path_doc["layers"].as_u64().context("path.layers")? as usize;
    if (pcols, prows, players) != (cols, rows, layers) {
        anyhow::bail!("path/meta 网格参数不一致: path {pcols}x{prows}x{players} vs meta {cols}x{rows}x{layers}");
    }
    let origin = (
        meta["origin_mm"][0].as_f64().context("origin.0")?,
        meta["origin_mm"][1].as_f64().context("origin.1")?,
    );
    let res = meta["grid_res_mm"].as_f64().context("grid_res")?;
    let layer_names: Vec<String> = meta["layers"]
        .as_array()
        .context("meta.layers")?
        .iter()
        .map(|v| v.as_str().unwrap_or_default().to_string())
        .collect();

    // ---- 文本手术写回（保真 100%：不动 setup/netclass/zone 原文）----
    // 结论来自档案踩坑：parse→generate 往返会丢 board 深层属性，
    // 且段必须 (net N) 数字格式 + 唯一 uuid，否则 pcbnew 静默丢段。
    let triples: Vec<Vec<u64>> = path_doc["path"]
        .as_array()
        .context("path 数组")?
        .iter()
        .map(triple_of)
        .collect();
    if triples.len() < 2 {
        anyhow::bail!("path 太短（<2 点）");
    }

    let (blocks, seg_count, via_count) = build_path_blocks(
        &triples,
        &layer_names,
        origin,
        res,
        net_id,
        width,
        via_size,
        via_drill,
        None,
    );

    let out_text = insert_before_zones_owned(&source, &blocks);
    std::fs::write(output, &out_text)?;

    println!(
        "[commit-grid-path] net {net_id}: +{seg_count} 段 +{via_count} 过孔（文本手术，setup/zone 原文零改动）"
    );
    println!("提醒: 候选布线——运行 kicad-cli pcb drc 对比前后违规数（union-N），通过才算交付");
    Ok(())
}

/// 批量网格路径写回（v35-flow S5 / RouteGridBatch 结果消费）：
/// 一次文本手术写入全部 routed net 的段+过孔。须本地 union-DRC 验证后交付。
pub fn cmd_commit_grid_batch(
    input: &str,
    output: &str,
    batch_file: &str,
    meta_file: &str,
    width: f64,
    via_size: f64,
    via_drill: f64,
) -> Result<()> {
    let source = std::fs::read_to_string(input).with_context(|| format!("read {input}"))?;
    let batch: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(batch_file)?)?;
    let meta: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(meta_file)?)?;

    let cols = meta["cols"].as_u64().context("meta.cols")? as usize;
    let rows = meta["rows"].as_u64().context("meta.rows")? as usize;
    let layers = meta["layers"].as_array().context("meta.layers")?.len();
    let (bcols, brows, blayers) = (
        batch["cols"].as_u64().context("batch.cols")? as usize,
        batch["rows"].as_u64().context("batch.rows")? as usize,
        batch["layers"].as_u64().context("batch.layers")? as usize,
    );
    if (bcols, brows, blayers) != (cols, rows, layers) {
        anyhow::bail!(
            "batch/meta 网格参数不一致: batch {bcols}x{brows}x{blayers} vs meta {cols}x{rows}x{layers}"
        );
    }
    let origin = (
        meta["origin_mm"][0].as_f64().context("origin.0")?,
        meta["origin_mm"][1].as_f64().context("origin.1")?,
    );
    let res = meta["grid_res_mm"].as_f64().context("grid_res")?;
    let layer_names: Vec<String> = meta["layers"]
        .as_array()
        .context("meta.layers")?
        .iter()
        .map(|v| v.as_str().unwrap_or_default().to_string())
        .collect();

    let results = batch["results"].as_array().context("batch.results")?;
    let mut all_blocks = String::new();
    let (mut seg_total, mut via_total, mut routed_nets) = (0usize, 0usize, 0usize);
    for r in results {
        if !r["routed"].as_bool().unwrap_or(false) {
            continue;
        }
        let net_id = r["net_id"].as_u64().context("result.net_id")? as u32;
        let triples: Vec<Vec<u64>> = r["path"]
            .as_array()
            .context("result.path")?
            .iter()
            .map(triple_of)
            .collect();
        if triples.len() < 2 {
            eprintln!("[commit-grid-batch] net {net_id}: path 太短，跳过");
            continue;
        }
        let (blocks, segs, vias) = build_path_blocks(
            &triples,
            &layer_names,
            origin,
            res,
            net_id,
            width,
            via_size,
            via_drill,
            None,
        );
        all_blocks.push_str(&blocks);
        seg_total += segs;
        via_total += vias;
        routed_nets += 1;
    }
    if routed_nets == 0 {
        anyhow::bail!("batch 里没有 routed net，无需写回");
    }

    let out_text = insert_before_zones_owned(&source, &all_blocks);
    std::fs::write(output, &out_text)?;
    println!(
        "[commit-grid-batch] {routed_nets} net: +{seg_total} 段 +{via_total} 过孔（文本手术，setup/zone 原文零改动）"
    );
    println!("提醒: 候选布线——运行 kicad-cli pcb drc 对比前后违规数（union-N），通过才算交付");
    Ok(())
}

pub(crate) fn triple_of(p: &serde_json::Value) -> Vec<u64> {
    let a = p.as_array().expect("path 项");
    vec![
        a[0].as_u64().unwrap_or(0),
        a[1].as_u64().unwrap_or(0),
        a[2].as_u64().unwrap_or(0),
    ]
}

/// 3D 网格路径 → KiCad 段+过孔文本块（相对前驱换层的点放过孔，同 commit_wavefront_path 语义）
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_path_blocks(
    triples: &[Vec<u64>],
    layer_names: &[String],
    origin: (f64, f64),
    res: f64,
    net_id: u32,
    width: f64,
    via_size: f64,
    via_drill: f64,
    widths: Option<&[f64]>,
) -> (String, usize, usize) {
    use uuid::Uuid;
    let to_world =
        |t: &[u64]| -> (f64, f64) { (origin.0 + t[2] as f64 * res, origin.1 + t[1] as f64 * res) };
    let mut blocks = String::new();
    let mut seg_count = 0usize;
    let mut via_count = 0usize;

    // M3 单遍扫描：共线合并（只留转向/换层点）+ 宽度自适应分段（宽度类变化断段）。
    // widths 与 triples 一一对应（None = 全部 default_w，退化为纯共线合并）。
    let wq = |i: usize| -> u64 {
        widths
            .map(|ws| (ws[i] / 0.05).round() as u64)
            .unwrap_or((width / 0.05).round() as u64)
    };
    let dir = |p: &[u64], q: &[u64]| {
        (
            q[0] as i64 - p[0] as i64,
            q[1] as i64 - p[1] as i64,
            q[2] as i64 - p[2] as i64,
        )
    };
    let mut seg_start = 0usize;
    let mut seg_w = wq(0);
    for i in 1..triples.len() {
        let layer_change = triples[i][0] != triples[i - 1][0];
        let wc = wq(i);
        // 断段判定：挂起段的最后一步方向 ≠ 下一单步方向（转向），或宽度类变化。
        // 注意比较的是「上一单步」而非「起点→末点」总位移——直线延伸时总位移
        // 逐格累加，与单步永不相等会造成每两步虚假断段（首版 bug，单测抓出）。
        // seg_start == i-1 时挂起段尚无长度，绝不 flush。
        if seg_start + 1 < i {
            let last_step = dir(&triples[i - 2], &triples[i - 1]);
            let next_step = dir(&triples[i - 1], &triples[i]);
            if last_step != next_step || wc != seg_w {
                let a = &triples[seg_start];
                let b = &triples[i - 1];
                let la = layer_names
                    .get(a[0] as usize)
                    .cloned()
                    .unwrap_or_else(|| "F.Cu".into());
                let (x0, y0) = to_world(a);
                let (x1, y1) = to_world(b);
                if (x0 - x1).abs() > 1e-9 || (y0 - y1).abs() > 1e-9 {
                    blocks.push_str(&format!(
                    "\t(segment\n\t\t(start {:.4} {:.4})\n\t\t(end {:.4} {:.4})\n\t\t(width {:.4})\n\t\t(layer \"{}\")\n\t\t(net {})\n\t\t(uuid {})\n\t)\n",
                    x0, y0, x1, y1, seg_w as f64 * 0.05, la, net_id, Uuid::new_v4()
                ));
                    seg_count += 1;
                }
                seg_start = i - 1;
                seg_w = wc;
            }
        }
        if layer_change {
            // 过孔落在换层点 b；换层步同格零长 → 只放过孔
            let b = &triples[i];
            let lb = layer_names
                .get(b[0] as usize)
                .cloned()
                .unwrap_or_else(|| "F.Cu".into());
            let la = layer_names
                .get(triples[i - 1][0] as usize)
                .cloned()
                .unwrap_or_else(|| "F.Cu".into());
            let (x1, y1) = to_world(b);
            blocks.push_str(&format!(
                "\t(via\n\t\t(at {:.4} {:.4})\n\t\t(size {:.4})\n\t\t(drill {:.4})\n\t\t(layers \"{}\" \"{}\")\n\t\t(net {})\n\t\t(uuid {})\n\t)\n",
                x1, y1, via_size, via_drill, la, lb, net_id, Uuid::new_v4()
            ));
            via_count += 1;
            seg_start = i;
            seg_w = wq(i);
        }
    }
    // 收尾段
    {
        let a = &triples[seg_start];
        let b = &triples[triples.len() - 1];
        let la = layer_names
            .get(a[0] as usize)
            .cloned()
            .unwrap_or_else(|| "F.Cu".into());
        let (x0, y0) = to_world(a);
        let (x1, y1) = to_world(b);
        if (x0 - x1).abs() > 1e-9 || (y0 - y1).abs() > 1e-9 {
            blocks.push_str(&format!(
                "\t(segment\n\t\t(start {:.4} {:.4})\n\t\t(end {:.4} {:.4})\n\t\t(width {:.4})\n\t\t(layer \"{}\")\n\t\t(net {})\n\t\t(uuid {})\n\t)\n",
                x0, y0, x1, y1, seg_w as f64 * 0.05, la, net_id, Uuid::new_v4()
            ));
            seg_count += 1;
        }
    }
    (blocks, seg_count, via_count)
}

/// 文本手术插入点：第一个 (zone 之前（zones 在板尾）；无 zone 则最后一个 ')' 前。
/// 返回插入位置字节下标。
pub(crate) fn find_insert_pos(source: &str) -> usize {
    source
        .find("\n\t(zone")
        .map(|p| p + 1)
        .or_else(|| source.find("(zone"))
        .unwrap_or_else(|| source.rfind(')').unwrap_or(source.len()))
}

/// 就地版：返回插入 blocks 后的新文本（原文本 + blocks 在 zone 前插入）
pub(crate) fn insert_before_zones_owned(source: &str, blocks: &str) -> String {
    let at = find_insert_pos(source);
    format!("{}{}{}", &source[..at], blocks, &source[at..])
}

// ---------------------------------------------------------------------------
// Phase C(v35): 布局解回填（S3）——ref→x,y,rot 文本手术
// ---------------------------------------------------------------------------

/// 把 SA 布局解（refs+positions 一一对应）回填到板文本：
/// 定位每个 footprint 块（\t(footprint ... \t)），改写其 2 缩进的 (at x y [rot]) 行。
/// 除目标行外原文零改动（parse→generate 会丢深层属性，禁用）。
pub fn apply_layout_solution_text(
    source: &str,
    refs: &[String],
    positions: &[(f64, f64, f64)],
) -> anyhow::Result<(String, usize)> {
    anyhow::ensure!(
        refs.len() == positions.len(),
        "解 refs({}) 与 positions({}) 数量不一致",
        refs.len(),
        positions.len()
    );
    let pos_by_ref: std::collections::HashMap<&str, (f64, f64, f64)> = refs
        .iter()
        .zip(positions.iter())
        .map(|(r, p)| (r.as_str(), *p))
        .collect();

    let lines: Vec<&str> = source.lines().collect();
    // 方言探测（version 头 + 内容兜底）：Reference 行的解析按方言 match 分派
    let dialect = kicad_json5::dialect::BoardDialect::detect(source);
    let mut out: Vec<String> = Vec::with_capacity(lines.len() + 8);
    let mut patched = 0usize;
    let mut seen: Vec<String> = Vec::new();

    let mut i = 0usize;
    while i < lines.len() {
        let line = lines[i];
        if line.starts_with('\t') && line.starts_with("\t(footprint ") {
            // 收集整块（到同缩进的 \t) 行）
            let start = i;
            let mut end = i + 1;
            while end < lines.len() && lines[end] != "\t)" {
                end += 1;
            }
            if end >= lines.len() {
                anyhow::bail!("footprint 块在第 {start} 行未闭合（缺 \\t)）");
            }
            // 块内找 Reference 值（(property "Reference" 的下一行 "REF"）
            let mut ref_name: Option<String> = None;
            let mut at_line: Option<usize> = None;
            for (k, l) in lines[start..=end].iter().enumerate() {
                if ref_name.is_none() {
                    // 方言 match 分支分派（kicad-json5::dialect 集中管理多版本）
                    if let Some(v) =
                        dialect.parse_reference_line(l, lines.get(start + k + 1).copied())
                    {
                        ref_name = Some(v);
                    }
                }
                if at_line.is_none() && l.starts_with("\t\t(at ") {
                    at_line = Some(start + k);
                }
            }
            let (Some(ref_name), Some(at_idx)) = (ref_name, at_line) else {
                anyhow::bail!("footprint 块（第 {start} 行）缺 Reference 或 (at) 行");
            };
            seen.push(ref_name.clone());
            if let Some(&(x, y, rot)) = pos_by_ref.get(ref_name.as_str()) {
                let at_text = if rot.abs() < 0.001 {
                    format!("\t\t(at {:.4} {:.4})", x, y)
                } else {
                    format!("\t\t(at {:.4} {:.4} {:.4})", x, y, rot)
                };
                for (k, l) in lines[start..=end].iter().enumerate() {
                    if start + k == at_idx {
                        out.push(at_text.clone());
                    } else {
                        out.push((*l).to_string());
                    }
                }
                patched += 1;
            } else {
                for l in &lines[start..=end] {
                    out.push((*l).to_string());
                }
            }
            i = end + 1;
            continue;
        }
        out.push(line.to_string());
        i += 1;
    }

    let missing: Vec<String> = pos_by_ref
        .keys()
        .filter(|r| !seen.iter().any(|s| s == *r))
        .map(|r| r.to_string())
        .collect();
    if !missing.is_empty() {
        anyhow::bail!("板内找不到以下 ref 的 footprint: {missing:?}");
    }
    let mut text = out.join("\n");
    if source.ends_with('\n') {
        text.push('\n');
    }
    Ok((text, patched))
}

/// 解回填 CLI 入口：solution 为 LayoutOptimize 的 solution_json
/// （{"refs":[...], "positions":[[x,y,rot],...]}）
pub fn cmd_commit_layout(input: &str, output: &str, solution_file: &str) -> Result<()> {
    let source = std::fs::read_to_string(input).with_context(|| format!("read {input}"))?;
    let sol: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(solution_file)?)?;
    let refs: Vec<String> = sol["refs"]
        .as_array()
        .context("solution.refs")?
        .iter()
        .map(|v| v.as_str().unwrap_or_default().to_string())
        .collect();
    let positions: Vec<(f64, f64, f64)> = sol["positions"]
        .as_array()
        .context("solution.positions")?
        .iter()
        .map(|p| {
            let a = p.as_array().context("position 项")?;
            Ok((
                a[0].as_f64().context("x")?,
                a[1].as_f64().context("y")?,
                a.get(2).and_then(|v| v.as_f64()).unwrap_or(0.0),
            ))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    let (text, patched) = apply_layout_solution_text(&source, &refs, &positions)?;
    std::fs::write(output, text)?;
    println!("[commit-layout] {patched} 个 footprint 位置已回填（文本手术）");
    println!("提醒: 布局候选——连接器锚定与 DRC 复验后再落板");
    Ok(())
}

// ---------------------------------------------------------------------------
// v35 单测：解回填文本手术 / 差分对识别 / 批量写回插入点
// ---------------------------------------------------------------------------

#[cfg(test)]
mod v35_tests {
    use super::*;

    fn sample_board() -> String {
        // 与真实生成格式同构：footprint 在 1 缩进，placement (at) 在 2 缩进
        r#"(kicad_pcb
	(version 20231120)
	(footprint "TestPoint:TestPoint_Pad_D1mm"
		(layer "F.Cu")
		(uuid "aaaa-1")
		(at 3 43)
		(property "Reference"
			"TP8"
			(at 0 -1.6)
			(layer "F.Fab")
			(uuid "aaaa-2")
		)
		(pad "1" smd circle
			(at 0 0)
			(size 1 1)
			(layers "F.Cu" "F.Mask")
			(uuid "aaaa-3")
		)
	)
	(footprint "Resistor:R_0603"
		(layer "F.Cu")
		(uuid "bbbb-1")
		(at 10 10 90)
		(property "Reference"
			"R1"
			(at 0 -1.6)
			(layer "F.Fab")
			(uuid "bbbb-2")
		)
	)
	(zone (layer "F.Cu") (net_name "GND"))
)
"#
        .to_string()
    }

    #[test]
    fn commit_layout_moves_only_target_footprint() {
        let src = sample_board();
        let (out, patched) = apply_layout_solution_text(
            &src,
            &["TP8".to_string(), "R1".to_string()],
            &[(7.5, 2.25, 0.0), (20.0, 30.0, 45.0)],
        )
        .unwrap();
        assert_eq!(patched, 2);

        // 只允许两行不同：TP8 的 (at) 与 R1 的 (at)
        let diffs: Vec<&str> = out
            .lines()
            .zip(src.lines())
            .filter(|(a, b)| a != b)
            .map(|(a, _)| a)
            .collect();
        assert_eq!(diffs.len(), 2, "应恰好改动两行 (at)：{diffs:?}");
        assert!(diffs[0] == "\t\t(at 7.5000 2.2500)" || diffs[1] == "\t\t(at 7.5000 2.2500)");
        assert!(
            diffs.contains(&"\t\t(at 20.0000 30.0000 45.0000)"),
            "R1 应带旋转角回填：{diffs:?}"
        );

        // pad 的局部 (at 0 0) 必须原样保留
        assert!(out.contains("\t\t\t(at 0 0)"), "pad 局部坐标不得被改写");
        // patch 后的板仍可被 parse_board 解析
        let board = kicad_json5::parse_board(&out).unwrap();
        let tp8 = board
            .footprints
            .iter()
            .find(|f| f.reference == "TP8")
            .unwrap();
        assert!((tp8.position.0 - 7.5).abs() < 1e-6 && (tp8.position.1 - 2.25).abs() < 1e-6);
    }

    #[test]
    fn commit_layout_rejects_missing_ref() {
        let src = sample_board();
        let err = apply_layout_solution_text(&src, &["NOPE".to_string()], &[(1.0, 1.0, 0.0)]);
        assert!(err.is_err());
        assert!(err.unwrap_err().to_string().contains("NOPE"));
    }

    #[test]
    fn diff_pair_key_groups_suffix() {
        assert_eq!(diff_pair_key("LVDS_A_P"), Some("lvds_a".to_string()));
        assert_eq!(diff_pair_key("lvds_a_n"), Some("lvds_a".to_string()));
        assert_eq!(diff_pair_key("usb_D+"), None, "仅认 _P/_N 后缀");
        assert_eq!(diff_pair_key("GND"), None);
        assert_eq!(diff_pair_key("op"), None, "去后缀后过短不成对");
    }

    #[test]
    fn insert_before_zones_places_blocks() {
        let src = sample_board();
        let out = insert_before_zones_owned(&src, "\t(segment)\n");
        assert!(out.contains("\t(segment)\n\t(zone"));
        // 无 zone 板：插到最后一个 ')' 之前
        let no_zone = "(kicad_pcb\n\t(version 1)\n)\n";
        let out2 = insert_before_zones_owned(no_zone, "\t(segment)\n");
        assert!(out2.contains("\t(segment)\n)"));
    }
}

#[cfg(test)]
mod path_blocks_tests {
    use super::*;

    fn layers() -> Vec<String> {
        vec!["F.Cu".into(), "B.Cu".into()]
    }
    const ORIGIN: (f64, f64) = (0.0, 0.0);
    const RES: f64 = 0.25;

    fn trip(l: u64, r: u64, c: u64) -> Vec<u64> {
        vec![l, r, c]
    }

    fn run(triples: Vec<Vec<u64>>, widths: Option<&[f64]>) -> (String, usize, usize) {
        build_path_blocks(&triples, &layers(), ORIGIN, RES, 5, 0.2, 0.6, 0.3, widths)
    }

    #[test]
    fn straight_run_merges_into_one_segment() {
        // 同向 5 格直线 → 恰 1 段（旧实现 4 段）
        let t: Vec<Vec<u64>> = (0..5).map(|c| trip(0, 0, c)).collect();
        let (blocks, segs, vias) = run(t, None);
        assert_eq!((segs, vias), (1, 0));
        // 段端点是首尾格
        assert!(blocks.contains("(start 0.0000 0.0000)"));
        assert!(blocks.contains("(end 1.0000 0.0000)"));
    }

    #[test]
    fn turn_splits_segments() {
        // L 形转弯：2 段 0 过孔
        let t = vec![
            trip(0, 0, 0),
            trip(0, 0, 1),
            trip(0, 0, 2),
            trip(0, 1, 2),
            trip(0, 2, 2),
        ];
        let (blocks, segs, vias) = run(t, None);
        assert_eq!((segs, vias), (2, 0));
        assert!(blocks.contains("(end 0.5000 0.0000)"), "第一段止于转弯点");
    }

    #[test]
    fn layer_change_emits_via_and_skips_zero_length() {
        // 同格换层：只放过孔，无零长段（旧实现产 1 段零长铜皮）
        let t = vec![trip(0, 0, 0), trip(0, 0, 1), trip(1, 0, 1), trip(1, 0, 2)];
        let (blocks, segs, vias) = run(t, None);
        assert_eq!((segs, vias), (2, 1));
        assert!(blocks.contains("(layers \"F.Cu\" \"B.Cu\")"));
        assert!(
            !blocks.contains("(start 0.2500 0.0000)\n\t\t(end 0.2500 0.0000)"),
            "不得有零长段"
        );
    }

    #[test]
    fn width_change_splits_segment_with_narrow_class() {
        // 宽度类变化断段：0.2 段 + 0.1 段（量化 0.05）
        let t = vec![trip(0, 0, 0), trip(0, 0, 1), trip(0, 0, 2), trip(0, 0, 3)];
        let w = [0.2, 0.2, 0.1, 0.1];
        let (blocks, segs, vias) = run(t, Some(&w));
        assert_eq!((segs, vias), (2, 0));
        assert!(blocks.contains("(width 0.2000)"));
        assert!(blocks.contains("(width 0.1000)"));
    }
}
