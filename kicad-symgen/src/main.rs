use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::Path;

use kicad_symgen::input;
use kicad_symgen::model::*;
use kicad_symgen::footprint::outline;
use kicad_symgen::footprint::sexpr as fp_sexpr;
use kicad_symgen::footprint::templates;
use kicad_symgen::symbol::sexpr as sym_sexpr;
use kicad_symgen::lib_table;

#[derive(Parser)]
#[command(name = "symgen", about = "KiCad symbol and footprint generator for AI-assisted circuit design")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate a .kicad_sym symbol library file
    Symbol {
        /// SQLite database path (requires --mpn or --category)
        #[arg(long)]
        db: Option<String>,
        /// Component MPN to export (requires --db)
        #[arg(long)]
        mpn: Option<String>,
        /// Category to export all components (requires --db)
        #[arg(long)]
        category: Option<String>,
        /// JSON5 input file (alternative to --db)
        #[arg(long)]
        input: Option<String>,
        /// Output .kicad_sym file path
        #[arg(short, long)]
        output: String,
        /// Library name prefix (default: "custom")
        #[arg(long, default_value = "custom")]
        lib_name: String,
        /// KiCad version: 7, 8, 9, 10 (default: 8)
        #[arg(long, default_value = "8")]
        kicad_version: u8,
    },

    /// Generate a .kicad_mod footprint file
    Footprint {
        /// Package type: DIP-8, SOIC-8, TSSOP-20, SOT-23-3, etc.
        #[arg(long)]
        package: String,
        /// Pin pitch in mm (default depends on package)
        #[arg(long)]
        pitch: Option<f64>,
        /// Row spacing in mm (for DIP/dual-row packages)
        #[arg(long)]
        row_spacing: Option<f64>,
        /// Output .kicad_mod file path
        #[arg(short, long)]
        output: String,
        /// KiCad version: 7, 8 (default: 8)
        #[arg(long, default_value = "8")]
        kicad_version: u8,
    },

    /// Generate/update sym-lib-table and fp-lib-table
    LibTable {
        /// Directory containing library files
        #[arg(long)]
        dir: String,
    },

    /// Batch generate symbols and footprints from database
    Batch {
        /// SQLite database path
        #[arg(long)]
        db: String,
        /// Category to export
        #[arg(long)]
        category: String,
        /// Output directory
        #[arg(long)]
        output_dir: String,
        /// Library name prefix
        #[arg(long, default_value = "custom")]
        lib_name: String,
        /// Also generate footprints
        #[arg(long)]
        with_footprints: bool,
        /// KiCad version
        #[arg(long, default_value = "8")]
        kicad_version: u8,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Symbol {
            db, mpn, category, input, output, lib_name, kicad_version,
        } => cmd_symbol(db, mpn, category, input, &output, &lib_name, kicad_version),
        Commands::Footprint {
            package, pitch, row_spacing, output, kicad_version,
        } => cmd_footprint(&package, pitch, row_spacing, &output, kicad_version),
        Commands::LibTable { dir } => cmd_lib_table(&dir),
        Commands::Batch {
            db, category, output_dir, lib_name, with_footprints, kicad_version,
        } => cmd_batch(&db, &category, &output_dir, &lib_name, with_footprints, kicad_version),
    }
}

fn cmd_symbol(
    db: Option<String>,
    mpn: Option<String>,
    category: Option<String>,
    input_file: Option<String>,
    output: &str,
    lib_name: &str,
    kicad_version: u8,
) -> Result<()> {
    let version = KicadVersion::from_u8(kicad_version);
    let mut specs = Vec::new();

    match (input_file, &db, &mpn, &category) {
        // JSON5 input
        (Some(path), _, _, _) => {
            let spec = input::from_json5_file(&path)?;
            specs.push(spec);
        }
        // DB + MPN
        (_, Some(db_path), Some(mpn_val), _) => {
            let cdb = kicad_cdb::ComponentDb::open(db_path)?;
            let mut spec = input::from_database(&cdb, mpn_val)?;
            if lib_name != "custom" {
                spec.lib_name = lib_name.to_string();
            }
            specs.push(spec);
        }
        // DB + Category
        (_, Some(db_path), _, Some(cat)) => {
            let cdb = kicad_cdb::ComponentDb::open(db_path)?;
            let mut batch = input::from_database_category(&cdb, cat)?;
            if lib_name != "custom" {
                for spec in &mut batch {
                    spec.lib_name = lib_name.to_string();
                }
            }
            specs = batch;
        }
        _ => {
            anyhow::bail!("Specify --input <file> or --db <path> with --mpn or --category");
        }
    }

    if specs.is_empty() {
        anyhow::bail!("No components found to export");
    }

    let content = sym_sexpr::generate_symbol_lib(&specs, version);
    std::fs::write(output, &content)?;
    println!("Generated {} symbol(s) → {}", specs.len(), output);

    for spec in &specs {
        println!("  {} ({}, {} pins)", spec.mpn, spec.lib_id(), spec.pins.len());
    }

    Ok(())
}

