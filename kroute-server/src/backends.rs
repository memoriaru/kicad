//! 执行后端：freerouting 全管线（FR）+ noop 冒烟。
//!
//! 管线移植自 `projects/ccd_backend/tools/fr-route.sh`（2026-09-01 定版流程），
//! 差异点：每个 job 独立工作目录（服务端强制隔离，杜绝 fr-route.sh 固定文件名
//! 并发互删 .sig/.pre-fr 的坑）；job 目录即沙箱，失败不回滚客户端板子。

use crate::store::JobStore;
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// 透传给 freerouting 的默认参数（fr-route.sh 定版值）
pub const DEFAULT_ROUTER_ARGS: &str = "-mp 99 -us Hybrid";

#[derive(Debug, Clone)]
pub struct BackendCfg {
    pub java: String,
    pub fr_jar: PathBuf,
    pub kicad_python: String,
    pub system_python: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Done,
    Cancelled,
}

/// 取消专用错误：execute 层用 `err.is::<Cancelled>()` 区分失败与取消
#[derive(Debug)]
pub struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "任务被取消")
    }
}
impl std::error::Error for Cancelled {}

const PCB_CLEAN_PY: &str = include_str!("../scripts/pcb_clean.py");
const DSN_EXPORT_PY: &str = include_str!("../scripts/dsn_export.py");
const DSN_SANITIZE_PY: &str = include_str!("../scripts/dsn_sanitize.py");
const SES_IMPORT_PY: &str = include_str!("../scripts/ses_import.py");
const LAYOUT_DUMP_PY: &str = include_str!("../scripts/layout_dump.py");

fn ensure_not_cancelled(store: &JobStore, job_id: &str) -> Result<()> {
    if store.has_cancel(job_id) {
        return Err(anyhow::Error::new(Cancelled));
    }
    Ok(())
}

fn write_scripts(store: &JobStore, job_id: &str) -> Result<()> {
    let dir = store.scripts_dir(job_id);
    std::fs::create_dir_all(&dir)?;
    for (name, content) in [
        ("pcb_clean.py", PCB_CLEAN_PY),
        ("dsn_export.py", DSN_EXPORT_PY),
        ("dsn_sanitize.py", DSN_SANITIZE_PY),
        ("ses_import.py", SES_IMPORT_PY),
        ("layout_dump.py", LAYOUT_DUMP_PY),
    ] {
        std::fs::write(dir.join(name), content)?;
    }
    Ok(())
}

fn split_args(s: &str) -> Vec<String> {
    s.split_whitespace().map(|s| s.to_string()).collect()
}

async fn pipe_lines<R: tokio::io::AsyncRead + Unpin>(r: &mut R, log_path: &Path) {
    use tokio::io::{AsyncBufReadExt, BufReader};
    let mut lines = BufReader::new(r).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        // 追加写；单行小写入 O_APPEND 原子，双路（stdout/stderr）不会交错撕行
        let _ = store_append(log_path, &line);
    }
}

fn store_append(log_path: &Path, line: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;
    writeln!(f, "{line}")
}

