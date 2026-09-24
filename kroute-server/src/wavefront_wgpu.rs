//! 引擎阶梯 gpu 档：wgpu（Metal/Vulkan/DX12）波前求解。
//!
//! kernel 与 CUDA 版（wavefront.rs WAVEFRONT_CUDA）逐语义同构：多层 + 跨层过孔边
//! + net-aware passable + active 计数早停。内存模式沿用 kicad-cdb gpu_router 的
//!   既有 WGSL 先例：frontier/active 走 atomic，cost/came_from 轮内 RMW 竞态容忍
//!   （多轮收敛，与 CUDA 语义一致）。

use anyhow::Context;

const WAVEFRONT_WGSL: &str = r#"
struct Params {
    cols: u32,
    rows: u32,
    per_layer: u32,
    layers: u32,
    via_cost: f32,
    current_net: u32,
    net_aware: u32,
    _pad: u32,
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> grid: array<u32>;
@group(0) @binding(2) var<storage, read_write> cost: array<f32>;
@group(0) @binding(3) var<storage, read_write> came_from: array<u32>;
@group(0) @binding(4) var<storage, read_write> frontier: array<atomic<u32>>;
@group(0) @binding(5) var<storage, read_write> active_ctr: array<atomic<u32>>;

// 与 CUDA passable / 主机 cell_passable 逐语义同构：
// 0=Free 通 1=Blocked 拒，其余按 encode_cell 解码 net 后只放行 current_net
fn passable(cell_val: u32) -> bool {
    if cell_val == 0u { return true; }
    if cell_val == 1u { return false; }
    if params.net_aware == 0u { return true; }
    var n: u32 = cell_val - 2u;
    if cell_val >= 2000000u { n = cell_val - 2000000u; }
    else if cell_val >= 1000000u { n = cell_val - 1000000u; }
    return n == params.current_net;
}

@compute @workgroup_size(256)
fn wavefront_expand(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    if idx >= params.per_layer * params.layers { return; }
    if atomicLoad(&frontier[idx]) == 0u { return; }
    atomicAdd(&active_ctr[0], 1u);

    let layer = idx / params.per_layer;
    let ridx = idx % params.per_layer;
    let col = ridx % params.cols;
    let row = ridx / params.cols;
    let current_cost = cost[idx];

    let dx = array<i32, 8>(1, -1, 0, 0, 1, 1, -1, -1);
    let dy = array<i32, 8>(0, 0, 1, -1, 1, -1, 1, -1);
    let mc = array<f32, 8>(1.0, 1.0, 1.0, 1.0, 1.414, 1.414, 1.414, 1.414);

    // 同层 8 向
    for (var i: u32 = 0u; i < 8u; i = i + 1u) {
        let nc = i32(col) + dx[i];
        let nr = i32(row) + dy[i];
        if nc < 0 || nr < 0 { continue; }
        let ncu = u32(nc);
        let nru = u32(nr);
        if ncu >= params.cols || nru >= params.rows { continue; }
        let ni = layer * params.per_layer + nru * params.cols + ncu;
        if !passable(grid[ni]) { continue; }
        let new_cost = current_cost + mc[i];
        if new_cost < cost[ni] {
            cost[ni] = new_cost;
            came_from[ni] = idx;
            atomicStore(&frontier[ni], 1u);
        }
    }

    // 跨层（过孔）：电源/机械层构建期即 Blocked，天然不可跨入
    if layer + 1u < params.layers {
        let ni = idx + params.per_layer;
        if passable(grid[ni]) {
            let new_cost = current_cost + params.via_cost;
            if new_cost < cost[ni] {
                cost[ni] = new_cost;
                came_from[ni] = idx;
                atomicStore(&frontier[ni], 1u);
            }
        }
    }
    if layer >= 1u {
        let ni = idx - params.per_layer;
        if passable(grid[ni]) {
            let new_cost = current_cost + params.via_cost;
            if new_cost < cost[ni] {
                cost[ni] = new_cost;
                came_from[ni] = idx;
                atomicStore(&frontier[ni], 1u);
            }
        }
    }
    atomicStore(&frontier[idx], 0u);
}
"#;

use crate::wavefront::{BatchNetOutcome, BatchNetQuery, GridSpec};

/// 一个 spec 一套 device/buffer/pipeline；批量逐 net 复用（与 CUDA WavefrontKernel 同构）
pub struct WgpuKernel {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::ComputePipeline,
    grid_buf: wgpu::Buffer,
    cost_buf: wgpu::Buffer,
    came_from_buf: wgpu::Buffer,
    frontier_buf: wgpu::Buffer,
    active_buf: wgpu::Buffer,
    active_read: wgpu::Buffer,
    cost_read: wgpu::Buffer,
    cf_read: wgpu::Buffer,
    params_buf: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    total: usize,
}

/// 运行时探测：有无可用 wgpu adapter（同步枚举，不建 device）
pub fn available() -> bool {
    let instance = wgpu::Instance::default();
    !instance
        .enumerate_adapters(wgpu::Backends::all())
        .is_empty()
}

impl WgpuKernel {
    pub fn new(spec: GridSpec) -> anyhow::Result<Self> {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))
        .context("wgpu: 无可用 adapter")?;
        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("kroute-wavefront"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::default(),
            },
            None,
        ))
        .context("wgpu: device 请求失败")?;

        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("wavefront_expand"),
            source: wgpu::ShaderSource::Wgsl(WAVEFRONT_WGSL.into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("wavefront_pipeline"),
            layout: None,
            module: &module,
            entry_point: Some("wavefront_expand"),
            compilation_options: Default::default(),
            cache: None,
        });

        let total = spec.cols * spec.rows * spec.layers;
        let u32_buf = |device: &wgpu::Device, usage, len| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: (len * 4) as u64,
                usage,
                mapped_at_creation: false,
            })
        };
        // COPY_SRC：读回 staging 需要（wgpu usage 校验是 fatal panic，必须建对）
        let storage_read = wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_DST
            | wgpu::BufferUsages::COPY_SRC;
        let staging = wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST;
        let grid_buf = u32_buf(&device, storage_read, total);
        let cost_buf = u32_buf(&device, storage_read, total);
        let came_from_buf = u32_buf(&device, storage_read, total);
        let frontier_buf = u32_buf(&device, storage_read, total);
        let active_buf = u32_buf(&device, storage_read, 1);
        let active_read = u32_buf(&device, staging, 1);
        let cost_read = u32_buf(&device, staging, total);
        let cf_read = u32_buf(&device, staging, total);
        let params_buf = u32_buf(
            &device,
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            8,
        );

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: params_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: grid_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: cost_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: came_from_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: frontier_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: active_buf.as_entire_binding(),
                },
            ],
        });

        Ok(Self {
            device,
            queue,
            pipeline,
            grid_buf,
            cost_buf,
            came_from_buf,
            frontier_buf,
            active_buf,
            active_read,
            cost_read,
            cf_read,
            params_buf,
            bind_group,
            total,
        })
    }

    fn read_back(&self, src: &wgpu::Buffer, dst: &wgpu::Buffer, len: usize) -> Vec<u8> {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        encoder.copy_buffer_to_buffer(src, 0, dst, 0, (len * 4) as u64);
        self.queue.submit(Some(encoder.finish()));
        {
            let slice = dst.slice(..(len * 4) as u64);
            let (tx, rx) = std::sync::mpsc::channel();
            slice.map_async(wgpu::MapMode::Read, move |res| {
                tx.send(res).ok();
            });
            self.device.poll(wgpu::Maintain::Wait);
            rx.recv().expect("map 回调通道").expect("map 失败");
            let data = slice.get_mapped_range().to_vec();
            dst.unmap();
            data
        }
    }

    /// 与 CUDA WavefrontKernel::route_one 同构：单 net 求解（批量调用方逐 net 复用 kernel）
    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::type_complexity)] // (cost, 3D 路径点列)
    fn route_one(
        &mut self,
        grid_host: &[u32],
        spec: GridSpec,
        start: (usize, usize, usize),
        goal: (usize, usize, usize),
        current_net: u32,
        net_aware: bool,
        max_rounds: u32,
    ) -> anyhow::Result<Option<(f32, Vec<(usize, usize, usize)>)>> {
        use bytemuck::cast_slice;
        let per_layer = spec.cols * spec.rows;
        let start_idx = start.0 * per_layer + start.1 * spec.cols + start.2;
        let goal_idx = goal.0 * per_layer + goal.1 * spec.cols + goal.2;
        if start_idx >= self.total || goal_idx >= self.total {
            return Ok(None);
        }
        if !crate::wavefront::cell_passable(grid_host[start_idx], current_net, net_aware)
            || !crate::wavefront::cell_passable(grid_host[goal_idx], current_net, net_aware)
        {
            return Ok(None);
        }

        // 每 net 全量重置（与 CUDA 版同口径：grid 按快照重传、cost=MAX 起点置 0、
        // frontier 清零起点置 1；came_from 不重传——重建路径只走本 net 更新过的 cell）
        let mut cost_host = vec![f32::MAX; self.total];
        cost_host[start_idx] = 0.0;
        let mut frontier_host = vec![0u32; self.total];
        frontier_host[start_idx] = 1;
        self.queue
            .write_buffer(&self.grid_buf, 0, cast_slice(grid_host));
        self.queue
            .write_buffer(&self.cost_buf, 0, cast_slice(&cost_host));
        self.queue
            .write_buffer(&self.frontier_buf, 0, cast_slice(&frontier_host));
        let params: [u32; 8] = [
            spec.cols as u32,
            spec.rows as u32,
            per_layer as u32,
            spec.layers as u32,
            spec.via_cost.to_bits(),
            current_net,
            net_aware as u32,
            0,
        ];
        self.queue
            .write_buffer(&self.params_buf, 0, cast_slice(&params));

        let workgroups = (self.total as u32).div_ceil(256);
        let mut rounds_used = 0u32;
        for round in 0..max_rounds {
            self.queue
                .write_buffer(&self.active_buf, 0, 0u32.to_le_bytes().as_slice());
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: None,
                    timestamp_writes: None,
                });
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &self.bind_group, &[]);
                pass.dispatch_workgroups(workgroups, 1, 1);
            }
            encoder.copy_buffer_to_buffer(&self.active_buf, 0, &self.active_read, 0, 4);
            self.queue.submit(Some(encoder.finish()));
            // active 读回（map 回调经 device.poll(Wait) 阻塞完成）
            let active = self.read_back(&self.active_buf, &self.active_read, 1);
            rounds_used = round + 1;
            if u32::from_le_bytes(active[0..4].try_into().unwrap()) == 0 {
                break; // frontier 归零 = 已收敛
            }
        }
        let _ = rounds_used;

        let cost_bytes = self.read_back(&self.cost_buf, &self.cost_read, self.total);
        let cf_bytes = self.read_back(&self.came_from_buf, &self.cf_read, self.total);
        let out_cost: Vec<f32> = cast_slice(&cost_bytes).to_vec();
        let out_cf: Vec<u32> = cast_slice(&cf_bytes).to_vec();

        if out_cost[goal_idx] == f32::MAX {
            return Ok(None);
        }
        let mut path = vec![goal];
        let mut p = goal_idx;
        let mut steps = 0;
        while p != start_idx && steps < self.total {
            let prev = out_cf[p] as usize;
            if prev == u32::MAX as usize || prev >= self.total || prev == p {
                break;
            }
            let layer = prev / per_layer;
            let rem = prev % per_layer;
            path.push((layer, rem / spec.cols, rem % spec.cols));
            p = prev;
            steps += 1;
        }
        path.reverse();
        Ok(Some((out_cost[goal_idx], path)))
    }
}

/// 批量求解（与 CUDA route_batch 同构：net-aware 时路径写回，后续 net 视为障碍）
#[allow(clippy::ptr_arg)]
pub fn route_batch(
    grid_host: &mut Vec<u32>,
    spec: GridSpec,
    nets: &[BatchNetQuery],
    net_aware: bool,
    max_rounds: u32,
) -> anyhow::Result<Vec<BatchNetOutcome>> {
    let mut kernel = WgpuKernel::new(spec)?;
    let max_rounds = if max_rounds == 0 {
        ((spec.cols + spec.rows) * spec.layers) as u32
    } else {
        max_rounds
    };
    let mut out = Vec::with_capacity(nets.len());
    for net in nets {
        let res = kernel.route_one(
            grid_host,
            spec,
            (net.sl, net.sr, net.sc),
            (net.gl, net.gr, net.gc),
            net.net_id,
            net_aware,
            max_rounds,
        )?;
        match res {
            Some((cost, path)) => {
                if net_aware {
                    crate::wavefront::write_back_path(grid_host, spec, net.net_id, &path);
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
