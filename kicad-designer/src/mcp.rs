use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

use kicad_cdb::ComponentDb;

// ---------------------------------------------------------------------------
// JSON-RPC types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct Request {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Serialize)]
struct Response {
    jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcError>,
}

#[derive(Serialize)]
struct RpcError {
    code: i32,
    message: String,
}

// ---------------------------------------------------------------------------
// MCP tool definitions
// ---------------------------------------------------------------------------

fn tool_list() -> Vec<Value> {
    vec![
        json!({
            "name": "query_components",
            "description": "Query components from the local database with filters",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "search": { "type": "string", "description": "Full-text search (MPN, description, package)" },
                    "category": { "type": "string", "description": "Category name filter" },
                    "params": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "name": { "type": "string" },
                                "min": { "type": "number" },
                                "max": { "type": "number" }
                            },
                            "required": ["name"]
                        },
                        "description": "Parameter range filters"
                    },
                    "manufacturer": { "type": "string" },
                    "package": { "type": "string" },
                    "in_stock": { "type": "boolean" },
                    "limit": { "type": "integer" }
                }
            }
        }),
        json!({
            "name": "show_component",
            "description": "Show full details for a component by MPN",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "mpn": { "type": "string", "description": "Manufacturer Part Number" }
                },
                "required": ["mpn"]
            }
        }),
        json!({
            "name": "list_categories",
            "description": "List all component categories in the database",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "list_parameters",
            "description": "List distinct parameter names available for filtering",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "category": { "type": "string", "description": "Filter by category" }
                }
            }
        }),
        json!({
            "name": "suggest_topology",
            "description": "Suggest power topology based on voltage/current requirements",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "vin": { "type": "number", "description": "Input voltage (V)" },
                    "vout": { "type": "number", "description": "Output voltage (V)" },
                    "iout": { "type": "number", "description": "Output current (A)" },
                    "isolated": { "type": "boolean", "description": "Require galvanic isolation" }
                },
                "required": ["vin", "vout", "iout"]
            }
        }),
        json!({
            "name": "run_pipeline",
            "description": "Run a design pipeline (buck, boost, ldo, led)",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Pipeline name" },
                    "params": {
                        "type": "object",
                        "description": "Key-value parameters (e.g. {\"vin\":12, \"vout\":3.3})"
                    }
                },
                "required": ["name"]
            }
        }),
        json!({
            "name": "check_rule",
            "description": "Apply a design rule with given parameters",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "rule": { "type": "string", "description": "Rule name" },
                    "params": {
                        "type": "object",
                        "description": "Key-value numeric parameters"
                    },
                    "candidate": { "type": "string", "description": "Candidate value as name=value" }
                },
                "required": ["rule", "params"]
            }
        }),
        json!({
            "name": "search_online",
            "description": "Search HuaQiu online component library",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "keyword": { "type": "string" },
                    "limit": { "type": "integer" }
                },
                "required": ["keyword"]
            }
        }),
        json!({
            "name": "fetch_component",
            "description": "Fetch component from HuaQiu and import into local database",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "mpn": { "type": "string" },
                    "mfg_id": { "type": "string" }
                },
                "required": ["mpn"]
            }
        }),
        json!({
            "name": "recommend_components",
            "description": "Apply a design rule and search for components matching computed constraints",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "rule": { "type": "string", "description": "Rule name" },
                    "params": {
                        "type": "object",
                        "description": "Key-value numeric parameters"
                    },
                    "candidate": { "type": "string", "description": "Candidate value as name=value" },
                    "limit": { "type": "integer" }
                },
                "required": ["rule", "params"]
            }
        }),
        json!({
            "name": "run_erc",
            "description": "Run ERC (Electrical Rules Check) on a KiCad schematic and return parsed results",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "input": { "type": "string", "description": "Path to .kicad_sch file" }
                },
                "required": ["input"]
            }
        }),
        json!({
            "name": "run_drc",
            "description": "Run DRC (Design Rules Check) on a KiCad PCB and return parsed results",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "input": { "type": "string", "description": "Path to .kicad_pcb file" }
                },
                "required": ["input"]
            }
        }),
        json!({
            "name": "match_skill",
            "description": "Match design skills from a natural language query. Returns ranked rules with extracted parameters.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Natural language query (e.g. '5V to 3.3V buck converter 2A')" }
                },
                "required": ["query"]
            }
        }),
        json!({
            "name": "explore_designs",
            "description": "Explore design space — compare topologies with scoring and ranking",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "vin": { "type": "number", "description": "Input voltage (V)" },
                    "vout": { "type": "number", "description": "Output voltage (V)" },
                    "iout": { "type": "number", "description": "Output current (A)" },
                    "isolated": { "type": "boolean", "description": "Require galvanic isolation" }
                },
                "required": ["vin", "vout", "iout"]
            }
        }),
        json!({
            "name": "run_workflow",
            "description": "Run a multi-stage design workflow (e.g. power_tree)",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "goal": { "type": "string", "description": "Workflow name (e.g. power_tree)" },
                    "params": {
                        "type": "object",
                        "description": "Key-value parameters (e.g. {\"vin\":12, \"vout\":3.3, \"iout\":2})"
                    }
                },
                "required": ["goal"]
            }
        }),
        json!({
            "name": "design_board",
            "description": "End-to-end design: from requirements to complete schematic via AI orchestrator",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "vin": { "type": "number", "description": "Input voltage (V)" },
                    "vout": { "type": "number", "description": "Output voltage (V)" },
                    "iout": { "type": "number", "description": "Output current (A)" },
                    "topology": { "type": "string", "description": "Topology override (auto-selected if omitted)" }
                },
                "required": ["vin", "vout", "iout"]
            }
        }),
        json!({
            "name": "design_power_tree",
            "description": "Design a multi-rail power tree: auto-decompose into cascaded/parallel converters and generate complete schematic",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "vin": { "type": "number", "description": "Input voltage (V)" },
                    "outputs": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "vout": { "type": "number", "description": "Output voltage (V)" },
                                "iout": { "type": "number", "description": "Output current (A)" },
                                "name": { "type": "string", "description": "Optional rail name" }
                            },
                            "required": ["vout", "iout"]
                        },
                        "description": "Output rail specifications"
                    },
                    "isolated": { "type": "boolean", "description": "Require galvanic isolation" }
                },
                "required": ["vin", "outputs"]
            }
        }),
        json!({
            "name": "generate_pcb",
            "description": "Generate a .kicad_pcb file from a composed schematic. Converts components to footprints, maps nets, adds board outline and GND copper zone. Returns built-in DRC results.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "schematic": { "type": "string", "description": "KiCad schematic content (.kicad_sch text)" }
                },
                "required": ["schematic"]
            }
        }),
        json!({
            "name": "check_pcb",
            "description": "Run built-in DRC checks on a PCB generated from a schematic. Checks pad clearance, trace widths, unconnected nets, board outline, and component overlap.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "schematic": { "type": "string", "description": "KiCad schematic content (.kicad_sch text)" }
                },
                "required": ["schematic"]
            }
        }),
        json!({
            "name": "render_schematic",
            "description": "Render a KiCad schematic file to SVG for preview",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "input": { "type": "string", "description": "Path to .kicad_sch file" },
                    "output": { "type": "string", "description": "Output SVG file path" }
                },
                "required": ["input"]
            }
        }),
        json!({
            "name": "export_gerber",
            "description": "Export Gerber and drill files from a .kicad_pcb using kicad-cli. Returns list of generated manufacturing files.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "pcb_path": { "type": "string", "description": "Path to .kicad_pcb file" },
                    "output_dir": { "type": "string", "description": "Output directory for Gerber files" }
                },
                "required": ["pcb_path", "output_dir"]
            }
        }),
        json!({
            "name": "generate_symbol",
            "description": "Generate a KiCad symbol library (.kicad_sym) for one or more components from the database by MPN or search query.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "mpn": { "type": "string", "description": "Manufacturer Part Number (exact match)" },
                    "search": { "type": "string", "description": "Full-text search to find components" },
                    "limit": { "type": "integer", "description": "Max components to include (default 10)" }
                }
            }
        }),
        json!({
            "name": "list_reference_designs",
            "description": "List saved reference designs from the library. Optionally filter by tag.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "tag": { "type": "string", "description": "Filter by tag (substring match)" }
                }
            }
        }),
        json!({
            "name": "get_reference_design",
            "description": "Get a full reference design by name, including schematic and parameters.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Design name" }
                },
                "required": ["name"]
            }
        }),
        json!({
            "name": "save_reference_design",
            "description": "Save a design to the reference design library for reuse.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Design name" },
                    "description": { "type": "string", "description": "Design description" },
                    "tags": { "type": "string", "description": "Comma-separated tags" },
                    "topology": { "type": "string", "description": "Topology type (buck, boost, ldo, etc.)" },
                    "requirements": { "type": "string", "description": "Design requirements (JSON)" },
                    "schematic": { "type": "string", "description": "KiCad schematic content (.kicad_sch)" },
                    "parameters": { "type": "string", "description": "Design parameters (JSON)" },
                    "verified": { "type": "boolean", "description": "Whether design has been verified" }
                },
                "required": ["name"]
            }
        }),
        json!({
            "name": "find_alternatives",
            "description": "Find pin-compatible alternative components for a given MPN. Searches same category with similar parameters.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "mpn": { "type": "string", "description": "Manufacturer Part Number to find alternatives for" },
                    "limit": { "type": "integer", "description": "Max results (default 5)" }
                },
                "required": ["mpn"]
            }
        }),
        json!({
            "name": "estimate_bom_cost",
            "description": "Estimate BOM cost for a list of components with quantities. Uses supply_info price breaks.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "items": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "mpn": { "type": "string" },
                                "quantity": { "type": "integer" }
                            },
                            "required": ["mpn", "quantity"]
                        },
                        "description": "List of {mpn, quantity} items"
                    }
                },
                "required": ["items"]
            }
        }),
        json!({
            "name": "design_ic_board",
            "description": "Design an IC-centric board from structured requirements. Supports MCU, OpAmp, ADC, DAC, USB-UART, CAN, Sensor, Driver IC types. Auto-generates composition with peripherals, power rails, and interfaces.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Board name" },
                    "core_ics": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "ic_type": { "type": "string", "enum": ["mcu","op_amp","adc","dac","usb_uart","can","sensor","driver"], "description": "IC type" },
                                "mpn": { "type": "string", "description": "Specific part number (optional)" },
                                "parameters": { "type": "object", "description": "IC parameters (frequency, vdd, gain, resolution, etc.)" }
                            },
                            "required": ["ic_type"]
                        },
                        "description": "Core ICs on the board"
                    },
                    "interfaces": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "type": { "type": "string", "enum": ["usb","uart","spi","i2c","swd","jtag","can","gpio","power_in","power_out","analog"] },
                                "role": { "type": "string", "description": "Interface role (debug, data, power_input, etc.)" },
                                "parameters": { "type": "object" }
                            },
                            "required": ["type"]
                        },
                        "description": "Board interfaces"
                    },
                    "power_input_voltage": { "type": "number", "description": "Main power input voltage (V)" },
                    "generate_schematic": { "type": "boolean", "description": "Auto-generate schematic (default true)" },
                    "constraints": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "name": { "type": "string" },
                                "value": { "type": "number" },
                                "unit": { "type": "string" }
                            },
                            "required": ["name", "value"]
                        },
                        "description": "Design constraints"
                    }
                },
                "required": ["name", "core_ics", "power_input_voltage"]
            }
        }),
        json!({
            "name": "list_ic_types",
            "description": "List all supported IC types and their peripheral/interface/power requirements from the knowledge base.",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        }),
        json!({
            "name": "save_design",
            "description": "Save a design snapshot for later iteration. Stores spec, composition, power tree, and schematic.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Design name" },
                    "spec": { "type": "object", "description": "DesignSpec JSON (PowerBoard, IcBoard, or MixedBoard)" },
                    "composition": { "type": "object", "description": "Composition JSON (optional)" },
                    "power_tree": { "type": "array", "description": "Power tree nodes (optional)" },
                    "schematic": { "type": "string", "description": "Generated schematic text (optional)" }
                },
                "required": ["name", "spec"]
            }
        }),
        json!({
            "name": "load_design",
            "description": "Load a previously saved design snapshot by ID.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "design_id": { "type": "string", "description": "Design snapshot ID" }
                },
                "required": ["design_id"]
            }
        }),
        json!({
            "name": "update_design",
            "description": "Update a saved design by changing parameters. Only re-computes affected modules (incremental update).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "design_id": { "type": "string", "description": "Design snapshot ID to update" },
                    "changes": {
                        "type": "object",
                        "description": "Parameter changes: {\"vin\": 24.0, \"output.0.vout\": 3.3, \"ic.0.frequency\": 48.0, \"power_input_voltage\": 12.0}",
                        "additionalProperties": { "type": "number" }
                    }
                },
                "required": ["design_id", "changes"]
            }
        }),
        json!({
            "name": "list_designs",
            "description": "List all saved design snapshots.",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        }),
        json!({
            "name": "list_design_versions",
            "description": "List all versions of a design (H1 version history). Returns versions newest-first for rollback selection.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "design_id": { "type": "string", "description": "Design snapshot ID" }
                },
                "required": ["design_id"]
            }
        }),
        json!({
            "name": "restore_design_version",
            "description": "Load a specific version of a design for rollback (H1). Returns the full snapshot at that version.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "design_id": { "type": "string", "description": "Design snapshot ID" },
                    "version": { "type": "integer", "description": "Version number to restore" }
                },
                "required": ["design_id", "version"]
            }
        }),
        json!({
            "name": "review_report_html",
            "description": "H5: Generate a self-contained HTML design review report. Run review on a composition+power_tree and return HTML (openable in browser).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "review_json": { "type": "object", "description": "DesignReviewResult JSON (from power-tree review field)" },
                    "title": { "type": "string", "description": "Report title (default: Design Review)" }
                },
                "required": ["review_json"]
            }
        }),
        json!({
            "name": "check_hierarchy",
            "description": "P1-5: Load a hierarchical KiCad project and validate sheet pin/label interfaces across all sub-sheets.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "root_sch_path": { "type": "string", "description": "Path to root .kicad_sch file" }
                },
                "required": ["root_sch_path"]
            }
        }),
        json!({
            "name": "analyze_pcb",
            "description": "Analyze a KiCad PCB file: extract footprint positions, net topology, density heatmap, and routing statistics. Returns structured data for AI-driven layout/routing decisions.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "pcb_path": { "type": "string", "description": "Path to .kicad_pcb file" }
                },
                "required": ["pcb_path"]
            }
        }),
        json!({
            "name": "move_footprint",
            "description": "Move a footprint to a new position on the PCB. Optionally rip-up connected traces (default: true). Modifies the .kicad_pcb file directly.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "pcb_path": { "type": "string", "description": "Path to .kicad_pcb file" },
                    "reference": { "type": "string", "description": "Footprint reference designator (e.g. 'U1', 'C3')" },
                    "x": { "type": "number", "description": "New X position in mm" },
                    "y": { "type": "number", "description": "New Y position in mm" },
                    "rotation": { "type": "number", "description": "New rotation in degrees (optional)" },
                    "rip_traces": { "type": "boolean", "description": "Rip-up traces connected to this footprint's nets (default: true)" }
                },
                "required": ["pcb_path", "reference", "x", "y"]
            }
        }),
        json!({
            "name": "reroute_nets",
            "description": "Rip-up and re-route specific nets on a PCB. Uses A* routing engine with DRC feedback.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "pcb_path": { "type": "string", "description": "Path to .kicad_pcb file" },
                    "output_path": { "type": "string", "description": "Output path for modified .kicad_pcb" },
                    "nets": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "List of net names to re-route. If empty, re-routes all signal nets."
                    },
                    "layers": { "type": "integer", "description": "Number of layers (default 4)" }
                },
                "required": ["pcb_path", "output_path"]
            }
        }),
        json!({
            "name": "open_in_kicad",
            "description": "Open a .kicad_pcb file in KiCad PCB editor for visual inspection.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "pcb_path": { "type": "string", "description": "Path to .kicad_pcb file" }
                },
                "required": ["pcb_path"]
            }
        }),
        json!({
            "name": "smart_fix",
            "description": "Analyze DRC violations and automatically apply targeted fixes: move footprints to resolve clearance issues, rip-up and re-route conflicting nets. Returns before/after DRC comparison.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "pcb_path": { "type": "string", "description": "Path to .kicad_pcb file" },
                    "output_path": { "type": "string", "description": "Output path for fixed .kicad_pcb" },
                    "max_iterations": { "type": "integer", "description": "Max fix iterations (default 3)" },
                    "layers": { "type": "integer", "description": "Number of layers (default 4)" }
                },
                "required": ["pcb_path", "output_path"]
            }
        }),
        // Phase 3: IPC tools
        json!({
            "name": "ipc_check",
            "description": "Check KiCad IPC connectivity. Reports whether KiCad API server is reachable, dependencies (protobuf/pynng) are installed, and which capabilities are available.",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        }),
        json!({
            "name": "ipc_move_footprint",
            "description": "Move a footprint to a new position via KiCad IPC (real-time update in editor) or file-level fallback. Requires KiCad running for IPC mode.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "pcb_path": { "type": "string", "description": "Path to .kicad_pcb file" },
                    "reference": { "type": "string", "description": "Footprint reference (e.g. 'U1', 'C3')" },
                    "x": { "type": "number", "description": "New X position in mm" },
                    "y": { "type": "number", "description": "New Y position in mm" },
                    "angle": { "type": "number", "description": "New rotation angle in degrees (optional)" },
                    "use_ipc": { "type": "boolean", "description": "Force IPC mode (default: auto-detect)" }
                },
                "required": ["pcb_path", "reference", "x", "y"]
            }
        }),
        json!({
            "name": "ipc_get_board_state",
            "description": "Get current board state from KiCad via IPC or file parsing. Returns footprints, tracks, nets summary. Requires KiCad running for IPC mode.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "pcb_path": { "type": "string", "description": "Path to .kicad_pcb file" },
                    "include_tracks": { "type": "boolean", "description": "Include track details (default: false)" },
                    "use_ipc": { "type": "boolean", "description": "Force IPC mode (default: auto-detect)" }
                },
                "required": ["pcb_path"]
            }
        }),
        json!({
            "name": "ipc_delete_tracks",
            "description": "Delete all tracks/vias belonging to a specific net. Works file-level (no KiCad needed) or via IPC.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "pcb_path": { "type": "string", "description": "Path to .kicad_pcb file" },
                    "net_name": { "type": "string", "description": "Net name to delete tracks from" },
                    "use_ipc": { "type": "boolean", "description": "Force IPC mode (default: auto-detect)" }
                },
                "required": ["pcb_path", "net_name"]
            }
        }),
    ]
}