/// 跑一步子进程：stdout/stderr 双路并入 job 日志；运行期间轮询取消标志，命中则 kill。
async fn run_step(
    store: &JobStore,
    job_id: &str,
    label: &str,
    program: &str,
    args: &[String],
    cwd: &Path,
) -> Result<()> {
    store.append_log(
        job_id,
        &format!("[step] {label}: {program} {}", args.join(" ")),
    )?;
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(args)
        .current_dir(cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = cmd
        .spawn()
        .with_context(|| format!("拉起子进程失败: {program}（依赖未就绪？看 Health）"))?;
    let log_path = store.log_path(job_id);
    let log_out = log_path.clone();
    let mut stdout = child.stdout.take().expect("stdout piped");
    let mut stderr = child.stderr.take().expect("stderr piped");
    let t_out = tokio::spawn(async move { pipe_lines(&mut stdout, &log_out).await });
    let t_err = tokio::spawn(async move { pipe_lines(&mut stderr, &log_path).await });

    let status = loop {
        tokio::select! {
            st = child.wait() => break st?,
            _ = tokio::time::sleep(Duration::from_millis(500)) => {
                if store.has_cancel(job_id) {
                    let _ = child.kill().await;
                    let _ = tokio::join!(t_out, t_err);
                    store.append_log(job_id, &format!("[step] {label}: 收到取消，已杀子进程"))?;
                    return Err(anyhow::Error::new(Cancelled));
                }
            }
        }
    };
    let _ = tokio::join!(t_out, t_err);
    if !status.success() {
        bail!("{label} 退出码 {:?}", status.code());
    }
    Ok(())
}

/// 跑一个快脚本并捕获 stdout（用于布局指纹；不做运行中取消，启动前已检查）。
async fn run_capture(
    store: &JobStore,
    job_id: &str,
    label: &str,
    program: &str,
    args: &[String],
    cwd: &Path,
) -> Result<String> {
    let out = tokio::process::Command::new(program)
        .args(args)
        .current_dir(cwd)
        .output()
        .await
        .with_context(|| format!("拉起子进程失败: {program}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        store.append_log(job_id, &format!("[step] {label} 失败: {err}"))?;
        bail!("{label} 退出码 {:?}", out.status.code());
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

fn mtime_of(p: &Path) -> Result<u128> {
    let meta = std::fs::metadata(p).with_context(|| format!("stat 失败: {}", p.display()))?;
    Ok(meta
        .modified()?
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos())
}

/// freerouting 全管线：清线 → DSN 导出 → 净化 → FR → SES 回导 → 布局指纹校验 → result。
pub async fn run_fr(
    store: &JobStore,
    job_id: &str,
    cfg: &BackendCfg,
    router_args: &str,
    keep_traces: bool,
) -> Result<Outcome> {
    ensure_not_cancelled(store, job_id)?;
    write_scripts(store, job_id)?;

    let dir = store.dir_of(job_id);
    let board = store.board_path(job_id);
    let dsn = dir.join("board.dsn");
    let ses = dir.join("board.ses");
    let scripts = store.scripts_dir(job_id);

    let s = |name: &str| scripts.join(name).to_string_lossy().to_string();
    let board_s = board.to_string_lossy().to_string();
    let dsn_s = dsn.to_string_lossy().to_string();
    let ses_s = ses.to_string_lossy().to_string();

    // 1. 清除现有走线/过孔（reroute 语义）；keep_traces=true 时跳过（M3 混合分流：
    //    保留 Direct 已布网进 DSN，freerouting 只布剩余开连接）
    if keep_traces {
        store
            .append_log(job_id, "[run] 混合分流：保留已有走线，跳过清线")
            .ok();
    } else {
        run_step(
            store,
            job_id,
            "清线",
            &cfg.system_python,
            &[s("pcb_clean.py"), board_s.clone()],
            &dir,
        )
        .await?;
    }

    // 2. 布局指纹（回导前后比对，防 SES 搬布局）
    let sig_before = run_capture(
        store,
        job_id,
        "布局指纹(before)",
        &cfg.system_python,
        &[s("layout_dump.py"), board_s.clone()],
        &dir,
    )
    .await?;
    std::fs::write(dir.join("sig.before.txt"), &sig_before)?;

    // 3. 导出 DSN
    run_step(
        store,
        job_id,
        "DSN 导出",
        &cfg.kicad_python,
        &[s("dsn_export.py"), board_s.clone(), dsn_s.clone()],
        &dir,
    )
    .await?;

    // 4. 净化 PN 非 ASCII
    run_step(
        store,
        job_id,
        "DSN 净化",
        &cfg.system_python,
        &[s("dsn_sanitize.py"), dsn_s.clone()],
        &dir,
    )
    .await?;

    // 5. 清旧 SES，防 FR 失败后误回导上一轮
    let _ = std::fs::remove_file(&ses);

    // 6. freerouting 布线
    let mut fr_args = vec![
        "-jar".to_string(),
        cfg.fr_jar.to_string_lossy().to_string(),
        "-de".to_string(),
        dsn_s.clone(),
        "-do".to_string(),
        ses_s.clone(),
    ];
    fr_args.extend(split_args(router_args));
    run_step(store, job_id, "freerouting", &cfg.java, &fr_args, &dir).await?;

    // 7. 校验 SES 产物
    if !ses.exists() {
        bail!("freerouting 未产出 SES");
    }
    if mtime_of(&ses)? < mtime_of(&dsn)? {
        bail!("SES 比 DSN 旧，疑似未重跑");
    }

    // 8. SES 回导 + 保存
    run_step(
        store,
        job_id,
        "SES 回导",
        &cfg.kicad_python,
        &[s("ses_import.py"), board_s.clone(), ses_s],
        &dir,
    )
    .await?;

    // 9. 布局指纹比对
    let sig_after = run_capture(
        store,
        job_id,
        "布局指纹(after)",
        &cfg.system_python,
        &[s("layout_dump.py"), board_s.clone()],
        &dir,
    )
    .await?;
    std::fs::write(dir.join("sig.after.txt"), &sig_after)?;
    if sig_before != sig_after {
        bail!("SES 回导改变了布局（SES 与当前布局不一致）——拒绝交付，见 sig.before/after.txt");
    }

    // 10. 稳定产物
    std::fs::copy(&board, store.result_path(job_id))?;
    Ok(Outcome::Done)
}

/// noop 冒烟后端：原样返回输入板。
pub async fn run_noop(store: &JobStore, job_id: &str) -> Result<Outcome> {
    tokio::time::sleep(Duration::from_millis(200)).await;
    ensure_not_cancelled(store, job_id)?;
    std::fs::copy(store.board_path(job_id), store.result_path(job_id))?;
    Ok(Outcome::Done)
}
