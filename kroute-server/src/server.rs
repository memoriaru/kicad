//! gRPC 服务实现 + 执行池。

use crate::backends::{self, BackendCfg, Cancelled, Outcome};
use crate::cuda;
use crate::gpuinfo;
use crate::proto::pb::k_route_server::KRoute;
use crate::proto::pb::{
    Backend, CancelReply, CudaDevice, FetchReply, GpuAdapter, HealthReply, HealthReq, JobHandle,
    JobRef, JobState, JobSummary, LayoutOptimizeReply, LayoutOptimizeReq, LayoutSolution,
    ListReply, ListReq, NetRouteResult, Progress, RouteGridBatchReply, RouteGridBatchReq,
    RouteGridReply, RouteGridReq, SubmitReq,
};
use crate::store::{is_terminal, JobStore, WATCH_INTERVAL};
use crate::vram::VramGate;
use futures::Stream;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use tonic::{Request, Response, Status};

pub struct AppState {
    pub store: Arc<JobStore>,
    pub cfg: Arc<BackendCfg>,
    pub sem: Arc<Semaphore>,
    pub active: AtomicU64,
    pub max_concurrent: u64,
    pub version: String,
    /// 显存水位门（共享卡：free VRAM 不足即拒 CUDA 任务）
    pub vram_gate: Arc<VramGate>,
}

pub struct KRouteImpl {
    pub state: Arc<AppState>,
}

impl KRouteImpl {
    /// 把一个 QUEUED 任务投进执行池（服务启动恢复与 Submit 共用）。
    pub fn spawn_queued(state: Arc<AppState>, job_id: String) {
        tokio::spawn(async move {
            execute_job(state, job_id).await;
        });
    }
}

async fn execute_job(state: Arc<AppState>, job_id: String) {
    let store = state.store.clone();
    // QUEUED 等并发额度；等待期间也可被取消
    let permit = loop {
        tokio::select! {
            p = state.sem.clone().acquire_owned() => break p.expect("semaphore 不关闭"),
            _ = tokio::time::sleep(WATCH_INTERVAL) => {
                if store.has_cancel(&job_id) {
                    let _ = store.update_state(&job_id, JobState::Cancelled, None);
                    return;
                }
            }
        }
    };
    let meta = match store.get(&job_id) {
        Ok(m) => m,
        Err(e) => {
            tracing::error!(job = %job_id, "meta 读取失败: {e:#}");
            return;
        }
    };
    if meta.state != JobState::Queued as i32 {
        return; // 已取消或状态漂移，不动
    }
    let _ = store.update_state(&job_id, JobState::Running, None);
    state.active.fetch_add(1, Ordering::Relaxed);
    store
        .append_log(
            &job_id,
            &format!(
                "[run] 开始执行 backend={} timeout={}s",
                meta.backend, meta.timeout_secs
            ),
        )
        .ok();

    let backend = Backend::try_from(meta.backend).unwrap_or(Backend::Unspecified);
    let run = async {
        match backend {
            Backend::Fr => {
                backends::run_fr(
                    &store,
                    &job_id,
                    &state.cfg,
                    &meta.router_args,
                    meta.keep_traces,
                )
                .await
            }
            Backend::Noop => backends::run_noop(&store, &job_id).await,
            Backend::Unspecified => Err(anyhow::anyhow!("backend 未指定")),
        }
    };
    let result = tokio::time::timeout(Duration::from_secs(meta.timeout_secs), run).await;

    let final_state = match &result {
        Ok(Ok(Outcome::Done)) => JobState::Done,
        Ok(Ok(Outcome::Cancelled)) => JobState::Cancelled,
        Ok(Err(e)) if e.is::<Cancelled>() => JobState::Cancelled,
        Ok(Err(_)) => JobState::Failed,
        Err(_elapsed) => {
            let _ = store.append_log(&job_id, "[run] 超时被终止");
            store.clear_cancel(&job_id);
            state.active.fetch_sub(1, Ordering::Relaxed);
            drop(permit);
            let err = format!("超时（{}s）", meta.timeout_secs);
            let _ = store.update_state(&job_id, JobState::Failed, Some(err.clone()));
            store
                .append_log(&job_id, &format!("[run] FAILED: {err}"))
                .ok();
            return;
        }
    };
    match final_state {
        JobState::Done => {
            let _ = store.update_state(&job_id, JobState::Done, None);
            store.append_log(&job_id, "[run] DONE").ok();
        }
        JobState::Cancelled => {
            let _ = store.update_state(&job_id, JobState::Cancelled, None);
            store.append_log(&job_id, "[run] CANCELLED").ok();
        }
        _ => {
            let err = match result {
                Ok(Err(e)) => format!("{e:#}"),
                _ => "未知错误".into(),
            };
            let short: String = err.chars().take(500).collect();
            let _ = store.update_state(&job_id, JobState::Failed, Some(short.clone()));
            store
                .append_log(&job_id, &format!("[run] FAILED: {err}"))
                .ok();
        }
    }
    store.clear_cancel(&job_id);
    state.active.fetch_sub(1, Ordering::Relaxed);
    drop(permit);
}

