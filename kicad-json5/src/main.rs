//! KiCad S-expression / JSON5 bidirectional compiler CLI

use clap::Parser;
use kicad_json5::{
    BoardSexprConfig, BoardSexprGenerator, InputFormat, Json5Generator, KicadVersion, Lexer,
    Parser as SExprParser, SexprGenerator,
};
use std::path::PathBuf;

/// Output format
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum OutputFormat {
    #[default]
    Json5,
    Sexpr,
    Topology,
}

impl std::str::FromStr for OutputFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "json5" | "json" => Ok(OutputFormat::Json5),
            "sexpr" | "s-expr" | "kicad" | "sch" => Ok(OutputFormat::Sexpr),
            "topology" | "top" => Ok(OutputFormat::Topology),
            _ => Err(format!("Unknown output format: {}", s)),
        }
    }
}

/// KiCad S-expression / JSON5 bidirectional compiler
#[derive(Parser, Debug)]
#[command(name = "kicad-json5")]
#[command(author, version, about = "Bidirectional KiCad S-expression ↔ JSON5 compiler\n\n\
  Schematic:\n\
    Forward:  .kicad_sch → JSON5         kicad-json5 input.kicad_sch -o output.json5\n\
    Reverse:  JSON5 → .kicad_sch         kicad-json5 input.json5 -o output.kicad_sch\n\
    Topology: extract circuit topology    kicad-json5 input.kicad_sch -t\n\n\
  PCB:\n\
    Forward:  .kicad_pcb → JSON5         kicad-json5 input.kicad_pcb -o output.json5\n\
    Reverse:  JSON5 → .kicad_pcb         kicad-json5 input.json5 -o output.kicad_pcb", long_about = None)]
struct Args {
    /// Input file (.kicad_sch, .kicad_pcb, or .json5)
    input: PathBuf,

    /// Output file (format determined by --format or file extension)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Output format: json5, sexpr, or topology (default: json5)
    #[arg(short = 'f', long, default_value = "json5")]
    format: OutputFormat,

    /// Extract and output circuit topology (shorthand for --format topology)
    #[arg(short = 't', long)]
    topology: bool,

    /// Target KiCad version for S-expr output: 7, 8, 9, 10... (default: auto-detect from input)
    #[arg(long)]
    kicad_version: Option<u8>,

    /// Indentation size for JSON5 (default: 2 spaces)
    #[arg(short = 'i', long, default_value = "2")]
    indent: usize,

    /// Include comments in JSON5 output
    #[arg(long, default_value = "true")]
    comments: bool,

    /// Validate input only (don't generate output)
    #[arg(long)]
    validate: bool,

    /// Print parsed AST (for debugging)
    #[arg(long)]
    debug_ast: bool,

    /// Verbose output
    #[arg(short, long)]
    verbose: bool,

    /// Vendor dialect for PCB s-expr output net references & emit prep:
    /// official (default: top-level net table + canonical references,
    /// loadable by stock kicad-cli), huaqiu (fork: inline net names, no net
    /// table, plus paper/silk-name fallbacks) or auto (content-detect,
    /// round-trip preserving). Board → .kicad_pcb emission only.
    #[arg(long, default_value = "official")]
    dialect: String,

    /// Insert PWR_FLAG symbols for power nets (JSON5→S-expression only).
    /// Adds power flags on nets with power_in pins but no power_out driver,
    /// eliminating KiCad ERC power_pin_not_driven warnings.
    #[arg(long, default_value_t = true)]
    power_flags: bool,
}

/// Board detection must tolerate both JSON5 unquoted keys (`segments:`) and
/// strict-JSON quoted keys (`"segments":`) - strict-JSON round trips of board
/// files previously misdetected as schematics.
fn looks_like_board(source: &str) -> bool {
    ["footprints", "segments"]
        .iter()
        .any(|key| source.contains(&format!("{key}:")) || source.contains(&format!("\"{key}\":")))
}

