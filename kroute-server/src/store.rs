//! JobStore：jobs/<job_id>/ 目录持久化 + 状态机。
//!
//! 目录布局：
//! ```text
//! jobs/
//!   <job_id>/            # = sha256(backend|board|strategy|note) 前 16 hex
//!     meta.json          # JobMeta（原子写：tmp + rename）
//!     board.kicad_pcb    # 输入板（fr 管线会在其上就地布线并保存）
//!     result.kicad_pcb   # DONE 后从 board 拷贝的稳定产物
//!     log.txt            # 全量日志（后端 stdout/stderr 追加）
//!     cancel.flag        # 出现即请求取消（执行器轮询后删除）
//! ```

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::proto::pb::{Backend, JobState};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobMeta {
    pub job_id: String,
    pub backend: i32,
    pub board_name: String,
    pub router_args: String,
    pub timeout_secs: u64,
    #[serde(default)]
    pub keep_traces: bool,
    pub note: String,
    pub state: i32,
    pub created_at: u64,
    pub started_at: Option<u64>,
    pub finished_at: Option<u64>,
    pub error: Option<String>,
}

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn job_id_for(backend: i32, board: &[u8], args: &str, timeout_secs: u64, note: &str) -> String {
    let mut h = Sha256::new();
    h.update(backend.to_le_bytes());
    h.update([0]);
    h.update(board);
    h.update([0]);
    h.update(args.as_bytes());
    h.update([0]);
    h.update(timeout_secs.to_le_bytes());
    h.update([0]);
    h.update(note.as_bytes());
    hex::encode(h.finalize())[..16].to_string()
}

#[derive(Debug, Clone)]
pub struct JobStore {
    root: PathBuf,
}

impl JobStore {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        std::fs::create_dir_all(&root)
            .with_context(|| format!("创建 jobs 目录失败: {}", root.display()))?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn dir_of(&self, job_id: &str) -> PathBuf {
        self.root.join(job_id)
    }

    pub fn meta_path(&self, job_id: &str) -> PathBuf {
        self.dir_of(job_id).join("meta.json")
    }
    pub fn board_path(&self, job_id: &str) -> PathBuf {
        self.dir_of(job_id).join("board.kicad_pcb")
    }
    pub fn result_path(&self, job_id: &str) -> PathBuf {
        self.dir_of(job_id).join("result.kicad_pcb")
    }
    pub fn log_path(&self, job_id: &str) -> PathBuf {
        self.dir_of(job_id).join("log.txt")
    }
    pub fn cancel_flag(&self, job_id: &str) -> PathBuf {
        self.dir_of(job_id).join("cancel.flag")
    }
    pub fn scripts_dir(&self, job_id: &str) -> PathBuf {
        self.dir_of(job_id).join("scripts")
    }

