//! KiCad S-expression / JSON5 bidirectional compiler CLI

use clap::Parser;
use kicad_json5::{InputFormat, Json5Generator, KicadVersion, Lexer, Parser as SExprParser, SexprGenerator};
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
  Forward:  .kicad_sch → JSON5         kicad-json5 input.kicad_sch -o output.json5\n\
  Reverse:  JSON5 → .kicad_sch         kicad-json5 input.json5 -o output.kicad_sch\n\
  Topology: extract circuit topology    kicad-json5 input.kicad_sch -t", long_about = None)]
struct Args {
    /// Input file (.kicad_sch or .json5)
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

    // Detect input format
    let input_format = kicad_json5::detect_input_format(&args.input);

    if args.verbose {
        match input_format {
            InputFormat::Sexpr => eprintln!("Input format: S-expression"),
            InputFormat::Json5 => eprintln!("Input format: JSON5"),
        }
    }

    // Parse
    let schematic = match input_format {
        InputFormat::Sexpr => {
            let lexer = Lexer::new(&source);
            let mut parser = SExprParser::new(lexer);

            if args.debug_ast {
                let ast = parser.parse_sexpr()?;
                println!("AST:\n{:#?}", ast);
                return Ok(());
            }

            parser.parse()?
        }
        InputFormat::Json5 => {
            kicad_json5::parse_json5(&source)?
        }
    };

    if args.verbose {
        eprintln!(
            "Parsed {} components, {} nets, {} wires",
            schematic.components.len(),
            schematic.nets.len(),
            schematic.wires.len()
        );
    }

    // Validate only mode
    if args.validate {
        println!("✓ {} is valid", args.input.display());
        return Ok(());
    }

    // Determine output format
    let format = if args.topology {
        OutputFormat::Topology
    } else if let Some(ref output) = args.output {
        let ext = output
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
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
    if let Some(output) = args.output {
        std::fs::write(&output, &output_content)?;
        if args.verbose {
            eprintln!("Written: {}", output.display());
        }
    } else {
        println!("{}", output_content);
    }

    Ok(())
}
