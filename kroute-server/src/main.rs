//! kroute-server：分布式布线服务（gRPC）。
//!
//! 同一个二进制两种用法：
//!   服务端: kroute-server serve --freerouting-jar /path/freerouting-1.9.0.jar
//!   客户端: kroute-server submit --backend fr --board x.kicad_pcb --note seed1 --wait

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use kroute_server::backends;
use kroute_server::proto::pb::{
    Backend, FetchReply, HealthReq, JobRef, JobState, ListReq, Progress, Strategy, SubmitReq,
};
use kroute_server::proto::KRouteClient;
use kroute_server::server::{AppState, KRouteImpl};
use kroute_server::store::{is_terminal, JobStore};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tonic::transport::Server;

#[derive(Parser)]
#[command(
    name = "kroute-server",
    version,
    about = "分布式布线服务：gRPC 任务模型 + freerouting 管线（远程结果=候选，本地 union-DRC=裁判）"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum BackendArg {
    /// freerouting 全管线（需要 java + jar + kicad-python）
    Fr,
    /// 冒烟：原样返回输入板
    Noop,
}

impl From<BackendArg> for Backend {
    fn from(v: BackendArg) -> Self {
        match v {
            BackendArg::Fr => Backend::Fr,
            BackendArg::Noop => Backend::Noop,
        }
    }
}

