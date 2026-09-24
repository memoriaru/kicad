use anyhow::Result;
use kicad_cdb::ComponentDb;

pub struct DesignBoardResult {
    pub topology: String,
    pub schematic: String,
    pub summary: Option<String>,
}

pub fn run_design_board(
    db: &ComponentDb,
    vin: f64,
    vout: f64,
    iout: f64,
    topology_override: Option<&str>,
) -> Result<DesignBoardResult> {
    // Step 1: Topology selection
    let topology = topology_override.map(|t| t.to_string()).unwrap_or_else(|| {
        let candidates = kicad_cdb::skills::suggest_topologies(vin, vout, iout, false);
        candidates
            .first()
            .map(|c| c.topology.clone())
            .unwrap_or_else(|| "buck".to_string())
    });

    // Step 2: Run design pipeline for parameter computation
    let pipeline_name = match topology.as_str() {
        "buck" | "boost" | "ldo" | "led" => topology.as_str(),
        _ => "buck",
    };

    let mut user_params = std::collections::HashMap::new();
    user_params.insert("vin".to_string(), vin);
    user_params.insert("vout".to_string(), vout);
    user_params.insert("iout".to_string(), iout);

    let pipeline_result =
        if let Some(pipeline) = kicad_cdb::pipeline::get_builtin_pipeline(pipeline_name) {
            kicad_cdb::pipeline::run_pipeline(db, &pipeline, &user_params).ok()
        } else {
            None
        };

    // Step 3: Generate schematic
    let schematic = kicad_cdb::design::generate_schematic(db, &topology, vin, vout, iout)?;

    // Step 4: Build summary
    let summary = pipeline_result.as_ref().map(|log| {
        let passed = log.passed;
        let failed = log.failed;
        let total_steps = log.steps.len();
        format!(
            "Design Board complete: {}V → {}V @ {}A\nTopology: {}\nPipeline: {} steps ({} passed, {} failed)\nSchematic: {} bytes",
            vin, vout, iout, topology, total_steps, passed, failed, schematic.len()
        )
    });

    Ok(DesignBoardResult {
        topology,
        schematic,
        summary,
    })
}

pub fn render_schematic_to_svg(sch_text: &str) -> Result<String> {
    let lexer = kicad_json5::Lexer::new(sch_text);
    let mut parser = kicad_json5::Parser::new(lexer);
    let schematic = parser.parse()?;

    let sch_renderer = kicad_render::schematic_renderer::SchematicRenderer::new(&schematic);
    let mut svg_renderer = kicad_render::renderer::SvgRenderer::new();
    sch_renderer.render(&mut svg_renderer);
    Ok(svg_renderer.output())
}
