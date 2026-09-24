//! CUDA 版波前扩展路由（feature = "cuda"）——kicad-cdb `gpu_router.rs` WGSL shader 的
//! 同构移植 + 多层扩展（过孔换层）。
//!
//! 模型：扁平 idx = layer*rows*cols + row*cols + col；同层 8 向（正交 1.0/对角 1.414），
//! 跨层过孔边（via_cost，默认 3.0）；电源/机械层构建期即 Blocked，天然不可跨入。
//! frontier 标志位 + 固定轮数 launch（每轮即隐式 barrier），轮内 RMW 竞态容忍
//! （多轮收敛，与 WGSL 版语义一致）。
//!
//! net-aware（G2）：kernel 带 `current_net`/`net_aware` 参数。net_aware=1 时
//! Pad(n)/Trace(n)/Via(n) 仅 n==current_net 可通行，其余视为障碍——已布 k 个 net
//! 后再布第 k+1 个成为合法操作（RouteGridBatch 增量模式的前提）。
//! net_aware=0 保持旧语义（除 Blocked 外全通），与已验证的 47/47 行为位级一致。

#[cfg(feature = "cuda")]
use anyhow::Context;

/// 多层波前扩展 kernel（与 WGSL wavefront_expand 同构 + 跨层过孔边 + net-aware 过滤）
#[allow(dead_code)] // cuda feature 专用
const WAVEFRONT_CUDA: &str = r#"
inline __device__ unsigned int passable(unsigned int cell, unsigned int current_net, unsigned int net_aware) {
    if (cell == 0u) return 1u;   // CELL_FREE
    if (cell == 1u) return 0u;   // CELL_BLOCKED
    if (net_aware == 0u) return 1u;  // 旧语义：其它 net 的 Pad/Trace/Via 全通
    unsigned int n = (cell >= 2000000u) ? (cell - 2000000u)
                   : (cell >= 1000000u) ? (cell - 1000000u)
                                        : (cell - 2u);
    return n == current_net ? 1u : 0u;
}

extern "C" __global__ void wavefront_expand(
    const unsigned int* grid,
    float* cost,
    unsigned int* came_from,
    unsigned int* frontier,
    unsigned int* active,
    unsigned int cols,
    unsigned int rows,
    unsigned int per_layer,
    unsigned int layers,
    float via_cost,
    unsigned int current_net,
    unsigned int net_aware)
{
    unsigned int idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (idx >= per_layer * layers) return;
    if (frontier[idx] == 0u) return;
    atomicAdd(active, 1u);

    unsigned int layer = idx / per_layer;
    unsigned int ridx  = idx % per_layer;
    unsigned int col = ridx % cols;
    unsigned int row = ridx / cols;
    float current_cost = cost[idx];

    const int dx[8] = {1, -1, 0, 0, 1, 1, -1, -1};
    const int dy[8] = {0, 0, 1, -1, 1, -1, 1, -1};
    const float mc[8] = {1.0f, 1.0f, 1.0f, 1.0f, 1.414f, 1.414f, 1.414f, 1.414f};

    // 同层 8 向
    for (int i = 0; i < 8; i++) {
        int nc = (int)col + dx[i];
        int nr = (int)row + dy[i];
        if (nc < 0 || nr < 0) continue;
        unsigned int ncu = (unsigned int)nc;
        unsigned int nru = (unsigned int)nr;
        if (ncu >= cols || nru >= rows) continue;

        unsigned int ni = layer * per_layer + nru * cols + ncu;
        if (passable(grid[ni], current_net, net_aware) == 0u) continue;

        float new_cost = current_cost + mc[i];
        if (new_cost < cost[ni]) {
            cost[ni] = new_cost;
            came_from[ni] = idx;
            atomicExch(&frontier[ni], 1u);
        }
    }

    // 跨层（过孔）：电源/机械层构建期即 Blocked，天然不可跨入
    if (layer + 1u < layers) {
        unsigned int ni = idx + per_layer;
        if (passable(grid[ni], current_net, net_aware) != 0u) {
            float new_cost = current_cost + via_cost;
            if (new_cost < cost[ni]) {
                cost[ni] = new_cost;
                came_from[ni] = idx;
                atomicExch(&frontier[ni], 1u);
            }
        }
    }
    if (layer >= 1u) {
        unsigned int ni = idx - per_layer;
        if (passable(grid[ni], current_net, net_aware) != 0u) {
            float new_cost = current_cost + via_cost;
            if (new_cost < cost[ni]) {
                cost[ni] = new_cost;
                came_from[ni] = idx;
                atomicExch(&frontier[ni], 1u);
            }
        }
    }
    atomicExch(&frontier[idx], 0u);
}
"#;

#[derive(Debug, Clone, PartialEq)]
pub struct RoutedPath3D {
    /// (layer, row, col) 起点→终点；相邻 layer 变化即过孔
    pub path: Vec<(usize, usize, usize)>,
    pub cost: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct GridSpec {
    pub cols: usize,
    pub rows: usize,
    pub layers: usize,
    pub via_cost: f32,
}

/// 批量路由的单个 net 查询（start/goal 均为 (layer,row,col)）
#[derive(Debug, Clone)]
pub struct BatchNetQuery {
    pub net_id: u32,
    pub sl: usize,
    pub sr: usize,
    pub sc: usize,
    pub gl: usize,
    pub gr: usize,
    pub gc: usize,
}

/// 批量路由的单个 net 结果
#[derive(Debug, Clone)]
pub struct BatchNetOutcome {
    pub net_id: u32,
    pub routed: bool,
    pub cost: f32,
    pub path: Vec<(usize, usize, usize)>,
    pub rounds: u32,
}

#[cfg(feature = "cuda")]
struct WavefrontKernel {
    stream: std::sync::Arc<cudarc::driver::CudaStream>,
    func: cudarc::driver::CudaFunction,
    dev_grid: cudarc::driver::CudaSlice<u32>,
    dev_cost: cudarc::driver::CudaSlice<f32>,
    dev_came_from: cudarc::driver::CudaSlice<u32>,
    dev_frontier: cudarc::driver::CudaSlice<u32>,
    dev_active: cudarc::driver::CudaSlice<u32>,
    total: usize,
    cfg: cudarc::driver::LaunchConfig,
    dims: (u32, u32, u32, u32), // cols, rows, per_layer, layers
}

#[cfg(feature = "cuda")]
impl WavefrontKernel {
    /// 一次性 nvrtc 编译 + 显存分配（批量复用，摊销 47 次往返的编译/建流开销）
    fn new(ordinal: usize, spec: GridSpec) -> anyhow::Result<Self> {
        use cudarc::driver::{CudaContext, LaunchConfig, PushKernelArg};
        use cudarc::nvrtc::compile_ptx;

        let per_layer = spec.cols * spec.rows;
        let total = per_layer * spec.layers;
        let ptx = compile_ptx(WAVEFRONT_CUDA)?;
        let ctx = CudaContext::new(ordinal)?;
        let stream = ctx.default_stream();
        let module = ctx.load_module(ptx)?;
        let func = module.load_function("wavefront_expand")?;

        let dev_grid = stream.alloc_zeros::<u32>(total)?;
        let dev_cost = stream.alloc_zeros::<f32>(total)?;
        let dev_came_from = stream.alloc_zeros::<u32>(total)?;
        let dev_frontier = stream.alloc_zeros::<u32>(total)?;
        let dev_active = stream.alloc_zeros::<u32>(1)?;

        let workgroups = (total as u32 + 255) / 256;
        let cfg = LaunchConfig {
            grid_dim: (workgroups, 1, 1),
            block_dim: (256, 1, 1),
            shared_mem_bytes: 0,
        };
        let dims = (
            spec.cols as u32,
            spec.rows as u32,
            per_layer as u32,
            spec.layers as u32,
        );
        Ok(Self {
            stream,
            func,
            dev_grid,
            dev_cost,
            dev_came_from,
            dev_frontier,
            dev_active,
            total,
            cfg,
            dims,
        })
    }