// ---------------------------------------------------------------------------
// Tool dispatch
// ---------------------------------------------------------------------------

fn dispatch(db: &ComponentDb, method: &str, params: Value) -> Result<Value> {
    match method {
        "tools/list" => Ok(json!({ "tools": tool_list() })),
        "tools/call" => {
            let name = params["name"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing tool name"))?;
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            call_tool(db, name, args)
        }
        "initialize" => Ok(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "kdesign", "version": env!("CARGO_PKG_VERSION") }
        })),
        "notifications/initialized" | "ping" => Ok(Value::Null),
        _ => Err(anyhow::anyhow!("Unknown method: {}", method)),
    }
}

fn call_tool(db: &ComponentDb, name: &str, args: Value) -> Result<Value> {
    match name {
        "query_components" => tool_query(db, &args),
        "show_component" => tool_show(db, &args),
        "list_categories" => tool_categories(db),
        "list_parameters" => tool_list_params(db, &args),
        "suggest_topology" => tool_suggest(&args),
        "run_pipeline" => tool_pipeline(db, &args),
        "check_rule" => tool_check(db, &args),
        "search_online" => tool_search_online(&args),
        "fetch_component" => tool_fetch(db, &args),
        "recommend_components" => tool_recommend(db, &args),
        "run_erc" => tool_erc(&args),
        "run_drc" => tool_drc(&args),
        "match_skill" => tool_match_skill(db, &args),
        "explore_designs" => tool_explore(db, &args),
        "run_workflow" => tool_workflow(db, &args),
        "design_board" => tool_design_board(db, &args),
        "design_power_tree" => tool_power_tree(db, &args),
        "render_schematic" => tool_render_schematic(&args),
        "generate_pcb" => tool_generate_pcb(db, &args),
        "check_pcb" => tool_check_pcb(db, &args),
        "export_gerber" => tool_export_gerber(&args),
        "generate_symbol" => tool_generate_symbol(db, &args),
        "list_reference_designs" => tool_list_ref_designs(db, &args),
        "get_reference_design" => tool_get_ref_design(db, &args),
        "save_reference_design" => tool_save_ref_design(db, &args),
        "find_alternatives" => tool_find_alternatives(db, &args),
        "estimate_bom_cost" => tool_estimate_bom_cost(db, &args),
        "design_ic_board" => tool_design_ic_board(db, &args),
        "list_ic_types" => tool_list_ic_types(),
        "save_design" => tool_save_design(db, &args),
        "load_design" => tool_load_design(&args),
        "update_design" => tool_update_design(db, &args),
        "list_designs" => tool_list_designs(),
        "list_design_versions" => tool_list_design_versions(&args),
        "restore_design_version" => tool_restore_design_version(&args),
        "review_report_html" => tool_review_report_html(&args),
        "check_hierarchy" => tool_check_hierarchy(&args),
        "analyze_pcb" => tool_analyze_pcb(&args),
        "move_footprint" => tool_move_footprint(&args),
        "reroute_nets" => tool_reroute_nets(&args),
        "open_in_kicad" => tool_open_in_kicad(&args),
        "smart_fix" => tool_smart_fix(&args),
        "ipc_check" => tool_ipc_check(),
        "ipc_move_footprint" => tool_ipc_move_footprint(&args),
        "ipc_get_board_state" => tool_ipc_get_board_state(&args),
        "ipc_delete_tracks" => tool_ipc_delete_tracks(&args),
        _ => Err(anyhow::anyhow!("Unknown tool: {}", name)),
    }
}

