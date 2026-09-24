# kicad-designer

AI-driven circuit design orchestrator — unified CLI + MCP Server that coordinates four kicad-* crates to deliver end-to-end circuit design automation.

## Architecture

```
                ┌─────────────────────┐
                │   kicad-designer     │  ← AI Orchestration Layer
                │   MCP Server + CLI   │
                └──┬──┬──┬──┬─────────┘
                   │  │  │  │
      ┌────────────┘  │  │  └──────────────┐
      ▼               ▼  ▼                  ▼
kicad-cdb      kicad-json5      kicad-symgen   kicad-render
(Data Layer)   (Compiler)       (Generator)    (Renderer)
```

## Features

- **Unified CLI** (`kdesign`) — Single entry point for design, query, and verification
- **MCP Server** — AI agents can invoke design capabilities via MCP protocol (stdio transport)
- **End-to-end workflow** — Requirements → Topology selection → Parameter calculation → Component selection → Schematic generation → ERC verification

## CLI Commands

```bash
# Start MCP server for AI integration
kdesign serve

# Topology suggestion based on requirements
kdesign suggest --vin 12 --vout 3.3 --iout 2

# Generate schematic from topology template
kdesign design --template buck --params "vin=12,vout=3.3,iout=2" -o output.kicad_sch

# Run design pipeline with parameter tracking
kdesign pipeline buck --params "vin=12,vout=3.3,iout=2,fsw=500000"

# Explore design space — compare topologies with scoring
kdesign explore --vin 12 --vout 3.3 --iout 2

# Run Electrical Rules Check
kdesign erc --input schematic.kicad_sch

# Run Design Rules Check
kdesign drc --input board.kicad_pcb
```

## MCP Tools (for AI agents)

| Tool | Description |
|------|-------------|
| `design_board` | Generate complete schematic from requirements |
| `suggest_topology` | Recommend topology based on Vin/Vout/Iout |
| `run_pipeline` | Execute design pipeline with parameter tracking |
| `query_components` | Search components with parametric filters |
| `recommend_components` | Rule-based component recommendation |
| `review_design` | Automated design review (decoupling, floating pins, etc.) |
| `render_schematic` | Generate SVG preview of schematic |
| `run_erc` / `run_drc` | Electrical/Design rule checking |

## Sub-crates

| Crate | Role | Status |
|-------|------|--------|
| [kicad-cdb](../kicad-cdb/) | Component database + design rules + templates | 98% |
| [kicad-json5](../kicad-json5/) | S-expression ↔ JSON5 compiler + layout engine | 90% |
| [kicad-symgen](../kicad-symgen/) | Symbol & footprint generator | 70% |
| [kicad-render](../kicad-render/) | SVG/PDF schematic & PCB renderer | 80% |

## Status

**Status**: CLI and MCP Server are functional (schematic generation, query, verification, routing orchestration end-to-end); PCB text surgery and v35 orchestration are under active development.

## License

Apache License 2.0
