//! v35 全链路伺服编排（docs/v35-flow-design.md 的 M1 骨架）。
//!
//! 五阶段管道，每阶段产物落盘 `<out_dir>/sN-*`，存在即跳过（断点续跑）：
//! [S1] layout-seeds  schematic → LayoutOptimize RPC → top-k 解（s1-solutions.json）
//! [S2] pick          top-k × RouteGridBatch 可布线性实测 → 选 1（s2-scoring.json）
//! [S3] commit-layout 解回填 board 文本手术（s3-board.kicad_pcb）
//! [S4] route-batch   export-grid → RouteGridBatch(net-aware)（s4.grid.* + s4-batch.json）
//! [S5] hybrid+DRC    直连集 commit → s5-board.kicad_pcb（候选）+ 混合分流计划
//!
//! 纪律（不变）：远程产物一律候选；落板前本地 kicad-cli union-DRC；
//! 显存门在服务端 RouteGridBatch 入口强制。

use anyhow::{bail, Context, Result};
use kroute_server::proto::pb::{
    Backend, JobRef, LayoutOptimizeReq, NetRoute, RouteGridBatchReply, RouteGridBatchReq, Strategy,
};
use serde::{Deserialize, Serialize};

/// S2 协同评分权重（v35-flow-design 关键设计 1；跑 v35 时标定）
const W_ROUTED: f64 = 1000.0;
const W_COST: f64 = 1.0;
const PENALTY_UNROUTED: f64 = 100_000.0;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolutionRec {
    pub seed: u64,
    pub cost: f64,
    pub refs: Vec<String>,
    /// (x, y, rot) 与 refs 一一对应（serde 直接从 [x,y,rot] 数组反序列化）
    pub positions: Vec<(f64, f64, f64)>,
}