#[derive(Subcommand)]
enum Cmd {
    /// 启动 gRPC 服务
    Serve {
        #[arg(long, default_value = "0.0.0.0:50051")]
        addr: String,
        #[arg(long, default_value = "./jobs")]
        jobs_dir: PathBuf,
        /// 并发执行上限（默认 min(CPU 核数, 8)）
        #[arg(long)]
        max_concurrent: Option<u32>,
        /// freerouting jar 路径（env KROUTE_FR_JAR）
        #[arg(long, env = "KROUTE_FR_JAR")]
        freerouting_jar: Option<PathBuf>,
        /// java 可执行（env KROUTE_JAVA，默认 PATH 上的 java）
        #[arg(long, env = "KROUTE_JAVA")]
        java: Option<String>,
        /// 带 pcbnew 的 python（env KROUTE_KICAD_PYTHON，macOS 默认 KiCad.app 内置）
        #[arg(long, env = "KROUTE_KICAD_PYTHON")]
        kicad_python: Option<String>,
        /// 系统纯 python3（文本处理步骤用）
        #[arg(long, env = "KROUTE_SYSTEM_PYTHON", default_value = "python3")]
        system_python: String,
        /// CUDA 任务显存准入阈值 MB：free 低于此值拒单（共享卡保护，0=不设卡）
        #[arg(long, default_value_t = 2048)]
        min_free_vram_mb: u64,
    },
    /// 服务端依赖自检
    Health {
        #[arg(long, default_value = "http://127.0.0.1:50051")]
        addr: String,
    },
    /// 提交布线任务
    Submit {
        #[arg(long, default_value = "http://127.0.0.1:50051")]
        addr: String,
        #[arg(long)]
        backend: BackendArg,
        /// .kicad_pcb 板文件
        #[arg(long)]
        board: PathBuf,
        /// 参与幂等哈希的备注（同板不同尝试用它区分，如 seed1/order2）
        #[arg(long, default_value = "")]
        note: String,
        /// 透传 freerouting 参数（默认 "-mp 99 -us Hybrid"）
        #[arg(long, default_value = "")]
        router_args: String,
        /// 单任务超时秒数
        #[arg(long, alias = "timeout", default_value_t = 3600)]
        timeout_secs: u64,
        /// 提交后跟随进度直到终态
        #[arg(long)]
        wait: bool,
        /// M3 混合分流：保留板内已有走线，FR 只布剩余开连接
        #[arg(long, default_value_t = false)]
        keep_traces: bool,
    },
    /// 订阅任务进度（到终态结束）
    Watch {
        #[arg(long, default_value = "http://127.0.0.1:50051")]
        addr: String,
        job_id: String,
    },
    /// 拉取结果（--out 落盘布线后板文件）
    Fetch {
        #[arg(long, default_value = "http://127.0.0.1:50051")]
        addr: String,
        job_id: String,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// 取消任务
    Cancel {
        #[arg(long, default_value = "http://127.0.0.1:50051")]
        addr: String,
        job_id: String,
    },
    /// 列出最近任务
    List {
        #[arg(long, default_value = "http://127.0.0.1:50051")]
        addr: String,
        #[arg(long, default_value_t = 50)]
        limit: u32,
    },
    /// 裸网格路由测试：本地合成障碍网格，服务端 CUDA 波前求解
    RouteGrid {
        #[arg(long, default_value = "http://127.0.0.1:50051")]
        addr: String,
        #[arg(long, default_value_t = 256)]
        cols: u32,
        #[arg(long, default_value_t = 256)]
        rows: u32,
        /// 障碍百分比（0-100）
        #[arg(long, default_value_t = 15)]
        obstacle: u32,
        /// xorshift 种子（固定则可复现）
        #[arg(long, default_value_t = 0x9E3779B97F4A7C15)]
        seed: u64,
        /// 起点 col,row（默认 0,0）
        #[arg(long, default_value = "0,0")]
        start: String,
        /// 终点 col,row（默认 cols-1,rows-1）
        #[arg(long, default_value = "")]
        goal: String,
        /// 从文件读网格（kdesign export-grid 产出的 .grid.u32，u32 LE）；
        /// 提供时忽略 --obstacle/--seed
        #[arg(long)]
        grid_file: Option<String>,
        /// 批量模式：nets.json（含 grid_file/cols/rows/layers/net_aware/nets[]），
        /// 提供时忽略单 net 参数，一次往返求解全部 net
        #[arg(long)]
        batch: Option<String>,
        /// 信号层数（grid-file 模式必与导出 meta 一致）
        #[arg(long, default_value_t = 1)]
        layers: u32,
        /// 换层（过孔）代价
        #[arg(long, default_value_t = 3.0)]
        via_cost: f32,
        /// 起点层（默认 0）
        #[arg(long, default_value_t = 0)]
        start_layer: u32,
        /// 终点层（默认 0）
        #[arg(long, default_value_t = 0)]
        goal_layer: u32,
        /// 修复模式：net-aware 求解（目标网铜皮 passable，异网铜皮/孔墙 blocked）
        #[arg(long)]
        net_aware: bool,
        /// 修复模式：目标网 id（配合 --net-aware；0 = 旧语义）
        #[arg(long, default_value_t = 0)]
        net_id: u32,
        /// NO-PATH 封锁归因（仅 CPU 引擎；结果进 --out JSON 的 attribution 数组）
        #[arg(long)]
        attribute: bool,
        /// 引擎覆盖：cpu / wgpu / cuda（空 = 默认阶梯；归因必须 cpu）
        #[arg(long)]
        engine: Option<String>,
        /// 单 net：路径落盘（JSON: cols/rows/layers/via_cost/cost/path[[l,r,c]...]）
        /// 批量：批量结果落盘（JSON: results[{net_id,routed,cost,pair,path[...]}]）
        #[arg(long)]
        out: Option<String>,
    },
    /// 远程多 seed SA 布局优化（重布局引擎算力分发）
    LayoutOptimize {
        #[arg(long, default_value = "http://127.0.0.1:50051")]
        addr: String,
        /// schematic json5 文件（kicad_json5::parse_json5 可解析）
        #[arg(long)]
        schematic: String,
        /// seed 池大小（seeds = base + i*7919，与主线 solve 同族）
        #[arg(long, default_value_t = 64)]
        count: u32,
        /// seed 基数
        #[arg(long, default_value_t = 42)]
        seed_base: u64,
        /// 固定板宽 mm（0 = 自动）
        #[arg(long, default_value_t = 0.0)]
        board_w: f64,
        /// 固定板高 mm（0 = 自动）
        #[arg(long, default_value_t = 0.0)]
        board_h: f64,
        /// 连接器锚边覆盖 REF=Edge（可重复；Edge ∈ top/bottom/left/right/center）
        #[arg(long = "anchor")]
        anchors: Vec<String>,
        /// 最佳解落盘（JSON: refs+positions）
        #[arg(long)]
        out: Option<String>,
        /// 打印前 N 个解的 seed/cost
        #[arg(long, default_value_t = 5)]
        top: u32,
    },
}

fn default_kicad_python() -> String {
    if cfg!(target_os = "macos") {
        "/Applications/KiCad.app/Contents/Frameworks/Python.framework/Versions/Current/bin/python3"
            .into()
    } else {
        "/usr/bin/python3".into()
    }
}

async fn connect(addr: &str) -> Result<KRouteClient<tonic::transport::Channel>> {
    let client = KRouteClient::connect(addr.to_string())
        .await
        .with_context(|| format!("连接失败: {addr}"))?
        .max_decoding_message_size(64 * 1024 * 1024)
        .max_encoding_message_size(64 * 1024 * 1024);
    Ok(client)
}

async fn run_client(cmd: Cmd) -> Result<()> {
    match cmd {
        Cmd::Serve { .. } => unreachable!("serve 在 run_main 处理"),
        Cmd::Health { addr } => {
            let mut c = connect(&addr).await?;
            let h = c.health(HealthReq {}).await?.into_inner();
            println!("version:        {}", h.version);
            println!("ready:          {}", h.ready);
            println!("java:           {}", h.java);
            println!("freerouting:    {}", h.freerouting_jar);
            println!("kicad_python:   {}", h.kicad_python);
            println!(
                "jobs:           {}/{} active",
                h.active_jobs, h.max_concurrent
            );
            if h.vram_total_mb > 0 {
                println!(
                    "vram:           free {}MB / {}MB（准入阈值 {}MB）",
                    h.vram_free_mb, h.vram_total_mb, h.vram_min_free_mb
                );
            }
            println!("cuda:           {}", h.cuda_selftest);
            for d in h.cuda_devices {
                println!(
                    "  [CUDA cc{cc}] {name} selftest={ok} ({ms}ms)",
                    cc = d.cc,
                    name = d.name,
                    ok = d.selftest_ok,
                    ms = d.selftest_ms
                );
            }
            println!("gpu_adapters:");
            if h.gpu_adapters.is_empty() {
                println!("  (未编译 gpu feature 或无 adapter)");
            }
            for a in h.gpu_adapters {
                println!("  [{backend}] {name}", backend = a.backend, name = a.name);
            }
        }
        Cmd::Submit {
            addr,
            backend,
            board,
            note,
            router_args,
            timeout_secs,
            wait,
            keep_traces,
        } => {
            let bytes =
                std::fs::read(&board).with_context(|| format!("读板失败: {}", board.display()))?;
            let board_name = board
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "board.kicad_pcb".into());
            let mut c = connect(&addr).await?;
            let handle = c
                .submit_job(SubmitReq {
                    backend: Backend::from(backend) as i32,
                    board: bytes,
                    board_name,
                    strategy: Some(Strategy {
                        router_args,
                        timeout_secs: timeout_secs as u32,
                        keep_traces,
                    }),
                    note,
                })
                .await?
                .into_inner();
            println!(
                "job_id={} reused={} state={:?}",
                handle.job_id,
                handle.reused,
                handle.state()
            );
            if wait {
                let mut stream = watch_stream(&mut c, &handle.job_id).await?;
                while let Some(p) = stream.message().await? {
                    println!("[{:^9?}] {}", p.state(), p.message);
                }
                let fr = c
                    .fetch_result(JobRef {
                        job_id: handle.job_id.clone(),
                    })
                    .await?
                    .into_inner();
                report_fetch(&fr, None)?;
            }
        }
        Cmd::Watch { addr, job_id } => {
            let mut c = connect(&addr).await?;
            let mut stream = watch_stream(&mut c, &job_id).await?;
            while let Some(p) = stream.message().await? {
                println!("[{:^9?}] {}", p.state(), p.message);
            }
            println!("job {job_id} 已到终态");
        }
        Cmd::Fetch { addr, job_id, out } => {
            let mut c = connect(&addr).await?;
            let fr = c
                .fetch_result(JobRef {
                    job_id: job_id.clone(),
                })
                .await?
                .into_inner();
            report_fetch(&fr, out.as_deref())?;
        }
        Cmd::Cancel { addr, job_id } => {
            let mut c = connect(&addr).await?;
            let r = c
                .cancel(JobRef {
                    job_id: job_id.clone(),
                })
                .await?
                .into_inner();
            if r.cancelled {
                println!("已请求取消: {job_id}，当前状态 {:?}", r.state());
            } else {
                println!("无法取消（已终态）: {job_id}，状态 {:?}", r.state());
            }
        }
        Cmd::List { addr, limit } => {
            let mut c = connect(&addr).await?;
            let r = c.list_jobs(ListReq { limit }).await?.into_inner();
            println!(
                "{:<18} {:<6} {:<12} {:<32} note",
                "job_id", "backend", "state", "board"
            );
            for j in r.jobs {
                println!(
                    "{:<18} {:<6} {:<12} {:<32} {}",
                    j.job_id,
                    format!("{:?}", j.backend()).to_uppercase(),
                    format!("{:?}", j.state()),
                    j.board_name,
                    j.note,
                );
            }
        }
        Cmd::RouteGrid {
            addr,
            cols,
            rows,
            obstacle,
            seed,
            start,
            goal,
            grid_file,
            batch,
            layers,
            via_cost,
            start_layer,
            goal_layer,
            net_aware,
            net_id,
            attribute,
            engine,
            out,
        } => {
            if let Some(batch_file) = batch {
                run_route_batch(&addr, &batch_file, out.as_deref()).await?;
                return Ok(());
            }
            let parse_xy = |s: &str, def: (u32, u32)| -> anyhow::Result<(u32, u32)> {
                if s.trim().is_empty() {
                    return Ok(def);
                }
                let (a, b) = s
                    .split_once(',')
                    .ok_or_else(|| anyhow::anyhow!("坐标格式应为 col,row: {s}"))?;
                Ok((a.trim().parse()?, b.trim().parse()?))
            };

            let grid: Vec<u32> = if let Some(f) = &grid_file {
                let bytes = std::fs::read(f)?;
                bytes
                    .as_chunks::<4>()
                    .0
                    .map(|c| u32::from_le_bytes(*c))
                    .collect()
            } else {
                let total = (cols * rows) as usize;
                let mut s = seed;
                let mut rnd = move || {
                    s ^= s << 13;
                    s ^= s >> 7;
                    s ^= s << 17;
                    s
                };
                (0..total)
                    .map(|_| (rnd() % 100 < obstacle as u64) as u32)
                    .collect()
            };
            let (sc, sr) = parse_xy(&start, (0, 0))?;
            let (gc, gr) = parse_xy(&goal, (cols.saturating_sub(1), rows.saturating_sub(1)))?;
            let (si, gi) = (sr * cols + sc, gr * cols + gc);
            let mut grid = grid;
            if grid.len() != (cols * rows * layers) as usize {
                anyhow::bail!(
                    "grid 大小不匹配: 文件 {} 项 vs cols*rows*layers={}",
                    grid.len(),
                    cols * rows * layers
                );
            }
            if grid_file.is_none() {
                grid[si as usize] = 0;
                grid[gi as usize] = 0;
            }

            let mut c = connect(&addr).await?;
            let t0 = std::time::Instant::now();
            let r = c
                .route_grid(kroute_server::proto::pb::RouteGridReq {
                    cols,
                    rows,
                    grid: grid.iter().flat_map(|v| v.to_le_bytes()).collect(),
                    start_col: sc,
                    start_row: sr,
                    goal_col: gc,
                    goal_row: gr,
                    max_rounds: 0,
                    layers,
                    via_cost,
                    start_layer,
                    goal_layer,
                    net_id,
                    net_aware,
                    attribute_no_path: attribute,
                    engine: engine.unwrap_or_default(),
                })
                .await?
                .into_inner();
            let wall = t0.elapsed().as_millis();
            if r.routed {
                println!(
                    "routed=true cost={:.3} path_cells={} engine={} server_ms={} rtt_ms={}",
                    r.cost,
                    r.path.len(),
                    r.engine,
                    r.elapsed_ms,
                    wall
                );
            } else {
                println!(
                    "routed=false（不可达或撞墙）engine={} server_ms={} rtt_ms={}",
                    r.engine, r.elapsed_ms, wall
                );
                // 归因簇（grid 坐标；世界坐标 = origin + idx*res，由 meta 换算）
                for c in &r.attribution {
                    println!(
                        "  blockade cells={} bbox=layer[{},{}] row[{},{}] col[{},{}] net_ids={:?}",
                        c.cells,
                        c.min_layer,
                        c.max_layer,
                        c.min_row,
                        c.max_row,
                        c.min_col,
                        c.max_col,
                        c.net_ids,
                    );
                }
            }
            if let Some(out) = out {
                let per_layer = (cols * rows) as usize;
                let triple: Vec<[u32; 3]> = r
                    .path
                    .iter()
                    .map(|&fi| {
                        let l = fi as usize / per_layer;
                        let rem = fi as usize % per_layer;
                        [
                            l as u32,
                            (rem / cols as usize) as u32,
                            (rem % cols as usize) as u32,
                        ]
                    })
                    .collect();
                let doc = serde_json::json!({
                    "cols": cols, "rows": rows, "layers": layers, "via_cost": via_cost,
                    "start_layer": start_layer, "goal_layer": goal_layer,
                    "start": [sc, sr], "goal": [gc, gr],
                    "cost": r.cost, "path": triple,
                    "engine": r.engine,
                    "attribution": r.attribution.iter().map(|c| serde_json::json!({
                        "cells": c.cells,
                        "bbox": [c.min_layer, c.min_row, c.min_col, c.max_layer, c.max_row, c.max_col],
                        "net_ids": c.net_ids,
                    })).collect::<Vec<_>>(),
                });
                std::fs::write(&out, serde_json::to_string_pretty(&doc)?)?;
                println!("路径已写出: {out}");
            }
        }
        Cmd::LayoutOptimize {
            addr,
            schematic,
            count,
            seed_base,
            board_w,
            board_h,
            anchors,
            out,
            top,
        } => {
            let source = std::fs::read_to_string(&schematic)
                .with_context(|| format!("读 schematic 失败: {schematic}"))?;
            let mut anchor_overrides = std::collections::HashMap::new();
            for a in &anchors {
                let (ref_name, edge) = a
                    .split_once('=')
                    .ok_or_else(|| anyhow::anyhow!("--anchor 格式应为 REF=Edge: {a}"))?;
                anchor_overrides.insert(ref_name.trim().to_string(), edge.trim().to_string());
            }
            let seeds: Vec<u64> = (0..count).map(|i| seed_base + i as u64 * 7919).collect();
            let mut c = connect(&addr).await?;
            let t0 = std::time::Instant::now();
            let r = c
                .layout_optimize(kroute_server::proto::pb::LayoutOptimizeReq {
                    schematic: source,
                    seeds,
                    fixed_w: board_w,
                    fixed_h: board_h,
                    anchor_overrides,
                    dim_overrides: Default::default(),
                    overlap_weight: 0.0,
                })
                .await?
                .into_inner();
            let wall = t0.elapsed().as_millis();
            println!(
                "solutions={} server_ms={} rtt_ms={}",
                r.solutions.len(),
                r.elapsed_ms,
                wall
            );
            for s in r.solutions.iter().take(top as usize) {
                println!("  seed={:<6} cost={:.2}", s.seed, s.cost);
            }
            if let (Some(out), Some(best)) = (out, r.solutions.first()) {
                std::fs::write(&out, &best.solution_json)?;
                println!(
                    "最佳解已写出: {out} (seed={} cost={:.2})",
                    best.seed, best.cost
                );
            }
        }
    }
    Ok(())
}