fn state_of(n: i32) -> JobState {
    JobState::try_from(n).unwrap_or(JobState::Failed)
}

/// 阶梯求解适配：route_batch 返回 BatchRun，这里拆成 outcomes + 引擎名的 Result 形态
#[allow(dead_code)] // 供上层服务组合的结果拆分 API
fn route_batch_outcome(
    grid: &mut Vec<u32>,
    spec: crate::wavefront::GridSpec,
    net: &crate::wavefront::BatchNetQuery,
    net_aware: bool,
    max_rounds: u32,
) -> anyhow::Result<Vec<crate::wavefront::BatchNetOutcome>> {
    crate::wavefront::route_batch(
        grid,
        spec,
        std::slice::from_ref(net),
        net_aware,
        max_rounds,
        0,
    )
    .map(|run| run.outcomes)
}

fn err_status(e: impl std::fmt::Display) -> Status {
    Status::internal(e.to_string())
}

#[tonic::async_trait]
impl KRoute for KRouteImpl {
    async fn health(&self, _req: Request<HealthReq>) -> Result<Response<HealthReply>, Status> {
        let cfg = self.state.cfg.clone();
        // java -version 打到 stderr
        let java = tokio::process::Command::new(&cfg.java)
            .arg("-version")
            .output()
            .await
            .ok()
            .map(|o| {
                let txt = String::from_utf8_lossy(&o.stderr);
                txt.lines().next().unwrap_or("").trim().to_string()
            })
            .unwrap_or_else(|| "ERR: java 不可用".into());

        let jar = if cfg.fr_jar.exists() {
            cfg.fr_jar.to_string_lossy().to_string()
        } else {
            format!("ERR: jar 不存在: {}", cfg.fr_jar.display())
        };

        let kicad_python = match tokio::time::timeout(
            Duration::from_secs(30),
            tokio::process::Command::new(&cfg.kicad_python)
                .args(["-c", "import pcbnew; print(pcbnew.GetBuildVersion())"])
                .output(),
        )
        .await
        {
            Ok(Ok(o)) if o.status.success() => {
                format!("pcbnew {}", String::from_utf8_lossy(&o.stdout).trim())
            }
            _ => "ERR: pcbnew 不可用（检查 kicad-python 配置）".into(),
        };

        let adapters: Vec<GpuAdapter> = gpuinfo::enumerate_adapters()
            .into_iter()
            .map(|a| GpuAdapter {
                name: a.name,
                backend: a.backend,
            })
            .collect();

        // 显存水位（NVML）
        let vram = self.state.vram_gate.probe();
        // CUDA 接入自检：枚举设备 + vec_add kernel 真实执行 + wavefront 路由对拍
        let report = cuda::self_test();
        let mut cuda_selftest = report.summary;
        let cuda_devices: Vec<CudaDevice> = report
            .devices
            .into_iter()
            .map(|d| CudaDevice {
                name: d.name,
                cc: d.cc,
                selftest_ok: d.selftest_ok,
                selftest_ms: d.selftest_ms,
            })
            .collect();
        // 引擎阶梯自检：无条件跑（无 GPU 时自报 ladder=cpu；逐引擎对拍 CPU 参照）
        match crate::wavefront::wavefront_self_test(0) {
            Ok(msg) => cuda_selftest = format!("{cuda_selftest}; wavefront: {msg}"),
            Err(e) => cuda_selftest = format!("{cuda_selftest}; wavefront: ERR: {e:#}"),
        }

        let ready =
            !java.starts_with("ERR") && !jar.starts_with("ERR") && !kicad_python.starts_with("ERR");

        Ok(Response::new(HealthReply {
            version: self.state.version.clone(),
            ready,
            gpu_adapters: adapters,
            java,
            freerouting_jar: jar,
            kicad_python,
            active_jobs: self.state.active.load(Ordering::Relaxed),
            max_concurrent: self.state.max_concurrent,
            cuda_devices,
            cuda_selftest,
            vram_free_mb: vram.as_ref().map(|v| v.free_mb).unwrap_or(0),
            vram_total_mb: vram.as_ref().map(|v| v.total_mb).unwrap_or(0),
            vram_min_free_mb: self.state.vram_gate.min_free_mb,
        }))
    }