    /// 在共享 kernel/buffer 上解一个 net；grid_host 为当前 grid 快照（net-aware
    /// 模式下调用方在写回后传入更新值）。返回 (cost, 3D 路径, 实际轮数)。
    #[allow(clippy::too_many_arguments)]
    fn route_one(
        &mut self,
        grid_host: &[u32],
        spec: GridSpec,
        start: (usize, usize, usize),
        goal: (usize, usize, usize),
        current_net: u32,
        net_aware: bool,
        max_rounds: u32,
    ) -> anyhow::Result<Option<(f32, Vec<(usize, usize, usize)>, u32)>> {
        use cudarc::driver::PushKernelArg;

        let per_layer = spec.cols * spec.rows;
        let total = self.total;
        let start_idx = start.0 * per_layer + start.1 * spec.cols + start.2;
        let goal_idx = goal.0 * per_layer + goal.1 * spec.cols + goal.2;
        if start_idx >= total || goal_idx >= total {
            return Ok(None);
        }
        // 起终点自身必须可通行（net-aware 下 Pad(own) 通过，Pad(other)/Blocked 拒单）
        if !cell_passable(grid_host[start_idx], current_net, net_aware)
            || !cell_passable(grid_host[goal_idx], current_net, net_aware)
        {
            return Ok(None);
        }

        // 每 net 全量重置：cost=MAX 起点置 0；frontier 清零起点置 1；grid 按快照重传。
        // came_from 不重传——重建路径只走本 net 内被更新过的 cell（cost 与 came_from
        // 在 kernel 中同条件写入，陈旧值不可达），与单发版本语义一致。
        let mut cost: Vec<f32> = vec![f32::MAX; total];
        let mut frontier: Vec<u32> = vec![0; total];
        cost[start_idx] = 0.0;
        frontier[start_idx] = 1;
        self.stream.memcpy_htod(grid_host, &mut self.dev_grid)?;
        self.stream.memcpy_htod(&cost, &mut self.dev_cost)?;
        self.stream.memcpy_htod(&frontier, &mut self.dev_frontier)?;

        let (cols_u, rows_u, pl_u, layers_u) = self.dims;
        let net_u = current_net;
        let aware_u = net_aware as u32;
        let mut rounds_used = 0u32;
        for round in 0..max_rounds {
            self.stream.memset_zeros(&mut self.dev_active)?;
            unsafe {
                self.stream
                    .launch_builder(&self.func)
                    .arg(&self.dev_grid)
                    .arg(&mut self.dev_cost)
                    .arg(&mut self.dev_came_from)
                    .arg(&mut self.dev_frontier)
                    .arg(&mut self.dev_active)
                    .arg(&cols_u)
                    .arg(&rows_u)
                    .arg(&pl_u)
                    .arg(&layers_u)
                    .arg(&spec.via_cost)
                    .arg(&net_u)
                    .arg(&aware_u)
                    .launch(self.cfg)?;
            }
            self.stream.synchronize()?;
            rounds_used = round + 1;
            let active: Vec<u32> = self.stream.memcpy_dtov(&self.dev_active)?;
            if active[0] == 0 {
                break; // 本轮无任何 frontier 扩散 = 已收敛
            }
        }

        let mut out_cost = vec![0.0f32; total];
        let mut out_came_from = vec![0u32; total];
        self.stream.memcpy_dtoh(&self.dev_cost, &mut out_cost)?;
        self.stream
            .memcpy_dtoh(&self.dev_came_from, &mut out_came_from)?;

        if out_cost[goal_idx] == f32::MAX {
            return Ok(None);
        }

        // 路径重建（3D）
        let mut path = vec![goal];
        let mut current = goal_idx;
        let mut steps = 0;
        while current != start_idx && steps < total {
            let prev = out_came_from[current] as usize;
            if prev == u32::MAX as usize || prev >= total || prev == current {
                break;
            }
            let layer = prev / per_layer;
            let ridx = prev % per_layer;
            path.push((layer, ridx / spec.cols, ridx % spec.cols));
            current = prev;
            steps += 1;
        }
        path.reverse();

        Ok(Some((out_cost[goal_idx], path, rounds_used)))
    }
}

/// 与 kernel 内 passable 同构的主机侧判定（起终点预检 + CPU 对拍参照共用）
pub fn cell_passable(cell: u32, current_net: u32, net_aware: bool) -> bool {
    match cell {
        0 => true,
        1 => false,
        _ if !net_aware => true,
        c => {
            let n = if c >= 2_000_000 {
                c - 2_000_000
            } else if c >= 1_000_000 {
                c - 1_000_000
            } else {
                c - 2
            };
            n == current_net
        }
    }
}

// ---------------------------------------------------------------------------
// 引擎阶梯：cuda（生产 4090）→ wgpu（本机 Metal/Vulkan/DX12）→ cpu（兜底=对拍参照）
// ---------------------------------------------------------------------------

/// 求解引擎档位（阶梯序：先 CUDA 后 wgpu，CPU 永远兜底）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineKind {
    Cuda,
    Wgpu,
    Cpu,
}

pub fn engine_name(kind: EngineKind) -> &'static str {
    match kind {
        EngineKind::Cuda => "cuda",
        EngineKind::Wgpu => "wgpu",
        EngineKind::Cpu => "cpu",
    }
}