#[derive(Debug, Deserialize)]
struct SolutionJson {
    refs: Vec<String>,
    positions: Vec<(f64, f64, f64)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct S1Artifact {
    schematic: String,
    board_w: f64,
    board_h: f64,
    /// 本次 S1 使用的锚边覆盖（断点留痕：换锚需 --force 重跑 S1）
    anchor_overrides: std::collections::BTreeMap<String, String>,
    solutions: Vec<SolutionRec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ScoreRec {
    seed: u64,
    sa_cost: f64,
    routed: usize,
    nets: usize,
    cost_sum: f64,
    unrouted_net_ids: Vec<u32>,
    /// M3：候选布局的实测盒重叠对数（shorting 的布局态根源）
    overlaps: usize,
    /// α·sa_cost_norm − (1−α)·rout_norm + 重叠罚，越小越好
    total: f64,
    server_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct S2Artifact {
    alpha: f64,
    winner_seed: u64,
    scoring: Vec<ScoreRec>,
}

pub struct V35FlowArgs {
    pub input: String,
    pub schematic: String,
    pub board_size: Option<(f64, f64)>,
    pub addr: String,
    pub seeds: u32,
    pub seed_base: u64,
    pub top: usize,
    pub out_dir: String,
    pub alpha: f64,
    pub net_aware: bool,
    pub res: f64,
    pub width: f64,
    pub via_size: f64,
    pub via_drill: f64,
    pub force: bool,
    pub drc: bool,
    /// 连接器锚边覆盖 ref → "top"/"bottom"/"left"/"right"/"center"（M2）
    pub anchor_overrides: std::collections::HashMap<String, String>,
    /// M3：S4 顺序择优——尝试 K 个布线顺序（导出序/短距优先/固定种子洗牌）取最优
    pub orders: u32,
    /// M3：间距膨胀格数（0=关；推荐 ceil((clearance+线宽/2)/res)）
    pub dilate: usize,
    /// M3：SA 重叠罚权重覆盖（0 = 服务端默认 50）
    pub overlap_weight: f64,
    /// M3：S5 commit 线宽自适应（按路径到异网铜皮距离收窄），传 false 关闭
    pub adaptive_width: bool,
    /// M3：信号层数（2=F/B；4=F+In1+In2+B 全信号——fpga 板原生 4 层）
    pub layers: usize,
    /// M3：S6 混合分流落地——s5 板（Direct 铜皮保留）提交 freerouting 只布硬网
    pub hybrid_fr: bool,
}

pub fn run(args: V35FlowArgs) -> Result<()> {
    std::fs::create_dir_all(&args.out_dir)?;
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(run_async(args))
}

async fn run_async(args: V35FlowArgs) -> Result<()> {
    let out = &args.out_dir;
    let s1_path = format!("{out}/s1-solutions.json");
    let s2_path = format!("{out}/s2-scoring.json");
    let s3_path = format!("{out}/s3-board.kicad_pcb");
    let s4_grid_prefix = format!("{out}/s4");
    let s4_batch_path = format!("{out}/s4-batch.json");
    let s5_path = format!("{out}/s5-board.kicad_pcb");
    let s5_plan_path = format!("{out}/s5-hybrid-plan.json");

    // ── S1: layout-seeds ─────────────────────────────────────────────
    let s1: S1Artifact = if std::path::Path::new(&s1_path).exists() && !args.force {
        println!("[S1] 命中断点: {s1_path}");
        serde_json::from_str(&std::fs::read_to_string(&s1_path)?)?
    } else {
        let schematic_text = std::fs::read_to_string(&args.schematic)
            .with_context(|| format!("读 {}", args.schematic))?;
        let seeds: Vec<u64> = (0..args.seeds)
            .map(|i| args.seed_base + i as u64 * 7919)
            .collect();
        println!(
            "[S1] LayoutOptimize: {} seeds → {} …",
            seeds.len(),
            args.addr
        );
        // M3 第二片：量测基线板真实 pad 包围盒（SA 盒尺寸实测优先于启发式估计）
        let measure_src =
            std::fs::read_to_string(&args.input).with_context(|| format!("读 {}", args.input))?;
        let measure_board = kicad_json5::parse_board(&measure_src)?;
        let dims = kicad_cdb::layout_engine::measure_footprint_extents(&measure_board, 0.5);
        println!(
            "[S1] 实测 {} 个 footprint 物理包围盒（margin 0.5mm）反馈 SA 盒尺寸",
            dims.len()
        );
        let dim_overrides: std::collections::HashMap<
            String,
            kroute_server::proto::pb::DimOverride,
        > = dims
            .iter()
            .map(|(k, (w, h))| {
                (
                    k.clone(),
                    kroute_server::proto::pb::DimOverride { w: *w, h: *h },
                )
            })
            .collect();
        let mut client = connect(&args.addr).await?;
        let t0 = std::time::Instant::now();
        let reply = client
            .layout_optimize(LayoutOptimizeReq {
                schematic: schematic_text,
                seeds,
                fixed_w: args.board_size.map(|(w, _)| w).unwrap_or(0.0),
                fixed_h: args.board_size.map(|(_, h)| h).unwrap_or(0.0),
                anchor_overrides: args.anchor_overrides.clone(),
                dim_overrides,
                overlap_weight: args.overlap_weight,
            })
            .await?
            .into_inner();
        let solutions: Vec<SolutionRec> = reply
            .solutions
            .iter()
            .filter_map(|s| {
                let v: SolutionJson = serde_json::from_str(&s.solution_json).ok()?;
                Some(SolutionRec {
                    seed: s.seed,
                    cost: s.cost,
                    refs: v.refs,
                    positions: v.positions,
                })
            })
            .collect();
        println!(
            "[S1] {} 解（server {}ms，总 {}ms）",
            solutions.len(),
            reply.elapsed_ms,
            t0.elapsed().as_millis()
        );
        let artifact = S1Artifact {
            schematic: args.schematic.clone(),
            board_w: args.board_size.map(|(w, _)| w).unwrap_or(0.0),
            board_h: args.board_size.map(|(_, h)| h).unwrap_or(0.0),
            anchor_overrides: args
                .anchor_overrides
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            solutions,
        };
        std::fs::write(&s1_path, serde_json::to_string_pretty(&artifact)?)?;
        println!("[S1] 已落盘 {s1_path}");
        artifact
    };
    if s1.solutions.is_empty() {
        bail!("S1 无解");
    }

    // ── S2: pick（可布线性实测评分）─────────────────────────────────
    let s2: S2Artifact = if std::path::Path::new(&s2_path).exists() && !args.force {
        println!("[S2] 命中断点: {s2_path}");
        serde_json::from_str(&std::fs::read_to_string(&s2_path)?)?
    } else {
        let src =
            std::fs::read_to_string(&args.input).with_context(|| format!("读 {}", args.input))?;
        // M3：量测真实盒尺寸（S2 计重叠对数用；与 S1 断点无关，独立重测）
        let mboard = kicad_json5::parse_board(&src)?;
        let dims = kicad_cdb::layout_engine::measure_footprint_extents(&mboard, 0.5);
        let candidates = &s1.solutions[..s1.solutions.len().min(args.top)];
        let mut client = connect(&args.addr).await?;
        let mut scoring: Vec<ScoreRec> = Vec::new();
        for (i, cand) in candidates.iter().enumerate() {
            let overlap_pairs = count_overlap_pairs(&cand.refs, &cand.positions, &dims);
            println!(
                "[S2] 候选 {}/{} seed={} sa_cost={:.1}：实测可布线性 …",
                i + 1,
                candidates.len(),
                cand.seed,
                cand.cost
            );
            let text =
                crate::commands::apply_layout_solution_text(&src, &cand.refs, &cand.positions)?;
            let board = kicad_json5::parse_board(&text.0)?;
            let export = score_export(&board, args.res, args.dilate, args.layers)?;
            let req = batch_req_from_export(&export, args.net_aware, 0, 3.0, None);
            let _t0 = std::time::Instant::now();
            let reply = client.route_grid_batch(req).await?.into_inner();
            let routed = reply.results.iter().filter(|r| r.routed).count();
            let cost_sum: f64 = reply
                .results
                .iter()
                .filter(|r| r.routed)
                .map(|r| r.cost as f64)
                .sum();
            let unrouted: Vec<u32> = reply
                .results
                .iter()
                .filter(|r| !r.routed)
                .map(|r| r.net_id)
                .collect();
            println!(
                "[S2]   routed {routed}/{} cost_sum={cost_sum:.0} engine={}（server {}ms）",
                reply.results.len(),
                reply.engine,
                reply.elapsed_ms
            );
            scoring.push(ScoreRec {
                seed: cand.seed,
                sa_cost: cand.cost,
                routed,
                nets: reply.results.len(),
                cost_sum,
                unrouted_net_ids: unrouted,
                overlaps: overlap_pairs,
                total: 0.0,
                server_ms: reply.elapsed_ms,
            });
        }
        // 总评 = α·sa_cost_norm − (1−α)·rout_norm（min 归一，越小越好）
        let min_sa = scoring
            .iter()
            .map(|s| s.sa_cost)
            .fold(f64::INFINITY, f64::min)
            .max(1e-9);
        const OV_PENALTY: f64 = 0.05; // 每对重叠盒的罚，量级=一条网的可布线性粒度
        for s in &mut scoring {
            let rout_norm = (W_ROUTED * s.routed as f64
                - W_COST * s.cost_sum
                - PENALTY_UNROUTED * (s.nets - s.routed) as f64)
                / (W_ROUTED * s.nets.max(1) as f64);
            s.total = args.alpha * (s.sa_cost / min_sa) - (1.0 - args.alpha) * rout_norm
                + (1.0 - args.alpha) * OV_PENALTY * s.overlaps as f64;
        }
        scoring.sort_by(|a, b| {
            a.total
                .partial_cmp(&b.total)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let winner_seed = scoring[0].seed;
        println!(
            "[S2] 胜出 seed={winner_seed}（total={:.4}）",
            scoring[0].total
        );
        let artifact = S2Artifact {
            alpha: args.alpha,
            winner_seed,
            scoring,
        };
        std::fs::write(&s2_path, serde_json::to_string_pretty(&artifact)?)?;
        println!("[S2] 已落盘 {s2_path}");
        artifact
    };

    // ── S3: commit-layout（解回填文本手术）──────────────────────────
    if std::path::Path::new(&s3_path).exists() && !args.force {
        println!("[S3] 命中断点: {s3_path}");
    } else {
        let src =
            std::fs::read_to_string(&args.input).with_context(|| format!("读 {}", args.input))?;
        let winner = s1
            .solutions
            .iter()
            .find(|s| s.seed == s2.winner_seed)
            .context("S2 胜出 seed 不在 S1 解集中")?;
        let (text, patched) =
            crate::commands::apply_layout_solution_text(&src, &winner.refs, &winner.positions)?;
        std::fs::write(&s3_path, text)?;
        println!(
            "[S3] 解回填 seed={}：{patched} footprint → {s3_path}",
            winner.seed
        );
    }

    // ── S4: route-batch（net-aware 全网波前）────────────────────────
    let (export_meta, s4) = if std::path::Path::new(&s4_batch_path).exists() && !args.force {
        println!("[S4] 命中断点: {s4_batch_path}");
        let meta: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(format!(
            "{s4_grid_prefix}.grid.json"
        ))?)?;
        let batch: RouteGridBatchReply = load_batch_reply(&s4_batch_path)?;
        (meta, batch)
    } else {
        let src = std::fs::read_to_string(&s3_path)?;
        let board = kicad_json5::parse_board(&src)?;
        let export = score_export(&board, args.res, args.dilate, args.layers)?;
        // 落盘 grid 产物（供断点后 S5 复用 meta）
        let mut bytes = Vec::with_capacity(export.grid_u32.len() * 4);
        for v in &export.grid_u32 {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        std::fs::write(format!("{s4_grid_prefix}.grid.u32"), &bytes)?;
        let meta = serde_json::json!({
            "input": s3_path,
            "cols": export.cols, "rows": export.rows,
            "grid_res_mm": export.grid_res_mm, "origin_mm": export.origin_mm,
            "layers": export.layers,
            "net_count": export.nets.len(),
            "nets": export.nets.iter().map(|n| serde_json::json!({
                "net_id": n.net_id, "name": n.name,
            })).collect::<Vec<_>>(),
        });
        std::fs::write(
            format!("{s4_grid_prefix}.grid.json"),
            serde_json::to_string_pretty(&meta)?,
        )?;
        println!(
            "[S4] export-grid {}x{}x{} nets={} → {s4_grid_prefix}.grid.*",
            export.cols,
            export.rows,
            export.layers.len(),
            export.nets.len()
        );
        // M3 顺序择优：K 个顺序各跑一轮（每轮从原始 grid 独立重放），取布通最多者
        let orders = build_orders(
            &batch_req_from_export(&export, args.net_aware, 0, 3.0, None).nets,
            args.orders,
        );
        let mut client = connect(&args.addr).await?;
        let t0 = std::time::Instant::now();
        let mut best: Option<(usize, f64, RouteGridBatchReply, Vec<usize>)> = None;
        let mut order_stats: Vec<serde_json::Value> = Vec::new();
        for (oi, order) in orders.iter().enumerate() {
            let req = batch_req_from_export(&export, args.net_aware, 0, 3.0, Some(order));
            let reply = client.route_grid_batch(req).await?.into_inner();
            let routed = reply.results.iter().filter(|r| r.routed).count();
            let cost_sum: f64 = reply
                .results
                .iter()
                .filter(|r| r.routed)
                .map(|r| r.cost as f64)
                .sum();
            println!(
                "[S4]   order {oi}: {routed}/{} routed cost_sum={cost_sum:.0}（server {}ms）",
                reply.results.len(),
                reply.elapsed_ms
            );
            order_stats.push(serde_json::json!({
                "order_index": oi,
                "routed": routed,
                "cost_sum": cost_sum,
                "server_ms": reply.elapsed_ms,
            }));
            let better = match &best {
                None => true,
                Some((br, bc, _, _)) => routed > *br || (routed == *br && cost_sum < *bc),
            };
            if better {
                best = Some((routed, cost_sum, reply, order.clone()));
            }
        }
        let (routed, cost_sum, reply, order) = best.expect("至少一个顺序");
        println!(
                "[S4] RouteGridBatch: {routed}/{} routed cost_sum={cost_sum:.0} engine={}（{} 顺序，总 {}ms）",
                reply.results.len(),
                reply.engine,
                orders.len(),
                t0.elapsed().as_millis()
            );
        save_batch_reply(
            &s4_batch_path,
            &reply,
            export.cols,
            export.rows,
            export.layers.len(),
            args.net_aware,
            &order_stats,
        )?;
        println!(
            "[S4] 已落盘 {s4_batch_path}（胜出顺序 {}/{}）",
            order.len(),
            orders.len()
        );
        (meta, reply)
    };

    // net 名对照（S5 计划用）：优先从内存 export 取，断点恢复时从 grid meta 补
    let net_names = net_name_map(&export_meta);

    // ── S5: hybrid+DRC（直连集 commit + 分流计划）───────────────────
    if std::path::Path::new(&s5_path).exists() && !args.force {
        println!("[S5] 命中断点: {s5_path}（混合分流计划见 {s5_plan_path}）");
    } else {
        let src = std::fs::read_to_string(&s3_path)?;
        let layer_names: Vec<String> = export_meta["layers"]
            .as_array()
            .context("meta.layers")?
            .iter()
            .map(|v| v.as_str().unwrap_or_default().to_string())
            .collect();
        let origin = (
            export_meta["origin_mm"][0].as_f64().context("origin.0")?,
            export_meta["origin_mm"][1].as_f64().context("origin.1")?,
        );
        let res = export_meta["grid_res_mm"].as_f64().context("grid_res")?;
        // M3 线宽自适应：从 S4 网格产物读障碍场（断点续跑同样可用）
        let grid_u32: Vec<u32> = if args.adaptive_width {
            let bytes = std::fs::read(format!("{s4_grid_prefix}.grid.u32"))
                .with_context(|| format!("读 {s4_grid_prefix}.grid.u32"))?;
            bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect()
        } else {
            Vec::new()
        };
        let cols = export_meta["cols"].as_u64().context("meta.cols")? as usize;
        let rows = export_meta["rows"].as_u64().context("meta.rows")? as usize;
        let per_layer = cols * rows;

        let mut blocks = String::new();
        let (mut seg_total, mut via_total) = (0usize, 0usize);
        let mut direct: Vec<serde_json::Value> = Vec::new();
        let mut hard: Vec<serde_json::Value> = Vec::new();
        for r in &s4.results {
            let name = net_names
                .get(&r.net_id)
                .cloned()
                .unwrap_or_else(|| format!("net{}", r.net_id));
            if r.routed && !r.path.is_empty() {
                let triples: Vec<Vec<u64>> = r
                    .path
                    .iter()
                    .map(|&fi| {
                        let l = fi as usize / per_layer;
                        let rem = fi as usize % per_layer;
                        vec![l as u64, (rem / cols) as u64, (rem % cols) as u64]
                    })
                    .collect();
                if triples.len() >= 2 {
                    // M3 线宽自适应：按路径到异网铜皮距离收窄（grid 场来自 S4 产物）
                    let width_profile = args.adaptive_width.then(|| {
                        let path: Vec<(usize, usize, usize)> = triples
                            .iter()
                            .map(|t| (t[0] as usize, t[1] as usize, t[2] as usize))
                            .collect();
                        // min_w 取规则最小线宽 0.2——低于它的窄线触发 track_width 违规
                        kicad_cdb::router::adaptive_path_widths(
                            &grid_u32,
                            cols,
                            rows,
                            layer_names.len(),
                            r.net_id,
                            &path,
                            res,
                            0.2,
                            args.width,
                            0.2,
                        )
                    });
                    let (b, segs, vias) = crate::commands::build_path_blocks(
                        &triples,
                        &layer_names,
                        origin,
                        res,
                        r.net_id,
                        args.width,
                        args.via_size,
                        args.via_drill,
                        width_profile.as_deref(),
                    );
                    blocks.push_str(&b);
                    seg_total += segs;
                    via_total += vias;
                }
                direct.push(serde_json::json!({
                    "net_id": r.net_id, "name": name, "cost": r.cost,
                    "segs": triples.len().saturating_sub(1), "pair": r.pair,
                }));
            } else {
                // 混合分流：波前不可达 → DSN→freerouting→SES 回导（M3 落地，这里先立项）
                hard.push(serde_json::json!({
                    "net_id": r.net_id, "name": name, "pair": r.pair,
                    "route": "freerouting", "reason": "wavefront-unreachable",
                }));
            }
        }
        let out_text = crate::commands::insert_before_zones_owned(&src, &blocks);
        std::fs::write(&s5_path, out_text)?;
        let plan = serde_json::json!({
            "board": s5_path,
            "direct_committed": { "nets": direct.len(), "segs": seg_total, "vias": via_total, "detail": direct },
            "freerouting_todo": { "nets": hard.len(), "detail": hard },
            "discipline": "s5-board 是候选：kicad-cli pcb drc + netlist-diff 通过后才落板",
        });
        std::fs::write(&s5_plan_path, serde_json::to_string_pretty(&plan)?)?;
        println!(
            "[S5] 直连 commit {} net（+{seg_total} 段 +{via_total} 过孔）→ {s5_path}",
            direct.len()
        );
        println!(
            "[S5] 混合分流：{} net 进 freerouting 队列 → {s5_plan_path}",
            hard.len()
        );
    }

    // ── S6: hybrid-FR（混合分流落地）─────────────────────────────
    let s6_path = format!("{out}/s6-board.kicad_pcb");
    if args.hybrid_fr {
        if std::path::Path::new(&s6_path).exists() && !args.force {
            println!("[S6] 命中断点: {s6_path}");
        } else {
            let board_bytes = std::fs::read(&s5_path).with_context(|| format!("读 {s5_path}"))?;
            let note = format!("v35-hybrid-{}", std::process::id());
            println!(
                "[S6] 提交 freerouting（保留 Direct 铜皮，只布硬网）→ {} …",
                args.addr
            );
            let mut client = connect(&args.addr).await?;
            let handle = client
                .submit_job(kroute_server::proto::pb::SubmitReq {
                    backend: Backend::Fr as i32,
                    board: board_bytes,
                    board_name: "v35-hybrid.kicad_pcb".into(),
                    strategy: Some(Strategy {
                        router_args: String::new(), // 默认 -mp 99 -us Hybrid
                        timeout_secs: 3600,
                        keep_traces: true,
                    }),
                    note,
                })
                .await?
                .into_inner();
            println!(
                "[S6] job_id={} reused={}，跟随进度到终态 …",
                handle.job_id, handle.reused
            );
            let mut stream = client
                .watch(JobRef {
                    job_id: handle.job_id.clone(),
                })
                .await?
                .into_inner();
            while let Some(p) = stream.message().await? {
                let st = p.state();
                let last = p.message.lines().last().unwrap_or("").to_string();
                println!("[S6] [{st:?}] {last}");
                if kroute_server::proto::pb::JobState::try_from(p.state)
                    .map(|s| {
                        matches!(
                            s,
                            kroute_server::proto::pb::JobState::Done
                                | kroute_server::proto::pb::JobState::Failed
                                | kroute_server::proto::pb::JobState::Cancelled
                        )
                    })
                    .unwrap_or(false)
                {
                    break;
                }
            }
            let fr = client
                .fetch_result(JobRef {
                    job_id: handle.job_id,
                })
                .await?
                .into_inner();
            if !fr.error.is_empty() {
                bail!("S6 freerouting 失败: {}", fr.error);
            }
            if fr.result_board.is_empty() {
                bail!("S6 FR 终态无结果板");
            }
            std::fs::write(&s6_path, &fr.result_board)?;
            println!(
                "[S6] FR 回导板已落盘 {s6_path}（{} bytes）",
                fr.result_board.len()
            );
        }
    }

    if args.drc {
        let final_board = if std::path::Path::new(&s6_path).exists() {
            s6_path.clone()
        } else {
            s5_path.clone()
        };
        run_drc(&final_board)?;
    } else {
        println!(
            "收尾提醒: 候选板为候选——跑 `kdesign v35-flow … --drc` 或本地 union-DRC 复验后交付"
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 内部工具
// ---------------------------------------------------------------------------

async fn connect(
    addr: &str,
) -> Result<kroute_server::proto::KRouteClient<tonic::transport::Channel>> {
    let client = kroute_server::proto::KRouteClient::connect(addr.to_string())
        .await
        .with_context(|| format!("连接 {addr} 失败（kroute-server serve 起了吗？）"))?
        .max_decoding_message_size(64 * 1024 * 1024)
        .max_encoding_message_size(64 * 1024 * 1024);
    Ok(client)
}

/// 板 → 波前网格导出（S2 评分与 S4 同源；dilate=间距膨胀格数）
fn score_export(
    board: &kicad_json5::Board,
    res: f64,
    dilate: usize,
    layers: usize,
) -> Result<kicad_cdb::router::WavefrontGridExport> {
    let has_bga = board.footprints.iter().any(|fp| {
        let u = fp.lib_id.to_uppercase();
        u.contains("BGA") || u.contains("CSP")
    });
    let grid_res = if res > 0.0 {
        res
    } else if has_bga {
        0.25
    } else {
        0.5
    };
    // M3：层数→栈配置完整映射。2=两层全信号；3/5/6/8+=n_signal 通用栈；
    // 4=预设四信号（等价 n_signal(4)）。任意 n≥2 均有定义，无静默降级。
    let cfg = match layers {
        2 => kicad_cdb::layer_config::BoardLayerConfig::two_layer(),
        n if n >= 3 => kicad_cdb::layer_config::BoardLayerConfig::n_signal_layer(n),
        _ => anyhow::bail!("信号层数至少为 2（传 0/1 属参数错误）"),
    };
    kicad_cdb::router::export_wavefront_grid_dilated(board, cfg, grid_res, dilate)
}

/// 差分对映射（与顺序解耦：pair id 按导出序固定，换布线顺序不漂移）
fn pair_map(
    export: &kicad_cdb::router::WavefrontGridExport,
) -> std::collections::HashMap<u32, u32> {
    let mut pair_ids: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    export
        .nets
        .iter()
        .map(|n| {
            let pair = crate::commands::diff_pair_key(&n.name).map_or(0, |k| {
                let next = pair_ids.len() as u32 + 1;
                *pair_ids.entry(k).or_insert(next)
            });
            (n.net_id, pair)
        })
        .collect()
}

/// M3 顺序择优：生成 K 个布线顺序（导出序 / 短距优先 / 固定种子洗牌，确定性）
fn build_orders(nets: &[NetRoute], k: u32) -> Vec<Vec<usize>> {
    let base: Vec<usize> = (0..nets.len()).collect();
    let mut orders = vec![base.clone()];
    if k >= 2 && !nets.is_empty() {
        // 短距优先（曼哈顿；层差权重放大——过孔贵）
        let mut by_span = base.clone();
        by_span.sort_by_key(|&i| {
            let n = &nets[i];
            3 * ((n.gl as i64 - n.sl as i64).abs())
                + (n.gr as i64 - n.sr as i64).abs()
                + (n.gc as i64 - n.sc as i64).abs()
        });
        orders.push(by_span);
    }
    let mut s = 0x9E37_79B9_7F4A_7C15u64 ^ (nets.len() as u64);
    while orders.len() < k as usize && nets.len() > 1 {
        let mut o = base.clone();
        for i in (1..o.len()).rev() {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            let j = (s % (i as u64 + 1)) as usize;
            o.swap(i, j);
        }
        if !orders.contains(&o) {
            orders.push(o);
        } else {
            break; // 小集合洗牌空间耗尽
        }
    }
    orders
}

/// 导出结果 + 顺序 → RouteGridBatchReq（net-aware 默认开）
fn batch_req_from_export(
    export: &kicad_cdb::router::WavefrontGridExport,
    net_aware: bool,
    max_rounds: u32,
    via_cost: f32,
    order: Option<&[usize]>,
) -> RouteGridBatchReq {
    let pairs = pair_map(export);
    let mut nets: Vec<NetRoute> = export
        .nets
        .iter()
        .map(|n| {
            // world_to_grid 返回 (col,row)；导出的 start/goal 同序
            NetRoute {
                net_id: n.net_id,
                sl: 0, // 导出网格只含信号层，0 = 第一信号层
                sr: n.start.1 as u32,
                sc: n.start.0 as u32,
                gl: 0,
                gr: n.goal.1 as u32,
                gc: n.goal.0 as u32,
                pair: pairs.get(&n.net_id).copied().unwrap_or(0),
            }
        })
        .collect();
    if let Some(order) = order {
        nets = order.iter().map(|&i| nets[i]).collect();
    }
    RouteGridBatchReq {
        cols: export.cols as u32,
        rows: export.rows as u32,
        layers: export.layers.len() as u32,
        grid: export
            .grid_u32
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect(),
        via_cost,
        max_rounds,
        nets,
        net_aware,
    }
}

#[allow(clippy::too_many_arguments)]
fn save_batch_reply(
    path: &str,
    reply: &RouteGridBatchReply,
    cols: usize,
    rows: usize,
    layers: usize,
    net_aware: bool,
    order_stats: &[serde_json::Value],
) -> Result<()> {
    let per_layer = cols * rows;
    let to_triple = |&fi: &u32| {
        let l = fi as usize / per_layer;
        let rem = fi as usize % per_layer;
        vec![l as u32, (rem / cols) as u32, (rem % cols) as u32]
    };
    let doc = serde_json::json!({
        "cols": cols, "rows": rows, "layers": layers, "net_aware": net_aware,
        "engine": reply.engine,
        "orders_tried": order_stats,
        "elapsed_ms": reply.elapsed_ms,
        "results": reply.results.iter().map(|r| serde_json::json!({
            "net_id": r.net_id, "routed": r.routed, "cost": r.cost, "pair": r.pair,
            "path": r.path.iter().map(to_triple).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    });
    std::fs::write(path, serde_json::to_string_pretty(&doc)?)?;
    Ok(())
}

/// 断点恢复：s4-batch.json → RouteGridBatchReply（只回填 results/elapsed_ms）
fn load_batch_reply(path: &str) -> Result<RouteGridBatchReply> {
    let doc: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path)?)?;
    let cols = doc["cols"].as_u64().context("batch.cols")? as usize;
    let rows = doc["rows"].as_u64().context("batch.rows")? as usize;
    let per_layer = cols * rows;
    let mut results = Vec::new();
    for r in doc["results"].as_array().context("batch.results")? {
        let mut path = Vec::new();
        if let Some(arr) = r["path"].as_array() {
            for p in arr {
                let a = p.as_array().context("path 项")?;
                let (l, rr, c) = (
                    a[0].as_u64().unwrap_or(0),
                    a[1].as_u64().unwrap_or(0),
                    a[2].as_u64().unwrap_or(0),
                );
                path.push((l * per_layer as u64 + rr * cols as u64 + c) as u32);
            }
        }
        results.push(kroute_server::proto::pb::NetRouteResult {
            net_id: r["net_id"].as_u64().unwrap_or(0) as u32,
            routed: r["routed"].as_bool().unwrap_or(false),
            cost: r["cost"].as_f64().unwrap_or(0.0) as f32,
            path,
            pair: r["pair"].as_u64().unwrap_or(0) as u32,
        });
    }
    Ok(RouteGridBatchReply {
        results,
        elapsed_ms: doc["elapsed_ms"].as_u64().unwrap_or(0),
        engine: doc["engine"].as_str().unwrap_or("cpu").to_string(),
    })
}

/// net_id → 名字（S5 计划可读性用；断点恢复时从 grid meta 取）
fn net_name_map(export_meta: &serde_json::Value) -> std::collections::HashMap<u32, String> {
    let mut m = std::collections::HashMap::new();
    if let Some(nets) = export_meta["nets"].as_array() {
        for n in nets {
            if let (Some(id), Some(name)) = (
                n["net_id"].as_u64(),
                n["name"].as_str().map(|s| s.to_string()),
            ) {
                m.insert(id as u32, name);
            }
        }
    }
    m
}

/// 可选 DRC 复验：kicad-cli pcb drc JSON 输出 → 违规计数
fn run_drc(board: &str) -> Result<()> {
    let out_json = format!("{}.drc.json", board.trim_end_matches(".kicad_pcb"));
    let status = std::process::Command::new(kicad_cli())
        .args([
            "pcb", "drc", "--format", "json", "--output", &out_json, board,
        ])
        .status()
        .context("kicad-cli 不可用（装 KiCad 或手动跑 union-DRC）")?;
    if !status.success() {
        bail!("kicad-cli pcb drc 退出码非零");
    }
    let doc: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&out_json)?)?;
    let violations = doc["violations"].as_array().map(|v| v.len()).unwrap_or(0);
    let unconnected = doc["unconnected_items"]
        .as_array()
        .map(|v| v.len())
        .unwrap_or(0);
    println!("[DRC] violations={violations} unconnected={unconnected}（报告 {out_json}）");
    if violations == 0 && unconnected == 0 {
        println!("[DRC] PASS——可交付");
    } else {
        println!("[DRC] 未清零——进入 M3 收敛（增量修复/混合分流调参）");
    }
    Ok(())
}

/// kicad-cli 解析：PATH 优先，macOS 回落 KiCad.app bundle 内置
fn kicad_cli() -> String {
    if std::process::Command::new("kicad-cli")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        return "kicad-cli".into();
    }
    let bundled = "/Applications/KiCad.app/Contents/MacOS/kicad-cli";
    if std::path::Path::new(bundled).exists() {
        return bundled.into();
    }
    "kicad-cli".into()
}

/// M3：候选布局的实测盒重叠对数（含 0.5mm margin 的包围盒两两相交检查）。
/// 重叠 = shorting 的布局态根源，S2 评分据此惩罚重叠多的候选。
fn count_overlap_pairs(
    refs: &[String],
    positions: &[(f64, f64, f64)],
    dims: &std::collections::HashMap<String, (f64, f64)>,
) -> usize {
    let rects: Vec<(f64, f64, f64, f64)> = refs
        .iter()
        .zip(positions.iter())
        .filter_map(|(r, (x, y, _))| dims.get(r).map(|(w, h)| (*x, *y, *w, *h)))
        .collect();
    let mut n = 0usize;
    for i in 0..rects.len() {
        for j in (i + 1)..rects.len() {
            let (cx1, cy1, w1, h1) = rects[i];
            let (cx2, cy2, w2, h2) = rects[j];
            if (cx1 - cx2).abs() < (w1 + w2) / 2.0 && (cy1 - cy2).abs() < (h1 + h2) / 2.0 {
                n += 1;
            }
        }
    }
    n
}