fn cmd_footprint(
    package: &str,
    pitch: Option<f64>,
    row_spacing: Option<f64>,
    output: &str,
    kicad_version: u8,
) -> Result<()> {
    let version = KicadVersion::from_u8(kicad_version);

    // Parse package string: "DIP-8", "SOIC-16", "SOT-23-5", etc.
    let (pkg_type_str, pin_count) = parse_package_string(package)
        .context(format!("Cannot parse package '{}'. Examples: DIP-8, SOIC-16, SOT-23-5, TSSOP-20", package))?;

    let pkg_type = pkg_type_str;
    let default_pitch = default_pitch_for_package(&pkg_type);

    let spec = FootprintSpec {
        name: package.to_string(),
        package_type: pkg_type,
        pin_count,
        pitch: pitch.unwrap_or(default_pitch),
        row_spacing,
        options: FootprintOptions::default(),
    };

    let result = templates::generate_from_spec(&spec)
        .context(format!("No template available for package '{}' ({} pins)", package, pin_count))?;

    let (lines, arc) = if result.is_through_hole {
        outline::compute_dip_outlines(
            pin_count,
            spec.pitch,
            row_spacing.unwrap_or(7.62),
            0.5,
        )
    } else {
        // SMD: simple rectangular outline (placeholder)
        (vec![], None)
    };

    let content = fp_sexpr::generate_footprint(
        &result.name,
        &result.description,
        &result.tags,
        result.is_through_hole,
        &result.pads,
        &lines,
        arc.as_ref(),
        version,
    );

    std::fs::write(output, &content)?;
    println!("Generated footprint {} ({} pads) → {}", result.name, result.pads.len(), output);

    Ok(())
}

fn cmd_lib_table(dir: &str) -> Result<()> {
    let dir_path = Path::new(dir);
    let output_dir = dir_path;

    // Symbol libraries
    let sym_libs = lib_table::scan_sym_libraries(dir_path)?;
    if !sym_libs.is_empty() {
        let content = lib_table::generate_sym_lib_table(dir_path, &sym_libs);
        let out_path = output_dir.join("sym-lib-table");
        std::fs::write(&out_path, &content)?;
        println!("Generated sym-lib-table ({} libraries) → {}", sym_libs.len(), out_path.display());
        for (name, uri) in &sym_libs {
            println!("  {} → {}", name, uri);
        }
    }

    // Footprint libraries
    let fp_libs = lib_table::scan_fp_libraries(dir_path)?;
    if !fp_libs.is_empty() {
        let content = lib_table::generate_fp_lib_table(dir_path, &fp_libs);
        let out_path = output_dir.join("fp-lib-table");
        std::fs::write(&out_path, &content)?;
        println!("Generated fp-lib-table ({} libraries) → {}", fp_libs.len(), out_path.display());
        for (name, uri) in &fp_libs {
            println!("  {} → {}", name, uri);
        }
    }

    if sym_libs.is_empty() && fp_libs.is_empty() {
        println!("No libraries found in {}", dir);
    }

    Ok(())
}