/// 运行时探测引擎阶梯（进程内粘性：探测一次）。
/// CUDA 探测用 catch_unwind——cudarc 在驱动库缺失时是 panic 不是 Err（见 cuda.rs 教训）。
pub fn resolve_ladder() -> &'static [EngineKind] {
    use std::sync::OnceLock;
    static LADDER: OnceLock<Vec<EngineKind>> = OnceLock::new();
    LADDER.get_or_init(|| {
        // 特性分级构造: CUDA(生产) → wgpu(本机) → CPU(兜底), 迭代器链避免 init-then-push
        #[cfg(feature = "cuda")]
        let cuda: Option<EngineKind> = {
            let ok = std::panic::catch_unwind(|| {
                cudarc::driver::result::init().is_ok()
                    && cudarc::driver::result::device::get_count()
                        .map(|n| n > 0)
                        .unwrap_or(false)
            })
            .unwrap_or(false);
            if ok {
                Some(EngineKind::Cuda)
            } else {
                None
            }
        };
        #[cfg(not(feature = "cuda"))]
        let cuda: Option<EngineKind> = None;
        #[cfg(feature = "gpu")]
        let wgpu: Option<EngineKind> = if crate::wavefront_wgpu::available() {
            Some(EngineKind::Wgpu)
        } else {
            None
        };
        #[cfg(not(feature = "gpu"))]
        let wgpu: Option<EngineKind> = None;
        let ladder: Vec<EngineKind> = cuda
            .into_iter()
            .chain(wgpu)
            .chain(std::iter::once(EngineKind::Cpu))
            .collect();
        eprintln!(
            "[wavefront] 引擎阶梯: {}",
            ladder
                .iter()
                .map(|k| engine_name(*k))
                .collect::<Vec<_>>()
                .join(" → ")
        );
        ladder
    })
}

/// 旧语义多层波前（除 Blocked 外全通），指定引擎。单层即 layers=1 的特例。
#[allow(clippy::too_many_arguments)]
pub fn route_layers_on(
    kind: EngineKind,
    grid_u32: &[u32],
    spec: GridSpec,
    start: (usize, usize, usize), // (layer, row, col)
    goal: (usize, usize, usize),
    max_rounds: u32,
    ordinal: usize,
) -> anyhow::Result<Option<RoutedPath3D>> {
    let mut g = grid_u32.to_vec();
    let q = [BatchNetQuery {
        net_id: 0,
        sl: start.0,
        sr: start.1,
        sc: start.2,
        gl: goal.0,
        gr: goal.1,
        gc: goal.2,
    }];
    let outcomes = run_batch_on(kind, &mut g, spec, &q, false, max_rounds, ordinal)?;
    Ok(outcomes
        .into_iter()
        .next()
        .filter(|o| o.routed)
        .map(|o| RoutedPath3D {
            path: o.path,
            cost: o.cost,
        }))
}

/// 批量路由（CUDA 实现；经 run_batch_on 调度）
#[cfg(feature = "cuda")]
pub fn route_batch_cuda(
    grid_host: &mut Vec<u32>,
    spec: GridSpec,
    nets: &[BatchNetQuery],
    net_aware: bool,
    max_rounds: u32,
    ordinal: usize,
) -> anyhow::Result<Vec<BatchNetOutcome>> {
    let mut kernel = WavefrontKernel::new(ordinal, spec)?;
    let max_rounds = if max_rounds == 0 {
        ((spec.cols + spec.rows) * spec.layers) as u32
    } else {
        max_rounds
    };
    let mut out = Vec::with_capacity(nets.len());
    for net in nets {
        let (sl, sr, sc) = (net.sl, net.sr, net.sc);
        let (gl, gr, gc) = (net.gl, net.gr, net.gc);
        let res = kernel.route_one(
            grid_host,
            spec,
            (sl, sr, sc),
            (gl, gr, gc),
            net.net_id,
            net_aware,
            max_rounds,
        )?;
        match res {
            Some((cost, path, rounds)) => {
                if net_aware {
                    write_back_path(grid_host, spec, net.net_id, &path);
                }
                out.push(BatchNetOutcome {
                    net_id: net.net_id,
                    routed: true,
                    cost,
                    path,
                    rounds,
                });
            }
            None => out.push(BatchNetOutcome {
                net_id: net.net_id,
                routed: false,
                cost: 0.0,
                path: Vec::new(),
                rounds: max_rounds,
            }),
        }
    }
    Ok(out)
}

/// 批量路由（CPU 实现；永远可用，同时是全部 GPU 引擎的对拍参照）
#[allow(clippy::ptr_arg)] // 网格缓冲宿主向量, 与 GPU 侧签名对称
pub fn route_batch_cpu(
    grid_host: &mut Vec<u32>,
    spec: GridSpec,
    nets: &[BatchNetQuery],
    net_aware: bool,
    max_rounds: u32,
    _ordinal: usize,
) -> anyhow::Result<Vec<BatchNetOutcome>> {
    let max_rounds = if max_rounds == 0 {
        ((spec.cols + spec.rows) * spec.layers) as u32
    } else {
        max_rounds
    };
    let mut out = Vec::with_capacity(nets.len());
    for net in nets {
        let res = dijkstra_3d_path(
            grid_host,
            spec.layers,
            spec.cols,
            spec.rows,
            (net.sl, net.sr, net.sc),
            (net.gl, net.gr, net.gc),
            spec.via_cost,
            net.net_id,
            net_aware,
        );
        match res {
            Some((cost, path)) => {
                if net_aware {
                    write_back_path(grid_host, spec, net.net_id, &path);
                }
                out.push(BatchNetOutcome {
                    net_id: net.net_id,
                    routed: true,
                    cost,
                    path,
                    rounds: 0,
                });
            }
            None => out.push(BatchNetOutcome {
                net_id: net.net_id,
                routed: false,
                cost: 0.0,
                path: Vec::new(),
                rounds: max_rounds,
            }),
        }
    }
    Ok(out)
}

/// net-aware 路径写回：内部 cell → Trace(net)，相对前驱换层点 → Via(net)。
/// 起终点 pad 保持原样（本 net 的电气端点）。全部引擎实现共用。
pub(crate) fn write_back_path(
    grid_host: &mut [u32],
    spec: GridSpec,
    net_id: u32,
    path: &[(usize, usize, usize)],
) {
    let per_layer = spec.cols * spec.rows;
    for (i, cell) in path.iter().enumerate().skip(1) {
        if i + 1 >= path.len() {
            break;
        }
        let is_via = cell.0 != path[i - 1].0;
        let flat = cell.0 * per_layer + cell.1 * spec.cols + cell.2;
        grid_host[flat] = if is_via {
            2_000_000 + net_id
        } else {
            1_000_000 + net_id
        };
    }
}

