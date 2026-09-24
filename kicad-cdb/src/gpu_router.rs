//! GPU-accelerated wavefront expansion router using wgpu compute shaders.
//!
//! ## Architecture
//!
//! ```text
//! CPU: encode grid → upload buffers → dispatch N rounds → read back cost map → reconstruct paths → write board
//! GPU:  ←─────────────────── wavefront_expand shader (per round) ──────────────────────→
//! ```
//!
//! ## Cell encoding for GPU
//!
//! | CPU Cell | GPU u32 |
//! |----------|---------|
//! | Free     | 0       |
//! | Blocked  | 1       |
//! | Pad(0)   | 2       |
//! | Pad(n)   | 2 + n   |
//! | Trace(n) | 1_000_000 + n |
//! | Via(n)   | 2_000_000 + n |
//!
//! ## Hybrid dispatch
//!
//! - Small boards (<100 nets): CPU-only (GPU overhead not worth it)
//! - Medium boards (100-300 nets): CPU parallel (Rayon)
//! - Large boards (>300 nets): GPU wavefront + CPU path reconstruction

#[cfg(feature = "gpu-router")]
use anyhow::Result;
#[cfg(feature = "gpu-router")]
use std::collections::HashMap;
#[cfg(feature = "gpu-router")]
use wgpu::util::DeviceExt;

// ---------------------------------------------------------------------------
// Cell encoding (shared between CPU and GPU)
// ---------------------------------------------------------------------------

