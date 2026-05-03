use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use kicad_symgen::input;
use kicad_symgen::model::*;
use kicad_symgen::footprint::outline;
use kicad_symgen::footprint::sexpr as fp_sexpr;
use kicad_symgen::footprint::templates;
use kicad_symgen::symbol::sexpr as sym_sexpr;
use kicad_symgen::lib_table;

#[derive(Parser)]
#[command(name = "symgen", about = "KiCad symbol and footprint generator")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate a .kicad_sym symbol library from a JSON5 spec file
    Symbol {
        /// JSON5 input file
        #[arg(long)]
        input: String,
        /// Output .kicad_sym file path
        #[arg(short, long)]
        output: String,
        /// Library name prefix (default: from spec)
        #[arg(long)]
        lib_name: Option<String>,
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
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Symbol { input, output, lib_name, kicad_version } => {
            cmd_symbol(&input, &output, lib_name.as_deref(), kicad_version)
        }
        Commands::Footprint { package, pitch, row_spacing, output, kicad_version } => {
            cmd_footprint(&package, pitch, row_spacing, &output, kicad_version)
        }
        Commands::LibTable { dir } => cmd_lib_table(&dir),
    }
}

fn cmd_symbol(
    input_file: &str,
    output: &str,
    lib_name: Option<&str>,
    kicad_version: u8,
) -> Result<()> {
    let version = KicadVersion::from_u8(kicad_version);
    let mut spec = input::from_json5_file(input_file)?;
    if let Some(name) = lib_name {
        spec.lib_name = name.to_string();
    }

    let content = sym_sexpr::generate_symbol_lib(&[spec.clone()], version);
    std::fs::write(output, &content)?;
    println!("Generated {} symbol → {}", spec.mpn, output);
    println!("  {} ({}, {} pins)", spec.mpn, spec.lib_id(), spec.pins.len());
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

    let pkg_type = PackageType::from_package_str(package)
        .context(format!("Cannot parse package '{}'. Examples: DIP-8, SOIC-16, SOT-23-5, TSSOP-20", package))?;
    let pin_count = extract_pin_count(package)
        .context(format!("No pin count in '{}'. Examples: DIP-8, SOIC-16", package))?;

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
        outline::compute_dip_outlines(pin_count, spec.pitch, row_spacing.unwrap_or(7.62), 0.5)
    } else {
        (vec![], None)
    };

    let content = fp_sexpr::generate_footprint(
        &result.name, &result.description, &result.tags,
        result.is_through_hole, &result.pads, &lines, arc.as_ref(), version,
    );

    std::fs::write(output, &content)?;
    println!("Generated footprint {} ({} pads) → {}", result.name, result.pads.len(), output);
    Ok(())
}

fn cmd_lib_table(dir: &str) -> Result<()> {
    let dir_path = std::path::Path::new(dir);

    let sym_libs = lib_table::scan_sym_libraries(dir_path)?;
    let fp_libs = lib_table::scan_fp_libraries(dir_path)?;

    if !sym_libs.is_empty() {
        let content = lib_table::generate_lib_table("sym_lib_table", &sym_libs);
        let out = dir_path.join("sym-lib-table");
        std::fs::write(&out, &content)?;
        println!("Generated sym-lib-table ({} libraries) → {}", sym_libs.len(), out.display());
        for (name, uri) in &sym_libs { println!("  {} → {}", name, uri); }
    }

    if !fp_libs.is_empty() {
        let content = lib_table::generate_lib_table("fp_lib_table", &fp_libs);
        let out = dir_path.join("fp-lib-table");
        std::fs::write(&out, &content)?;
        println!("Generated fp-lib-table ({} libraries) → {}", fp_libs.len(), out.display());
        for (name, uri) in &fp_libs { println!("  {} → {}", name, uri); }
    }

    if sym_libs.is_empty() && fp_libs.is_empty() {
        println!("No libraries found in {}", dir);
    }
    Ok(())
}

fn default_pitch_for_package(pkg_type: &PackageType) -> f64 {
    match pkg_type {
        PackageType::Dip | PackageType::Sip | PackageType::DipSocket | PackageType::PinHeader => 2.54,
        PackageType::Tssop => 0.65,
        PackageType::Qfp | PackageType::Lqfp | PackageType::Tqfp | PackageType::Qfn | PackageType::Dfn => 0.5,
        _ => 1.27,
    }
}