/// 指定引擎批量求解（引擎未编译进本构建时显式报错，不静默降级）
pub fn run_batch_on(
    kind: EngineKind,
    grid_host: &mut Vec<u32>,
    spec: GridSpec,
    nets: &[BatchNetQuery],
    net_aware: bool,
    max_rounds: u32,
    ordinal: usize,
) -> anyhow::Result<Vec<BatchNetOutcome>> {
    match kind {
        EngineKind::Cuda => {
            #[cfg(feature = "cuda")]
            {
                route_batch_cuda(grid_host, spec, nets, net_aware, max_rounds, ordinal)
            }
            #[cfg(not(feature = "cuda"))]
            {
                let _ = (grid_host, spec, nets, net_aware, max_rounds, ordinal);
                anyhow::bail!("cuda 引擎未编译进本构建（--features cuda）")
            }
        }
        EngineKind::Wgpu => {
            #[cfg(feature = "gpu")]
            {
                crate::wavefront_wgpu::route_batch(grid_host, spec, nets, net_aware, max_rounds)
            }
            #[cfg(not(feature = "gpu"))]
            {
                let _ = (grid_host, spec, nets, net_aware, max_rounds);
                anyhow::bail!("wgpu 引擎未编译进本构建（--features gpu，默认开）")
            }
        }
        EngineKind::Cpu => route_batch_cpu(grid_host, spec, nets, net_aware, max_rounds, ordinal),
    }
}

/// 批量路由结果（含实际使用的引擎名，RPC 回复透传审计）
pub struct BatchRun {
    pub outcomes: Vec<BatchNetOutcome>,
    pub engine: &'static str,
}

/// 批量路由（阶梯调度）：按 ladder 顺序尝试，单引擎失败自动降级下一档
pub fn route_batch(
    grid_host: &mut Vec<u32>,
    spec: GridSpec,
    nets: &[BatchNetQuery],
    net_aware: bool,
    max_rounds: u32,
    ordinal: usize,
) -> anyhow::Result<BatchRun> {
    let mut last_err: Option<String> = None;
    for &kind in resolve_ladder() {
        match run_batch_on(kind, grid_host, spec, nets, net_aware, max_rounds, ordinal) {
            Ok(outcomes) => {
                return Ok(BatchRun {
                    outcomes,
                    engine: engine_name(kind),
                });
            }
            Err(e) => {
                eprintln!(
                    "[wavefront] 引擎 {} 求解失败，降级下一档: {e:#}",
                    engine_name(kind)
                );
                last_err = Some(format!("{e:#}"));
            }
        }
    }
    anyhow::bail!("引擎阶梯全部失败: {}", last_err.unwrap_or_default())
}

// ---------------------------------------------------------------------------
// 自检：单层确定性例（回归）+ 多层墙（过孔救活）+ 3D 随机对拍 CPU Dijkstra
// ---------------------------------------------------------------------------