fn cmd_batch(
    db_path: &str,
    category: &str,
    output_dir: &str,
    lib_name: &str,
    with_footprints: bool,
    kicad_version: u8,
) -> Result<()> {
    let version = KicadVersion::from_u8(kicad_version);
    let cdb = kicad_cdb::ComponentDb::open(db_path)?;

    let mut specs = input::from_database_category(&cdb, category)?;
    if lib_name != "custom" {
        for spec in &mut specs {
            spec.lib_name = lib_name.to_string();
        }
    }

    if specs.is_empty() {
        anyhow::bail!("No components found in category '{}'", category);
    }

    // Create output directory
    std::fs::create_dir_all(output_dir)?;

    // Generate symbol library
    let content = sym_sexpr::generate_symbol_lib(&specs, version);
    let sym_path = Path::new(output_dir).join(format!("{}.kicad_sym", lib_name));
    std::fs::write(&sym_path, &content)?;
    println!("Generated {} symbols → {}", specs.len(), sym_path.display());

    for spec in &specs {
        println!("  {} ({} pins)", spec.mpn, spec.pins.len());
    }

    // Generate footprints if requested
    if with_footprints {
        let fp_dir = Path::new(output_dir).join(format!("{}.pretty", lib_name));
        std::fs::create_dir_all(&fp_dir)?;

        let mut fp_count = 0;
        for spec in &specs {
            if let Some(ref pkg) = spec.package {
                if let Some((pkg_type, pin_count)) = parse_package_string(pkg).ok() {
                    let fp_spec = FootprintSpec {
                        name: pkg.clone(),
                        package_type: pkg_type,
                        pin_count,
                        pitch: default_pitch_for_package(&pkg_type),
                        row_spacing: None,
                        options: FootprintOptions::default(),
                    };

                    if let Some(result) = templates::generate_from_spec(&fp_spec) {
                        let fp_content = fp_sexpr::generate_footprint(
                            &result.name,
                            &result.description,
                            &result.tags,
                            result.is_through_hole,
                            &result.pads,
                            &[],
                            None,
                            version,
                        );
                        let fp_path = fp_dir.join(format!("{}.kicad_mod", result.name));
                        std::fs::write(&fp_path, &fp_content)?;
                        fp_count += 1;
                    }
                }
            }
        }
        if fp_count > 0 {
            println!("Generated {} footprints → {}", fp_count, fp_dir.display());
        }
    }

    Ok(())
}

/// Parse a package string like "DIP-8", "SOIC-16", "SOT-23-5" into (PackageType, pin_count)
fn parse_package_string(s: &str) -> Result<(PackageType, u32)> {
    let upper = s.to_uppercase().replace([' ', '_'], "-");

    // Try to extract pin count from suffix
    let pin_count = extract_pin_count(&upper)?;

    // Determine package type
    let pkg_type = if upper.starts_with("DIP-SOCKET") {
        PackageType::DipSocket
    } else if upper.starts_with("DIP") {
        PackageType::Dip
    } else if upper.starts_with("SIP") {
        PackageType::Sip
    } else if upper.starts_with("TSSOP") {
        PackageType::Tssop
    } else if upper.starts_with("SOIC") {
        PackageType::Soic
    } else if upper.starts_with("SOP") {
        PackageType::Sop
    } else if upper.starts_with("MSOP") {
        PackageType::MsoP
    } else if upper.starts_with("LQFP") {
        PackageType::Lqfp
    } else if upper.starts_with("TQFP") {
        PackageType::Tqfp
    } else if upper.starts_with("QFP") {
        PackageType::Qfp
    } else if upper.starts_with("QFN") {
        PackageType::Qfn
    } else if upper.starts_with("DFN") {
        PackageType::Dfn
    } else if upper.contains("SOT-23") || upper.contains("SOT23") {
        PackageType::Sot23
    } else if upper.contains("SOT-223") || upper.contains("SOT223") {
        PackageType::Sot223
    } else if upper.contains("SOT-89") || upper.contains("SOT89") {
        PackageType::Sot89
    } else if upper.starts_with("BGA") {
        PackageType::Bga
    } else if upper.contains("PIN") && upper.contains("HEADER") {
        PackageType::PinHeader
    } else {
        // Try the model's parser
        PackageType::from_package_str(s)
            .context(format!("Unknown package type: '{}'", s))?
    };

    Ok((pkg_type, pin_count))
}

fn extract_pin_count(s: &str) -> Result<u32> {
    // Look for trailing number: "DIP-8" → 8, "SOT-23-5" → 5
    let parts: Vec<&str> = s.split('-').collect();
    for part in parts.into_iter().rev() {
        if let Ok(n) = part.parse::<u32>() {
            return Ok(n);
        }
    }
    anyhow::bail!("No pin count found in '{}'", s)
}

fn default_pitch_for_package(pkg_type: &PackageType) -> f64 {
    match pkg_type {
        PackageType::Dip | PackageType::Sip | PackageType::DipSocket | PackageType::PinHeader => 2.54,
        PackageType::Soic | PackageType::Sop | PackageType::MsoP => 1.27,
        PackageType::Tssop => 0.65,
        PackageType::Qfp | PackageType::Lqfp | PackageType::Tqfp => 0.5,
        PackageType::Qfn | PackageType::Dfn => 0.5,
        _ => 1.27,
    }
}