async fn watch_stream(
    client: &mut KRouteClient<tonic::transport::Channel>,
    job_id: &str,
) -> Result<tonic::Streaming<Progress>> {
    let stream = client
        .watch(JobRef {
            job_id: job_id.to_string(),
        })
        .await?
        .into_inner();
    Ok(stream)
}

/// 批量模式 nets.json 格式：
/// {
///   "grid_file": "board.grid.u32",
///   "cols": 340, "rows": 220, "layers": 2,
///   "via_cost": 3.0,          // 可选，默认 3.0
///   "max_rounds": 0,          // 可选，0 = (cols+rows)*layers
///   "net_aware": true,        // 可选，默认 false（各 net 独立求解）
///   "nets": [{"net_id":5,"sl":0,"sr":10,"sc":20,"gl":0,"gr":40,"gc":60,"pair":0}, ...]
/// }
async fn run_route_batch(addr: &str, batch_file: &str, out: Option<&str>) -> Result<()> {
    use kroute_server::proto::pb::{NetRoute, RouteGridBatchReq};

    let doc: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(batch_file)?)
        .with_context(|| format!("解析 {batch_file}"))?;
    let grid_file = doc["grid_file"].as_str().context("batch.grid_file 缺失")?;
    let cols = doc["cols"].as_u64().context("batch.cols")? as u32;
    let rows = doc["rows"].as_u64().context("batch.rows")? as u32;
    let layers = doc["layers"].as_u64().unwrap_or(1) as u32;
    let via_cost = doc["via_cost"].as_f64().unwrap_or(3.0) as f32;
    let max_rounds = doc["max_rounds"].as_u64().unwrap_or(0) as u32;
    let net_aware = doc["net_aware"].as_bool().unwrap_or(false);
    let nets_json = doc["nets"].as_array().context("batch.nets")?;
    if nets_json.is_empty() {
        anyhow::bail!("batch.nets 为空");
    }
    let mut nets = Vec::with_capacity(nets_json.len());
    for n in nets_json {
        let field = |k: &str| {
            n[k].as_u64()
                .with_context(|| format!("nets[{}].{k} 缺失", n["net_id"]))
        };
        nets.push(NetRoute {
            net_id: field("net_id")? as u32,
            sl: field("sl")? as u32,
            sr: field("sr")? as u32,
            sc: field("sc")? as u32,
            gl: field("gl")? as u32,
            gr: field("gr")? as u32,
            gc: field("gc")? as u32,
            pair: n["pair"].as_u64().unwrap_or(0) as u32,
        });
    }

    let bytes = std::fs::read(grid_file).with_context(|| format!("读 {grid_file}"))?;
    let expected = (cols as usize) * (rows as usize) * (layers as usize);
    let grid: Vec<u32> = bytes
        .as_chunks::<4>()
        .0
        .map(|c| u32::from_le_bytes(*c))
        .collect();
    anyhow::ensure!(
        grid.len() == expected,
        "grid 大小不匹配: 文件 {} 项 vs cols*rows*layers={expected}",
        grid.len()
    );

    let mut c = connect(addr).await?;
    let t0 = std::time::Instant::now();
    let r = c
        .route_grid_batch(RouteGridBatchReq {
            cols,
            rows,
            layers,
            grid: bytes,
            via_cost,
            max_rounds,
            nets,
            net_aware,
        })
        .await?
        .into_inner();
    let wall = t0.elapsed().as_millis();

    let routed_n = r.results.iter().filter(|x| x.routed).count();
    let total_cost: f32 = r.results.iter().filter(|x| x.routed).map(|x| x.cost).sum();
    println!(
        "batch[engine={}]: {routed_n}/{} routed cost_sum={:.1} net_aware={net_aware} server_ms={} rtt_ms={}",
        r.engine,
        r.results.len(),
        total_cost,
        r.elapsed_ms,
        wall
    );
    for x in &r.results {
        if !x.routed {
            println!("  UNROUTED net_id={} pair={}", x.net_id, x.pair);
        }
    }
    if let Some(out) = out {
        let per_layer = (cols * rows) as usize;
        let to_triple = |&fi: &u32| {
            let l = fi as usize / per_layer;
            let rem = fi as usize % per_layer;
            vec![
                l as u32,
                (rem / cols as usize) as u32,
                (rem % cols as usize) as u32,
            ]
        };
        let doc = serde_json::json!({
            "cols": cols, "rows": rows, "layers": layers,
            "via_cost": via_cost, "net_aware": net_aware,
            "elapsed_ms": r.elapsed_ms,
            "results": r.results.iter().map(|x| serde_json::json!({
                "net_id": x.net_id, "routed": x.routed, "cost": x.cost, "pair": x.pair,
                "path": x.path.iter().map(to_triple).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        });
        std::fs::write(out, serde_json::to_string_pretty(&doc)?)?;
        println!("批量结果已写出: {out}");
    }
    Ok(())
}

fn report_fetch(fr: &FetchReply, out: Option<&std::path::Path>) -> Result<()> {
    let st = fr.state();
    println!("state={st:?}");
    if !fr.error.is_empty() {
        println!("error={}", fr.error);
    }
    println!("log_bytes={}", fr.log.len());
    if st == JobState::Done {
        if let Some(out) = out {
            if fr.result_board.is_empty() {
                bail!("服务端 DONE 但没有 result_board");
            }
            let mut f = std::fs::File::create(out)
                .with_context(|| format!("创建输出失败: {}", out.display()))?;
            f.write_all(&fr.result_board)?;
            println!(
                "已写出布线结果: {} ({} bytes)",
                out.display(),
                fr.result_board.len()
            );
            println!("提醒: 远程结果是候选，落板前先跑本地 kicad-cli union-DRC + netlist-diff");
        } else {
            println!(
                "result_board_bytes={}（用 --out 落盘）",
                fr.result_board.len()
            );
        }
    }
    if is_terminal(st) {
        return Ok(());
    }
    bail!("任务未到终态（用 watch 跟踪）")
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Serve {
            addr,
            jobs_dir,
            max_concurrent,
            freerouting_jar,
            java,
            kicad_python,
            system_python,
            min_free_vram_mb,
        } => {
            let fr_jar = match freerouting_jar {
                Some(p) => p,
                None => bail!("必须提供 --freerouting-jar（或 env KROUTE_FR_JAR）"),
            };
            let max = max_concurrent.map(|n| n.max(1)).unwrap_or_else(|| {
                std::thread::available_parallelism()
                    .map(|n| n.get() as u32)
                    .unwrap_or(4)
                    .min(8)
            });
            let store = Arc::new(JobStore::open(&jobs_dir)?);
            let queued = store.recover()?;
            let vram_gate = Arc::new(kroute_server::vram::VramGate::new(min_free_vram_mb));
            tracing::info!("{}", kroute_server::vram::init_report(&vram_gate));
            let state = Arc::new(AppState {
                store: store.clone(),
                cfg: Arc::new(backends::BackendCfg {
                    java: java.unwrap_or_else(|| "java".into()),
                    fr_jar,
                    kicad_python: kicad_python.unwrap_or_else(default_kicad_python),
                    system_python,
                }),
                sem: Arc::new(Semaphore::new(max as usize)),
                active: AtomicU64::new(0),
                max_concurrent: max as u64,
                version: env!("CARGO_PKG_VERSION").into(),
                vram_gate,
            });
            for m in queued {
                tracing::info!(job = %m.job_id, "重启恢复：重新入队");
                KRouteImpl::spawn_queued(state.clone(), m.job_id);
            }
            tracing::info!(
                "kroute-server v{} listening on {addr}，jobs_dir={}，并发上限={max}",
                state.version,
                jobs_dir.display()
            );
            Server::builder()
                .add_service(
                    kroute_server::proto::KRouteServer::new(KRouteImpl { state })
                        .max_decoding_message_size(64 * 1024 * 1024)
                        .max_encoding_message_size(64 * 1024 * 1024),
                )
                .serve(addr.parse()?)
                .await
                .context("serve 异常退出")?;
        }
        other => run_client(other).await?,
    }
    Ok(())
}