/// 对指定引擎跑全套对拍用例（CPU Dijkstra 参照；Cpu 引擎即参照本身，不进套件）
fn engine_suite(kind: EngineKind, ordinal: usize) -> anyhow::Result<String> {
    let mut failures: Vec<String> = Vec::new();
    let mut checks = 0usize;

    // 用例 1：5x5 单层竖墙（回归：layers=1 与旧版一致）
    {
        let (cols, rows) = (5usize, 5usize);
        let mut grid = vec![0u32; cols * rows];
        for r in 0..3 {
            grid[r * cols + 2] = 1;
        }
        for (start, goal) in [
            ((0usize, 2usize), (4usize, 2usize)),
            ((0, 0), (4, 4)),
            ((4, 0), (0, 4)),
        ] {
            checks += 1;
            let want = dijkstra_3d(
                &grid,
                1,
                cols,
                rows,
                (0, start.1, start.0),
                (0, goal.1, goal.0),
                3.0,
                0,
                false,
            );
            let got = route_layers_on(
                kind,
                &grid,
                GridSpec {
                    cols,
                    rows,
                    layers: 1,
                    via_cost: 3.0,
                },
                (0, start.1, start.0),
                (0, goal.1, goal.0),
                64,
                ordinal,
            )?
            .map(|r| r.cost);
            if want != got {
                failures.push(format!(
                    "case1 {start:?}->{goal:?}: GPU {got:?} vs CPU {want:?}"
                ));
            }
        }
    }

    // 用例 2：双层全高墙——首层完全隔断，必须过孔换层
    {
        let (cols, rows, layers) = (6usize, 4usize, 2usize);
        let mut grid = vec![0u32; layers * cols * rows];
        for r in 0..rows {
            grid[r * cols + 3] = 1; // 首层 Blocked 墙（第二层无此墙）
        }
        for (start, goal) in [
            ((0usize, 1usize, 0usize), (1usize, 1usize, 5usize)),
            ((0, 1, 1), (1, 1, 4)),
        ] {
            checks += 1;
            let want = dijkstra_3d(&grid, layers, cols, rows, start, goal, 3.0, 0, false);
            let got = route_layers_on(
                kind,
                &grid,
                GridSpec {
                    cols,
                    rows,
                    layers,
                    via_cost: 3.0,
                },
                start,
                goal,
                128,
                ordinal,
            )?
            .map(|r| r.cost);
            if want != got {
                failures.push(format!(
                    "case2 {start:?}->{goal:?}: GPU {got:?} vs CPU {want:?}"
                ));
            }
        }
    }

    // 用例 3：3 层 32x32 随机障碍（xorshift 固定 seed）vs CPU 3D Dijkstra
    {
        let (cols, rows, layers) = (32usize, 32usize, 3usize);
        let mut seed: u64 = 0x9E3779B97F4A7C15;
        let mut rnd = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let grid: Vec<u32> = (0..layers * cols * rows)
            .map(|_| (rnd() % 100 < 15) as u32)
            .collect();
        let cases = [
            ((0usize, 0usize, 0usize), (2usize, 31usize, 31usize)),
            ((1, 5, 5), (0, 25, 20)),
            ((2, 0, 16), (0, 16, 0)),
        ];
        for (start, goal) in cases {
            checks += 1;
            let want = dijkstra_3d(&grid, layers, cols, rows, start, goal, 3.0, 0, false);
            let got = route_layers_on(
                kind,
                &grid,
                GridSpec {
                    cols,
                    rows,
                    layers,
                    via_cost: 3.0,
                },
                start,
                goal,
                ((cols + rows) * layers) as u32,
                ordinal,
            )?
            .map(|r| r.cost);
            match (want, got) {
                (None, None) => {}
                (Some(w), Some(g)) if (w - g).abs() < 1e-3 => {}
                _ => failures.push(format!(
                    "case3 {start:?}->{goal:?}: GPU {got:?} vs CPU {want:?}"
                )),
            }
        }
    }

    // 用例 4（net-aware 单层）：竖墙 + 唯一绕行格被其它 net 的 Trace 占据。
    // 旧语义可绕（全通），net-aware（current_net=5）不可达——归属过滤生效。
    {
        let (cols, rows) = (5usize, 5usize);
        let mut grid = vec![0u32; cols * rows];
        for r in 0..4 {
            grid[r * cols + 2] = 1; // col2 墙（row0..3）
        }
        grid[4 * cols + 2] = 1_000_000 + 7u32; // 唯一穿越点 = net7 的 Trace
        let spec = GridSpec {
            cols,
            rows,
            layers: 1,
            via_cost: 3.0,
        };
        let (start, goal) = ((0usize, 2usize, 0usize), (0usize, 2usize, 4usize));
        checks += 2;
        let want_legacy = dijkstra_3d(&grid, 1, cols, rows, start, goal, 3.0, 0, false);
        let got_legacy =
            route_layers_on(kind, &grid, spec, start, goal, 64, ordinal)?.map(|r| r.cost);
        if want_legacy != got_legacy {
            failures.push(format!(
                "case4-legacy: GPU {got_legacy:?} vs CPU {want_legacy:?}"
            ));
        }
        let want_aware = dijkstra_3d(&grid, 1, cols, rows, start, goal, 3.0, 5, true);
        let got_aware = run_batch_on(
            kind,
            &mut grid.clone(),
            spec,
            &[BatchNetQuery {
                net_id: 5,
                sl: start.0,
                sr: start.1,
                sc: start.2,
                gl: goal.0,
                gr: goal.1,
                gc: goal.2,
            }],
            true,
            64,
            ordinal,
        )?
        .into_iter()
        .next()
        .map(|o| o.routed.then_some(o.cost));
        if want_aware != got_aware.flatten() {
            failures.push(format!(
                    "case4-aware: GPU {got_aware:?} vs CPU {want_aware:?}（应为 None：绕行被 net7 占据）"
                ));
        }
    }

    // 用例 5（net-aware 多层 + 批量写回）：首层唯一走廊被墙截断必须过孔换层；
    // net1 布通后其 Trace/Via 写回 grid，net2 同端点被完全阻断（走廊无替代路径）。
    {
        let (cols, rows, layers) = (5usize, 1usize, 2usize);
        let mut grid = vec![0u32; layers * cols * rows];
        grid[2] = 1; // 首层 col2 墙（rows=1 → 全高）；第二层无此墙
        checks += 3;
        let spec = GridSpec {
            cols,
            rows,
            layers,
            via_cost: 3.0,
        };
        let want = dijkstra_3d(
            &grid,
            layers,
            cols,
            rows,
            (0, 0, 0),
            (0, 0, 4),
            3.0,
            1,
            true,
        );
        let mut g1 = grid.clone();
        let out1 = run_batch_on(
            kind,
            &mut g1,
            spec,
            &[BatchNetQuery {
                net_id: 1,
                sl: 0,
                sr: 0,
                sc: 0,
                gl: 0,
                gr: 0,
                gc: 4,
            }],
            true,
            128,
            ordinal,
        )?;
        let c1 = out1[0].routed.then_some(out1[0].cost);
        if want != c1 {
            failures.push(format!("case5-net1: GPU {c1:?} vs CPU {want:?}"));
        }
        if out1[0].routed {
            // 写回完整性：路径内部 cell 全是 Trace(1)/Via(1)，且必须含过孔（换层）
            let path = &out1[0].path;
            let mut via_seen = false;
            let interior_ok = path
                .iter()
                .enumerate()
                .skip(1)
                .take(path.len().saturating_sub(2))
                .all(|(i, c)| {
                    let flat = c.0 * cols * rows + c.1 * cols + c.2;
                    let v = g1[flat];
                    if c.0 != path[i - 1].0 {
                        via_seen = true;
                    }
                    v == 1_000_000 + 1 || v == 2_000_000 + 1
                });
            if !interior_ok {
                failures
                    .push("case5-writeback: 路径内部 cell 未全部写回 Trace(1)/Via(1)".to_string());
            }
            if !via_seen {
                failures
                    .push("case5-writeback: 首层被墙截断却未出现换层（Via 写回缺失）".to_string());
            }
            // net2 同端点：走廊被 net1 写回路径完全占用 → 不可达（必须在 g1 上跑，
            // 原始 grid 没有 net1 的铜皮——此行曾在 M1 写成 grid.clone()，Metal 首跑才暴露）
            let out2 = run_batch_on(
                kind,
                &mut g1,
                spec,
                &[BatchNetQuery {
                    net_id: 2,
                    sl: 0,
                    sr: 0,
                    sc: 0,
                    gl: 0,
                    gr: 0,
                    gc: 4,
                }],
                true,
                128,
                ordinal,
            )?;
            if out2[0].routed {
                failures.push(format!(
                    "case5-net2: 应被 net1 写回路径阻断，实际 routed cost={}",
                    out2[0].cost
                ));
            }
        }
    }

    // 用例 6（归属过滤·端点与穿越）：row0 是唯一走廊，Pad(9) 挡在中间。
    // net5 从 Pad(5) 出发穿不过 Pad(9) → None；net9 从 Pad(5) 起步 → 起点拒单；
    // net9 从 Pad(9) 后一格出发可直达（穿越自己 pad 合法）。
    {
        let (cols, rows) = (5usize, 5usize);
        let mut grid = vec![0u32; cols * rows];
        for r in 1..rows {
            for c in 0..cols {
                grid[r * cols + c] = 1; // 只有 row0 是走廊
            }
        }
        grid[1] = 2 + 5u32; // Pad(5), layer 0
        grid[3] = 2 + 9u32; // Pad(9) 挡路, layer 0
        let spec = GridSpec {
            cols,
            rows,
            layers: 1,
            via_cost: 3.0,
        };
        let q = |net_id: u32, sc: usize, gc: usize| BatchNetQuery {
            net_id,
            sl: 0,
            sr: 0,
            sc,
            gl: 0,
            gr: 0,
            gc,
        };
        checks += 3;
        let want5 = dijkstra_3d(&grid, 1, cols, rows, (0, 0, 1), (0, 0, 4), 3.0, 5, true);
        let got5 = run_batch_on(
            kind,
            &mut grid.clone(),
            spec,
            &[q(5, 1, 4)],
            true,
            64,
            ordinal,
        )?
        .into_iter()
        .next()
        .map(|o| o.routed.then_some(o.cost));
        if want5 != got5.flatten() {
            failures.push(format!(
                "case6-net5: GPU {got5:?} vs CPU {want5:?}（应为 None：穿越 Pad(9) 被归属过滤）"
            ));
        }
        let got_start_other = run_batch_on(
            kind,
            &mut grid.clone(),
            spec,
            &[q(9, 1, 4)],
            true,
            64,
            ordinal,
        )?
        .into_iter()
        .next()
        .map(|o| o.routed);
        if got_start_other != Some(false) {
            failures.push(format!(
                "case6-net9-start: net9 从 Pad(5) 起步应拒单，实际 {got_start_other:?}"
            ));
        }
        let got9 = run_batch_on(
            kind,
            &mut grid.clone(),
            spec,
            &[q(9, 2, 4)],
            true,
            64,
            ordinal,
        )?
        .into_iter()
        .next()
        .map(|o| o.routed.then_some(o.cost));
        if got9.flatten().is_none() {
            failures.push("case6-net9: net9 从 (2,0) 穿越自身 Pad(9) 到 (4,0) 应可达".to_string());
        }
    }

    if failures.is_empty() {
        Ok(format!(
            "OK {checks} wavefront cases match CPU Dijkstra (含多层过孔/net-aware 三组)"
        ))
    } else {
        Ok(format!(
            "FAIL {}/{} cases: {}",
            failures.len(),
            checks,
            failures.join("; ")
        ))
    }
}