fn main() {
    let args = Args::parse();

    if let Err(e) = run(args) {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn run(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    // Read input file
    if args.verbose {
        eprintln!("Reading: {}", args.input.display());
    }

    let source = std::fs::read_to_string(&args.input)?;
    let input_format = kicad_json5::detect_input_format(&args.input);

    if args.verbose {
        match input_format {
            InputFormat::Sexpr => eprintln!("Input format: S-expression (schematic)"),
            InputFormat::PcbSexpr => eprintln!("Input format: S-expression (PCB)"),
            InputFormat::Json5 => eprintln!("Input format: JSON5"),
        }
    }

    // Validate only mode
    if args.validate {
        match input_format {
            InputFormat::PcbSexpr => {
                let board = kicad_json5::parse_board(&source)?;
                if args.verbose {
                    eprintln!(
                        "Parsed {} footprints, {} nets, {} segments",
                        board.footprints.len(),
                        board.nets.len(),
                        board.segments.len()
                    );
                }
            }
            InputFormat::Json5 => {
                // Try board first, then schematic
                if looks_like_board(&source) {
                    let _board = kicad_json5::parse_board_json5(&source)?;
                } else {
                    let _sch = kicad_json5::parse_json5(&source)?;
                }
            }
            InputFormat::Sexpr => {
                let lexer = Lexer::new(&source);
                let mut parser = SExprParser::new(lexer);
                parser.parse()?;
            }
        }
        println!("✓ {} is valid", args.input.display());
        return Ok(());
    }

    // Dispatch to PCB or schematic path
    match input_format {
        InputFormat::PcbSexpr => run_pcb(&args, &source),
        InputFormat::Json5 => {
            // Auto-detect: if JSON5 contains board fields, treat as PCB
            if looks_like_board(&source) {
                run_pcb(&args, &source)
            } else {
                run_schematic(&args, &source)
            }
        }
        InputFormat::Sexpr => run_schematic(&args, &source),
    }
}

fn run_pcb(args: &Args, source: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Parse
    let board = if source.trim_start().starts_with('{') {
        kicad_json5::parse_board_json5(source)?
    } else {
        kicad_json5::parse_board(source)?
    };

    if args.verbose {
        eprintln!(
            "Parsed {} footprints, {} nets, {} segments",
            board.footprints.len(),
            board.nets.len(),
            board.segments.len()
        );
    }

    // Determine output format
    let output_ext = args
        .output
        .as_ref()
        .and_then(|p| p.extension().and_then(|e| e.to_str()));

    // Vendor dialect emit 整备（仅 .kicad_pcb 输出；json5 真源保持 IR 原样）。
    // 方言同时驱动 net 引用形态: Official 写顶层 net 表 + 规范引用
    // (stock kicad-cli 可加载); Huaqiu 内联 net 名省表 (HQ fork 要求)。
    let mut board = board;
    let mut dialect = kicad_json5::dialect::VendorDialect::Official;
    if matches!(output_ext, Some("kicad_pcb") | None | Some("sexpr")) {
        dialect = match args.dialect.as_str() {
            "huaqiu" => kicad_json5::dialect::VendorDialect::Huaqiu,
            "official" => kicad_json5::dialect::VendorDialect::Official,
            "auto" => kicad_json5::dialect::VendorDialect::detect_board(source),
            other => {
                return Err(format!("unknown --dialect '{other}' (auto|huaqiu|official)").into())
            }
        };
        if args.verbose {
            eprintln!("Vendor dialect: {dialect:?}");
        }
        dialect.prepare_board(&mut board);
    }

    let output_content = match output_ext {
        Some("json5" | "json") => kicad_json5::generate_board_json5(&board)?,
        Some("kicad_pcb") | None => {
            let config = BoardSexprConfig {
                dialect,
                ..Default::default()
            };
            let mut gen = BoardSexprGenerator::with_config(config);
            gen.generate(&board)?
        }
        _ => {
            let config = BoardSexprConfig {
                dialect,
                ..Default::default()
            };
            let mut gen = BoardSexprGenerator::with_config(config);
            gen.generate(&board)?
        }
    };

    // Output
    if let Some(output) = &args.output {
        std::fs::write(output, &output_content)?;
        if args.verbose {
            eprintln!("Written: {}", output.display());
        }
    } else {
        println!("{}", output_content);
    }

    Ok(())
}

fn run_schematic(args: &Args, source: &str) -> Result<(), Box<dyn std::error::Error>> {
    let input_format = kicad_json5::detect_input_format(&args.input);

    // Parse
    let schematic = match input_format {
        InputFormat::Sexpr | InputFormat::PcbSexpr => {
            let lexer = Lexer::new(source);
            let mut parser = SExprParser::new(lexer);

            if args.debug_ast {
                let ast = parser.parse_sexpr()?;
                println!("AST:\n{:#?}", ast);
                return Ok(());
            }

            parser.parse()?
        }
        InputFormat::Json5 => kicad_json5::parse_json5(source)?,
    };

    if args.verbose {
        eprintln!(
            "Parsed {} components, {} nets, {} wires",
            schematic.components.len(),
            schematic.nets.len(),
            schematic.wires.len()
        );
    }

    // Determine output format
    let format = if args.topology {
        OutputFormat::Topology
    } else if let Some(ref output) = args.output {
        let ext = output.extension().and_then(|e| e.to_str()).unwrap_or("");
        match ext {
            "kicad_sch" | "sch" => OutputFormat::Sexpr,
            "json5" | "json" => OutputFormat::Json5,
            _ => args.format,
        }
    } else {
        args.format
    };

    // Generate output
    let output_content = match format {
        OutputFormat::Json5 => {
            let config = kicad_json5::codegen::Json5Config {
                indent: " ".repeat(args.indent),
                comments: args.comments,
                include_empty: false,
            };
            let generator = Json5Generator::with_config(config);
            generator.generate(&schematic)?
        }
        OutputFormat::Sexpr => {
            let kicad_version = match args.kicad_version {
                Some(7) => Some(KicadVersion::V7),
                Some(8) => Some(KicadVersion::V8),
                Some(9) => Some(KicadVersion::V9),
                Some(v) if v >= 10 => Some(KicadVersion::V10),
                None => None, // auto-detect from input
                Some(_) => Some(KicadVersion::V8),
            };
            let config = kicad_json5::codegen::SexprConfig {
                indent: "\t".to_string(),
                include_uuids: true,
                kicad_version,
                generate_uuids: true,
                insert_power_flags: args.power_flags,
            };
            let mut generator = SexprGenerator::with_config(config);
            generator.generate(&schematic)?
        }
        OutputFormat::Topology => {
            let summary = kicad_json5::topology::extract_topology(&schematic);
            summary.to_json5()
        }
    };

    // Output
    if let Some(ref output) = args.output {
        std::fs::write(output, &output_content)?;
        if args.verbose {
            eprintln!("Written: {}", output.display());
        }
    } else {
        println!("{}", output_content);
    }

    Ok(())
}