    async fn submit_job(&self, req: Request<SubmitReq>) -> Result<Response<JobHandle>, Status> {
        let SubmitReq {
            backend,
            board,
            board_name: raw_name,
            strategy,
            note,
        } = req.into_inner();
        if board.is_empty() {
            return Err(Status::invalid_argument("board 为空"));
        }
        let backend =
            Backend::try_from(backend).map_err(|_| Status::invalid_argument("backend 非法"))?;
        let board_name = if raw_name.trim().is_empty() {
            "board.kicad_pcb".to_string()
        } else {
            std::path::Path::new(raw_name.trim())
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "board.kicad_pcb".into())
        };
        let strategy = strategy.unwrap_or_default();
        let router_args = if strategy.router_args.trim().is_empty() {
            backends::DEFAULT_ROUTER_ARGS.to_string()
        } else {
            strategy.router_args.trim().to_string()
        };
        let timeout_secs = if strategy.timeout_secs == 0 {
            3600
        } else {
            strategy.timeout_secs as u64
        };

        let keep_traces = strategy.keep_traces;
        let (meta, reused) = self
            .state
            .store
            .create(
                backend,
                &board,
                &board_name,
                &router_args,
                timeout_secs,
                &note,
                keep_traces,
            )
            .map_err(err_status)?;
        if !reused {
            Self::spawn_queued(self.state.clone(), meta.job_id.clone());
        }
        Ok(Response::new(JobHandle {
            job_id: meta.job_id,
            reused,
            state: state_of(meta.state).into(),
        }))
    }

    type WatchStream = Pin<Box<dyn Stream<Item = Result<Progress, Status>> + Send + 'static>>;

    async fn watch(&self, req: Request<JobRef>) -> Result<Response<Self::WatchStream>, Status> {
        let job_id = req.into_inner().job_id;
        let store = self.state.store.clone();
        if store.get(&job_id).is_err() {
            return Err(Status::not_found(format!("job 不存在: {job_id}")));
        }
        let stream = async_stream::stream! {
            let mut last_size: u64 = 0;
            let mut last_state: Option<JobState> = None;
            // 上限 24h 防泄漏；正常由终态结束
            let deadline = tokio::time::Instant::now() + Duration::from_secs(24 * 3600);
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(WATCH_INTERVAL) => {}
                    _ = tokio::time::sleep_until(deadline) => {
                        yield Err(Status::deadline_exceeded("watch 超时"));
                        break;
                    }
                }
                let meta = match store.get(&job_id) {
                    Ok(m) => m,
                    Err(e) => { yield Err(Status::not_found(e.to_string())); break; }
                };
                let st = state_of(meta.state);
                let (size, tail) = store.log_tail(&job_id);
                if last_state != Some(st) || size != last_size {
                    last_state = Some(st);
                    last_size = size;
                    yield Ok(Progress { state: st as i32, message: tail, log_size: size });
                }
                if is_terminal(st) {
                    break;
                }
            }
        };
        Ok(Response::new(Box::pin(stream)))
    }

    async fn fetch_result(&self, req: Request<JobRef>) -> Result<Response<FetchReply>, Status> {
        let job_id = req.into_inner().job_id;
        let meta = self
            .state
            .store
            .get(&job_id)
            .map_err(|e| Status::not_found(e.to_string()))?;
        let log = std::fs::read(self.state.store.log_path(&job_id)).unwrap_or_default();
        let result_board = if meta.state == JobState::Done as i32 {
            std::fs::read(self.state.store.result_path(&job_id)).unwrap_or_default()
        } else {
            Vec::new()
        };
        Ok(Response::new(FetchReply {
            state: state_of(meta.state).into(),
            result_board,
            log,
            error: meta.error.unwrap_or_default(),
        }))
    }

    async fn cancel(&self, req: Request<JobRef>) -> Result<Response<CancelReply>, Status> {
        let job_id = req.into_inner().job_id;
        let store = self.state.store.clone();
        let meta = store
            .get(&job_id)
            .map_err(|e| Status::not_found(e.to_string()))?;
        let st = state_of(meta.state);
        if is_terminal(st) {
            return Ok(Response::new(CancelReply {
                cancelled: false,
                state: st as i32,
            }));
        }
        store.request_cancel(&job_id).map_err(err_status)?;
        // QUEUED 还没进执行器，直接标记；RUNNING 由执行器轮询到标志后收尾
        if st == JobState::Queued {
            let _ = store.update_state(&job_id, JobState::Cancelled, None);
        }
        let st = state_of(store.get(&job_id).map(|m| m.state).unwrap_or(0));
        Ok(Response::new(CancelReply {
            cancelled: true,
            state: st as i32,
        }))
    }

    async fn list_jobs(&self, req: Request<ListReq>) -> Result<Response<ListReply>, Status> {
        let limit = req.into_inner().limit;
        let limit = if limit == 0 { 50 } else { limit as usize };
        let jobs = self
            .state
            .store
            .list()
            .map_err(err_status)?
            .into_iter()
            .take(limit)
            .map(|m| JobSummary {
                job_id: m.job_id,
                backend: m.backend,
                state: state_of(m.state).into(),
                board_name: m.board_name,
                created_at: m.created_at,
                note: m.note,
            })
            .collect();
        Ok(Response::new(ListReply { jobs }))
    }

    async fn route_grid(
        &self,
        req: Request<RouteGridReq>,
    ) -> Result<Response<RouteGridReply>, Status> {
        let r = req.into_inner();
        if r.cols == 0 || r.rows == 0 {
            return Err(Status::invalid_argument("cols/rows 不能为 0"));
        }
        // 显存水位门：共享卡上其它负载占用时拒绝，绝不抢显存
        if let Err(msg) = self.state.vram_gate.check() {
            return Err(Status::resource_exhausted(msg));
        }
        let max_rounds = if r.max_rounds == 0 {
            (r.cols + r.rows) * 2
        } else {
            r.max_rounds
        };
        let layers = if r.layers == 0 { 1 } else { r.layers as usize };
        let via_cost = if r.via_cost == 0.0 { 3.0 } else { r.via_cost };
        let expected = (r.cols as usize) * (r.rows as usize) * layers * 4;
        if r.grid.len() != expected {
            return Err(Status::invalid_argument(format!(
                "grid 大小不匹配: got {} bytes, want {}*{}*{}layers*4 = {}",
                r.grid.len(),
                r.cols,
                r.rows,
                layers,
                expected
            )));
        }
        let start_layer = r.start_layer as usize;
        let goal_layer = r.goal_layer as usize;
        if start_layer >= layers || goal_layer >= layers {
            return Err(Status::invalid_argument("start/goal_layer 越界"));
        }
        let r_cols = r.cols as usize;
        let r_per_layer = r_cols * (r.rows as usize);
        let net_aware = r.net_aware;
        let req_net_id = r.net_id;
        // 引擎覆盖：空 = 阶梯（cuda→wgpu→cpu）；指定引擎不可用时 run_batch_on 显式报错
        let engine_kind = match r.engine.as_str() {
            "" => None,
            "cuda" => Some(crate::wavefront::EngineKind::Cuda),
            "wgpu" => Some(crate::wavefront::EngineKind::Wgpu),
            "cpu" => Some(crate::wavefront::EngineKind::Cpu),
            other => return Err(Status::invalid_argument(format!("非法 engine: {other}"))),
        };
        let t0 = std::time::Instant::now();
        // 求解是阻塞型，丢进 blocking 池避免占用 worker；引擎阶梯 cuda→wgpu→cpu。
        // grid 随 BatchRun 一并返回：NO-PATH 归因要复用同一份编码。
        let run = tokio::task::spawn_blocking(move || {
            let mut grid: Vec<u32> = r
                .grid
                .chunks_exact(4)
                .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect();
            let net = crate::wavefront::BatchNetQuery {
                net_id: req_net_id,
                sl: start_layer,
                sr: r.start_row as usize,
                sc: r.start_col as usize,
                gl: goal_layer,
                gr: r.goal_row as usize,
                gc: r.goal_col as usize,
            };
            let spec = crate::wavefront::GridSpec {
                cols: r.cols as usize,
                rows: r.rows as usize,
                layers,
                via_cost,
            };
            let (outcomes, engine_used) = match engine_kind {
                Some(kind) => (
                    crate::wavefront::run_batch_on(
                        kind,
                        &mut grid,
                        spec,
                        std::slice::from_ref(&net),
                        net_aware,
                        max_rounds,
                        0, // MVP: 单卡取 ordinal 0
                    )?,
                    crate::wavefront::engine_name(kind),
                ),
                None => {
                    let run = crate::wavefront::route_batch(
                        &mut grid,
                        spec,
                        std::slice::from_ref(&net),
                        net_aware,
                        max_rounds,
                        0,
                    )?;
                    (run.outcomes, run.engine)
                }
            };
            Ok::<_, anyhow::Error>((outcomes, engine_used, grid))
        })
        .await
        .map_err(|e| Status::internal(format!("join: {e}")))?
        .map_err(|e| Status::internal(format!("wavefront: {e:#}")))?;
        let (outcomes, engine_used, grid) = run;

        let elapsed_ms = t0.elapsed().as_millis() as u64;
        let first = outcomes.first();
        let (routed, cost, path) = match first {
            Some(o) if o.routed => (
                true,
                o.cost,
                o.path
                    .iter()
                    .map(|(l, rw, c)| (l * r_per_layer + rw * r_cols + c) as u32)
                    .collect(),
            ),
            _ => (false, 0.0, Vec::new()),
        };
        // NO-PATH 封锁归因：仅 CPU 引擎路径（GPU 内核不动，修复场景是单网查询，
        // CPU 全空间扩展一次的开销可接受；cuda/wgpu 档归因留空——字段说明已注明）
        let attribution = if !routed && r.attribute_no_path && engine_used == "cpu" {
            crate::wavefront::attribute_blockades(
                &grid,
                crate::wavefront::GridSpec {
                    cols: r_cols,
                    rows: r.rows as usize,
                    layers,
                    via_cost,
                },
                (start_layer, r.start_row as usize, r.start_col as usize),
                (goal_layer, r.goal_row as usize, r.goal_col as usize),
                req_net_id,
                net_aware,
                5, // top5 簇
            )
            .into_iter()
            .map(|c| crate::proto::pb::BlockadeCluster {
                min_layer: c.min_layer as u32,
                min_row: c.min_row as u32,
                min_col: c.min_col as u32,
                max_layer: c.max_layer as u32,
                max_row: c.max_row as u32,
                max_col: c.max_col as u32,
                cells: c.cells as u64,
                net_ids: c.net_ids,
            })
            .collect()
        } else {
            Vec::new()
        };
        Ok(Response::new(RouteGridReply {
            routed,
            cost,
            path,
            elapsed_ms,
            engine: engine_used.into(),
            attribution,
        }))
    }

    async fn route_grid_batch(
        &self,
        req: Request<RouteGridBatchReq>,
    ) -> Result<Response<RouteGridBatchReply>, Status> {
        let r = req.into_inner();
        if r.cols == 0 || r.rows == 0 || r.nets.is_empty() {
            return Err(Status::invalid_argument("cols/rows/nets 不能为空"));
        }
        // 显存水位门：共享卡上其它负载占用时拒绝，绝不抢显存
        if let Err(msg) = self.state.vram_gate.check() {
            return Err(Status::resource_exhausted(msg));
        }
        let layers = if r.layers == 0 { 1 } else { r.layers as usize };
        let via_cost = if r.via_cost == 0.0 { 3.0 } else { r.via_cost };
        let expected = (r.cols as usize) * (r.rows as usize) * layers * 4;
        if r.grid.len() != expected {
            return Err(Status::invalid_argument(format!(
                "grid 大小不匹配: got {} bytes, want {}*{}*{}layers*4 = {}",
                r.grid.len(),
                r.cols,
                r.rows,
                layers,
                expected
            )));
        }
        let (cols, rows) = (r.cols as usize, r.rows as usize);
        let per_layer = cols * rows;
        let total = per_layer * layers;
        let mut queries = Vec::with_capacity(r.nets.len());
        for n in &r.nets {
            let q = crate::wavefront::BatchNetQuery {
                net_id: n.net_id,
                sl: n.sl as usize,
                sr: n.sr as usize,
                sc: n.sc as usize,
                gl: n.gl as usize,
                gr: n.gr as usize,
                gc: n.gc as usize,
            };
            let bad =
                |what: &str| Status::invalid_argument(format!("net {}: {} 越界", n.net_id, what));
            if q.sl >= layers || q.gl >= layers {
                return Err(bad("layer"));
            }
            if q.sr >= rows || q.gr >= rows {
                return Err(bad("row"));
            }
            if q.sc >= cols || q.gc >= cols {
                return Err(bad("col"));
            }
            queries.push(q);
        }
        let t0 = std::time::Instant::now();
        // CUDA 调用是阻塞型，整批丢进 blocking 池（服务端一次 kernel 编译 + 逐 net 求解）
        let run = tokio::task::spawn_blocking(move || {
            let mut grid_host: Vec<u32> = r
                .grid
                .chunks_exact(4)
                .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect();
            if grid_host.len() != total {
                anyhow::bail!("grid 解码后长度异常: {} vs {total}", grid_host.len());
            }
            crate::wavefront::route_batch(
                &mut grid_host,
                crate::wavefront::GridSpec {
                    cols,
                    rows,
                    layers,
                    via_cost,
                },
                &queries,
                r.net_aware,
                r.max_rounds,
                0, // 单卡取 ordinal 0
            )
        })
        .await
        .map_err(|e| Status::internal(format!("join: {e}")))?
        .map_err(|e| Status::internal(format!("cuda: {e:#}")))?;

        let elapsed_ms = t0.elapsed().as_millis() as u64;
        Ok(Response::new(RouteGridBatchReply {
            results: run
                .outcomes
                .into_iter()
                .zip(r.nets)
                .map(|(o, req_net)| NetRouteResult {
                    net_id: o.net_id,
                    routed: o.routed,
                    cost: o.cost,
                    path: o
                        .path
                        .iter()
                        .map(|(l, rw, c)| (l * per_layer + rw * cols + c) as u32)
                        .collect(),
                    pair: req_net.pair,
                })
                .collect(),
            elapsed_ms,
            engine: run.engine.into(),
        }))
    }

    async fn layout_optimize(
        &self,
        req: Request<LayoutOptimizeReq>,
    ) -> Result<Response<LayoutOptimizeReply>, Status> {
        let r = req.into_inner();
        if r.seeds.is_empty() {
            return Err(Status::invalid_argument("seeds 为空"));
        }
        let fixed = if r.fixed_w > 0.0 && r.fixed_h > 0.0 {
            Some((r.fixed_w, r.fixed_h))
        } else {
            None
        };
        let t0 = std::time::Instant::now();
        let res = tokio::task::spawn_blocking(move || {
            let schematic = kicad_json5::parse_json5(&r.schematic)
                .map_err(|e| anyhow::anyhow!("schematic 解析失败: {e}"))?;
            // v35（M2）：连接器锚边覆盖进 directives（非法边名整单拒绝，不静默丢）
            let mut directives = kicad_cdb::layout_directives::LayoutDirectives::default();
            for (ref_name, edge) in &r.anchor_overrides {
                let at = kicad_cdb::layout_engine::parse_anchor_type(edge).ok_or_else(|| {
                    anyhow::anyhow!("anchor_overrides[{ref_name}]: 非法边名 '{edge}'（可用 top/bottom/left/right/center）")
                })?;
                directives.anchor_overrides.insert(ref_name.clone(), at);
            }
            // M3 第二片：盒尺寸实测覆盖
            let mut dim_overrides = std::collections::HashMap::new();
            for (ref_name, d) in &r.dim_overrides {
                dim_overrides.insert(ref_name.clone(), (d.w, d.h));
            }
            if r.overlap_weight > 0.0 {
                directives.sa_weights.overlap = r.overlap_weight;
            }
            let sols = kicad_cdb::layout_engine::run_sa_seeds_with_dims(
                &schematic,
                &directives,
                fixed,
                &r.seeds,
                &dim_overrides,
            );
            Ok::<Vec<(u64, f64, String)>, anyhow::Error>(
                sols.into_iter()
                    .map(|s| {
                        let json = serde_json::json!({
                            "refs": s.refs,
                            "positions": s.positions,
                        })
                        .to_string();
                        (s.seed, s.cost, json)
                    })
                    .collect(),
            )
        })
        .await
        .map_err(|e| Status::internal(format!("join: {e}")))?
        .map_err(|e| Status::internal(format!("{e:#}")))?;

        let elapsed_ms = t0.elapsed().as_millis() as u64;
        Ok(Response::new(LayoutOptimizeReply {
            solutions: res
                .into_iter()
                .map(|(seed, cost, solution_json)| LayoutSolution {
                    seed,
                    cost,
                    solution_json,
                })
                .collect(),
            elapsed_ms,
        }))
    }
}