/// CPU 3D Dijkstra 参照/回退（同层 8 向 + 跨层过孔），返回 (cost, 3D 路径)。
/// passable 规则与 kernel 完全同构（cell_passable），对拍自检与 CPU 回退共用。
#[allow(clippy::too_many_arguments, clippy::type_complexity)] // 3D 网格求解器上下文
fn dijkstra_3d_path(
    grid: &[u32],
    layers: usize,
    cols: usize,
    rows: usize,
    start: (usize, usize, usize),
    goal: (usize, usize, usize),
    via_cost: f32,
    current_net: u32,
    net_aware: bool,
) -> Option<(f32, Vec<(usize, usize, usize)>)> {
    use std::collections::BinaryHeap;
    let per_layer = cols * rows;
    let idx = |l: usize, r: usize, c: usize| l * per_layer + r * cols + c;
    let (sl, sr, sc) = start;
    let (gl, gr, gc) = goal;
    let (si, gi) = (idx(sl, sr, sc), idx(gl, gr, gc));
    if !cell_passable(grid[si], current_net, net_aware)
        || !cell_passable(grid[gi], current_net, net_aware)
    {
        return None;
    }
    let total = layers * per_layer;
    let mut dist = vec![f32::MAX; total];
    let mut came_from = vec![u32::MAX; total];
    let mut heap = BinaryHeap::new();
    dist[si] = 0.0;
    heap.push((std::cmp::Reverse(0.0f32.to_bits()), si));
    let dx = [1i32, -1, 0, 0, 1, 1, -1, -1];
    let dy = [0i32, 0, 1, -1, 1, -1, 1, -1];
    let mc = [1.0f32, 1.0, 1.0, 1.0, 1.414, 1.414, 1.414, 1.414];
    while let Some((_, cur)) = heap.pop() {
        if cur == gi {
            // 路径重建（3D）
            let mut path = vec![goal];
            let mut p = gi;
            let mut steps = 0;
            while p != si && steps < total {
                let prev = came_from[p] as usize;
                if prev == u32::MAX as usize || prev >= total || prev == p {
                    break;
                }
                let layer = prev / per_layer;
                let rem = prev % per_layer;
                path.push((layer, rem / cols, rem % cols));
                p = prev;
                steps += 1;
            }
            path.reverse();
            return Some((dist[gi], path));
        }
        let layer = cur / per_layer;
        let rem = cur % per_layer;
        let cc = (rem % cols) as i32;
        let cr = (rem / cols) as i32;
        for i in 0..8 {
            let nc = cc + dx[i];
            let nr = cr + dy[i];
            if nc < 0 || nr < 0 {
                continue;
            }
            let (ncu, nru) = (nc as usize, nr as usize);
            if ncu >= cols || nru >= rows {
                continue;
            }
            let ni = layer * per_layer + nru * cols + ncu;
            if !cell_passable(grid[ni], current_net, net_aware) {
                continue;
            }
            let nd = dist[cur] + mc[i];
            if nd < dist[ni] {
                dist[ni] = nd;
                came_from[ni] = cur as u32;
                heap.push((std::cmp::Reverse(nd.to_bits()), ni));
            }
        }
        for nl in [layer.checked_sub(1), Some(layer + 1)] {
            let Some(nl) = nl else { continue };
            if nl >= layers {
                continue;
            }
            let ni = idx(nl, rem / cols, rem % cols);
            if !cell_passable(grid[ni], current_net, net_aware) {
                continue;
            }
            let nd = dist[cur] + via_cost;
            if nd < dist[ni] {
                dist[ni] = nd;
                came_from[ni] = cur as u32;
                heap.push((std::cmp::Reverse(nd.to_bits()), ni));
            }
        }
    }
    None
}

/// 自检对拍用 cost-only 包装
#[allow(clippy::too_many_arguments)]
fn dijkstra_3d(
    grid: &[u32],
    layers: usize,
    cols: usize,
    rows: usize,
    start: (usize, usize, usize),
    goal: (usize, usize, usize),
    via_cost: f32,
    current_net: u32,
    net_aware: bool,
) -> Option<f32> {
    dijkstra_3d_path(
        grid,
        layers,
        cols,
        rows,
        start,
        goal,
        via_cost,
        current_net,
        net_aware,
    )
    .map(|(cost, _)| cost)
}

/// 引擎阶梯自检：逐 GPU 引擎跑全套对拍用例（CPU 即参照，不入套件）。
/// health 常驻调用——任何引擎的规则漂移都会在这里被 CPU 参照抓出来。
pub fn wavefront_self_test(ordinal: usize) -> anyhow::Result<String> {
    let mut parts: Vec<String> = Vec::new();
    for &kind in resolve_ladder() {
        if kind == EngineKind::Cpu {
            continue;
        }
        let name = engine_name(kind);
        match engine_suite(kind, ordinal) {
            Ok(msg) => parts.push(format!("{name}: {msg}")),
            Err(e) => parts.push(format!("{name}: ERR {e:#}")),
        }
    }
    if parts.is_empty() {
        return Ok("no GPU engine（阶梯=cpu，cpu 即对拍参照）".into());
    }
    Ok(parts.join(" | "))
}

// ---------------------------------------------------------------------------
// Task1 修订版：NO-PATH 封锁归因（CPU 全空间扩展，修复寻路专用）
// ---------------------------------------------------------------------------

/// NO-PATH 封锁归因簇：可达集与非可达障碍格的边界 → 连通环簇。
/// 全部为 grid 坐标；世界坐标 = origin + idx*res，由客户端（meta 持有方）换算。
#[derive(Debug, Clone, PartialEq)]
pub struct BlockadeCluster {
    pub min_layer: usize,
    pub min_row: usize,
    pub min_col: usize,
    pub max_layer: usize,
    pub max_row: usize,
    pub max_col: usize,
    /// 簇内边界障碍格数（量级≈封锁规模）
    pub cells: usize,
    /// 簇内带网身份格（Pad/Trace/Via 编码）的 net id 集合（升序去重）。
    /// 纯 Blocked 格（板框/器件体/孔墙）无身份——孔墙的孔主人从其包着的铜皮格反查。
    pub net_ids: Vec<u32>,
}

