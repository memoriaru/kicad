// v35 编排器: 求解上下文按显式参数传递(领域常态), 存量风格债定向放行
#![allow(clippy::too_many_arguments)]

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use kicad_cdb::ComponentDb;

// jemalloc: actively returns freed memory to OS (unlike macOS malloc which keeps arenas)
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

mod commands;
mod mcp;
mod netlist_diff;
mod v35_flow;
mod workflow;

#[derive(Parser)]
#[command(name = "kdesign", about = "AI-driven circuit design orchestrator")]
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

    /// Import components from a CSV file (batch import)
    ImportCsv {
        /// Path to CSV file (must have header row with 'mpn' column)
        path: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
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
        #[arg(long)]
        category: Option<String>,
        #[arg(long)]
        param: Option<String>,
        #[arg(long)]
        manufacturer: Option<String>,
        #[arg(long)]
        package: Option<String>,
        #[arg(short, long)]
        search: Option<String>,
        #[arg(long)]
        in_stock: bool,
        #[arg(long)]
        lifecycle: Option<String>,
        #[arg(short, long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },

    /// Show component details
    Show {
        mpn: String,
        #[arg(long)]
        json: bool,
    },

    /// List all categories
    Categories {
        #[arg(long)]
        json: bool,
    },

    /// Apply a design rule with given parameters
    Check {
        #[arg(long)]
        rule: String,
        #[arg(long)]
        params: String,
        #[arg(long)]
        candidate: Option<String>,
        #[arg(long)]
        json: bool,
    },

    /// Export component(s) as KiCad .kicad_sym, JSON5 spec, or lib-table
    Export {
        #[arg(long)]
        mpn: Option<String>,
        #[arg(long)]
        category: Option<String>,
        #[arg(long, default_value = "kicad_sym")]
        format: String,
        #[arg(short, long)]
        output: String,
    },

    /// Generate .kicad_sym from DB component (symgen --db --mpn equivalent)
    SymFromDb {
        #[arg(long)]
        db: String,
        #[arg(long)]
        mpn: String,
        #[arg(short, long)]
        output: String,
        #[arg(long)]
        lib_name: Option<String>,
    },

    /// Fetch component from HuaQiu EDA and import into database
    Fetch {
        #[arg(long)]
        mpn: String,
        #[arg(long)]
        mfg_id: Option<String>,
    },

    /// Search HuaQiu online component library
    HqSearch {
        keyword: String,
        #[arg(short, long, default_value = "20")]
        limit: usize,
        #[arg(long)]
        json: bool,
    },

    /// Generate a schematic from a topology template
    Design {
        #[arg(long)]
        template: String,
        #[arg(long)]
        vin: f64,
        #[arg(long)]
        vout: f64,
        #[arg(long)]
        iout: f64,
        #[arg(short, long)]
        output: String,
    },

    /// Suggest suitable power topology based on requirements
    Suggest {
        #[arg(long)]
        vin: f64,
        #[arg(long)]
        vout: f64,
        #[arg(long)]
        iout: f64,
        #[arg(long, default_value = "false")]
        isolated: bool,
        #[arg(long)]
        json: bool,
    },

    /// Compose multiple modules into a single schematic
    Compose {
        #[arg(long)]
        file: String,
        #[arg(short, long)]
        output: String,
    },

    /// Generate a schematic from an IC core template
    IcDesign {
        #[arg(long)]
        template: String,
        #[arg(long)]
        params: String,
        #[arg(long)]
        nets: Option<String>,
        #[arg(short, long)]
        output: String,
    },

    /// Fetch pin list from HuaQiu API for an MPN
    TemplatePins {
        mpn: String,
        #[arg(long)]
        json: bool,
    },

    /// Run a design pipeline (chained rule execution)
    Pipeline {
        name: Option<String>,
        #[arg(long)]
        list: bool,
        #[arg(long)]
        params: Option<String>,
        #[arg(long)]
        json: bool,
    },

    /// List or manage design rules (Skills)
    Rules {
        #[arg(long)]
        seed: bool,
        #[arg(long)]
        apply: Option<String>,
        #[arg(long)]
        params: Option<String>,
        #[arg(long)]
        candidate: Option<String>,
        #[arg(long)]
        json: bool,
    },

    /// List parameter names available in the database
    ListParams {
        #[arg(long)]
        category: Option<String>,
        #[arg(long)]
        json: bool,
    },

    /// List simulation models in the database
    ListModels {
        #[arg(long)]
        model_type: Option<String>,
        #[arg(long)]
        unverified: bool,
        #[arg(long)]
        json: bool,
    },

    /// Mark a simulation model as verified
    VerifyModel {
        #[arg(long)]
        id: Option<i64>,
        #[arg(long)]
        mpn: Option<String>,
        #[arg(long)]
        model_type: Option<String>,
    },

    /// View or update component lifecycle status
    Lifecycle {
        #[arg(long)]
        mpn: String,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        json: bool,
    },

    /// Compare parameters of multiple components side-by-side
    Diff {
        mpns: Vec<String>,
        #[arg(long)]
        json: bool,
    },

    /// Compare multiple candidate values against a design rule
    Compare {
        #[arg(long)]
        rule: String,
        #[arg(long)]
        params: String,
        #[arg(long)]
        candidates: String,
        #[arg(long)]
        json: bool,
    },

    /// List or filter registered design skills
    Skills {
        #[arg(long)]
        domain: Option<String>,
        #[arg(long)]
        tag: Option<String>,
        #[arg(long)]
        json: bool,
    },

    /// Match design skills from natural language query
    MatchSkill {
        query: String,
        #[arg(long)]
        json: bool,
    },

    /// Trace a parameter back to its source in a design log
    Trace {
        #[arg(long)]
        log: String,
        #[arg(long)]
        param: String,
    },

    /// Analyze which downstream steps are affected by changing a parameter
    Impact {
        #[arg(long)]
        log: String,
        #[arg(long)]
        param: String,
    },

    /// Start MCP server for AI integration (stdio transport)
    Serve,

    /// Recommend components based on design rule outputs
    Recommend {
        #[arg(long)]
        rule: String,
        #[arg(long)]
        params: String,
        #[arg(long)]
        candidate: Option<String>,
        #[arg(short, long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },

    /// Generate BOM (Bill of Materials) from database
    Bom {
        #[arg(long, default_value = "csv")]
        format: String,
        #[arg(short, long)]
        output: String,
    },

    /// Generate KiCad netlist from schematic file
    Netlist {
        #[arg(long)]
        input: String,
        #[arg(short, long)]
        output: String,
    },

    /// Run ERC (Electrical Rules Check) on a KiCad schematic
    Erc {
        #[arg(long)]
        input: String,
        #[arg(short, long)]
        output: Option<String>,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        annotate_svg: Option<String>,
        #[arg(long, default_value = "3.0")]
        scale: f64,
    },

    /// Run DRC (Design Rules Check) on a KiCad PCB
    Drc {
        #[arg(long)]
        input: String,
        #[arg(short, long)]
        output: Option<String>,
        #[arg(long)]
        json: bool,
    },

    /// Export Gerber + drill manufacturing files via kicad-cli
    ExportGerber {
        /// Path to input .kicad_pcb file
        #[arg(long)]
        input: String,
        /// Output directory (created if missing)
        #[arg(short, long)]
        output: String,
    },

    /// Synthesize pin-1 dots, polarity marks, refdes silk and courtyards
    AddSilk {
        /// PCB file
        #[arg(long)]
        input: String,
        /// Output path (default: in place)
        #[arg(short, long)]
        output: Option<String>,
    },

    /// Add mounting holes and test points (fixtures) to a PCB
    AddFixture {
        /// PCB file
        #[arg(long)]
        input: String,
        /// Output path (default: in place)
        #[arg(short, long)]
        output: Option<String>,
        /// Number of corner mounting holes (0 to disable, default 4)
        #[arg(long, default_value_t = 4)]
        holes: usize,
        /// Mounting hole drill diameter in mm
        #[arg(long, default_value_t = 2.2)]
        hole_size: f64,
        /// Mounting hole inset from board edge in mm
        #[arg(long, default_value_t = 3.0)]
        hole_inset: f64,
        /// Test points: "NET" (auto-place along right edge) or "NET@x,y"; repeatable
        #[arg(long = "tp")]
        tps: Vec<String>,
        /// Test point pad diameter in mm
        #[arg(long, default_value_t = 1.0)]
        tp_size: f64,
        /// Test point auto-placement inset from board edge in mm
        #[arg(long, default_value_t = 3.0)]
        tp_inset: f64,
    },

    /// Audit junction angles (acute/degenerate) and optionally fix
    AuditAngles {
        /// PCB file
        #[arg(long)]
        input: String,
        /// Apply fixes (remove overlaps, miter corners) in place
        #[arg(long)]
        fix: bool,
    },

    /// Design audit: decoupling distances + trace ampacity
    AuditDesign {
        /// PCB file
        #[arg(long)]
        input: String,
        /// Required current per rail, e.g. --rail "5V_BUCK=1A" (repeatable)
        #[arg(long = "rail")]
        rails: Vec<String>,
    },

    /// Audit switching hot loop, GND return convergence, and plane cuts
    AuditLoop {
        /// PCB file
        #[arg(long)]
        input: String,
    },

    /// Audit vias by layer-side connectivity (gnd_tie/signal/stitch/floating)
    AuditVias {
        /// PCB file
        #[arg(long)]
        input: String,
    },

    /// Generate patch proposals from DRC unconnected pairs
    PatchFromDrc {
        /// PCB file
        #[arg(long)]
        input: String,
    },

    /// Verify schematic ↔ PCB netlist consistency pin-by-pin
    NetlistDiff {
        /// Schematic (.kicad_sch) or exported netlist (.net)
        #[arg(long)]
        sch: String,
        /// PCB (.kicad_pcb)
        #[arg(long)]
        pcb: String,
        /// Fail on pcb-only pads too (default: TP*/H* fixture pads exempt)
        #[arg(long)]
        strict: bool,
    },

    /// H5: Render a design review HTML report from a DesignReviewResult JSON
    ReviewReport {
        #[arg(long)]
        input: String,
        #[arg(short, long)]
        output: String,
        #[arg(long, default_value = "Design Review")]
        title: String,
    },

    /// P1-5: Check hierarchical sheet interfaces (multi-page validation)
    CheckHierarchy {
        #[arg(long)]
        input: String,
        #[arg(long)]
        json: bool,
    },

    /// Explore design space — compare topologies with scoring and ranking
    Explore {
        #[arg(long)]
        vin: f64,
        #[arg(long)]
        vout: f64,
        #[arg(long)]
        iout: f64,
        #[arg(long, default_value = "false")]
        isolated: bool,
        #[arg(long)]
        json: bool,
    },

    /// Import IC and topology templates from directories into database
    ImportTemplates {
        #[arg(long, default_value = "ic-templates")]
        ic_dir: String,
        #[arg(long, default_value = "templates")]
        topo_dir: String,
    },

    /// Run a multi-stage design workflow (e.g. power_tree)
    Workflow {
        #[arg(long)]
        goal: Option<String>,
        #[arg(long)]
        list: bool,
        #[arg(long)]
        params: Option<String>,
        #[arg(long)]
        json: bool,
    },

    /// End-to-end design: requirements → schematic (AI orchestrator)
    DesignBoard {
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
        /// Topology override (auto-selected if omitted)
        #[arg(long)]
        topology: Option<String>,
    },

    /// Generate .kicad_pcb from a schematic file
    GenPcb {
        #[arg(long)]
        input: String,
        #[arg(short, long)]
        output: String,
        #[arg(long)]
        svg: Option<String>,
        #[arg(long)]
        json: bool,
        /// Number of copper layers (2 or 4)
        #[arg(long, default_value = "2")]
        layers: usize,
        /// Only run SA layout, skip routing
        #[arg(long)]
        layout_only: bool,
        /// Only run routing, skip layout (use schematic positions)
        #[arg(long)]
        route_only: bool,
        /// Fixed board size WxH in mm (e.g. 78x76) — constrains SA floorplan
        #[arg(long)]
        board_size: Option<String>,
        /// P1-4: SVG render mode (assembly, fabrication, copper)
        #[arg(long, default_value = "assembly")]
        render_mode: String,
        /// P1-4: SVG layer whitelist, comma-separated (e.g. "F.Cu,B.Cu")
        #[arg(long)]
        render_layers: Option<String>,
    },

    /// H3: Import footprint metadata from KiCad system .pretty libraries
    ImportFootprints {
        /// Override KiCad footprint directory (default: auto-detect)
        #[arg(long)]
        dir: Option<String>,
    },

    /// Archive a board: generate all outputs (sch, pcb, svg, erc, netlist, bom, components, db)
    Archive {
        #[arg(long)]
        input: String,
        #[arg(long)]
        name: String,
        #[arg(short, long)]
        output_dir: String,
        #[arg(long)]
        db_export: bool,
        /// Number of copper layers (2 or 4)
        #[arg(long, default_value = "2")]
        layers: usize,
        /// Fixed board width in mm (locks board outline)
        #[arg(long)]
        board_width: Option<f64>,
        /// Fixed board height in mm (locks board outline)
        #[arg(long)]
        board_height: Option<f64>,
    },

    /// Re-route an existing .kicad_pcb: clear traces and re-run auto-router
    /// Bake footprint rotations into pad geometry (zero the rot angle)
    Bake {
        /// Path to input .kicad_pcb file
        #[arg(long)]
        input: String,
        /// Path to output .kicad_pcb file
        #[arg(short, long)]
        output: String,
    },

    /// 将 route-grid 的 CUDA 路径写回板文件（候选布线——须本地 union-DRC 验证）
    CommitGridPath {
        /// 输入 .kicad_pcb
        #[arg(long)]
        input: String,
        /// 输出 .kicad_pcb
        #[arg(short, long)]
        output: String,
        /// route-grid --out 产出的路径 JSON
        #[arg(long)]
        path: String,
        /// export-grid 产出的 meta JSON
        #[arg(long)]
        meta: String,
        /// 目标 net id（fpga2.grid.json 的 nets 里查）
        #[arg(long)]
        net_id: u32,
        /// 信号线宽 mm
        #[arg(long, default_value_t = 0.2)]
        width: f64,
        /// 过孔外径 mm
        #[arg(long, default_value_t = 0.6)]
        via_size: f64,
        /// 过孔钻孔 mm
        #[arg(long, default_value_t = 0.3)]
        via_drill: f64,
    },

    /// 导出第一信号层波前网格（供 kroute-server RouteGrid / CUDA 求解）：
    /// 生成 <out>.grid.u32（encode_cell LE）+ <out>.grid.json（尺寸/net 起终点/差分对）
    ExportGrid {
        /// Path to input .kicad_pcb file
        #[arg(long)]
        input: String,
        /// Output prefix（生成 <output>.grid.u32 与 <output>.grid.json）
        #[arg(short, long)]
        output: String,
        /// 网格分辨率 mm（0 = 自动：含 BGA/CSP 0.25，否则 0.5）
        #[arg(long, default_value_t = 0.0)]
        res: f64,
        /// M3：间距膨胀格数（0=关；使波前中心线远离其他网铜皮）
        #[arg(long, default_value_t = 0)]
        dilate: usize,
        /// M3：信号层数（2=F/B 两层；4=F+In1+In2+B 全信号）
        #[arg(long, default_value_t = 2)]
        layers: usize,
        /// 修复模式（Task1 修订版）：单网自由端点寻路，忽略 2-pin 网枚举
        #[arg(long, default_value_t = false)]
        repair: bool,
        /// 修复模式：目标网名（任意 pin 数，含电源地网）
        #[arg(long)]
        net: Option<String>,
        /// 修复模式：起点世界坐标 x,y
        #[arg(long)]
        from: Option<String>,
        /// 修复模式：终点世界坐标 x,y
        #[arg(long)]
        to: Option<String>,
        /// 修复模式：孔约束净空 mm（drill/2+hole_clr 全信号层墙；0=不编孔墙，华秋口径 0.15）
        #[arg(long, default_value_t = 0.0)]
        hole_clr: f64,
    },

    /// 布局解回填（v35-flow S3）：LayoutOptimize 的 solution_json → 文本手术改 footprint 位置
    CommitLayout {
        /// 输入 .kicad_pcb
        #[arg(long)]
        input: String,
        /// 输出 .kicad_pcb
        #[arg(short, long)]
        output: String,
        /// solution JSON（{"refs":[...],"positions":[[x,y,rot],...]}）
        #[arg(long)]
        solution: String,
    },

    /// 批量网格路径写回（v35-flow S5 / route-grid --batch 结果消费）：候选布线，须本地 union-DRC
    CommitGridBatch {
        /// 输入 .kicad_pcb
        #[arg(long)]
        input: String,
        /// 输出 .kicad_pcb
        #[arg(short, long)]
        output: String,
        /// route-grid --out 的批量结果 JSON
        #[arg(long)]
        batch: String,
        /// export-grid 产出的 meta JSON
        #[arg(long)]
        meta: String,
        /// 信号线宽 mm
        #[arg(long, default_value_t = 0.2)]
        width: f64,
        /// 过孔外径 mm
        #[arg(long, default_value_t = 0.6)]
        via_size: f64,
        /// 过孔钻孔 mm
        #[arg(long, default_value_t = 0.3)]
        via_drill: f64,
    },

    /// v35 全链路伺服（五阶段：布局 seeds → 可布线性评分 → 解回填 → 批量波前 → 直连提交+分流）
    V35Flow {
        /// 基线板 .kicad_pcb（S2/S3 在其上回填布局解）
        #[arg(long)]
        input: String,
        /// schematic json5（SA 布局问题输入）
        #[arg(long)]
        schematic: String,
        /// 固定板宽高 mm（如 85,55；不传自动推算）
        #[arg(long)]
        board_size: Option<String>,
        /// kroute-server 地址
        #[arg(long, default_value = "http://127.0.0.1:50051")]
        addr: String,
        /// S1 seed 池大小（seeds = base + i*7919）
        #[arg(long, default_value_t = 256)]
        seeds: u32,
        /// S1 seed 基数
        #[arg(long, default_value_t = 42)]
        seed_base: u64,
        /// 连接器锚边覆盖 REF=Edge（可重复；Edge ∈ top/bottom/left/right/center）
        /// 例：--anchor J_EBAZ1=north 不合法，须 --anchor J_EBAZ1=top
        #[arg(long = "anchor")]
        anchors: Vec<String>,
        /// S2 参与评分的 top-k 解数
        #[arg(long, default_value_t = 8)]
        top: usize,
        /// 阶段产物目录（断点续跑：产物存在即跳过）
        #[arg(long, default_value = "output/v35-flow")]
        out_dir: String,
        /// 协同评分权重：总评 = α·sa_cost_norm − (1−α)·可布线性
        #[arg(long, default_value_t = 0.5)]
        alpha: f64,
        /// S4 用 net-aware 增量模式（已布 net 阻挡后续 net），传 false 关闭
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        net_aware: bool,
        /// 网格分辨率 mm（0 = 自动：含 BGA/CSP 0.25，否则 0.5）
        #[arg(long, default_value_t = 0.0)]
        res: f64,
        /// 信号线宽 mm（S5 直连 commit）
        #[arg(long, default_value_t = 0.2)]
        width: f64,
        /// 过孔外径 mm
        #[arg(long, default_value_t = 0.6)]
        via_size: f64,
        /// 过孔钻孔 mm
        #[arg(long, default_value_t = 0.3)]
        via_drill: f64,
        /// 忽略断点产物全量重跑
        #[arg(long, default_value_t = false)]
        force: bool,
        /// S5 后跑 kicad-cli pcb drc 复验
        #[arg(long, default_value_t = false)]
        drc: bool,
        /// M3：S4 顺序择优轮数（导出序/短距优先/固定种子洗牌）
        #[arg(long, default_value_t = 3)]
        orders: u32,
        /// M3：间距膨胀格数（0=关；推荐 ceil((clearance+线宽/2)/分辨率)）
        #[arg(long, default_value_t = 1)]
        dilate: usize,
        /// M3：SA 重叠罚权重覆盖（0 = 服务端默认 50；实测盒尺寸后建议 2000+）
        #[arg(long, default_value_t = 0.0)]
        overlap_weight: f64,
        /// M3：S5 commit 线宽自适应（按到异网铜皮距离收窄），传 false 关闭
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        adaptive_width: bool,
        /// M3：信号层数（2=F/B 两层；4=F+In1+In2+B 全信号）
        #[arg(long, default_value_t = 2)]
        layers: usize,
        /// M3：S6 混合分流落地——s5 板提交 freerouting 只布硬网（需 FR 后端就绪）
        #[arg(long, default_value_t = false)]
        hybrid_fr: bool,
    },

    Reroute {
        /// Path to input .kicad_pcb file
        #[arg(long)]
        input: String,
        /// Path to output .kicad_pcb file
        #[arg(short, long)]
        output: String,
        /// Number of copper layers (2, 4, or 6)
        #[arg(long, default_value = "4")]
        layers: usize,
        /// Force this trace width (mm) on every signal net
        #[arg(long)]
        trace_width: Option<f64>,
        /// Force this clearance (mm) for the whole routing run
        #[arg(long)]
        clearance: Option<f64>,
        /// Per-net width overrides, e.g. "SW_NODE=0.5,VBAT=0.8" (repeatable not needed)
        #[arg(long)]
        width_by_net: Option<String>,
        /// Open output PCB in KiCad after completion
        #[arg(long)]
        open: bool,
    },

    /// Re-layout an existing PCB: SA floorplanner + optional re-route
    Relayout {
        /// Path to input .kicad_pcb file
        #[arg(long)]
        input: String,
        /// Path to output .kicad_pcb file
        #[arg(short, long)]
        output: String,
        /// Number of copper layers (2, 4, or 6)
        #[arg(long, default_value = "4")]
        layers: usize,
        /// Also re-route after re-layout (default: true)
        #[arg(long, default_value = "true")]
        route: bool,
        /// Render PCB to SVG
        #[arg(long)]
        svg: Option<String>,
        /// Open output PCB in KiCad after completion
        #[arg(long)]
        open: bool,
    },

    /// Design a multi-rail power tree (auto-decompose into converter modules)
    PowerTree {
        /// Input voltage (V)
        #[arg(long)]
        vin: f64,
        /// Output rails as vout:iout pairs (e.g. "5:2,3.3:1,1.8:0.5")
        #[arg(long)]
        outputs: String,
        /// Output .kicad_sch file path
        #[arg(short, long)]
        output: String,
        /// Require galvanic isolation
        #[arg(long, default_value = "false")]
        isolated: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.db == ":memory:" {
        eprintln!("Note: using in-memory database, data will not persist");
    }
    let db = ComponentDb::open(&cli.db)?;

    match cli.command {
        Commands::Import { path } => commands::cmd_import(&db, &path),
        Commands::ImportCsv { path, json } => commands::cmd_import_csv(&db, &path, json),
        Commands::ImportModel {
            mpn,
            model_type,
            path,
            format,
        } => commands::cmd_import_model(&db, &mpn, &model_type, &path, &format),
        Commands::Query {
            category,
            param,
            manufacturer,
            package,
            search,
            in_stock,
            lifecycle,
            limit,
            json,
        } => commands::cmd_query(
            &db,
            category.as_deref(),
            param.as_deref(),
            manufacturer.as_deref(),
            package.as_deref(),
            search.as_deref(),
            in_stock,
            lifecycle.as_deref(),
            limit,
            json,
        ),
        Commands::Show { mpn, json } => commands::cmd_show(&db, &mpn, json),
        Commands::Categories { json } => commands::cmd_categories(&db, json),
        Commands::Check {
            rule,
            params,
            candidate,
            json,
        } => commands::cmd_check(&db, &rule, &params, candidate.as_deref(), json),
        Commands::Export {
            mpn,
            category,
            format,
            output,
        } => commands::cmd_export(&db, mpn.as_deref(), category.as_deref(), &format, &output),
        Commands::SymFromDb {
            db,
            mpn,
            output,
            lib_name,
        } => {
            let msg = kicad_cdb::symgen::generate_symbol_from_mpn(
                &db,
                &mpn,
                &output,
                lib_name.as_deref(),
            )?;
            println!("{}", msg);
            Ok(())
        }
        Commands::Fetch { mpn, mfg_id } => commands::cmd_fetch(&db, &mpn, mfg_id.as_deref()),
        Commands::Design {
            template,
            vin,
            vout,
            iout,
            output,
        } => commands::cmd_design(&db, &template, vin, vout, iout, &output),
        Commands::Compose { file, output } => commands::cmd_compose(&db, &file, &output),
        Commands::IcDesign {
            template,
            params,
            nets,
            output,
        } => commands::cmd_ic_design(&db, &template, &params, nets.as_deref(), &output),
        Commands::Suggest {
            vin,
            vout,
            iout,
            isolated,
            json,
        } => commands::cmd_suggest(vin, vout, iout, isolated, json),
        Commands::HqSearch {
            keyword,
            limit,
            json,
        } => commands::cmd_hqsearch(&keyword, limit, json),
        Commands::Rules {
            seed,
            apply,
            params,
            candidate,
            json,
        } => commands::cmd_rules(
            &db,
            seed,
            apply.as_deref(),
            params.as_deref(),
            candidate.as_deref(),
            json,
        ),
        Commands::TemplatePins { mpn, json } => commands::cmd_template_pins(&mpn, json),
        Commands::ImportTemplates { ic_dir, topo_dir } => {
            commands::cmd_import_templates(&db, &ic_dir, &topo_dir)
        }
        Commands::Pipeline {
            name,
            list,
            params,
            json,
        } => commands::cmd_pipeline(&db, name.as_deref(), list, params.as_deref(), json),
        Commands::ListParams { category, json } => {
            commands::cmd_list_params(&db, category.as_deref(), json)
        }
        Commands::ListModels {
            model_type,
            unverified,
            json,
        } => commands::cmd_list_models(&db, model_type.as_deref(), unverified, json),
        Commands::VerifyModel {
            id,
            mpn,
            model_type,
        } => commands::cmd_verify_model(&db, id, mpn.as_deref(), model_type.as_deref()),
        Commands::Lifecycle { mpn, status, json } => {
            commands::cmd_lifecycle(&db, &mpn, status.as_deref(), json)
        }
        Commands::Diff { mpns, json } => commands::cmd_diff(&db, &mpns, json),
        Commands::Compare {
            rule,
            params,
            candidates,
            json,
        } => commands::cmd_compare(&db, &rule, &params, &candidates, json),
        Commands::Skills { domain, tag, json } => {
            commands::cmd_skills(&db, domain.as_deref(), tag.as_deref(), json)
        }
        Commands::MatchSkill { query, json } => commands::cmd_match_skill(&db, &query, json),
        Commands::Trace { log, param } => commands::cmd_trace(&log, &param),
        Commands::Impact { log, param } => commands::cmd_impact(&log, &param),
        Commands::Serve => mcp::serve(&db),
        Commands::Recommend {
            rule,
            params,
            candidate,
            limit,
            json,
        } => commands::cmd_recommend(&db, &rule, &params, candidate.as_deref(), limit, json),
        Commands::Bom { format, output } => commands::cmd_bom(&db, &format, &output),
        Commands::Netlist { input, output } => commands::cmd_netlist(&input, &output),
        Commands::Erc {
            input,
            output,
            json,
            annotate_svg,
            scale,
        } => commands::cmd_erc(
            &input,
            output.as_deref(),
            json,
            annotate_svg.as_deref(),
            scale,
        ),
        Commands::Drc {
            input,
            output,
            json,
        } => commands::cmd_drc(&input, output.as_deref(), json),
        Commands::ExportGerber { input, output } => commands::cmd_export_gerber(&input, &output),
        Commands::AddSilk { input, output } => commands::cmd_add_silk(&input, output.as_deref()),
        Commands::AddFixture {
            input,
            output,
            holes,
            hole_size,
            hole_inset,
            tps,
            tp_size,
            tp_inset,
        } => commands::cmd_add_fixture(
            &input,
            output.as_deref(),
            holes,
            hole_size,
            hole_inset,
            &tps,
            tp_size,
            tp_inset,
        ),
        Commands::AuditAngles { input, fix } => commands::cmd_audit_angles(&input, fix),
        Commands::AuditDesign { input, rails } => commands::cmd_audit_design(&input, &rails),
        Commands::AuditLoop { input } => {
            let source = std::fs::read_to_string(&input)?;
            let board = kicad_json5::parse_board(&source)?;
            let report = kicad_cdb::loop_audit::audit_loop(&board);
            let (text, advisory, action) = kicad_cdb::loop_audit::format_loop_report(&report);
            println!("{text}");
            let gate = if action == 0 {
                if advisory == 0 {
                    "PASS".to_string()
                } else {
                    format!("PASS — {} advisory", advisory)
                }
            } else {
                format!("FAIL — {} action items", action)
            };
            println!("LOOP GATE: {}", gate);
            Ok(())
        }
        Commands::AuditVias { input } => commands::cmd_audit_vias(&input),
        Commands::PatchFromDrc { input } => commands::cmd_patch_from_drc(&input),
        Commands::NetlistDiff { sch, pcb, strict } => {
            let r = commands::cmd_netlist_diff(&sch, &pcb, strict)?;
            if !r {
                std::process::exit(1);
            }
            Ok(())
        }
        Commands::ReviewReport {
            input,
            output,
            title,
        } => commands::cmd_review_report(&input, &output, &title),
        Commands::CheckHierarchy { input, json } => commands::cmd_check_hierarchy(&input, json),
        Commands::ImportFootprints { dir } => commands::cmd_import_footprints(&db, dir.as_deref()),
        Commands::Explore {
            vin,
            vout,
            iout,
            isolated,
            json,
        } => commands::cmd_explore(&db, vin, vout, iout, isolated, json),
        Commands::Workflow {
            goal,
            list,
            params,
            json,
        } => commands::cmd_workflow(&db, goal.as_deref(), list, params.as_deref(), json),
        Commands::DesignBoard {
            vin,
            vout,
            iout,
            output,
            topology,
        } => commands::cmd_design_board(&db, vin, vout, iout, &output, topology.as_deref()),
        Commands::PowerTree {
            vin,
            outputs,
            output,
            isolated,
        } => commands::cmd_power_tree(&db, vin, &outputs, &output, isolated),
        Commands::GenPcb {
            input,
            output,
            svg,
            json,
            layers,
            layout_only,
            route_only,
            render_mode,
            render_layers,
            board_size,
        } => {
            let board_size = board_size.as_deref().and_then(|s| {
                let s = s.to_lowercase().replace(['x', 'X', '*'], "x");
                let mut it = s.split('x');
                match (it.next(), it.next(), it.next()) {
                    (Some(w), Some(h), None) => {
                        match (w.trim().parse::<f64>(), h.trim().parse::<f64>()) {
                            (Ok(w), Ok(h)) if w > 5.0 && h > 5.0 => Some((w, h)),
                            _ => None,
                        }
                    }
                    _ => None,
                }
            });
            commands::cmd_gen_pcb(
                &input,
                &output,
                svg.as_deref(),
                json,
                layers,
                layout_only,
                route_only,
                render_mode.as_str(),
                render_layers.as_deref(),
                board_size,
            )
        }
        Commands::Archive {
            input,
            name,
            output_dir,
            db_export,
            layers,
            board_width,
            board_height,
        } => commands::cmd_archive(
            &db,
            &input,
            &name,
            &output_dir,
            db_export,
            layers,
            board_width,
            board_height,
        ),
        Commands::Bake { input, output } => commands::cmd_bake(&input, &output),
        Commands::CommitGridPath {
            input,
            output,
            path,
            meta,
            net_id,
            width,
            via_size,
            via_drill,
        } => commands::cmd_commit_grid_path(
            &input, &output, &path, &meta, net_id, width, via_size, via_drill,
        ),
        Commands::ExportGrid {
            input,
            output,
            res,
            dilate,
            layers,
            repair,
            net,
            from,
            to,
            hole_clr,
        } => {
            if repair {
                let net = net.as_deref().context("--repair 需要 --net NAME")?;
                let parse_xy = |s: &str, what: &str| -> anyhow::Result<(f64, f64)> {
                    let (a, b) = s
                        .split_once(',')
                        .ok_or_else(|| anyhow::anyhow!("--{what} 格式应为 x,y: {s}"))?;
                    Ok((a.trim().parse()?, b.trim().parse()?))
                };
                let from = parse_xy(from.as_deref().context("--repair 需要 --from x,y")?, "from")?;
                let to = parse_xy(to.as_deref().context("--repair 需要 --to x,y")?, "to")?;
                commands::cmd_export_grid_repair(
                    &input, &output, res, dilate, layers, net, from, to, hole_clr,
                )
            } else {
                commands::cmd_export_grid(&input, &output, res, dilate, layers)
            }
        }
        Commands::CommitLayout {
            input,
            output,
            solution,
        } => commands::cmd_commit_layout(&input, &output, &solution),
        Commands::CommitGridBatch {
            input,
            output,
            batch,
            meta,
            width,
            via_size,
            via_drill,
        } => commands::cmd_commit_grid_batch(
            &input, &output, &batch, &meta, width, via_size, via_drill,
        ),
        Commands::V35Flow {
            input,
            schematic,
            board_size,
            addr,
            seeds,
            seed_base,
            anchors,
            top,
            out_dir,
            alpha,
            net_aware,
            res,
            width,
            via_size,
            via_drill,
            force,
            drc,
            orders,
            dilate,
            overlap_weight,
            adaptive_width,
            layers,
            hybrid_fr,
        } => {
            let board_size = board_size.as_deref().map(|s| {
                let s = s.to_lowercase().replace(['x', 'X', '*', ','], "x");
                let mut it = s.split('x');
                let w: f64 = it
                    .next()
                    .unwrap_or("")
                    .trim()
                    .parse()
                    .expect("board_size 宽");
                let h: f64 = it
                    .next()
                    .unwrap_or("")
                    .trim()
                    .parse()
                    .expect("board_size 高");
                (w, h)
            });
            let mut anchor_overrides = std::collections::HashMap::new();
            for a in &anchors {
                let (ref_name, edge) = a
                    .split_once('=')
                    .ok_or_else(|| anyhow::anyhow!("--anchor 格式应为 REF=Edge: {a}"))?;
                anchor_overrides.insert(ref_name.trim().to_string(), edge.trim().to_string());
            }
            v35_flow::run(v35_flow::V35FlowArgs {
                input,
                schematic,
                board_size,
                addr,
                seeds,
                seed_base,
                top,
                out_dir,
                alpha,
                net_aware,
                res,
                width,
                via_size,
                via_drill,
                force,
                drc,
                anchor_overrides,
                orders,
                dilate,
                overlap_weight,
                adaptive_width,
                layers,
                hybrid_fr,
            })
        }
        Commands::Reroute {
            input,
            output,
            layers,
            trace_width,
            clearance,
            width_by_net,
            open,
        } => commands::cmd_reroute(
            &input,
            &output,
            layers,
            trace_width,
            clearance,
            width_by_net.as_deref(),
            open,
        ),
        Commands::Relayout {
            input,
            output,
            layers,
            route,
            svg,
            open,
        } => commands::cmd_relayout(&input, &output, layers, route, svg.as_deref(), open),
    }
}
