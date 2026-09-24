//! PCB signal auto-router using A* pathfinding with 45° mitered corners.
//!
//! Routing strategies (by net type):
//! 1. Power/GND → L-shape with 45° miter (already done in design.rs)
//! 2. High-speed → A* with length/impedance constraints (priority routing)
//! 3. Differential pairs → parallel A* routing
//! 4. General signal → A* maze routing (default)
//! 5. Rip-up retry → when completion rate < 95%

use rayon::prelude::*;
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

use kicad_json5::ir::board::{Board, Footprint, Segment, Via};

use crate::layer_config::ViaType;
use crate::layout_directives::LayoutDirectives;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Grid resolution in mm — default 250μm for small/medium boards.
/// Large boards (>200 nets) use 500μm to reduce memory by 4x and avoid macOS page compression OOM.
const GRID_RES: f64 = 0.25;
const GRID_RES_LARGE: f64 = 0.5;
const GRID_RES_FINE: f64 = 0.125;

/// Default minimum clearance between different-net traces in mm.
const DEFAULT_CLEARANCE: f64 = 0.2;

fn power_net_name(name: &str) -> bool {
    let u = name.to_uppercase();
    u == "GND"
        || u.contains("VIN")
        || u.contains("VCC")
        || u.contains("VDD")
        || u.contains("VBAT")
        || u.contains("VOUT")
        || u.contains("5V")
        || u.contains("3V3")
        || u.contains("12V")
        || u.contains("9V")
}

/// Cost penalty for layer change (via) relative to straight move.
const VIA_COST: f64 = 2.0;

/// Cost penalty for direction change (encourages straight traces).
const TURN_COST: f64 = 1.0;

/// Penalty for going against the preferred layer direction.
/// F.Cu prefers horizontal (dc != 0), B.Cu prefers vertical (dr != 0).
const LAYER_DIR_PENALTY: f64 = 0.3;

/// Penalty per congestion unit above threshold — discourages routing through heavily congested areas.
const CONGESTION_PENALTY: f64 = 0.5;
/// Congestion threshold — penalty only applies when congestion exceeds this.
const CONGESTION_THRESHOLD: u16 = 3;

/// Proximity penalty for cells adjacent to other nets' traces.
/// Pushes routes away from existing traces to reduce geometric shorts.
const PROXIMITY_PENALTY: f64 = 2.0;

/// Maximum A* iterations before giving up on a single connection.
const MAX_ITERATIONS: usize = 300_000;
/// Iteration limit for desperation pass (doubled for harder nets).
const DESPERATION_MAX_ITERATIONS: usize = 600_000;
/// Cell size for spatial hash conflict detection (mm).
const SPATIAL_CELL_SIZE: f64 = 2.0;

// ---------------------------------------------------------------------------
// Global Routing (tile-based coarse routing phase)
// ---------------------------------------------------------------------------

/// Tile size for global routing phase (mm).
const GLOBAL_TILE_SIZE: f64 = 2.0;
/// PathFinder history cost growth factor per overflow round.
const GLOBAL_HISTORY_FACTOR: f64 = 0.5;
/// Maximum global routing passes (PathFinder iterations).
const GLOBAL_MAX_PASSES: usize = 3;
/// Default trace capacity per signal layer per tile.
const GLOBAL_BASE_CAPACITY: u16 = 5;
/// A* cost bonus for cells inside the global routing guide.
const GLOBAL_GUIDE_BONUS: f64 = 0.15;

/// Coarse tile for global routing phase.
struct GlobalTile {
    capacity: u16,
    usage: u16,
    history_cost: f64,
    blocked: bool,
}

/// Global routing guide — maps each net to the set of tiles it should pass through.
struct GlobalRouteGuide {
    net_guides: HashMap<u32, HashSet<usize>>,
    tile_cols: usize,
    tile_rows: usize,
    tile_size: f64,
    origin_x: f64,
    origin_y: f64,
}

// ---------------------------------------------------------------------------
// Grid Cell
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Cell {
    Free,
    Blocked,
    Pad(u32),   // net_id
    Trace(u32), // net_id (clearance-inflated)
    Via(u32),   // net_id (clearance-inflated)
}

// ---------------------------------------------------------------------------
// Routing Grid (N-layer: configurable via BoardLayerConfig)
// ---------------------------------------------------------------------------

/// Pre-allocated A* state, reused across all routing calls to prevent
/// macOS from compressing freed heap pages into swap.
/// Uses version counter instead of fill() — avoids touching all pages each call.
struct AStarCache {
    g_score: Vec<f64>,
    g_version: Vec<u32>,
    came_from: Vec<u32>,
    cf_version: Vec<u32>,
    open: BinaryHeap<OpenNode>,
    version: u32,
}

impl AStarCache {
    fn new(total: usize) -> Self {
        AStarCache {
            g_score: vec![f64::MAX; total],
            g_version: vec![0; total],
            came_from: vec![u32::MAX; total],
            cf_version: vec![0; total],
            open: BinaryHeap::with_capacity(4096),
            version: 1,
        }
    }
}

struct RoutingGrid {
    cols: usize,
    rows: usize,
    origin_x: f64,
    origin_y: f64,
    grid_res: f64,
    /// Flat grid storage: layer_data[layer_idx * rows * cols + row * cols + col].
    /// Replaces Vec<Vec<Cell>> for better cache locality and parallel-friendly access.
    layer_data: Vec<Cell>,
    num_layers: usize,
    layer_config: crate::layer_config::BoardLayerConfig,
    congestion: Vec<u16>,
    /// A* cache for serial routing paths. Parallel routing creates per-thread caches.
    astar_cache: std::cell::RefCell<AStarCache>,
    /// Index: net_id → list of (layer, cell_flat_idx) occupied by that net.
    net_cells: Vec<Vec<(usize, usize)>>,
}

// Safety: RoutingGrid contains RefCell<AStarCache> which is not Sync.
// However, parallel pathfinding only reads layer_data/congestion via &self methods
// and uses externally-provided per-thread AStarCache. The astar_cache RefCell is
// never touched during parallel pathfinding.
unsafe impl Sync for RoutingGrid {}

impl RoutingGrid {
    fn new(
        board: &Board,
        layer_config: crate::layer_config::BoardLayerConfig,
        grid_res: f64,
    ) -> Self {
        // Use Edge.Cuts outline for bounds, fallback to footprint bbox + 3mm margin
        let (min_x, min_y, max_x, max_y) = Self::compute_bounds(board);
        let max_net_id = board.nets.iter().map(|n| n.id).max().unwrap_or(0) as usize;
        Self::new_bounded(
            min_x,
            min_y,
            max_x,
            max_y,
            layer_config,
            grid_res,
            max_net_id,
        )
    }

    /// F4: Construct a grid scoped to a custom bounding box (for BGA fine-grid fan-out).
    /// Same allocation logic as `new`, but bounds come from the caller instead of
    /// `compute_bounds(board)`. This lets us build a 0.125mm grid over just the BGA
    /// region (~15×15mm = 120×120 cells) without 4× memory on the whole board.
    fn new_bounded(
        min_x: f64,
        min_y: f64,
        max_x: f64,
        max_y: f64,
        layer_config: crate::layer_config::BoardLayerConfig,
        grid_res: f64,
        max_net_id: usize,
    ) -> Self {
        let cols = ((max_x - min_x) / grid_res).ceil() as usize;
        let rows = ((max_y - min_y) / grid_res).ceil() as usize;
        let total = cols * rows;

        // Initialize flat grid: all layers in one contiguous Vec
        let num_layers = layer_config.layer_count();
        let mut layer_data = Vec::with_capacity(num_layers * total);
        for entry in &layer_config.copper_layers {
            use crate::layer_config::LayerPurpose;
            let fill = if entry.purpose != LayerPurpose::Signal {
                Cell::Blocked
            } else {
                Cell::Free
            };
            layer_data.extend(std::iter::repeat_n(fill, total));
        }

        // Pre-allocate A* arrays: one entry per cell per signal layer
        let sig_count = layer_config.signal_layer_indices().len();
        let astar_total = sig_count * total;

        RoutingGrid {
            cols,
            rows,
            origin_x: min_x,
            origin_y: min_y,
            grid_res,
            layer_data,
            num_layers,
            layer_config,
            congestion: vec![0u16; total],
            astar_cache: std::cell::RefCell::new(AStarCache {
                g_score: vec![f64::MAX; astar_total],
                g_version: vec![0; astar_total],
                came_from: vec![u32::MAX; astar_total],
                cf_version: vec![0; astar_total],
                open: BinaryHeap::with_capacity(4096),
                version: 0,
            }),
            net_cells: vec![Vec::new(); max_net_id + 1],
        }
    }

    fn idx(&self, col: usize, row: usize) -> usize {
        row * self.cols + col
    }

    fn world_to_grid(&self, x: f64, y: f64) -> (usize, usize) {
        let col = ((x - self.origin_x) / self.grid_res).round() as usize;
        let row = ((y - self.origin_y) / self.grid_res).round() as usize;
        (col.min(self.cols - 1), row.min(self.rows - 1))
    }

    fn grid_to_world(&self, col: usize, row: usize) -> (f64, f64) {
        (
            self.origin_x + col as f64 * self.grid_res,
            self.origin_y + row as f64 * self.grid_res,
        )
    }

    fn in_bounds(&self, col: usize, row: usize) -> bool {
        col < self.cols && row < self.rows
    }

    /// Compute routing bounds from Edge.Cuts graphics.
    /// Falls back to footprint bounding box + 3mm margin.
    fn compute_bounds(board: &Board) -> (f64, f64, f64, f64) {
        use kicad_json5::ir::board::BoardGraphicKind;

        let edge: Vec<_> = board
            .graphics
            .iter()
            .filter(|g| g.layer == "Edge.Cuts")
            .collect();

        if !edge.is_empty() {
            let mut min_x = f64::MAX;
            let mut min_y = f64::MAX;
            let mut max_x = f64::MIN;
            let mut max_y = f64::MIN;
            for gr in &edge {
                match &gr.kind {
                    BoardGraphicKind::Rect { start, end } => {
                        min_x = min_x.min(start.0).min(end.0);
                        min_y = min_y.min(start.1).min(end.1);
                        max_x = max_x.max(start.0).max(end.0);
                        max_y = max_y.max(start.1).max(end.1);
                    }
                    BoardGraphicKind::Line { start, end } => {
                        min_x = min_x.min(start.0).min(end.0);
                        min_y = min_y.min(start.1).min(end.1);
                        max_x = max_x.max(start.0).max(end.0);
                        max_y = max_y.max(start.1).max(end.1);
                    }
                    BoardGraphicKind::Circle { center, end: e } => {
                        let r = ((e.0 - center.0).powi(2) + (e.1 - center.1).powi(2)).sqrt();
                        min_x = min_x.min(center.0 - r);
                        min_y = min_y.min(center.1 - r);
                        max_x = max_x.max(center.0 + r);
                        max_y = max_y.max(center.1 + r);
                    }
                    BoardGraphicKind::Poly { points } => {
                        for (px, py) in points {
                            min_x = min_x.min(*px);
                            min_y = min_y.min(*py);
                            max_x = max_x.max(*px);
                            max_y = max_y.max(*py);
                        }
                    }
                    _ => {}
                }
            }
            if min_x < max_x && min_y < max_y {
                return (min_x, min_y, max_x, max_y);
            }
        }

        // Fallback: footprint bbox + 3mm margin
        let mut min_x = f64::MAX;
        let mut min_y = f64::MAX;
        let mut max_x = f64::MIN;
        let mut max_y = f64::MIN;
        for fp in &board.footprints {
            let (fx, fy, _) = fp.position;
            let (bw, bh) = crate::layout_engine::infer_body_size(&fp.lib_id, fp.pads.len());
            min_x = min_x.min(fx - bw / 2.0);
            min_y = min_y.min(fy - bh / 2.0);
            max_x = max_x.max(fx + bw / 2.0);
            max_y = max_y.max(fy + bh / 2.0);
        }
        (min_x - 3.0, min_y - 3.0, max_x + 3.0, max_y + 3.0)
    }

    /// Increment congestion counter at a cell (capped at u16 max).
    fn inc_congestion(&mut self, col: usize, row: usize) {
        let idx = self.idx(col, row);
        self.congestion[idx] = self.congestion[idx].saturating_add(1);
    }

    /// Get congestion value at a cell.
    fn congestion_at(&self, col: usize, row: usize) -> u16 {
        self.congestion[self.idx(col, row)]
    }

    fn get(&self, layer: usize, col: usize, row: usize) -> Cell {
        let flat = layer * self.rows * self.cols + row * self.cols + col;
        // SAFETY: flat index is within bounds (verified by callers)
        unsafe { *self.layer_data.get_unchecked(flat) }
    }

    fn set(&mut self, layer: usize, col: usize, row: usize, cell: Cell) {
        let idx = self.idx(col, row);
        let flat = layer * self.rows * self.cols + idx;
        // Track which net occupies this cell for fast unmark
        let net_id = match cell {
            Cell::Trace(n) | Cell::Pad(n) | Cell::Via(n) => Some(n),
            _ => None,
        };
        if let Some(n) = net_id {
            let n = n as usize;
            if n < self.net_cells.len() {
                self.net_cells[n].push((layer, idx));
            }
        }
        self.layer_data[flat] = cell;
    }

    /// Mark component bodies as blocked on all layers.
    fn mark_component_bodies(&mut self, board: &Board) {
        for fp in &board.footprints {
            let (fx, fy, _) = fp.position;
            let (bw, bh) = crate::layout_engine::infer_body_size(&fp.lib_id, fp.pads.len());
            let (c0, r0) = self.world_to_grid(fx - bw / 2.0, fy - bh / 2.0);
            let (c1, r1) = self.world_to_grid(fx + bw / 2.0, fy + bh / 2.0);

            // Only block the component's own layer — traces on the opposite
            // side can safely route under the IC body.
            let body_layer = self.layer_config.layer_index(&fp.layer);

            for c in c0..=c1.min(self.cols - 1) {
                for r in r0..=r1.min(self.rows - 1) {
                    if let Some(li) = body_layer {
                        self.set(li, c, r, Cell::Blocked);
                    } else {
                        // Fallback: block all layers if layer unknown
                        for layer in 0..self.num_layers {
                            self.set(layer, c, r, Cell::Blocked);
                        }
                    }
                }
            }
        }
    }

    /// Mark pad positions (these are passable for same-net, blocked for others).
    /// Pads are expanded to their full geometric size on the routing grid.
    /// SMD pads only block their own layer(s); through-hole pads block all signal layers.
    fn mark_pads(&mut self, board: &Board) {
        use kicad_json5::ir::board::PadType;
        let signal_layers = self.layer_config.signal_layer_indices();
        for fp in &board.footprints {
            let (fx, fy, _) = fp.position;
            for pad in &fp.pads {
                let net_id = pad.net.unwrap_or(0);
                // Unconnected pads (net_id=0) are marked as Blocked to prevent
                // traces from routing through their copper area.
                let cell_type = if net_id == 0 {
                    Cell::Blocked
                } else {
                    Cell::Pad(net_id)
                };
                let (px, py) = fp.pad_rotated_offset(pad);
                let pad_x = fx + px;
                let pad_y = fy + py;
                // Non-square pads swap w/h when the total rotation is 90/270
                let (_, _, fr) = fp.position;
                let (_, _, pr) = pad.position;
                let total_rot = (fr + pr).rem_euclid(180.0);
                let (sw_raw, sh_raw) = pad.size;
                let (sw, sh) = if (total_rot - 90.0).abs() < 1e-6 {
                    (sh_raw, sw_raw)
                } else {
                    (sw_raw, sh_raw)
                };

                // Mark all grid cells covered by the pad's actual geometry + clearance buffer.
                // Buffer = ~0.25mm in PHYSICAL units (cells scale with grid_res),
                // so finer grids keep the same absolute pad protection.
                let exp_cells = ((0.25 / self.grid_res).ceil() as usize).max(1);
                let (c0, r0) = self.world_to_grid(pad_x - sw / 2.0, pad_y - sh / 2.0);
                let (c1, r1) = self.world_to_grid(pad_x + sw / 2.0, pad_y + sh / 2.0);
                let c0 = c0.saturating_sub(exp_cells);
                let r0 = r0.saturating_sub(exp_cells);
                let c1 = (c1 + exp_cells).min(self.cols - 1);
                let r1 = (r1 + exp_cells).min(self.rows - 1);

                // Determine which layers this pad blocks
                let pad_layers: Vec<usize> =
                    if pad.pad_type == PadType::ThruHole || pad.pad_type == PadType::NpThruHole {
                        signal_layers.clone()
                    } else {
                        // SMD: only layers matching pad's declared layers
                        signal_layers
                            .iter()
                            .filter(|&&l| {
                                let name = self.layer_config.layer_name(l);
                                pad.layers.iter().any(|pl| pl == name || pl == "*.Cu")
                            })
                            .copied()
                            .collect()
                    };

                for c in c0..=c1 {
                    for r in r0..=r1 {
                        for &layer in &pad_layers {
                            self.set(layer, c, r, cell_type);
                        }
                    }
                }
            }
        }
    }

    /// Mark existing segments with clearance inflation.
    fn mark_existing_traces(&mut self, board: &Board, clearance: f64) {
        let clear_cells = (clearance / self.grid_res).ceil() as usize;

        for seg in &board.segments {
            let layer = self.layer_config.layer_index(&seg.layer).unwrap_or(0);
            // Walk along segment and mark cells + clearance
            let (c0, r0) = self.world_to_grid(seg.start.0, seg.start.1);
            let (c1, r1) = self.world_to_grid(seg.end.0, seg.end.1);

            // Simple line rasterization
            let steps = ((c0 as i64 - c1 as i64).unsigned_abs())
                .max((r0 as i64 - r1 as i64).unsigned_abs())
                .max(1);

            for s in 0..=steps {
                let t = s as f64 / steps as f64;
                let col = (c0 as f64 + (c1 as f64 - c0 as f64) * t).round() as usize;
                let row = (r0 as f64 + (r1 as f64 - r0 as f64) * t).round() as usize;

                // Mark trace + clearance
                for dc in 0..=clear_cells {
                    for dr in 0..=clear_cells {
                        for &(cc, cr) in &[
                            (col + dc, row + dr),
                            (col.saturating_sub(dc), row + dr),
                            (col + dc, row.saturating_sub(dr)),
                            (col.saturating_sub(dc), row.saturating_sub(dr)),
                        ] {
                            if self.in_bounds(cc, cr) {
                                let existing = self.get(layer, cc, cr);
                                if existing == Cell::Free {
                                    self.set(layer, cc, cr, Cell::Trace(seg.net));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Unmark cells belonging to a specific net (make them passable for routing).
    fn unmark_net(&mut self, net_id: u32) {
        let n = net_id as usize;
        if n >= self.net_cells.len() {
            return;
        }
        // Swap out the cell list to avoid holding a borrow while writing layers
        let cells = std::mem::take(&mut self.net_cells[n]);
        let per_layer = self.rows * self.cols;
        for (layer, idx) in cells {
            let flat = layer * per_layer + idx;
            match self.layer_data[flat] {
                Cell::Trace(m) | Cell::Pad(m) | Cell::Via(m) if m == net_id => {
                    self.layer_data[flat] = Cell::Free;
                }
                _ => {}
            }
        }
    }
}

// ---------------------------------------------------------------------------
// IC Fan-out: break out multi-pin IC pads with vias just outside body boundary
// ---------------------------------------------------------------------------

/// Fan-out result: additional routing endpoints (via positions) per net.
#[allow(dead_code)]
struct FanoutResult {
    /// Extra pad positions (fan-out via locations) to add to pads_by_net.
    extra_pads: HashMap<u32, Vec<(f64, f64)>>,
    /// Number of fan-outs performed (for logging).
    count: usize,
}

/// Fan-out multi-pin ICs: for ICs with >= 16 signal pads (QFP-48+, BGA, etc.),
/// place a via just outside the IC body boundary for each signal pad and connect
/// with a short straight trace. The via enables routing on the opposite signal layer.
fn fanout_ic_pads(
    board: &mut Board,
    grid: &mut RoutingGrid,
    power_nets: &HashSet<u32>,
) -> FanoutResult {
    let mut extra_pads: HashMap<u32, Vec<(f64, f64)>> = HashMap::new();
    let signal_layers = grid.layer_config.signal_layer_indices();
    if signal_layers.len() < 2 {
        return FanoutResult {
            extra_pads,
            count: 0,
        };
    }

    let (first_via_layer, last_via_layer) = grid.layer_config.via_layer_names();
    // For blind/buried via boards, use top-blind via (F.Cu→In2.Cu) for fan-out
    // so B.Cu routing channel is not blocked by through-hole vias
    let fanout_via_layers: Vec<String> =
        if grid.layer_config.blind_buried_vias && signal_layers.len() >= 3 {
            vec![
                grid.layer_config.layer_name(signal_layers[0]).to_string(),
                grid.layer_config.layer_name(signal_layers[1]).to_string(),
            ]
        } else {
            vec![first_via_layer.clone(), last_via_layer.clone()]
        };

    let mut total_fanouts = 0usize;
    let mut layer_counts: Vec<usize> = vec![0; signal_layers.len()];
    let mut occupied_positions: HashSet<(usize, usize)> = HashSet::new();

    for fp_idx in 0..board.footprints.len() {
        let fp = &board.footprints[fp_idx];
        let (fx, fy, _) = fp.position;

        let signal_pads: Vec<(u32, (f64, f64))> = fp
            .pads
            .iter()
            .filter_map(|pad| {
                let net_id = pad.net?;
                if net_id == 0 || power_nets.contains(&net_id) {
                    return None;
                }
                let (px, py) = fp.pad_rotated_offset(pad);
                Some((net_id, (fx + px, fy + py)))
            })
            .collect();

        // Only fan-out ICs with >= 16 signal pads
        if signal_pads.len() < 16 {
            continue;
        }

        // Skip connectors, sockets, test points
        let lib_upper = fp.lib_id.to_uppercase();
        let is_connector = lib_upper.contains("PINHEADER")
            || lib_upper.contains("SOCKET")
            || lib_upper.contains("CONNECTOR")
            || lib_upper.contains("PIN_HDR")
            || lib_upper.contains("01X")
            || lib_upper.contains("TESTPOINT")
            || lib_upper.contains("HDR")
            || lib_upper.contains("TERMINAL");
        if is_connector {
            continue;
        }

        // Only fan-out BGA-style packages
        let is_bga = lib_upper.contains("BGA")
            || lib_upper.contains("CSP")
            || lib_upper.contains("WLCSP")
            || lib_upper.contains("LGA");
        if !is_bga {
            continue;
        }

        // Compute pad bounding box and center
        let mut pad_min_x = f64::MAX;
        let mut pad_max_x = f64::MIN;
        let mut pad_min_y = f64::MAX;
        let mut pad_max_y = f64::MIN;
        for &(_, (px, py)) in &signal_pads {
            pad_min_x = pad_min_x.min(px);
            pad_max_x = pad_max_x.max(px);
            pad_min_y = pad_min_y.min(py);
            pad_max_y = pad_max_y.max(py);
        }
        let center_x = (pad_min_x + pad_max_x) / 2.0;
        let center_y = (pad_min_y + pad_max_y) / 2.0;

        // Detect BGA pitch from adjacent pad distances
        let mut pitch_samples: Vec<f64> = Vec::new();
        for i in 0..signal_pads.len().min(30) {
            for j in (i + 1)..signal_pads.len().min(30) {
                let dx = (signal_pads[i].1 .0 - signal_pads[j].1 .0).abs();
                let dy = (signal_pads[i].1 .1 - signal_pads[j].1 .1).abs();
                if dx < 0.01 && dy > 0.3 {
                    pitch_samples.push(dy);
                } else if dy < 0.01 && dx > 0.3 {
                    pitch_samples.push(dx);
                }
            }
        }
        let bga_pitch = if !pitch_samples.is_empty() {
            pitch_samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
            pitch_samples[pitch_samples.len() / 2]
        } else {
            1.0
        };

        let body_left = pad_min_x - 0.5;
        let body_right = pad_max_x + 0.5;
        let body_top = pad_min_y - 0.5;
        let body_bottom = pad_max_y + 0.5;

        // Adaptive via/trace sizes based on BGA pitch
        let (via_size, via_drill) = if bga_pitch < 1.0 {
            (0.3, 0.15)
        } else {
            (0.4, 0.2)
        };
        let trace_width = if bga_pitch < 1.0 { 0.15 } else { 0.2 };
        let fanout_clearance = if bga_pitch < 1.0 {
            (bga_pitch * 0.15).max(0.15)
        } else {
            DEFAULT_CLEARANCE
        };

        eprintln!(
            "[router] Fan-out IC: {} ({}) — {} signal pads, pitch {:.2}mm, bbox {:.1}x{:.1}mm",
            fp.reference,
            fp.lib_id,
            signal_pads.len(),
            bga_pitch,
            body_right - body_left,
            body_bottom - body_top
        );

        // Multi-row fan-out: sort pads by distance from center (outer first)
        // P1: Within same ring, sort by escape direction to spread load across edges
        let mut sorted_pads: Vec<(u32, (f64, f64))> = signal_pads.clone();
        {
            let pad_dir = |px: f64, py: f64| -> u8 {
                let dl = (px - body_left).abs();
                let dr = (px - body_right).abs();
                let dt = (py - body_top).abs();
                let db = (py - body_bottom).abs();
                let m = dl.min(dr).min(dt).min(db);
                if m == dl {
                    0
                } else if m == dr {
                    1
                } else if m == dt {
                    2
                } else {
                    3
                }
            };
            sorted_pads.sort_by(|a, b| {
                let da = (a.1 .0 - center_x).hypot(a.1 .1 - center_y);
                let db = (b.1 .0 - center_x).hypot(b.1 .1 - center_y);
                let ring_a = (da / bga_pitch).ceil() as usize;
                let ring_b = (db / bga_pitch).ceil() as usize;
                // Outer ring first, then by direction to spread across edges
                ring_b
                    .cmp(&ring_a)
                    .then_with(|| pad_dir(a.1 .0, a.1 .1).cmp(&pad_dir(b.1 .0, b.1 .1)))
                    .then_with(|| da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal))
            });
        }

        // F4: Build a fine 0.125mm grid scoped to this BGA's bbox for escape routing.
        // The coarse main grid has 0.25/0.5mm cells — too coarse for 0.8mm BGA pitch
        // (only 3-4 cells between pads). The fine grid gives 6-8 cells, dramatically
        // improving escape via placement precision.
        //
        // The fine grid is temporary: we escape on it, write results to board
        // (world coords), then discard. The coarse grid is synced later by the
        // caller via mark_existing_traces.
        let fine_res = 0.125f64.min(bga_pitch / 6.0).max(0.05); // adaptive: 0.05-0.125mm
        let margin = bga_pitch * 3.0; // 3 rings of escape room beyond body
        let mut fine_grid = RoutingGrid::new_bounded(
            body_left - margin,
            body_top - margin,
            body_right + margin,
            body_bottom + margin,
            grid.layer_config.clone(),
            fine_res,
            grid.net_cells.len().saturating_sub(1),
        );
        // Mark this BGA's pads onto the fine grid (body outline is implicit in pad positions).
        for &(_, (px, py)) in &signal_pads {
            let (gc, gr) = fine_grid.world_to_grid(px, py);
            for l in 0..fine_grid.num_layers {
                if fine_grid.get(l, gc, gr) == Cell::Free {
                    fine_grid.set(l, gc, gr, Cell::Blocked);
                }
            }
        }
        eprintln!(
            "[fanout] F4 fine grid: {:.3}mm, {}x{} cells for {}",
            fine_res, fine_grid.cols, fine_grid.rows, fp.reference
        );

        // Route escapes on the fine grid instead of the coarse main grid.
        let escape_grid: &mut RoutingGrid = &mut fine_grid;

        // Distribute fan-out across signal layers: round-robin by escape ring
        let num_sig_layers = signal_layers.len();
        let base_offset = bga_pitch * 0.75; // Via offset proportional to pitch
                                            // Cap fan-outs per IC to limit memory growth (grid marking)
        let max_fanouts_per_ic = (grid.cols * grid.rows / 2000).min(signal_pads.len());
        let mut ic_fanout_count = 0usize;

        for &(net_id, (px, py)) in &sorted_pads {
            if ic_fanout_count >= max_fanouts_per_ic {
                break;
            }
            let dist_left = (px - body_left).abs();
            let dist_right = (px - body_right).abs();
            let dist_top = (py - body_top).abs();
            let dist_bottom = (py - body_bottom).abs();
            let min_edge_dist = dist_left.min(dist_right).min(dist_top).min(dist_bottom);

            // Compute ring number (distance from center in pitch units)
            let dist_from_center = (px - center_x).hypot(py - center_y);
            let ring = ((dist_from_center / bga_pitch).ceil() as usize).max(1);

            // Escape offset increases with ring number
            let escape_offset = base_offset + (ring as f64 - 1.0) * bga_pitch * 0.6;

            // Choose escape direction: nearest edge
            let (via_x, via_y) = if min_edge_dist == dist_left {
                (body_left - escape_offset, py)
            } else if min_edge_dist == dist_right {
                (body_right + escape_offset, py)
            } else if min_edge_dist == dist_top {
                (px, body_top - escape_offset)
            } else {
                (px, body_bottom + escape_offset)
            };

            // Direction-aware layer assignment:
            // Left/Right escape → slot 0 (horizontal routing channel)
            // Top/Bottom escape → last slot (vertical routing channel)
            // For 3+ signal layers: diagonal pads → middle slot(s)
            let is_horizontal = min_edge_dist == dist_left || min_edge_dist == dist_right;
            let direction_slot = if num_sig_layers >= 3 {
                // Use escape angle to distribute across all signal layers
                let angle = ((py - center_y) / (px - center_x).max(0.01)).atan().abs();
                // angle=0 → horizontal (slot 0), angle=PI/2 → vertical (last slot)
                let normalized = angle / std::f64::consts::FRAC_PI_2;
                let slot = (normalized * (num_sig_layers - 1) as f64).round() as usize;
                slot.min(num_sig_layers - 1)
            } else if is_horizontal {
                0
            } else if num_sig_layers >= 2 {
                1
            } else {
                0
            };
            let max_per_dir = (max_fanouts_per_ic / num_sig_layers.max(1)) * 2;
            let best_layer_slot = if layer_counts[direction_slot] < max_per_dir {
                direction_slot
            } else {
                // Fallback: least-used layer
                (0..num_sig_layers)
                    .min_by_key(|&l| layer_counts[l])
                    .unwrap_or(0)
            };
            let _layer_idx = signal_layers[best_layer_slot];

            let (mut via_col, mut via_row) = escape_grid.world_to_grid(via_x, via_y);

            // Conflict detection: if position is occupied, try nearby offsets
            // Search ±3 cells (48 candidates), sorted by Chebyshev distance
            if occupied_positions.contains(&(via_col, via_row)) {
                let mut found_pos = false;
                for d in 1..=3i32 {
                    for dx in (-d)..=d {
                        for dy in (-d)..=d {
                            if dx.abs().max(dy.abs()) != d {
                                continue;
                            }
                            let nc = (via_col as i32 + dx).max(0) as usize;
                            let nr = (via_row as i32 + dy).max(0) as usize;
                            if nc < escape_grid.cols
                                && nr < escape_grid.rows
                                && !occupied_positions.contains(&(nc, nr))
                            {
                                via_col = nc;
                                via_row = nr;
                                found_pos = true;
                                break;
                            }
                        }
                        if found_pos {
                            break;
                        }
                    }
                    if found_pos {
                        break;
                    }
                }
                if !found_pos {
                    continue;
                }
            }

            let (via_x, via_y) = escape_grid.grid_to_world(via_col, via_row);
            let (pad_col, pad_row) = escape_grid.world_to_grid(px, py);

            if !escape_grid.in_bounds(via_col, via_row) {
                continue;
            }
            if pad_col == via_col && pad_row == via_row {
                continue;
            }

            // Helper: check if a via position is clear on any signal layer and place it
            let mut placed = false;
            for &try_layer in &signal_layers {
                match escape_grid.get(try_layer, via_col, via_row) {
                    Cell::Blocked | Cell::Trace(_) | Cell::Via(_) => continue,
                    Cell::Pad(n) if n != net_id && n != 0 => continue,
                    _ => {
                        let seg = Segment {
                            start: (px, py),
                            end: (via_x, via_y),
                            width: trace_width,
                            layer: escape_grid.layer_config.layer_name(try_layer).to_string(),
                            net: net_id,
                        };
                        let via = Via {
                            at: (via_x, via_y),
                            size: via_size,
                            drill: via_drill,
                            layers: fanout_via_layers.clone(),
                            net: net_id,
                        };
                        mark_segment_on_grid(escape_grid, &seg, net_id, fanout_clearance);
                        let via_from = signal_layers[0];
                        let via_to = if escape_grid.layer_config.blind_buried_vias
                            && signal_layers.len() >= 3
                        {
                            signal_layers[1]
                        } else {
                            *signal_layers.last().unwrap()
                        };
                        let via_traversed = escape_grid
                            .layer_config
                            .via_traversed_layers(via_from, via_to);
                        for l in via_traversed {
                            if escape_grid.in_bounds(via_col, via_row) {
                                escape_grid.set(l, via_col, via_row, Cell::Via(net_id));
                            }
                        }
                        board.segments.push(seg);
                        board.vias.push(via);
                        total_fanouts += 1;
                        ic_fanout_count += 1;
                        layer_counts[best_layer_slot] += 1;
                        occupied_positions.insert((via_col, via_row));
                        extra_pads.entry(net_id).or_default().push((via_x, via_y));
                        placed = true;
                        break;
                    }
                }
            }
            if placed {
                continue;
            }

            // Dog-leg escape: L-shaped route when direct escape is blocked
            let mut dogleg_success = false;
            let escape_dir_x = if dist_left <= dist_right { -1.0 } else { 1.0 };
            let escape_dir_y = if dist_top <= dist_bottom { -1.0 } else { 1.0 };
            let dl_candidates: Vec<(f64, f64)> = vec![
                (px + escape_dir_x * bga_pitch, py + escape_dir_y * bga_pitch),
                (
                    px + escape_dir_x * bga_pitch * 1.5,
                    py + escape_dir_y * bga_pitch * 0.5,
                ),
                (px - escape_dir_x * bga_pitch, py + escape_dir_y * bga_pitch),
                (px + escape_dir_x * bga_pitch, py - escape_dir_y * bga_pitch),
            ];
            for (dl_vx, dl_vy) in &dl_candidates {
                let (dl_col, dl_row) = escape_grid.world_to_grid(*dl_vx, *dl_vy);
                if !escape_grid.in_bounds(dl_col, dl_row) {
                    continue;
                }
                if occupied_positions.contains(&(dl_col, dl_row)) {
                    continue;
                }
                let corner_x = px + escape_dir_x * bga_pitch;
                let corner_y = py;

                for &try_layer in &signal_layers {
                    match escape_grid.get(try_layer, dl_col, dl_row) {
                        Cell::Blocked | Cell::Trace(_) | Cell::Via(_) => continue,
                        Cell::Pad(n) if n != net_id && n != 0 => continue,
                        _ => {
                            let seg1 = Segment {
                                start: (px, py),
                                end: (corner_x, corner_y),
                                width: trace_width,
                                layer: escape_grid.layer_config.layer_name(try_layer).to_string(),
                                net: net_id,
                            };
                            let seg2 = Segment {
                                start: (corner_x, corner_y),
                                end: (*dl_vx, *dl_vy),
                                width: trace_width,
                                layer: escape_grid.layer_config.layer_name(try_layer).to_string(),
                                net: net_id,
                            };
                            let via = Via {
                                at: (*dl_vx, *dl_vy),
                                size: via_size,
                                drill: via_drill,
                                layers: fanout_via_layers.clone(),
                                net: net_id,
                            };
                            mark_segment_on_grid(escape_grid, &seg1, net_id, fanout_clearance);
                            mark_segment_on_grid(escape_grid, &seg2, net_id, fanout_clearance);
                            let via_from = signal_layers[0];
                            let via_to = if escape_grid.layer_config.blind_buried_vias
                                && signal_layers.len() >= 3
                            {
                                signal_layers[1]
                            } else {
                                *signal_layers.last().unwrap()
                            };
                            for l in escape_grid
                                .layer_config
                                .via_traversed_layers(via_from, via_to)
                            {
                                if escape_grid.in_bounds(dl_col, dl_row) {
                                    escape_grid.set(l, dl_col, dl_row, Cell::Via(net_id));
                                }
                            }
                            board.segments.push(seg1);
                            board.segments.push(seg2);
                            board.vias.push(via);
                            total_fanouts += 1;
                            ic_fanout_count += 1;
                            layer_counts[best_layer_slot] += 1;
                            occupied_positions.insert((dl_col, dl_row));
                            extra_pads.entry(net_id).or_default().push((*dl_vx, *dl_vy));
                            dogleg_success = true;
                            break;
                        }
                    }
                }
                if dogleg_success {
                    break;
                }
            }
        }
    }

    if total_fanouts > 0 {
        eprintln!(
            "[router] Fan-out: {} pads broken out with vias (layers: {:?})",
            total_fanouts, layer_counts
        );
    }

    FanoutResult {
        extra_pads,
        count: total_fanouts,
    }
}

// ---------------------------------------------------------------------------
// A* Pathfinding (8-directional for 45° corners)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct Node {
    col: usize,
    row: usize,
    layer: usize,
}

impl Node {
    #[allow(dead_code)]
    fn key(&self, _cols: usize) -> u64 {
        // Pack into a single u64 for HashMap efficiency
        (self.layer as u64) << 48 | (self.row as u64) << 24 | self.col as u64
    }
}

#[derive(Debug)]
struct OpenNode {
    node: Node,
    f: f64, // g + h
}

impl PartialEq for OpenNode {
    fn eq(&self, other: &Self) -> bool {
        self.f == other.f
    }
}
impl Eq for OpenNode {}
impl PartialOrd for OpenNode {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for OpenNode {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Max-heap → reverse for min-heap (lower f = higher priority)
        other
            .f
            .partial_cmp(&self.f)
            .unwrap_or(std::cmp::Ordering::Equal)
    }
}

/// 8-directional neighbors: (dcol, drow, cost)
/// Diagonal moves cost sqrt(2) ≈ 1.414 for accurate distance.
const DIRECTIONS: &[(i32, i32, f64)] = &[
    (1, 0, 1.0),     // Right
    (-1, 0, 1.0),    // Left
    (0, 1, 1.0),     // Up
    (0, -1, 1.0),    // Down
    (1, 1, 1.414),   // 45° NE
    (-1, 1, 1.414),  // 45° NW
    (1, -1, 1.414),  // 45° SE
    (-1, -1, 1.414), // 45° SW
];

fn manhattan_heuristic(n: &Node, goal: &Node) -> f64 {
    let dx = (n.col as f64 - goal.col as f64).abs();
    let dy = (n.row as f64 - goal.row as f64).abs();
    // Octile distance (admissible for 8-directional movement)
    let d = 1.0;
    let d2 = 1.414;
    d * (dx + dy) + (d2 - 2.0 * d) * dx.min(dy)
}

fn astar_route(
    grid: &RoutingGrid,
    start: Node,
    goal: Node,
    net_id: u32,
    prev_dir: Option<(i32, i32)>,
    trace_width: f64,
    clearance: f64,
    history_congestion: &[f64],
) -> Option<Vec<Node>> {
    astar_route_with_limit(
        grid,
        start,
        goal,
        net_id,
        prev_dir,
        trace_width,
        clearance,
        MAX_ITERATIONS,
        history_congestion,
        false,
    )
}

fn astar_route_with_limit(
    grid: &RoutingGrid,
    start: Node,
    goal: Node,
    net_id: u32,
    prev_dir: Option<(i32, i32)>,
    trace_width: f64,
    clearance: f64,
    max_iters: usize,
    history_congestion: &[f64],
    prefer_inner_layer: bool,
) -> Option<Vec<Node>> {
    let mut cache = grid.astar_cache.borrow_mut();
    astar_route_with_limit_impl(
        grid,
        start,
        goal,
        net_id,
        prev_dir,
        trace_width,
        clearance,
        max_iters,
        history_congestion,
        prefer_inner_layer,
        &mut cache,
        None,
    )
}

/// A* implementation that accepts an external cache — enables per-thread caches
/// in parallel contexts without RefCell contention.
fn astar_route_with_limit_impl(
    grid: &RoutingGrid,
    start: Node,
    goal: Node,
    net_id: u32,
    prev_dir: Option<(i32, i32)>,
    _trace_width: f64,
    _clearance: f64,
    max_iters: usize,
    history_congestion: &[f64],
    prefer_inner_layer: bool,
    cache: &mut AStarCache,
    global_guide: Option<&GlobalRouteGuide>,
) -> Option<Vec<Node>> {
    let _num_layers = grid.layer_config.layer_count();
    let signal_layers = grid.layer_config.signal_layer_indices();
    cache.version = cache.version.wrapping_add(1);
    let ver = cache.version;
    cache.open.clear();
    let AStarCache {
        g_score,
        g_version,
        came_from,
        cf_version,
        open,
        ..
    } = cache;

    // Versioned access macros: avoid closures that would conflict with borrows.
    // Unvisited cells have stale version → treated as MAX. No fill() needed.
    macro_rules! get_g {
        ($i:expr) => {
            if g_version[$i] == ver {
                g_score[$i]
            } else {
                f64::MAX
            }
        };
    }
    macro_rules! set_g {
        ($i:expr, $v:expr) => {{
            g_score[$i] = $v;
            g_version[$i] = ver;
        }};
    }
    macro_rules! get_cf {
        ($i:expr) => {
            if cf_version[$i] == ver {
                came_from[$i]
            } else {
                u32::MAX
            }
        };
    }
    macro_rules! set_cf {
        ($i:expr, $v:expr) => {{
            came_from[$i] = $v;
            cf_version[$i] = ver;
        }};
    }

    // Cap open set to prevent excessive memory use.
    const MAX_OPEN_SIZE: usize = 50_000;

    // P-route-1: Precompute layer→slot lookup table
    let max_layer_idx = *signal_layers.iter().max().unwrap_or(&0);
    let layer_slot_tbl: Vec<usize> = {
        let mut tbl = vec![0usize; max_layer_idx + 1];
        for (slot, &layer) in signal_layers.iter().enumerate() {
            tbl[layer] = slot;
        }
        tbl
    };
    let layer_slot = |layer: usize| -> Option<usize> { layer_slot_tbl.get(layer).copied() };
    // Helper: flat index for a node (signal layers only)
    let flat_idx = |n: &Node| -> usize {
        let slot = layer_slot(n.layer).unwrap_or(0);
        (slot * grid.rows + n.row) * grid.cols + n.col
    };
    // Helper: reconstruct Node from packed u32 (signal layer slot → actual layer)
    let unpack_node = |packed: u32| -> Node {
        let col = (packed & 0xFFF) as usize;
        let row = ((packed >> 12) & 0xFFF) as usize;
        let slot = ((packed >> 24) & 0xFF) as usize;
        let layer = signal_layers.get(slot).copied().unwrap_or(signal_layers[0]);
        Node { col, row, layer }
    };
    let pack_node = |n: &Node| -> u32 {
        let slot = layer_slot(n.layer).unwrap_or(0);
        (n.col as u32) | ((n.row as u32) << 12) | ((slot as u32) << 24)
    };

    let si = flat_idx(&start);
    set_g![si, 0.0];
    open.push(OpenNode {
        node: start,
        f: manhattan_heuristic(&start, &goal),
    });

    let mut iterations = 0;

    while let Some(OpenNode { node, .. }) = open.pop() {
        iterations += 1;
        if iterations > max_iters {
            return None;
        }

        // Check if we reached the goal
        if node.col == goal.col && node.row == goal.row {
            let mut path = vec![node];
            let mut ci = flat_idx(&node);
            while get_cf![ci] != u32::MAX {
                let prev = unpack_node(get_cf![ci]);
                path.push(prev);
                ci = flat_idx(&prev);
            }
            path.reverse();
            return Some(path);
        }

        let ci = flat_idx(&node);
        let current_g = get_g![ci];

        // Determine direction from parent for turn cost
        let dir_from_parent = if get_cf![ci] != u32::MAX {
            let parent = unpack_node(get_cf![ci]);
            Some((
                node.col as i32 - parent.col as i32,
                node.row as i32 - parent.row as i32,
            ))
        } else {
            None
        };

        // Expand neighbors (8-directional with K4 jump optimization for cardinal dirs)
        for &(dc, dr, move_cost) in DIRECTIONS {
            // K4: For cardinal directions (horizontal/vertical), jump to farthest clear cell
            let (nc, nr, jump_dist) = if (dc == 0 || dr == 0) && (dc != 0 || dr != 0) {
                let max_jump = 8i32; // Max 8 cells (2mm) jump
                let mut jc = node.col as i32 + dc;
                let mut jr = node.row as i32 + dr;
                let mut dist = 1i32;
                let mut best_c = jc;
                let mut best_r = jr;
                while dist < max_jump {
                    if jc < 0 || jr < 0 || jc as usize >= grid.cols || jr as usize >= grid.rows {
                        break;
                    }
                    // Check if cell is passable on any signal layer
                    let passable = signal_layers.iter().any(|&l| {
                        let c = grid.get(l, jc as usize, jr as usize);
                        matches!(c, Cell::Free)
                            || matches!(c, Cell::Pad(n) if n == net_id || n == 0)
                    });
                    if !passable {
                        break;
                    }
                    best_c = jc;
                    best_r = jr;
                    jc += dc;
                    jr += dr;
                    dist += 1;
                }
                (best_c, best_r, dist - 1)
            } else {
                (node.col as i32 + dc, node.row as i32 + dr, 1)
            };

            if nc < 0 || nr < 0 {
                continue;
            }
            let nc = nc as usize;
            let nr = nr as usize;
            if nc >= grid.cols || nr >= grid.rows {
                continue;
            }

            // Check all signal layers
            for &layer in &signal_layers {
                let neighbor = Node {
                    col: nc,
                    row: nr,
                    layer,
                };

                // Check if center cell is passable
                let cell = grid.get(layer, nc, nr);
                match cell {
                    Cell::Blocked => continue,
                    Cell::Trace(n) | Cell::Via(n) if n != net_id => continue,
                    Cell::Pad(n) if n != net_id && n != 0 => continue,
                    _ => {}
                }

                // Diagonal move: prevent corner-cutting through blocked cells
                if dc != 0 && dr != 0 {
                    let c1 = grid.get(layer, (node.col as i32 + dc) as usize, node.row);
                    let c2 = grid.get(layer, node.col, (node.row as i32 + dr) as usize);
                    let blocked1 = matches!(c1, Cell::Blocked | Cell::Trace(_) | Cell::Via(_))
                        || matches!(c1, Cell::Pad(n) if n != net_id && n != 0);
                    let blocked2 = matches!(c2, Cell::Blocked | Cell::Trace(_) | Cell::Via(_))
                        || matches!(c2, Cell::Pad(n) if n != net_id && n != 0);
                    if blocked1 && blocked2 {
                        continue;
                    }
                }

                // Compute costs (K4: scale by jump distance for cardinal jumps)
                let mut cost = move_cost * jump_dist as f64;

                // Layer change penalty (via)
                if layer != node.layer {
                    if grid.layer_config.blind_buried_vias {
                        let pairs = grid.layer_config.valid_via_pairs();
                        if !pairs.iter().any(|&(a, b)| {
                            (a == node.layer && b == layer) || (b == node.layer && a == layer)
                        }) {
                            continue;
                        }
                        let vt = grid.layer_config.via_type_for(node.layer, layer);
                        cost += match vt {
                            ViaType::Through => VIA_COST,
                            ViaType::Blind => VIA_COST * 0.8,
                            ViaType::Buried => VIA_COST * 0.7,
                        };
                    } else {
                        cost += VIA_COST;
                    }
                }

                // Layer direction preference: F.Cu → horizontal, B.Cu → vertical
                if signal_layers.len() >= 2 {
                    let is_horizontal = dc != 0 && dr == 0;
                    let is_vertical = dc == 0 && dr != 0;
                    if layer == signal_layers[0] && is_vertical {
                        cost += LAYER_DIR_PENALTY;
                    } else if layer == *signal_layers.last().unwrap() && is_horizontal {
                        cost += LAYER_DIR_PENALTY;
                    }
                }

                // High-speed net inner layer preference (EMI reduction)
                if prefer_inner_layer && signal_layers.len() >= 3 {
                    if layer == signal_layers[1] {
                        cost -= 0.3; // bonus for inner signal layer
                    } else if layer == signal_layers[0] || layer == *signal_layers.last().unwrap() {
                        cost += 0.2; // penalty for outer layers
                    }
                }

                // Direction change penalty (encourages straight traces)
                if let Some((pd, _pr)) = dir_from_parent.or(prev_dir) {
                    if dc != pd {
                        cost += TURN_COST;
                    }
                }

                // NCR congestion penalty: history + present, amplified by distance to goal
                let flat = nc + nr * grid.cols;
                let hist = if flat < history_congestion.len() {
                    history_congestion[flat]
                } else {
                    0.0
                };
                let cong = grid.congestion_at(nc, nr);
                if hist > 0.0 || cong > CONGESTION_THRESHOLD {
                    let present = if cong > CONGESTION_THRESHOLD {
                        (cong - CONGESTION_THRESHOLD) as f64
                    } else {
                        0.0
                    };
                    let dist_to_goal = ((nc as i32 - goal.col as i32).unsigned_abs())
                        .max((nr as i32 - goal.row as i32).unsigned_abs())
                        as f64;
                    let congestion_mult = 1.0 + dist_to_goal * 0.05;
                    cost += (hist + CONGESTION_PENALTY * present) * congestion_mult;
                }

                // Proximity penalty: discourage routing adjacent to other nets' traces and pads
                // Check 8-neighbor (1 cell) + 16 outer ring (2 cells) for wider DRC awareness
                let mut adj_other = 0u8;
                let mut adj_outer = 0u8;
                for (dcol, drow) in [
                    (0i32, 1i32),
                    (0, -1),
                    (1, 0),
                    (-1, 0),
                    (1, 1),
                    (1, -1),
                    (-1, 1),
                    (-1, -1),
                ] {
                    let ac = nc as i32 + dcol;
                    let ar = nr as i32 + drow;
                    if ac >= 0 && ar >= 0 && (ac as usize) < grid.cols && (ar as usize) < grid.rows
                    {
                        match grid.get(layer, ac as usize, ar as usize) {
                            Cell::Trace(n) | Cell::Via(n) if n != net_id => adj_other += 1,
                            Cell::Pad(n) if n != net_id && n != 0 => adj_other += 1,
                            _ => {}
                        }
                    }
                }
                // 2-cell ring: weaker penalty for traces 2 cells away
                // Only enable for low-density boards (grid < 500k cells ≈ < 80 footprints)
                // High-density boards have tighter routing — wider avoidance causes excessive detours
                let outer_weight = if grid.cols * grid.rows < 500_000 {
                    0.25
                } else {
                    0.0
                };
                if outer_weight > 0.0 {
                    for (dcol, drow) in [
                        (2, 0),
                        (-2, 0),
                        (0, 2),
                        (0, -2),
                        (2, 1),
                        (2, -1),
                        (-2, 1),
                        (-2, -1),
                        (1, 2),
                        (1, -2),
                        (-1, 2),
                        (-1, -2),
                        (2, 2),
                        (2, -2),
                        (-2, 2),
                        (-2, -2),
                    ] {
                        let ac = nc as i32 + dcol;
                        let ar = nr as i32 + drow;
                        if ac >= 0
                            && ar >= 0
                            && (ac as usize) < grid.cols
                            && (ar as usize) < grid.rows
                        {
                            match grid.get(layer, ac as usize, ar as usize) {
                                Cell::Trace(n) | Cell::Via(n) if n != net_id => adj_outer += 1,
                                Cell::Pad(n) if n != net_id && n != 0 => adj_outer += 1,
                                _ => {}
                            }
                        }
                    }
                } // close outer ring block
                if adj_other > 0 || adj_outer > 0 {
                    cost += PROXIMITY_PENALTY * adj_other as f64
                        + PROXIMITY_PENALTY * outer_weight * adj_outer as f64;
                }

                // Global routing guide bonus — steer through planned corridors
                if let Some(guide) = global_guide {
                    let wx = grid.origin_x + nc as f64 * grid.grid_res;
                    let wy = grid.origin_y + nr as f64 * grid.grid_res;
                    let tc = (((wx - guide.origin_x) / guide.tile_size) as usize)
                        .min(guide.tile_cols - 1);
                    let tr = (((wy - guide.origin_y) / guide.tile_size) as usize)
                        .min(guide.tile_rows - 1);
                    let tile_idx = tc * guide.tile_rows + tr;
                    if let Some(guide_set) = guide.net_guides.get(&net_id) {
                        if guide_set.contains(&tile_idx) {
                            cost -= GLOBAL_GUIDE_BONUS;
                        }
                    }
                }

                let tentative_g = current_g + cost;
                let ni = flat_idx(&neighbor);

                if tentative_g < get_g![ni] {
                    set_g![ni, tentative_g];
                    set_cf![ni, pack_node(&node)];
                    if open.len() < MAX_OPEN_SIZE {
                        open.push(OpenNode {
                            node: neighbor,
                            f: tentative_g + manhattan_heuristic(&neighbor, &goal),
                        });
                    }
                }
            }
        }
    }

    None // No path found
}

/// Try direct straight-line routing between two grid nodes.
/// Uses DDA rasterization to walk grid cells along the line, checking each for obstacles.
/// Returns `Some(vec![start, goal])` if the entire path is clear, otherwise `None`.
fn try_straight_line_route(
    grid: &RoutingGrid,
    start: Node,
    goal: Node,
    net_id: u32,
    layer: usize,
    clearance: f64,
) -> Option<Vec<Node>> {
    if start.col == goal.col && start.row == goal.row {
        return None;
    }

    // Only attempt for short-to-medium distances (< 80 grid cells)
    let dx = (goal.col as i32 - start.col as i32).unsigned_abs();
    let dy = (goal.row as i32 - start.row as i32).unsigned_abs();
    if dx + dy > 80 {
        return None;
    }

    let expansion = (clearance / grid.grid_res).ceil() as i32;

    // Helper: check a single grid cell + clearance zone
    let cell_clear = |col: usize, row: usize| -> bool {
        if !grid.in_bounds(col, row) {
            return false;
        }
        match grid.get(layer, col, row) {
            Cell::Blocked => return false,
            Cell::Trace(n) | Cell::Via(n) | Cell::Pad(n) if n != net_id && n != 0 => return false,
            _ => {}
        }
        // Check clearance zone
        for dc in -expansion..=expansion {
            for dr in -expansion..=expansion {
                if dc == 0 && dr == 0 {
                    continue;
                }
                let nc = col as i32 + dc;
                let nr = row as i32 + dr;
                if nc < 0 || nr < 0 {
                    continue;
                }
                let (nc, nr) = (nc as usize, nr as usize);
                if !grid.in_bounds(nc, nr) {
                    continue;
                }
                match grid.get(layer, nc, nr) {
                    Cell::Blocked => return false,
                    Cell::Trace(n) | Cell::Via(n) | Cell::Pad(n) if n != net_id && n != 0 => {
                        return false
                    }
                    _ => {}
                }
            }
        }
        true
    };

    // DDA (Digital Differential Analyzer) to walk grid cells along the line
    let steps = dx.max(dy).max(1) as usize;
    let step_col = (goal.col as f64 - start.col as f64) / steps as f64;
    let step_row = (goal.row as f64 - start.row as f64) / steps as f64;

    for i in 0..=steps {
        let col = (start.col as f64 + step_col * i as f64).round() as usize;
        let row = (start.row as f64 + step_row * i as f64).round() as usize;
        if !cell_clear(col, row) {
            return None;
        }
    }

    Some(vec![start, goal])
}

/// Try L-shape pattern routing: horizontal-then-vertical or vertical-then-horizontal.
/// Returns a 2-segment path if both legs are clear on the grid, otherwise None.
fn try_l_shape_route(
    grid: &RoutingGrid,
    start: Node,
    goal: Node,
    net_id: u32,
    layer: usize,
    clearance: f64,
) -> Option<Vec<Node>> {
    // Skip if start == goal
    if start.col == goal.col && start.row == goal.row {
        return None;
    }

    // Only use L-shape for short distances (< 60 grid cells = 15mm)
    let dx = (start.col as i32 - goal.col as i32).unsigned_abs();
    let dy = (start.row as i32 - goal.row as i32).unsigned_abs();
    if dx + dy > 60 {
        return None;
    }

    let expansion = (clearance / grid.grid_res).ceil() as usize;

    // Helper: check if a straight line of cells is clear
    let line_clear = |c1: usize, c2: usize, fixed: usize, is_horizontal: bool| -> bool {
        let lo = c1.min(c2);
        let hi = c1.max(c2);
        for c in lo..=hi {
            let (col, row) = if is_horizontal {
                (c, fixed)
            } else {
                (fixed, c)
            };
            if !grid.in_bounds(col, row) {
                return false;
            }
            match grid.get(layer, col, row) {
                Cell::Blocked => return false,
                Cell::Trace(n) | Cell::Via(n) | Cell::Pad(n) if n != net_id && n != 0 => {
                    return false
                }
                _ => {}
            }
            // Check clearance zone
            for dc in -(expansion as i32)..=(expansion as i32) {
                for dr in -(expansion as i32)..=(expansion as i32) {
                    if dc == 0 && dr == 0 {
                        continue;
                    }
                    let nc = col as i32 + dc;
                    let nr = row as i32 + dr;
                    if nc < 0 || nr < 0 {
                        continue;
                    }
                    let (nc, nr) = (nc as usize, nr as usize);
                    if !grid.in_bounds(nc, nr) {
                        continue;
                    }
                    match grid.get(layer, nc, nr) {
                        Cell::Blocked => return false,
                        Cell::Trace(n) | Cell::Via(n) | Cell::Pad(n) if n != net_id && n != 0 => {
                            return false
                        }
                        _ => {}
                    }
                }
            }
        }
        true
    };

    // Corner point for H-then-V: (goal.col, start.row)
    let corner1_clear = grid.in_bounds(goal.col, start.row) && {
        let cell = grid.get(layer, goal.col, start.row);
        matches!(cell, Cell::Free) || matches!(cell, Cell::Pad(n) if n == net_id || n == 0)
    };

    // Option 1: Horizontal first, then vertical (corner at goal.col, start.row)
    if corner1_clear {
        let h_clear = line_clear(start.col, goal.col, start.row, true);
        let v_clear = line_clear(start.row, goal.row, goal.col, false);
        if h_clear && v_clear {
            // Build path: start → corner → goal
            let mut path = Vec::new();
            let lo = start.col.min(goal.col);
            let hi = start.col.max(goal.col);
            for c in lo..=hi {
                path.push(Node {
                    col: c,
                    row: start.row,
                    layer,
                });
            }
            // Vertical: skip first (same as last horizontal)
            let lo = start.row.min(goal.row);
            let hi = start.row.max(goal.row);
            let start_row = if start.row < goal.row {
                if path.last().map(|n| n.row) == Some(lo) {
                    lo + 1
                } else {
                    lo
                }
            } else {
                if path.last().map(|n| n.row) == Some(hi) {
                    hi - 1
                } else {
                    hi
                }
            };
            let step: i32 = if goal.row > start.row { 1 } else { -1 };
            let mut r = start_row as i32;
            loop {
                let ur = r as usize;
                path.push(Node {
                    col: goal.col,
                    row: ur,
                    layer,
                });
                if ur == goal.row {
                    break;
                }
                r += step;
                if (step > 0 && r > goal.row as i32) || (step < 0 && r < goal.row as i32) {
                    break;
                }
            }
            return Some(path);
        }
    }

    // Option 2: Vertical first, then horizontal (corner at start.col, goal.row)
    let corner2_clear = grid.in_bounds(start.col, goal.row) && {
        let cell = grid.get(layer, start.col, goal.row);
        matches!(cell, Cell::Free) || matches!(cell, Cell::Pad(n) if n == net_id || n == 0)
    };

    if corner2_clear {
        let v_clear = line_clear(start.row, goal.row, start.col, false);
        let h_clear = line_clear(start.col, goal.col, goal.row, true);
        if v_clear && h_clear {
            let mut path = Vec::new();
            let lo = start.row.min(goal.row);
            let hi = start.row.max(goal.row);
            for r in lo..=hi {
                path.push(Node {
                    col: start.col,
                    row: r,
                    layer,
                });
            }
            let lo = start.col.min(goal.col);
            let hi = start.col.max(goal.col);
            let start_col = if start.col < goal.col {
                if path.last().map(|n| n.col) == Some(lo) {
                    lo + 1
                } else {
                    lo
                }
            } else {
                if path.last().map(|n| n.col) == Some(hi) {
                    hi - 1
                } else {
                    hi
                }
            };
            let step: i32 = if goal.col > start.col { 1 } else { -1 };
            let mut c = start_col as i32;
            loop {
                let uc = c as usize;
                path.push(Node {
                    col: uc,
                    row: goal.row,
                    layer,
                });
                if uc == goal.col {
                    break;
                }
                c += step;
                if (step > 0 && c > goal.col as i32) || (step < 0 && c < goal.col as i32) {
                    break;
                }
            }
            return Some(path);
        }
    }

    None
}

/// Check if the clearance zone around (col, row) is clear of other nets' obstacles.
#[allow(dead_code)]
fn is_clearance_clear(
    grid: &RoutingGrid,
    layer: usize,
    col: usize,
    row: usize,
    net_id: u32,
    expansion: usize,
) -> bool {
    for dc in -(expansion as i32)..=(expansion as i32) {
        for dr in -(expansion as i32)..=(expansion as i32) {
            if dc == 0 && dr == 0 {
                continue;
            } // Center already checked
            let nc = col as i32 + dc;
            let nr = row as i32 + dr;
            if nc < 0 || nr < 0 {
                continue;
            }
            let (nc, nr) = (nc as usize, nr as usize);
            if !grid.in_bounds(nc, nr) {
                continue;
            }

            match grid.get(layer, nc, nr) {
                Cell::Blocked => return false,
                Cell::Pad(n) | Cell::Trace(n) | Cell::Via(n) if n != net_id && n != 0 => {
                    return false
                }
                _ => {}
            }
        }
    }
    true
}

// ---------------------------------------------------------------------------
// P4: geometric (grid-free) L-routing between exact pad coordinates
// ---------------------------------------------------------------------------

/// Squared distance from point p to segment ab.
fn pt_seg_dist2(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (px, py) = p;
    let (ax, ay) = a;
    let (bx, by) = b;
    let dx = bx - ax;
    let dy = by - ay;
    let len2 = dx * dx + dy * dy;
    let t = if len2 <= 1e-12 {
        0.0
    } else {
        (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0)
    };
    let cx = ax + t * dx;
    let cy = ay + t * dy;
    (px - cx) * (px - cx) + (py - cy) * (py - cy)
}

/// Segment-to-segment distance (squared when disjoint, 0 when intersecting).
fn seg_seg_dist2(a1: (f64, f64), a2: (f64, f64), b1: (f64, f64), b2: (f64, f64)) -> f64 {
    let d1 = (a2.0 - a1.0, a2.1 - a1.1);
    let d2 = (b2.0 - b1.0, b2.1 - b1.1);
    let _r = (a1.0 - b1.0, a1.1 - b1.1);
    let denom = d1.0 * d2.1 - d1.1 * d2.0;
    if denom.abs() > 1e-12 {
        let t = ((b1.0 - a1.0) * d2.1 - (b1.1 - a1.1) * d2.0) / denom;
        let u = ((b1.0 - a1.0) * d1.1 - (b1.1 - a1.1) * d1.0) / denom;
        if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u) {
            return 0.0; // proper intersection
        }
    }
    pt_seg_dist2(a1, b1, b2)
        .min(pt_seg_dist2(a2, b1, b2))
        .min(pt_seg_dist2(b1, a1, a2))
        .min(pt_seg_dist2(b2, a1, a2))
}

/// P4: route one MST edge as a single-layer L-path between the exact pad
/// coordinates, keeping endpoints ON the pads (grid A* snaps endpoints to the
/// grid, which leaves pads starved). Tries both bend orders on the preferred
/// signal layers; every segment is checked geometrically against all other
/// nets' copper. Returns None → caller falls back to the grid router.
#[allow(clippy::too_many_arguments)]
fn geometric_route_edge(
    board: &Board,
    net_id: u32,
    sx: f64,
    sy: f64,
    ex: f64,
    ey: f64,
    trace_width: f64,
    clearance: f64,
    signal_layers: &[usize],
    layer_config: &crate::layer_config::BoardLayerConfig,
    escape: &[((f64, f64), (f64, f64))],
) -> Option<(Vec<Segment>, Vec<Via>)> {
    // Pre-collect other nets' copper in world coords
    let other_segs: Vec<((f64, f64), (f64, f64), f64)> = board
        .segments
        .iter()
        .filter(|s| s.net != net_id && s.net != 0)
        .map(|s| (s.start, s.end, s.width))
        .collect();
    let other_pads: Vec<((f64, f64), f64, u32)> = board
        .footprints
        .iter()
        .flat_map(|fp| {
            let (fx, fy, _) = fp.position;
            fp.pads.iter().filter_map(move |p| {
                let id = p.net?;
                let (ox, oy) = fp.pad_rotated_offset(p);
                let hd = p.size.0.hypot(p.size.1) / 2.0;
                Some(((fx + ox, fy + oy), hd, id))
            })
        })
        .filter(|(_, _, id)| *id != net_id)
        .collect();
    let other_vias: Vec<((f64, f64), f64, u32)> = board
        .vias
        .iter()
        .filter(|v| v.net != net_id && v.net != 0)
        .map(|v| (v.at, v.size / 2.0, v.net))
        .collect();

    let need = clearance + trace_width / 2.0;
    let cand_clear = |segs: &[(f64, f64, f64, f64)]| -> bool {
        for &(x1, y1, x2, y2) in segs {
            // other tracks: edge-to-edge distance
            for &((ox1, oy1), (ox2, oy2), ow) in &other_segs {
                let dist = seg_seg_dist2((x1, y1), (x2, y2), (ox1, oy1), (ox2, oy2)).sqrt();
                if dist < need + ow / 2.0 - 0.01 {
                    return false;
                }
            }
            // other pads: center distance to segment < half-diagonal + need
            for &((px, py), hd, _) in &other_pads {
                let dist = pt_seg_dist2((px, py), (x1, y1), (x2, y2)).sqrt();
                if dist < hd + need - 0.01 {
                    return false;
                }
            }
            // other vias
            for &((vx, vy), vr, _) in &other_vias {
                let dist = pt_seg_dist2((vx, vy), (x1, y1), (x2, y2)).sqrt();
                if dist < vr + need - 0.01 {
                    return false;
                }
            }
        }
        true
    };

    // P4: perpendicular escape stubs for fine-pitch pads. A trace leaving a
    // QFN pad straight out (perpendicular to the IC edge) reaches open space
    // before turning, so the L candidates no longer hug the pad row.
    const STUB: f64 = 0.8;
    let esc_of = |x: f64, y: f64| -> Option<(f64, f64)> {
        escape
            .iter()
            .find(|((px, py), _)| (px - x).hypot(py - y) < 0.05)
            .map(|(_, d)| *d)
    };
    let es = esc_of(sx, sy);
    let ee = esc_of(ex, ey);
    let stub_pt =
        |p: (f64, f64), d: (f64, f64)| -> (f64, f64) { (p.0 + d.0 * STUB, p.1 + d.1 * STUB) };

    for &lay in signal_layers {
        let name = layer_config.layer_name(lay).to_string();
        // Waypoint chains from s to e: plain L (2 bend orders), plus escape
        // stubs on either/both ends when the pad has a known escape direction.
        let mut chains: Vec<Vec<(f64, f64)>> = Vec::new();
        chains.push(vec![(sx, sy), (ex, sy), (ex, ey)]);
        chains.push(vec![(sx, sy), (sx, ey), (ex, ey)]);
        if let Some(ds) = es {
            let s2 = stub_pt((sx, sy), ds);
            chains.push(vec![(sx, sy), s2, (ex, s2.1), (ex, ey)]);
            chains.push(vec![(sx, sy), s2, (s2.0, ey), (ex, ey)]);
        }
        if let Some(de) = ee {
            let e2 = stub_pt((ex, ey), de);
            chains.push(vec![(sx, sy), (ex, sy), e2, (ex, ey)]);
            chains.push(vec![(sx, sy), (sx, ey), e2, (ex, ey)]);
        }
        if let (Some(ds), Some(de)) = (es, ee) {
            let s2 = stub_pt((sx, sy), ds);
            let e2 = stub_pt((ex, ey), de);
            chains.push(vec![(sx, sy), s2, (e2.0, s2.1), e2, (ex, ey)]);
            chains.push(vec![(sx, sy), s2, (s2.0, e2.1), e2, (ex, ey)]);
        }

        for chain in chains {
            let segs: Vec<(f64, f64, f64, f64)> = chain
                .windows(2)
                .map(|w| (w[0].0, w[0].1, w[1].0, w[1].1))
                .filter(|&(x1, y1, x2, y2)| (x1 - x2).hypot(y1 - y2) > 1e-6)
                .collect();
            if segs.is_empty() {
                continue;
            }
            if cand_clear(&segs) {
                let out = segs
                    .into_iter()
                    .map(|(x1, y1, x2, y2)| Segment {
                        start: (x1, y1),
                        end: (x2, y2),
                        width: trace_width,
                        layer: name.clone(),
                        net: net_id,
                    })
                    .collect();
                return Some((out, Vec::new()));
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Path → Board IR conversion
// ---------------------------------------------------------------------------

fn path_to_board_elements(
    path: &[Node],
    grid: &RoutingGrid,
    net_id: u32,
    trace_width: f64,
) -> (Vec<Segment>, Vec<Via>) {
    let mut segments = Vec::new();
    let mut vias = Vec::new();

    if path.len() < 2 {
        return (segments, vias);
    }

    // Walk path and merge collinear same-layer segments
    let mut seg_start = path[0];
    let mut prev = path[0];

    for i in 1..path.len() {
        let curr = path[i];

        // Layer change → emit via + finish segment
        if curr.layer != prev.layer {
            // Finish current segment
            let (sx, sy) = grid.grid_to_world(seg_start.col, seg_start.row);
            let (ex, ey) = grid.grid_to_world(prev.col, prev.row);
            if (sx - ex).abs() > 0.001 || (sy - ey).abs() > 0.001 {
                segments.push(Segment {
                    start: (sx, sy),
                    end: (ex, ey),
                    width: trace_width,
                    layer: grid.layer_config.layer_name(prev.layer).to_string(),
                    net: net_id,
                });
            }

            // Insert via at transition point
            let (vx, vy) = grid.grid_to_world(prev.col, prev.row);
            let (via_size, via_drill) = if grid.layer_config.blind_buried_vias {
                let vt = grid.layer_config.via_type_for(prev.layer, curr.layer);
                grid.layer_config.via_spec(vt)
            } else {
                (0.6, 0.3)
            };
            let via_layer_names = vec![
                grid.layer_config.layer_name(prev.layer).to_string(),
                grid.layer_config.layer_name(curr.layer).to_string(),
            ];
            vias.push(Via {
                at: (vx, vy),
                size: via_size,
                drill: via_drill,
                layers: via_layer_names,
                net: net_id,
            });

            // New layer starts from prev (the via sits there), NOT curr —
            // seg_start = curr dropped the prev→curr hop on the new layer and
            // let the direction check below emit a bogus reversed segment.
            seg_start = prev;
        }

        // Check if direction changed (not collinear with start→prev→curr)
        let dc1 = prev.col as i32 - seg_start.col as i32;
        let dr1 = prev.row as i32 - seg_start.row as i32;
        let dc2 = curr.col as i32 - prev.col as i32;
        let dr2 = curr.row as i32 - prev.row as i32;

        // Direction changed → emit segment from seg_start to prev
        if (dc1, dr1) != (dc2, dr2) && (dc1 != 0 || dr1 != 0) {
            let (sx, sy) = grid.grid_to_world(seg_start.col, seg_start.row);
            let (ex, ey) = grid.grid_to_world(prev.col, prev.row);
            if (sx - ex).abs() > 0.001 || (sy - ey).abs() > 0.001 {
                segments.push(Segment {
                    start: (sx, sy),
                    end: (ex, ey),
                    width: trace_width,
                    layer: grid.layer_config.layer_name(prev.layer).to_string(),
                    net: net_id,
                });
            }
            seg_start = prev;
        }

        prev = curr;
    }

    // Final segment
    let (sx, sy) = grid.grid_to_world(seg_start.col, seg_start.row);
    let (ex, ey) = grid.grid_to_world(prev.col, prev.row);
    if (sx - ex).abs() > 0.001 || (sy - ey).abs() > 0.001 {
        segments.push(Segment {
            start: (sx, sy),
            end: (ex, ey),
            width: trace_width,
            layer: grid.layer_config.layer_name(prev.layer).to_string(),
            net: net_id,
        });
    }

    (segments, vias)
}

// ---------------------------------------------------------------------------
// Public API: Signal Auto-Router
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct RoutingResult {
    pub total_nets: usize,
    pub routed_nets: usize,
    pub total_segments: usize,
    pub total_vias: usize,
    pub failed_nets: Vec<String>,
}

/// Auto-route all signal nets using A* pathfinding with 45° mitered corners.
pub fn auto_route_signal_nets(board: &mut Board, directives: &LayoutDirectives) -> RoutingResult {
    auto_route_signal_nets_with_config(
        board,
        directives,
        &crate::layer_config::BoardLayerConfig::two_layer(),
    )
}

/// Signal routing with explicit layer configuration.
pub fn auto_route_signal_nets_with_config(
    board: &mut Board,
    directives: &LayoutDirectives,
    layer_config: &crate::layer_config::BoardLayerConfig,
) -> RoutingResult {
    // P1-8: CLI clearance override (constant for the whole run) or the default
    let base_clearance = directives.clearance_override.unwrap_or(DEFAULT_CLEARANCE);

    // Perf budget (P0-10 follow-up): gen-pcb must finish in ~90s on battery-class
    // boards, not 4.5min. Cap: 60s ≤100 nets, 180s beyond. ROUTER_BUDGET_SECS overrides.
    let routing_budget_secs: f64 = std::env::var("ROUTER_BUDGET_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| {
            // Safety fuse only — normal-path speed comes from the deterministic
            // caps (rip-up rounds / stagnation / aggressive rounds) so results
            // stay reproducible. Fuse fires on abnormal slowdowns only.
            let sig_net_count = board
                .nets
                .iter()
                .filter(|n| n.id != 0 && !power_net_name(&n.name))
                .count();
            if sig_net_count > 100 {
                600.0
            } else {
                240.0
            }
        });
    let routing_started = std::time::Instant::now();
    let mut result = RoutingResult {
        total_nets: 0,
        routed_nets: 0,
        total_segments: 0,
        total_vias: 0,
        failed_nets: Vec::new(),
    };

    // 1. Build routing grid
    // Use coarser grid for large boards (>200 nets) to reduce memory 4x and avoid macOS page compression OOM.
    let estimated_nets = board.nets.iter().filter(|n| n.id != 0).count();
    // F4: Use finer grid (0.25mm) for large boards with BGA components — the
    // 0.5mm coarse grid only gives 3-4 cells between 0.8mm BGA pitch pads,
    // causing poor routing of DDR3/high-density buses between BGAs. 0.25mm
    // doubles routing channel density (6-8 cells) at 20MB memory (vs 5MB at
    // 0.5mm) — well within budget. Non-BGA large boards keep 0.5mm for speed.
    let has_bga = board.footprints.iter().any(|fp| {
        let lib = fp.lib_id.to_uppercase();
        (lib.contains("BGA") || lib.contains("CSP") || lib.contains("WLCSP") || lib.contains("LGA"))
            && fp.pads.iter().filter(|p| p.net.is_some()).count() >= 16
    });
    let grid_res = if estimated_nets > 200 {
        if has_bga {
            GRID_RES
        } else {
            GRID_RES_LARGE
        } // 0.25mm for BGA boards, 0.5mm otherwise
    } else if estimated_nets <= 30 {
        // Small boards: 0.125mm halves the sub-clearance corner shorts that
        // 0.25mm cannot resolve (memory cost is trivial at this size)
        GRID_RES_FINE
    } else {
        GRID_RES
    };
    let mut grid = RoutingGrid::new(board, layer_config.clone(), grid_res);
    grid.mark_component_bodies(board);
    grid.mark_pads(board);
    grid.mark_existing_traces(board, base_clearance);

    // Memory safety check: estimate peak vs system RAM, auto-downgrade if needed
    {
        let sig_layers = grid.layer_config.signal_layer_indices().len();
        let total_cells = sig_layers * grid.rows * grid.cols;
        let astar_bytes = total_cells * 12; // g_score(8) + came_from(4)
        let grid_bytes = grid.num_layers * grid.rows * grid.cols * 8
            + grid.rows * grid.cols * 2
            + grid.rows * grid.cols * 8;
        // Conservative peak: grid + 2× A* (active + open set) + 50% overhead
        let peak_bytes = (grid_bytes + astar_bytes * 2) as f64 * 1.5;
        let peak_mb = peak_bytes / 1_048_576.0;

        // Try to read system total RAM
        let sys_ram_mb = get_total_memory_mb();
        let mem_budget_mb = sys_ram_mb * 0.9;
        eprintln!("[router] Grid: {}×{} cells, {} signal layers, grid_res={:.2}mm, ~{:.0} MB peak (system RAM: {:.0} MB, budget: {:.0} MB)",
            grid.cols, grid.rows, sig_layers, grid_res, peak_mb, sys_ram_mb, mem_budget_mb);

        if peak_mb > mem_budget_mb {
            eprintln!(
                "[router] WARNING: Peak estimate {:.0} MB > budget {:.0} MB — reducing parameters",
                peak_mb, mem_budget_mb
            );
        }
    }

    // 2. Collect power nets for filtering
    let power_nets: HashSet<u32> = board
        .nets
        .iter()
        .filter(|n| {
            let name = n.name.to_uppercase();
            // GND variants
            let is_gnd = name == "GND"
                || name == "DGND"
                || name == "AGND"
                || name.starts_with("GND_")
                || name.ends_with("_GND")
                || name == "PGND"
                || name == "SGND"
                || name == "EGND"
                || name == "CHASSIS_GND"
                || name.contains("FAN_GND");
            // Power rails
            let is_pwr = name.contains("VIN")
                || name.contains("VCC")
                || name.contains("VDD")
                || name.contains("5V")
                || name.contains("3V3")
                || name.contains("3.3V")
                || name.contains("12V")
                || name.contains("9V")
                || name.contains("VOUT")
                || name.contains("+2V")
                || name.contains("+4V")
                || name.contains("+15V")
                || name.contains("+13V")
                || name.contains("-4V")
                || name.contains("-9V");
            is_gnd || is_pwr
        })
        .map(|n| n.id)
        .collect();

    // P1: zone-aware power filtering. A power/GND net is only excluded from
    // routing when a copper zone actually covers it — otherwise nobody would
    // ever connect it (reroute on a board without zones left every rail
    // unconnected: AFE 499 / battery 40 unconnected). Nets not covered by a
    // zone route like ordinary signals.
    let zoned: HashSet<u32> = board.zones.iter().map(|z| z.net).collect();
    let power_total = power_nets.len();
    let mut power_nets = power_nets;
    let unzoned: Vec<u32> = power_nets.difference(&zoned).copied().collect();
    for id in unzoned {
        power_nets.remove(&id);
    }
    let routed_as_signal = power_total - power_nets.len();

    eprintln!("[router] Board: {} nets, {} footprints, {} power/GND nets ({} zoned-excluded, {} routed as signal), {} segments, {} vias",
        board.nets.len(), board.footprints.len(), power_total,
        power_nets.len(), routed_as_signal,
        board.segments.len(), board.vias.len());

    // 2a. Fan-out multi-pin ICs: place vias outside IC body for each signal pad
    let fanout = fanout_ic_pads(board, &mut grid, &power_nets);

    // F4: Sync fan-out geometry (from fine grid) back onto the coarse main grid.
    // Fan-out ran on a 0.125mm fine grid and wrote segments/vias to board in
    // world coords. The coarse grid needs to know about these obstacles so the
    // main A* router can avoid them. mark_existing_traces re-reads all board
    // segments/vias and marks them at coarse resolution.
    if fanout.count > 0 {
        grid.mark_existing_traces(board, base_clearance);
    }

    // 2b. Collect signal net pads (original pads + fan-out via positions).
    // L6: also collect per-pad PadRole in a sidecar (same net_id, same order)
    // so build_topology_edges can pick fly-by for DDR3 ADDR/CMD/CLK nets.
    let mut pads_by_net: HashMap<u32, Vec<(f64, f64)>> = HashMap::new();
    let mut roles_by_net: HashMap<u32, Vec<PadRole>> = HashMap::new();
    for fp in &board.footprints {
        let role = classify_footprint_role(fp);
        let (fx, fy, _) = fp.position;
        for pad in &fp.pads {
            if let Some(net_id) = pad.net {
                if net_id == 0 || power_nets.contains(&net_id) {
                    continue;
                }
                let (px, py) = fp.pad_rotated_offset(pad);
                pads_by_net
                    .entry(net_id)
                    .or_default()
                    .push((fx + px, fy + py));
                roles_by_net.entry(net_id).or_default().push(role);
            }
        }
    }

    // L6: build the topology context once — read-only from here on.
    let net_name_by_id: HashMap<u32, String> =
        board.nets.iter().map(|n| (n.id, n.name.clone())).collect();
    // Pre-compute the set of DDR3-named net ids — O(1) fly-by gate on the
    // hot path so non-DDR3 nets pay zero topology-analysis overhead.
    let ddr3_net_ids: HashSet<u32> = net_name_by_id
        .iter()
        .filter(|(_, name)| name.to_uppercase().contains("DDR3"))
        .map(|(&id, _)| id)
        .collect();
    // P4: escape directions — pad world position → unit vector from its
    // footprint center (perpendicular to the IC edge for QFN/QFP pads)
    let mut escape_by_net: HashMap<u32, Vec<((f64, f64), (f64, f64))>> = HashMap::new();
    for fp in &board.footprints {
        let (fx, fy, _) = fp.position;
        for pad in &fp.pads {
            let Some(net_id) = pad.net else { continue };
            if net_id == 0 {
                continue;
            }
            let (ox, oy) = fp.pad_rotated_offset(pad);
            let (px, py) = (fx + ox, fy + oy);
            let len = ox.hypot(oy);
            let dir = if len > 1e-6 {
                (ox / len, oy / len)
            } else {
                (1.0, 0.0)
            };
            escape_by_net
                .entry(net_id)
                .or_default()
                .push(((px, py), dir));
        }
    }
    let topo_ctx = TopologyContext {
        roles_by_net,
        net_name_by_id,
        ddr3_net_ids,
        escape_by_net,
    };
    // Count how many nets actually pick fly-by (for the startup log line).
    {
        let flyby_count = pads_by_net
            .iter()
            .filter(|(net_id, pads)| {
                if let Some(roles) = topo_ctx.roles_for(**net_id) {
                    let name = topo_ctx.name_for(**net_id);
                    analyze_net_topology(name, pads, roles).kind == TopologyKind::FlyBy
                } else {
                    false
                }
            })
            .count();
        if flyby_count > 0 {
            eprintln!("[router] L6 topology: {flyby_count} nets use fly-by (DDR3 ADDR/CMD/CLK)");
        }
    }

    // K2: Second-pass BGA escape — retry pads that didn't get fan-out vias
    let mut escape_targets: Vec<(u32, (f64, f64), (f64, f64))> = Vec::new();
    {
        for fp in &board.footprints {
            let (fx, fy, _) = fp.position;
            let lib_upper = fp.lib_id.to_uppercase();
            if !lib_upper.contains("BGA") && !lib_upper.contains("CSP") {
                continue;
            }

            for pad in &fp.pads {
                let Some(net_id) = pad.net else { continue };
                if net_id == 0 || power_nets.contains(&net_id) {
                    continue;
                }
                let (px, py) = fp.pad_rotated_offset(pad);
                let (wx, wy) = (fx + px, fy + py);

                let has_fanout = board
                    .vias
                    .iter()
                    .any(|v| (v.at.0 - wx).hypot(v.at.1 - wy) < 2.0 && v.net == net_id)
                    || board.segments.iter().any(|s| {
                        s.net == net_id
                            && ((s.start.0 - wx).hypot(s.start.1 - wy) < 1.0
                                || (s.end.0 - wx).hypot(s.end.1 - wy) < 1.0)
                    });
                if has_fanout {
                    continue;
                }

                // Find nearest point from both fanout.extra_pads and pads_by_net
                let mut best_dist = f64::MAX;
                let mut best_pos = (wx, wy);
                for pts in [&fanout.extra_pads.get(&net_id), &pads_by_net.get(&net_id)]
                    .into_iter()
                    .flatten()
                {
                    for &(ox, oy) in *pts {
                        let d = (wx - ox).hypot(wy - oy);
                        if d > 0.5 && d < best_dist {
                            best_dist = d;
                            best_pos = (ox, oy);
                        }
                    }
                }
                if best_dist < 20.0 {
                    escape_targets.push((net_id, (wx, wy), best_pos));
                }
            }
        }
        if !escape_targets.is_empty() {
            eprintln!(
                "[router] BGA 2nd-pass escape: {} pads to retry",
                escape_targets.len()
            );
        }
    }

    // Merge fan-out via positions as additional routing endpoints
    let mut sorted_fanout: Vec<_> = fanout.extra_pads.into_iter().collect();
    sorted_fanout.sort_by_key(|(id, _)| *id);
    for (net_id, positions) in sorted_fanout {
        pads_by_net.entry(net_id).or_default().extend(positions);
    }

    // Execute second-pass BGA escape routes (after merge, so board is free)
    {
        let mut second_pass_ok = 0;
        for (net_id, start, goal) in &escape_targets {
            let trace_width = 0.15; // Use narrower trace for second-pass escape
            let routed = route_single_net_with_clearance_and_iters(
                &mut grid,
                *net_id,
                &[*start, *goal],
                trace_width,
                board,
                0.0,
                100_000,
                &[],
                None,
                &topo_ctx,
            );
            if routed {
                second_pass_ok += 1;
            }
        }
        if second_pass_ok > 0 {
            eprintln!(
                "[router] BGA 2nd-pass escape: {}/{} connected",
                second_pass_ok,
                escape_targets.len()
            );
        }
    }

    // Filter: only nets with 2+ pads
    pads_by_net.retain(|_, pads| pads.len() >= 2);

    // I4: Incremental routing — detect already-connected nets and skip them
    {
        let mut already_routed: HashSet<u32> = HashSet::new();
        let mut sorted_net_ids: Vec<&u32> = pads_by_net.keys().collect();
        sorted_net_ids.sort();
        for &net_id in sorted_net_ids {
            let pads = &pads_by_net[&net_id];
            if pads.len() < 2 {
                continue;
            }
            // A net is fully connected if every pad is reachable via existing segments
            let segs: Vec<_> = board.segments.iter().filter(|s| s.net == net_id).collect();
            let vias: Vec<_> = board.vias.iter().filter(|v| v.net == net_id).collect();
            if segs.is_empty() {
                continue;
            }

            // Build connectivity: collect all endpoints from segments + via positions + pad positions
            let mut points: Vec<(f64, f64)> = Vec::new();
            for s in &segs {
                points.push(s.start);
                points.push(s.end);
            }
            for v in &vias {
                points.push(v.at);
            }
            for &p in pads {
                points.push(p);
            }

            // Simple flood-fill connectivity check: grow connected component from pad[0]
            let threshold = grid.grid_res * 2.0;
            let mut connected: HashSet<usize> = HashSet::new();
            connected.insert(0);
            let mut changed = true;
            while changed {
                changed = false;
                for i in 0..points.len() {
                    if connected.contains(&i) {
                        continue;
                    }
                    for &j in &connected {
                        let dx = (points[i].0 - points[j].0).abs();
                        let dy = (points[i].1 - points[j].1).abs();
                        if dx < threshold && dy < threshold {
                            connected.insert(i);
                            changed = true;
                            break;
                        }
                    }
                }
            }

            // Check if all pads are connected
            let pad_count = pads.len();
            let pads_connected = pads
                .iter()
                .enumerate()
                .filter(|(idx, _)| connected.contains(idx))
                .count();
            if pads_connected == pad_count {
                already_routed.insert(net_id);
            }
        }

        if !already_routed.is_empty() {
            let _total = pads_by_net.len();
            pads_by_net.retain(|id, _| !already_routed.contains(id));
            result.routed_nets += already_routed.len();
            eprintln!(
                "[router] I4 incremental: {} nets already routed, {} remaining",
                already_routed.len(),
                pads_by_net.len()
            );
        }
    }

    result.total_nets = pads_by_net.len();
    if pads_by_net.is_empty() {
        return result;
    }

    // 3. Identify differential pairs and route them first (higher priority)
    // 3a. L5: Build impedance-controlled trace width map from SI directives
    let impedance_widths = build_impedance_width_map(board, directives, layer_config);
    if !impedance_widths.is_empty() {
        eprintln!(
            "[router] L5 impedance control: {} nets with controlled widths",
            impedance_widths.len()
        );
    }

    let diff_pairs = identify_diff_pairs(board);
    let mut diff_paired_nets: HashSet<u32> = HashSet::new();
    for &(net_p, net_n, spacing) in &diff_pairs {
        let pads_p: Vec<(f64, f64)> = pads_by_net.get(&net_p).cloned().unwrap_or_default();
        let pads_n: Vec<(f64, f64)> = pads_by_net.get(&net_n).cloned().unwrap_or_default();

        if pads_p.len() >= 2 && pads_n.len() >= 2 {
            let p_name = board
                .nets
                .iter()
                .find(|n| n.id == net_p)
                .map(|n| n.name.as_str())
                .unwrap_or("");
            let trace_width = effective_trace_width(net_p, p_name, directives, &impedance_widths);
            grid.unmark_net(net_p);
            grid.unmark_net(net_n);
            let ok = route_diff_pair(
                &mut grid,
                net_p,
                net_n,
                &pads_p,
                &pads_n,
                trace_width,
                spacing,
                board,
                &topo_ctx,
            );
            if ok {
                result.routed_nets += 2;
                diff_paired_nets.insert(net_p);
                diff_paired_nets.insert(net_n);
            } else {
                // I8: Retry with swapped polarity (P↔N)
                grid.unmark_net(net_p);
                grid.unmark_net(net_n);
                let ok_swapped = route_diff_pair(
                    &mut grid,
                    net_n,
                    net_p,
                    &pads_n,
                    &pads_p,
                    trace_width,
                    spacing,
                    board,
                    &topo_ctx,
                );
                if ok_swapped {
                    result.routed_nets += 2;
                    diff_paired_nets.insert(net_p);
                    diff_paired_nets.insert(net_n);
                }
            }
        }
    }

    // 4. Optimize pad assignments (Hungarian algorithm) + MPS network ordering
    // 4a. Hungarian: optimize swappable pad-net assignments to reduce crossovers
    optimize_pad_assignments(board, &mut pads_by_net, &power_nets, &diff_paired_nets);

    // 4b. Collect remaining nets (exclude already-routed diff pairs)
    let mut remaining_nets: Vec<(u32, Vec<(f64, f64)>)> = pads_by_net
        .into_iter()
        .filter(|(id, _)| !diff_paired_nets.contains(id))
        .collect();

    // 4b2. High-speed net priority + bus grouping (DDR3/SerDes/PCIe/USB3)
    let hs_nets = identify_high_speed_nets(board);
    let bus_groups = identify_bus_groups(board);
    remaining_nets.sort_by(|a, b| {
        let a_hs = hs_nets.contains(&a.0);
        let b_hs = hs_nets.contains(&b.0);
        // Non-HS nets always come after HS nets
        if a_hs != b_hs {
            return b_hs.cmp(&a_hs);
        }
        // Within HS nets, sort by bus group priority (lower = first)
        let a_grp = bus_groups
            .iter()
            .position(|g| g.net_ids.contains(&a.0))
            .unwrap_or(999);
        let b_grp = bus_groups
            .iter()
            .position(|g| g.net_ids.contains(&b.0))
            .unwrap_or(999);
        a_grp.cmp(&b_grp).then_with(|| a.0.cmp(&b.0))
    });

    // 4c. MPS ordering: group into conflict-free rounds with layer preferences
    let sig_count = grid.layer_config.signal_layer_indices().len();
    let (mps_rounds, layer_preferences) = compute_mps_ordering(&remaining_nets, sig_count);
    let mut sorted_nets: Vec<(u32, Vec<(f64, f64)>)> = Vec::new();
    for round_indices in &mps_rounds {
        for &idx in round_indices {
            sorted_nets.push(remaining_nets[idx].clone());
        }
    }

    // 4d. J1: Global routing pre-planning — tile-based congestion estimation
    {
        let tile_size = 5.0;
        let tile_cols = ((grid.cols as f64 * grid.grid_res) / tile_size).ceil() as usize;
        let tile_rows = ((grid.rows as f64 * grid.grid_res) / tile_size).ceil() as usize;
        let tile_cols = tile_cols.max(1);
        let tile_rows = tile_rows.max(1);

        // Cache bbox → tile range for each net (single pass)
        let mut tile_demand: Vec<u16> = vec![0; tile_cols * tile_rows];
        let mut net_tile_ranges: Vec<(usize, usize, usize, usize)> =
            Vec::with_capacity(sorted_nets.len());

        for (_net_id, pads) in &sorted_nets {
            if pads.is_empty() {
                net_tile_ranges.push((0, 0, 0, 0));
                continue;
            }
            let (min_px, min_py, max_px, max_py) = net_bbox(pads);
            let tc0 = (((min_px - grid.origin_x) / tile_size).floor() as usize).min(tile_cols - 1);
            let tr0 = (((min_py - grid.origin_y) / tile_size).floor() as usize).min(tile_rows - 1);
            let tc1 = (((max_px - grid.origin_x) / tile_size).floor() as usize).min(tile_cols - 1);
            let tr1 = (((max_py - grid.origin_y) / tile_size).floor() as usize).min(tile_rows - 1);
            for tc in tc0..=tc1 {
                for tr in tr0..=tr1 {
                    tile_demand[tc + tr * tile_cols] += 1;
                }
            }
            net_tile_ranges.push((tc0, tr0, tc1, tr1));
        }

        // Add footprint obstacle demand
        for fp in &board.footprints {
            let (fx, fy, _) = fp.position;
            let (bw, bh) = crate::layout_engine::infer_body_size(&fp.lib_id, fp.pads.len());
            let tc0 = ((((fx - bw / 2.0) - grid.origin_x) / tile_size).floor() as usize)
                .min(tile_cols - 1);
            let tr0 = ((((fy - bh / 2.0) - grid.origin_y) / tile_size).floor() as usize)
                .min(tile_rows - 1);
            let tc1 = ((((fx + bw / 2.0) - grid.origin_x) / tile_size).floor() as usize)
                .min(tile_cols - 1);
            let tr1 = ((((fy + bh / 2.0) - grid.origin_y) / tile_size).floor() as usize)
                .min(tile_rows - 1);
            for tc in tc0..=tc1 {
                for tr in tr0..=tr1 {
                    tile_demand[tc + tr * tile_cols] += 3;
                }
            }
        }

        // Score: congestion * 0.6 + HPWL_norm * 0.4 — low score routes first
        let max_hpwl = sorted_nets
            .iter()
            .filter(|(_, pads)| pads.len() >= 2)
            .map(|(_, pads)| {
                let (x0, y0, x1, y1) = net_bbox(pads);
                (x1 - x0) + (y1 - y0)
            })
            .fold(0.0f64, f64::max)
            .max(1.0);

        let mut scored_nets: Vec<(usize, f64)> = net_tile_ranges
            .into_iter()
            .enumerate()
            .map(|(idx, (tc0, tr0, tc1, tr1))| {
                let mut cong = 0.0;
                for tc in tc0..=tc1 {
                    for tr in tr0..=tr1 {
                        cong += tile_demand[tc + tr * tile_cols] as f64;
                    }
                }
                let (_, pads) = &sorted_nets[idx];
                let hpwl = if pads.len() >= 2 {
                    let (x0, y0, x1, y1) = net_bbox(pads);
                    ((x1 - x0) + (y1 - y0)) / max_hpwl
                } else {
                    0.0
                };
                let score = cong * 0.6 + hpwl * 0.4;
                (idx, score)
            })
            .collect();

        scored_nets.sort_by(|a, b| {
            a.1.partial_cmp(&b.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        });

        let old_nets = std::mem::take(&mut sorted_nets);
        for (idx, _) in scored_nets {
            sorted_nets.push(old_nets[idx].clone());
        }

        if sorted_nets.len() > 20 {
            let peak = tile_demand.iter().max().copied().unwrap_or(0);
            eprintln!(
                "[router] J1 global: {}x{} tiles, peak_demand={}, nets resorted",
                tile_cols, tile_rows, peak
            );
        }
    }

    // Global routing phase: plan coarse corridors on 2mm tile grid
    let global_guide = if sorted_nets.len() >= 5 {
        let guide = global_route_phase(&grid, board, &sorted_nets, &mps_rounds);
        eprintln!(
            "[router] Global routing: {}x{} tiles, {} nets guided",
            guide.tile_cols,
            guide.tile_rows,
            guide.net_guides.len()
        );
        Some(Arc::new(guide))
    } else {
        None
    };

    // Build net_name lookup
    let net_names: HashMap<u32, String> = sorted_nets
        .iter()
        .map(|(id, _)| {
            let name = board
                .nets
                .iter()
                .find(|n| n.id == *id)
                .map(|n| n.name.clone())
                .unwrap_or_else(|| format!("net_{}", id));
            (*id, name)
        })
        .collect();

    // 5. Route each remaining net (first pass)
    // NCR: history congestion accumulates over all rip-up rounds
    let mut history_congestion: Vec<f64> = vec![0.0; grid.cols * grid.rows];
    const HISTORY_FACTOR: f64 = 0.5;
    // Adaptive iteration limits based on board size
    let first_pass_iters = if sorted_nets.len() > 200 {
        5_000 // Minimal iterations for huge boards to limit memory bloat
    } else if sorted_nets.len() > 100 {
        20_000
    } else {
        MAX_ITERATIONS
    };
    let _total_first_pass = sorted_nets.len();

    // Memory check before parallel routing
    {
        let sig_layers = grid.layer_config.signal_layer_indices().len();
        let total_cells = sig_layers * grid.rows * grid.cols;
        // Per-thread A* cache: g_score(8) + g_version(4) + came_from(4) + cf_version(4) = 20 bytes
        let per_thread_mb = total_cells as f64 * 20.0 / 1_048_576.0;
        let num_threads = rayon::current_num_threads().min(8);
        let total_cache_mb = per_thread_mb * num_threads as f64;
        eprintln!(
            "[router] Parallel routing: {} threads, ~{:.0}MB A* cache per thread, ~{:.0}MB total",
            num_threads, per_thread_mb, total_cache_mb
        );
        let sys_ram_mb = get_total_memory_mb();
        let grid_mb = grid.layer_data.len() as f64 / 1_048_576.0;
        let estimated_peak_mb = grid_mb + total_cache_mb + 100.0; // 100MB for board data
        if estimated_peak_mb > sys_ram_mb * 0.7 {
            eprintln!("[router] WARNING: Parallel routing may use {:.0}MB > 70% of {:.0}MB RAM — using serial fallback",
                estimated_peak_mb, sys_ram_mb);
        }
    }

    // Phase 0: Try GPU wavefront routing for large boards (feature-gated)
    // For boards with >300 nets, attempt GPU-accelerated wavefront for 2-pin nets.
    // GPU results are committed to grid, remaining nets fall through to CPU parallel A*.
    #[cfg(feature = "gpu-router")]
    {
        let gpu_threshold = 300;
        if sorted_nets.len() > gpu_threshold {
            eprintln!(
                "[router] Large board ({} nets > {} threshold) — attempting GPU wavefront",
                sorted_nets.len(),
                gpu_threshold
            );
            match try_gpu_wavefront(&mut grid, &sorted_nets, &net_names, board, &mut result) {
                Ok(gpu_routed) => {
                    if gpu_routed > 0 {
                        eprintln!(
                            "[router] GPU wavefront routed {} nets, remaining → CPU parallel",
                            gpu_routed
                        );
                    }
                }
                Err(e) => {
                    eprintln!("[router] GPU wavefront failed: {} — falling back to CPU", e);
                }
            }
        }
    }

    // Phase 1: Parallel A* pathfinding (grid read-only)
    // Each net routes independently against the initial grid state.
    // This is an approximation: nets don't see each other's paths during parallel phase,
    // but since the grid already has pads/component bodies marked, A* finds valid paths
    // that avoid obstacles. Conflicts are resolved in the serial commit phase.
    let _prev_free = 0.0f64;

    // P-route-2: Use indices into sorted_nets instead of cloning pads
    let route_tasks: Vec<(usize, u32, f64, Option<usize>)> = sorted_nets
        .iter()
        .enumerate()
        .map(|(idx, (net_id, _pads))| {
            let tw = effective_trace_width(
                *net_id,
                net_names[net_id].as_str(),
                directives,
                &impedance_widths,
            );
            let pl = layer_preferences.get(net_id).copied();
            (idx, *net_id, tw, pl)
        })
        .collect();

    // Parallel pathfinding — use Arc for shared read-only access to signal layers
    let sig_layers = std::sync::Arc::new(grid.layer_config.signal_layer_indices());
    let per_layer = grid.rows * grid.cols;
    let astar_total = sig_layers.len() * per_layer;
    let grid_cols = grid.cols;
    let grid_rows = grid.rows;
    let grid_res = grid.grid_res;
    let origin_x = grid.origin_x;
    let origin_y = grid.origin_y;
    let layer_data = &grid.layer_data;
    let _congestion = &grid.congestion;

    // P-route-1: Precompute layer→slot lookup table (avoids iter().position() in hot loop)
    let max_layer = *sig_layers.iter().max().unwrap_or(&0);
    let layer_to_slot: Vec<usize> = {
        let mut tbl = vec![0usize; max_layer + 1];
        for (slot, &layer) in sig_layers.iter().enumerate() {
            tbl[layer] = slot;
        }
        tbl
    };
    let layer_to_slot = std::sync::Arc::new(layer_to_slot);

    // L6: topology context captured by reference into the parallel closure.
    // &TopologyContext is Sync (read-only HashMaps), safe to share across rays.
    let topo_ref = &topo_ctx;
    let parallel_results: Vec<(u32, Option<Vec<Node>>)> = route_tasks
        .par_iter()
        .map(|&(sorted_idx, net_id, _trace_width, preferred_layer)| {
            let sig_layers = sig_layers.clone(); // Arc clone (cheap)
            let layer_to_slot = layer_to_slot.clone();
            let pads = &sorted_nets[sorted_idx].1;
            if pads.len() < 2 {
                return (net_id, None);
            }

            let mut local_cache = AStarCache::new(astar_total);

            let (edges, steiner_pts) = build_topology_edges(pads, net_id, topo_ref);
            let all_pads: Vec<(f64, f64)> = pads
                .iter()
                .copied()
                .chain(steiner_pts.iter().copied())
                .collect();
            let mut path_segments: Vec<Node> = Vec::new();
            let mut all_connected = true;

            for (pi, pj) in edges {
                let (sx, sy) = all_pads[pi];
                let (ex, ey) = all_pads[pj];
                let preferred = if let Some(pl) = preferred_layer {
                    pl
                } else if sig_layers.len() >= 2 {
                    if (sx - ex).abs() >= (sy - ey).abs() {
                        sig_layers[0]
                    } else {
                        *sig_layers.last().unwrap()
                    }
                } else {
                    0
                };

                let start_col = (((sx - origin_x) / grid_res).round() as usize).min(grid_cols - 1);
                let start_row = (((sy - origin_y) / grid_res).round() as usize).min(grid_rows - 1);
                let goal_col = (((ex - origin_x) / grid_res).round() as usize).min(grid_cols - 1);
                let goal_row = (((ey - origin_y) / grid_res).round() as usize).min(grid_rows - 1);

                // Try preferred layer, then alternate
                let alternate = if sig_layers.len() >= 2 && preferred == sig_layers[0] {
                    *sig_layers.last().unwrap()
                } else if sig_layers.len() >= 2 {
                    sig_layers[0]
                } else {
                    0
                };

                let mut path = None;
                for &try_layer in &[preferred, alternate] {
                    let start = Node {
                        col: start_col,
                        row: start_row,
                        layer: try_layer,
                    };
                    let goal = Node {
                        col: goal_col,
                        row: goal_row,
                        layer: try_layer,
                    };

                    // Inline A* using flat grid access (no mutable grid borrow needed)
                    local_cache.version = local_cache.version.wrapping_add(1);
                    let ver = local_cache.version;
                    local_cache.open.clear();
                    let AStarCache {
                        g_score,
                        g_version,
                        came_from,
                        cf_version,
                        open,
                        version: _,
                    } = &mut local_cache;
                    // P-route-1: Use precomputed lookup table instead of linear search
                    let slot_of =
                        |layer: usize| -> usize { *layer_to_slot.get(layer).unwrap_or(&0) };
                    let flat = |n: &Node| -> usize {
                        (slot_of(n.layer) * grid_rows + n.row) * grid_cols + n.col
                    };
                    macro_rules! lg {
                        ($i:expr) => {
                            if g_version[$i] == ver {
                                g_score[$i]
                            } else {
                                f64::MAX
                            }
                        };
                    }
                    macro_rules! sg {
                        ($i:expr, $v:expr) => {{
                            g_score[$i] = $v;
                            g_version[$i] = ver;
                        }};
                    }
                    macro_rules! lcf {
                        ($i:expr) => {
                            if cf_version[$i] == ver {
                                came_from[$i]
                            } else {
                                u32::MAX
                            }
                        };
                    }
                    macro_rules! scf {
                        ($i:expr, $v:expr) => {{
                            came_from[$i] = $v;
                            cf_version[$i] = ver;
                        }};
                    }

                    let si = flat(&start);
                    sg![si, 0.0];
                    open.push(OpenNode {
                        node: start,
                        f: manhattan_heuristic(&start, &goal),
                    });

                    let mut iters = 0;
                    let mut found = None;
                    while let Some(OpenNode { node, .. }) = open.pop() {
                        iters += 1;
                        if iters > first_pass_iters {
                            break;
                        }
                        if node.col == goal.col && node.row == goal.row {
                            let mut p = vec![node];
                            let mut ci = flat(&node);
                            while lcf![ci] != u32::MAX {
                                let packed = lcf![ci];
                                let pc = (packed & 0xFFF) as usize;
                                let pr = ((packed >> 12) & 0xFFF) as usize;
                                let ps = ((packed >> 24) & 0xFF) as usize;
                                let pl = sig_layers.get(ps).copied().unwrap_or(sig_layers[0]);
                                let prev = Node {
                                    col: pc,
                                    row: pr,
                                    layer: pl,
                                };
                                p.push(prev);
                                ci = flat(&prev);
                            }
                            p.reverse();
                            found = Some(p);
                            break;
                        }
                        let ci = flat(&node);
                        let current_g = lg![ci];
                        for &(dc, dr, move_cost) in DIRECTIONS {
                            let nc = node.col as i32 + dc;
                            let nr = node.row as i32 + dr;
                            if nc < 0 || nr < 0 {
                                continue;
                            }
                            let nc = nc as usize;
                            let nr = nr as usize;
                            if nc >= grid_cols || nr >= grid_rows {
                                continue;
                            }
                            for &layer in sig_layers.iter() {
                                let neighbor = Node {
                                    col: nc,
                                    row: nr,
                                    layer,
                                };
                                let fidx = layer * per_layer + nr * grid_cols + nc;
                                let cell = unsafe { *layer_data.get_unchecked(fidx) };
                                match cell {
                                    Cell::Blocked => continue,
                                    Cell::Trace(n) | Cell::Via(n) if n != net_id => continue,
                                    Cell::Pad(n) if n != net_id && n != 0 => continue,
                                    _ => {}
                                }
                                let mut cost = move_cost;
                                if layer != node.layer {
                                    cost += VIA_COST;
                                }
                                // Global guide bonus: prefer cells inside planned corridor
                                if let Some(ref guide) = global_guide {
                                    let wx = origin_x + nc as f64 * grid_res;
                                    let wy = origin_y + nr as f64 * grid_res;
                                    let tc = (((wx - guide.origin_x) / guide.tile_size) as usize)
                                        .min(guide.tile_cols - 1);
                                    let tr = (((wy - guide.origin_y) / guide.tile_size) as usize)
                                        .min(guide.tile_rows - 1);
                                    let tile_idx = tc * guide.tile_rows + tr;
                                    if let Some(guide_set) = guide.net_guides.get(&net_id) {
                                        if guide_set.contains(&tile_idx) {
                                            cost -= GLOBAL_GUIDE_BONUS;
                                        }
                                    }
                                }
                                let ni = flat(&neighbor);
                                let tentative_g = current_g + cost;
                                if tentative_g < lg![ni] {
                                    sg![ni, tentative_g];
                                    let pn = (node.col as u32)
                                        | ((node.row as u32) << 12)
                                        | ((slot_of(node.layer) as u32) << 24);
                                    scf![ni, pn];
                                    if open.len() < 50_000 {
                                        open.push(OpenNode {
                                            node: neighbor,
                                            f: tentative_g + manhattan_heuristic(&neighbor, &goal),
                                        });
                                    }
                                }
                            }
                        }
                    }
                    if found.is_some() {
                        path = found;
                        break;
                    }
                }

                if let Some(p) = path {
                    path_segments.extend_from_slice(&p);
                } else {
                    all_connected = false;
                    break;
                }
            }

            if all_connected && !path_segments.is_empty() {
                (net_id, Some(path_segments))
            } else {
                (net_id, None)
            }
        })
        .collect();

    // Phase 2: Serial commit — write paths to grid and board
    // Check each net's new path for tt-shorts with already-committed segments.
    // Conflicting nets are deferred to rip-up rounds instead of creating immediate DRC violations.
    eprintln!(
        "[router] Parallel pathfinding done, committing {} paths serially",
        parallel_results.len()
    );
    let sig_layer_set: HashSet<usize> = grid
        .layer_config
        .signal_layer_indices()
        .into_iter()
        .collect();
    // Track committed signal segments for incremental conflict check
    let mut committed_segs: Vec<Segment> = Vec::new();
    let mut commit_spatial: HashMap<(i32, i32, String), Vec<usize>> = HashMap::new();
    let commit_bin = SPATIAL_CELL_SIZE;
    let mut commit_skipped = 0usize;

    // Phase 1: compute conflict counts for all results, then sort by ascending conflicts
    // so low-conflict nets commit first (leaving more grid space for harder nets)
    let mut results_indexed: Vec<(usize, u32)> = Vec::with_capacity(parallel_results.len());
    for (i, (_, path_opt)) in parallel_results.iter().enumerate() {
        if path_opt.is_none() {
            continue;
        }
        let path = path_opt.as_ref().unwrap();
        let net_id = parallel_results[i].0;
        let trace_width = effective_trace_width(
            net_id,
            net_names[&net_id].as_str(),
            directives,
            &impedance_widths,
        );
        let (segs, _) = path_to_board_elements(path, &grid, net_id, trace_width);
        let mut new_conflicts = 0u32;
        for seg in &segs {
            if seg.width <= 0.0 || !seg.layer.ends_with(".Cu") {
                continue;
            }
            let (x0, y0) = (seg.start.0.min(seg.end.0), seg.start.1.min(seg.end.1));
            let (x1, y1) = (seg.start.0.max(seg.end.0), seg.start.1.max(seg.end.1));
            for c in (((x0 - 0.5) / commit_bin).floor() as i32)
                ..=((x1 + 0.5) / commit_bin).floor() as i32
            {
                for r in (((y0 - 0.5) / commit_bin).floor() as i32)
                    ..=((y1 + 0.5) / commit_bin).floor() as i32
                {
                    if let Some(nearby) = commit_spatial.get(&(c, r, seg.layer.clone())) {
                        for &ci in nearby {
                            let existing = &committed_segs[ci];
                            if existing.net == net_id {
                                continue;
                            }
                            let min_dist = seg.width / 2.0 + existing.width / 2.0;
                            let d = crate::drc::segment_to_segment_dist(
                                seg.start,
                                seg.end,
                                existing.start,
                                existing.end,
                            );
                            if d < min_dist {
                                new_conflicts += 1;
                                if new_conflicts > 10 {
                                    break;
                                }
                            }
                        }
                    }
                    if new_conflicts > 10 {
                        break;
                    }
                }
                if new_conflicts > 10 {
                    break;
                }
            }
            if new_conflicts > 10 {
                break;
            }
        }
        results_indexed.push((i, new_conflicts));
    }
    results_indexed.sort_by_key(|&(_, c)| c);

    // Phase 2: commit in sorted order, with alternate-layer retry for high-conflict nets
    for (idx, _initial_conflicts) in results_indexed {
        let (net_id, path_opt) = &parallel_results[idx];
        let path = match path_opt {
            Some(p) => p,
            None => {
                result.failed_nets.push(net_names[net_id].clone());
                continue;
            }
        };
        let trace_width = effective_trace_width(
            *net_id,
            net_names[net_id].as_str(),
            directives,
            &impedance_widths,
        );
        let (segs, vias) = path_to_board_elements(path, &grid, *net_id, trace_width);

        // Re-count conflicts against current committed state (grid has changed since Phase 1)
        let mut new_conflicts = 0u32;
        for seg in &segs {
            if seg.width <= 0.0 || !seg.layer.ends_with(".Cu") {
                continue;
            }
            let (x0, y0) = (seg.start.0.min(seg.end.0), seg.start.1.min(seg.end.1));
            let (x1, y1) = (seg.start.0.max(seg.end.0), seg.start.1.max(seg.end.1));
            for c in (((x0 - 0.5) / commit_bin).floor() as i32)
                ..=((x1 + 0.5) / commit_bin).floor() as i32
            {
                for r in (((y0 - 0.5) / commit_bin).floor() as i32)
                    ..=((y1 + 0.5) / commit_bin).floor() as i32
                {
                    if let Some(nearby) = commit_spatial.get(&(c, r, seg.layer.clone())) {
                        for &ci in nearby {
                            let existing = &committed_segs[ci];
                            if existing.net == *net_id {
                                continue;
                            }
                            let min_dist = seg.width / 2.0 + existing.width / 2.0;
                            let d = crate::drc::segment_to_segment_dist(
                                seg.start,
                                seg.end,
                                existing.start,
                                existing.end,
                            );
                            if d < min_dist {
                                new_conflicts += 1;
                                if new_conflicts > 3 {
                                    break;
                                }
                            }
                        }
                    }
                    if new_conflicts > 3 {
                        break;
                    }
                }
                if new_conflicts > 3 {
                    break;
                }
            }
            if new_conflicts > 3 {
                break;
            }
        }

        if new_conflicts > 5 {
            // Try alternate layer before skipping
            let sig_layers = grid.layer_config.signal_layer_indices();
            if sig_layers.len() >= 2 {
                let pads: Vec<(f64, f64)> = board
                    .footprints
                    .iter()
                    .flat_map(|fp| {
                        let (fx, fy, _) = fp.position;
                        fp.pads
                            .iter()
                            .filter_map(move |p| {
                                if p.net == Some(*net_id) {
                                    let (px, py, _) = p.position;
                                    Some((fx + px, fy + py))
                                } else {
                                    None
                                }
                            })
                            .collect::<Vec<_>>()
                    })
                    .collect();
                if pads.len() >= 2 {
                    // Find which layer the current path uses most
                    let layer_counts = grid
                        .layer_config
                        .signal_layer_indices()
                        .iter()
                        .map(|&li| {
                            let layer_name = grid.layer_config.layer_name(li);
                            let count = segs.iter().filter(|s| s.layer == layer_name).count();
                            (li, count)
                        })
                        .collect::<Vec<_>>();
                    let alt_layer = layer_counts
                        .iter()
                        .min_by_key(|(_, c)| *c)
                        .map(|(li, _)| *li);
                    if let Some(alt_li) = alt_layer {
                        let sig_layer_count = sig_layers.len();
                        let astar_total = sig_layer_count * grid.cols * grid.rows;
                        let mut alt_cache = AStarCache {
                            g_score: vec![f64::MAX; astar_total],
                            g_version: vec![0; astar_total],
                            came_from: vec![u32::MAX; astar_total],
                            cf_version: vec![0; astar_total],
                            open: BinaryHeap::with_capacity(4096),
                            version: 0,
                        };
                        let alt_result = pathfind_single_net(
                            &grid,
                            *net_id,
                            &pads,
                            trace_width,
                            board,
                            base_clearance,
                            MAX_ITERATIONS,
                            &[],
                            Some(alt_li),
                            &mut alt_cache,
                            &topo_ctx,
                        );
                        if alt_result.all_connected {
                            // Extract segments from edge_results for conflict counting
                            let alt_segs: Vec<Segment> = alt_result
                                .edge_results
                                .iter()
                                .flat_map(|e| e.segments.iter().cloned())
                                .collect();
                            let alt_vias: Vec<Via> = alt_result
                                .edge_results
                                .iter()
                                .flat_map(|e| e.vias.iter().cloned())
                                .collect();
                            // Count conflicts on alternate path
                            let mut alt_conflicts = 0u32;
                            for seg in &alt_segs {
                                if seg.width <= 0.0 || !seg.layer.ends_with(".Cu") {
                                    continue;
                                }
                                let (sx0, sy0) =
                                    (seg.start.0.min(seg.end.0), seg.start.1.min(seg.end.1));
                                let (sx1, sy1) =
                                    (seg.start.0.max(seg.end.0), seg.start.1.max(seg.end.1));
                                for c in (((sx0 - 0.5) / commit_bin).floor() as i32)
                                    ..=((sx1 + 0.5) / commit_bin).floor() as i32
                                {
                                    for r in (((sy0 - 0.5) / commit_bin).floor() as i32)
                                        ..=((sy1 + 0.5) / commit_bin).floor() as i32
                                    {
                                        if let Some(nearby) =
                                            commit_spatial.get(&(c, r, seg.layer.clone()))
                                        {
                                            for &ci in nearby {
                                                let existing = &committed_segs[ci];
                                                if existing.net == *net_id {
                                                    continue;
                                                }
                                                let min_dist =
                                                    seg.width / 2.0 + existing.width / 2.0;
                                                let d = crate::drc::segment_to_segment_dist(
                                                    seg.start,
                                                    seg.end,
                                                    existing.start,
                                                    existing.end,
                                                );
                                                if d < min_dist {
                                                    alt_conflicts += 1;
                                                    if alt_conflicts > 3 {
                                                        break;
                                                    }
                                                }
                                            }
                                        }
                                        if alt_conflicts > 3 {
                                            break;
                                        }
                                    }
                                    if alt_conflicts > 3 {
                                        break;
                                    }
                                }
                                if alt_conflicts > 3 {
                                    break;
                                }
                            }
                            if alt_conflicts <= 5 {
                                // Use alternate path — commit it
                                for seg in &alt_segs {
                                    board.segments.push(seg.clone());
                                    mark_segment_on_grid(&mut grid, seg, *net_id, base_clearance);
                                    if seg.width > 0.0 && seg.layer.ends_with(".Cu") {
                                        let si = committed_segs.len();
                                        committed_segs.push(seg.clone());
                                        let (sx0, sy0) = (
                                            seg.start.0.min(seg.end.0),
                                            seg.start.1.min(seg.end.1),
                                        );
                                        let (sx1, sy1) = (
                                            seg.start.0.max(seg.end.0),
                                            seg.start.1.max(seg.end.1),
                                        );
                                        for c in ((sx0 / commit_bin).floor() as i32)
                                            ..=((sx1 / commit_bin).floor() as i32)
                                        {
                                            for r in ((sy0 / commit_bin).floor() as i32)
                                                ..=((sy1 / commit_bin).floor() as i32)
                                            {
                                                commit_spatial
                                                    .entry((c, r, seg.layer.clone()))
                                                    .or_default()
                                                    .push(si);
                                            }
                                        }
                                    }
                                }
                                for via in &alt_vias {
                                    board.vias.push(via.clone());
                                    let (gx, gy) = grid.world_to_grid(via.at.0, via.at.1);
                                    for layer in 0..grid.num_layers {
                                        if sig_layer_set.contains(&layer) {
                                            grid.set(layer, gx, gy, Cell::Via(*net_id));
                                        }
                                    }
                                }
                                result.routed_nets += 1;
                                continue;
                            }
                        }
                    }
                }
            }
            commit_skipped += 1;
            result.failed_nets.push(net_names[net_id].clone());
            continue;
        }

        // Commit: add to board, grid, and spatial hash
        for seg in &segs {
            board.segments.push(seg.clone());
            mark_segment_on_grid(&mut grid, seg, *net_id, base_clearance);
            if seg.width > 0.0 && seg.layer.ends_with(".Cu") {
                let si = committed_segs.len();
                committed_segs.push(seg.clone());
                let (x0, y0) = (seg.start.0.min(seg.end.0), seg.start.1.min(seg.end.1));
                let (x1, y1) = (seg.start.0.max(seg.end.0), seg.start.1.max(seg.end.1));
                for c in ((x0 / commit_bin).floor() as i32)..=((x1 / commit_bin).floor() as i32) {
                    for r in ((y0 / commit_bin).floor() as i32)..=((y1 / commit_bin).floor() as i32)
                    {
                        commit_spatial
                            .entry((c, r, seg.layer.clone()))
                            .or_default()
                            .push(si);
                    }
                }
            }
        }
        for via in &vias {
            board.vias.push(via.clone());
            let (gx, gy) = grid.world_to_grid(via.at.0, via.at.1);
            for layer in 0..grid.num_layers {
                if sig_layer_set.contains(&layer) {
                    grid.set(layer, gx, gy, Cell::Via(*net_id));
                }
            }
        }
        result.routed_nets += 1;
    }
    if commit_skipped > 0 {
        eprintln!(
            "[router] Serial commit: {} nets skipped (tt-short conflict), {} committed",
            commit_skipped, result.routed_nets
        );
    }
    // Recount routed_nets from actual segments (catches early-exit and double-counting)
    let signal_seg_nets: HashSet<u32> = board
        .segments
        .iter()
        .map(|s| s.net)
        .filter(|n| !power_nets.contains(n) && *n != 0)
        .collect();
    result.routed_nets = signal_seg_nets.len();
    result.failed_nets.clear();
    for (net_id, _) in &sorted_nets {
        if !signal_seg_nets.contains(net_id) {
            result.failed_nets.push(net_names[net_id].clone());
        }
    }
    eprintln!(
        "[router] First pass: {}/{} routed, {} failed",
        result.routed_nets,
        result.total_nets,
        result.failed_nets.len()
    );

    // 6. Rip-up retry with NCR congestion-based strategy
    let is_large_board = sorted_nets.len() > 100;
    // P2-5 perf: 16 rounds × 300k-iter A* was 2min+ of diminishing returns;
    // 6 rounds keeps completion within noise of the best (measured on battery)
    let mut max_ripup_rounds: usize = if sorted_nets.len() > 200 {
        3
    } else if is_large_board {
        4
    } else {
        6
    };
    let ripup_iter_limit = if sorted_nets.len() > 200 {
        30_000 // Very fast for huge boards
    } else if is_large_board {
        50_000
    } else {
        MAX_ITERATIONS
    };

    // Memory pressure monitor: background thread checks system RAM every 5s
    // Signal: 0=normal, 1=reduce (free<15%), 2=critical (free<5%)
    let mem_pressure = Arc::new(AtomicU8::new(0));
    {
        let mem_pressure_clone = mem_pressure.clone();
        std::thread::spawn(move || loop {
            let free_mb = get_available_memory_mb();
            let total_mb = get_total_memory_mb();
            if total_mb > 0.0 {
                let ratio = free_mb / total_mb;
                let level = if ratio < 0.05 {
                    2
                } else if ratio < 0.15 {
                    1
                } else {
                    0
                };
                mem_pressure_clone.store(level, Ordering::Relaxed);
            }
            std::thread::sleep(std::time::Duration::from_secs(5));
        });
    }

    let mut round = 0;
    // Use take+swap instead of clone to avoid touching new pages.
    // Only save best result as net counts — reconstruct from board at the end.
    let mut best_routed = result.routed_nets;
    let mut best_snapshot: Option<(Vec<Segment>, Vec<Via>)> = None;
    let mut stagnant_rounds: usize = 0;
    let mut net_fail_counter: HashMap<u32, u32> = HashMap::new();

    // H5: Rip-up heatmap — tracks how many times each grid cell has been associated
    // with failed nets. When a region is repeatedly rip-up'd without improvement,
    // we expand the search radius to try different areas.
    let mut rip_up_heatmap: HashMap<(i32, i32), u32> = HashMap::new();
    let heatmap_bin = 4.0; // 4mm bins — coarse granularity for area-level tracking

    while round < max_ripup_rounds && !result.failed_nets.is_empty() {
        let completion = result.routed_nets as f64 / result.total_nets as f64;
        if completion >= 0.95 {
            break;
        }
        if routing_started.elapsed().as_secs_f64() > routing_budget_secs {
            eprintln!("[router] Time budget {:.0}s exhausted — stopping rip-up rounds ({}/{} routed, {} failed)",
                routing_budget_secs, result.routed_nets, result.total_nets, result.failed_nets.len());
            break;
        }

        // Memory pressure: dynamically reduce or stop
        match mem_pressure.load(Ordering::Relaxed) {
            2 => {
                eprintln!(
                    "[router] Memory critical (<5% free) — stopping rip-up early at round {}",
                    round
                );
                break;
            }
            1 => {
                if max_ripup_rounds > 2 {
                    max_ripup_rounds = max_ripup_rounds.saturating_sub(1);
                    eprintln!(
                        "[router] Memory low (<15% free) — reduced max rounds to {}",
                        max_ripup_rounds
                    );
                }
            }
            _ => {}
        }

        // Early exit: if no improvement for 6 consecutive rounds, stop
        // Dynamic stagnation: more failed nets → allow more stagnant rounds
        let max_stagnant = if result.failed_nets.len() > 5 { 4 } else { 3 };
        if stagnant_rounds >= max_stagnant {
            break;
        }

        round += 1;

        // Adaptive clearance: reduce in later rounds for tighter routing.
        // At 0.25mm grid: ceil(0.15/0.25)=1, ceil(0.0/0.25)=0
        // Tight routing (expansion=0) allows traces to squeeze through congested areas.
        // P1-8: an explicit clearance override is respected at every round —
        // the adaptive reduction is a completion fallback, not a user request.
        let current_clearance = if directives.clearance_override.is_some() {
            base_clearance
        } else if round <= 4 {
            base_clearance // expansion=1: normal clearance
        } else if round <= 7 {
            base_clearance * 0.7 // expansion=1: slightly less conservative
        } else {
            0.0 // expansion=0: tight routing from round 8+
        };

        // Collect failed net IDs
        let failed_ids: HashSet<u32> = result
            .failed_nets
            .iter()
            .filter_map(|name| board.nets.iter().find(|n| n.name == *name).map(|n| n.id))
            .collect();

        // P2-5 perf: freeze nets that failed repeatedly — ripping them up again
        // burns 20s/round of 300k-iter A* with zero success (battery: FB_NODE /
        // VBAT_PROT failed identically every round). Deterministic counter.
        let frozen: HashSet<u32> = failed_ids
            .iter()
            .copied()
            .filter(|id| {
                let c = net_fail_counter.entry(*id).or_insert(0);
                *c += 1;
                *c > 2
            })
            .collect();
        if !frozen.is_empty() {
            eprintln!(
                "[router] Frozen {} repeatedly-failed nets (no further rip-up): {:?}",
                frozen.len(),
                result
                    .failed_nets
                    .iter()
                    .filter(|n| board
                        .nets
                        .iter()
                        .any(|m| m.name == **n && frozen.contains(&m.id)))
                    .collect::<Vec<_>>()
            );
        }
        let active_failed: HashSet<u32> = failed_ids.difference(&frozen).copied().collect();
        let _newly_frozen_names: Vec<String> = board
            .nets
            .iter()
            .filter(|n| frozen.contains(&n.id))
            .map(|n| n.name.clone())
            .collect();

        // Find pads of all failed nets
        let failed_pad_positions: Vec<(f64, f64)> = sorted_nets
            .iter()
            .filter(|(id, _)| active_failed.contains(id))
            .flat_map(|(_, pads)| pads.iter().copied())
            .collect();

        // Adaptive rip-up radius: expand in later rounds
        let rip_radius = if is_large_board {
            if round <= 3 {
                4.0
            } else if round <= 5 {
                6.0
            } else {
                8.0
            }
        } else {
            if round <= 4 {
                8.0
            } else if round <= 7 {
                12.0
            } else {
                16.0
            }
        };

        eprintln!(
            "[router] Rip-up round {}: {}/{} routed, {} failed (clearance={:.2}, radius={:.0}mm)",
            round,
            result.routed_nets,
            result.total_nets,
            result.failed_nets.len(),
            current_clearance,
            rip_radius
        );

        // Rip up failed nets + nearby nets within rip_radius of failed pads
        // For large boards: only rip failed nets, skip neighbor pull-in to speed up
        let mut to_rip: HashSet<u32> = active_failed.clone();

        // H5: Only use heatmap guidance when stuck (stagnant_rounds > 0)
        // When making progress, use the standard distance-based neighbor selection
        let max_heat: u32 = if stagnant_rounds > 0 {
            failed_pad_positions
                .iter()
                .map(|&(fx, fy)| {
                    let key = (
                        (fx / heatmap_bin).floor() as i32,
                        (fy / heatmap_bin).floor() as i32,
                    );
                    rip_up_heatmap.get(&key).copied().unwrap_or(0)
                })
                .max()
                .unwrap_or(0)
        } else {
            0
        };
        // Conservative bonus: only activate after 4+ failures in same area, cap at 4mm
        let heat_radius_bonus = if max_heat > 4 {
            ((max_heat as f64 - 3.0) * 1.5).min(4.0)
        } else {
            0.0
        };
        let effective_rip_radius = rip_radius + heat_radius_bonus;

        if !is_large_board {
            let max_neighbors = if sorted_nets.len() > 100 { 3 } else { 5 };
            let mut candidates: Vec<(u32, f64)> = Vec::new();
            let mut seen: HashSet<u32> = active_failed.clone();
            for seg in &board.segments {
                if seen.contains(&seg.net) {
                    continue;
                }
                let (mx, my) = (
                    (seg.start.0 + seg.end.0) / 2.0,
                    (seg.start.1 + seg.end.1) / 2.0,
                );
                let mut min_d = f64::MAX;
                for &(fx, fy) in &failed_pad_positions {
                    let d = (mx - fx).hypot(my - fy);
                    if d < effective_rip_radius {
                        min_d = min_d.min(d);
                    }
                }
                if min_d < effective_rip_radius {
                    candidates.push((seg.net, min_d));
                    seen.insert(seg.net);
                }
            }
            candidates.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
            let n_selected = candidates
                .iter()
                .take(max_neighbors)
                .map(|(n, _)| to_rip.insert(*n))
                .count();
            eprintln!("[router] Neighbor rip-up: {}/{} candidates (heat_max={}, radius={:.0}+{:.0}mm, K={})",
                n_selected, candidates.len(), max_heat, rip_radius, heat_radius_bonus, max_neighbors);
        }

        // NCR: update history congestion — only for cells near failed nets
        for &(fx, fy) in &failed_pad_positions {
            let (fc, fr) = grid.world_to_grid(fx, fy);
            let r = 4; // Only update cells within 4 grid cells of failed pads
            for dc in -r..=r {
                for dr in -r..=r {
                    let nc = (fc as i32 + dc) as usize;
                    let nr = (fr as i32 + dr) as usize;
                    if nc < grid.cols && nr < grid.rows {
                        let idx = nc + nr * grid.cols;
                        let cong = grid.congestion[idx];
                        if cong > CONGESTION_THRESHOLD {
                            history_congestion[idx] +=
                                HISTORY_FACTOR * (cong - CONGESTION_THRESHOLD) as f64;
                        }
                    }
                }
            }

            // H5: Mark this region in the rip-up heatmap
            let key = (
                (fx / heatmap_bin).floor() as i32,
                (fy / heatmap_bin).floor() as i32,
            );
            *rip_up_heatmap.entry(key).or_insert(0) += 1;
        }
        for &net_id in &to_rip {
            rip_up_net(board, &mut grid, net_id);
        }
        // Recount routed_nets from remaining segments
        let signal_seg_nets: HashSet<u32> = board
            .segments
            .iter()
            .map(|s| s.net)
            .filter(|n| !power_nets.contains(n) && *n != 0)
            .collect();
        result.routed_nets = signal_seg_nets.len();

        result.failed_nets.clear();

        // NCR ordering: sort failed nets by history congestion, but keep some randomness
        let mut failed_retry: Vec<&(u32, Vec<(f64, f64)>)> = sorted_nets
            .iter()
            .filter(|(id, _)| active_failed.contains(id))
            .collect();
        let mut neighbor_retry: Vec<&(u32, Vec<(f64, f64)>)> = sorted_nets
            .iter()
            .filter(|(id, _)| to_rip.contains(id) && !failed_ids.contains(id))
            .collect();

        // NCR: sort top 1/3 by history congestion (highest first), shuffle the rest
        let ncr_net_cost = |net: &(u32, Vec<(f64, f64)>)| -> f64 {
            let mut cost = 0.0;
            for &(px, py) in &net.1 {
                let (col, row) = grid.world_to_grid(px, py);
                let idx = col + row * grid.cols;
                if idx < history_congestion.len() {
                    cost += history_congestion[idx];
                }
            }
            cost
        };
        // Sort all by NCR cost
        failed_retry.sort_by(|a, b| {
            let ca = ncr_net_cost(a);
            let cb = ncr_net_cost(b);
            cb.partial_cmp(&ca).unwrap_or(std::cmp::Ordering::Equal)
        });
        // Shuffle the bottom 2/3 for exploration
        let ncr_split = (failed_retry.len() / 3).max(1);
        if failed_retry.len() > ncr_split {
            // P0-10: fixed seed — process::id() randomized retry order per run
            let mut rng = 0x5DEECE66Du64 ^ (round as u64 * 65537);
            for i in (ncr_split..failed_retry.len()).rev() {
                rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
                let range = (i + 1 - ncr_split) as u64;
                let j = ncr_split + (rng % range) as usize;
                failed_retry.swap(i, j);
            }
        }
        // Shuffle neighbor_retry randomly
        let mut rng = round as u64 * 7919;
        for i in (1..neighbor_retry.len()).rev() {
            rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
            let j = (rng % (i as u64 + 1)) as usize;
            neighbor_retry.swap(i, j);
        }

        // Route failed nets first, then neighbors
        let retry_nets: Vec<&(u32, Vec<(f64, f64)>)> =
            failed_retry.into_iter().chain(neighbor_retry).collect();

        // --- Parallel rip-up retry: 3 phases ---
        // Phase 1: Serial unmark (grid mutation)
        for (net_id, _pads) in &retry_nets {
            grid.unmark_net(*net_id);
        }

        // Precompute per-net parameters
        struct NetParams {
            net_id: u32,
            pads: Vec<(f64, f64)>,
            trace_width: f64,
            preferred_layer: Option<usize>,
        }
        let net_params: Vec<NetParams> = retry_nets
            .iter()
            .map(|(net_id, pads)| {
                let trace_width = effective_trace_width(
                    *net_id,
                    net_names[net_id].as_str(),
                    directives,
                    &impedance_widths,
                );
                let swapped_layer = if round >= 2 && active_failed.contains(net_id) {
                    let signal_layers = grid.layer_config.signal_layer_indices();
                    let current = layer_preferences.get(net_id).copied().unwrap_or(0);
                    signal_layers
                        .iter()
                        .find(|&&l| l != current)
                        .copied()
                        .or_else(|| signal_layers.first().copied())
                } else {
                    layer_preferences.get(net_id).copied()
                };
                NetParams {
                    net_id: *net_id,
                    pads: pads.clone(),
                    trace_width,
                    preferred_layer: swapped_layer,
                }
            })
            .collect();

        // Phase 2: Parallel A* pathfinding (grid read-only)
        let sig_layers_arc = Arc::new(grid.layer_config.signal_layer_indices());
        let astar_total = sig_layers_arc.len() * grid.rows * grid.cols;
        let grid_ref: &RoutingGrid = &grid; // shared ref for parallel read
        let topo_ctx_ref = &topo_ctx; // L6: shared read-only topology context

        let parallel_results: Vec<NetPathResult> = net_params
            .par_iter()
            .map(|params| {
                let mut local_cache = AStarCache::new(astar_total);
                pathfind_single_net(
                    grid_ref,
                    params.net_id,
                    &params.pads,
                    params.trace_width,
                    board,
                    current_clearance,
                    ripup_iter_limit,
                    &history_congestion,
                    params.preferred_layer,
                    &mut local_cache,
                    topo_ctx_ref,
                )
            })
            .collect();

        // Phase 3: Serial commit (grid mutation)
        for pf_result in &parallel_results {
            if pf_result.all_connected {
                commit_net_path(&mut grid, board, pf_result, current_clearance);
            } else {
                result
                    .failed_nets
                    .push(net_names[&pf_result.net_id].clone());
            }
        }

        // Recount routed_nets from segments (avoids double-counting)
        let signal_seg_nets2: HashSet<u32> = board
            .segments
            .iter()
            .map(|s| s.net)
            .filter(|n| !power_nets.contains(n) && *n != 0)
            .collect();
        result.routed_nets = signal_seg_nets2.len();

        // Update best-so-far if improved
        if result.routed_nets > best_routed {
            eprintln!(
                "[router] New best: {}/{}",
                result.routed_nets, result.total_nets
            );
            best_routed = result.routed_nets;
            best_snapshot = Some((board.segments.clone(), board.vias.clone()));
            stagnant_rounds = 0;
        } else {
            stagnant_rounds += 1;
        }
    }

    // Roll back to best state if rip-up degraded below best
    if result.routed_nets < best_routed {
        if let Some((best_segs, best_vias)) = best_snapshot.take() {
            eprintln!(
                "[router] Rolling back to best state ({}/{} vs current {}/{})",
                best_routed, result.total_nets, result.routed_nets, result.total_nets
            );
            board.segments = best_segs;
            board.vias = best_vias;
            // Rebuild grid from restored board state
            grid = RoutingGrid::new(board, layer_config.clone(), grid_res);
            grid.mark_component_bodies(board);
            grid.mark_pads(board);
            let sig_layers: Vec<usize> = grid.layer_config.signal_layer_indices();
            for seg in &board.segments {
                if seg.net != 0 && !power_nets.contains(&seg.net) {
                    mark_segment_on_grid(&mut grid, seg, seg.net, base_clearance);
                }
            }
            for via in &board.vias {
                let (gx, gy) = grid.world_to_grid(via.at.0, via.at.1);
                if gx < grid.cols && gy < grid.rows {
                    for &layer in &sig_layers {
                        grid.set(layer, gx, gy, Cell::Via(via.net));
                    }
                }
            }
            result.routed_nets = best_routed;
        }
    }

    // Rebuild accurate failed_nets from segments (fixes stale/inaccurate tracking)
    {
        let routed_net_ids: HashSet<u32> = board
            .segments
            .iter()
            .map(|s| s.net)
            .filter(|n| !power_nets.contains(n) && *n != 0)
            .collect();
        result.routed_nets = routed_net_ids.len();
        // Collect all signal net IDs (sorted_nets + diff_paired_nets that were routed)
        let all_signal_net_ids: HashSet<u32> = board
            .nets
            .iter()
            .filter(|n| !power_nets.contains(&n.id) && n.id != 0)
            .filter(|n| {
                // Check if this net has 2+ pads
                let mut pad_count = 0;
                for fp in &board.footprints {
                    for pad in &fp.pads {
                        if pad.net == Some(n.id) {
                            pad_count += 1;
                        }
                    }
                }
                pad_count >= 2
            })
            .map(|n| n.id)
            .collect();
        result.total_nets = all_signal_net_ids.len();
        // P0-10: iterate board.nets (Vec) — HashSet order randomized the
        // failed_nets sequence and everything downstream
        result.failed_nets = board
            .nets
            .iter()
            .filter(|n| all_signal_net_ids.contains(&n.id) && !routed_net_ids.contains(&n.id))
            .map(|n| n.name.clone())
            .collect();
        eprintln!(
            "[router] After rebuild: {}/{} routed, {} failed",
            result.routed_nets,
            result.total_nets,
            result.failed_nets.len()
        );
        if !result.failed_nets.is_empty() {
            eprintln!("[router] Failed: {:?}", result.failed_nets);
        }
    }

    // 6b. Desperation pass: try routing each remaining failed net individually
    let aggressive_iters = if is_large_board {
        50_000
    } else {
        DESPERATION_MAX_ITERATIONS
    };
    // Skip desperation for large boards with many failures — not worth the time
    if !result.failed_nets.is_empty() && result.failed_nets.len() < 50 {
        let mut desperate_routed = 0;
        let mut still_failed: Vec<String> = Vec::new();

        for failed_name in &result.failed_nets {
            let net_id = board
                .nets
                .iter()
                .find(|n| &n.name == failed_name)
                .map(|n| n.id);
            let Some(net_id) = net_id else { continue };

            let pads: Vec<(f64, f64)> = sorted_nets
                .iter()
                .find(|(id, _)| *id == net_id)
                .map(|(_, p)| p.clone())
                .unwrap_or_default();
            if pads.len() < 2 {
                continue;
            }

            grid.unmark_net(net_id);
            rip_up_net(board, &mut grid, net_id);

            let trace_width =
                effective_trace_width(net_id, failed_name.as_str(), directives, &impedance_widths);
            let routed = route_single_net_with_clearance_and_iters(
                &mut grid,
                net_id,
                &pads,
                trace_width,
                board,
                base_clearance * 0.3,
                aggressive_iters,
                &history_congestion,
                None,
                &topo_ctx,
            );

            if routed {
                desperate_routed += 1;
            } else {
                still_failed.push(failed_name.clone());
            }
        }

        if desperate_routed > 0 {
            result.routed_nets += desperate_routed;
            result.failed_nets = still_failed;
            eprintln!("[router] Desperation pass: +{} nets", desperate_routed);
        }
    }

    // 6c. Aggressive desperation: only for small/medium boards with few failures
    let aggressive_rounds = 1; // P2-5 perf: desperation rounds were 96s for +0 nets on battery
    let pre_aggressive_snapshot = (board.segments.clone(), board.vias.clone());
    let pre_aggressive_routed = result.routed_nets;
    let mut aggressive_best = result.routed_nets;
    let mut aggressive_best_snapshot: Option<(Vec<Segment>, Vec<Via>)> = None;
    for _aggressive_round in 0..aggressive_rounds {
        if result.failed_nets.is_empty() || result.failed_nets.len() >= 50 {
            break;
        }
        if routing_started.elapsed().as_secs_f64() > routing_budget_secs {
            eprintln!("[router] Time budget {:.0}s exhausted — skipping aggressive desperation ({} failed remain)",
                routing_budget_secs, result.failed_nets.len());
            break;
        }
        eprintln!(
            "[router] Aggressive desperation (round {}): attempting {} failed nets",
            _aggressive_round + 1,
            result.failed_nets.len()
        );

        // Collect pads for failed nets from ALL sources (sorted_nets + diff_paired + original signal_nets)
        // Build a comprehensive pads lookup from board footprints
        let mut all_pads_by_net: HashMap<u32, Vec<(f64, f64)>> = HashMap::new();
        for fp in &board.footprints {
            let (fx, fy, _) = fp.position;
            for pad in &fp.pads {
                if let Some(net_id) = pad.net {
                    if net_id == 0 || power_nets.contains(&net_id) {
                        continue;
                    }
                    let (px, py) = fp.pad_rotated_offset(pad);
                    all_pads_by_net
                        .entry(net_id)
                        .or_default()
                        .push((fx + px, fy + py));
                }
            }
        }

        // Collect all failed net IDs and pads
        let mut failed_data: Vec<(u32, String, Vec<(f64, f64)>)> = Vec::new();
        let mut bx_min = f64::MAX;
        let mut bx_max = f64::MIN;
        let mut by_min = f64::MAX;
        let mut by_max = f64::MIN;
        for failed_name in &result.failed_nets {
            let net_id = board
                .nets
                .iter()
                .find(|n| &n.name == failed_name)
                .map(|n| n.id);
            let Some(net_id) = net_id else { continue };
            // Try sorted_nets first, then fallback to all_pads_by_net
            let pads = sorted_nets
                .iter()
                .find(|(id, _)| *id == net_id)
                .map(|(_, p)| p.clone())
                .unwrap_or_else(|| all_pads_by_net.get(&net_id).cloned().unwrap_or_default());
            if pads.len() < 2 {
                continue;
            }
            for &(px, py) in &pads {
                bx_min = bx_min.min(px);
                bx_max = bx_max.max(px);
                by_min = by_min.min(py);
                by_max = by_max.max(py);
            }
            failed_data.push((net_id, failed_name.clone(), pads));
        }

        // Expand bounding box by adaptive margin (proportional to span, clamped)
        let board_span = (bx_max - bx_min).max(by_max - by_min);
        let margin = (board_span * 0.1).clamp(4.0, 12.0);
        bx_min -= margin;
        bx_max += margin;
        by_min -= margin;
        by_max += margin;

        // Find blocking nets within combined bounding box — limit to top-K closest
        let failed_ids: HashSet<u32> = failed_data.iter().map(|(id, _, _)| *id).collect();
        let failed_pad_set: Vec<(f64, f64)> = failed_data
            .iter()
            .flat_map(|(_, _, pads)| pads.iter().copied())
            .collect();
        let max_blockers = if sorted_nets.len() > 100 { 5 } else { 8 };
        let mut blocker_candidates: Vec<(u32, f64)> = Vec::new();
        let mut blocker_seen: HashSet<u32> = failed_ids.clone();
        for seg in &board.segments {
            if blocker_seen.contains(&seg.net) || seg.net == 0 || power_nets.contains(&seg.net) {
                continue;
            }
            let (mx, my) = (
                (seg.start.0 + seg.end.0) / 2.0,
                (seg.start.1 + seg.end.1) / 2.0,
            );
            if mx >= bx_min && mx <= bx_max && my >= by_min && my <= by_max {
                let mut min_d = f64::MAX;
                for &(px, py) in &failed_pad_set {
                    min_d = min_d.min((mx - px).hypot(my - py));
                }
                blocker_candidates.push((seg.net, min_d));
                blocker_seen.insert(seg.net);
            }
        }
        blocker_candidates
            .sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        let blocking_nets: HashSet<u32> = blocker_candidates
            .iter()
            .take(max_blockers)
            .map(|(n, _)| *n)
            .collect();

        // Save blocking nets' pads for re-routing (use all_pads_by_net for completeness)
        // P0-10: iterate the distance-sorted candidate Vec, NOT the HashSet —
        // HashSet iteration order varies per run and randomized rip-up order.
        let blocking_pads: Vec<(u32, Vec<(f64, f64)>)> = blocker_candidates
            .iter()
            .take(max_blockers)
            .filter_map(|&(bid, _)| {
                let pads = sorted_nets
                    .iter()
                    .find(|(id, _)| *id == bid)
                    .map(|(_, p)| p.clone())
                    .or_else(|| all_pads_by_net.get(&bid).cloned());
                pads.map(|p| (bid, p))
            })
            .collect();

        eprintln!("[router] Aggressive: {} failed nets, {} blocking nets ({}/{} candidates, K={}) in {:.0}x{:.0}mm area",
            failed_data.len(), blocking_nets.len(),
            blocking_nets.len(), blocker_candidates.len(), max_blockers,
            bx_max - bx_min, by_max - by_min);

        // Rip up all failed nets and blocking nets
        for &(net_id, _, _) in &failed_data {
            grid.unmark_net(net_id);
            rip_up_net(board, &mut grid, net_id);
        }
        for &bid in &blocking_nets {
            grid.unmark_net(bid);
            rip_up_net(board, &mut grid, bid);
        }

        // Route all failed nets first
        let mut aggressive_routed = 0;
        let mut still_failed: Vec<String> = Vec::new();
        for (net_id, name, pads) in &failed_data {
            let trace_width =
                effective_trace_width(*net_id, name.as_str(), directives, &impedance_widths);
            let routed = route_single_net_with_clearance_and_iters(
                &mut grid,
                *net_id,
                pads,
                trace_width,
                board,
                base_clearance * 0.3,
                aggressive_iters,
                &history_congestion,
                None,
                &topo_ctx,
            );
            if routed {
                aggressive_routed += 1;
            } else {
                still_failed.push(name.clone());
            }
        }

        // Re-route blocking nets
        let mut re_route_ok = 0;
        for (bid, bpads) in &blocking_pads {
            let bname = board
                .nets
                .iter()
                .find(|n| n.id == *bid)
                .map(|n| n.name.as_str())
                .unwrap_or("");
            let btw = effective_trace_width(*bid, bname, directives, &impedance_widths);
            grid.unmark_net(*bid);
            if route_single_net_with_clearance_and_iters(
                &mut grid,
                *bid,
                bpads,
                btw,
                board,
                base_clearance * 0.3,
                aggressive_iters,
                &history_congestion,
                None,
                &topo_ctx,
            ) {
                re_route_ok += 1;
            }
        }

        // Report results
        let signal_seg_nets: HashSet<u32> = board
            .segments
            .iter()
            .map(|s| s.net)
            .filter(|n| !power_nets.contains(n) && *n != 0)
            .collect();
        result.routed_nets = signal_seg_nets.len();

        // Rebuild failed_nets from segments
        let all_signal_net_ids: HashSet<u32> = board
            .nets
            .iter()
            .filter(|n| !power_nets.contains(&n.id) && n.id != 0)
            .filter(|n| {
                let mut pad_count = 0;
                for fp in &board.footprints {
                    for pad in &fp.pads {
                        if pad.net == Some(n.id) {
                            pad_count += 1;
                        }
                    }
                }
                pad_count >= 2
            })
            .map(|n| n.id)
            .collect();
        result.total_nets = all_signal_net_ids.len();
        // P0-10: deterministic order (HashSet iteration randomized this list)
        result.failed_nets = board
            .nets
            .iter()
            .filter(|n| all_signal_net_ids.contains(&n.id) && !signal_seg_nets.contains(&n.id))
            .map(|n| n.name.clone())
            .collect();

        eprintln!("[router] Aggressive: {}/{} routed, {} still failed (+{} failed, {}/{} blockers restored)",
            result.routed_nets, result.total_nets, result.failed_nets.len(),
            aggressive_routed, re_route_ok, blocking_pads.len());

        // Track best and break on degradation
        if result.routed_nets > aggressive_best {
            aggressive_best = result.routed_nets;
            aggressive_best_snapshot = Some((board.segments.clone(), board.vias.clone()));
        } else if result.routed_nets < aggressive_best {
            break; // degrading, stop aggressive
        }
    }

    // Roll back aggressive if it degraded below pre-aggressive state
    if result.routed_nets < pre_aggressive_routed {
        if let Some((segs, vias)) = aggressive_best_snapshot.or(Some(pre_aggressive_snapshot)) {
            eprintln!(
                "[router] Aggressive rollback: {}/{} → {}/{}",
                result.routed_nets, result.total_nets, pre_aggressive_routed, result.total_nets
            );
            board.segments = segs;
            board.vias = vias;
            let lc = grid.layer_config.clone();
            let gr = grid.grid_res;
            let sig_layers: Vec<usize> = lc.signal_layer_indices();
            grid = RoutingGrid::new(board, lc, gr);
            grid.mark_component_bodies(board);
            grid.mark_pads(board);
            for seg in &board.segments {
                if seg.net != 0 && !power_nets.contains(&seg.net) {
                    mark_segment_on_grid(&mut grid, seg, seg.net, base_clearance);
                }
            }
            for via in &board.vias {
                let (gx, gy) = grid.world_to_grid(via.at.0, via.at.1);
                if gx < grid.cols && gy < grid.rows {
                    for &layer in &sig_layers {
                        grid.set(layer, gx, gy, Cell::Via(via.net));
                    }
                }
            }
            result.routed_nets = pre_aggressive_routed;
            // Rebuild failed_nets
            let routed_ids: HashSet<u32> = board
                .segments
                .iter()
                .map(|s| s.net)
                .filter(|n| !power_nets.contains(n) && *n != 0)
                .collect();
            result.failed_nets = board
                .nets
                .iter()
                .filter(|n| !power_nets.contains(&n.id) && n.id != 0)
                .filter(|n| !routed_ids.contains(&n.id))
                .filter_map(|n| {
                    let mut pc = 0;
                    for fp in &board.footprints {
                        for p in &fp.pads {
                            if p.net == Some(n.id) {
                                pc += 1;
                            }
                        }
                    }
                    if pc >= 2 {
                        Some(n.name.clone())
                    } else {
                        None
                    }
                })
                .collect();
        }
    }

    // J5: Layout-routing feedback — nudge footprints of failed nets and retry
    if !result.failed_nets.is_empty() && result.failed_nets.len() < result.total_nets / 2 {
        // Identify footprints involved in failed nets
        let failed_net_ids: HashSet<u32> = result
            .failed_nets
            .iter()
            .filter_map(|name| board.nets.iter().find(|n| n.name == *name).map(|n| n.id))
            .collect();

        let mut nudge_fps: Vec<usize> = Vec::new();
        for (fi, fp) in board.footprints.iter().enumerate() {
            // Skip connectors — their anchored positions should not be modified
            if fp.lib_id.contains("Conn") || fp.reference.starts_with('J') {
                continue;
            }
            let has_failed_pad = fp
                .pads
                .iter()
                .any(|p| p.net.map(|n| failed_net_ids.contains(&n)).unwrap_or(false));
            if has_failed_pad {
                nudge_fps.push(fi);
            }
        }

        if !nudge_fps.is_empty() && nudge_fps.len() <= 20 {
            eprintln!(
                "[router] J5: {} footprints involved in {} failed nets",
                nudge_fps.len(),
                result.failed_nets.len()
            );
            // Get board outline bounds for clamping nudged positions
            let outline = crate::design::extract_board_outline(board);
            let (bx_min, bx_max, by_min, by_max) = if outline.len() >= 2 {
                let xs: Vec<f64> = outline.iter().map(|p| p.0).collect();
                let ys: Vec<f64> = outline.iter().map(|p| p.1).collect();
                let (x0, x1) = xs
                    .iter()
                    .cloned()
                    .fold((f64::MAX, f64::MIN), |(a, b), v| (a.min(v), b.max(v)));
                let (y0, y1) = ys
                    .iter()
                    .cloned()
                    .fold((f64::MAX, f64::MIN), |(a, b), v| (a.min(v), b.max(v)));
                (x0, x1, y0, y1)
            } else {
                (f64::MIN, f64::MAX, f64::MIN, f64::MAX)
            };

            // Nudge involved footprints by small random offsets to open new routing paths
            for &fi in &nudge_fps {
                let fp = &board.footprints[fi];
                let (fx, fy, fr) = fp.position;

                // Compute clearance from max pad offset + pad half-size
                let pad_margin: f64 = fp
                    .pads
                    .iter()
                    .map(|p| {
                        p.position.0.abs().max(p.position.1.abs()) + p.size.0.max(p.size.1) / 2.0
                    })
                    .fold(0.0f64, f64::max)
                    .max(1.5); // at least 1.5mm

                let offset_range = (grid.cols as f64 * grid.grid_res * 0.03).min(3.0);
                // Try multiple offset directions, pick the one farthest from other footprints
                let offsets: [(f64, f64); 8] = [
                    (offset_range, 0.0),
                    (-offset_range, 0.0),
                    (0.0, offset_range),
                    (0.0, -offset_range),
                    (offset_range * 0.7, offset_range * 0.7),
                    (-offset_range * 0.7, offset_range * 0.7),
                    (offset_range * 0.7, -offset_range * 0.7),
                    (-offset_range * 0.7, -offset_range * 0.7),
                ];
                let (best_dx, best_dy) = offsets
                    .iter()
                    .map(|&(dx, dy)| {
                        let nx = (fx + dx).max(bx_min + pad_margin).min(bx_max - pad_margin);
                        let ny = (fy + dy).max(by_min + pad_margin).min(by_max - pad_margin);
                        // Score: min distance to other footprints (prefer more open space)
                        let min_d = board
                            .footprints
                            .iter()
                            .enumerate()
                            .filter(|&(j, _)| j != fi)
                            .map(|(_, other)| {
                                let (ox, oy, _) = other.position;
                                (nx - ox).hypot(ny - oy)
                            })
                            .fold(f64::MAX, f64::min);
                        (dx, dy, min_d)
                    })
                    .max_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal))
                    .map(|(dx, dy, _)| (dx, dy))
                    .unwrap_or((offset_range, 0.0));
                let new_x = (fx + best_dx)
                    .max(bx_min + pad_margin)
                    .min(bx_max - pad_margin);
                let new_y = (fy + best_dy)
                    .max(by_min + pad_margin)
                    .min(by_max - pad_margin);
                board.footprints[fi].position = (new_x, new_y, fr);
            }

            // Rebuild grid and retry failed nets only
            let mut retry_grid = RoutingGrid::new(board, layer_config.clone(), grid_res);
            retry_grid.mark_component_bodies(board);
            retry_grid.mark_pads(board);
            retry_grid.mark_existing_traces(board, base_clearance);

            // Rotation attempt: for few failures, try 90° rotation on ICs
            if result.failed_nets.len() <= 7 {
                let rotation_candidates: Vec<usize> = nudge_fps
                    .iter()
                    .filter(|&&fi| {
                        let fp = &board.footprints[fi];
                        let lib = fp.lib_id.to_uppercase();
                        lib.contains("SOIC")
                            || lib.contains("QFP")
                            || lib.contains("TSSOP")
                            || lib.contains("SSOP")
                            || lib.contains("SOP")
                            || lib.contains("QFN")
                            || lib.contains("DFN")
                            || lib.contains("MSOP")
                            || lib.contains("BGA")
                            || lib.contains("SOT-23-5")
                            || lib.contains("SOT-23-6")
                    })
                    .copied()
                    .collect();

                for &fi in &rotation_candidates {
                    // Snapshot original state
                    let (fx, fy, fr) = board.footprints[fi].position;
                    let orig_pads: Vec<_> = board.footprints[fi]
                        .pads
                        .iter()
                        .map(|p| p.position)
                        .collect();
                    let ref_name = board.footprints[fi].reference.clone();
                    // Collect failed net IDs on this footprint
                    let fp_net_ids: HashSet<u32> = board.footprints[fi]
                        .pads
                        .iter()
                        .filter_map(|p| p.net)
                        .filter(|n| failed_net_ids.contains(n))
                        .collect();

                    for &rot_delta in &[90.0_f64, 180.0, 270.0] {
                        let rad = rot_delta.to_radians();
                        let (cos_r, sin_r) = (rad.cos(), rad.sin());
                        // Apply rotation to pads
                        for (pi, &(px, py, _)) in orig_pads.iter().enumerate() {
                            let new_px = px * cos_r - py * sin_r;
                            let new_py = px * sin_r + py * cos_r;
                            board.footprints[fi].pads[pi].position = (new_px, new_py, 0.0);
                        }
                        board.footprints[fi].position = (fx, fy, (fr + rot_delta) % 360.0);

                        // Rebuild grid for this rotation
                        let mut rot_grid = RoutingGrid::new(board, layer_config.clone(), grid_res);
                        rot_grid.mark_component_bodies(board);
                        rot_grid.mark_pads(board);
                        rot_grid.mark_existing_traces(board, base_clearance);

                        let mut rot_ok = 0;
                        for failed_name in &result.failed_nets.clone() {
                            let net_id = match board.nets.iter().find(|n| n.name == *failed_name) {
                                Some(n) => n.id,
                                None => continue,
                            };
                            if !fp_net_ids.contains(&net_id) {
                                continue;
                            }
                            let pads: Vec<(f64, f64)> = board
                                .footprints
                                .iter()
                                .flat_map(|fp2| {
                                    let (fx2, fy2, _) = fp2.position;
                                    fp2.pads
                                        .iter()
                                        .filter_map(move |p| {
                                            if p.net == Some(net_id) {
                                                let (px2, py2, _) = p.position;
                                                Some((fx2 + px2, fy2 + py2))
                                            } else {
                                                None
                                            }
                                        })
                                        .collect::<Vec<_>>()
                                })
                                .collect();
                            if pads.len() < 2 {
                                continue;
                            }
                            rot_grid.unmark_net(net_id);
                            rip_up_net(board, &mut rot_grid, net_id);
                            let tw = effective_trace_width(
                                net_id,
                                failed_name.as_str(),
                                directives,
                                &impedance_widths,
                            );
                            if route_single_net_with_clearance_and_iters(
                                &mut rot_grid,
                                net_id,
                                &pads,
                                tw,
                                board,
                                0.0,
                                50_000,
                                &[],
                                None,
                                &topo_ctx,
                            ) {
                                rot_ok += 1;
                            }
                        }

                        if rot_ok > 0 {
                            eprintln!(
                                "[router] J5 rotation: {} +{:.0}° → {}/{} recovered",
                                ref_name,
                                rot_delta,
                                rot_ok,
                                fp_net_ids.len()
                            );
                            retry_grid = rot_grid;
                            result.routed_nets += rot_ok;
                            result.failed_nets.retain(|name| {
                                let nid = board.nets.iter().find(|n| n.name == *name).map(|n| n.id);
                                nid.map(|id| !board.segments.iter().any(|s| s.net == id))
                                    .unwrap_or(true)
                            });
                            break;
                        } else {
                            // Restore original pad positions and rotation
                            for (pi, &(px, py, _)) in orig_pads.iter().enumerate() {
                                board.footprints[fi].pads[pi].position = (px, py, 0.0);
                            }
                            board.footprints[fi].position = (fx, fy, fr);
                        }
                    }
                    if result.failed_nets.is_empty() {
                        break;
                    }
                }
            }

            let mut retry_ok = 0;
            for failed_name in &result.failed_nets.clone() {
                let net_id = match board.nets.iter().find(|n| n.name == *failed_name) {
                    Some(n) => n.id,
                    None => continue,
                };
                let pads: Vec<(f64, f64)> = board
                    .footprints
                    .iter()
                    .flat_map(|fp| {
                        let (fx, fy, _) = fp.position;
                        fp.pads
                            .iter()
                            .filter_map(move |p| {
                                if p.net == Some(net_id) {
                                    let (px, py, _) = p.position;
                                    Some((fx + px, fy + py))
                                } else {
                                    None
                                }
                            })
                            .collect::<Vec<_>>()
                    })
                    .collect();

                if pads.len() < 2 {
                    continue;
                }
                retry_grid.unmark_net(net_id);
                rip_up_net(board, &mut retry_grid, net_id);
                let trace_width = effective_trace_width(
                    net_id,
                    failed_name.as_str(),
                    directives,
                    &impedance_widths,
                );
                if route_single_net_with_clearance_and_iters(
                    &mut retry_grid,
                    net_id,
                    &pads,
                    trace_width,
                    board,
                    0.0,
                    50_000,
                    &[],
                    None,
                    &topo_ctx,
                ) {
                    retry_ok += 1;
                }
            }

            if retry_ok > 0 {
                eprintln!(
                    "[router] J5 feedback: nudged {} footprints, {}/{} failed nets recovered",
                    nudge_fps.len(),
                    retry_ok,
                    result.failed_nets.len()
                );
                result.routed_nets += retry_ok;
                result.failed_nets.retain(|name| {
                    let net_id = board.nets.iter().find(|n| n.name == *name).map(|n| n.id);
                    net_id
                        .map(|id| !board.segments.iter().any(|s| s.net == id))
                        .unwrap_or(true)
                });
            }
        }
    }

    // 7. Post-routing optimization
    optimize_routes(board);

    // 7.4 Remove stub segments before P&S
    remove_stub_segments(board);

    // 7.5 Push-and-shove: nudge overlapping segments apart using exact geometry
    push_and_shove_repair(board);

    // 8. DRC feedback loop: detect and fix trace-to-trace / trace-to-pad shorts
    // TEMP P1.5-diag: DRC feedback loop disabled — its rip-up/revert pass is
    // the prime suspect for severed segment chains (0.75-1.0mm gaps between
    // grid-aligned segments, TG_X1L: 30 segments in 23 components)
    if std::env::var("ROUTER_NO_FEEDBACK").is_err() {
        drc_feedback_loop(
            board,
            &mut grid,
            &sorted_nets,
            &net_names,
            directives,
            &power_nets,
            &impedance_widths,
            &topo_ctx,
            &history_congestion,
        );
    }

    result.total_segments = board.segments.len();
    result.total_vias = board.vias.len();
    eprintln!(
        "[router] Total routing time: {:.1}s (budget {:.0}s)",
        routing_started.elapsed().as_secs_f64(),
        routing_budget_secs
    );

    result
}

/// Remove all segments and vias belonging to a net, and unmark them from the grid./// Post-routing DRC feedback loop: detect actual trace overlaps (shorts) and re-route.//////
/// Remove stub segments: very short dead-end traces (< 0.3mm) that don't
/// connect to any pad or via at one endpoint. These are routing artifacts that cause DRC shorts.
fn remove_stub_segments(board: &mut Board) {
    let max_stub_len = 0.2; // mm — very short only
    let snap = |v: f64| -> i64 { (v * 100.0).round() as i64 };

    // Build endpoint connectivity: for each snapped endpoint+net, count how many
    // distinct entities (segments, vias, pads) touch it
    let mut endpoint_count: HashMap<((i64, i64), u32), usize> = HashMap::new();
    for seg in &board.segments {
        if seg.net == 0 {
            continue;
        }
        *endpoint_count
            .entry(((snap(seg.start.0), snap(seg.start.1)), seg.net))
            .or_insert(0) += 1;
        *endpoint_count
            .entry(((snap(seg.end.0), snap(seg.end.1)), seg.net))
            .or_insert(0) += 1;
    }
    // Vias and pads add connectivity — they count as a connection
    for via in &board.vias {
        *endpoint_count
            .entry(((snap(via.at.0), snap(via.at.1)), via.net))
            .or_insert(0) += 1;
    }
    for fp in &board.footprints {
        let (fx, fy, _) = fp.position;
        for pad in &fp.pads {
            if let Some(net) = pad.net {
                let (px, py) = fp.pad_rotated_offset(pad);
                *endpoint_count
                    .entry(((snap(fx + px), snap(fy + py)), net))
                    .or_insert(0) += 1;
            }
        }
    }

    let mut to_remove: Vec<usize> = Vec::new();
    for (i, seg) in board.segments.iter().enumerate() {
        if seg.net == 0 {
            continue;
        }
        let len = (seg.end.0 - seg.start.0).hypot(seg.end.1 - seg.start.1);
        if len >= max_stub_len {
            continue;
        }

        let start_key = ((snap(seg.start.0), snap(seg.start.1)), seg.net);
        let end_key = ((snap(seg.end.0), snap(seg.end.1)), seg.net);
        let start_count = endpoint_count.get(&start_key).copied().unwrap_or(0);
        let end_count = endpoint_count.get(&end_key).copied().unwrap_or(0);

        // Truly isolated stub: both endpoints only touched by this segment itself (count=1)
        // count=1 means only this segment's own endpoint, no via/pad/other segment
        if start_count == 1 && end_count == 1 {
            to_remove.push(i);
        }
    }

    if !to_remove.is_empty() {
        let count = to_remove.len();
        to_remove.sort_unstable_by(|a, b| b.cmp(a));
        for idx in to_remove {
            board.segments.remove(idx);
        }
        eprintln!("[router] Stub removal: {} dead-end segments removed", count);
    }
}

/// Single round of push-and-shove: find conflicting segment pairs and nudge
/// the shorter one perpendicular to resolve the overlap. Returns (fixed, total_conflicts).
fn push_and_shove_single_round(board: &mut Board) -> (usize, usize) {
    let cell_size = SPATIAL_CELL_SIZE;

    // Build spatial hash (owned layer strings to avoid borrow conflicts)
    let spatial = build_segment_spatial_hash(&board.segments, cell_size, |seg| {
        seg.width > 0.0 && seg.net != 0
    });

    // Find conflicting pairs
    let mut conflicts: Vec<(usize, usize, f64)> = Vec::new();
    let mut checked: HashSet<(usize, usize)> = HashSet::new();
    for ((_bx, _by, _), indices) in &spatial {
        for i in 0..indices.len() {
            for j in (i + 1)..indices.len() {
                let (a, b) = (indices[i], indices[j]);
                if a == b {
                    continue;
                }
                let (lo, hi) = if a < b { (a, b) } else { (b, a) };
                if !checked.insert((lo, hi)) {
                    continue;
                }
                let sa = &board.segments[lo];
                let sb = &board.segments[hi];
                if sa.net == sb.net || sa.layer != sb.layer {
                    continue;
                }
                let min_dist = sa.width / 2.0 + sb.width / 2.0;
                let dist = crate::drc::segment_to_segment_dist(sa.start, sa.end, sb.start, sb.end);
                if dist < min_dist {
                    conflicts.push((lo, hi, dist));
                }
            }
        }
    }

    let total = conflicts.len();
    if total == 0 {
        return (0, 0);
    }

    conflicts.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));

    let mut fixed = 0usize;

    // Build snap-based endpoint map for connectivity checks (P6)
    let snap_eps = 0.15; // mm — grid resolution
    let snap_key = |x: f64, y: f64| -> (i64, i64) {
        ((x / snap_eps).round() as i64, (y / snap_eps).round() as i64)
    };
    let ep_peers = build_ep_peers(board);

    for (ia, ib, dist) in &conflicts {
        let sa = &board.segments[*ia];
        let sb = &board.segments[*ib];
        if sa.net == sb.net {
            continue;
        }

        let min_dist = sa.width / 2.0 + sb.width / 2.0;

        let len_a = (sa.end.0 - sa.start.0).hypot(sa.end.1 - sa.start.1);
        let len_b = (sb.end.0 - sb.start.0).hypot(sb.end.1 - sb.start.1);
        let (move_idx, other_idx, move_len) = if len_a <= len_b {
            (*ia, *ib, len_a)
        } else {
            (*ib, *ia, len_b)
        };

        // P1.5: skip very short segments as move candidates — patch segments are
        // short by design and must not be re-nudged (avalanche guard)
        if move_len < 0.35 {
            continue;
        }

        let mseg = board.segments[move_idx].clone();
        let dx = mseg.end.0 - mseg.start.0;
        let dy = mseg.end.1 - mseg.start.1;
        let perp1 = (-dy, dx);
        let perp2 = (dy, -dx);

        let perp_len = perp1.0.hypot(perp1.1);
        if perp_len < 0.01 {
            continue;
        }

        // P8: Try both perpendicular directions, pick the one with fewer conflicts
        let directions = [
            (perp1.0 / perp_len, perp1.1 / perp_len),
            (perp2.0 / perp_len, perp2.1 / perp_len),
        ];

        // Adaptive nudge: try increasing push distances
        let mut best_dir: Option<(f64, f64, (f64, f64), (f64, f64), usize, Vec<usize>)> = None;
        'nudge: for &extra in &[0.02, 0.05, 0.10] {
            let push_needed = min_dist - dist + extra;
            for &(nx, ny) in &directions {
                let new_start = (
                    mseg.start.0 + nx * push_needed,
                    mseg.start.1 + ny * push_needed,
                );
                let new_end = (mseg.end.0 + nx * push_needed, mseg.end.1 + ny * push_needed);

                let new_min_dist = mseg.width / 2.0;
                let max_hw = 0.5;
                let (cx0, cy0) = (new_start.0.min(new_end.0), new_start.1.min(new_end.1));
                let (cx1, cy1) = (new_start.0.max(new_end.0), new_start.1.max(new_end.1));
                let mut conflict_count = 0usize;
                let mut blockers: Vec<usize> = Vec::new();
                let mut dir_checked: HashSet<usize> = HashSet::new();
                for c in (((cx0 - max_hw) / cell_size).floor() as i32)
                    ..=((cx1 + max_hw) / cell_size).floor() as i32
                {
                    for r in (((cy0 - max_hw) / cell_size).floor() as i32)
                        ..=((cy1 + max_hw) / cell_size).floor() as i32
                    {
                        if let Some(nearby) = spatial.get(&(c, r, mseg.layer.clone())) {
                            for &si in nearby {
                                if si == move_idx || si == other_idx {
                                    continue;
                                }
                                if !dir_checked.insert(si) {
                                    continue;
                                }
                                let seg = &board.segments[si];
                                if seg.net == mseg.net {
                                    continue;
                                }
                                let d = crate::drc::segment_to_segment_dist(
                                    new_start, new_end, seg.start, seg.end,
                                );
                                if d < new_min_dist + seg.width / 2.0 {
                                    conflict_count += 1;
                                    blockers.push(si);
                                }
                            }
                        }
                    }
                }

                if conflict_count == 0 {
                    best_dir = Some((nx, ny, new_start, new_end, 0, Vec::new()));
                    break 'nudge;
                }
                let prev_count = best_dir.as_ref().map_or(usize::MAX, |d| d.4);
                if conflict_count < prev_count {
                    best_dir = Some((nx, ny, new_start, new_end, conflict_count, blockers));
                }
            }
        } // close nudge loop

        let Some((_nx, _ny, new_start, new_end, conflict_count, blockers)) = best_dir else {
            continue;
        };

        if conflict_count == 0 {
            // P1.5: patch move — displaced endpoints get patch segments back to
            // their original anchors so the chain never severs (the old snap-eps
            // check waved sub-0.15mm nudges through and left micro-gaps).
            if apply_move_with_patch(board, &ep_peers, move_idx, new_start, new_end) {
                fixed += 1;
            }
        } else if blockers.len() <= 3 {
            // T2: Shove Chain — push blocking segments aside, then apply original nudge
            let mut shove_moves: Vec<(usize, (f64, f64), (f64, f64))> = Vec::new();
            let mut all_shoved = true;

            for &bsi in &blockers {
                let bseg = board.segments[bsi].clone();
                let bdx = bseg.end.0 - bseg.start.0;
                let bdy = bseg.end.1 - bseg.start.1;
                let bperp_len = bdx.hypot(bdy);
                if bperp_len < 0.01 {
                    all_shoved = false;
                    break;
                }

                let bp1 = (-bdy / bperp_len, bdx / bperp_len);
                let bp2 = (bdy / bperp_len, -bdx / bperp_len);
                let bpush = bseg.width / 2.0 + mseg.width / 2.0 + 0.03;

                let mut best_bdir: Option<((f64, f64), (f64, f64))> = None;
                let mut best_bcount = usize::MAX;

                for &(bnx, bny) in &[bp1, bp2] {
                    let bns = (bseg.start.0 + bnx * bpush, bseg.start.1 + bny * bpush);
                    let bne = (bseg.end.0 + bnx * bpush, bseg.end.1 + bny * bpush);

                    let (bcx0, bcy0) = (bns.0.min(bne.0), bns.1.min(bne.1));
                    let (bcx1, bcy1) = (bns.0.max(bne.0), bns.1.max(bne.1));
                    let mut bcount = 0usize;
                    let mut bchecked: HashSet<usize> = HashSet::new();
                    for bc in (((bcx0 - 0.5) / cell_size).floor() as i32)
                        ..=((bcx1 + 0.5) / cell_size).floor() as i32
                    {
                        for br in (((bcy0 - 0.5) / cell_size).floor() as i32)
                            ..=((bcy1 + 0.5) / cell_size).floor() as i32
                        {
                            if let Some(nearby) = spatial.get(&(bc, br, bseg.layer.clone())) {
                                for &si in nearby {
                                    if si == bsi || si == move_idx || blockers.contains(&si) {
                                        continue;
                                    }
                                    if !bchecked.insert(si) {
                                        continue;
                                    }
                                    let seg = &board.segments[si];
                                    if seg.net == bseg.net {
                                        continue;
                                    }
                                    let d = crate::drc::segment_to_segment_dist(
                                        bns, bne, seg.start, seg.end,
                                    );
                                    if d < bseg.width / 2.0 + seg.width / 2.0 {
                                        bcount += 1;
                                    }
                                }
                            }
                        }
                    }

                    if bcount == 0 {
                        best_bdir = Some((bns, bne));
                        best_bcount = 0;
                        break;
                    }
                    if bcount < best_bcount {
                        best_bdir = Some((bns, bne));
                        best_bcount = bcount;
                    }
                }

                if best_bcount != 0 {
                    all_shoved = false;
                    break;
                }

                // Connectivity check for blocker
                let bnet = bseg.net;
                let bs_ok = ep_peers
                    .get(&(snap_key(bseg.start.0, bseg.start.1), bnet))
                    .is_some_and(|peers| peers.iter().any(|&p| p != bsi));
                let be_ok = ep_peers
                    .get(&(snap_key(bseg.end.0, bseg.end.1), bnet))
                    .is_some_and(|peers| peers.iter().any(|&p| p != bsi));
                if let Some((bns, bne)) = &best_bdir {
                    let bns_ok = ep_peers
                        .get(&(snap_key(bns.0, bns.1), bnet))
                        .is_none_or(|peers| peers.iter().any(|&p| p != bsi));
                    let bne_ok = ep_peers
                        .get(&(snap_key(bne.0, bne.1), bnet))
                        .is_none_or(|peers| peers.iter().any(|&p| p != bsi));
                    if (bs_ok && !bns_ok) || (be_ok && !bne_ok) {
                        all_shoved = false;
                        break;
                    }
                    shove_moves.push((bsi, *bns, *bne));
                } else {
                    all_shoved = false;
                    break;
                }
            }

            if all_shoved && !shove_moves.is_empty() {
                // Save original blocker positions for rollback
                let orig_blockers: Vec<(usize, (f64, f64), (f64, f64))> = shove_moves
                    .iter()
                    .map(|&(bsi, _, _)| (bsi, board.segments[bsi].start, board.segments[bsi].end))
                    .collect();

                // Apply blocker shoves tentatively (P1.5: patch moves —
                // shoved blockers keep their chains via patch segments)
                for &(bsi, bs, be) in &shove_moves {
                    apply_move_with_patch(board, &ep_peers, bsi, bs, be);
                }

                // Re-check original nudge position against updated board
                let (rcx0, rcy0) = (new_start.0.min(new_end.0), new_start.1.min(new_end.1));
                let (rcx1, rcy1) = (new_start.0.max(new_end.0), new_start.1.max(new_end.1));
                let mut recheck_conflicts = 0usize;
                let mut rechecked: HashSet<usize> = HashSet::new();
                for rc in (((rcx0 - 0.5) / cell_size).floor() as i32)
                    ..=((rcx1 + 0.5) / cell_size).floor() as i32
                {
                    for rr in (((rcy0 - 0.5) / cell_size).floor() as i32)
                        ..=((rcy1 + 0.5) / cell_size).floor() as i32
                    {
                        if let Some(nearby) = spatial.get(&(rc, rr, mseg.layer.clone())) {
                            for &si in nearby {
                                if si == move_idx || si == other_idx {
                                    continue;
                                }
                                if !rechecked.insert(si) {
                                    continue;
                                }
                                let seg = &board.segments[si];
                                if seg.net == mseg.net {
                                    continue;
                                }
                                let d = crate::drc::segment_to_segment_dist(
                                    new_start, new_end, seg.start, seg.end,
                                );
                                if d < mseg.width / 2.0 + seg.width / 2.0 {
                                    recheck_conflicts += 1;
                                }
                            }
                        }
                    }
                }

                if recheck_conflicts == 0 {
                    // Connectivity check for original nudge
                    let net = mseg.net;
                    let _start_ok = ep_peers
                        .get(&(snap_key(mseg.start.0, mseg.start.1), net))
                        .is_some_and(|peers| peers.iter().any(|&p| p != move_idx));
                    let _end_ok = ep_peers
                        .get(&(snap_key(mseg.end.0, mseg.end.1), net))
                        .is_some_and(|peers| peers.iter().any(|&p| p != move_idx));
                    let _new_start_ok = ep_peers
                        .get(&(snap_key(new_start.0, new_start.1), net))
                        .is_none_or(|peers| peers.iter().any(|&p| p != move_idx));
                    let _new_end_ok = ep_peers
                        .get(&(snap_key(new_end.0, new_end.1), net))
                        .is_none_or(|peers| peers.iter().any(|&p| p != move_idx));
                    // P1.5: patch move — patches guarantee chain continuity,
                    // no rollback needed for connectivity
                    if apply_move_with_patch(board, &ep_peers, move_idx, new_start, new_end) {
                        fixed += 1 + shove_moves.len();
                    } else {
                        // a shove-victim endpoint would need >PATCH_MAX travel
                        for (bsi, os, oe) in &orig_blockers {
                            board.segments[*bsi].start = *os;
                            board.segments[*bsi].end = *oe;
                        }
                    }
                } else {
                    // Re-check failed — rollback blocker shoves
                    for (bsi, os, oe) in &orig_blockers {
                        board.segments[*bsi].start = *os;
                        board.segments[*bsi].end = *oe;
                    }
                }
            }
        }
    }

    (fixed, total)
}

/// Push-and-shove: nudge overlapping trace segments apart using exact geometry.
/// Runs 3 rounds before DRC feedback to fix near-miss shorts cheaply.
fn push_and_shove_repair(board: &mut Board) {
    for round in 0..8 {
        let (fixed, total) = push_and_shove_single_round(board);
        eprintln!(
            "[router] Push-and-shove round {}: {}/{} fixed",
            round, fixed, total
        );
        if fixed == 0 || total == 0 {
            break;
        }
        // Stop if fixing less than 5% of remaining conflicts
        if fixed * 20 < total {
            break;
        }
    }
}
/// Post-routing DRC feedback loop: detect actual trace overlaps (shorts) and re-route.
/// Only targets traces that physically overlap (distance < sum of half widths),
/// not general clearance violations which can't be fixed at 0.25mm grid resolution.
fn drc_feedback_loop(
    board: &mut Board,
    grid: &mut RoutingGrid,
    sorted_nets: &[(u32, Vec<(f64, f64)>)],
    net_names: &HashMap<u32, String>,
    directives: &LayoutDirectives,
    power_nets: &HashSet<u32>,
    impedance_widths: &HashMap<u32, f64>,
    topo_ctx: &TopologyContext,
    history_congestion: &[f64],
) {
    const MAX_DRC_ROUNDS: usize = 8;
    let mut prev_total = usize::MAX;
    // Track nets that couldn't be fixed in previous rounds — use higher clearance
    let mut stubborn_nets: HashSet<u32> = HashSet::new();
    // DRC bottom-noise reduction: track how many consecutive rounds each net
    // has been violating. Only rip-up nets that violate ≥2 consecutive rounds
    // (persistent offenders). First-time violators get Phase 0 nudging only —
    // this avoids the "fix then re-break" cycle where DRC feedback rip-up
    // destroys Phase 0's repairs.
    let mut violation_counter: HashMap<u32, u32> = HashMap::new();
    // Snapshot for rollback on divergence
    let mut prev_snapshot: Option<(Vec<Segment>, Vec<Via>)> = None;

    for round in 0..MAX_DRC_ROUNDS {
        let t_detect_start = std::time::Instant::now();
        let mut violating_nets: HashSet<u32> = HashSet::new();
        let mut tt_shorts = 0usize; // trace-to-trace
        let mut tp_shorts = 0usize; // trace-to-pad

        // Check trace-to-trace: spatial hash acceleration
        // Instead of O(n²) pairwise, use grid-based spatial index → O(n) average
        let cell_size = SPATIAL_CELL_SIZE;
        let mut spatial: HashMap<(i32, i32, &str), Vec<usize>> = HashMap::new();
        let sig_segs: Vec<&Segment> = board
            .segments
            .iter()
            .filter(|s| s.width > 0.0 && s.layer.ends_with(".Cu"))
            .collect();

        for (idx, seg) in sig_segs.iter().enumerate() {
            let (x0, y0) = (seg.start.0.min(seg.end.0), seg.start.1.min(seg.end.1));
            let (x1, y1) = (seg.start.0.max(seg.end.0), seg.start.1.max(seg.end.1));
            let c0 = (x0 / cell_size).floor() as i32;
            let r0 = (y0 / cell_size).floor() as i32;
            let c1 = (x1 / cell_size).floor() as i32;
            let r1 = (y1 / cell_size).floor() as i32;
            for c in c0..=c1 {
                for r in r0..=r1 {
                    spatial
                        .entry((c, r, seg.layer.as_str()))
                        .or_default()
                        .push(idx);
                }
            }
        }

        let mut checked_pairs: HashSet<(usize, usize)> = HashSet::new();
        let mut conflict_pairs: Vec<(usize, usize, f64)> = Vec::new();
        let mut min_tt_dist = f64::MAX;
        for indices in spatial.values() {
            for i in 0..indices.len() {
                for j in (i + 1)..indices.len() {
                    let (a, b) = (indices[i], indices[j]);
                    let (lo, hi) = if a < b { (a, b) } else { (b, a) };
                    if !checked_pairs.insert((lo, hi)) {
                        continue;
                    }
                    let s1 = sig_segs[lo];
                    let s2 = sig_segs[hi];
                    if s1.net == s2.net {
                        continue;
                    }
                    let overlap_dist = s1.width / 2.0 + s2.width / 2.0;
                    let dist =
                        crate::drc::segment_to_segment_dist(s1.start, s1.end, s2.start, s2.end);
                    if dist < overlap_dist {
                        violating_nets.insert(s1.net);
                        violating_nets.insert(s2.net);
                        tt_shorts += 1;
                        if dist < min_tt_dist {
                            min_tt_dist = dist;
                        }
                        conflict_pairs.push((lo, hi, dist));
                    }
                }
            }
        }

        // Check trace-to-pad overlaps: spatial hash lookup
        for fp in &board.footprints {
            let (fx, fy, _) = fp.position;
            for pad in &fp.pads {
                let pad_net = pad.net.unwrap_or(0);
                if pad_net == 0 {
                    continue;
                }
                let (px, py) = fp.pad_rotated_offset(pad);
                let abs_px = fx + px;
                let abs_py = fy + py;
                let pad_r = pad.size.0.max(pad.size.1) / 2.0;
                let check_r = pad_r + 1.0; // max trace half-width
                let c0 = ((abs_px - check_r) / cell_size).floor() as i32;
                let c1 = ((abs_px + check_r) / cell_size).floor() as i32;
                let r0 = ((abs_py - check_r) / cell_size).floor() as i32;
                let r1 = ((abs_py + check_r) / cell_size).floor() as i32;
                let mut checked: HashSet<usize> = HashSet::new();
                for c in c0..=c1 {
                    for r in r0..=r1 {
                        for layer in &pad.layers {
                            if let Some(indices) = spatial.get(&(c, r, layer.as_str())) {
                                for &idx in indices {
                                    if !checked.insert(idx) {
                                        continue;
                                    }
                                    let seg = sig_segs[idx];
                                    if seg.net == pad_net {
                                        continue;
                                    }
                                    let dist = crate::drc::point_to_segment_dist(
                                        (abs_px, abs_py),
                                        seg.start,
                                        seg.end,
                                    );
                                    if dist < seg.width / 2.0 + pad_r {
                                        violating_nets.insert(seg.net);
                                        violating_nets.insert(pad_net);
                                        tp_shorts += 1;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        violating_nets.retain(|&n| !power_nets.contains(&n) && n != 0);
        let t_detect = t_detect_start.elapsed().as_millis();

        if violating_nets.is_empty() {
            if round > 0 {
                eprintln!("[router] DRC feedback: clean after {} rounds", round);
            }
            return;
        }

        eprintln!("[router] DRC feedback round {}: {} nets (tt={}, tp={}, min_tt_dist={:.3}mm) detect={:.0}ms", round + 1, violating_nets.len(), tt_shorts, tp_shorts,
            if min_tt_dist == f64::MAX { 0.0 } else { min_tt_dist }, t_detect);

        let total_shorts = tt_shorts + tp_shorts;
        // Diverging: allow up to 2 rounds with <1% improvement before quitting
        let improvement = if prev_total > 0 && prev_total < usize::MAX {
            prev_total.saturating_sub(total_shorts) as f64 / prev_total as f64
        } else {
            1.0
        };
        if total_shorts >= prev_total || (round > 0 && improvement < 0.01) {
            let reason = if total_shorts >= prev_total {
                "worsening"
            } else {
                "stagnant"
            };
            eprintln!("[router] DRC feedback: diverging ({} {}, improvement={:.1}%), restoring previous state", reason, total_shorts, improvement * 100.0);
            if let Some((prev_segs, prev_vias)) = prev_snapshot.take() {
                board.segments = prev_segs;
                board.vias = prev_vias;
                let lc = grid.layer_config.clone();
                let gr = grid.grid_res;
                let sig_layers: Vec<usize> = lc.signal_layer_indices();
                *grid = RoutingGrid::new(board, lc, gr);
                grid.mark_component_bodies(board);
                grid.mark_pads(board);
                for seg in &board.segments {
                    if seg.net != 0 && !power_nets.contains(&seg.net) {
                        mark_segment_on_grid(grid, seg, seg.net, DEFAULT_CLEARANCE);
                    }
                }
                for via in &board.vias {
                    let (gx, gy) = grid.world_to_grid(via.at.0, via.at.1);
                    if gx < grid.cols && gy < grid.rows {
                        for &layer in &sig_layers {
                            grid.set(layer, gx, gy, Cell::Via(via.net));
                        }
                    }
                }
            }
            return;
        }
        prev_total = total_shorts;
        // Save snapshot before attempting fixes this round
        prev_snapshot = Some((board.segments.clone(), board.vias.clone()));

        // Phase 0: Segment-level micro-fix — nudge conflicting segments apart
        // Uses the same dual-direction P&S logic but applied directly to DRC conflict pairs.
        // This fixes many near-misses without needing to rip-up entire nets.
        if !conflict_pairs.is_empty() {
            let snap_eps = 0.15;
            let snap_key = |x: f64, y: f64| -> (i64, i64) {
                ((x / snap_eps).round() as i64, (y / snap_eps).round() as i64)
            };
            let mut ep_peers = build_ep_peers(board);

            // Spatial hash for O(k) conflict check (owned layer strings)
            let nudge_bin = SPATIAL_CELL_SIZE;
            let seg_spatial = build_segment_spatial_hash(&board.segments, nudge_bin, |seg| {
                seg.net != 0 && seg.width > 0.0 && seg.layer.ends_with(".Cu")
            });

            let mut pairs_sorted = conflict_pairs.clone();
            // P0-10: tie-break equal distances by pair indices (HashMap cell
            // iteration order randomized which equal-dist pair nudged first)
            pairs_sorted.sort_by(|a, b| {
                a.2.partial_cmp(&b.2)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| (a.0, a.1).cmp(&(b.0, b.1)))
            });
            let mut seg_fixed = 0usize;
            let mut nudged: HashSet<usize> = HashSet::new();
            for (lo, hi, dist) in &pairs_sorted {
                for &try_idx in &[*lo, *hi] {
                    if nudged.contains(&try_idx) {
                        continue;
                    }
                    // Clone segment data to avoid borrow conflicts when writing back
                    let mseg = board.segments[try_idx].clone();
                    let other_idx = if try_idx == *lo { *hi } else { *lo };
                    let oseg = &board.segments[other_idx];
                    if mseg.net == oseg.net {
                        continue;
                    }

                    let min_dist = mseg.width / 2.0 + oseg.width / 2.0;

                    let move_len = (mseg.end.0 - mseg.start.0).hypot(mseg.end.1 - mseg.start.1);
                    // P1.5: skip very short segments as move candidates — patch segments are
                    // short by design and must not be re-nudged (avalanche guard)
                    if move_len < 0.35 {
                        continue;
                    }

                    let dx = mseg.end.0 - mseg.start.0;
                    let dy = mseg.end.1 - mseg.start.1;
                    let perp1 = (-dy, dx);
                    let perp2 = (dy, -dx);
                    let perp_len = perp1.0.hypot(perp1.1);
                    if perp_len < 0.01 {
                        continue;
                    }

                    let directions = [
                        (perp1.0 / perp_len, perp1.1 / perp_len),
                        (perp2.0 / perp_len, perp2.1 / perp_len),
                    ];

                    // Adaptive nudge: try increasing push distances
                    let mut applied = false;
                    for &extra in &[0.02, 0.05, 0.10, 0.15, 0.20, 0.25, 0.30] {
                        let push_needed = min_dist - dist + extra;
                        for &(nx, ny) in &directions {
                            let new_start = (
                                mseg.start.0 + nx * push_needed,
                                mseg.start.1 + ny * push_needed,
                            );
                            let new_end =
                                (mseg.end.0 + nx * push_needed, mseg.end.1 + ny * push_needed);

                            // Spatial hash conflict check — O(k) instead of O(n)
                            let new_min_dist = mseg.width / 2.0;
                            let max_hw = 0.5; // max expected half-width
                            let (cx0, cy0) =
                                (new_start.0.min(new_end.0), new_start.1.min(new_end.1));
                            let (cx1, cy1) =
                                (new_start.0.max(new_end.0), new_start.1.max(new_end.1));
                            let mut has_conflict = false;
                            let mut checked: HashSet<usize> = HashSet::new();
                            for c in (((cx0 - max_hw) / nudge_bin).floor() as i32)
                                ..=((cx1 + max_hw) / nudge_bin).floor() as i32
                            {
                                for r in (((cy0 - max_hw) / nudge_bin).floor() as i32)
                                    ..=((cy1 + max_hw) / nudge_bin).floor() as i32
                                {
                                    if let Some(nearby) =
                                        seg_spatial.get(&(c, r, mseg.layer.clone()))
                                    {
                                        for &si in nearby {
                                            if si == try_idx || si == other_idx {
                                                continue;
                                            }
                                            if !checked.insert(si) {
                                                continue;
                                            }
                                            let seg = &board.segments[si];
                                            if seg.net == mseg.net {
                                                continue;
                                            }
                                            let d = crate::drc::segment_to_segment_dist(
                                                new_start, new_end, seg.start, seg.end,
                                            );
                                            if d < new_min_dist + seg.width / 2.0 {
                                                has_conflict = true;
                                                break;
                                            }
                                        }
                                        if has_conflict {
                                            break;
                                        }
                                    }
                                }
                                if has_conflict {
                                    break;
                                }
                            }
                            if has_conflict {
                                continue;
                            }

                            // Connectivity check
                            let net = mseg.net;
                            let start_ok = ep_peers
                                .get(&(snap_key(mseg.start.0, mseg.start.1), net))
                                .is_some_and(|p| p.iter().any(|&x| x != try_idx));
                            let end_ok = ep_peers
                                .get(&(snap_key(mseg.end.0, mseg.end.1), net))
                                .is_some_and(|p| p.iter().any(|&x| x != try_idx));
                            // P1-fix: HARD check — an endpoint that currently has a
                            // same-net neighbor (pad/via/segment) must land on a
                            // neighbor after the move, or the chain is severed.
                            // map_or(true) here let nudges silently disconnect
                            // segments (TG_X1L: 33 segments → 21 components).
                            let new_start_ok = ep_peers
                                .get(&(snap_key(new_start.0, new_start.1), net))
                                .is_some_and(|p| p.iter().any(|&x| x != try_idx));
                            let new_end_ok = ep_peers
                                .get(&(snap_key(new_end.0, new_end.1), net))
                                .is_some_and(|p| p.iter().any(|&x| x != try_idx));
                            if start_ok && !new_start_ok {
                                continue;
                            }
                            if end_ok && !new_end_ok {
                                continue;
                            }

                            // Update ep_peers for the moved segment
                            let old_sk_s = snap_key(mseg.start.0, mseg.start.1);
                            let old_sk_e = snap_key(mseg.end.0, mseg.end.1);
                            let new_sk_s = snap_key(new_start.0, new_start.1);
                            let new_sk_e = snap_key(new_end.0, new_end.1);
                            if let Some(peers) = ep_peers.get_mut(&(old_sk_s, net)) {
                                peers.retain(|&x| x != try_idx);
                            }
                            if let Some(peers) = ep_peers.get_mut(&(old_sk_e, net)) {
                                peers.retain(|&x| x != try_idx);
                            }
                            ep_peers.entry((new_sk_s, net)).or_default().push(try_idx);
                            ep_peers.entry((new_sk_e, net)).or_default().push(try_idx);

                            // P1.5: patch move keeps the chain whole
                            if apply_move_with_patch(board, &ep_peers, try_idx, new_start, new_end)
                            {
                                seg_fixed += 1;
                                applied = true;
                                nudged.insert(try_idx);
                                break;
                            }
                        }
                        if applied {
                            break;
                        }
                    } // close extra loop
                    if applied {
                        break;
                    }

                    // Dog-leg nudging: if whole-segment push failed, try pushing
                    // only the START endpoint to create a small dog-leg.
                    for &extra in &[0.02, 0.05, 0.10, 0.15, 0.20, 0.25, 0.30] {
                        let push_needed = min_dist - dist + extra;
                        for &(nx, ny) in &directions {
                            let new_start = (
                                mseg.start.0 + nx * push_needed,
                                mseg.start.1 + ny * push_needed,
                            );
                            let new_end = mseg.end;

                            let new_min_dist = mseg.width / 2.0;
                            let max_hw = 0.5;
                            let (cx0, cy0) =
                                (new_start.0.min(new_end.0), new_start.1.min(new_end.1));
                            let (cx1, cy1) =
                                (new_start.0.max(new_end.0), new_start.1.max(new_end.1));
                            let mut has_conflict = false;
                            let mut checked: HashSet<usize> = HashSet::new();
                            for c in (((cx0 - max_hw) / nudge_bin).floor() as i32)
                                ..=((cx1 + max_hw) / nudge_bin).floor() as i32
                            {
                                for r in (((cy0 - max_hw) / nudge_bin).floor() as i32)
                                    ..=((cy1 + max_hw) / nudge_bin).floor() as i32
                                {
                                    if let Some(nearby) =
                                        seg_spatial.get(&(c, r, mseg.layer.clone()))
                                    {
                                        for &si in nearby {
                                            if si == try_idx || si == other_idx {
                                                continue;
                                            }
                                            if !checked.insert(si) {
                                                continue;
                                            }
                                            let seg = &board.segments[si];
                                            if seg.net == mseg.net {
                                                continue;
                                            }
                                            let d = crate::drc::segment_to_segment_dist(
                                                new_start, new_end, seg.start, seg.end,
                                            );
                                            if d < new_min_dist + seg.width / 2.0 {
                                                has_conflict = true;
                                                break;
                                            }
                                        }
                                        if has_conflict {
                                            break;
                                        }
                                    }
                                }
                                if has_conflict {
                                    break;
                                }
                            }
                            if has_conflict {
                                continue;
                            }

                            let net = mseg.net;
                            let start_ok = ep_peers
                                .get(&(snap_key(mseg.start.0, mseg.start.1), net))
                                .is_some_and(|p| p.iter().any(|&x| x != try_idx));
                            // P1-fix: hard check, same as whole-segment push —
                            // a connected endpoint must stay connected.
                            let new_start_ok = ep_peers
                                .get(&(snap_key(new_start.0, new_start.1), net))
                                .is_some_and(|p| p.iter().any(|&x| x != try_idx));
                            if start_ok && !new_start_ok {
                                continue;
                            }

                            let old_sk_s = snap_key(mseg.start.0, mseg.start.1);
                            let new_sk_s = snap_key(new_start.0, new_start.1);
                            if let Some(peers) = ep_peers.get_mut(&(old_sk_s, net)) {
                                peers.retain(|&x| x != try_idx);
                            }
                            ep_peers.entry((new_sk_s, net)).or_default().push(try_idx);
                            // P1.5: patch move — the displaced endpoint gets a
                            // patch segment back to its original anchor
                            if apply_move_with_patch(board, &ep_peers, try_idx, new_start, mseg.end)
                            {
                                seg_fixed += 1;
                                applied = true;
                                nudged.insert(try_idx);
                                break;
                            }
                        }
                        if applied {
                            break;
                        }
                    }
                }
            }
            if seg_fixed > 0 {
                eprintln!(
                    "[router] DRC Phase 0 seg-level: {} shorts fixed by nudging",
                    seg_fixed
                );
                // If Phase 0 fixed all or most shorts, skip rip-up/re-route
                // which can introduce new conflicts by disturbing the layout.
                let estimated_remaining = tt_shorts.saturating_sub(seg_fixed);
                if estimated_remaining == 0 {
                    eprintln!("[router] DRC Phase 0 fixed all shorts, skipping rip-up/re-route");
                    // Re-evaluate for convergence check
                    let total_shorts = estimated_remaining;
                    if total_shorts >= prev_total {
                        break;
                    }
                    prev_total = total_shorts;
                    continue;
                }
            }
        }

        // DRC bottom-noise reduction: only rip-up nets that have been violating
        // for ≥2 consecutive rounds. First-time violators already got Phase 0
        // nudging — ripping them up immediately would destroy those repairs and
        // re-introduce the same conflicts next round (the "fix-break" cycle).
        // Persistent offenders (counter ≥2) need full rip-up + re-route.
        let mut net_list: Vec<u32> = violating_nets.iter().copied().collect();
        net_list.sort_unstable(); // P0-10: HashSet iteration is randomized per process
        for &net_id in &net_list {
            *violation_counter.entry(net_id).or_insert(0) += 1;
        }
        violation_counter.retain(|k, _| violating_nets.contains(k));
        // Only rip-up persistent offenders (≥2 consecutive rounds)
        net_list.retain(|net_id| violation_counter.get(net_id).copied().unwrap_or(0) >= 2);
        // P2: Sort by HPWL ascending — short nets first for rip-up (they're more flexible)
        {
            let net_hpwl = |nid: u32| -> f64 {
                sorted_nets
                    .iter()
                    .find(|(id, _)| *id == nid)
                    .map(|(_, pads)| {
                        if pads.len() < 2 {
                            return 0.0;
                        }
                        let (x0, y0, x1, y1) = net_bbox(pads);
                        (x1 - x0) + (y1 - y0)
                    })
                    .unwrap_or(0.0)
            };
            net_list.sort_by(|a, b| {
                let ha = net_hpwl(*a);
                let hb = net_hpwl(*b);
                ha.partial_cmp(&hb)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| a.cmp(b)) // P0-10: total order on ties
            });
        }

        // Escalating clearance for stubborn nets (P1-8: honor CLI override)
        let base_clearance = directives.clearance_override.unwrap_or(DEFAULT_CLEARANCE);
        let mut fixed = 0;
        let mut reverted = 0;
        let mut failed = 0;
        let mut new_stubborn = HashSet::new();

        let t_fix_start = std::time::Instant::now();

        // Phase 1: Serial rip-up all violating nets, save originals
        struct OriginalRoute {
            net_id: u32,
            segs: Vec<Segment>,
            vias: Vec<Via>,
            orig_conflicts: usize,
            trace_width: f64,
            clearance: f64,
            pads: Vec<(f64, f64)>,
            best_layer: Option<usize>,
        }
        let mut originals: Vec<OriginalRoute> = Vec::with_capacity(net_list.len());
        for &net_id in &net_list {
            let net_name = match net_names.get(&net_id) {
                Some(n) => n.as_str(),
                None => continue,
            };
            let pads: Vec<(f64, f64)> = board
                .footprints
                .iter()
                .flat_map(|fp| {
                    let (fx, fy, _) = fp.position;
                    fp.pads
                        .iter()
                        .filter_map(move |p| {
                            if p.net == Some(net_id) {
                                let (px, py, _) = p.position;
                                Some((fx + px, fy + py))
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<_>>()
                })
                .collect();
            if pads.len() < 2 {
                continue;
            }

            let net_clearance = if stubborn_nets.contains(&net_id) {
                base_clearance + 0.1 * (round as f64).min(3.0)
            } else {
                base_clearance
            };
            let trace_width = effective_trace_width(net_id, net_name, directives, impedance_widths);

            let orig_segs: Vec<Segment> = board
                .segments
                .iter()
                .filter(|s| s.net == net_id)
                .cloned()
                .collect();
            let orig_vias: Vec<Via> = board
                .vias
                .iter()
                .filter(|v| v.net == net_id)
                .cloned()
                .collect();
            let orig_conflicts = count_segment_conflicts(&orig_segs, board, net_id);

            // For multi-layer boards: find which signal layer has fewest conflicts
            // so re-route can prefer that layer
            let sig_layers = grid.layer_config.signal_layer_indices();
            let best_layer = if sig_layers.len() >= 3 {
                let mut layer_conflicts: HashMap<usize, usize> = HashMap::new();
                for seg in &orig_segs {
                    if let Some(li) = grid.layer_config.layer_index(&seg.layer) {
                        if sig_layers.contains(&li) {
                            let overlap_count = board
                                .segments
                                .iter()
                                .filter(|s| {
                                    s.net != net_id && s.layer == seg.layer && s.width > 0.0
                                })
                                .filter(|s| {
                                    let d = crate::drc::segment_to_segment_dist(
                                        seg.start, seg.end, s.start, s.end,
                                    );
                                    d < seg.width / 2.0 + s.width / 2.0
                                })
                                .count();
                            *layer_conflicts.entry(li).or_insert(0) += overlap_count;
                        }
                    }
                }
                // Pick the layer with fewest conflicts (least congested)
                layer_conflicts
                    .iter()
                    .min_by_key(|(_, &c)| c)
                    .map(|(&l, _)| l)
            } else {
                None
            };

            grid.unmark_net(net_id);
            rip_up_net(board, grid, net_id);

            originals.push(OriginalRoute {
                net_id,
                segs: orig_segs,
                vias: orig_vias,
                orig_conflicts,
                trace_width,
                clearance: net_clearance,
                pads,
                best_layer,
            });
        }

        // Phase 2: Parallel pathfind
        let grid_ref: &RoutingGrid = grid;
        let topo_ctx_ref = &topo_ctx; // L6: shared read-only topology context
        let path_results: Vec<(usize, Option<NetPathResult>)> = originals
            .iter()
            .enumerate()
            .map(|(i, orig)| {
                let sig_layer_count = grid_ref.layer_config.signal_layer_indices().len();
                let astar_total = sig_layer_count * grid_ref.cols * grid_ref.rows;
                let mut cache = AStarCache {
                    g_score: vec![f64::MAX; astar_total],
                    g_version: vec![0; astar_total],
                    came_from: vec![u32::MAX; astar_total],
                    cf_version: vec![0; astar_total],
                    open: BinaryHeap::with_capacity(4096),
                    version: 0,
                };
                let result = pathfind_single_net(
                    grid_ref,
                    orig.net_id,
                    &orig.pads,
                    orig.trace_width,
                    board,
                    orig.clearance,
                    MAX_ITERATIONS,
                    history_congestion,
                    orig.best_layer,
                    &mut cache,
                    topo_ctx_ref,
                );
                (
                    i,
                    if result.all_connected {
                        Some(result)
                    } else {
                        None
                    },
                )
            })
            .collect();

        // Phase 3: Serial commit + conflict check
        for (i, path_result) in path_results {
            let orig = &originals[i];
            match path_result {
                Some(result) => {
                    commit_net_path(grid, board, &result, orig.clearance);
                    let new_segs: Vec<Segment> = board
                        .segments
                        .iter()
                        .filter(|s| s.net == orig.net_id)
                        .cloned()
                        .collect();
                    let new_conflicts = count_segment_conflicts(&new_segs, board, orig.net_id);
                    // Allow slight regression: accept if new conflicts <= orig + 2
                    // (strict "not worse" causes ~50% reverts which stagnates DRC)
                    let tolerance = 2;
                    if new_conflicts > orig.orig_conflicts + tolerance {
                        // Not better — restore original
                        grid.unmark_net(orig.net_id);
                        rip_up_net(board, grid, orig.net_id);
                        let clearance = orig.trace_width * 0.5;
                        for seg in &orig.segs {
                            mark_segment_on_grid(grid, seg, orig.net_id, clearance);
                        }
                        for via in &orig.vias {
                            let (gx, gy) = grid.world_to_grid(via.at.0, via.at.1);
                            for layer in 0..grid.num_layers {
                                if gx < grid.cols && gy < grid.rows {
                                    grid.set(layer, gx, gy, Cell::Via(orig.net_id));
                                }
                            }
                        }
                        board.segments.extend(orig.segs.clone());
                        board.vias.extend(orig.vias.clone());
                        reverted += 1;
                        new_stubborn.insert(orig.net_id);
                    } else {
                        fixed += 1;
                    }
                }
                None => {
                    // Restore original
                    let clearance = orig.trace_width * 0.5;
                    for seg in &orig.segs {
                        mark_segment_on_grid(grid, seg, orig.net_id, clearance);
                    }
                    for via in &orig.vias {
                        let (gx, gy) = grid.world_to_grid(via.at.0, via.at.1);
                        for layer in 0..grid.num_layers {
                            if gx < grid.cols && gy < grid.rows {
                                grid.set(layer, gx, gy, Cell::Via(orig.net_id));
                            }
                        }
                    }
                    board.segments.extend(orig.segs.clone());
                    board.vias.extend(orig.vias.clone());
                    failed += 1;
                    new_stubborn.insert(orig.net_id);
                }
            }
        }

        let t_fix = t_fix_start.elapsed().as_millis();
        eprintln!("[router] DRC feedback: {}/{} fixed, {} reverted, {} failed (base_clearance={:.2}mm, round={}) fix={:.0}ms",
            fixed, net_list.len(), reverted, failed, base_clearance, round + 1, t_fix);
        stubborn_nets = new_stubborn;

        // Run 1 round of push-and-shove between DRC feedback rounds
        // to fix near-misses created by the rip-up/re-route above
        let (ps_fixed, ps_total) = push_and_shove_single_round(board);
        if ps_fixed > 0 {
            eprintln!(
                "[router] DRC inter-round P&S: {}/{} fixed",
                ps_fixed, ps_total
            );
        }
    }
}

/// Build spatial hash from board segments for O(k) conflict detection.
/// Groups segment indices by (cell_x, cell_y, layer) bins.
fn build_segment_spatial_hash(
    segments: &[Segment],
    cell_size: f64,
    filter: fn(&Segment) -> bool,
) -> HashMap<(i32, i32, String), Vec<usize>> {
    let mut spatial: HashMap<(i32, i32, String), Vec<usize>> = HashMap::new();
    for (i, seg) in segments.iter().enumerate() {
        if !filter(seg) {
            continue;
        }
        let (x0, y0) = (seg.start.0.min(seg.end.0), seg.start.1.min(seg.end.1));
        let (x1, y1) = (seg.start.0.max(seg.end.0), seg.start.1.max(seg.end.1));
        let c0 = (x0 / cell_size).floor() as i32;
        let r0 = (y0 / cell_size).floor() as i32;
        let c1 = (x1 / cell_size).floor() as i32;
        let r1 = (y1 / cell_size).floor() as i32;
        for c in c0..=c1 {
            for r in r0..=r1 {
                spatial
                    .entry((c, r, seg.layer.clone()))
                    .or_default()
                    .push(i);
            }
        }
    }
    spatial
}

/// P1.5: move a segment to new endpoints, patching the chain continuity.
///
/// KiCad connectivity requires segment endpoints to EXACTLY meet pads, vias or
/// neighbouring segment endpoints. Any nudge — even 0.02mm — that moves a
/// connected endpoint away from its neighbour severs the chain (measured:
/// TG_X1L fell apart into 21+ components after a few P&S rounds; a nudged
/// segment's y went 20.5 → 20.556545). The old snap-eps connectivity check
/// (0.15mm) waved every small move through.
///
/// The fix is what a real push-and-shove does: after moving, add a tiny patch
/// segment from each displaced endpoint back to its ORIGINAL position, so the
/// chain stays whole at the old anchor while the body clears the obstacle.
/// Returns false when the move is rejected: a connected endpoint would have to
/// travel > PATCH_MAX (0.3mm) — the patch would be a huge detour — so the
/// caller should try another candidate instead of severing the chain.
type EpPeers = HashMap<((i64, i64), u32), Vec<usize>>;
const PATCH_MAX: f64 = 0.3;
fn apply_move_with_patch(
    board: &mut Board,
    ep_peers: &EpPeers,
    idx: usize,
    new_start: (f64, f64),
    new_end: (f64, f64),
) -> bool {
    const SNAP: f64 = 0.15;
    let key =
        |x: f64, y: f64| -> (i64, i64) { ((x / SNAP).round() as i64, (y / SNAP).round() as i64) };

    let seg = &board.segments[idx];
    let (old_start, old_end) = (seg.start, seg.end);
    let layer = seg.layer.clone();
    let net = seg.net;
    let width = seg.width;

    let moved_start = (new_start.0 - old_start.0).hypot(new_start.1 - old_start.1);
    let moved_end = (new_end.0 - old_end.0).hypot(new_end.1 - old_end.1);

    // Only endpoints actually attached to something else need patches; a patch
    // longer than PATCH_MAX means "give up, move something else".
    let start_ok = ep_peers
        .get(&(key(old_start.0, old_start.1), net))
        .is_some_and(|peers| peers.iter().any(|&p| p != idx));
    let end_ok = ep_peers
        .get(&(key(old_end.0, old_end.1), net))
        .is_some_and(|peers| peers.iter().any(|&p| p != idx));
    if start_ok && moved_start > PATCH_MAX {
        return false;
    }
    if end_ok && moved_end > PATCH_MAX {
        return false;
    }

    if start_ok && moved_start > 1e-9 {
        board.segments.push(Segment {
            start: old_start,
            end: new_start,
            width,
            layer: layer.clone(),
            net,
        });
    }
    if end_ok && moved_end > 1e-9 {
        board.segments.push(Segment {
            start: old_end,
            end: new_end,
            width,
            layer: layer.clone(),
            net,
        });
    }

    board.segments[idx].start = new_start;
    board.segments[idx].end = new_end;
    true
}

/// Build snap-based endpoint connectivity map for nudging connectivity checks.
/// Maps ((snapped_x, snapped_y), net_id) → list of segment indices (usize::MAX for via/pad).
fn build_ep_peers(board: &Board) -> HashMap<((i64, i64), u32), Vec<usize>> {
    let snap_eps = 0.15; // mm — grid resolution
    let snap_key = |x: f64, y: f64| -> (i64, i64) {
        ((x / snap_eps).round() as i64, (y / snap_eps).round() as i64)
    };
    let mut ep_peers: HashMap<((i64, i64), u32), Vec<usize>> = HashMap::new();
    for (i, seg) in board.segments.iter().enumerate() {
        if seg.net == 0 {
            continue;
        }
        ep_peers
            .entry((snap_key(seg.start.0, seg.start.1), seg.net))
            .or_default()
            .push(i);
        ep_peers
            .entry((snap_key(seg.end.0, seg.end.1), seg.net))
            .or_default()
            .push(i);
    }
    for via in &board.vias {
        ep_peers
            .entry((snap_key(via.at.0, via.at.1), via.net))
            .or_default()
            .push(usize::MAX);
    }
    for fp in &board.footprints {
        let (fx, fy, _) = fp.position;
        for pad in &fp.pads {
            if let Some(net) = pad.net {
                let (px, py) = fp.pad_rotated_offset(pad);
                ep_peers
                    .entry((snap_key(fx + px, fy + py), net))
                    .or_default()
                    .push(usize::MAX);
            }
        }
    }
    ep_peers
}

/// Remove all segments and vias belonging to a net, and unmark them from the grid.
fn rip_up_net(board: &mut Board, grid: &mut RoutingGrid, net_id: u32) {
    // Remove segments
    let mut i = 0;
    while i < board.segments.len() {
        if board.segments[i].net == net_id {
            let seg = board.segments.remove(i);
            // Clear grid cells along the removed segment
            clear_segment_from_grid(grid, &seg);
        } else {
            i += 1;
        }
    }

    // Remove vias
    let mut i = 0;
    while i < board.vias.len() {
        if board.vias[i].net == net_id {
            let via = board.vias.remove(i);
            let (gx, gy) = grid.world_to_grid(via.at.0, via.at.1);
            for layer in 0..grid.num_layers {
                if gx < grid.cols && gy < grid.rows {
                    grid.set(layer, gx, gy, Cell::Free);
                }
            }
        } else {
            i += 1;
        }
    }
}

/// Clear grid cells that were occupied by a removed segment.
fn clear_segment_from_grid(grid: &mut RoutingGrid, seg: &Segment) {
    let layer = grid.layer_config.layer_index(&seg.layer).unwrap_or(0);
    let expansion = (DEFAULT_CLEARANCE / grid.grid_res).ceil() as i32;
    let net_id = seg.net;
    let (sx, sy) = grid.world_to_grid(seg.start.0, seg.start.1);
    let (ex, ey) = grid.world_to_grid(seg.end.0, seg.end.1);

    let dx = (ex as i32) - (sx as i32);
    let dy = (ey as i32) - (sy as i32);
    let steps = dx.abs().max(dy.abs()).max(1) as usize;
    let step_x = dx as f64 / steps as f64;
    let step_y = dy as f64 / steps as f64;
    let cols = grid.cols as i32;
    let rows = grid.rows as i32;

    for i in 0..=steps {
        let cx = sx as i32 + (step_x * i as f64).round() as i32;
        let cy = sy as i32 + (step_y * i as f64).round() as i32;

        let x_lo = (cx - expansion).max(0) as usize;
        let x_hi = (cx + expansion + 1).min(cols) as usize;
        let y_lo = (cy - expansion).max(0) as usize;
        let y_hi = (cy + expansion + 1).min(rows) as usize;

        for uy in y_lo..y_hi {
            let row_offset = uy * grid.cols;
            let per_layer = grid.rows * grid.cols;
            for ux in x_lo..x_hi {
                let idx = row_offset + ux;
                let flat = layer * per_layer + idx;
                if matches!(grid.layer_data[flat], Cell::Trace(n) | Cell::Via(n) if n == net_id) {
                    grid.layer_data[flat] = Cell::Free;
                }
            }
        }
    }
}

/// Calculate bounding box area of pad positions.
fn bounding_box_area(pads: &[(f64, f64)]) -> f64 {
    if pads.is_empty() {
        return 0.0;
    }
    let mut min_x = f64::MAX;
    let mut min_y = f64::MAX;
    let mut max_x = f64::MIN;
    let mut max_y = f64::MIN;
    for &(x, y) in pads {
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }
    (max_x - min_x) * (max_y - min_y)
}

/// Route a single net: build minimum spanning tree of pads, then A* each connection.
/// Build MST using Prim's algorithm. Returns 2-pin edges sorted by length (short first).
/// Build MST using Prim's algorithm with Manhattan distance (rectilinear).
/// Returns 2-pin edges sorted by length (short first), plus Steiner points.
fn build_mst_edges(pads: &[(f64, f64)]) -> (Vec<(usize, usize)>, Vec<(f64, f64)>) {
    let n = pads.len();
    if n <= 2 {
        return (vec![(0, 1)], Vec::new());
    }
    if n == 3 {
        // For 3-pin nets: find optimal Steiner point and return star topology
        let (x0, y0) = pads[0];
        let (x1, y1) = pads[1];
        let (x2, y2) = pads[2];
        let sx = median3(x0, x1, x2);
        let sy = median3(y0, y1, y2);
        // Check if Steiner point helps (reduces total wirelength vs MST)
        let mst_len = {
            let mut d: Vec<f64> = Vec::new();
            for i in 0..3 {
                for j in (i + 1)..3 {
                    d.push((pads[i].0 - pads[j].0).abs() + (pads[i].1 - pads[j].1).abs());
                }
            }
            d.sort_by(|a, b| a.partial_cmp(b).unwrap());
            d[0] + d[1] // MST = 2 shortest edges
        };
        let steiner_len = (x0 - sx).abs()
            + (y0 - sy).abs()
            + (x1 - sx).abs()
            + (y1 - sy).abs()
            + (x2 - sx).abs()
            + (y2 - sy).abs();
        if steiner_len < mst_len * 0.98 {
            // Steiner star: 3 edges all through the Steiner point
            let steiner_idx = pads.len(); // will be added to pads by caller
            return (
                vec![(0, steiner_idx), (1, steiner_idx), (2, steiner_idx)],
                vec![(sx, sy)],
            );
        }
    }

    // Prim's MST with Manhattan distance
    let mut in_mst = vec![false; n];
    let mut min_dist = vec![f64::MAX; n];
    let mut nearest = vec![0usize; n];
    let mut edges: Vec<(usize, usize, f64)> = Vec::with_capacity(n - 1);

    in_mst[0] = true;
    for j in 1..n {
        let d = (pads[0].0 - pads[j].0).abs() + (pads[0].1 - pads[j].1).abs();
        min_dist[j] = d;
        nearest[j] = 0;
    }

    for _ in 1..n {
        let mut best = 0usize;
        let mut best_d = f64::MAX;
        for j in 0..n {
            if !in_mst[j] && min_dist[j] < best_d {
                best_d = min_dist[j];
                best = j;
            }
        }
        if best_d == f64::MAX {
            break;
        }
        in_mst[best] = true;
        edges.push((nearest[best], best, best_d));

        for j in 0..n {
            if in_mst[j] {
                continue;
            }
            let d = (pads[best].0 - pads[j].0).abs() + (pads[best].1 - pads[j].1).abs();
            if d < min_dist[j] {
                min_dist[j] = d;
                nearest[j] = best;
            }
        }
    }

    // Sort edges by Manhattan length (short first for routing priority)
    edges.sort_by(|a, b| {
        a.2.partial_cmp(&b.2)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)))
    });
    (
        edges.into_iter().map(|(i, j, _)| (i, j)).collect(),
        Vec::new(),
    )
}

// ============================================================================
// L6 Step 2: Fly-by daisy-chain edge construction + net topology analysis.
// Pure functions, no routing behavior change until Step 3 wires them in.
// ============================================================================

/// Construct a fly-by daisy-chain edge sequence for a multi-pin net.
///
/// Geometry-greedy strategy:
/// 1. Controller pads = chain start. If multiple, the one closest to the
///    centroid of the memory pads seeds the chain (rest attach as a small
///    star at the head — rare for DDR3, which has one controller pin per net).
/// 2. Memory pads sorted by Manhattan distance from the controller, ties
///    broken by quadrant angle to reduce path crossings.
/// 3. Termination pad (if any) appends at the tail.
/// 4. Edges: c → s0 → s1 → ... → sk → [t]
///
/// Returns `None` when fly-by is not applicable, so the caller falls back to
/// `build_mst_edges`:
///   - no Controller pad
///   - pad/role length mismatch (defensive)
///   - fewer than 2 pads total
///   - no Memory and no Termination (controller-only net is degenerate)
///
/// The second return value (Steiner points) is always empty for fly-by: the
/// chain visits real pads only, matching the build_mst_edges signature so the
/// two are drop-in interchangeable at call sites.
pub fn build_flyby_edges(
    pads: &[(f64, f64)],
    roles: &[PadRole],
) -> Option<(Vec<(usize, usize)>, Vec<(f64, f64)>)> {
    if pads.len() != roles.len() || pads.len() < 2 {
        return None;
    }

    // Partition pad indices by role.
    let mut ctrl_idx: Vec<usize> = Vec::new();
    let mut mem_idx: Vec<usize> = Vec::new();
    let mut term_idx: Vec<usize> = Vec::new();
    for (i, r) in roles.iter().enumerate() {
        match r {
            PadRole::Controller => ctrl_idx.push(i),
            PadRole::Memory => mem_idx.push(i),
            PadRole::Termination => term_idx.push(i),
            PadRole::Generic => {} // ignored: fly-by only chains ctrl/mem/term
        }
    }

    // Need at least a controller and one downstream pad (memory or termination).
    if ctrl_idx.is_empty() || (mem_idx.is_empty() && term_idx.is_empty()) {
        return None;
    }

    // Seed = controller closest to memory centroid (reduces initial stub).
    // When there is no memory, just use the first controller.
    let seed = if mem_idx.is_empty() {
        ctrl_idx[0]
    } else {
        let (cx, cy) = mem_idx
            .iter()
            .map(|&i| pads[i])
            .fold((0.0_f64, 0.0_f64), |(sx, sy), (x, y)| (sx + x, sy + y));
        let n = mem_idx.len() as f64;
        let centroid = (cx / n, cy / n);
        *ctrl_idx
            .iter()
            .min_by(|&&a, &&b| {
                let da = (pads[a].0 - centroid.0).abs() + (pads[a].1 - centroid.1).abs();
                let db = (pads[b].0 - centroid.0).abs() + (pads[b].1 - centroid.1).abs();
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            })
            .unwrap()
    };
    let (sx, sy) = pads[seed];

    // Sort memory pads by Manhattan distance from the seed controller.
    // Tie-break by atan2 angle (CCW from +x) so pads in the same quadrant
    // cluster together, which minimizes diagonal crossings in the chain.
    mem_idx.sort_by(|&a, &b| {
        let da = (pads[a].0 - sx).abs() + (pads[a].1 - sy).abs();
        let db = (pads[b].0 - sx).abs() + (pads[b].1 - sy).abs();
        if (da - db).abs() > 1e-9 {
            return da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal);
        }
        let ang_a = angle_from(pads[a], (sx, sy));
        let ang_b = angle_from(pads[b], (sx, sy));
        ang_a
            .partial_cmp(&ang_b)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Build the visit chain: [seed] ++ extra controllers ++ memory ++ termination.
    // Extra controllers (rare) attach right after the seed as a 1-step star.
    let mut order: Vec<usize> = Vec::with_capacity(pads.len());
    order.push(seed);
    for &c in &ctrl_idx {
        if c != seed {
            order.push(c);
        }
    }
    order.extend_from_slice(&mem_idx);
    order.extend_from_slice(&term_idx);

    // Edges = consecutive pairs along the visit order.
    let edges: Vec<(usize, usize)> = order.windows(2).map(|w| (w[0], w[1])).collect();
    Some((edges, Vec::new()))
}

/// atan2 angle of `p` relative to `origin`, in radians [−π, π].
/// Used only as a stable tie-breaker, so the exact range does not matter.
fn angle_from(p: (f64, f64), origin: (f64, f64)) -> f64 {
    let dy = p.1 - origin.1;
    let dx = p.0 - origin.0;
    dy.atan2(dx)
}

/// Derive the linear visit order implied by an edge list.
/// `edges` is assumed to be a simple path (each interior vertex appears once
/// as a head and once as a tail). Returns the vertex sequence.
fn derive_visit_order(edges: &[(usize, usize)], _n_pads: usize) -> Vec<usize> {
    if edges.is_empty() {
        return Vec::new();
    }
    let mut order = Vec::with_capacity(edges.len() + 1);
    order.push(edges[0].0);
    for &(_, j) in edges {
        order.push(j);
    }
    order
}

/// Decide a net's routing topology from its name + the roles of its pads.
///
/// Fly-by is enabled only when ALL hold:
///   - net name matches DDR3 address/command/clock patterns
///     (DQ/DQS data groups stay on Auto — they are point-to-point
///     controller↔memory with no termination, so daisy-chain would hurt)
///   - at least one Controller pad and one Memory pad are present
///   - 3+ pads total (2-pin nets are trivially point-to-point)
///   - `build_flyby_edges` succeeds (it returns None on degenerate inputs)
///
/// Everything else returns Auto, preserving existing MST/Steiner behavior.
pub fn analyze_net_topology(net_name: &str, pads: &[(f64, f64)], roles: &[PadRole]) -> NetTopology {
    let u = net_name.to_uppercase();
    let is_flyby_net = u.contains("DDR3_A")
        || u.contains("DDR3_BA")
        || u.contains("DDR3_CAS")
        || u.contains("DDR3_RAS")
        || u.contains("DDR3_WE")
        || u.contains("DDR3_CK")
        || u.contains("DDR3_CKE")
        || u.contains("DDR3_ODT")
        || u.contains("DDR3_CS");
    // Fly-by only makes sense for true multi-drop: controller + ≥2 memory
    // (or ≥1 memory + termination). Single-SDRAM boards (e.g. scopefun) have
    // point-to-point ADDR/CMD (controller↔memory, 2 pads) — forcing fly-by
    // there lengthens routes and inflates DRC conflicts without benefit.
    // Threshold ≥4 pads = controller + ≥2 downstream (typical: 2 SDRAM, or
    // 1 SDRAM + termination + stub). Below this, MST is shorter and safer.
    if !is_flyby_net || pads.len() < 4 || roles.len() != pads.len() {
        return NetTopology {
            kind: TopologyKind::Auto,
            visit_order: vec![],
        };
    }
    let has_ctrl = roles.contains(&PadRole::Controller);
    let has_mem = roles.contains(&PadRole::Memory);
    if !has_ctrl || !has_mem {
        return NetTopology {
            kind: TopologyKind::Auto,
            visit_order: vec![],
        };
    }
    match build_flyby_edges(pads, roles) {
        Some((edges, _)) => {
            let visit_order = derive_visit_order(&edges, pads.len());
            NetTopology {
                kind: TopologyKind::FlyBy,
                visit_order,
            }
        }
        None => NetTopology {
            kind: TopologyKind::Auto,
            visit_order: vec![],
        },
    }
}

/// Unified entry point replacing direct `build_mst_edges` calls in routing.
///
/// When a `TopologyContext` is available and the net qualifies for fly-by
/// (DDR3 ADDR/CMD/CLK with controller+memory pads), returns the fly-by
/// daisy-chain edges. Otherwise falls back to MST/Steiner — identical to the
/// previous behavior. This is the single decision point that Step 3 wires
/// into the 3 former `build_mst_edges` call sites.
///
/// Performance: this is a hot path (called per net per rip-up round). The
/// fast path is a single cheap net-name substring check — only DDR3-named
/// nets pay the HashMap lookups + role analysis. All other nets hit MST
/// directly with zero overhead vs. the previous `build_mst_edges(pads)` call.
pub fn build_topology_edges(
    pads: &[(f64, f64)],
    net_id: u32,
    topo: &TopologyContext,
) -> (Vec<(usize, usize)>, Vec<(f64, f64)>) {
    // Fast path: O(1) HashSet check. Non-DDR3 nets (the >95% majority) skip
    // all role analysis and hit MST directly — zero overhead vs. the previous
    // build_mst_edges(pads) call.
    if !topo.is_ddr3(net_id) {
        return build_mst_edges(pads);
    }
    if let Some(roles) = topo.roles_for(net_id) {
        if roles.len() == pads.len() {
            let net_name = topo.name_for(net_id);
            let t = analyze_net_topology(net_name, pads, roles);
            if t.kind == TopologyKind::FlyBy {
                if let Some(edges_sp) = build_flyby_edges(pads, roles) {
                    return edges_sp;
                }
            }
        }
    }
    build_mst_edges(pads)
}

fn median3(a: f64, b: f64, c: f64) -> f64 {
    if a <= b {
        if b <= c {
            b
        } else if a <= c {
            c
        } else {
            a
        }
    } else {
        if a <= c {
            a
        } else if b <= c {
            c
        } else {
            b
        }
    }
}

fn route_single_net(
    grid: &mut RoutingGrid,
    net_id: u32,
    pads: &[(f64, f64)],
    trace_width: f64,
    board: &mut Board,
    history_congestion: &[f64],
    topo_ctx: &TopologyContext,
) -> bool {
    if pads.len() < 2 {
        return false;
    }

    // L6: topology-aware decomposition (fly-by for DDR3 ADDR/CMD/CLK, else MST)
    let (edges, steiner_pts) = build_topology_edges(pads, net_id, topo_ctx);
    let all_pads: Vec<(f64, f64)> = pads
        .iter()
        .copied()
        .chain(steiner_pts.iter().copied())
        .collect();
    let mut all_connected = true;

    for (pi, pj) in edges {
        let (sx, sy) = all_pads[pi];
        let (ex, ey) = all_pads[pj];
        // Auto-select starting layer based on predominant direction
        let signal_layers = grid.layer_config.signal_layer_indices();
        let preferred = if signal_layers.len() >= 2 {
            let h_span = (sx - ex).abs();
            let v_span = (sy - ey).abs();
            if h_span >= v_span {
                signal_layers[0]
            } else {
                *signal_layers.last().unwrap()
            }
        } else {
            0
        };
        let alternate = if signal_layers.len() >= 2 && preferred == signal_layers[0] {
            *signal_layers.last().unwrap()
        } else if signal_layers.len() >= 2 {
            signal_layers[0]
        } else {
            0
        };

        let mut path = None;
        #[allow(unused_assignments)] // 首值在 geometric L-route 失败后被覆盖
        let mut geo_elements: Option<(Vec<Segment>, Vec<Via>)> = None;

        // P4: try the grid-free geometric L-route first — exact pad-to-pad
        // endpoints (grid A* snaps them, starving pads) and no 44-segment
        // mazes. Falls back to the grid router when blocked.
        {
            let sig_idx: Vec<usize> = grid.layer_config.signal_layer_indices().to_vec();
            let lc = grid.layer_config.clone();
            let empty: Vec<((f64, f64), (f64, f64))> = Vec::new();
            let esc = topo_ctx.escape_by_net.get(&net_id).unwrap_or(&empty);
            geo_elements = geometric_route_edge(
                board,
                net_id,
                sx,
                sy,
                ex,
                ey,
                trace_width,
                DEFAULT_CLEARANCE,
                &sig_idx,
                &lc,
                esc,
            );
        }

        if geo_elements.is_none() {
            for &layer in &[preferred, alternate] {
                let start = Node {
                    col: grid.world_to_grid(sx, sy).0,
                    row: grid.world_to_grid(sx, sy).1,
                    layer,
                };
                let goal = Node {
                    col: grid.world_to_grid(ex, ey).0,
                    row: grid.world_to_grid(ex, ey).1,
                    layer,
                };
                path = try_straight_line_route(grid, start, goal, net_id, layer, DEFAULT_CLEARANCE)
                    .or_else(|| {
                        try_l_shape_route(grid, start, goal, net_id, layer, DEFAULT_CLEARANCE)
                    })
                    .or_else(|| {
                        astar_route(
                            grid,
                            start,
                            goal,
                            net_id,
                            None,
                            trace_width,
                            DEFAULT_CLEARANCE,
                            history_congestion,
                        )
                    });
                if path.is_some() {
                    break;
                }
            }
        }

        if geo_elements.is_some() || path.is_some() {
            let (segments, vias) = if let Some((segs, vias)) = geo_elements {
                (segs, vias)
            } else {
                path_to_board_elements(&path.unwrap(), grid, net_id, trace_width)
            };
            let sig_layers = grid.layer_config.signal_layer_indices();

            // Incremental grid update: only mark the new segments
            for seg in &segments {
                mark_segment_on_grid(grid, seg, net_id, DEFAULT_CLEARANCE);
            }
            for via in &vias {
                let (gx, gy) = grid.world_to_grid(via.at.0, via.at.1);
                if gx < grid.cols && gy < grid.rows {
                    for &layer in &sig_layers {
                        grid.set(layer, gx, gy, Cell::Via(net_id));
                    }
                }
            }

            board.segments.extend(segments);
            board.vias.extend(vias);
        } else {
            // No fallback — mark as failed for rip-up retry
            all_connected = false;
        }
    }

    all_connected
}

/// Count geometric conflicts between new segments and existing board elements.
/// Uses spatial hashing for O(1) neighbor lookup instead of O(n) full scan.
/// Pre-built spatial index for segment conflict checking.
/// Build once per net, reuse across multiple edge conflict checks.
struct SegmentSpatialIndex<'a> {
    index: HashMap<(i32, i32, &'a str), Vec<usize>>,
    segments: &'a [Segment],
    footprints: &'a [kicad_json5::ir::board::Footprint],
}

impl<'a> SegmentSpatialIndex<'a> {
    fn build(board: &'a Board, exclude_net: u32) -> Self {
        let bin_size = 2.0f64;
        let hash_bin = |x: f64, y: f64| -> (i32, i32) {
            ((x / bin_size).floor() as i32, (y / bin_size).floor() as i32)
        };
        let mut index: HashMap<(i32, i32, &str), Vec<usize>> = HashMap::new();
        for (i, es) in board.segments.iter().enumerate() {
            if es.net == exclude_net || es.width <= 0.0 {
                continue;
            }
            let (b1x, b1y) = hash_bin(es.start.0, es.start.1);
            let (b2x, b2y) = hash_bin(es.end.0, es.end.1);
            for bx in b1x.min(b2x)..=b1x.max(b2x) {
                for by in b1y.min(b2y)..=b1y.max(b2y) {
                    index
                        .entry((bx, by, es.layer.as_str()))
                        .or_default()
                        .push(i);
                }
            }
        }
        SegmentSpatialIndex {
            index,
            segments: &board.segments,
            footprints: &board.footprints,
        }
    }

    fn count_conflicts(&self, new_segments: &[Segment], net_id: u32) -> usize {
        if new_segments.is_empty() {
            return 0;
        }
        let bin_size = 2.0f64;
        let hash_bin = |x: f64, y: f64| -> (i32, i32) {
            ((x / bin_size).floor() as i32, (y / bin_size).floor() as i32)
        };
        let mut conflicts = 0;
        for ns in new_segments {
            if ns.width <= 0.0 {
                continue;
            }
            let half_w = ns.width / 2.0;
            let (b1x, b1y) = hash_bin(ns.start.0, ns.start.1);
            let (b2x, b2y) = hash_bin(ns.end.0, ns.end.1);
            let mut checked: HashSet<usize> = HashSet::new();
            for bx in b1x.min(b2x) - 1..=b1x.max(b2x) + 1 {
                for by in b1y.min(b2y) - 1..=b1y.max(b2y) + 1 {
                    if let Some(indices) = self.index.get(&(bx, by, ns.layer.as_str())) {
                        for &i in indices {
                            if !checked.insert(i) {
                                continue;
                            }
                            let es = &self.segments[i];
                            let min_dist = half_w + es.width / 2.0;
                            let dist = crate::drc::segment_to_segment_dist(
                                ns.start, ns.end, es.start, es.end,
                            );
                            if dist < min_dist {
                                conflicts += 1;
                            }
                        }
                    }
                }
            }
            // Trace-to-pad
            for fp in self.footprints {
                let (fx, fy, _) = fp.position;
                for pad in &fp.pads {
                    let pad_net = pad.net.unwrap_or(0);
                    if pad_net == 0 || pad_net == net_id {
                        continue;
                    }
                    if !pad.layers.iter().any(|pl| pl == &ns.layer || pl == "*.Cu") {
                        continue;
                    }
                    let (px, py) = fp.pad_rotated_offset(pad);
                    let pad_r = pad.size.0.max(pad.size.1) / 2.0;
                    let dist =
                        crate::drc::point_to_segment_dist((fx + px, fy + py), ns.start, ns.end);
                    if dist < half_w + pad_r {
                        conflicts += 1;
                    }
                }
            }
        }
        conflicts
    }
}

fn count_segment_conflicts(new_segments: &[Segment], board: &Board, net_id: u32) -> usize {
    if new_segments.is_empty() {
        return 0;
    }

    let mut conflicts = 0;

    // Spatial hash: bin size = 2mm (covers typical trace width + clearance)
    let bin_size = 2.0f64;
    let hash_bin = |x: f64, y: f64| -> (i32, i32) {
        ((x / bin_size).floor() as i32, (y / bin_size).floor() as i32)
    };

    // Build spatial index of existing same-layer segments
    // Only index segments that could conflict (different net, has width)
    let mut seg_index: HashMap<(i32, i32, String), Vec<usize>> = HashMap::new();
    for (i, es) in board.segments.iter().enumerate() {
        if es.net == net_id || es.width <= 0.0 {
            continue;
        }
        // Add midpoint bin + neighbor bins for the segment extent
        let (b1x, b1y) = hash_bin(es.start.0, es.start.1);
        let (b2x, b2y) = hash_bin(es.end.0, es.end.1);
        let min_bx = b1x.min(b2x);
        let max_bx = b1x.max(b2x);
        let min_by = b1y.min(b2y);
        let max_by = b1y.max(b2y);
        for bx in min_bx..=max_bx {
            for by in min_by..=max_by {
                seg_index
                    .entry((bx, by, es.layer.clone()))
                    .or_default()
                    .push(i);
            }
        }
    }

    for ns in new_segments {
        if ns.width <= 0.0 {
            continue;
        }
        let half_w = ns.width / 2.0;

        // Collect candidate segments from nearby bins
        let (b1x, b1y) = hash_bin(ns.start.0, ns.start.1);
        let (b2x, b2y) = hash_bin(ns.end.0, ns.end.1);
        let min_bx = b1x.min(b2x) - 1;
        let max_bx = b1x.max(b2x) + 1;
        let min_by = b1y.min(b2y) - 1;
        let max_by = b1y.max(b2y) + 1;

        let mut checked: HashSet<usize> = HashSet::new();
        for bx in min_bx..=max_bx {
            for by in min_by..=max_by {
                if let Some(indices) = seg_index.get(&(bx, by, ns.layer.clone())) {
                    for &i in indices {
                        if !checked.insert(i) {
                            continue;
                        }
                        let es = &board.segments[i];
                        let min_dist = half_w + es.width / 2.0;
                        let dist =
                            crate::drc::segment_to_segment_dist(ns.start, ns.end, es.start, es.end);
                        if dist < min_dist {
                            conflicts += 1;
                        }
                    }
                }
            }
        }

        // Trace-to-pad: check pads of different nets (kept as-is, pad count is small)
        for fp in &board.footprints {
            let (fx, fy, _) = fp.position;
            for pad in &fp.pads {
                let pad_net = pad.net.unwrap_or(0);
                if pad_net == 0 || pad_net == net_id {
                    continue;
                }
                if !pad.layers.iter().any(|pl| pl == &ns.layer || pl == "*.Cu") {
                    continue;
                }
                let (px, py) = fp.pad_rotated_offset(pad);
                let pad_r = pad.size.0.max(pad.size.1) / 2.0;
                let dist = crate::drc::point_to_segment_dist((fx + px, fy + py), ns.start, ns.end);
                if dist < half_w + pad_r {
                    conflicts += 1;
                }
            }
        }
    }
    conflicts
}

/// Route a single net with explicit clearance (used by adaptive rip-up).
#[allow(dead_code)]
fn route_single_net_with_clearance(
    grid: &mut RoutingGrid,
    net_id: u32,
    pads: &[(f64, f64)],
    trace_width: f64,
    board: &mut Board,
    clearance: f64,
    history_congestion: &[f64],
    topo_ctx: &TopologyContext,
) -> bool {
    route_single_net_with_clearance_and_iters(
        grid,
        net_id,
        pads,
        trace_width,
        board,
        clearance,
        MAX_ITERATIONS,
        history_congestion,
        None,
        topo_ctx,
    )
}

/// Route a single net with explicit clearance and iteration limit.
/// `preferred_layer`: Optional MPS-assigned signal layer index to prefer.
// ---------------------------------------------------------------------------
// Parallel-safe pathfinding: split read-only A* from grid mutation
// ---------------------------------------------------------------------------
/// Per-edge path result (before grid mutation).
struct EdgePathResult {
    segments: Vec<Segment>,
    vias: Vec<Via>,
}

/// Per-net pathfinding result — produced by `pathfind_single_net`.
struct NetPathResult {
    net_id: u32,
    edge_results: Vec<EdgePathResult>,
    all_connected: bool,
}

/// BFS maze routing fallback: ignores congestion penalties, only avoids physical obstacles.
/// Uses 4-directional search on signal layers. No clearance expansion — fast but may
/// produce tighter spacing than A*. Commit step handles clearance marking on grid.
const BFS_MAX_ITERS: usize = 30_000;

fn bfs_route(
    grid: &RoutingGrid,
    start: Node,
    goal: Node,
    net_id: u32,
    _clearance: f64,
    cache: &mut AStarCache,
) -> Option<Vec<Node>> {
    let signal_layers = grid.layer_config.signal_layer_indices();
    if signal_layers.is_empty() {
        return None;
    }

    cache.version = cache.version.wrapping_add(1);
    let ver = cache.version;
    let AStarCache {
        g_version,
        came_from,
        cf_version,
        ..
    } = cache;

    macro_rules! is_visited {
        ($i:expr) => {
            g_version[$i] == ver
        };
    }
    macro_rules! mark_visited {
        ($i:expr, $parent:expr) => {
            g_version[$i] = ver;
            came_from[$i] = $parent;
            cf_version[$i] = ver;
        };
    }

    let max_layer_idx = *signal_layers.iter().max().unwrap_or(&0);
    let layer_slot_tbl: Vec<usize> = {
        let mut tbl = vec![0usize; max_layer_idx + 1];
        for (slot, &layer) in signal_layers.iter().enumerate() {
            tbl[layer] = slot;
        }
        tbl
    };
    let layer_slot = |layer: usize| -> Option<usize> { layer_slot_tbl.get(layer).copied() };

    let flat_idx = |n: &Node| -> usize {
        let slot = layer_slot(n.layer).unwrap_or(0);
        (slot * grid.rows + n.row) * grid.cols + n.col
    };
    let pack_node = |n: &Node| -> u32 {
        let slot = layer_slot(n.layer).unwrap_or(0);
        (n.col as u32) | ((n.row as u32) << 12) | ((slot as u32) << 24)
    };
    let unpack_node = |packed: u32| -> Node {
        let col = (packed & 0xFFF) as usize;
        let row = ((packed >> 12) & 0xFFF) as usize;
        let slot = ((packed >> 24) & 0xFF) as usize;
        let layer = signal_layers.get(slot).copied().unwrap_or(signal_layers[0]);
        Node { col, row, layer }
    };

    let cols = grid.cols;

    let si = flat_idx(&start);
    mark_visited![si, pack_node(&start)];

    let mut queue: VecDeque<Node> = VecDeque::with_capacity(4096);
    queue.push_back(start);

    let goal_flat = flat_idx(&goal);
    let start_packed = pack_node(&start);
    let mut iters = 0usize;

    while let Some(current) = queue.pop_front() {
        iters += 1;
        if iters > BFS_MAX_ITERS {
            return None;
        }

        let cur_flat = flat_idx(&current);
        if cur_flat == goal_flat {
            // Reconstruct path via came_from
            let mut path = Vec::new();
            let mut node = current;
            loop {
                path.push(node);
                let ni = flat_idx(&node);
                let parent_packed = if cf_version[ni] == ver {
                    came_from[ni]
                } else {
                    break;
                };
                if parent_packed == start_packed {
                    path.push(unpack_node(parent_packed));
                    break;
                }
                node = unpack_node(parent_packed);
            }
            path.reverse();
            return Some(path);
        }

        let cur_packed = pack_node(&current);
        let layer = current.layer;
        let c = current.col;
        let r = current.row;

        // 4-directional neighbors on same layer
        for (dc, dr) in [(1i32, 0i32), (-1, 0), (0, 1), (0, -1)] {
            let nc = c as i32 + dc;
            let nr = r as i32 + dr;
            if nc < 0 || nr < 0 {
                continue;
            }
            let (nc, nr) = (nc as usize, nr as usize);
            if nc >= cols || nr >= grid.rows {
                continue;
            }

            let ni = flat_idx(&Node {
                col: nc,
                row: nr,
                layer,
            });
            if is_visited![ni] {
                continue;
            }
            if !grid.in_bounds(nc, nr) {
                continue;
            }
            match grid.get(layer, nc, nr) {
                Cell::Blocked => continue,
                Cell::Trace(n) | Cell::Via(n) | Cell::Pad(n) if n != net_id && n != 0 => continue,
                _ => {}
            }
            mark_visited![ni, cur_packed];
            queue.push_back(Node {
                col: nc,
                row: nr,
                layer,
            });
        }

        // Via to other signal layers
        for &other_layer in &signal_layers {
            if other_layer == layer {
                continue;
            }
            let ni = flat_idx(&Node {
                col: c,
                row: r,
                layer: other_layer,
            });
            if is_visited![ni] {
                continue;
            }
            match grid.get(other_layer, c, r) {
                Cell::Blocked => continue,
                Cell::Trace(n) | Cell::Via(n) | Cell::Pad(n) if n != net_id && n != 0 => continue,
                _ => {}
            }
            mark_visited![ni, cur_packed];
            queue.push_back(Node {
                col: c,
                row: r,
                layer: other_layer,
            });
        }
    }

    None
}

/// Pathfind a single net without mutating grid or board.
/// All grid reads use `&RoutingGrid` — safe for parallel invocation via rayon.
/// The caller must provide a per-thread `AStarCache` to avoid RefCell contention.
fn pathfind_single_net(
    grid: &RoutingGrid,
    net_id: u32,
    pads: &[(f64, f64)],
    trace_width: f64,
    board: &Board,
    clearance: f64,
    max_iters: usize,
    history_congestion: &[f64],
    preferred_layer: Option<usize>,
    cache: &mut AStarCache,
    topo_ctx: &TopologyContext,
) -> NetPathResult {
    if pads.len() < 2 {
        return NetPathResult {
            net_id,
            edge_results: Vec::new(),
            all_connected: false,
        };
    }

    let prefer_inner = board
        .nets
        .iter()
        .find(|n| n.id == net_id)
        .map(|n| {
            let u = n.name.to_uppercase();
            u.contains("DDR")
                || u.contains("LVDS")
                || u.contains("MIPI")
                || u.contains("USB")
                || u.contains("DQS")
                || u.contains("CLK")
        })
        .unwrap_or(false);

    // L6: topology-aware decomposition (fly-by for DDR3 ADDR/CMD/CLK, else MST)
    let (edges, steiner_pts) = build_topology_edges(pads, net_id, topo_ctx);
    let all_pads: Vec<(f64, f64)> = pads
        .iter()
        .copied()
        .chain(steiner_pts.iter().copied())
        .collect();
    let mut all_connected = true;
    let mut edge_results = Vec::new();

    // Pre-build spatial index once for all edge conflict checks (board is read-only)
    let spatial_idx = SegmentSpatialIndex::build(board, net_id);

    for (pi, pj) in edges {
        let (sx, sy) = all_pads[pi];
        let (ex, ey) = all_pads[pj];
        let signal_layers = grid.layer_config.signal_layer_indices();

        let preferred = if let Some(pl) = preferred_layer {
            pl
        } else if signal_layers.len() >= 2 {
            if (sx - ex).abs() >= (sy - ey).abs() {
                signal_layers[0]
            } else {
                *signal_layers.last().unwrap()
            }
        } else {
            0
        };
        let alternate = if signal_layers.len() >= 2 && preferred == signal_layers[0] {
            *signal_layers.last().unwrap()
        } else if signal_layers.len() >= 2 {
            signal_layers[0]
        } else {
            0
        };

        let skip_conflict_check = board.footprints.len() > 200;

        let mut best_segments: Vec<Segment> = Vec::new();
        let mut best_vias: Vec<Via> = Vec::new();
        let mut best_conflicts = usize::MAX;
        let mut found = false;

        // P4: geometric L-with-escape-stubs first — exact pad endpoints and
        // 2-5 segments instead of the grid A* staircase. Any conflicts are
        // counted the same way; 0-conflict wins immediately.
        if !skip_conflict_check {
            let sig_idx: Vec<usize> = signal_layers.to_vec();
            let lc = grid.layer_config.clone();
            let esc = topo_ctx.escape_by_net.get(&net_id);
            if let Some((segs, vias)) = geometric_route_edge(
                board,
                net_id,
                sx,
                sy,
                ex,
                ey,
                trace_width,
                clearance,
                &sig_idx,
                &lc,
                esc.map(|v| v.as_slice()).unwrap_or(&[]),
            ) {
                let conflicts = spatial_idx.count_conflicts(&segs, net_id);
                if conflicts == 0 {
                    edge_results.push(EdgePathResult {
                        segments: segs,
                        vias,
                    });
                    continue;
                }
                best_segments = segs;
                best_vias = vias;
                best_conflicts = conflicts;
                found = true;
            }
        }

        // Build layer try list: preferred first, then remaining signal layers
        let mut layers_vec: Vec<usize> = vec![preferred];
        if !skip_conflict_check {
            if alternate != preferred {
                layers_vec.push(alternate);
            }
            // For 3+ signal layers, try all remaining layers too
            for &l in &signal_layers {
                if l != preferred && l != alternate && !layers_vec.contains(&l) {
                    layers_vec.push(l);
                }
            }
        }
        let layers_to_try = &layers_vec;

        for &layer in layers_to_try {
            let start = Node {
                col: grid.world_to_grid(sx, sy).0,
                row: grid.world_to_grid(sx, sy).1,
                layer,
            };
            let goal = Node {
                col: grid.world_to_grid(ex, ey).0,
                row: grid.world_to_grid(ex, ey).1,
                layer,
            };
            if let Some(p) = try_straight_line_route(grid, start, goal, net_id, layer, clearance)
                .or_else(|| try_l_shape_route(grid, start, goal, net_id, layer, clearance))
                .or_else(|| {
                    astar_route_with_limit_impl(
                        grid,
                        start,
                        goal,
                        net_id,
                        None,
                        trace_width,
                        clearance,
                        max_iters,
                        history_congestion,
                        prefer_inner,
                        cache,
                        None, // global guide not passed through pathfind_single_net
                    )
                })
            {
                let (segs, vias) = path_to_board_elements(&p, grid, net_id, trace_width);
                let conflicts = spatial_idx.count_conflicts(&segs, net_id);
                if conflicts < best_conflicts {
                    best_segments = segs;
                    best_vias = vias;
                    best_conflicts = conflicts;
                    found = true;
                }
                if conflicts == 0 {
                    break;
                }
            }
        }

        if found {
            edge_results.push(EdgePathResult {
                segments: best_segments,
                vias: best_vias,
            });
        } else {
            // BFS fallback: relaxed search ignoring congestion, only avoiding obstacles
            for &layer in layers_to_try {
                let start = Node {
                    col: grid.world_to_grid(sx, sy).0,
                    row: grid.world_to_grid(sx, sy).1,
                    layer,
                };
                let goal = Node {
                    col: grid.world_to_grid(ex, ey).0,
                    row: grid.world_to_grid(ex, ey).1,
                    layer,
                };
                if let Some(p) = bfs_route(grid, start, goal, net_id, clearance, cache) {
                    let (segs, vias) = path_to_board_elements(&p, grid, net_id, trace_width);
                    edge_results.push(EdgePathResult {
                        segments: segs,
                        vias,
                    });
                    found = true;
                    break;
                }
            }
            if !found {
                all_connected = false;
            }
        }
    }

    NetPathResult {
        net_id,
        edge_results,
        all_connected,
    }
}

/// Commit a successfully pathfound net to grid and board.
fn commit_net_path(
    grid: &mut RoutingGrid,
    board: &mut Board,
    result: &NetPathResult,
    clearance: f64,
) {
    let net_id = result.net_id;
    let sig_layers: Vec<usize> = grid.layer_config.signal_layer_indices();
    for edge in &result.edge_results {
        for seg in &edge.segments {
            mark_segment_on_grid(grid, seg, net_id, clearance);
        }
        for via in &edge.vias {
            let (gx, gy) = grid.world_to_grid(via.at.0, via.at.1);
            if gx < grid.cols && gy < grid.rows {
                for &layer in &sig_layers {
                    grid.set(layer, gx, gy, Cell::Via(net_id));
                }
            }
        }
        board.segments.extend(edge.segments.iter().cloned());
        board.vias.extend(edge.vias.iter().cloned());
    }
}

fn route_single_net_with_clearance_and_iters(
    grid: &mut RoutingGrid,
    net_id: u32,
    pads: &[(f64, f64)],
    trace_width: f64,
    board: &mut Board,
    clearance: f64,
    max_iters: usize,
    history_congestion: &[f64],
    preferred_layer: Option<usize>,
    topo_ctx: &TopologyContext,
) -> bool {
    // Reuse grid's pre-allocated AStarCache for serial calls (avoids allocation overhead)
    let mut cache = grid.astar_cache.borrow_mut();
    let result = pathfind_single_net(
        grid,
        net_id,
        pads,
        trace_width,
        board,
        clearance,
        max_iters,
        history_congestion,
        preferred_layer,
        &mut cache,
        topo_ctx,
    );
    drop(cache); // release borrow before mutation
    if result.all_connected {
        commit_net_path(grid, board, &result, clearance);
    }
    result.all_connected
}

// ---------------------------------------------------------------------------
// Differential pair routing + length matching
// ---------------------------------------------------------------------------

/// Identify high-speed signal nets by naming convention.
/// These nets get priority routing and inner-layer preference.
fn identify_high_speed_nets(board: &Board) -> HashSet<u32> {
    let mut hs = HashSet::new();
    for net in &board.nets {
        let upper = net.name.to_uppercase();
        let is_hs = upper.contains("DDR")
            || upper.contains("LVDS")
            || upper.contains("MIPI")
            || upper.contains("USB")
            || upper.contains("SPI")
            || upper.contains("CLK")
            || upper.contains("FDATA")
            || upper.contains("FADDR")
            || upper.contains("DAC_D")
            || upper.contains("ADC_")
            || upper.contains("DATA")
            || upper.contains("HS_")
            || upper.contains("PCIE")
            || upper.contains("PCI_")
            || upper.contains("SATA")
            || upper.contains("ETH")
            || upper.contains("SERDES")
            || upper.contains("SFP")
            || upper.contains("XAUI")
            || upper.contains("AURORA");
        if is_hs {
            hs.insert(net.id);
        }
    }
    hs
}

/// Bus group for coordinated routing (e.g., DDR3 byte lanes, SerDes channels).
struct BusGroup {
    name: String,
    net_ids: Vec<u32>,
    _priority: u32,
    /// Maximum allowed length skew within this group (mm).
    intra_skew_mm: f64,
    /// Allowed length delta relative to the reference group (mm).
    /// Positive means this group can be longer; negative means shorter.
    inter_skew_mm: Option<f64>,
    /// Reference group index for inter-group skew (index into the groups vec).
    reference_group: Option<usize>,
}

// ============================================================================
// L6: Routing topology framework — Step 1 (types + footprint role classifier)
//
// General-purpose topology constraint system. First consumer is DDR3 fly-by
// (daisy-chain: controller → SDRAM pads → termination), but the enum is
// designed to host T-branch / point-to-point / future topologies too.
// Step 1 only adds types + classifier; routing behavior is unchanged until
// Step 3 wires build_topology_edges into the 3 build_mst_edges call sites.
// ============================================================================

/// Routing topology constraint applied to a multi-pin net.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopologyKind {
    /// Default: MST / 3-pin Steiner decomposition (existing behavior).
    Auto,
    /// Fly-by daisy-chain: controller → [SDRAM...] → termination, visited in
    /// geometrically greedy order (Manhattan distance from controller).
    FlyBy,
    /// T-branch: controller → central T-junction → endpoints (future use).
    TBranch,
    /// Strict point-to-point (2-pin). Equivalent to Auto for n==2 (future use).
    PointToPoint,
}

/// Functional role of a pad within a multi-pin net, derived from its footprint.
/// Used to pick a routing topology (e.g. FlyBy needs Controller + Memory +
/// optional Termination).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadRole {
    /// SoC / FPGA / MCU hosting the DDR controller (signal source).
    Controller,
    /// SDRAM / DDR DRAM package (signal destination, may be multiple).
    Memory,
    /// Termination resistor (R*) at the far end of a fly-by chain.
    Termination,
    /// Anything else (caps, generic ICs, connectors).
    Generic,
}

/// Topology analysis result for a single net.
pub struct NetTopology {
    pub kind: TopologyKind,
    /// Pad indices in visit order. Empty for Auto (MST decides per-edge).
    /// For FlyBy this is [controller, sdrm_0, sdrm_1, ..., termination].
    pub visit_order: Vec<usize>,
}

/// Read-only topology context threaded through routing.
///
/// Built once at the top of `route_board` from the pad collection loop:
///   - `roles_by_net` is a sidecar of `pads_by_net` — same net_id → same pad
///     order, each entry carrying that pad's `PadRole`. Keeping it parallel
///     avoids touching the load-bearing `HashMap<u32, Vec<(f64, f64)>>` type
///     (~15 call sites).
///   - `net_name_by_id` lets the 3 routing entry points resolve a net name
///     from just its id, which is all they have in scope.
///
/// Fly-by is only consulted for DDR3 ADDR/CMD/CLK nets (see
/// `analyze_net_topology`), so for the vast majority of nets this struct is a
/// no-op pass-through to `build_mst_edges`.
#[derive(Default)]
pub struct TopologyContext {
    pub roles_by_net: HashMap<u32, Vec<PadRole>>,
    pub net_name_by_id: HashMap<u32, String>,
    /// Pre-computed set of net ids whose name contains "DDR3" — the fly-by
    /// gating check. Hot-path lookup in `build_topology_edges` is O(1) on
    /// this set, avoiding a HashMap + string scan per net per rip-up round.
    pub ddr3_net_ids: HashSet<u32>,
    /// P4: per-pad escape directions (pad position → unit vector from the
    /// footprint center). The geometric L-router uses them to start each
    /// fine-pitch pad with a perpendicular stub so traces never run parallel
    /// inside a pad row.
    pub escape_by_net: HashMap<u32, Vec<((f64, f64), (f64, f64))>>,
}

impl TopologyContext {
    /// Resolve roles for a net. Returns None if the net has no role info
    /// (e.g. a net added during rip-up that wasn't in the original collection).
    pub fn roles_for(&self, net_id: u32) -> Option<&[PadRole]> {
        self.roles_by_net.get(&net_id).map(|v| v.as_slice())
    }

    /// Resolve a net name. Falls back to the empty string when unknown, in
    /// which case `analyze_net_topology` returns Auto (safe fallback).
    pub fn name_for(&self, net_id: u32) -> &str {
        self.net_name_by_id
            .get(&net_id)
            .map(|s| s.as_str())
            .unwrap_or("")
    }

    /// O(1) check whether a net is DDR3-named (fly-by candidate gate).
    pub fn is_ddr3(&self, net_id: u32) -> bool {
        self.ddr3_net_ids.contains(&net_id)
    }
}

/// Classify a footprint's role in high-speed topology by lib_id + reference.
///
/// Keyword-matching style mirrors `classify_ic_priority_from_footprint`
/// (layout_engine.rs:2326). Conservative: only returns Memory/Controller when
/// strong evidence exists; falls back to Termination for R-prefixed refdes
/// (termination resistors) and Generic otherwise. Misclassifications are safe
/// because `analyze_net_topology` only enables FlyBy on DDR3 ADDR/CMD/CLK nets
/// that additionally contain both a Controller and a Memory pad.
///
/// Priority order (first match wins):
///   1. Termination — R*/RN* refdes (termination resistors). Checked FIRST so
///      that a resistor whose value string happens to contain "DDR3" is not
///      misread as Memory. Value-string DRAM matching is intentionally weak.
///   2. Memory — DRAM package. lib_id must carry SDRAM/DDR/DRAM, OR value must
///      match a well-known DRAM MPN family (MT41/MT47/H5TQ...). A bare value
///      substring of "DDR3" is NOT enough (terminators often read "DDR3_TERM").
///   3. Controller — large ICs hosting the DDR controller.
///   4. Generic.
pub fn classify_footprint_role(fp: &Footprint) -> PadRole {
    let lib = fp.lib_id.to_uppercase();
    let value = fp.value.to_uppercase();
    let refdes = fp.reference.to_uppercase();

    // 1. Termination first: R*/RN* refdes always wins over weak value hints.
    if refdes.starts_with('R') {
        return PadRole::Termination;
    }

    // 2. Memory: DRAM package. Strong evidence only — lib_id keyword OR a
    //    recognized DRAM MPN prefix in value. Avoids matching "DDR3_TERM" etc.
    if lib.contains("SDRAM")
        || lib.contains("DDR3")
        || lib.contains("DDR4")
        || lib.contains("DRAM")
        || value.starts_with("MT41")
        || value.starts_with("MT47")
        || value.starts_with("H5TQ")
        || value.starts_with("K4B")
        || value.starts_with("IS43")
        || value.starts_with("AS4C")
    {
        return PadRole::Memory;
    }

    // 3. Controller: large ICs that typically host the DDR controller.
    //    Keyword match covers common families; the 100-pad fallback catches
    //    unlabeled BGAs/SoCs (scopefun's FPGA has 256 pads).
    if lib.contains("FPGA")
        || lib.contains("SOC")
        || lib.contains("MCU")
        || lib.contains("STM32")
        || lib.contains("CPU")
        || lib.contains("ZYNQ")
        || lib.contains("CYCLONE")
        || lib.contains("ARTIX")
        || lib.contains("KINTEX")
        || fp.pads.len() >= 100
    {
        return PadRole::Controller;
    }

    PadRole::Generic
}

/// Identify bus groups for coordinated routing: DDR3, SerDes, PCIe, USB3, SPI, etc.
/// Each group carries intra-group skew tolerance and optional inter-group skew constraints.
fn identify_bus_groups(board: &Board) -> Vec<BusGroup> {
    let mut groups = Vec::new();
    let mut assigned: HashSet<u32> = HashSet::new();
    let net_by_name: HashMap<String, u32> = board
        .nets
        .iter()
        .map(|n| (n.name.to_uppercase(), n.id))
        .collect();

    let mut add_group = |name: &str, priority: u32, intra_skew_mm: f64, patterns: &[&str]| {
        let mut ids = Vec::new();
        for pat in patterns {
            for (uname, &nid) in &net_by_name {
                if uname.contains(pat) && !assigned.contains(&nid) {
                    ids.push(nid);
                    assigned.insert(nid);
                }
            }
        }
        if !ids.is_empty() {
            groups.push(BusGroup {
                name: name.to_string(),
                net_ids: ids,
                _priority: priority,
                intra_skew_mm,
                inter_skew_mm: None,
                reference_group: None,
            });
        }
    };

    // --- DDR3 bus groups (tight intra-group skew) ---
    add_group("DQS0", 0, 0.25, &["DDR3_DQS0"]);
    add_group("DQS1", 0, 0.25, &["DDR3_DQS1"]);
    add_group(
        "DQ0-7",
        1,
        0.5,
        &[
            "DDR3_DQ0", "DDR3_DQ1", "DDR3_DQ2", "DDR3_DQ3", "DDR3_DQ4", "DDR3_DQ5", "DDR3_DQ6",
            "DDR3_DQ7",
        ],
    );
    add_group(
        "DQ8-15",
        1,
        0.5,
        &[
            "DDR3_DQ8",
            "DDR3_DQ9",
            "DDR3_DQ10",
            "DDR3_DQ11",
            "DDR3_DQ12",
            "DDR3_DQ13",
            "DDR3_DQ14",
            "DDR3_DQ15",
        ],
    );
    add_group("CLK", 2, 0.25, &["DDR3_CK"]);
    add_group(
        "ADDR",
        3,
        1.0,
        &[
            "DDR3_A0", "DDR3_A1", "DDR3_A2", "DDR3_A3", "DDR3_A4", "DDR3_A5", "DDR3_A6", "DDR3_A7",
            "DDR3_A8", "DDR3_A9", "DDR3_A10", "DDR3_A11", "DDR3_A12", "DDR3_A13", "DDR3_BA0",
            "DDR3_BA1", "DDR3_BA2",
        ],
    );
    add_group(
        "CMD",
        4,
        1.0,
        &["DDR3_CAS", "DDR3_RAS", "DDR3_WE", "DDR3_CKE", "DDR3_ODT"],
    );
    add_group("CTRL", 5, 1.0, &["DDR3_RESET"]);

    // --- SerDes / PCIe / SATA / USB3 lane groups ---
    // Intra-pair skew: ±0.127mm (5mil), tight tolerance
    let serdes_patterns: &[(&str, &[&str])] = &[
        ("PCIe_TX", &["PCIE_TX", "PCI_TX"]),
        ("PCIe_RX", &["PCIE_RX", "PCI_RX"]),
        ("SATA_TX", &["SATA_TX"]),
        ("SATA_RX", &["SATA_RX"]),
        ("USB3_TX", &["USB3_TX", "USB_SS_TX"]),
        ("USB3_RX", &["USB3_RX", "USB_SS_RX"]),
        ("LVDS_CH0", &["LVDS0", "LVDS_0"]),
        ("LVDS_CH1", &["LVDS1", "LVDS_1"]),
        ("LVDS_CH2", &["LVDS2", "LVDS_2"]),
        ("LVDS_CH3", &["LVDS3", "LVDS_3"]),
        ("MIPI_DSI", &["MIPI_DSI"]),
        ("MIPI_CSI", &["MIPI_CSI"]),
    ];
    for (name, patterns) in serdes_patterns {
        let pats: Vec<&str> = patterns.to_vec();
        add_group(name, 1, 0.25, &pats);
    }

    // --- SPI bus ---
    add_group(
        "SPI",
        5,
        2.0,
        &["SPI_CLK", "SPI_MOSI", "SPI_MISO", "SPI_CS"],
    );

    // --- Generic high-speed data bus ---
    add_group("FDATA", 4, 1.0, &["FDATA"]);
    add_group("FADDR", 4, 1.0, &["FADDR"]);

    // --- Inter-group skew relationships ---
    // DDR3 fly-by: ADDR/CMD groups should match CLK within ±2mm
    let clk_idx = groups.iter().position(|g| g.name == "CLK");
    let addr_idx = groups.iter().position(|g| g.name == "ADDR");
    let cmd_idx = groups.iter().position(|g| g.name == "CMD");
    if let (Some(ci), Some(ai)) = (clk_idx, addr_idx) {
        groups[ai].inter_skew_mm = Some(2.0);
        groups[ai].reference_group = Some(ci);
    }
    if let (Some(ci), Some(cmdi)) = (clk_idx, cmd_idx) {
        groups[cmdi].inter_skew_mm = Some(2.0);
        groups[cmdi].reference_group = Some(ci);
    }

    // SerDes: TX and RX pairs should match within ±1mm
    let tx_idx = groups
        .iter()
        .position(|g| g.name == "PCIe_TX" || g.name == "SATA_TX" || g.name == "USB3_TX");
    let rx_idx = groups
        .iter()
        .position(|g| g.name == "PCIe_RX" || g.name == "SATA_RX" || g.name == "USB3_RX");
    if let (Some(txi), Some(rxi)) = (tx_idx, rx_idx) {
        groups[rxi].inter_skew_mm = Some(1.0);
        groups[rxi].reference_group = Some(txi);
    }

    groups
}

/// L5: Build impedance-controlled trace width map from SI directives and layer stackup.
/// Returns net_id → trace_width_mm for nets that have impedance requirements.
fn build_impedance_width_map(
    board: &Board,
    directives: &LayoutDirectives,
    layer_config: &crate::layer_config::BoardLayerConfig,
) -> HashMap<u32, f64> {
    let mut widths = HashMap::new();
    if directives.signal_integrity.is_empty() {
        return widths;
    }

    // Use the first signal layer for impedance calculation (most common case)
    let sig_layers = layer_config.signal_layer_indices();
    let default_layer = sig_layers.first().copied().unwrap_or(0);

    for si in &directives.signal_integrity {
        if si.target_impedance_ohm <= 0.0 {
            continue;
        }
        if let Some(net) = board.nets.iter().find(|n| n.name == si.net_name) {
            let w = layer_config.impedance_width(si.target_impedance_ohm, default_layer);
            if w > 0.0 {
                widths.insert(net.id, w.max(si.min_trace_width_mm));
            }
        }
    }

    // Auto-detect high-speed nets and assign 50-ohm default impedance
    let hs_nets = identify_high_speed_nets(board);
    for &nid in &hs_nets {
        if widths.contains_key(&nid) {
            continue;
        }
        let w = layer_config.impedance_width(50.0, default_layer);
        if w > 0.0 {
            widths.insert(nid, w);
        }
    }

    widths
}

// ---------------------------------------------------------------------------
// Global Routing Phase (tile-based coarse routing)
// ---------------------------------------------------------------------------

/// Build coarse tile grid from detailed routing grid.
fn build_global_tile_grid(
    grid: &RoutingGrid,
    board: &Board,
) -> (Vec<GlobalTile>, usize, usize, f64, f64) {
    let tile_size = GLOBAL_TILE_SIZE;
    let sig_layers = grid.layer_config.signal_layer_indices();

    // Compute tile grid bounds from detailed grid
    let world_min_x = grid.origin_x;
    let world_min_y = grid.origin_y;
    let world_max_x = grid.origin_x + grid.cols as f64 * grid.grid_res;
    let world_max_y = grid.origin_y + grid.rows as f64 * grid.grid_res;

    let tile_cols = ((world_max_x - world_min_x) / tile_size).ceil() as usize;
    let tile_rows = ((world_max_y - world_min_y) / tile_size).ceil() as usize;
    let num_tiles = tile_cols * tile_rows;

    // Count blocked cells per tile per layer
    let mut blocked_count: Vec<u16> = vec![0; num_tiles];
    let mut total_count: Vec<u16> = vec![0; num_tiles];

    for layer in &sig_layers {
        for r in 0..grid.rows {
            for c in 0..grid.cols {
                let wx = grid.origin_x + c as f64 * grid.grid_res;
                let wy = grid.origin_y + r as f64 * grid.grid_res;
                let tc = (((wx - world_min_x) / tile_size) as usize).min(tile_cols - 1);
                let tr = (((wy - world_min_y) / tile_size) as usize).min(tile_rows - 1);
                let idx = tc * tile_rows + tr;
                total_count[idx] += 1;
                if let Cell::Blocked = grid.get(*layer, c, r) {
                    blocked_count[idx] += 1;
                }
            }
        }
    }

    // Mark tiles blocked by component bodies (using footprint bboxes)
    let mut body_blocked = vec![false; num_tiles];
    for fp in &board.footprints {
        let (fx, fy, _) = fp.position;
        let (bw, bh) = crate::layout_engine::infer_body_size(&fp.lib_id, fp.pads.len());
        let x0 = fx - bw / 2.0;
        let y0 = fy - bh / 2.0;
        let x1 = fx + bw / 2.0;
        let y1 = fy + bh / 2.0;
        let tc0 = (((x0 - world_min_x) / tile_size).floor() as usize).min(tile_cols - 1);
        let tc1 = (((x1 - world_min_x) / tile_size).ceil() as usize).min(tile_cols - 1);
        let tr0 = (((y0 - world_min_y) / tile_size).floor() as usize).min(tile_rows - 1);
        let tr1 = (((y1 - world_min_y) / tile_size).ceil() as usize).min(tile_rows - 1);
        for tc in tc0..=tc1 {
            for tr in tr0..=tr1 {
                body_blocked[tc * tile_rows + tr] = true;
            }
        }
    }

    // Build tiles
    let mut tiles: Vec<GlobalTile> = Vec::with_capacity(num_tiles);
    for i in 0..num_tiles {
        let total = total_count[i].max(1);
        let blocked_ratio = blocked_count[i] as f64 / total as f64;
        let blocked = blocked_ratio > 0.8 || body_blocked[i];
        let capacity = if blocked {
            0
        } else {
            (GLOBAL_BASE_CAPACITY as f64 * sig_layers.len() as f64 * (1.0 - blocked_ratio * 0.5))
                .max(1.0) as u16
        };
        tiles.push(GlobalTile {
            capacity,
            usage: 0,
            history_cost: 0.0,
            blocked,
        });
    }

    (tiles, tile_cols, tile_rows, world_min_x, world_min_y)
}

/// A* on the tile graph to find coarse path between two tile positions.
fn global_route_tile_astar(
    tiles: &[GlobalTile],
    tile_cols: usize,
    tile_rows: usize,
    start_tc: usize,
    start_tr: usize,
    goal_tc: usize,
    goal_tr: usize,
) -> Option<Vec<usize>> {
    if start_tc == goal_tc && start_tr == goal_tr {
        return Some(vec![start_tc * tile_rows + start_tr]);
    }

    let num = tile_cols * tile_rows;
    let mut g_score: Vec<f64> = vec![f64::MAX; num];
    let mut came_from: Vec<usize> = vec![usize::MAX; num];
    let mut closed: Vec<bool> = vec![false; num];

    let start_idx = start_tc * tile_rows + start_tr;
    g_score[start_idx] = 0.0;

    // BinaryHeap is max-heap, so we use Reverse to get min-heap
    let mut open: BinaryHeap<std::cmp::Reverse<(i64, usize)>> = BinaryHeap::new();
    let h0 = (goal_tc as i64 - start_tc as i64).abs() + (goal_tr as i64 - start_tr as i64).abs();
    open.push(std::cmp::Reverse((h0, start_idx)));

    let dirs: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

    while let Some(std::cmp::Reverse((_, idx))) = open.pop() {
        let tc = idx / tile_rows;
        let tr = idx % tile_rows;
        if tc == goal_tc && tr == goal_tr {
            // Reconstruct path
            let mut path = Vec::new();
            let mut cur = idx;
            loop {
                path.push(cur);
                if came_from[cur] == usize::MAX {
                    break;
                }
                cur = came_from[cur];
            }
            path.reverse();
            return Some(path);
        }
        if closed[idx] {
            continue;
        }
        closed[idx] = true;

        for (dc, dr) in &dirs {
            let nc = tc as i32 + dc;
            let nr = tr as i32 + dr;
            if nc < 0 || nr < 0 || nc >= tile_cols as i32 || nr >= tile_rows as i32 {
                continue;
            }
            let nidx = nc as usize * tile_rows + nr as usize;
            if closed[nidx] {
                continue;
            }
            let tile = &tiles[nidx];
            if tile.blocked || tile.capacity == 0 {
                continue;
            }

            let mut cost = 1.0;
            // Overflow penalty
            if tile.usage > tile.capacity {
                cost += (tile.usage - tile.capacity) as f64 * 2.0;
            }
            // History cost
            cost += tile.history_cost;

            let new_g = g_score[idx] + cost;
            if new_g < g_score[nidx] {
                g_score[nidx] = new_g;
                came_from[nidx] = idx;
                let h = (goal_tc as i64 - nc as i64).abs() + (goal_tr as i64 - nr as i64).abs();
                let f = (new_g * 1000.0) as i64 + h;
                open.push(std::cmp::Reverse((f, nidx)));
            }
        }
    }
    None
}

/// Compute global route for a single net. Returns set of tile indices.
fn global_route_net(
    tiles: &[GlobalTile],
    tile_cols: usize,
    tile_rows: usize,
    pads: &[(f64, f64)],
    origin_x: f64,
    origin_y: f64,
    tile_size: f64,
) -> HashSet<usize> {
    if pads.len() < 2 {
        return HashSet::new();
    }

    // Map pads to tile coordinates
    let pad_tiles: Vec<(usize, usize)> = pads
        .iter()
        .map(|&(px, py)| {
            let tc = (((px - origin_x) / tile_size).floor() as usize).min(tile_cols - 1);
            let tr = (((py - origin_y) / tile_size).floor() as usize).min(tile_rows - 1);
            (tc, tr)
        })
        .collect();

    let mut guide_tiles: HashSet<usize> = HashSet::new();

    // Add all pad tiles
    for &(tc, tr) in &pad_tiles {
        guide_tiles.insert(tc * tile_rows + tr);
    }

    if pads.len() == 2 {
        // Direct tile path for 2-pin net
        let (tc0, tr0) = pad_tiles[0];
        let (tc1, tr1) = pad_tiles[1];
        if let Some(path) = global_route_tile_astar(tiles, tile_cols, tile_rows, tc0, tr0, tc1, tr1)
        {
            for &idx in &path {
                guide_tiles.insert(idx);
            }
        }
    } else {
        // Multi-pin: connect each pin to nearest unvisited pin (greedy spanning tree)
        let mut visited = vec![false; pad_tiles.len()];
        visited[0] = true;
        let mut visited_count = 1;
        while visited_count < pad_tiles.len() {
            let mut best_dist = i64::MAX;
            let mut best_i = 0;
            let mut best_j = 0;
            for i in 0..pad_tiles.len() {
                if !visited[i] {
                    continue;
                }
                for j in 0..pad_tiles.len() {
                    if visited[j] {
                        continue;
                    }
                    let d = (pad_tiles[i].0 as i64 - pad_tiles[j].0 as i64).abs()
                        + (pad_tiles[i].1 as i64 - pad_tiles[j].1 as i64).abs();
                    if d < best_dist {
                        best_dist = d;
                        best_i = i;
                        best_j = j;
                    }
                }
            }
            if best_j == 0 && best_i == 0 {
                break;
            } // safety
            visited[best_j] = true;
            visited_count += 1;
            let (tc0, tr0) = pad_tiles[best_i];
            let (tc1, tr1) = pad_tiles[best_j];
            if let Some(path) =
                global_route_tile_astar(tiles, tile_cols, tile_rows, tc0, tr0, tc1, tr1)
            {
                for &idx in &path {
                    guide_tiles.insert(idx);
                }
            }
        }
    }
    guide_tiles
}

/// Run global routing phase: build tile grid, route all nets, multi-pass PathFinder.
fn global_route_phase(
    grid: &RoutingGrid,
    board: &Board,
    sorted_nets: &[(u32, Vec<(f64, f64)>)],
    mps_rounds: &[Vec<usize>],
) -> GlobalRouteGuide {
    let t0 = std::time::Instant::now();

    let (mut tiles, tile_cols, tile_rows, origin_x, origin_y) = build_global_tile_grid(grid, board);
    let mut net_guides: HashMap<u32, HashSet<usize>> = HashMap::new();

    for pass in 0..GLOBAL_MAX_PASSES {
        // Reset usage for re-routing
        for tile in tiles.iter_mut() {
            tile.usage = 0;
        }
        net_guides.clear();

        // Route nets in MPS round order
        for round in mps_rounds {
            for &net_idx in round {
                let (net_id, pads) = &sorted_nets[net_idx];
                let guide = global_route_net(
                    &tiles,
                    tile_cols,
                    tile_rows,
                    pads,
                    origin_x,
                    origin_y,
                    GLOBAL_TILE_SIZE,
                );
                // Update tile usage
                for &idx in &guide {
                    if idx < tiles.len() {
                        tiles[idx].usage += 1;
                    }
                }
                net_guides.insert(*net_id, guide);
            }
        }

        // Update history costs for overflow tiles
        let mut overflow_count = 0;
        for tile in tiles.iter_mut() {
            if tile.usage > tile.capacity && tile.capacity > 0 {
                tile.history_cost += GLOBAL_HISTORY_FACTOR;
                overflow_count += 1;
            }
        }

        if overflow_count == 0 {
            break;
        }
        if pass == 0 {
            eprintln!(
                "[router] Global routing pass {}: {} overflow tiles",
                pass + 1,
                overflow_count
            );
        }
    }

    let guided = net_guides.len();
    let elapsed = t0.elapsed().as_millis();
    eprintln!(
        "[router] Global routing: {}x{} tiles, {} nets guided ({:.0}ms)",
        tile_cols, tile_rows, guided, elapsed
    );

    GlobalRouteGuide {
        net_guides,
        tile_cols,
        tile_rows,
        tile_size: GLOBAL_TILE_SIZE,
        origin_x,
        origin_y,
    }
}

/// Get effective trace width: impedance-controlled width if available, else directive default.
fn effective_trace_width(
    net_id: u32,
    net_name: &str,
    directives: &LayoutDirectives,
    impedance_widths: &HashMap<u32, f64>,
) -> f64 {
    // P1-8: per-net CLI override (uppercase key) beats everything
    if let Some(&w) = directives.net_width_overrides.get(&net_name.to_uppercase()) {
        return w;
    }
    // P1-8: global CLI override beats net classes and SI directives
    if let Some(w) = directives.trace_width_override {
        return w;
    }
    let _ = net_id;
    impedance_widths
        .get(&net_id)
        .copied()
        .unwrap_or_else(|| directives.trace_width_for(net_name))
}

/// Identify differential pair nets by naming convention.
/// Matches patterns like: USB_DP/DM, D+/D-, TX_P/TX_N, MIPI_P/MIPI_N, SDA_P/SDA_N, etc.
fn identify_diff_pairs(board: &Board) -> Vec<(u32, u32, f64)> {
    let mut pairs: Vec<(u32, u32, f64)> = Vec::new();
    let mut paired: HashSet<u32> = HashSet::new();

    let net_names: Vec<(u32, &str)> = board.nets.iter().map(|n| (n.id, n.name.as_str())).collect();

    // Pair suffixes to try (positive, negative)
    let suffix_pairs = [
        ("_P", "_N"),
        ("_DP", "_DM"),
        ("+", "-"),
        ("_POS", "_NEG"),
        ("_TRUE", "_COMP"),
    ];

    for (i, (id1, name1)) in net_names.iter().enumerate() {
        if paired.contains(id1) {
            continue;
        }
        for (pos_sfx, neg_sfx) in &suffix_pairs {
            if !name1.to_uppercase().ends_with(pos_sfx) {
                continue;
            }
            let base = &name1[..name1.len() - pos_sfx.len()];
            let neg_name = format!("{}{}", base, neg_sfx);

            for (id2, name2) in &net_names {
                if paired.contains(id2) {
                    continue;
                }
                if name2.eq_ignore_ascii_case(&neg_name) {
                    pairs.push((*id1, *id2, 0.15)); // default 0.15mm pair spacing
                    paired.insert(*id1);
                    paired.insert(*id2);
                    break;
                }
            }
            if paired.contains(id1) {
                break;
            }
        }

        // Also try D+/D- pattern
        if !paired.contains(id1) {
            for (id2, name2) in net_names.iter().skip(i + 1) {
                if paired.contains(id2) {
                    continue;
                }
                let n1 = name1.to_uppercase();
                let n2 = name2.to_uppercase();
                let is_pair = (n1.ends_with("D+") && n2.ends_with("D-"))
                    || (n1.ends_with("D-") && n2.ends_with("D+"));
                if is_pair {
                    pairs.push((*id1, *id2, 0.15));
                    paired.insert(*id1);
                    paired.insert(*id2);
                    break;
                }
            }
        }
    }

    pairs
}

/// Route a differential pair using parallel A* routing.
/// The positive net is routed first, then the negative net follows with matched spacing.
fn route_diff_pair(
    grid: &mut RoutingGrid,
    net_p: u32,
    net_n: u32,
    pads_p: &[(f64, f64)],
    pads_n: &[(f64, f64)],
    trace_width: f64,
    pair_spacing: f64,
    board: &mut Board,
    topo_ctx: &TopologyContext,
) -> bool {
    if pads_p.len() < 2 || pads_n.len() < 2 {
        return false;
    }

    // High-speed diff pairs use tighter spacing
    let name_p = board
        .nets
        .iter()
        .find(|n| n.id == net_p)
        .map(|n| n.name.to_uppercase())
        .unwrap_or_default();
    let is_high_speed = name_p.contains("DDR")
        || name_p.contains("LVDS")
        || name_p.contains("MIPI")
        || name_p.contains("USB")
        || name_p.contains("DQS");
    let effective_spacing = if is_high_speed {
        pair_spacing * 0.5
    } else {
        pair_spacing
    };

    // Route positive net first with A*
    grid.unmark_net(net_p);
    let p_routed = route_single_net(grid, net_p, pads_p, trace_width, board, &[], topo_ctx);
    if !p_routed {
        return false;
    }

    // Mark positive net traces on grid
    let _seg_count = board.segments.len();
    for seg in board.segments.iter().rev() {
        if seg.net == net_p {
            mark_segment_on_grid(grid, seg, net_p, effective_spacing);
        }
    }

    // Route negative net — A* will naturally follow alongside positive
    grid.unmark_net(net_n);
    let n_routed = route_single_net(grid, net_n, pads_n, trace_width, board, &[], topo_ctx);
    if !n_routed {
        return false;
    }

    // Length matching: if traces differ by more than 0.5mm, add serpentine
    let p_len: f64 = board
        .segments
        .iter()
        .filter(|s| s.net == net_p)
        .map(segment_length)
        .sum();
    let n_len: f64 = board
        .segments
        .iter()
        .filter(|s| s.net == net_n)
        .map(segment_length)
        .sum();

    let diff = (p_len - n_len).abs();
    // I7: propagation delay — FR4 microstrip ~150mm/ns, 0.5mm ≈ 3.3ps
    // Default threshold 0.5mm; tighter for high-speed nets (configurable via net_classes)
    let length_match_threshold = 0.5; // mm
    if diff > length_match_threshold {
        let shorter_net = if p_len < n_len { net_p } else { net_n };
        add_length_matching(board, shorter_net, diff, trace_width);
    }

    true
}

/// Mark a segment's cells on the grid with clearance expansion.
fn mark_segment_on_grid(grid: &mut RoutingGrid, seg: &Segment, net_id: u32, clearance: f64) {
    let layer = grid.layer_config.layer_index(&seg.layer).unwrap_or(0);
    let expansion = (clearance / grid.grid_res).ceil() as i32;

    let (sx, sy) = grid.world_to_grid(seg.start.0, seg.start.1);
    let (ex, ey) = grid.world_to_grid(seg.end.0, seg.end.1);

    // Bresenham-style integer stepping along the segment
    let dx = (ex as i32) - (sx as i32);
    let dy = (ey as i32) - (sy as i32);
    let steps = dx.abs().max(dy.abs()).max(1) as usize;
    let step_x = dx as f64 / steps as f64;
    let step_y = dy as f64 / steps as f64;

    let cols = grid.cols as i32;
    let rows = grid.rows as i32;

    for i in 0..=steps {
        let cx = sx as i32 + (step_x * i as f64).round() as i32;
        let cy = sy as i32 + (step_y * i as f64).round() as i32;

        // Update congestion
        if cx >= 0 && cx < cols && cy >= 0 && cy < rows {
            grid.inc_congestion(cx as usize, cy as usize);
        }

        // Mark clearance zone
        let x_lo = (cx - expansion).max(0) as usize;
        let x_hi = (cx + expansion + 1).min(cols) as usize;
        let y_lo = (cy - expansion).max(0) as usize;
        let y_hi = (cy + expansion + 1).min(rows) as usize;

        for uy in y_lo..y_hi {
            let row_offset = uy * grid.cols;
            let per_layer = grid.rows * grid.cols;
            for ux in x_lo..x_hi {
                let idx = row_offset + ux;
                let flat = layer * per_layer + idx;
                if grid.layer_data[flat] == Cell::Free {
                    grid.layer_data[flat] = Cell::Trace(net_id);
                    let n = net_id as usize;
                    if n < grid.net_cells.len() {
                        grid.net_cells[n].push((layer, idx));
                    }
                }
            }
        }
    }
}

/// Calculate the length of a segment.
fn segment_length(seg: &Segment) -> f64 {
    let dx = seg.end.0 - seg.start.0;
    let dy = seg.end.1 - seg.start.1;
    (dx * dx + dy * dy).sqrt()
}

/// Add serpentine (accordion) length matching to a net's longest straight segment.
fn add_length_matching(board: &mut Board, net_id: u32, target_delta: f64, trace_width: f64) {
    // Find the longest straight segment on this net
    let mut best_idx = 0;
    let mut best_len = 0.0_f64;
    for (i, seg) in board.segments.iter().enumerate() {
        if seg.net != net_id {
            continue;
        }
        let len = segment_length(seg);
        if len > best_len {
            best_len = len;
            best_idx = i;
        }
    }

    if best_len < 1.0 {
        return;
    } // Too short for serpentine

    let seg = &board.segments[best_idx];
    let (sx, sy) = seg.start;
    let (ex, ey) = seg.end;

    // Direction perpendicular to the segment
    let dx = ex - sx;
    let dy = ey - sy;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 0.01 {
        return;
    }
    let perp_x = -dy / len;
    let perp_y = dx / len;

    // Serpentine parameters
    let amplitude = 0.5; // 0.5mm bump amplitude
    let period = 1.0; // 1mm per bump
    let num_bumps = ((target_delta / (2.0 * amplitude)) as usize).clamp(1, 10);
    let layer = seg.layer.clone();

    // Split the segment: keep start portion, add serpentine, keep end portion
    let bump_start = 0.3; // Start bumps 30% along the segment
    let bump_end = (0.3 + num_bumps as f64 * period / best_len).min(0.7);

    let split_s = (sx + dx * bump_start, sy + dy * bump_start);
    let split_e = (sx + dx * bump_end, sy + dy * bump_end);

    // Remove the original segment and replace with serpentine
    board.segments.remove(best_idx);

    // First part: straight to bump start
    board.segments.push(Segment {
        start: (sx, sy),
        end: split_s,
        width: trace_width,
        layer: layer.clone(),
        net: net_id,
    });

    // Serpentine bumps
    let bump_len = bump_end - bump_start;
    for i in 0..num_bumps {
        let t0 = bump_start + bump_len * (i as f64 / num_bumps as f64);
        let t1 = bump_start + bump_len * ((i as f64 + 0.5) / num_bumps as f64);
        let t2 = bump_start + bump_len * ((i as f64 + 1.0) / num_bumps as f64);

        let p0 = (sx + dx * t0, sy + dy * t0);
        let p1 = (
            sx + dx * t1 + perp_x * amplitude,
            sy + dy * t1 + perp_y * amplitude,
        );
        let p2 = (sx + dx * t2, sy + dy * t2);

        board.segments.push(Segment {
            start: p0,
            end: p1,
            width: trace_width,
            layer: layer.clone(),
            net: net_id,
        });
        board.segments.push(Segment {
            start: p1,
            end: p2,
            width: trace_width,
            layer: layer.clone(),
            net: net_id,
        });
    }

    // Last part: straight from bump end to destination
    board.segments.push(Segment {
        start: split_e,
        end: (ex, ey),
        width: trace_width,
        layer: layer.clone(),
        net: net_id,
    });
}

// ---------------------------------------------------------------------------
// Post-routing optimization
// ---------------------------------------------------------------------------

/// Optimize routes: straighten traces + reduce unnecessary vias + bus length matching.
fn optimize_routes(board: &mut Board) {
    straighten_traces(board);
    reduce_unnecessary_vias(board);
    serdes_length_match(board);
}

/// Merge collinear segments of the same net and layer into single segments.
/// If three consecutive segments share a layer+net and the first and last are
/// collinear with the middle one, they can be merged.
fn straighten_traces(board: &mut Board) {
    // Group segment indices by (net, layer)
    let mut groups: HashMap<(u32, String), Vec<usize>> = HashMap::new();
    for (i, seg) in board.segments.iter().enumerate() {
        groups
            .entry((seg.net, seg.layer.clone()))
            .or_default()
            .push(i);
    }

    // Collect pad obstacles once (x, y, half_diagonal, net).
    // net-0 (NC/unconnected) pads are obstacles for EVERY net — copper is copper.
    let mut pad_obstacles: Vec<(f64, f64, f64, u32)> = Vec::new();
    for fp in &board.footprints {
        for pad in &fp.pads {
            let net_id = pad.net.unwrap_or(0);
            let (fx, fy, _) = fp.position;
            let (px, py) = fp.pad_rotated_offset(pad);
            let half = (pad.size.0.hypot(pad.size.1)) / 2.0;
            pad_obstacles.push((fx + px, fy + py, half, net_id));
        }
    }
    let pad_obstacles = &pad_obstacles;

    let mut to_remove: HashSet<usize> = HashSet::new();
    let mut to_add: Vec<Segment> = Vec::new();
    let mut bends_before: usize = 0;
    let mut bends_after: usize = 0;

    // P0-10: HashMap group iteration order randomized straightening output
    // order — iterate keys in sorted order
    let mut group_keys: Vec<(u32, String)> = groups.keys().cloned().collect();
    group_keys.sort();
    for key in group_keys {
        let indices = groups.get_mut(&key).unwrap();
        // Sort by spatial connectivity: build a chain by matching end→start
        if indices.len() < 3 {
            continue;
        }
        let segs: Vec<&Segment> = indices.iter().map(|&i| &board.segments[i]).collect();

        // Build adjacency: find segments that connect end→start
        let mut chain: Vec<usize> = vec![0];
        let mut used = vec![false; segs.len()];
        used[0] = true;

        // Greedy chain building
        loop {
            let last_end = segs[*chain.last().unwrap()].end;
            let mut found = false;
            for j in 0..segs.len() {
                if used[j] {
                    continue;
                }
                let d = (segs[j].start.0 - last_end.0).hypot(segs[j].start.1 - last_end.1);
                if d < GRID_RES {
                    chain.push(j);
                    used[j] = true;
                    found = true;
                    break;
                }
            }
            if !found {
                break;
            }
        }

        // Waypoints of the chain
        let mut pts: Vec<(f64, f64)> = vec![segs[chain[0]].start];
        for &ci in &chain {
            pts.push(segs[ci].end);
        }
        bends_before += pts.len().saturating_sub(2);

        // Conflict check for a candidate direct segment A→C on this layer/net:
        // other-net segments, vias, and pads must keep their clearance.
        let net = segs[chain[0]].net;
        let layer = segs[chain[0]].layer.clone();
        let width = segs[chain[0]].width;
        let direct_clear = |a: (f64, f64), c: (f64, f64)| -> bool {
            for seg in board.segments.iter() {
                if seg.net == net || seg.layer != layer {
                    continue;
                }
                let min_dist = width / 2.0 + seg.width / 2.0 + 0.12;
                if crate::drc::segment_to_segment_dist(a, c, seg.start, seg.end) < min_dist {
                    return false;
                }
            }
            for via in board.vias.iter() {
                if via.net == net {
                    continue;
                }
                let via_layers = ["F.Cu", "B.Cu"];
                if !via_layers.contains(&layer.as_str()) {
                    continue;
                }
                let min_dist = width / 2.0 + via.size / 2.0 + 0.12;
                if crate::drc::point_to_segment_dist((via.at.0, via.at.1), a, c) < min_dist {
                    return false;
                }
            }
            for &(px, py, half, pnet) in pad_obstacles.iter() {
                if pnet == net {
                    continue;
                }
                let min_dist = width / 2.0 + half + 0.12;
                if crate::drc::point_to_segment_dist((px, py), a, c) < min_dist {
                    return false;
                }
            }
            true
        };

        // Greedy string-pulling: from waypoint i, jump as far as a clear direct
        // segment allows. This removes staircase jaggies that collinear merging
        // cannot (bends only survive where an obstacle forces them).
        let mut new_pts: Vec<(f64, f64)> = vec![pts[0]];
        let mut i = 0;
        while i + 1 < pts.len() {
            let mut jumped = false;
            for j in ((i + 2)..pts.len()).rev() {
                if direct_clear(pts[i], pts[j]) {
                    new_pts.push(pts[j]);
                    i = j;
                    jumped = true;
                    break;
                }
            }
            if !jumped {
                new_pts.push(pts[i + 1]);
                i += 1;
            }
        }
        bends_after += new_pts.len().saturating_sub(2);

        if new_pts.len() < pts.len() {
            for &ci in &chain {
                to_remove.insert(indices[ci]);
            }
            for w in new_pts.windows(2) {
                if (w[0].0 - w[1].0).hypot(w[0].1 - w[1].1) < 1e-9 {
                    continue;
                }
                to_add.push(Segment {
                    start: w[0],
                    end: w[1],
                    width,
                    layer: layer.clone(),
                    net,
                });
            }
        }
    }

    if !to_remove.is_empty() {
        eprintln!(
            "[router] Straightening: removing {} segments, adding {} (bends {} → {})",
            to_remove.len(),
            to_add.len(),
            bends_before,
            bends_after
        );
        // Remove merged segments (iterate in reverse to keep indices valid)
        let mut remove_list: Vec<usize> = to_remove.into_iter().collect();
        remove_list.sort_unstable_by(|a, b| b.cmp(a));
        for idx in remove_list {
            board.segments.remove(idx);
        }
        board.segments.extend(to_add);
    }
}

/// Remove vias that connect segments on the same layer (unnecessary layer change).
fn reduce_unnecessary_vias(board: &mut Board) {
    if board.vias.is_empty() {
        return;
    }

    // For each via, check if it only connects segments on one layer
    let via_positions: Vec<(f64, f64, u32)> =
        board.vias.iter().map(|v| (v.at.0, v.at.1, v.net)).collect();

    let mut to_remove: Vec<usize> = Vec::new();

    for (vi, &(vx, vy, vnet)) in via_positions.iter().enumerate() {
        // Collect unique layers of segments connected to this via
        let mut connected_layers: HashSet<String> = HashSet::new();
        let threshold = GRID_RES;

        for seg in &board.segments {
            if seg.net != vnet {
                continue;
            }
            let at_start = (seg.start.0 - vx).hypot(seg.start.1 - vy) < threshold;
            let at_end = (seg.end.0 - vx).hypot(seg.end.1 - vy) < threshold;
            if !at_start && !at_end {
                continue;
            }

            connected_layers.insert(seg.layer.clone());
        }

        // If all connected segments are on one layer, the via is unnecessary
        if connected_layers.len() <= 1 {
            to_remove.push(vi);
        }
    }

    if !to_remove.is_empty() {
        eprintln!("[router] Removing {} unnecessary vias", to_remove.len());
        for idx in to_remove.into_iter().rev() {
            board.vias.remove(idx);
        }
    }
}

// ---------------------------------------------------------------------------
// MPS (Maximum Planar Subset) Network Ordering
// ---------------------------------------------------------------------------

/// Compute bounding box of pad positions: (min_x, min_y, max_x, max_y).
fn net_bbox(pads: &[(f64, f64)]) -> (f64, f64, f64, f64) {
    let mut min_x = f64::MAX;
    let mut min_y = f64::MAX;
    let mut max_x = f64::MIN;
    let mut max_y = f64::MIN;
    for &(px, py) in pads {
        min_x = min_x.min(px);
        min_y = min_y.min(py);
        max_x = max_x.max(px);
        max_y = max_y.max(py);
    }
    (min_x, min_y, max_x, max_y)
}

/// L4: SerDes / bus group length matching with intra-group and inter-group skew control.
///
/// Phase 1 — Intra-group: align all nets within each group to the longest net.
/// Phase 2 — Inter-group: align groups to their reference groups using skew tolerance.
fn serdes_length_match(board: &mut Board) {
    // Escape hatch: serpentine bumps interact badly with straighten/commit
    // on dense boards (fragmented, disconnected bump chains). Set
    // KDESIGN_NO_SERDES=1 to keep raw routed connectivity.
    if std::env::var("KDESIGN_NO_SERDES").is_ok() {
        return;
    }
    let groups = identify_bus_groups(board);
    if groups.is_empty() {
        return;
    }

    let mut total_matched = 0;
    let mut group_target_lengths: Vec<Option<f64>> = vec![None; groups.len()];

    // --- Phase 1: Intra-group matching ---
    for (gi, group) in groups.iter().enumerate() {
        if group.net_ids.len() < 2 {
            continue;
        }

        let lengths: Vec<(u32, f64)> = group
            .net_ids
            .iter()
            .map(|&nid| {
                let len: f64 = board
                    .segments
                    .iter()
                    .filter(|s| s.net == nid)
                    .map(segment_length)
                    .sum();
                (nid, len)
            })
            .collect();

        let routed: Vec<(u32, f64)> = lengths.into_iter().filter(|(_, l)| *l > 0.0).collect();
        if routed.len() < 2 {
            continue;
        }

        let target_len = routed.iter().map(|(_, l)| *l).fold(0.0_f64, f64::max);
        group_target_lengths[gi] = Some(target_len);

        let threshold = group.intra_skew_mm;
        for &(nid, len) in &routed {
            let delta = target_len - len;
            if delta > threshold {
                let trace_width = board
                    .segments
                    .iter()
                    .find(|s| s.net == nid)
                    .map(|s| s.width)
                    .unwrap_or(0.2);
                add_length_matching(board, nid, delta, trace_width);
                total_matched += 1;
            }
        }
    }

    // --- Phase 2: Inter-group skew matching ---
    // If group B references group A, match B's target length to A's within the skew budget.
    let mut inter_matched = 0;
    for (gi, group) in groups.iter().enumerate() {
        let (ref_idx, max_skew) = match (group.reference_group, group.inter_skew_mm) {
            (Some(ri), Some(skew)) => (ri, skew),
            _ => continue,
        };

        let my_target = match group_target_lengths[gi] {
            Some(t) => t,
            None => continue,
        };
        let ref_target = match group_target_lengths[ref_idx] {
            Some(t) => t,
            None => continue,
        };

        let delta = my_target - ref_target;
        // If this group is shorter than reference + skew budget, extend it
        if delta < -max_skew {
            let needed = ref_target - my_target + max_skew * 0.5; // match to midpoint of skew window
            if needed > 0.5 {
                // Extend the shortest net in this group
                let shortest = group
                    .net_ids
                    .iter()
                    .filter_map(|&nid| {
                        let len: f64 = board
                            .segments
                            .iter()
                            .filter(|s| s.net == nid)
                            .map(segment_length)
                            .sum();
                        if len > 0.0 {
                            Some((nid, len))
                        } else {
                            None
                        }
                    })
                    .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
                if let Some((nid, _len)) = shortest {
                    let tw = board
                        .segments
                        .iter()
                        .find(|s| s.net == nid)
                        .map(|s| s.width)
                        .unwrap_or(0.2);
                    add_length_matching(board, nid, needed, tw);
                    inter_matched += 1;
                }
            }
        }
        // If this group is longer than reference + skew budget, extend the reference group instead
        else if delta > max_skew {
            let needed = my_target - ref_target - max_skew * 0.5;
            if needed > 0.5 {
                let ref_group = &groups[ref_idx];
                let shortest = ref_group
                    .net_ids
                    .iter()
                    .filter_map(|&nid| {
                        let len: f64 = board
                            .segments
                            .iter()
                            .filter(|s| s.net == nid)
                            .map(segment_length)
                            .sum();
                        if len > 0.0 {
                            Some((nid, len))
                        } else {
                            None
                        }
                    })
                    .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
                if let Some((nid, _)) = shortest {
                    let tw = board
                        .segments
                        .iter()
                        .find(|s| s.net == nid)
                        .map(|s| s.width)
                        .unwrap_or(0.2);
                    add_length_matching(board, nid, needed, tw);
                    inter_matched += 1;
                }
            }
        }
    }

    if total_matched > 0 || inter_matched > 0 {
        eprintln!("[router] L4 SerDes length matching: {} intra-group + {} inter-group nets adjusted in {} groups",
            total_matched, inter_matched, groups.len());
    }
}

/// Compute overlap ratio between two bounding boxes.
/// Returns intersection_area / min(area_a, area_b). 0.0 if no overlap.
fn bbox_overlap_ratio(a: (f64, f64, f64, f64), b: (f64, f64, f64, f64)) -> f64 {
    let ix_min = a.0.max(b.0);
    let iy_min = a.1.max(b.1);
    let ix_max = a.2.min(b.2);
    let iy_max = a.3.min(b.3);
    if ix_max <= ix_min || iy_max <= iy_min {
        return 0.0;
    }
    let inter = (ix_max - ix_min) * (iy_max - iy_min);
    let area_a = (a.2 - a.0) * (a.3 - a.1);
    let area_b = (b.2 - b.0) * (b.3 - b.1);
    let min_area = area_a.min(area_b);
    if min_area < 1e-9 {
        return 0.0;
    }
    inter / min_area
}

/// Build conflict graph: returns adjacency list mapping net_id -> [(conflicting_net_id, overlap_ratio)].
fn build_conflict_graph(nets: &[(u32, Vec<(f64, f64)>)]) -> HashMap<u32, Vec<(u32, f64)>> {
    let bboxes: Vec<(u32, (f64, f64, f64, f64))> = nets
        .iter()
        .map(|(id, pads)| (*id, net_bbox(pads)))
        .collect();

    let mut conflicts: HashMap<u32, Vec<(u32, f64)>> = HashMap::new();
    const OVERLAP_THRESHOLD: f64 = 0.1;

    for i in 0..bboxes.len() {
        for j in (i + 1)..bboxes.len() {
            let ratio = bbox_overlap_ratio(bboxes[i].1, bboxes[j].1);
            if ratio > OVERLAP_THRESHOLD {
                conflicts
                    .entry(bboxes[i].0)
                    .or_default()
                    .push((bboxes[j].0, ratio));
                conflicts
                    .entry(bboxes[j].0)
                    .or_default()
                    .push((bboxes[i].0, ratio));
            }
        }
    }
    conflicts
}

/// MPS decomposition: group nets into rounds where each round is an independent set
/// (no significant bounding-box conflicts within the round).
/// Returns (rounds as Vec<Vec<index>>, per-net layer_preference).
/// `num_signal_layers`: number of signal layers (2 for 2/4-layer, 3 for 6-layer).
fn compute_mps_ordering(
    nets: &[(u32, Vec<(f64, f64)>)],
    num_signal_layers: usize,
) -> (Vec<Vec<usize>>, HashMap<u32, usize>) {
    let n = nets.len();
    if n == 0 {
        return (vec![], HashMap::new());
    }

    let conflicts = build_conflict_graph(nets);
    let mut remaining: Vec<usize> = (0..n).collect();
    let mut rounds: Vec<Vec<usize>> = Vec::new();
    let mut layer_preferences: HashMap<u32, usize> = HashMap::new();

    // Compute conflict degree for each net
    let conflict_degree =
        |idx: usize| -> usize { conflicts.get(&nets[idx].0).map_or(0, |v| v.len()) };

    // Track per-layer net count for load balancing
    let mut layer_counts: Vec<usize> = vec![0; num_signal_layers];

    let mut round_idx = 0;
    while !remaining.is_empty() {
        // Sort remaining by (conflict_degree ascending, bbox_area ascending)
        remaining.sort_by(|&a, &b| {
            let deg_a = conflict_degree(a);
            let deg_b = conflict_degree(b);
            deg_a
                .cmp(&deg_b)
                .then_with(|| {
                    let area_a = bounding_box_area(&nets[a].1);
                    let area_b = bounding_box_area(&nets[b].1);
                    area_a
                        .partial_cmp(&area_b)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .then_with(|| nets[a].0.cmp(&nets[b].0))
        });

        // Greedy MIS: pick nets that don't conflict with already-picked nets
        let mut round_nets: Vec<usize> = Vec::new();
        let mut picked: HashSet<u32> = HashSet::new();
        let mut new_remaining: Vec<usize> = Vec::new();

        for &idx in &remaining {
            let net_id = nets[idx].0;
            let net_conflicts = conflicts.get(&net_id);
            let has_conflict = net_conflicts
                .is_some_and(|conflicts| conflicts.iter().any(|(cid, _)| picked.contains(cid)));

            if has_conflict {
                new_remaining.push(idx);
            } else {
                round_nets.push(idx);
                picked.insert(net_id);
            }
        }

        // Assign layer preference: direction-aware + load-balanced across all signal layers
        for &idx in &round_nets {
            let net_id = nets[idx].0;

            if num_signal_layers <= 2 {
                // 2 signal layers: exact original behavior for regression safety
                if round_idx == 0 {
                    layer_preferences.insert(net_id, 0); // F.Cu
                } else {
                    let (min_x, min_y, max_x, max_y) = net_bbox(&nets[idx].1);
                    let h_span = max_x - min_x;
                    let v_span = max_y - min_y;
                    layer_preferences.insert(net_id, if h_span >= v_span { 0 } else { 1 });
                }
            } else {
                // 3+ signal layers: direction-aware + load-balanced assignment
                let (min_x, min_y, max_x, max_y) = net_bbox(&nets[idx].1);
                let h_span = max_x - min_x;
                let v_span = max_y - min_y;

                let preferred_slot = if num_signal_layers == 3 {
                    let aspect = h_span / v_span.max(0.01);
                    if aspect >= 1.5 {
                        0
                    } else if aspect <= 0.67 {
                        2
                    } else {
                        1
                    }
                } else {
                    let angle = (v_span / h_span.max(0.01)).atan();
                    ((angle / std::f64::consts::PI * num_signal_layers as f64).floor() as usize)
                        .min(num_signal_layers - 1)
                };

                let max_count = layer_counts.iter().max().copied().unwrap_or(0);
                let min_count = layer_counts.iter().min().copied().unwrap_or(0);
                let slot = if layer_counts[preferred_slot] > min_count + (max_count - min_count) / 3
                {
                    layer_counts
                        .iter()
                        .enumerate()
                        .min_by_key(|(_, &c)| c)
                        .map(|(i, _)| i)
                        .unwrap_or(preferred_slot)
                } else {
                    preferred_slot
                };

                layer_preferences.insert(net_id, slot);
                layer_counts[slot] += 1;
            }
        }

        remaining = new_remaining;
        rounds.push(round_nets);
        round_idx += 1;
    }

    let total_conflicts: usize = conflicts.values().map(|v| v.len()).sum::<usize>() / 2;
    let layer_swaps = layer_preferences.values().filter(|&&l| l != 0).count();
    eprintln!(
        "[router] MPS: {} nets, {} conflicts, {} rounds, {} layer swaps, layer_dist={:?}",
        n,
        total_conflicts,
        rounds.len(),
        layer_swaps,
        layer_counts
    );

    (rounds, layer_preferences)
}

// ---------------------------------------------------------------------------
// Hungarian Algorithm for Pad-Net Assignment Optimization
// ---------------------------------------------------------------------------

/// Solve the assignment problem using the Hungarian algorithm.
/// Given an n x n cost matrix, returns the optimal column assignment for each row.
/// O(n³) time using the Jonker-Volgenant approach.
fn hungarian_assignment(cost: &[Vec<f64>]) -> Vec<usize> {
    let n = cost.len();
    if n == 0 {
        return vec![];
    }
    if n == 1 {
        return vec![0];
    }

    // Use 1-indexed arrays for the algorithm
    let mut u = vec![0.0_f64; n + 1];
    let mut v = vec![0.0_f64; n + 1];
    let mut p = vec![0_usize; n + 1]; // p[j] = row assigned to column j
    let mut way = vec![0_usize; n + 1]; // path tracking

    for i in 1..=n {
        p[0] = i;
        let mut j0 = 0_usize;
        let mut minv = vec![f64::MAX; n + 1];
        let mut used = vec![false; n + 1];

        loop {
            used[j0] = true;
            let i0 = p[j0];
            let mut delta = f64::MAX;
            let mut j1 = 0_usize;

            for j in 1..=n {
                if !used[j] {
                    let cur = cost[i0 - 1][j - 1] - u[i0] - v[j];
                    if cur < minv[j] {
                        minv[j] = cur;
                        way[j] = j0;
                    }
                    if minv[j] < delta {
                        delta = minv[j];
                        j1 = j;
                    }
                }
            }

            for j in 0..=n {
                if used[j] {
                    u[p[j]] += delta;
                    v[j] -= delta;
                } else {
                    minv[j] -= delta;
                }
            }

            j0 = j1;
            if p[j0] == 0 {
                break;
            }
        }

        // Update assignments along the path
        loop {
            let j1 = way[j0];
            p[j0] = p[j1];
            j0 = j1;
            if j0 == 0 {
                break;
            }
        }
    }

    // Extract result: result[row] = column
    let mut result = vec![0; n];
    for j in 1..=n {
        if p[j] > 0 {
            result[p[j] - 1] = j - 1;
        }
    }
    result
}

/// A group of pads on the same component whose net assignments can be swapped.
#[allow(dead_code)]
struct SwappableGroup {
    /// (net_id, local_pad_position, remote_endpoint_position)
    entries: Vec<(u32, (f64, f64), (f64, f64))>,
    /// Index into pads_by_net for each entry (which pad to swap)
    pad_indices: Vec<usize>,
}

/// Find the "remote endpoint" for a net: given a local pad position,
/// return the closest pad on a different component.
fn find_remote_endpoint(pads: &[(f64, f64)], local: (f64, f64)) -> (f64, f64) {
    let mut best = local;
    let mut best_dist = f64::MAX;
    for &p in pads {
        let d = (p.0 - local.0).powi(2) + (p.1 - local.1).powi(2);
        if d > 1e-9 && d < best_dist {
            best_dist = d;
            best = p;
        }
    }
    best
}

/// Optimize pad-to-net assignments for swappable endpoint groups.
fn optimize_pad_assignments(
    board: &Board,
    pads_by_net: &mut HashMap<u32, Vec<(f64, f64)>>,
    power_nets: &HashSet<u32>,
    diff_paired_nets: &HashSet<u32>,
) {
    // Pad swapping is electrically unsafe by default: pins with distinct
    // functions (e.g. MOSFET G/S/D) are NOT interchangeable, and rewriting
    // pads_by_net here reroutes a signal to the wrong physical pad.
    // Opt in only for footprints with genuinely swappable pin groups via
    // KDESIGN_ENABLE_PAD_SWAP=1 (needs a per-footprint swappable_pins
    // property before it can be safe).
    if !std::env::var("KDESIGN_ENABLE_PAD_SWAP")
        .map(|v| v == "1")
        .unwrap_or(false)
    {
        return;
    }
    // Group signal pads by component
    let mut groups: HashMap<String, Vec<(u32, (f64, f64))>> = HashMap::new();
    for fp in &board.footprints {
        let (fx, fy, _) = fp.position;
        for pad in &fp.pads {
            if let Some(net_id) = pad.net {
                if net_id == 0 || power_nets.contains(&net_id) || diff_paired_nets.contains(&net_id)
                {
                    continue;
                }
                let (px, py) = fp.pad_rotated_offset(pad);
                groups
                    .entry(fp.reference.clone())
                    .or_default()
                    .push((net_id, (fx + px, fy + py)));
            }
        }
    }

    let mut total_swaps = 0;
    let mut total_groups = 0;

    let mut sorted_groups: Vec<(String, Vec<(u32, (f64, f64))>)> = groups.into_iter().collect();
    sorted_groups.sort_by(|a, b| a.0.cmp(&b.0));
    for (_ref, mut group) in sorted_groups {
        if group.len() < 2 {
            continue;
        }

        // Deduplicate: keep first occurrence of each net_id
        let mut seen_nets: HashSet<u32> = HashSet::new();
        group.retain(|(net_id, _)| seen_nets.insert(*net_id));
        if group.len() < 2 {
            continue;
        }

        // Build cost matrix
        let n = group.len();
        let mut cost: Vec<Vec<f64>> = vec![vec![0.0; n]; n];

        for i in 0..n {
            let net_id = group[i].0;
            let local_pad = group[i].1;
            let pads = pads_by_net.get(&net_id).cloned().unwrap_or_default();
            let remote = find_remote_endpoint(&pads, local_pad);

            for j in 0..n {
                let target_pad = group[j].1;
                cost[i][j] = (remote.0 - target_pad.0).abs() + (remote.1 - target_pad.1).abs();
            }
        }

        // Solve with Hungarian algorithm
        let assignment = hungarian_assignment(&cost);

        // Check if assignment differs from identity
        let mut any_swap = false;
        for i in 0..n {
            if assignment[i] != i {
                any_swap = true;
                break;
            }
        }

        if any_swap {
            // Apply swaps: reorder pads in pads_by_net
            for i in 0..n {
                let net_id = group[i].0;
                let target_j = assignment[i];
                let target_pad = group[target_j].1;

                if let Some(pads) = pads_by_net.get_mut(&net_id) {
                    // Replace the pad closest to the original local_pad with target_pad
                    let local_pad = group[i].1;
                    if let Some(closest) = pads
                        .iter()
                        .enumerate()
                        .filter(|(_, p)| {
                            (p.0 - local_pad.0).abs() < 0.01 && (p.1 - local_pad.1).abs() < 0.01
                        })
                        .map(|(idx, _)| idx)
                        .next()
                    {
                        pads[closest] = target_pad;
                    }
                }
            }
            total_swaps += 1;
            total_groups += 1;
        }
    }

    if total_swaps > 0 {
        eprintln!(
            "[router] Hungarian: {} swappable groups, {} swaps applied",
            total_groups, total_swaps
        );
    }
}

// ---------------------------------------------------------------------------
// System memory helpers (cross-platform)
// ---------------------------------------------------------------------------

/// Total physical RAM in MB.
fn get_total_memory_mb() -> f64 {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("sysctl")
            .arg("-n")
            .arg("hw.memsize")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| s.trim().parse::<u64>().ok())
            .map(|b| b as f64 / 1_048_576.0)
            .unwrap_or(16_384.0)
    }
    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string("/proc/meminfo")
            .ok()
            .and_then(|s| {
                for line in s.lines() {
                    if line.starts_with("MemTotal:") {
                        return line
                            .split_whitespace()
                            .nth(1)
                            .and_then(|v| v.parse::<u64>().ok())
                            .map(|kb| kb as f64 / 1024.0);
                    }
                }
                None
            })
            .unwrap_or(16_384.0)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        16_384.0
    }
}

/// Available (free + inactive/cached) memory in MB.
fn get_available_memory_mb() -> f64 {
    #[cfg(target_os = "macos")]
    {
        // macOS: parse vm_stat for "Pages free" + "Pages inactive" × page_size
        let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) as f64 };
        std::process::Command::new("vm_stat")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| {
                let mut free: f64 = 0.0;
                let mut inactive: f64 = 0.0;
                for line in s.lines() {
                    let line = line.trim();
                    if line.starts_with("Pages free:") {
                        free = line
                            .split(':')
                            .nth(1)
                            .map(|v| v.trim().trim_end_matches('.').parse::<f64>().unwrap_or(0.0))
                            .unwrap_or(0.0);
                    } else if line.starts_with("Pages inactive:") {
                        inactive = line
                            .split(':')
                            .nth(1)
                            .map(|v| v.trim().trim_end_matches('.').parse::<f64>().unwrap_or(0.0))
                            .unwrap_or(0.0);
                    }
                }
                (free + inactive) * page_size / 1_048_576.0
            })
            .unwrap_or(0.0)
    }
    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string("/proc/meminfo")
            .ok()
            .and_then(|s| {
                for line in s.lines() {
                    if line.starts_with("MemAvailable:") {
                        return line
                            .split_whitespace()
                            .nth(1)
                            .and_then(|v| v.parse::<u64>().ok())
                            .map(|kb| kb as f64 / 1024.0);
                    }
                }
                None
            })
            .unwrap_or(0.0)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        0.0
    }
}

// ---------------------------------------------------------------------------
// GPU Wavefront Integration (feature-gated)
// ---------------------------------------------------------------------------

/// Attempt GPU wavefront routing for large boards.
/// Routes 2-pin nets on the first signal layer using GPU compute shaders.
/// Successfully routed nets are committed to grid + board, remaining nets
/// fall through to CPU parallel A*.
#[cfg(feature = "gpu-router")]
fn try_gpu_wavefront(
    grid: &mut RoutingGrid,
    sorted_nets: &[(u32, Vec<(f64, f64)>)],
    _net_names: &HashMap<u32, String>,
    board: &mut Board,
    result: &mut RoutingResult,
) -> anyhow::Result<usize> {
    use crate::gpu_router::{encode_cell, GpuRouter};

    // Only route 2-pin nets on GPU (wavefront is single-source -> single-goal)
    let sig_layers = grid.layer_config.signal_layer_indices();
    let first_sig_layer = match sig_layers.first() {
        Some(&l) => l,
        None => return Ok(0),
    };
    let per_layer = grid.rows * grid.cols;

    // Encode grid to u32 for GPU — only the first signal layer
    let mut grid_u32: Vec<u32> = vec![0; per_layer];
    for i in 0..per_layer {
        let flat = first_sig_layer * per_layer + i;
        let cell = grid.layer_data[flat];
        let (cell_byte, nid) = match cell {
            Cell::Free => (0u8, 0u32),
            Cell::Blocked => (1u8, 0u32),
            Cell::Pad(n) => (2u8, n),
            Cell::Trace(n) => (3u8, n),
            Cell::Via(n) => (4u8, n),
        };
        grid_u32[i] = encode_cell(cell_byte, nid);
    }

    // Build net start/goal pairs for 2-pin nets only
    let gpu_nets: Vec<(u32, (usize, usize), (usize, usize))> = sorted_nets
        .iter()
        .filter_map(|&(net_id, ref pads)| {
            if pads.len() != 2 {
                return None;
            }
            let start = grid.world_to_grid(pads[0].0, pads[0].1);
            let goal = grid.world_to_grid(pads[1].0, pads[1].1);
            if !grid.in_bounds(start.0, start.1) || !grid.in_bounds(goal.0, goal.1) {
                return None;
            }
            Some((net_id, start, goal))
        })
        .collect();

    if gpu_nets.is_empty() {
        eprintln!("[gpu-router] No 2-pin nets suitable for GPU routing");
        return Ok(0);
    }

    // Limit GPU nets — route at most 500 nets on GPU to bound GPU time
    let gpu_nets: Vec<_> = gpu_nets.into_iter().take(500).collect();
    eprintln!(
        "[gpu-router] Routing {} 2-pin nets on GPU layer {}",
        gpu_nets.len(),
        first_sig_layer
    );

    // Initialize GPU router
    let gpu = pollster::block_on(GpuRouter::new())?;

    // Dispatch batch routing
    let max_rounds = (grid.cols + grid.rows) as u32 * 2;
    let gpu_results = gpu.route_batch(&grid_u32, grid.cols, grid.rows, &gpu_nets, max_rounds);

    // Commit GPU-routed paths to board segments and grid
    let trace_width = 0.2; // Default signal trace width (mm)
    let mut gpu_routed = 0usize;

    for (net_id, path) in &gpu_results {
        if path.len() < 2 {
            continue;
        }

        let layer_name = grid.layer_config.layer_name(first_sig_layer).to_string();
        let mut segments_added = 0usize;

        for window in path.windows(2) {
            let (c0, r0) = window[0];
            let (c1, r1) = window[1];
            let (x0, y0) = grid.grid_to_world(c0, r0);
            let (x1, y1) = grid.grid_to_world(c1, r1);

            let seg = Segment {
                start: (x0, y0),
                end: (x1, y1),
                width: trace_width,
                layer: layer_name.clone(),
                net: *net_id,
            };

            board.segments.push(seg);
            segments_added += 1;
        }

        // Mark path cells on grid to block subsequent CPU routing
        for &(c, r) in path {
            let idx = r * grid.cols + c;
            let flat = first_sig_layer * per_layer + idx;
            if grid.layer_data[flat] == Cell::Free {
                grid.layer_data[flat] = Cell::Trace(*net_id);
                let n = *net_id as usize;
                if n < grid.net_cells.len() {
                    grid.net_cells[n].push((first_sig_layer, idx));
                }
            }
        }

        if segments_added > 0 {
            gpu_routed += 1;
            result.routed_nets += 1;
            result.total_segments += segments_added;
        }
    }

    eprintln!(
        "[gpu-router] Successfully routed {}/{} nets on GPU",
        gpu_routed,
        gpu_nets.len()
    );
    Ok(gpu_routed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fp(lib_id: &str, reference: &str, value: &str, n_pads: usize) -> Footprint {
        // Footprint::new fills all fields with sane defaults; we only need
        // pads.len() to drive the >= 100 fallback in classify_footprint_role.
        let mut f = Footprint::new(lib_id, reference, value);
        for i in 0..n_pads {
            f.pads.push(kicad_json5::ir::board::Pad {
                number: format!("{i}"),
                pad_type: kicad_json5::ir::board::PadType::Smd,
                shape: kicad_json5::ir::board::PadShape::Rect,
                position: (0.0, 0.0, 0.0),
                size: (0.5, 0.5),
                layers: vec!["F.Cu".into()],
                drill: None,
                net: None,
                net_name: None,
                pin_function: None,
                pin_type: None,
                roundrect_rratio: None,
                solder_mask_margin: None,
                thermal_bridge_width: None,
                thermal_bridge_angle: None,
                thermal_gap: None,
                clearance: None,
                zone_connect: None,
                remove_unused_layers: None,
                options: None,
                primitives: Vec::new(),
            });
        }
        f
    }

    #[test]
    fn test_topology_kind_auto_default() {
        // Auto is the default/documented existing-behavior variant.
        let t = NetTopology {
            kind: TopologyKind::Auto,
            visit_order: vec![],
        };
        assert_eq!(t.kind, TopologyKind::Auto);
        assert!(t.visit_order.is_empty());
    }

    #[test]
    fn test_classify_footprint_role_memory() {
        // lib_id carrying DDR3 → Memory.
        assert_eq!(
            classify_footprint_role(&fp("Package_BGA:DDR3-96", "U5", "MT41J128M16", 96)),
            PadRole::Memory
        );
        // value carrying a known DRAM MPN (lib_id generic) → Memory.
        assert_eq!(
            classify_footprint_role(&fp("Package_BGA:Generic", "U6", "H5TQ2G63BFR", 96)),
            PadRole::Memory
        );
    }

    #[test]
    fn test_classify_footprint_role_controller() {
        // Explicit FPGA keyword → Controller.
        assert_eq!(
            classify_footprint_role(&fp("Package_BGA:BGA256_FTG256", "U12", "XC6SLX9", 256)),
            PadRole::Controller
        );
        // ≥100 pads fallback for unlabeled large IC.
        assert_eq!(
            classify_footprint_role(&fp("Package_BGA:CustomBGA", "U1", "MY-IC", 128)),
            PadRole::Controller
        );
    }

    #[test]
    fn test_classify_footprint_role_termination() {
        // Resistor refdes → Termination (regardless of value).
        assert_eq!(
            classify_footprint_role(&fp("Resistor_SMD:R_0402", "R5", "49.9", 2)),
            PadRole::Termination
        );
        // Resistor network RN*.
        assert_eq!(
            classify_footprint_role(&fp("Resistor_SMD:R_Array", "RN1", "4x49.9", 8)),
            PadRole::Termination
        );
    }

    #[test]
    fn test_classify_footprint_role_generic() {
        // Small non-resistor part with no keyword → Generic.
        assert_eq!(
            classify_footprint_role(&fp("Capacitor_SMD:C_0402", "C7", "100nF", 2)),
            PadRole::Generic
        );
        // Connector, no pads ≥100, no keyword.
        assert_eq!(
            classify_footprint_role(&fp("Connector:USB-C", "J1", "USB-C", 24)),
            PadRole::Generic
        );
    }

    #[test]
    fn test_classify_footprint_role_resistor_takes_priority_over_value_ddr() {
        // A resistor whose value string happens to contain "DDR3" still classifies
        // as Termination (resistor refdes wins over value-string DRAM match).
        assert_eq!(
            classify_footprint_role(&fp("Resistor_SMD:R_0402", "R9", "DDR3_TERM", 2)),
            PadRole::Termination
        );
    }

    // ----- Step 2: fly-by edge construction + topology analysis -----

    fn roles(spec: &[PadRole]) -> Vec<PadRole> {
        spec.to_vec()
    }

    #[test]
    fn test_flyby_basic() {
        // controller at origin, two SDRAM at increasing distance, one term far.
        let pads = vec![(0.0, 0.0), (5.0, 0.0), (10.0, 0.0), (15.0, 0.0)];
        let r = roles(&[
            PadRole::Controller,
            PadRole::Memory,
            PadRole::Memory,
            PadRole::Termination,
        ]);
        let (edges, sp) = build_flyby_edges(&pads, &r).expect("fly-by should apply");
        assert!(sp.is_empty(), "no Steiner points for fly-by");
        // Chain: 0 → 1 → 2 → 3 (already in distance order).
        assert_eq!(edges, vec![(0, 1), (1, 2), (2, 3)]);
    }

    #[test]
    fn test_flyby_memory_sorted_by_distance() {
        // Memory pads given out of distance order should be sorted ascending.
        let pads = vec![(0.0, 0.0), (10.0, 0.0), (3.0, 0.0)];
        let r = roles(&[PadRole::Controller, PadRole::Memory, PadRole::Memory]);
        let (edges, _) = build_flyby_edges(&pads, &r).expect("fly-by should apply");
        // Visit: controller(0) → closer mem(2 @3mm) → farther mem(1 @10mm).
        assert_eq!(edges, vec![(0, 2), (2, 1)]);
    }

    #[test]
    fn test_flyby_no_controller_returns_none() {
        // No controller → fly-by inapplicable, caller falls back to MST.
        let pads = vec![(0.0, 0.0), (5.0, 0.0), (10.0, 0.0)];
        let r = roles(&[PadRole::Memory, PadRole::Memory, PadRole::Termination]);
        assert!(build_flyby_edges(&pads, &r).is_none());
    }

    #[test]
    fn test_flyby_no_memory_controller_and_termination_only() {
        // controller + termination, no memory → single edge c → t.
        let pads = vec![(0.0, 0.0), (8.0, 0.0)];
        let r = roles(&[PadRole::Controller, PadRole::Termination]);
        let (edges, _) = build_flyby_edges(&pads, &r).expect("fly-by should apply");
        assert_eq!(edges, vec![(0, 1)]);
    }

    #[test]
    fn test_flyby_length_mismatch_returns_none() {
        // pads/roles length mismatch → defensive None.
        let pads = vec![(0.0, 0.0), (5.0, 0.0)];
        let r = roles(&[PadRole::Controller]); // too short
        assert!(build_flyby_edges(&pads, &r).is_none());
    }

    #[test]
    fn test_flyby_equal_distance_angle_tiebreak() {
        // Two memory pads at equal Manhattan distance, different angles.
        // pad1 at (5, 0) angle 0, pad2 at (0, 5) angle +π/2. Tie → sort by angle.
        let pads = vec![(0.0, 0.0), (5.0, 0.0), (0.0, 5.0)];
        let r = roles(&[PadRole::Controller, PadRole::Memory, PadRole::Memory]);
        let (edges, _) = build_flyby_edges(&pads, &r).expect("fly-by should apply");
        // angle 0 (pad1) < angle π/2 (pad2) → visit pad1 then pad2.
        assert_eq!(edges, vec![(0, 1), (1, 2)]);
    }

    #[test]
    fn test_analyze_topology_ddr3_addr_is_flyby() {
        let pads = vec![(0.0, 0.0), (5.0, 0.0), (10.0, 0.0), (15.0, 0.0)];
        let r = roles(&[
            PadRole::Controller,
            PadRole::Memory,
            PadRole::Memory,
            PadRole::Termination,
        ]);
        let topo = analyze_net_topology("DDR3_A0", &pads, &r);
        assert_eq!(topo.kind, TopologyKind::FlyBy);
        assert_eq!(topo.visit_order, vec![0, 1, 2, 3]);
    }

    #[test]
    fn test_analyze_topology_ddr3_dq_is_auto() {
        // DDR3 data group is point-to-point, must NOT become fly-by.
        let pads = vec![(0.0, 0.0), (5.0, 0.0), (10.0, 0.0)];
        let r = roles(&[PadRole::Controller, PadRole::Memory, PadRole::Memory]);
        let topo = analyze_net_topology("DDR3_DQ0", &pads, &r);
        assert_eq!(topo.kind, TopologyKind::Auto);
        assert!(topo.visit_order.is_empty());
    }

    #[test]
    fn test_analyze_topology_no_memory_role_is_auto() {
        // DDR3 net name but no Memory pad → can't fly-by, fall back to Auto.
        let pads = vec![(0.0, 0.0), (5.0, 0.0), (10.0, 0.0)];
        let r = roles(&[PadRole::Controller, PadRole::Generic, PadRole::Generic]);
        let topo = analyze_net_topology("DDR3_A5", &pads, &r);
        assert_eq!(topo.kind, TopologyKind::Auto);
    }

    #[test]
    fn test_analyze_topology_two_pins_is_auto() {
        // 2-pin DDR3 net is trivially point-to-point, MST handles it fine.
        let pads = vec![(0.0, 0.0), (5.0, 0.0)];
        let r = roles(&[PadRole::Controller, PadRole::Memory]);
        let topo = analyze_net_topology("DDR3_A0", &pads, &r);
        assert_eq!(topo.kind, TopologyKind::Auto);
    }

    // ----- Task1 修订版：修复模式（repair）导出——孔约束墙 + 自由端点 -----

    use kicad_json5::ir::board::{BoardGraphic, BoardGraphicKind, DrillDef, NetDef, Pad};

    /// mini 板框 (0,0)-(40,40)，SIG(1)/OTHER(2) 两张网。
    fn repair_board() -> Board {
        let mut b = Board::new();
        b.nets = vec![
            NetDef {
                id: 0,
                name: String::new(),
            },
            NetDef {
                id: 1,
                name: "SIG".into(),
            },
            NetDef {
                id: 2,
                name: "OTHER".into(),
            },
        ];
        b.graphics.push(BoardGraphic {
            kind: BoardGraphicKind::Rect {
                start: (0.0, 0.0),
                end: (40.0, 40.0),
            },
            layer: "Edge.Cuts".into(),
            stroke_width: 0.05,
            fill: false,
        });
        b
    }

    /// thru_hole 圆 pad（孔约束墙的主角：drill 贯穿全部铜层）
    fn thru_pad(at: (f64, f64), size: f64, drill: f64, net: Option<u32>) -> Pad {
        Pad {
            number: "1".into(),
            pad_type: kicad_json5::ir::board::PadType::ThruHole,
            shape: kicad_json5::ir::board::PadShape::Circle,
            position: (at.0, at.1, 0.0),
            size: (size, size),
            layers: vec!["*.Cu".into()],
            drill: Some(DrillDef {
                diameter: drill,
                width: None,
                offset: None,
            }),
            net,
            net_name: None,
            pin_function: None,
            pin_type: None,
            roundrect_rratio: None,
            solder_mask_margin: None,
            thermal_bridge_width: None,
            thermal_bridge_angle: None,
            thermal_gap: None,
            clearance: None,
            zone_connect: None,
            remove_unused_layers: None,
            options: None,
            primitives: Vec::new(),
        }
    }

    fn smd_pad(at: (f64, f64), net: Option<u32>) -> Pad {
        let mut p = thru_pad(at, 0.5, 0.0, net);
        p.pad_type = kicad_json5::ir::board::PadType::Smd;
        p.drill = None;
        p.layers = vec!["F.Cu".into()];
        p
    }

    /// 网格格值读取（与 export 编码同构：0=Free 1=Blocked 2+n=Pad 1e6+n=Trace 2e6+n=Via）
    fn cell_at(e: &WavefrontGridExport, layer: usize, x: f64, y: f64) -> u32 {
        let col = ((x - e.origin_mm.0) / e.grid_res_mm).round() as usize;
        let row = ((y - e.origin_mm.1) / e.grid_res_mm).round() as usize;
        let per_layer = e.cols * e.rows;
        e.grid_u32[layer * per_layer + row * e.cols + col]
    }

    #[test]
    fn test_repair_hole_wall_blocks_foreign_holes_only() {
        let mut b = repair_board();
        // 目标网 SIG：一对 SMD pad 作 from/to
        let mut fp_sig = Footprint::new("Test:R", "R1", "X");
        fp_sig.position = (5.0, 5.0, 0.0);
        fp_sig.pads.push(smd_pad((0.0, 0.0), Some(1)));
        fp_sig.pads.push(smd_pad((0.0, 5.0), Some(1)));
        b.footprints.push(fp_sig);
        // 异网 OTHER：PTH pad（drill 1.4）+ via（drill 0.3）。
        // footprint 主体按 infer_body_size 兜底 4x4mm，用大 pad offset 让 body
        // 远离孔墙环测点（pad 世界坐标 = at(14,20) + offset(6,0) = (20,20)）。
        let mut fp_other = Footprint::new("Test:PTH", "J1", "X");
        fp_other.position = (14.0, 20.0, 0.0);
        fp_other.pads.push(thru_pad((6.0, 0.0), 0.6, 1.4, Some(2)));
        b.footprints.push(fp_other);
        b.vias.push(Via {
            at: (30.0, 20.0),
            size: 0.6,
            drill: 0.3,
            layers: vec!["F.Cu".into(), "B.Cu".into()],
            net: 2,
        });

        let cfg = crate::layer_config::BoardLayerConfig::two_layer();
        // 孔墙半径：PTH = 1.4/2+0.2 = 0.9；via = 0.3/2+0.2 = 0.35
        let e_on = export_wavefront_grid_repair(
            &b,
            cfg.clone(),
            0.1,
            0,
            "SIG",
            (5.0, 5.0),
            (5.0, 10.0),
            0.2,
        )
        .expect("repair export");
        let e_off = export_wavefront_grid_repair(
            &b,
            cfg.clone(),
            0.1,
            0,
            "SIG",
            (5.0, 5.0),
            (5.0, 10.0),
            0.0,
        )
        .expect("repair export hole_clr=0");

        // 异网 PTH 孔墙环（0.8mm 处：pad 铜 bbox 0.55 之外、墙 0.9 之内）全信号层 Blocked
        for layer in 0..2 {
            assert_eq!(
                cell_at(&e_on, layer, 20.8, 20.0),
                1,
                "异网 PTH 孔墙应 Blocked layer{layer}"
            );
            assert_eq!(
                cell_at(&e_on, layer, 30.3, 20.0),
                1,
                "异网 via 孔墙应 Blocked layer{layer}"
            );
            // 对照：无孔墙时同格非 Blocked（区分孔墙与 body/铜既有墙）
            assert_ne!(
                cell_at(&e_off, layer, 20.8, 20.0),
                1,
                "hole_clr=0 不应有孔墙 layer{layer}"
            );
            assert_ne!(
                cell_at(&e_off, layer, 30.3, 20.0),
                1,
                "hole_clr=0 不应有孔墙 layer{layer}"
            );
        }
        // 自家网 SIG 的 PTH 周围不封（与 passable 只看净身份一致：目标常是自家 PTH pad）
        let mut fp_own = Footprint::new("Test:PTH", "J2", "X");
        fp_own.position = (6.0, 30.0, 0.0);
        fp_own.pads.push(thru_pad((6.0, 0.0), 0.6, 1.4, Some(1)));
        b.footprints.push(fp_own);
        let e_own = export_wavefront_grid_repair(
            &b,
            cfg.clone(),
            0.1,
            0,
            "SIG",
            (5.0, 5.0),
            (5.0, 10.0),
            0.2,
        )
        .expect("repair export");
        for layer in 0..2 {
            assert_ne!(
                cell_at(&e_own, layer, 12.6, 30.0),
                1,
                "自家网 PTH 不应被孔墙封死 layer{layer}"
            );
        }
    }
    #[test]
    fn test_repair_free_endpoints_synthetic_net() {
        let mut b = repair_board();
        // GND(id 3)：仅 1 pin 的电源网（常规 export 会因 pads.len()!=2 / power_net_name 被过滤）
        b.nets.push(NetDef {
            id: 3,
            name: "GND".into(),
        });
        let mut fp_gnd = Footprint::new("Test:PAD", "TP1", "X");
        fp_gnd.position = (8.0, 8.0, 0.0);
        fp_gnd.pads.push(smd_pad((0.0, 0.0), Some(3)));
        b.footprints.push(fp_gnd);

        let cfg = crate::layer_config::BoardLayerConfig::two_layer();
        let (from, to) = ((12.0, 12.0), (16.0, 16.0));
        let e = export_wavefront_grid_repair(&b, cfg.clone(), 0.5, 0, "GND", from, to, 0.0)
            .expect("repair export");
        assert_eq!(
            e.nets.len(),
            1,
            "nets 数组装一条合成 net（1-pin 电源网不过滤）"
        );
        let n = &e.nets[0];
        assert_eq!(n.net_id, 3);
        assert_eq!(n.name, "GND");
        assert_eq!(n.start_mm, from, "start_mm = from 原值");
        assert_eq!(n.goal_mm, to, "goal_mm = to 原值");
        let want_start = (
            ((from.0 - e.origin_mm.0) / e.grid_res_mm).round() as usize,
            ((from.1 - e.origin_mm.1) / e.grid_res_mm).round() as usize,
        );
        assert_eq!(n.start, want_start, "start = from 的 grid 坐标");

        // 板上查不到的网名 → net_id=0 但仍导出（合成端点）
        let e0 = export_wavefront_grid_repair(&b, cfg, 0.5, 0, "NOPE", from, to, 0.0)
            .expect("repair export unknown net");
        assert_eq!(e0.nets.len(), 1);
        assert_eq!(e0.nets[0].net_id, 0);
    }

    #[test]
    fn test_repair_endpoints_outside_board_bbox_rejected() {
        let b = repair_board();
        let cfg = crate::layer_config::BoardLayerConfig::two_layer();
        // 板框 (0,0)-(40,40)：越界端点必须报错而非静默 clamp
        assert!(export_wavefront_grid_repair(
            &b,
            cfg.clone(),
            0.5,
            0,
            "SIG",
            (-1.0, 5.0),
            (5.0, 5.0),
            0.0
        )
        .is_err());
        assert!(
            export_wavefront_grid_repair(&b, cfg, 0.5, 0, "SIG", (5.0, 5.0), (41.0, 5.0), 0.0)
                .is_err()
        );
    }
}

// ---------------------------------------------------------------------------
// Phase B: Wavefront grid export (for kroute-server RouteGrid / CUDA router)
// ---------------------------------------------------------------------------

/// 全信号层波前网格导出（kroute-server RouteGrid 多层模式消费格式）。
#[derive(Debug)]
pub struct WavefrontGridExport {
    pub cols: usize,
    pub rows: usize,
    pub grid_res_mm: f64,
    pub origin_mm: (f64, f64),
    /// 各信号层名（grid_u32 按 此顺序 扁平：layer*rows*cols + row*cols + col）
    pub layers: Vec<String>,
    /// encode_cell 语义 u32：0=Free 1=Blocked 2+n=Pad(n) 1e6+n=Trace(n) 2e6+n=Via(n)
    pub grid_u32: Vec<u32>,
    /// 2-pin 信号 net（已排除 net_id=0 与电源网）
    pub nets: Vec<WavefrontNetExport>,
}

#[derive(Debug)]
pub struct WavefrontNetExport {
    pub net_id: u32,
    pub name: String,
    pub start: (usize, usize),
    pub goal: (usize, usize),
    pub start_mm: (f64, f64),
    pub goal_mm: (f64, f64),
}

/// 从板子导出"可路由起点状态"的第一信号层网格：
/// RoutingGrid::new + mark_component_bodies + mark_pads + mark_existing_traces，
/// 与 auto_route 主流程进入 A* 前的障碍状态一致（不含 fan-out/zone 后处理）。
/// 只读板子，不改板。
pub fn export_wavefront_grid(
    board: &Board,
    layer_config: crate::layer_config::BoardLayerConfig,
    grid_res: f64,
) -> anyhow::Result<WavefrontGridExport> {
    export_wavefront_grid_dilated(board, layer_config, grid_res, 0)
}

/// 信号层编码（导出共用）：RoutingGrid Cell → encode_cell u32 扁平数组 + 层名。
/// 扁平序 = 信号层序 × (row*cols + col)。
fn encode_signal_layers(grid: &RoutingGrid) -> anyhow::Result<(Vec<u32>, Vec<String>)> {
    use crate::gpu_router::encode_cell;

    let sig_layers = grid.layer_config.signal_layer_indices();
    if sig_layers.is_empty() {
        anyhow::bail!("board layer config has no signal layer");
    }
    let per_layer = grid.rows * grid.cols;
    let mut grid_u32 = Vec::with_capacity(sig_layers.len() * per_layer);
    let mut layer_names = Vec::with_capacity(sig_layers.len());
    for &l in sig_layers.iter() {
        for i in 0..per_layer {
            let cell = grid.layer_data[l * per_layer + i];
            let (cell_byte, nid) = match cell {
                Cell::Free => (0u8, 0u32),
                Cell::Blocked => (1u8, 0u32),
                Cell::Pad(n) => (2u8, n),
                Cell::Trace(n) => (3u8, n),
                Cell::Via(n) => (4u8, n),
            };
            grid_u32.push(encode_cell(cell_byte, nid));
        }
        layer_names.push(grid.layer_config.layer_name(l).to_string());
    }
    Ok((grid_u32, layer_names))
}

/// 带间距膨胀的导出（M3）：`dilate_cells` 轮 8 邻域膨胀——非 Free 格把净身份
/// 写进相邻 Free 格（Pad/Trace/Via 保持原类型，Blocked 膨胀为 Blocked），
/// 使波前中心线天然远离其他网铜皮 ≥ dilate_cells 格。净 n 靠近自己的铜皮不受限
/// （passable 规则只看 net 身份）。0 = 关闭（旧行为）。
pub fn export_wavefront_grid_dilated(
    board: &Board,
    layer_config: crate::layer_config::BoardLayerConfig,
    grid_res: f64,
    dilate_cells: usize,
) -> anyhow::Result<WavefrontGridExport> {
    let mut grid = RoutingGrid::new(board, layer_config.clone(), grid_res);
    grid.mark_component_bodies(board);
    grid.mark_pads(board);
    grid.mark_existing_traces(board, DEFAULT_CLEARANCE);

    let (mut grid_u32, layer_names) = encode_signal_layers(&grid)?;
    let per_layer = grid.rows * grid.cols;

    // M3 间距膨胀：逐轮 8 邻域把非 Free 格的净身份写进 Free 邻格。
    // 用快照防串染（一轮生长一格）；层内膨胀，不跨层。
    if dilate_cells > 0 {
        const NEIGH8: [(i64, i64); 8] = [
            (0, 1),
            (0, -1),
            (1, 0),
            (-1, 0),
            (1, 1),
            (1, -1),
            (-1, 1),
            (-1, -1),
        ];
        let (cols, rows, n_layers) = (grid.cols, grid.rows, layer_names.len());
        for _ in 0..dilate_cells {
            let snap = grid_u32.clone();
            for l in 0..n_layers {
                for r in 0..rows {
                    for c in 0..cols {
                        let idx = l * per_layer + r * cols + c;
                        if snap[idx] != 0 {
                            continue; // 已占
                        }
                        let mut src = 0u32;
                        for &(dr, dc) in &NEIGH8 {
                            let (rr, cc) = (r as i64 + dr, c as i64 + dc);
                            if rr < 0 || cc < 0 || rr >= rows as i64 || cc >= cols as i64 {
                                continue;
                            }
                            let v = snap[l * per_layer + rr as usize * cols + cc as usize];
                            if v != 0 {
                                // Blocked 膨胀为 Blocked；带网身份则继承（自己网靠近自己的铜皮不受限）
                                src = v;
                                break;
                            }
                        }
                        if src != 0 {
                            grid_u32[idx] = src;
                        }
                    }
                }
            }
        }
    }

    // 收集 net→pad 世界坐标（与主流程 pads_by_net 同源），只保留 2-pin 信号 net
    let mut pads_by_net: std::collections::HashMap<u32, Vec<(f64, f64)>> =
        std::collections::HashMap::new();
    let mut name_by_net: std::collections::HashMap<u32, String> = std::collections::HashMap::new();
    for fp in &board.footprints {
        let (fx, fy, _) = fp.position;
        for pad in &fp.pads {
            let Some(net_id) = pad.net else { continue };
            if net_id == 0 {
                continue;
            }
            let (px, py) = fp.pad_rotated_offset(pad);
            pads_by_net
                .entry(net_id)
                .or_default()
                .push((fx + px, fy + py));
        }
    }
    for n in &board.nets {
        name_by_net.insert(n.id, n.name.clone());
    }

    let mut nets = Vec::new();
    for (net_id, mut pads) in pads_by_net {
        if pads.len() != 2 {
            continue;
        }
        let name = name_by_net.get(&net_id).cloned().unwrap_or_default();
        if power_net_name(&name) {
            continue;
        }
        pads.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let start = grid.world_to_grid(pads[0].0, pads[0].1);
        let goal = grid.world_to_grid(pads[1].0, pads[1].1);
        if !grid.in_bounds(start.0, start.1) || !grid.in_bounds(goal.0, goal.1) {
            continue;
        }
        nets.push(WavefrontNetExport {
            net_id,
            name,
            start,
            goal,
            start_mm: pads[0],
            goal_mm: pads[1],
        });
    }
    nets.sort_by_key(|n| n.net_id);

    Ok(WavefrontGridExport {
        cols: grid.cols,
        rows: grid.rows,
        grid_res_mm: grid.grid_res,
        origin_mm: (grid.origin_x, grid.origin_y),
        layers: layer_names,
        grid_u32,
        nets,
    })
}

/// 孔约束墙（修复模式，Task1 修订版）：PTH pad 与 via 的钻孔（drill/2）+ `hole_clr`
/// 半径区域在**全部信号层**标 Blocked——孔约束独立于铜净空、作用于所有铜层
///（KB「via 合法性两套墙」）。只填 Free 格：孔墙包着的带网身份铜皮（Pad/Trace/Via）
/// 保留原编码，NO-PATH 归因才能从簇内格反查孔主人。
/// net 身份等于 `skip_net` 的孔不封：与 passable 只看净身份的既有规则一致——
/// 修复目标常是自家 PTH pad（如 in-pad 缝合孔落点），全封会不可达。
fn mark_hole_walls(grid: &mut RoutingGrid, board: &Board, skip_net: u32, hole_clr: f64) {
    use kicad_json5::ir::board::PadType;
    if hole_clr <= 0.0 {
        return;
    }
    let signal_layers = grid.layer_config.signal_layer_indices();

    // 圆盘 Blocked：bbox 扫描 + 格心圆判，只写 Free 格（不覆盖带身份铜皮）
    let mark_disc = |grid: &mut RoutingGrid, cx: f64, cy: f64, radius: f64| {
        let (c0, r0) = grid.world_to_grid(cx - radius, cy - radius);
        let (c1, r1) = grid.world_to_grid(cx + radius, cy + radius);
        let r2 = radius * radius;
        for c in c0..=c1.min(grid.cols - 1) {
            for r in r0..=r1.min(grid.rows - 1) {
                let (wx, wy) = grid.grid_to_world(c, r);
                let dx = wx - cx;
                let dy = wy - cy;
                if dx * dx + dy * dy > r2 {
                    continue;
                }
                for &layer in &signal_layers {
                    if grid.get(layer, c, r) == Cell::Free {
                        grid.set(layer, c, r, Cell::Blocked);
                    }
                }
            }
        }
    };

    // PTH 类 pad（含安装孔 np_thru_hole——无网孔对任何网都是墙）
    for fp in &board.footprints {
        let (fx, fy, _) = fp.position;
        for pad in &fp.pads {
            if !matches!(pad.pad_type, PadType::ThruHole | PadType::NpThruHole) {
                continue;
            }
            let Some(drill) = &pad.drill else { continue };
            let net_id = pad.net.unwrap_or(0);
            if net_id == skip_net {
                continue;
            }
            let (px, py) = fp.pad_rotated_offset(pad);
            mark_disc(grid, fx + px, fy + py, drill.diameter / 2.0 + hole_clr);
        }
    }
    // via（mark_existing_traces 只编 segment，via 的全部障碍都在这里补）：
    // - 铜净空环：size/2 + DEFAULT_CLEARANCE + 新段半宽(0.125/2)≈0.575 —— 新走线
    //   中心与新 via 落点都要避开（via pad 0.3 边到段边净空 0.15）；
    // - 孔约束环：drill/2 + hole_clr —— 孔约束独立于铜净空、作用于所有铜层。
    // 两者取大。
    for via in &board.vias {
        if via.net == skip_net {
            continue;
        }
        let copper_r = via.size / 2.0 + DEFAULT_CLEARANCE + 0.125;
        let hole_r = via.drill / 2.0 + hole_clr;
        mark_disc(grid, via.at.0, via.at.1, copper_r.max(hole_r));
    }
}

/// zone 填充铜（修复模式）：把 refilled 的 filled_polygon 栅格化进网格，
/// 编码为 Trace(net)（带净身份：异网 zone 是墙且可归因，自家 zone 可爬）。
/// 只写 Free 格（pad/body/既有走线优先——thermal relief 挖空区不会误标）。
/// 没有这一 pass，修复路径会从异网灌铜上穿过（波前网格只见 segment/pad 的已知局限）。
/// 栅格化用逐行扫描线（even-odd），O(rows × 顶点数)。
fn mark_zone_fills(grid: &mut RoutingGrid, board: &Board) {
    for zone in &board.zones {
        let Some(layer) = grid.layer_config.layer_index(&zone.layer) else {
            continue; // 非信号层 zone 不进波前网格
        };
        for poly in &zone.filled_polygons {
            if poly.points.len() < 3 {
                continue;
            }
            // bbox 行范围（格心落在多边形内的行才需要求交）
            let ys: Vec<f64> = poly.points.iter().map(|p| p.1).collect();
            let (y_min, y_max) = (
                ys.iter().cloned().fold(f64::MAX, f64::min),
                ys.iter().cloned().fold(f64::MIN, f64::max),
            );
            // world_to_grid 返回 (col,row)：这里只要行范围
            let (_, r0) = grid.world_to_grid(0.0, y_min);
            let (_, r1) = grid.world_to_grid(0.0, y_max);
            let n = poly.points.len();
            for row in r0..=r1.min(grid.rows - 1) {
                let cy = grid.origin_y + row as f64 * grid.grid_res;
                // 求所有边与扫描线 y=cy 的交点横坐标（even-odd）
                let mut xs: Vec<f64> = Vec::new();
                for i in 0..n {
                    let (x1, y1) = poly.points[i];
                    let (x2, y2) = poly.points[(i + 1) % n];
                    if (y1 <= cy && y2 > cy) || (y2 <= cy && y1 > cy) {
                        let t = (cy - y1) / (y2 - y1);
                        xs.push(x1 + t * (x2 - x1));
                    }
                }
                xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                // 成对区间内（格心在两交点之间）标记
                for pair in xs.chunks(2) {
                    if pair.len() < 2 {
                        break;
                    }
                    let (xa, xb) = (pair[0], pair[1]);
                    let (c0, _) = grid.world_to_grid(xa, cy);
                    let (c1, _) = grid.world_to_grid(xb, cy);
                    for col in c0..=c1.min(grid.cols - 1) {
                        let cx = grid.origin_x + col as f64 * grid.grid_res;
                        if cx < xa || cx > xb {
                            continue;
                        }
                        if grid.get(layer, col, row) == Cell::Free {
                            grid.set(layer, col, row, Cell::Trace(zone.net));
                        }
                    }
                }
            }
        }
    }
}

/// 修复模式波前网格导出（孤岛缝合 / 死锁补线寻路，Task1 修订版）：
/// - **自由端点**：指定 `net_name`（任意 pin 数，含电源地网——不受 2-pin 枚举与
///   power_net_name 过滤），from/to 为任意世界坐标起终点（越界报错不 clamp）。
/// - **孔约束墙**：`hole_clr > 0` 时启用（典型 0.15 = 华秋口径），异网 PTH/via 的
///   drill/2+hole_clr 在全部信号层 Blocked；自家网孔不封（见 mark_hole_walls）。
/// - nets 数组装**一条**合成 WavefrontNetExport：net_id 取板上该网 id（查不到 = 0），
///   start/goal = from/to 的 grid 坐标，start_mm/goal_mm = 原值。
///     铜皮/器件体编码复用既有管线（mark_component_bodies + mark_pads +
///     mark_existing_traces），目标网铜皮对求解器 passable（net-aware 规则）。
///     只读板子，不改板。
pub fn export_wavefront_grid_repair(
    board: &Board,
    layer_config: crate::layer_config::BoardLayerConfig,
    grid_res: f64,
    dilate_cells: usize,
    net_name: &str,
    from: (f64, f64),
    to: (f64, f64),
    hole_clr: f64,
) -> anyhow::Result<WavefrontGridExport> {
    let mut grid = RoutingGrid::new(board, layer_config.clone(), grid_res);

    // from/to 必须落在板 bbox 内（world_to_grid 会静默 clamp，先显式拒绝）
    let in_board = |p: (f64, f64)| -> bool {
        let (x, y) = p;
        x >= grid.origin_x
            && y >= grid.origin_y
            && x <= grid.origin_x + (grid.cols - 1) as f64 * grid.grid_res
            && y <= grid.origin_y + (grid.rows - 1) as f64 * grid.grid_res
    };
    if !in_board(from) {
        anyhow::bail!("from ({},{}) 越出板 bbox", from.0, from.1);
    }
    if !in_board(to) {
        anyhow::bail!("to ({},{}) 越出板 bbox", to.0, to.1);
    }

    grid.mark_component_bodies(board);
    grid.mark_pads(board);
    grid.mark_existing_traces(board, DEFAULT_CLEARANCE);
    // zone 填充铜（repair 特有）：refill 后的真实灌铜形态进网格，异网 zone 是墙
    mark_zone_fills(&mut grid, board);

    // 目标网 id（板上查不到则 0——纯坐标对坐标修复查询仍合法）
    let target_net = board
        .nets
        .iter()
        .find(|n| n.name == net_name)
        .map(|n| n.id)
        .unwrap_or(0);

    mark_hole_walls(&mut grid, board, target_net, hole_clr);

    let (mut grid_u32, layer_names) = encode_signal_layers(&grid)?;
    let per_layer = grid.rows * grid.cols;

    // M3 间距膨胀（与 export_wavefront_grid_dilated 同一轮实现：快照逐轮生长）
    if dilate_cells > 0 {
        const NEIGH8: [(i64, i64); 8] = [
            (0, 1),
            (0, -1),
            (1, 0),
            (-1, 0),
            (1, 1),
            (1, -1),
            (-1, 1),
            (-1, -1),
        ];
        let (cols, rows, n_layers) = (grid.cols, grid.rows, layer_names.len());
        for _ in 0..dilate_cells {
            let snap = grid_u32.clone();
            for l in 0..n_layers {
                for r in 0..rows {
                    for c in 0..cols {
                        let idx = l * per_layer + r * cols + c;
                        if snap[idx] != 0 {
                            continue; // 已占
                        }
                        let mut src = 0u32;
                        for &(dr, dc) in &NEIGH8 {
                            let (rr, cc) = (r as i64 + dr, c as i64 + dc);
                            if rr < 0 || cc < 0 || rr >= rows as i64 || cc >= cols as i64 {
                                continue;
                            }
                            let v = snap[l * per_layer + rr as usize * cols + cc as usize];
                            if v != 0 {
                                // Blocked 膨胀为 Blocked；带网身份则继承（自己网靠近自己的铜皮不受限）
                                src = v;
                                break;
                            }
                        }
                        if src != 0 {
                            grid_u32[idx] = src;
                        }
                    }
                }
            }
        }
    }

    let start = grid.world_to_grid(from.0, from.1);
    let goal = grid.world_to_grid(to.0, to.1);
    let nets = vec![WavefrontNetExport {
        net_id: target_net,
        name: net_name.to_string(),
        start,
        goal,
        start_mm: from,
        goal_mm: to,
    }];

    Ok(WavefrontGridExport {
        cols: grid.cols,
        rows: grid.rows,
        grid_res_mm: grid.grid_res,
        origin_mm: (grid.origin_x, grid.origin_y),
        layers: layer_names,
        grid_u32,
        nets,
    })
}

/// Phase B: GPU 波前路径写回——3D 网格路径转 board 线段 + 过孔。
/// meta 来自 `export_wavefront_grid`（origin/res/layers 必须与导出时一致）。
/// 候选布线纪律：产物须经本地权威 DRC（union-N）后才算交付。
pub fn commit_wavefront_path(
    board: &mut Board,
    export_meta: &WavefrontGridExport,
    net_id: u32,
    path: &[(usize, usize, usize)], // (layer_idx, row, col)
    trace_width: f64,
    via_size: f64,
    via_drill: f64,
) -> anyhow::Result<(usize, usize)> {
    let _per_layer = export_meta.cols * export_meta.rows;
    let to_world = |&(l, r, c): &(usize, usize, usize)| -> anyhow::Result<((f64, f64), String)> {
        let layer = export_meta
            .layers
            .get(l)
            .ok_or_else(|| anyhow::anyhow!("path 层索引越界: {l}"))?
            .clone();
        Ok((
            (
                export_meta.origin_mm.0 + c as f64 * export_meta.grid_res_mm,
                export_meta.origin_mm.1 + r as f64 * export_meta.grid_res_mm,
            ),
            layer,
        ))
    };

    if path.len() < 2 {
        anyhow::bail!("path 太短（<2 点）");
    }

    let mut seg_count = 0usize;
    let mut via_count = 0usize;
    for pair in path.windows(2) {
        let ((x0, y0), layer0) = to_world(&pair[0])?;
        let ((x1, y1), layer1) = to_world(&pair[1])?;
        if layer0 != layer1 {
            // 过孔落在换层点（p1 侧）
            board.vias.push(Via {
                at: (x1, y1),
                size: via_size,
                drill: via_drill,
                layers: vec![layer0.clone(), layer1.clone()],
                net: net_id,
            });
            via_count += 1;
        }
        board.segments.push(Segment {
            start: (x0, y0),
            end: (x1, y1),
            width: trace_width,
            layer: layer0,
            net: net_id,
        });
        seg_count += 1;
    }
    Ok((seg_count, via_count))
}

// ---------------------------------------------------------------------------
// M3：commit 侧线宽/间距自适应
// ---------------------------------------------------------------------------

/// 线宽自适应剖面：路径每个格到最近"异网铜皮"格的 8 向 Dijkstra 距离（层内），
/// 换算可用线宽 = 2×max(0, dist_mm − res/2 − clearance)，夹在 [min_w, default_w]。
/// 密处自动收窄（消除 clearance/shorting），宽处用足默认线宽。
///
/// 距离源 = 解码后 net ≠ net_id 的铜皮格（Pad/Trace/Via）；Blocked（板框/器件体）
/// 非电气对象不入源。返回值与 `path` 一一对应。
pub fn adaptive_path_widths(
    grid_u32: &[u32],
    cols: usize,
    rows: usize,
    layers: usize,
    net_id: u32,
    path: &[(usize, usize, usize)],
    res: f64,
    clearance: f64,
    default_w: f64,
    min_w: f64,
) -> Vec<f64> {
    let per_layer = cols * rows;
    let total = per_layer * layers;

    // 多源 BFS（0-1 边权用 Dijkstra 更准：正交 1.0 / 对角 1.414，与 kernel 距离度量一致）
    const INF: f32 = f32::MAX;
    let mut dist = vec![INF; total];
    let mut heap = std::collections::BinaryHeap::new();
    // 异网铜皮判定（与 encode_cell 编码互逆：Pad=2+n Trace=1e6+n Via=2e6+n）
    let net_of = |c: u32| -> Option<u32> {
        if c >= 2_000_000 {
            Some(c - 2_000_000)
        } else if c >= 1_000_000 {
            Some(c - 1_000_000)
        } else if c >= 2 {
            Some(c - 2)
        } else {
            None // Free / Blocked
        }
    };
    let is_other = |c: u32| net_of(c).is_some_and(|n| n != net_id);
    for (l, r, c) in
        (0..layers).flat_map(|l| (0..rows).flat_map(move |r| (0..cols).map(move |c| (l, r, c))))
    {
        let idx = l * per_layer + r * cols + c;
        if is_other(grid_u32[idx]) {
            dist[idx] = 0.0;
            heap.push((std::cmp::Reverse(0.0f32.to_bits()), idx));
        }
    }
    let (dx, dy, dc): ([i64; 8], [i64; 8], [f32; 8]) = (
        [1, -1, 0, 0, 1, 1, -1, -1],
        [0, 0, 1, -1, 1, -1, 1, -1],
        [1.0, 1.0, 1.0, 1.0, 1.414, 1.414, 1.414, 1.414],
    );
    while let Some((_, cur)) = heap.pop() {
        let l = cur / per_layer;
        let rem = cur % per_layer;
        let c0 = (rem % cols) as i64;
        let r0 = (rem / cols) as i64;
        let d0 = dist[cur];
        for i in 0..8 {
            let nc = c0 + dx[i];
            let nr = r0 + dy[i];
            if nc < 0 || nr < 0 {
                continue;
            }
            let (ncu, nru) = (nc as usize, nr as usize);
            if ncu >= cols || nru >= rows {
                continue;
            }
            let ni = l * per_layer + nru * cols + ncu;
            let nd = d0 + dc[i];
            if nd < dist[ni] {
                dist[ni] = nd;
                heap.push((std::cmp::Reverse(nd.to_bits()), ni));
            }
        }
    }

    path.iter()
        .map(|&(l, r, c)| {
            let idx = l * per_layer + r * cols + c;
            let d = if idx < total { dist[idx] } else { INF };
            let dist_mm = if d >= INF { f64::MAX } else { d as f64 * res };
            let allowed = 2.0 * (dist_mm - res / 2.0 - clearance);
            allowed.clamp(min_w, default_w)
        })
        .collect()
}
