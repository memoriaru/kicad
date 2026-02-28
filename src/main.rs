//! KiCad S-expression to JSON5 Compiler CLI

use clap::Parser;
use kicad_json5::{convert_file, convert_str, Json5Generator, Lexer, Parser as SExprParser};
use std::path::PathBuf;

/// KiCad S-expression to JSON5 compiler
#[derive(Parser, Debug)]
#[command(name = "kicad-json5")]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Input KiCad schematic file (.kicad_sch)
    input: PathBuf,

    /// Output JSON5 file
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Output directory for batch conversion
    #[arg(short = 'd', long)]
    output_dir: Option<PathBuf>,

    /// Indentation size (default: 2 spaces)
    #[arg(short = 'i', long, default_value = "2")]
    indent: usize,

    /// Include comments in output
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

    // Parse
    let lexer = Lexer::new(&source);
    let mut parser = SExprParser::new(lexer);

    if args.debug_ast {
        let ast = parser.parse_sexpr()?;
        println!("AST:\n{:#?}", ast);
        return Ok(());
    }

    let schematic = parser.parse()?;

    if args.verbose {
        eprintln!("Parsed {} components, {} nets, {} wires",
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

    // Generate JSON5
    let config = kicad_json5::codegen::Json5Config {
        indent: " ".repeat(args.indent),
        comments: args.comments,
        include_empty: false,
    };
    let generator = Json5Generator::with_config(config);
    let json5 = generator.generate(&schematic)?;

    // Output
    if let Some(output) = args.output {
        std::fs::write(&output, &json5)?;
        if args.verbose {
            eprintln!("Written: {}", output.display());
        }
    } else {
        // Print to stdout
        println!("{}", json5);
    }

    Ok(())
}