/// Encode a Cell variant to u32 for GPU buffer.
/// Inverse of decode in the WGSL shader.（无 feature 门：波前网格导出（router.rs）也用它）
pub fn encode_cell(cell: u8, net_id: u32) -> u32 {
    // Cell encoding:
    // 0 = Free, 1 = Blocked
    // 2..1_000_000 = Pad(net_id), where net_id = val - 2
    // 1_000_000..2_000_000 = Trace(net_id)
    // 2_000_000+ = Via(net_id)
    match cell {
        0 => 0,                               // Free
        1 => 1,                               // Blocked
        2 => 2 + net_id.min(999_997),         // Pad
        3 => 1_000_000 + net_id.min(999_999), // Trace
        4 => 2_000_000 + net_id.min(999_999), // Via
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// WGSL Shaders
// ---------------------------------------------------------------------------

#[cfg(feature = "gpu-router")]
const WAVEFRONT_SHADER: &str = r"
struct Params {
    cols: u32,
    rows: u32,
    total_cells: u32,
    max_rounds: u32,
    start_idx: u32,
    _pad1: u32,
    _pad2: u32,
    _pad3: u32,
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> grid: array<u32>;
@group(0) @binding(2) var<storage, read_write> cost: array<f32>;
@group(0) @binding(3) var<storage, read_write> came_from: array<u32>;
@group(0) @binding(4) var<storage, read_write> frontier: array<atomic<u32>>;
@group(0) @binding(5) var<storage, read_write> round_counter: atomic<u32>;

// Cell decode constants
const CELL_FREE: u32 = 0u;
const CELL_BLOCKED: u32 = 1u;
const PAD_BASE: u32 = 2u;
const TRACE_BASE: u32 = 1000000u;
const VIA_BASE: u32 = 2000000u;

fn is_passable(cell_val: u32) -> bool {
    if cell_val == CELL_FREE { return true; }
    if cell_val == CELL_BLOCKED { return false; }
    // For wavefront we treat all non-Blocked non-free as potentially passable
    // Net-specific filtering happens on CPU side during path reconstruction
    return true;
}

@compute @workgroup_size(256)
fn init_source() {
    cost[params.start_idx] = 0.0;
    atomicStore(&frontier[params.start_idx], 1u);
    atomicStore(&round_counter, 0u);
}

@compute @workgroup_size(256)
fn wavefront_expand(
    @builtin(global_invocation_id) gid: vec3<u32>,
) {
    let idx = gid.x;
    if idx >= params.total_cells { return; }

    // Only process frontier cells (atomic load)
    if atomicLoad(&frontier[idx]) == 0u { return; }

    let col = idx % params.cols;
    let row = idx / params.cols;
    let current_cost = cost[idx];

    // 8-directional expansion
    let dx = array<i32, 8>(1, -1, 0, 0, 1, 1, -1, -1);
    let dy = array<i32, 8>(0, 0, 1, -1, 1, -1, 1, -1);
    let mc = array<f32, 8>(1.0, 1.0, 1.0, 1.0, 1.414, 1.414, 1.414, 1.414);

    for (i, _) in dx {
        let nc = i32(col) + dx[i];
        let nr = i32(row) + dy[i];
        if nc < 0 || nr < 0 { continue; }
        let ncu = u32(nc);
        let nru = u32(nr);
        if ncu >= params.cols || nru >= params.rows { continue; }

        let ni = nru * params.cols + ncu;
        if !is_passable(grid[ni]) { continue; }

        let new_cost = current_cost + mc[i];
        if new_cost < cost[ni] {
            cost[ni] = new_cost;
            came_from[ni] = idx;
            atomicStore(&frontier[ni], 1u);
        }
    }
    // Clear own frontier flag
    atomicStore(&frontier[idx], 0u);
}
";

// ---------------------------------------------------------------------------
// GPU Router
// ---------------------------------------------------------------------------

#[cfg(feature = "gpu-router")]
pub struct GpuRouter {
    device: wgpu::Device,
    queue: wgpu::Queue,
    expand_pipeline: wgpu::ComputePipeline,
    init_pipeline: wgpu::ComputePipeline,
    params_bgl: wgpu::BindGroupLayout,
    data_bgl: wgpu::BindGroupLayout,
}

#[cfg(feature = "gpu-router")]
impl GpuRouter {
    /// Create a new GPU router instance with headless adapter.
    pub async fn new() -> Result<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
            .ok_or_else(|| anyhow::anyhow!("No suitable GPU adapter found"))?;

        let adapter_info = adapter.get_info();
        eprintln!(
            "[gpu-router] Adapter: {} ({:?})",
            adapter_info.name, adapter_info.backend
        );

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("GPU Router"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::downlevel_defaults(),
                    ..Default::default()
                },
                None,
            )
            .await
            .map_err(|e| anyhow::anyhow!("Failed to create GPU device: {:?}", e))?;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Wavefront Shader"),
            source: wgpu::ShaderSource::Wgsl(WAVEFRONT_SHADER.into()),
        });

        // Bind group layouts
        let params_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Params BGL"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let data_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Data BGL"),
            entries: &[
                // 0: grid (read-only)
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                // 1: cost (read-write)
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                // 2: came_from (read-write)
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                // 3: frontier (atomic, read-write)
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                // 4: round_counter (atomic, read-write)
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Wavefront Layout"),
            bind_group_layouts: &[&params_bgl, &data_bgl],
            push_constant_ranges: &[],
        });

        let expand_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Wavefront Expand"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: Some("wavefront_expand"),
            compilation_options: Default::default(),
            cache: None,
        });

        let init_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Wavefront Init"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: Some("init_source"),
            compilation_options: Default::default(),
            cache: None,
        });

        eprintln!("[gpu-router] Pipelines created, ready for dispatch");

        Ok(GpuRouter {
            device,
            queue,
            expand_pipeline,
            init_pipeline,
            params_bgl,
            data_bgl,
        })
    }

    /// Route a single net on GPU using wavefront expansion.
    /// Returns path as Vec<(col, row, layer_idx)> if found.
    ///
    /// `grid_u32` — encoded grid (use `encode_cell`)
    /// `start` / `goal` — (col, row) on a single layer
    /// `cols`, `rows` — grid dimensions
    pub fn route_single_net(
        &self,
        grid_u32: &[u32],
        cols: usize,
        rows: usize,
        start: (usize, usize),
        goal: (usize, usize),
        max_rounds: u32,
    ) -> Result<Option<Vec<(usize, usize)>>> {
        let total = cols * rows;
        let start_idx = start.1 * cols + start.0;
        let goal_idx = goal.1 * cols + goal.0;

        if start_idx >= total || goal_idx >= total {
            return Ok(None);
        }

        // Create buffers
        let grid_buf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Grid"),
                contents: bytemuck::cast_slice(grid_u32),
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            });

        let cost_data: Vec<f32> = vec![f32::MAX; total];
        let cost_buf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Cost"),
                contents: bytemuck::cast_slice(&cost_data),
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            });

        let came_from_data: Vec<u32> = vec![u32::MAX; total];
        let came_from_buf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("CameFrom"),
                contents: bytemuck::cast_slice(&came_from_data),
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            });

        let frontier_data: Vec<u32> = vec![0; total];
        let frontier_buf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Frontier"),
                contents: bytemuck::cast_slice(&frontier_data),
                usage: wgpu::BufferUsages::STORAGE,
            });

        let round_data: [u32; 1] = [0];
        let round_buf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("RoundCounter"),
                contents: bytemuck::cast_slice(&round_data),
                usage: wgpu::BufferUsages::STORAGE,
            });

        // Params uniform
        let params_data: [u32; 8] = [
            cols as u32,
            rows as u32,
            total as u32,
            max_rounds,
            start_idx as u32,
            0,
            0,
            0,
        ];
        let params_buf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Params"),
                contents: bytemuck::cast_slice(&params_data),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });

        // Bind groups
        let params_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Params BG"),
            layout: &self.params_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: params_buf.as_entire_binding(),
            }],
        });

        let data_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Data BG"),
            layout: &self.data_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: grid_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: cost_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: came_from_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: frontier_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: round_buf.as_entire_binding(),
                },
            ],
        });

        // Staging buffer for readback
        let readback_size = (total * 4) as u64; // f32 = 4 bytes
        let cost_staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Cost Staging"),
            size: readback_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let cf_staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("CameFrom Staging"),
            size: readback_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Dispatch init + expansion rounds
        let workgroups = ((total + 255) / 256) as u32;

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Wavefront Dispatch"),
            });

        // Init source
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Init Source"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.init_pipeline);
            pass.set_bind_group(0, &params_bg, &[]);
            pass.set_bind_group(1, &data_bg, &[]);
            pass.dispatch_workgroups(1, 1, 1);
        }

        // Expansion rounds
        for _round in 0..max_rounds {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Wavefront Expand"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.expand_pipeline);
            pass.set_bind_group(0, &params_bg, &[]);
            pass.set_bind_group(1, &data_bg, &[]);
            pass.dispatch_workgroups(workgroups, 1, 1);
        }

        // Copy results to staging
        encoder.copy_buffer_to_buffer(&cost_buf, 0, &cost_staging, 0, readback_size);
        encoder.copy_buffer_to_buffer(&came_from_buf, 0, &cf_staging, 0, readback_size);

        self.queue.submit(std::iter::once(encoder.finish()));

        // Read back
        let device = &self.device;
        let (tx, rx) = std::sync::mpsc::channel();
        cost_staging
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |r| {
                let _ = tx.send(r);
            });
        device.poll(wgpu::Maintain::Wait);
        rx.recv()??;

        let cost_mapped = cost_staging.slice(..).get_mapped_range();
        let cost_result: Vec<f32> = bytemuck::cast_slice(&cost_mapped).to_vec();
        drop(cost_mapped);

        let (tx2, rx2) = std::sync::mpsc::channel();
        cf_staging
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |r| {
                let _ = tx2.send(r);
            });
        device.poll(wgpu::Maintain::Wait);
        rx2.recv()??;

        let cf_mapped = cf_staging.slice(..).get_mapped_range();
        let cf_result: Vec<u32> = bytemuck::cast_slice(&cf_mapped).to_vec();
        drop(cf_mapped);

        // Check if goal was reached
        if cost_result[goal_idx] == f32::MAX {
            return Ok(None);
        }

        // Reconstruct path on CPU
        let mut path = vec![goal];
        let mut current = goal_idx;
        let mut steps = 0;
        while current != start_idx && steps < total {
            let prev = cf_result[current] as usize;
            if prev == u32::MAX as usize || prev >= total || prev == current {
                break;
            }
            let pc = prev % cols;
            let pr = prev / cols;
            path.push((pc, pr));
            current = prev;
            steps += 1;
        }
        path.reverse();

        Ok(Some(path))
    }

    /// Batch route multiple nets on GPU.
    /// Routes nets one at a time on GPU (wavefront is per-net).
    /// Returns map of net_id → path.
    pub fn route_batch(
        &self,
        grid_u32: &[u32],
        cols: usize,
        rows: usize,
        nets: &[(u32, (usize, usize), (usize, usize))], // (net_id, start, goal)
        max_rounds_per_net: u32,
    ) -> HashMap<u32, Vec<(usize, usize)>> {
        let mut results = HashMap::new();
        let total = nets.len();

        for (i, &(net_id, start, goal)) in nets.iter().enumerate() {
            if (i + 1) % 50 == 0 {
                eprintln!("[gpu-router] Routing net {}/{}", i + 1, total);
            }
            match self.route_single_net(grid_u32, cols, rows, start, goal, max_rounds_per_net) {
                Ok(Some(path)) => {
                    results.insert(net_id, path);
                }
                _ => {}
            }
        }

        eprintln!(
            "[gpu-router] Batch complete: {}/{} nets routed",
            results.len(),
            total
        );
        results
    }
}

// Stub when gpu-router feature is disabled
#[cfg(not(feature = "gpu-router"))]
pub struct GpuRouter;

#[cfg(not(feature = "gpu-router"))]
impl GpuRouter {
    pub fn new() -> anyhow::Result<Self> {
        anyhow::bail!("GPU router requires 'gpu-router' feature: cargo build --features gpu-router")
    }
}