// ---------------------------------------------------------------------------
// Tool implementations
// ---------------------------------------------------------------------------

fn tool_query(db: &ComponentDb, args: &Value) -> Result<Value> {
    let param_filter = args["params"].as_array().and_then(|arr| {
        arr.first().map(|pf| {
            let name = pf["name"].as_str().unwrap_or("");
            (name, pf["min"].as_f64(), pf["max"].as_f64())
        })
    });

    let results = kicad_cdb::service::query_filtered(
        db,
        args["search"].as_str(),
        args["category"].as_str(),
        args["manufacturer"].as_str(),
        args["package"].as_str(),
        param_filter,
        args["in_stock"].as_bool().unwrap_or(false),
        args["limit"].as_u64().map(|n| n as usize),
    )?;

    Ok(json!({ "count": results.len(), "components": results }))
}

fn tool_show(db: &ComponentDb, args: &Value) -> Result<Value> {
    let mpn = args["mpn"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing mpn"))?;
    let comp = db
        .get_component_by_mpn_any(mpn)?
        .ok_or_else(|| anyhow::anyhow!("Component '{}' not found", mpn))?;

    let id = comp.id.context("Component missing id")?;
    let pins = db.get_pins(id)?;
    let params = db.get_parameters(id)?;
    let models = db.get_simulation_models(id)?;
    let supply = db.get_supply_info(id)?;

    Ok(json!({
        "component": comp,
        "pins": pins,
        "parameters": params,
        "models": models,
        "supply": supply
    }))
}

fn tool_categories(db: &ComponentDb) -> Result<Value> {
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
    Ok(json!({ "categories": cats }))
}

fn tool_list_params(db: &ComponentDb, args: &Value) -> Result<Value> {
    let cat = args["category"].as_str();
    let names = db.list_parameter_names(cat)?;
    Ok(json!({ "parameters": names }))
}

fn tool_suggest(args: &Value) -> Result<Value> {
    let vin = args["vin"]
        .as_f64()
        .ok_or_else(|| anyhow::anyhow!("Missing vin"))?;
    let vout = args["vout"]
        .as_f64()
        .ok_or_else(|| anyhow::anyhow!("Missing vout"))?;
    let iout = args["iout"]
        .as_f64()
        .ok_or_else(|| anyhow::anyhow!("Missing iout"))?;
    let isolated = args["isolated"].as_bool().unwrap_or(false);
    let recs = kicad_cdb::skills::suggest_topologies(vin, vout, iout, isolated);
    Ok(json!({
        "requirements": { "vin": vin, "vout": vout, "iout": iout, "isolated": isolated },
        "recommendations": recs
    }))
}

fn tool_pipeline(db: &ComponentDb, args: &Value) -> Result<Value> {
    let name = args["name"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing pipeline name"))?;
    let pipeline = kicad_cdb::pipeline::get_builtin_pipeline(name)
        .ok_or_else(|| anyhow::anyhow!("Unknown pipeline: {}", name))?;

    let user_params: std::collections::HashMap<String, f64> = args["params"]
        .as_object()
        .map(|obj| {
            obj.iter()
                .filter_map(|(k, v)| v.as_f64().map(|n| (k.clone(), n)))
                .collect()
        })
        .unwrap_or_default();

    let log = kicad_cdb::pipeline::run_pipeline(db, &pipeline, &user_params)?;
    Ok(serde_json::to_value(&log)?)
}

fn tool_check(db: &ComponentDb, args: &Value) -> Result<Value> {
    let rule_name = args["rule"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing rule name"))?;

    let params_str = args["params"]
        .as_object()
        .map(|obj| {
            obj.iter()
                .filter_map(|(k, v)| v.as_f64().map(|n| format!("{}={}", k, n)))
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_default();

    let candidate_str = args["candidate"].as_str();
    let (rule, result) =
        kicad_cdb::service::apply_rule_with_str_params(db, rule_name, &params_str, candidate_str)?;

    Ok(json!({
        "rule": rule.name,
        "description": rule.description,
        "outputs": result.outputs,
        "check_expr": result.check_expression,
        "pass": result.pass
    }))
}

fn tool_search_online(args: &Value) -> Result<Value> {
    let keyword = args["keyword"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing keyword"))?;
    let limit = args["limit"].as_u64().unwrap_or(20) as usize;
    let client = kicad_cdb::hqapi::HqClient::new()?;
    let results = client.search(keyword, limit)?;
    Ok(json!({ "count": results.len(), "results": results }))
}

fn tool_recommend(db: &ComponentDb, args: &Value) -> Result<Value> {
    let rule_name = args["rule"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing rule name"))?;

    let params_str = args["params"]
        .as_object()
        .map(|obj| {
            obj.iter()
                .filter_map(|(k, v)| v.as_f64().map(|n| format!("{}={}", k, n)))
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_default();

    let candidate_str = args["candidate"].as_str();
    let limit = args["limit"].as_u64().map(|n| n as usize);

    let (rule, result, recommendations) =
        kicad_cdb::service::recommend_components(db, rule_name, &params_str, candidate_str, limit)?;

    Ok(json!({
        "rule": rule.name,
        "outputs": result.outputs,
        "pass": result.pass,
        "recommendation_count": recommendations.len(),
        "recommendations": recommendations,
    }))
}

fn tool_fetch(db: &ComponentDb, args: &Value) -> Result<Value> {
    let mpn = args["mpn"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing mpn"))?;
    let mfg_id = args["mfg_id"].as_str();
    let id = kicad_cdb::hqapi::fetch_and_import(db, mpn, mfg_id)?;
    let comp = db
        .get_component(id)?
        .ok_or_else(|| anyhow::anyhow!("Component {} not found after import", id))?;
    Ok(json!({ "imported": comp, "id": id }))
}

fn tool_erc(args: &Value) -> Result<Value> {
    let input = args["input"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing input path"))?;
    let cfg = kicad_cdb::config::AppConfig::load()?;
    let report = kicad_cdb::erc::run_erc(&cfg.kicad_cli_path, input)?;
    Ok(serde_json::to_value(&report)?)
}

fn tool_drc(args: &Value) -> Result<Value> {
    let input = args["input"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing input path"))?;
    let cfg = kicad_cdb::config::AppConfig::load()?;
    let report = kicad_cdb::drc::run_drc(&cfg.kicad_cli_path, input)?;
    Ok(serde_json::to_value(&report)?)
}

fn tool_match_skill(db: &ComponentDb, args: &Value) -> Result<Value> {
    let query = args["query"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing query"))?;
    let rules = db.get_all_design_rules()?;
    let matches = kicad_cdb::skill_match::match_skills(query, &rules)?;
    Ok(serde_json::to_value(&matches)?)
}

fn tool_explore(db: &ComponentDb, args: &Value) -> Result<Value> {
    let vin = args["vin"]
        .as_f64()
        .ok_or_else(|| anyhow::anyhow!("Missing vin"))?;
    let vout = args["vout"]
        .as_f64()
        .ok_or_else(|| anyhow::anyhow!("Missing vout"))?;
    let iout = args["iout"]
        .as_f64()
        .ok_or_else(|| anyhow::anyhow!("Missing iout"))?;
    let isolated = args["isolated"].as_bool().unwrap_or(false);
    let result = kicad_cdb::explore::explore(db, vin, vout, iout, isolated, None)?;
    Ok(serde_json::to_value(&result)?)
}

fn tool_workflow(db: &ComponentDb, args: &Value) -> Result<Value> {
    let goal = args["goal"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing goal"))?;
    let compositions = kicad_cdb::skill_comp::builtin_compositions();
    let spec = compositions
        .into_iter()
        .find(|c| c.name == goal)
        .ok_or_else(|| anyhow::anyhow!("Unknown workflow: {}", goal))?;

    let user_params: std::collections::HashMap<String, f64> = args["params"]
        .as_object()
        .map(|obj| {
            obj.iter()
                .filter_map(|(k, v)| v.as_f64().map(|n| (k.clone(), n)))
                .collect()
        })
        .unwrap_or_default();

    let result = kicad_cdb::skill_comp::run_composition(db, &spec, &user_params)?;
    Ok(serde_json::to_value(&result)?)
}

fn tool_design_board(db: &ComponentDb, args: &Value) -> Result<Value> {
    let vin = args["vin"]
        .as_f64()
        .ok_or_else(|| anyhow::anyhow!("Missing vin"))?;
    let vout = args["vout"]
        .as_f64()
        .ok_or_else(|| anyhow::anyhow!("Missing vout"))?;
    let iout = args["iout"]
        .as_f64()
        .ok_or_else(|| anyhow::anyhow!("Missing iout"))?;
    let topology = args["topology"].as_str();

    let result = crate::workflow::run_design_board(db, vin, vout, iout, topology)?;
    Ok(json!({
        "topology": result.topology,
        "schematic_length": result.schematic.len(),
        "summary": result.summary,
    }))
}

fn tool_power_tree(db: &ComponentDb, args: &Value) -> Result<Value> {
    let vin = args["vin"]
        .as_f64()
        .ok_or_else(|| anyhow::anyhow!("Missing vin"))?;
    let outputs_val = args["outputs"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("Missing outputs array"))?;

    let mut outputs = Vec::new();
    for item in outputs_val {
        let vout = item["vout"]
            .as_f64()
            .ok_or_else(|| anyhow::anyhow!("Missing vout in output spec"))?;
        let iout = item["iout"]
            .as_f64()
            .ok_or_else(|| anyhow::anyhow!("Missing iout in output spec"))?;
        let name = item["name"].as_str().map(|s| s.to_string());
        outputs.push(kicad_cdb::power_tree::RailSpec { vout, iout, name });
    }

    let isolated = args["isolated"].as_bool().unwrap_or(false);

    let request = kicad_cdb::power_tree::PowerTreeRequest {
        vin,
        outputs,
        isolated,
    };
    let result = kicad_cdb::power_tree::run_power_tree(db, &request)?;

    let incremental = result.incremental_info.as_ref().map(|info| {
        json!({
            "modules_recomputed": info.modules_recomputed,
            "modules_cached": info.modules_cached,
            "changed_params": info.changed_params,
        })
    });

    Ok(json!({
        "tree": result.tree,
        "module_count": result.tree.len(),
        "validation": result.validation,
        "review": result.review,
        "incremental": incremental,
        "schematic_length": result.schematic.len(),
        "summary": result.summary,
    }))
}

fn tool_render_schematic(args: &Value) -> Result<Value> {
    let input = args["input"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing input path"))?;
    let output = args["output"].as_str();

    let sch_text = std::fs::read_to_string(input)?;
    let svg = crate::workflow::render_schematic_to_svg(&sch_text)?;

    if let Some(out_path) = output {
        std::fs::write(out_path, &svg)?;
        Ok(json!({ "output": out_path, "size": svg.len() }))
    } else {
        Ok(json!({ "svg": svg }))
    }
}

fn tool_generate_pcb(_db: &ComponentDb, args: &Value) -> Result<Value> {
    let sch_text = args["schematic"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing schematic text"))?;

    // Parse the .kicad_sch text into Schematic IR
    let schematic = kicad_json5::parse_schematic(sch_text, kicad_json5::InputFormat::Sexpr)?;

    // Convert schematic → Board IR
    let board = kicad_cdb::design::schematic_to_board(&schematic)?;

    // Run built-in DRC
    let directives = kicad_cdb::layout_directives::LayoutDirectives::default();
    let drc_report = kicad_cdb::drc::builtin_drc(&board, Some(&directives));

    // Generate .kicad_pcb S-expression
    let pcb_text = kicad_cdb::design::generate_kicad_pcb(&board)?;

    Ok(json!({
        "pcb": pcb_text,
        "footprint_count": board.footprints.len(),
        "net_count": board.nets.len(),
        "zone_count": board.zones.len(),
        "board_size": format!("{:.1}x{:.1}mm",
            board.footprints.iter().map(|f| f.position.0).fold(f64::MIN, f64::max)
                - board.footprints.iter().map(|f| f.position.0).fold(f64::MAX, f64::min) + 16.0,
            board.footprints.iter().map(|f| f.position.1).fold(f64::MIN, f64::max)
                - board.footprints.iter().map(|f| f.position.1).fold(f64::MAX, f64::min) + 16.0,
        ),
        "drc": {
            "errors": drc_report.summary.errors,
            "warnings": drc_report.summary.warnings,
            "violations": drc_report.violations.iter().map(|v| json!({
                "type": v.violation_type,
                "description": v.description,
                "severity": match v.severity {
                    kicad_cdb::drc::DrcSeverity::Error => "error",
                    kicad_cdb::drc::DrcSeverity::Warning => "warning",
                    kicad_cdb::drc::DrcSeverity::Exclusion => "exclusion",
                },
            })).collect::<Vec<_>>(),
        },
    }))
}

fn tool_check_pcb(_db: &ComponentDb, args: &Value) -> Result<Value> {
    let sch_text = args["schematic"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing schematic text"))?;

    let schematic = kicad_json5::parse_schematic(sch_text, kicad_json5::InputFormat::Sexpr)?;
    let board = kicad_cdb::design::schematic_to_board(&schematic)?;
    let report = kicad_cdb::drc::builtin_drc(&board, None);

    Ok(serde_json::to_value(&report)?)
}

fn tool_generate_symbol(db: &ComponentDb, args: &Value) -> Result<Value> {
    let limit = args["limit"].as_u64().unwrap_or(10) as usize;

    let components = if let Some(mpn) = args["mpn"].as_str() {
        kicad_cdb::service::query_filtered(db, Some(mpn), None, None, None, None, false, Some(1))?
    } else if let Some(search) = args["search"].as_str() {
        kicad_cdb::service::query_filtered(
            db,
            Some(search),
            None,
            None,
            None,
            None,
            false,
            Some(limit),
        )?
    } else {
        anyhow::bail!("Must provide 'mpn' or 'search'");
    };

    if components.is_empty() {
        return Ok(json!({"error": "No components found", "symbol": ""}));
    }

    let lib_content = kicad_cdb::symgen::generate_rich_symbol_lib(&components, db)?;

    Ok(json!({
        "symbol": lib_content,
        "component_count": components.len(),
        "components": components.iter().map(|c| json!({
            "mpn": c.mpn,
            "manufacturer": c.manufacturer,
            "description": c.description,
        })).collect::<Vec<_>>(),
    }))
}

fn tool_export_gerber(args: &Value) -> Result<Value> {
    let pcb_path = args["pcb_path"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing pcb_path"))?;
    let output_dir = args["output_dir"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing output_dir"))?;

    let cfg = kicad_cdb::config::AppConfig::load()?;

    // Create output dir if needed
    std::fs::create_dir_all(output_dir)
        .with_context(|| format!("Failed to create output dir '{}'", output_dir))?;

    let gerber_files = kicad_cdb::design::export_gerber(&cfg.kicad_cli_path, pcb_path, output_dir)?;
    let drill_result = kicad_cdb::design::export_drill(&cfg.kicad_cli_path, pcb_path, output_dir);

    let mut result = json!({
        "gerber_files": gerber_files,
        "file_count": gerber_files.len(),
    });

    if let Ok((pth, npth)) = drill_result {
        result["drill_pth"] = json!(pth);
        result["drill_npth"] = json!(npth);
    }

    Ok(result)
}

fn tool_list_ref_designs(db: &ComponentDb, args: &Value) -> Result<Value> {
    let tag = args["tag"].as_str();
    let designs = db.list_reference_designs(tag)?;
    Ok(json!({
        "designs": designs.iter().map(|(id, name, desc, tags, verified)| json!({
            "id": id,
            "name": name,
            "description": desc,
            "tags": tags,
            "verified": verified,
        })).collect::<Vec<_>>(),
        "count": designs.len(),
    }))
}

fn tool_get_ref_design(db: &ComponentDb, args: &Value) -> Result<Value> {
    let name = args["name"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing design name"))?;
    let design = db
        .get_reference_design(name)?
        .ok_or_else(|| anyhow::anyhow!("Design '{}' not found", name))?;
    Ok(serde_json::to_value(&design)?)
}

fn tool_save_ref_design(db: &ComponentDb, args: &Value) -> Result<Value> {
    let name = args["name"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing design name"))?;
    let description = args["description"].as_str().unwrap_or("");
    let tags = args["tags"].as_str().unwrap_or("");
    let topology = args["topology"].as_str().unwrap_or("");
    let requirements = args["requirements"].as_str().unwrap_or("");
    let schematic = args["schematic"].as_str().unwrap_or("");
    let parameters = args["parameters"].as_str().unwrap_or("");
    let verified = args["verified"].as_bool().unwrap_or(false);

    let id = db.save_reference_design(
        name,
        description,
        tags,
        topology,
        requirements,
        schematic,
        parameters,
        verified,
    )?;
    Ok(json!({"id": id, "name": name, "saved": true}))
}

fn tool_find_alternatives(db: &ComponentDb, args: &Value) -> Result<Value> {
    let mpn = args["mpn"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing mpn"))?;
    let limit = args["limit"].as_u64().unwrap_or(5) as usize;

    let alternatives = kicad_cdb::service::find_alternatives(db, mpn, limit)?;

    Ok(json!({
        "original_mpn": mpn,
        "alternatives": alternatives.iter().map(|c| json!({
            "mpn": c.mpn,
            "manufacturer": c.manufacturer,
            "package": c.package,
            "description": c.description,
            "lifecycle": c.lifecycle,
        })).collect::<Vec<_>>(),
        "count": alternatives.len(),
    }))
}

fn tool_estimate_bom_cost(db: &ComponentDb, args: &Value) -> Result<Value> {
    let items = args["items"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("Missing items array"))?;

    let bom_items: Vec<(String, u32)> = items
        .iter()
        .filter_map(|item| {
            let mpn = item["mpn"].as_str()?;
            let qty = item["quantity"].as_u64()? as u32;
            Some((mpn.to_string(), qty))
        })
        .collect();

    if bom_items.is_empty() {
        anyhow::bail!("No valid items in BOM");
    }

    let result = kicad_cdb::service::estimate_bom_cost(db, &bom_items)?;
    Ok(serde_json::to_value(&result)?)
}

fn tool_design_ic_board(db: &ComponentDb, args: &Value) -> Result<Value> {
    let name = args["name"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing name"))?;
    let power_input_voltage = args["power_input_voltage"]
        .as_f64()
        .ok_or_else(|| anyhow::anyhow!("Missing power_input_voltage"))?;

    let core_ics_val = args["core_ics"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("Missing core_ics array"))?;
    let mut core_ics = Vec::new();
    for ic in core_ics_val {
        let ic_type_str = ic["ic_type"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing ic_type in core_ic"))?;
        let ic_type = match ic_type_str {
            "mcu" => kicad_cdb::requirement::IcType::Mcu,
            "op_amp" => kicad_cdb::requirement::IcType::OpAmp,
            "adc" => kicad_cdb::requirement::IcType::Adc,
            "dac" => kicad_cdb::requirement::IcType::Dac,
            "usb_uart" => kicad_cdb::requirement::IcType::UsbUart,
            "can" => kicad_cdb::requirement::IcType::Can,
            "sensor" => kicad_cdb::requirement::IcType::Sensor,
            "driver" => kicad_cdb::requirement::IcType::Driver,
            other => kicad_cdb::requirement::IcType::Custom(other.to_string()),
        };
        let mpn = ic["mpn"].as_str().map(|s| s.to_string());
        let mut parameters = std::collections::HashMap::new();
        if let Some(obj) = ic["parameters"].as_object() {
            for (k, v) in obj {
                if let Some(n) = v.as_f64() {
                    parameters.insert(k.clone(), n);
                }
            }
        }
        core_ics.push(kicad_cdb::requirement::IcRequest {
            ic_type,
            mpn,
            parameters,
        });
    }

    let mut interfaces = Vec::new();
    if let Some(iface_arr) = args["interfaces"].as_array() {
        for iface in iface_arr {
            let iface_type_str = iface["type"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing type in interface"))?;
            let interface_type = match iface_type_str {
                "usb" => kicad_cdb::requirement::InterfaceType::Usb,
                "uart" => kicad_cdb::requirement::InterfaceType::Uart,
                "spi" => kicad_cdb::requirement::InterfaceType::Spi,
                "i2c" => kicad_cdb::requirement::InterfaceType::I2c,
                "swd" => kicad_cdb::requirement::InterfaceType::Swd,
                "jtag" => kicad_cdb::requirement::InterfaceType::Jtag,
                "can" => kicad_cdb::requirement::InterfaceType::Can,
                "gpio" => kicad_cdb::requirement::InterfaceType::Gpio,
                "power_in" => kicad_cdb::requirement::InterfaceType::PowerIn,
                "power_out" => kicad_cdb::requirement::InterfaceType::PowerOut,
                "analog" => kicad_cdb::requirement::InterfaceType::Analog,
                other => kicad_cdb::requirement::InterfaceType::Custom(other.to_string()),
            };
            let role = iface["role"].as_str().unwrap_or("").to_string();
            let mut parameters = std::collections::HashMap::new();
            if let Some(obj) = iface["parameters"].as_object() {
                for (k, v) in obj {
                    if let Some(n) = v.as_f64() {
                        parameters.insert(k.clone(), n);
                    }
                }
            }
            interfaces.push(kicad_cdb::requirement::InterfaceReq {
                interface_type,
                role,
                parameters,
            });
        }
    }

    let mut constraints = Vec::new();
    if let Some(c_arr) = args["constraints"].as_array() {
        for c in c_arr {
            let cname = c["name"].as_str().unwrap_or("").to_string();
            let cvalue = c["value"].as_f64().unwrap_or(0.0);
            let cunit = c["unit"].as_str().unwrap_or("").to_string();
            constraints.push(kicad_cdb::requirement::DesignConstraint {
                name: cname,
                value: cvalue,
                unit: cunit,
            });
        }
    }

    let spec = kicad_cdb::requirement::DesignSpec::IcBoard(kicad_cdb::requirement::IcBoardSpec {
        name: name.to_string(),
        core_ics,
        interfaces,
        power_input_voltage,
        constraints,
    });

    let validation = kicad_cdb::requirement::validate_spec(&spec);
    let knowledge = kicad_cdb::requirement::IcKnowledge::new();
    let mut assembly = kicad_cdb::requirement::spec_to_composition(&spec, &knowledge)?;

    // Optional: merge power tree modules into composition
    let generate_sch = args["generate_schematic"].as_bool().unwrap_or(true);
    let mut schematic_result: Option<String> = None;

    if generate_sch {
        // Merge power tree topology modules if power_request exists
        if let Some(ref power_req) = assembly.power_request {
            match kicad_cdb::power_tree::run_power_tree(db, power_req) {
                Ok(power_result) => {
                    for (i, node) in power_result.tree.iter().enumerate() {
                        let mut nets = std::collections::HashMap::new();
                        nets.insert("input".into(), node.input_net.clone());
                        nets.insert("output".into(), node.output_net.clone());
                        let pipeline_outputs = node.pipeline_outputs.clone();

                        assembly
                            .composition
                            .modules
                            .push(kicad_cdb::composition::ModuleInstance {
                                id: format!("power_{}", i),
                                template: node.template_name.clone(),
                                template_type: "topology".into(),
                                params: pipeline_outputs.clone(),
                                nets,
                                y_offset: Some(node.y_offset),
                                topology_inputs: Some(kicad_cdb::composition::TopologyInputs {
                                    vin: node.vin,
                                    vout: node.vout,
                                    iout: node.iout,
                                }),
                                computed_values: pipeline_outputs,
                            });
                    }
                }
                Err(e) => {
                    assembly.warnings.push(format!("电源树生成失败: {}", e));
                }
            }
        }

        // Generate schematic from the merged composition
        match kicad_cdb::design::generate_composed_schematic(db, &assembly.composition) {
            Ok(sch) => {
                schematic_result = Some(sch);
            }
            Err(e) => {
                assembly.warnings.push(format!(
                    "原理图生成失败: {}（Composition 已返回，可手动调用 compose）",
                    e
                ));
            }
        }
    }

    Ok(json!({
        "composition": {
            "name": assembly.composition.name,
            "description": assembly.composition.description,
            "module_count": assembly.composition.modules.len(),
            "modules": assembly.composition.modules.iter().map(|m| json!({
                "id": m.id,
                "template": m.template,
                "template_type": m.template_type,
            })).collect::<Vec<_>>(),
            "global_nets": assembly.composition.global_nets.iter().map(|n| &n.name).collect::<Vec<_>>(),
        },
        "power_request": assembly.power_request.map(|pr| json!({
            "vin": pr.vin,
            "outputs": pr.outputs,
        })),
        "peripheral_summary": assembly.peripheral_summary,
        "total_component_count": assembly.total_component_count,
        "schematic_generated": schematic_result.is_some(),
        "schematic_length": schematic_result.as_ref().map(|s| s.len()),
        "validation": validation,
        "warnings": assembly.warnings,
    }))
}

fn tool_list_ic_types() -> Result<Value> {
    let knowledge = kicad_cdb::requirement::IcKnowledge::new();
    let types = knowledge.list_supported_types();
    let mut type_info = Vec::new();
    for t in &types {
        let rule = knowledge.get_rule(t);
        let info = json!({
            "type": format!("{}", t),
            "peripheral_count": rule.map(|r| r.peripherals.len()).unwrap_or(0),
            "interface_count": rule.map(|r| r.interfaces.len()).unwrap_or(0),
            "power_rail_count": rule.map(|r| r.power_rails.len()).unwrap_or(0),
        });
        type_info.push(info);
    }
    Ok(json!({
        "supported_types": type_info,
        "total": type_info.len(),
    }))
}

fn tool_save_design(_db: &ComponentDb, args: &Value) -> Result<Value> {
    let name = args["name"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing name"))?;
    let spec: kicad_cdb::requirement::DesignSpec = serde_json::from_value(args["spec"].clone())
        .map_err(|e| anyhow::anyhow!("Invalid spec: {}", e))?;

    let composition = if !args["composition"].is_null() {
        serde_json::from_value(args["composition"].clone()).ok()
    } else {
        None
    };
    let power_tree_nodes: Vec<kicad_cdb::power_tree::TreeNode> = if args["power_tree"].is_array() {
        serde_json::from_value(args["power_tree"].clone()).unwrap_or_default()
    } else {
        vec![]
    };
    let schematic = args["schematic"].as_str().map(|s| s.to_string());

    let id = kicad_cdb::design_iteration::generate_new_uuid();
    let now = format!(
        "{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    );

    let snapshot = kicad_cdb::design_iteration::DesignSnapshot {
        id: id.clone(),
        name: name.to_string(),
        version: 1,
        created_at: now,
        spec,
        composition: composition.unwrap_or_else(|| kicad_cdb::composition::Composition {
            name: name.to_string(),
            description: String::new(),
            modules: vec![],
            global_nets: vec![],
        }),
        power_tree_nodes,
        module_pipeline_results: std::collections::HashMap::new(),
        schematic,
    };

    kicad_cdb::design_iteration::save_snapshot(&snapshot)?;

    Ok(json!({
        "design_id": id,
        "name": snapshot.name,
        "version": snapshot.version,
        "module_count": snapshot.composition.modules.len(),
    }))
}

fn tool_load_design(args: &Value) -> Result<Value> {
    let design_id = args["design_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing design_id"))?;

    let snapshot = kicad_cdb::design_iteration::load_snapshot(design_id)?;

    Ok(json!({
        "id": snapshot.id,
        "name": snapshot.name,
        "version": snapshot.version,
        "created_at": snapshot.created_at,
        "spec": snapshot.spec,
        "module_count": snapshot.composition.modules.len(),
        "power_tree_nodes": snapshot.power_tree_nodes.len(),
        "has_schematic": snapshot.schematic.is_some(),
    }))
}

fn tool_update_design(db: &ComponentDb, args: &Value) -> Result<Value> {
    let design_id = args["design_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing design_id"))?;

    let changes_obj = args["changes"]
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("Missing changes object"))?;
    let mut changes = std::collections::HashMap::new();
    for (k, v) in changes_obj {
        if let Some(n) = v.as_f64() {
            changes.insert(k.clone(), n);
        }
    }
    if changes.is_empty() {
        anyhow::bail!("No valid numeric changes provided");
    }

    let snapshot = kicad_cdb::design_iteration::load_snapshot(design_id)?;
    let (new_snapshot, diff) = kicad_cdb::design_iteration::update_design(db, &snapshot, &changes)?;

    // Optionally regenerate schematic
    let generate_sch = args["generate_schematic"].as_bool().unwrap_or(true);
    let mut schematic_generated = false;
    let mut schematic_length: Option<usize> = None;

    if generate_sch {
        if let Ok(sch) =
            kicad_cdb::design::generate_composed_schematic(db, &new_snapshot.composition)
        {
            schematic_length = Some(sch.len());
            schematic_generated = true;
        }
    }

    // Save updated snapshot
    kicad_cdb::design_iteration::save_snapshot(&new_snapshot)?;

    Ok(json!({
        "design_id": new_snapshot.id,
        "version": new_snapshot.version,
        "changed_params": diff.changed_params,
        "affected_modules": diff.affected_modules,
        "unaffected_modules": diff.unaffected_modules,
        "modules_recomputed": diff.modules_recomputed,
        "modules_cached": diff.modules_cached,
        "schematic_generated": schematic_generated,
        "schematic_length": schematic_length,
    }))
}

fn tool_list_designs() -> Result<Value> {
    let designs = kicad_cdb::design_iteration::list_snapshots()?;
    Ok(json!({
        "designs": designs.iter().map(|(id, name, version, updated)| json!({
            "id": id,
            "name": name,
            "version": version,
            "updated_at": updated,
        })).collect::<Vec<_>>(),
        "total": designs.len(),
    }))
}

/// H1: List all versions of a design for rollback selection.
fn tool_list_design_versions(args: &Value) -> Result<Value> {
    let design_id = args["design_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing design_id"))?;
    let versions = kicad_cdb::design_iteration::list_snapshot_versions(design_id)?;
    Ok(json!({
        "design_id": design_id,
        "versions": versions.iter().map(|(v, created, updated)| json!({
            "version": v,
            "created_at": created,
            "updated_at": updated,
        })).collect::<Vec<_>>(),
        "total": versions.len(),
    }))
}

/// H1: Restore a specific version of a design (rollback).
fn tool_restore_design_version(args: &Value) -> Result<Value> {
    let design_id = args["design_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing design_id"))?;
    let version = args["version"]
        .as_i64()
        .ok_or_else(|| anyhow::anyhow!("Missing version"))? as u32;
    let snapshot = kicad_cdb::design_iteration::load_snapshot_version(design_id, version)?;
    Ok(json!({
        "id": snapshot.id,
        "name": snapshot.name,
        "version": snapshot.version,
        "created_at": snapshot.created_at,
        "spec": snapshot.spec,
        "module_count": snapshot.composition.modules.len(),
        "power_tree_nodes": snapshot.power_tree_nodes.len(),
        "has_schematic": snapshot.schematic.is_some(),
    }))
}

/// H5: Generate HTML design review report from a DesignReviewResult JSON.
fn tool_review_report_html(args: &Value) -> Result<Value> {
    let review: kicad_cdb::design_review::DesignReviewResult =
        serde_json::from_value(args["review_json"].clone())
            .map_err(|e| anyhow::anyhow!("Invalid review_json: {}", e))?;
    let title = args["title"].as_str().unwrap_or("Design Review");
    let html = kicad_cdb::design_review::render_html_report(&review, title);
    Ok(json!({
        "html": html,
        "passed": review.passed,
        "issue_count": review.issues.len(),
    }))
}

/// P1-5: Load hierarchical project and validate sheet interfaces.
fn tool_check_hierarchy(args: &Value) -> Result<Value> {
    let path = args["root_sch_path"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing root_sch_path"))?;
    let project =
        kicad_json5::hierarchy::HierarchicalProject::load_from_file(std::path::Path::new(path))?;
    let check = kicad_json5::hierarchy::check_sheet_interfaces(&project);
    Ok(json!({
        "total_sheets": project.total_sheets(),
        "sub_sheet_count": project.sub_sheets.len(),
        "passed": check.passed,
        "findings": check.findings,
        "finding_count": check.findings.len(),
    }))
}

// ---------------------------------------------------------------------------
// AI-assisted layout/routing tools (Phase 1)
// ---------------------------------------------------------------------------

fn tool_analyze_pcb(args: &Value) -> Result<Value> {
    let pcb_path = args["pcb_path"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing pcb_path"))?;

    let source = std::fs::read_to_string(pcb_path)
        .with_context(|| format!("Failed to read {}", pcb_path))?;
    let board = kicad_json5::parse_board(&source).with_context(|| "Failed to parse .kicad_pcb")?;

    // Board dimensions
    let xs: Vec<f64> = board.footprints.iter().map(|f| f.position.0).collect();
    let ys: Vec<f64> = board.footprints.iter().map(|f| f.position.1).collect();
    let (min_x, max_x) = xs
        .iter()
        .cloned()
        .fold((f64::MAX, f64::MIN), |(a, b), v| (a.min(v), b.max(v)));
    let (min_y, max_y) = ys
        .iter()
        .cloned()
        .fold((f64::MAX, f64::MIN), |(a, b), v| (a.min(v), b.max(v)));
    let board_w = max_x - min_x;
    let board_h = max_y - min_y;

    // Net statistics — collect net id→name mapping first
    let net_id_to_name: std::collections::HashMap<u32, String> =
        board.nets.iter().map(|n| (n.id, n.name.clone())).collect();

    let mut nets_by_size: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for fp in &board.footprints {
        for pad in &fp.pads {
            if let Some(net_id) = pad.net {
                let net_name = net_id_to_name
                    .get(&net_id)
                    .cloned()
                    .unwrap_or_else(|| format!("net_{}", net_id));
                *nets_by_size.entry(net_name).or_insert(0) += 1;
            }
        }
    }

    let mut net_list: Vec<Value> = nets_by_size
        .iter()
        .map(|(name, count)| json!({ "name": name, "pad_count": count }))
        .collect();
    net_list.sort_by(|a, b| b["pad_count"].as_u64().cmp(&a["pad_count"].as_u64()));

    // Footprint details
    let footprint_list: Vec<Value> = board
        .footprints
        .iter()
        .map(|fp| {
            let (x, y, rot) = fp.position;
            let pad_nets: Vec<String> = fp
                .pads
                .iter()
                .filter_map(|p| p.net.and_then(|nid| net_id_to_name.get(&nid).cloned()))
                .collect();
            json!({
                "reference": fp.reference,
                "value": fp.value,
                "footprint": fp.lib_id,
                "position": { "x": x, "y": y, "rotation": rot },
                "pad_count": fp.pads.len(),
                "nets": pad_nets,
            })
        })
        .collect();

    // Segment statistics
    let seg_by_layer: std::collections::HashMap<String, usize> = board
        .segments
        .iter()
        .filter(|s| s.width > 0.0)
        .fold(std::collections::HashMap::new(), |mut acc, s| {
            *acc.entry(s.layer.clone()).or_insert(0) += 1;
            acc
        });

    let seg_by_net: std::collections::HashMap<String, usize> = board
        .segments
        .iter()
        .filter(|s| s.width > 0.0)
        .fold(std::collections::HashMap::new(), |mut acc, s| {
            let name = net_id_to_name
                .get(&s.net)
                .cloned()
                .unwrap_or_else(|| format!("net_{}", s.net));
            *acc.entry(name).or_insert(0) += 1;
            acc
        });

    // Density analysis: 5mm grid cells, count footprints per cell
    let cell_size = 5.0;
    let cols = ((board_w / cell_size).ceil() as usize).max(1);
    let rows = ((board_h / cell_size).ceil() as usize).max(1);
    let mut density_grid = vec![0u32; cols * rows];
    for fp in &board.footprints {
        let col = ((fp.position.0 - min_x) / cell_size).floor() as usize;
        let row = ((fp.position.1 - min_y) / cell_size).floor() as usize;
        if col < cols && row < rows {
            density_grid[row * cols + col] += 1;
        }
    }
    let max_density = *density_grid.iter().max().unwrap_or(&0);
    let total_cells = cols * rows;
    let occupied_cells = density_grid.iter().filter(|&&d| d > 0).count();
    let avg_density = board.footprints.len() as f64 / total_cells as f64;

    // Top 10 nets by segment count
    let mut seg_vec: Vec<(String, usize)> = seg_by_net.into_iter().collect();
    seg_vec.sort_by_key(|&(_, n)| std::cmp::Reverse(n));
    let top10_nets: Vec<Value> = seg_vec
        .into_iter()
        .take(10)
        .map(|(k, v)| json!({ "net": k, "segments": v }))
        .collect();

    Ok(json!({
        "board": {
            "width_mm": format!("{:.1}", board_w),
            "height_mm": format!("{:.1}", board_h),
            "area_mm2": format!("{:.0}", board_w * board_h),
            "dimensions": format!("{:.1} x {:.1}mm", board_w, board_h),
        },
        "statistics": {
            "footprint_count": board.footprints.len(),
            "net_count": board.nets.len(),
            "segment_count": board.segments.len(),
            "via_count": board.vias.len(),
            "zone_count": board.zones.len(),
        },
        "density": {
            "grid_cell_mm": cell_size,
            "grid_cols": cols,
            "grid_rows": rows,
            "max_footprints_per_cell": max_density,
            "avg_density": format!("{:.2}", avg_density),
            "occupancy": format!("{:.0}%", occupied_cells as f64 / total_cells as f64 * 100.0),
        },
        "segments_by_layer": seg_by_layer,
        "segments_by_net_top10": top10_nets,
        "nets": net_list,
        "footprints": footprint_list,
    }))
}

fn tool_move_footprint(args: &Value) -> Result<Value> {
    let pcb_path = args["pcb_path"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing pcb_path"))?;
    let reference = args["reference"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing reference"))?;
    let new_x = args["x"]
        .as_f64()
        .ok_or_else(|| anyhow::anyhow!("Missing x"))?;
    let new_y = args["y"]
        .as_f64()
        .ok_or_else(|| anyhow::anyhow!("Missing y"))?;
    let new_rot = args["rotation"].as_f64();
    let rip_traces = args["rip_traces"].as_bool().unwrap_or(true);

    let source = std::fs::read_to_string(pcb_path)
        .with_context(|| format!("Failed to read {}", pcb_path))?;
    let mut board =
        kicad_json5::parse_board(&source).with_context(|| "Failed to parse .kicad_pcb")?;

    // Find the footprint index first to avoid borrow conflicts
    let fp_idx = board
        .footprints
        .iter()
        .position(|f| f.reference == reference)
        .ok_or_else(|| anyhow::anyhow!("Footprint '{}' not found", reference))?;

    let old_x = board.footprints[fp_idx].position.0;
    let old_y = board.footprints[fp_idx].position.1;
    let old_rot = board.footprints[fp_idx].position.2;
    board.footprints[fp_idx].position.0 = new_x;
    board.footprints[fp_idx].position.1 = new_y;
    if let Some(rot) = new_rot {
        board.footprints[fp_idx].position.2 = rot;
    }

    // Optionally rip-up traces connected to this footprint's nets
    let mut ripped_segments = 0usize;
    let mut ripped_vias = 0usize;
    if rip_traces {
        let fp_net_ids: std::collections::HashSet<u32> = board.footprints[fp_idx]
            .pads
            .iter()
            .filter_map(|p| p.net)
            .collect();

        let before_segs = board.segments.len();
        let before_vias = board.vias.len();
        board.segments.retain(|s| !fp_net_ids.contains(&s.net));
        board.vias.retain(|v| !fp_net_ids.contains(&v.net));
        ripped_segments = before_segs - board.segments.len();
        ripped_vias = before_vias - board.vias.len();
    }

    // Regenerate the .kicad_pcb file
    let pcb_text = kicad_cdb::design::generate_kicad_pcb(&board)?;
    std::fs::write(pcb_path, &pcb_text).with_context(|| format!("Failed to write {}", pcb_path))?;

    Ok(json!({
        "reference": reference,
        "old_position": { "x": old_x, "y": old_y, "rotation": old_rot },
        "new_position": { "x": new_x, "y": new_y, "rotation": board.footprints[fp_idx].position.2 },
        "ripped_segments": ripped_segments,
        "ripped_vias": ripped_vias,
        "updated": true,
    }))
}

fn tool_reroute_nets(args: &Value) -> Result<Value> {
    let pcb_path = args["pcb_path"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing pcb_path"))?;
    let output_path = args["output_path"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing output_path"))?;
    let layers = args["layers"].as_u64().unwrap_or(4) as usize;
    let target_nets: Vec<String> = args["nets"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    // Use the existing reroute command
    crate::commands::cmd_reroute(pcb_path, output_path, layers, None, None, None, false)?;

    // If specific nets were requested, verify they were routed
    if !target_nets.is_empty() {
        let result_source =
            std::fs::read_to_string(output_path).with_context(|| "Failed to read output PCB")?;
        let result_board = kicad_json5::parse_board(&result_source)
            .with_context(|| "Failed to parse output PCB")?;

        let routed_nets: Vec<String> = result_board
            .segments
            .iter()
            .filter(|s| s.width > 0.0)
            .filter_map(|s| {
                result_board
                    .nets
                    .iter()
                    .find(|n| n.id == s.net)
                    .map(|n| n.name.clone())
            })
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        let target_routed: Vec<&str> = target_nets
            .iter()
            .filter(|n| routed_nets.iter().any(|r| r == *n))
            .map(|n| n.as_str())
            .collect();
        let target_failed: Vec<&str> = target_nets
            .iter()
            .filter(|n| !routed_nets.iter().any(|r| r == *n))
            .map(|n| n.as_str())
            .collect();

        Ok(json!({
            "total_nets": result_board.nets.len(),
            "segments": result_board.segments.len(),
            "vias": result_board.vias.len(),
            "target_nets": target_nets,
            "target_routed": target_routed,
            "target_failed": target_failed,
        }))
    } else {
        let result_source =
            std::fs::read_to_string(output_path).with_context(|| "Failed to read output PCB")?;
        let result_board = kicad_json5::parse_board(&result_source)
            .with_context(|| "Failed to parse output PCB")?;

        Ok(json!({
            "total_nets": result_board.nets.len(),
            "segments": result_board.segments.len(),
            "vias": result_board.vias.len(),
        }))
    }
}

fn tool_open_in_kicad(args: &Value) -> Result<Value> {
    let pcb_path = args["pcb_path"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing pcb_path"))?;

    // Verify file exists
    std::fs::metadata(pcb_path).with_context(|| format!("PCB file not found: {}", pcb_path))?;

    // Open with system default application (KiCad)
    #[cfg(target_os = "macos")]
    let status = std::process::Command::new("open").arg(pcb_path).status();

    #[cfg(target_os = "linux")]
    let status = std::process::Command::new("xdg-open")
        .arg(pcb_path)
        .status();

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let status = std::process::Command::new("cmd")
        .args(["/C", "start", "", pcb_path])
        .status();

    let status = status.with_context(|| "Failed to launch KiCad")?;
    Ok(json!({
        "pcb_path": pcb_path,
        "opened": status.success(),
    }))
}

/// Find nets connected to a footprint by reference designator
#[allow(dead_code)] // MCP 工具面预留查询原语
fn nets_of_footprint(board: &kicad_json5::ir::board::Board, reference: &str) -> Vec<u32> {
    let fp = match board.footprints.iter().find(|f| f.reference == reference) {
        Some(f) => f,
        None => return vec![],
    };
    fp.pads
        .iter()
        .filter_map(|p| p.net)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect()
}

fn tool_smart_fix(args: &Value) -> Result<Value> {
    let pcb_path = args["pcb_path"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing pcb_path"))?;
    let output_path = args["output_path"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing output_path"))?;
    let _max_iterations = args["max_iterations"].as_u64().unwrap_or(3) as usize;
    let layers = args["layers"].as_u64().unwrap_or(4) as usize;

    // Step 1: Copy input to output as starting point
    let source = std::fs::read_to_string(pcb_path)
        .with_context(|| format!("Failed to read {}", pcb_path))?;
    std::fs::write(output_path, &source)
        .with_context(|| format!("Failed to write {}", output_path))?;

    let cfg = kicad_cdb::config::AppConfig::load().ok();
    let kicad_cli = cfg
        .as_ref()
        .map(|c| c.kicad_cli_path.as_str())
        .unwrap_or("kicad-cli");

    // Run initial DRC if kicad-cli is available
    let initial_drc = if std::path::Path::new(kicad_cli).exists() || which_exists(kicad_cli) {
        kicad_cdb::drc::run_drc(kicad_cli, output_path).ok()
    } else {
        None
    };

    let initial_errors = initial_drc.as_ref().map(|r| r.summary.errors).unwrap_or(0);
    let initial_warnings = initial_drc
        .as_ref()
        .map(|r| r.summary.warnings)
        .unwrap_or(0);

    let mut fix_log: Vec<Value> = Vec::new();

    // Step 2: Parse board and analyze DRC violations
    let board = kicad_json5::parse_board(&source).with_context(|| "Failed to parse .kicad_pcb")?;

    // Build net id→name mapping
    let net_id_to_name: std::collections::HashMap<u32, String> =
        board.nets.iter().map(|n| (n.id, n.name.clone())).collect();

    // Analyze violations if we have DRC report
    let violation_nets: Vec<String> = if let Some(ref drc) = initial_drc {
        // Extract net names from violation descriptions
        let mut nets = std::collections::HashSet::new();
        for v in &drc.violations {
            // Try to match net names in descriptions like "Net 'VCC' and 'GND'"
            for item in &v.items {
                for (id, name) in &net_id_to_name {
                    if item.description.contains(name.as_str())
                        || item.description.contains(&format!("net_{}", id))
                    {
                        nets.insert(name.clone());
                    }
                }
            }
            // Also check violation description itself
            for name in net_id_to_name.values() {
                if v.description.contains(name.as_str()) {
                    nets.insert(name.clone());
                }
            }
        }
        nets.into_iter().collect()
    } else {
        vec![]
    };

    // Step 3: If we have DRC violations, try rerouting
    if initial_errors > 0 {
        eprintln!(
            "[smart_fix] Initial DRC: {} errors, {} warnings",
            initial_errors, initial_warnings
        );
        eprintln!("[smart_fix] Violation nets: {:?}", violation_nets);

        if !violation_nets.is_empty() {
            fix_log.push(json!({
                "action": "identified_violations",
                "error_nets": violation_nets.len(),
                "nets": violation_nets,
            }));
        }

        // Try rerouting the board — this is the most effective fix
        eprintln!("[smart_fix] Rerouting board ({} layers)...", layers);
        match crate::commands::cmd_reroute(
            output_path,
            output_path,
            layers,
            None,
            None,
            None,
            false,
        ) {
            Ok(()) => {
                fix_log.push(json!({ "action": "reroute", "result": "success" }));
            }
            Err(e) => {
                fix_log.push(
                    json!({ "action": "reroute", "result": "failed", "error": e.to_string() }),
                );
            }
        }

        // Step 4: Run DRC again to verify improvement
        let final_drc = if std::path::Path::new(kicad_cli).exists() || which_exists(kicad_cli) {
            kicad_cdb::drc::run_drc(kicad_cli, output_path).ok()
        } else {
            None
        };

        let final_errors = final_drc.as_ref().map(|r| r.summary.errors).unwrap_or(0);
        let final_warnings = final_drc.as_ref().map(|r| r.summary.warnings).unwrap_or(0);

        let improved = final_errors < initial_errors;

        fix_log.push(json!({
            "action": "drc_verify",
            "initial_errors": initial_errors,
            "final_errors": final_errors,
            "improved": improved,
            "error_delta": initial_errors as i64 - final_errors as i64,
        }));

        Ok(json!({
            "initial_drc": {
                "errors": initial_errors,
                "warnings": initial_warnings,
            },
            "final_drc": {
                "errors": final_errors,
                "warnings": final_warnings,
            },
            "improved": improved,
            "violation_nets_count": violation_nets.len(),
            "fix_log": fix_log,
        }))
    } else {
        Ok(json!({
            "initial_drc": {
                "errors": initial_errors,
                "warnings": initial_warnings,
            },
            "final_drc": {
                "errors": initial_errors,
                "warnings": initial_warnings,
            },
            "improved": false,
            "message": "No DRC errors found, no fixes needed",
            "fix_log": fix_log,
        }))
    }
}

/// Check if a command exists in PATH
fn which_exists(cmd: &str) -> bool {
    std::process::Command::new("which")
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Phase 3: IPC tool implementations
// ---------------------------------------------------------------------------

/// Get the path to the IPC bridge script
fn ipc_bridge_path() -> Option<String> {
    // Try relative to executable, then relative to CWD
    let candidates = [
        "scripts/kicad_ipc_bridge.py",
        "../scripts/kicad_ipc_bridge.py",
    ];
    for c in &candidates {
        if std::path::Path::new(c).exists() {
            return Some(c.to_string());
        }
    }
    None
}

/// Run the IPC bridge script and return parsed JSON output
fn run_ipc_bridge(args: &[&str]) -> Result<Value> {
    let bridge = ipc_bridge_path().ok_or_else(|| anyhow::anyhow!("IPC bridge script not found"))?;

    let output = std::process::Command::new("python3")
        .arg(&bridge)
        .args(args)
        .output()
        .context("Failed to run IPC bridge")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() {
        return Err(anyhow::anyhow!("IPC bridge failed: {}", stderr));
    }

    let result: Value = serde_json::from_str(stdout.trim())
        .context(format!("Invalid JSON from IPC bridge: {}", stdout))?;
    Ok(result)
}

fn tool_ipc_check() -> Result<Value> {
    match run_ipc_bridge(&["check"]) {
        Ok(result) => Ok(json!({
            "ipc_status": result,
            "bridge_available": true,
        })),
        Err(e) => Ok(json!({
            "ipc_status": {
                "bridge_available": false,
                "error": e.to_string(),
            },
            "bridge_available": false,
            "fallback_capabilities": {
                "file_ops": true,
                "cli_drc": which_exists("kicad-cli"),
                "open_in_kicad": which_exists("open"),
            },
        })),
    }
}

fn tool_ipc_move_footprint(args: &Value) -> Result<Value> {
    let pcb_path = args["pcb_path"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing pcb_path"))?;
    let reference = args["reference"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing reference"))?;
    let x = args["x"]
        .as_f64()
        .ok_or_else(|| anyhow::anyhow!("Missing x"))?;
    let y = args["y"]
        .as_f64()
        .ok_or_else(|| anyhow::anyhow!("Missing y"))?;
    let angle = args["angle"].as_f64();

    // Use file-level operation (reliable, no KiCad needed)
    let x_str = format!("{:.4}", x);
    let y_str = format!("{:.4}", y);
    let mut cmd_args = vec!["move_footprint", pcb_path, reference, &x_str, &y_str];

    let angle_str;
    if let Some(a) = angle {
        angle_str = format!("{:.2}", a);
        cmd_args.push(&angle_str);
    }

    let result = run_ipc_bridge(&cmd_args)?;

    // Try to refresh KiCad view if KiCad is running
    if which_exists("osascript") {
        let _ = std::process::Command::new("osascript")
            .arg("-e")
            .arg("tell application \"kicad\" to activate")
            .output();
    }

    Ok(json!({
        "action": "move_footprint",
        "reference": reference,
        "position": { "x": x, "y": y, "angle": angle },
        "result": result,
        "note": "KiCad will need to reload the file to see changes (File > Reload)",
    }))
}

fn tool_ipc_get_board_state(args: &Value) -> Result<Value> {
    let pcb_path = args["pcb_path"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing pcb_path"))?;
    let include_tracks = args["include_tracks"].as_bool().unwrap_or(false);

    // Parse footprints
    let fps = run_ipc_bridge(&["parse_footprints", pcb_path])
        .unwrap_or_else(|e| json!({"error": e.to_string()}));

    let mut result = json!({
        "board_path": pcb_path,
        "footprints": fps,
    });

    if include_tracks {
        let tracks = run_ipc_bridge(&["parse_tracks", pcb_path])
            .unwrap_or_else(|e| json!({"error": e.to_string()}));
        result
            .as_object_mut()
            .unwrap()
            .insert("tracks".to_string(), tracks);
    }

    Ok(result)
}

fn tool_ipc_delete_tracks(args: &Value) -> Result<Value> {
    let pcb_path = args["pcb_path"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing pcb_path"))?;
    let net_name = args["net_name"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing net_name"))?;

    let result = run_ipc_bridge(&["delete_tracks_by_net", pcb_path, net_name])?;

    Ok(json!({
        "action": "delete_tracks",
        "net": net_name,
        "result": result,
        "note": "KiCad will need to reload the file to see changes",
    }))
}

pub fn serve(db: &ComponentDb) -> Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let req: Request = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(e) => {
                let resp = Response {
                    jsonrpc: "2.0",
                    id: None,
                    result: None,
                    error: Some(RpcError {
                        code: -32700,
                        message: format!("Parse error: {}", e),
                    }),
                };
                writeln!(stdout, "{}", serde_json::to_string(&resp)?)?;
                stdout.flush()?;
                continue;
            }
        };

        let id = req.id.clone();

        if req.method == "notifications/initialized" {
            continue;
        }

        let result = dispatch(db, &req.method, req.params);

        let resp = match result {
            Ok(val) => Response {
                jsonrpc: "2.0",
                id,
                result: Some(val),
                error: None,
            },
            Err(e) => Response {
                jsonrpc: "2.0",
                id,
                result: None,
                error: Some(RpcError {
                    code: -32603,
                    message: e.to_string(),
                }),
            },
        };

        writeln!(stdout, "{}", serde_json::to_string(&resp)?)?;
        stdout.flush()?;
    }

    Ok(())
}