/// CPU 全空间扩展（无 goal 早停的 Dijkstra），返回全格可达标记。
/// 与 dijkstra_3d_path 同构（8 向 + 跨层），但不重建路径、不设终点。
#[allow(clippy::too_many_arguments)]
fn flood_reachable(
    grid: &[u32],
    layers: usize,
    cols: usize,
    rows: usize,
    start: (usize, usize, usize),
    via_cost: f32,
    current_net: u32,
    net_aware: bool,
) -> Vec<bool> {
    use std::collections::BinaryHeap;
    let per_layer = cols * rows;
    let idx = |l: usize, r: usize, c: usize| l * per_layer + r * cols + c;
    let total = layers * per_layer;
    let mut dist = vec![f32::MAX; total];
    let mut seen = vec![false; total];
    let (sl, sr, sc) = start;
    let si = idx(sl, sr, sc);
    if !cell_passable(grid[si], current_net, net_aware) {
        return seen; // 起点自身被封：可达集为空，归因退化为「起点撞墙」
    }
    dist[si] = 0.0;
    let mut heap = BinaryHeap::new();
    heap.push((std::cmp::Reverse(0.0f32.to_bits()), si));
    let dx = [1i32, -1, 0, 0, 1, 1, -1, -1];
    let dy = [0i32, 0, 1, -1, 1, -1, 1, -1];
    let mc = [1.0f32, 1.0, 1.0, 1.0, 1.414, 1.414, 1.414, 1.414];
    while let Some((_, cur)) = heap.pop() {
        if seen[cur] {
            continue;
        }
        seen[cur] = true;
        let layer = cur / per_layer;
        let rem = cur % per_layer;
        let cc = (rem % cols) as i32;
        let cr = (rem / cols) as i32;
        for i in 0..8 {
            let nc = cc + dx[i];
            let nr = cr + dy[i];
            if nc < 0 || nr < 0 {
                continue;
            }
            let (ncu, nru) = (nc as usize, nr as usize);
            if ncu >= cols || nru >= rows {
                continue;
            }
            let ni = layer * per_layer + nru * cols + ncu;
            if seen[ni] || !cell_passable(grid[ni], current_net, net_aware) {
                continue;
            }
            let nd = dist[cur] + mc[i];
            if nd < dist[ni] {
                dist[ni] = nd;
                heap.push((std::cmp::Reverse(nd.to_bits()), ni));
            }
        }
        // 跨层（过孔）
        for nl in [layer.checked_sub(1), Some(layer + 1)] {
            let Some(nl) = nl else { continue };
            if nl >= layers {
                continue;
            }
            let ni = idx(nl, rem / cols, rem % cols);
            if seen[ni] || !cell_passable(grid[ni], current_net, net_aware) {
                continue;
            }
            let nd = dist[cur] + via_cost;
            if nd < dist[ni] {
                dist[ni] = nd;
                heap.push((std::cmp::Reverse(nd.to_bits()), ni));
            }
        }
    }
    seen
}

/// NO-PATH 封锁归因（CPU 专用）：起点全空间扩展得可达集，收集「可达格的邻居中
/// 不可达且非 Free」的边界障碍格，按 3D 连通性（同层 8 邻域 + 跨层同位）聚簇，
/// 按簇规模降序（平局按 bbox 字典序，确定性）取 top_k。
/// goal 可达（有路）时返回空。GPU 引擎不做归因（内核不动），调用方在 CPU 档调用。
pub fn attribute_blockades(
    grid: &[u32],
    spec: GridSpec,
    start: (usize, usize, usize),
    goal: (usize, usize, usize),
    current_net: u32,
    net_aware: bool,
    top_k: usize,
) -> Vec<BlockadeCluster> {
    let per_layer = spec.cols * spec.rows;
    let (sl, sr, sc) = start;
    let (gl, gr, gc) = goal;
    let si = sl * per_layer + sr * spec.cols + sc;
    let gi = gl * per_layer + gr * spec.cols + gc;
    if si >= grid.len() || gi >= grid.len() {
        return Vec::new();
    }
    // goal 本身不可通行（落在异网铜皮/墙里）也是合法归因场景：起点扩展照常进行
    let seen = flood_reachable(
        grid,
        spec.layers,
        spec.cols,
        spec.rows,
        start,
        spec.via_cost,
        current_net,
        net_aware,
    );
    if seen[gi] {
        return Vec::new(); // 有路：无需归因
    }

    // 边界障碍格：非可达 && 非 Free && 存在可达邻居（8 向 + 跨层同位）
    let idx = |l: usize, r: usize, c: usize| l * per_layer + r * spec.cols + c;
    let mut is_border = vec![false; grid.len()];
    for l in 0..spec.layers {
        for r in 0..spec.rows {
            for c in 0..spec.cols {
                let i = idx(l, r, c);
                if seen[i] || grid[i] == 0 {
                    continue;
                }
                // 邻居序固定（确定性）
                let mut reachable_neighbor = false;
                for &(dr, dc) in &[
                    (0i64, 1i64),
                    (0, -1),
                    (1, 0),
                    (-1, 0),
                    (1, 1),
                    (1, -1),
                    (-1, 1),
                    (-1, -1),
                ] {
                    let (rr, cc) = (r as i64 + dr, c as i64 + dc);
                    if rr < 0 || cc < 0 || rr >= spec.rows as i64 || cc >= spec.cols as i64 {
                        continue;
                    }
                    if seen[idx(l, rr as usize, cc as usize)] {
                        reachable_neighbor = true;
                        break;
                    }
                }
                if !reachable_neighbor {
                    for nl in [l.checked_sub(1), Some(l + 1)] {
                        let Some(nl) = nl else { continue };
                        if nl < spec.layers && seen[idx(nl, r, c)] {
                            reachable_neighbor = true;
                            break;
                        }
                    }
                }
                is_border[i] = reachable_neighbor;
            }
        }
    }

    // 连通聚簇（BFS，队列 FIFO + 邻居序固定 → 确定性）
    let mut visited = vec![false; grid.len()];
    let mut clusters: Vec<BlockadeCluster> = Vec::new();
    for l in 0..spec.layers {
        for r in 0..spec.rows {
            for c in 0..spec.cols {
                let i = idx(l, r, c);
                if !is_border[i] || visited[i] {
                    continue;
                }
                let mut queue = std::collections::VecDeque::new();
                queue.push_back(i);
                visited[i] = true;
                let mut cluster = BlockadeCluster {
                    min_layer: l,
                    min_row: r,
                    min_col: c,
                    max_layer: l,
                    max_row: r,
                    max_col: c,
                    cells: 0,
                    net_ids: Vec::new(),
                };
                let mut net_set = std::collections::BTreeSet::new();
                while let Some(cur) = queue.pop_front() {
                    let cl = cur / per_layer;
                    let rem = cur % per_layer;
                    let (cr, cc) = (rem / spec.cols, rem % spec.cols);
                    cluster.cells += 1;
                    cluster.min_layer = cluster.min_layer.min(cl);
                    cluster.max_layer = cluster.max_layer.max(cl);
                    cluster.min_row = cluster.min_row.min(cr);
                    cluster.max_row = cluster.max_row.max(cr);
                    cluster.min_col = cluster.min_col.min(cc);
                    cluster.max_col = cluster.max_col.max(cc);
                    // 网身份（encode_cell 互逆：2+n=Pad 1e6+n=Trace 2e6+n=Via）
                    let v = grid[cur];
                    if v >= 2 {
                        let n = if v >= 2_000_000 {
                            v - 2_000_000
                        } else if v >= 1_000_000 {
                            v - 1_000_000
                        } else {
                            v - 2
                        };
                        net_set.insert(n);
                    }
                    let mut neighbors: Vec<usize> = Vec::with_capacity(10);
                    for &(dr, dc) in &[
                        (0i64, 1i64),
                        (0, -1),
                        (1, 0),
                        (-1, 0),
                        (1, 1),
                        (1, -1),
                        (-1, 1),
                        (-1, -1),
                    ] {
                        let (rr, cc2) = (cr as i64 + dr, cc as i64 + dc);
                        if rr < 0 || cc2 < 0 || rr >= spec.rows as i64 || cc2 >= spec.cols as i64 {
                            continue;
                        }
                        neighbors.push(idx(cl, rr as usize, cc2 as usize));
                    }
                    for nl in [cl.checked_sub(1), Some(cl + 1)] {
                        let Some(nl) = nl else { continue };
                        if nl < spec.layers {
                            neighbors.push(idx(nl, cr, cc));
                        }
                    }
                    for ni in neighbors {
                        if is_border[ni] && !visited[ni] {
                            visited[ni] = true;
                            queue.push_back(ni);
                        }
                    }
                }
                cluster.net_ids = net_set.into_iter().collect();
                clusters.push(cluster);
            }
        }
    }

    // 簇规模降序，平局按 (min_layer,min_row,min_col) 字典序 → 确定性 top_k
    clusters.sort_by(|a, b| {
        b.cells.cmp(&a.cells).then_with(|| {
            (a.min_layer, a.min_row, a.min_col).cmp(&(b.min_layer, b.min_row, b.min_col))
        })
    });
    clusters.truncate(top_k);
    clusters
}