    /// 创建 job；已存在同 id 时返回 reused=true 且不覆盖任何文件。
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        &self,
        backend: Backend,
        board: &[u8],
        board_name: &str,
        args: &str,
        timeout_secs: u64,
        note: &str,
        keep_traces: bool,
    ) -> Result<(JobMeta, bool)> {
        let job_id = job_id_for(backend as i32, board, args, timeout_secs, note);
        let dir = self.dir_of(&job_id);
        if dir.exists() {
            let meta = self.read_meta(&job_id)?;
            return Ok((meta, true));
        }
        std::fs::create_dir_all(&dir)?;
        let meta = JobMeta {
            job_id: job_id.clone(),
            backend: backend as i32,
            board_name: board_name.to_string(),
            router_args: args.to_string(),
            timeout_secs,
            keep_traces,
            note: note.to_string(),
            state: JobState::Queued as i32,
            created_at: unix_now(),
            started_at: None,
            finished_at: None,
            error: None,
        };
        self.write_meta(&meta)?;
        std::fs::write(self.board_path(&job_id), board)
            .with_context(|| format!("写输入板失败: {}", self.board_path(&job_id).display()))?;
        std::fs::File::create(self.log_path(&job_id))?;
        Ok((meta, false))
    }

    fn read_meta(&self, job_id: &str) -> Result<JobMeta> {
        let path = self.meta_path(job_id);
        let txt = std::fs::read_to_string(&path)
            .with_context(|| format!("读 meta 失败: {}", path.display()))?;
        serde_json::from_str(&txt)
            .with_context(|| format!("meta.json 解析失败: {}", path.display()))
    }

    fn write_meta(&self, meta: &JobMeta) -> Result<()> {
        let path = self.meta_path(&meta.job_id);
        let tmp = path.with_extension("json.tmp");
        let json = serde_json::to_string_pretty(meta)?;
        std::fs::File::create(&tmp)?.write_all(json.as_bytes())?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    pub fn get(&self, job_id: &str) -> Result<JobMeta> {
        if !self.meta_path(job_id).exists() {
            bail!("job 不存在: {job_id}");
        }
        self.read_meta(job_id)
    }

    pub fn update_state(&self, job_id: &str, state: JobState, error: Option<String>) -> Result<()> {
        let mut meta = self.read_meta(job_id)?;
        meta.state = state as i32;
        meta.error = error;
        match state {
            JobState::Running => meta.started_at = Some(unix_now()),
            JobState::Done | JobState::Failed | JobState::Cancelled => {
                meta.finished_at = Some(unix_now())
            }
            _ => {}
        }
        self.write_meta(&meta)
    }

    pub fn append_log(&self, job_id: &str, line: &str) -> Result<()> {
        use std::io::Seek;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.log_path(job_id))?;
        f.seek(std::io::SeekFrom::End(0))?;
        writeln!(f, "{line}")?;
        Ok(())
    }

    pub fn log_tail(&self, job_id: &str) -> (u64, String) {
        let content = std::fs::read_to_string(self.log_path(job_id)).unwrap_or_default();
        let size = content.len() as u64;
        let last = content
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("")
            .to_string();
        (size, last)
    }

    pub fn request_cancel(&self, job_id: &str) -> Result<()> {
        std::fs::File::create(self.cancel_flag(job_id))?;
        Ok(())
    }

    pub fn has_cancel(&self, job_id: &str) -> bool {
        self.cancel_flag(job_id).exists()
    }

    pub fn clear_cancel(&self, job_id: &str) {
        let _ = std::fs::remove_file(self.cancel_flag(job_id));
    }

    /// 列出全部 job（按 created_at 倒序）。
    pub fn list(&self) -> Result<Vec<JobMeta>> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(&self.root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let id = entry.file_name().to_string_lossy().to_string();
            if self.meta_path(&id).exists() {
                match self.read_meta(&id) {
                    Ok(m) => out.push(m),
                    Err(e) => tracing::warn!(job = %id, "meta 读取失败: {e:#}"),
                }
            }
        }
        out.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then(a.job_id.cmp(&b.job_id))
        });
        Ok(out)
    }

    /// 重启恢复：崩溃遗留的 RUNNING 标记为 FAILED；返回仍处于 QUEUED 的任务（需重新入队）。
    pub fn recover(&self) -> Result<Vec<JobMeta>> {
        let mut queued = Vec::new();
        for meta in self.list()? {
            match JobState::try_from(meta.state) {
                Ok(JobState::Running) => {
                    tracing::warn!(job = %meta.job_id, "重启发现遗留 RUNNING，标记 FAILED");
                    self.update_state(&meta.job_id, JobState::Failed, Some("服务重启中断".into()))?;
                }
                Ok(JobState::Queued) => queued.push(meta),
                _ => {}
            }
        }
        Ok(queued)
    }
}

pub fn is_terminal(state: JobState) -> bool {
    matches!(
        state,
        JobState::Done | JobState::Failed | JobState::Cancelled
    )
}

/// Watch 轮询间隔
pub const WATCH_INTERVAL: Duration = Duration::from_millis(500);