// ---------------------------------------------------------------------------
// 单元测试：passable 语义 / route_batch_cpu 确定性 / 过孔救活路径
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cell_passable_semantics() {
        // 0=空地可走; 1=障碍不可走
        assert!(cell_passable(0, 7, true));
        assert!(!cell_passable(1, 7, true));
        // net-aware 关闭时除障碍外全可走
        assert!(cell_passable(1_000_007, 7, false));
        // 异网 pad/走线/netcode 三种编码在 net-aware 下只放行本网
        assert!(!cell_passable(2, 7, true));
        assert!(!cell_passable(1_000_003, 7, true));
        assert!(!cell_passable(2_000_003, 7, true));
        assert!(cell_passable(1_000_007, 7, true));
        assert!(cell_passable(2_000_007, 7, true));
    }

    #[test]
    fn route_batch_cpu_deterministic() {
        // 同一输入两次求解: 网格逐位一致(路径写回确定性)
        let build = || {
            let (cols, rows, layers) = (8usize, 6usize, 2usize);
            let mut grid = vec![0u32; cols * rows * layers];
            // 中层竖墙(同层不可穿越, 逼绕行或过孔)
            for r in 0..rows {
                grid[r * cols + 4] = 1;
            }
            grid
        };
        let spec = GridSpec {
            cols: 8,
            rows: 6,
            layers: 2,
            via_cost: 8.0,
        };
        let nets = vec![BatchNetQuery {
            net_id: 5,
            sl: 0,
            sr: 0,
            sc: 0,
            gl: 0,
            gr: 0,
            gc: 7,
        }];
        let run = || {
            let mut g = build();
            let outcomes = route_batch_cpu(&mut g, spec, &nets, true, 64, 0).expect("cpu batch ok");
            (g, outcomes)
        };
        let (a, oa) = run();
        let (b, ob) = run();
        assert_eq!(a, b, "同输入两次 CPU 求解必须逐位一致");
        assert_eq!(oa.len(), ob.len());
        assert!(oa[0].routed, "8x6 绕墙必可达");
        // 写回语义: 内部格 = Trace(net)=1_000_000+net(起终点 pad 保持原样)
        assert!(a.contains(&1_000_005), "路径内部格应写回 Trace(5)");
    }

    #[test]
    fn via_rescues_blocked_layer() {
        // 单层被墙完全隔断 → 必须不可达; 双层加过孔 → 同一起终点可达
        let (cols, rows) = (7usize, 5usize);
        // col3 全行墙; layers=1 时 L0 全隔断, layers=2 时墙只砌在起点层 L0
        let wall = |grid: &mut Vec<u32>, layers: usize, wall_layers: usize| {
            for l in 0..wall_layers.min(layers) {
                for r in 0..rows {
                    grid[l * cols * rows + r * cols + 3] = 1;
                }
            }
        };
        let nets = BatchNetQuery {
            net_id: 9,
            sl: 0,
            sr: 2,
            sc: 1,
            gl: 0,
            gr: 2,
            gc: 5,
        };
        // 1 层: 无路
        let mut g1 = vec![0u32; cols * rows];
        wall(&mut g1, 1, 1);
        let spec1 = GridSpec {
            cols,
            rows,
            layers: 1,
            via_cost: 8.0,
        };
        let o1 = route_batch_cpu(&mut g1, spec1, std::slice::from_ref(&nets), true, 64, 0)
            .expect("cpu batch ok");
        assert!(!o1[0].routed, "单层全隔断应判 NO-PATH");

        // 2 层: 墙只砌 L0, 目标在 L1 —— 必经跨层过孔绕开墙
        let mut g2 = vec![0u32; cols * rows * 2];
        wall(&mut g2, 2, 1);
        let spec2 = GridSpec {
            cols,
            rows,
            layers: 2,
            via_cost: 8.0,
        };
        let mut nets2 = nets.clone();
        nets2.gl = 1;
        let o2 = route_batch_cpu(&mut g2, spec2, std::slice::from_ref(&nets2), true, 64, 0)
            .expect("cpu batch ok");
        assert!(o2[0].routed, "双层过孔必须救活隔断层");
        // 写回含 Via(9)=2_000_009 与 B 层 Trace(9)
        assert!(g2.contains(&2_000_009), "必须有跨层过孔写回");
        assert!(o2[0].path.iter().any(|(l, _, _)| *l == 1), "路径落在 B 层");
    }
}
