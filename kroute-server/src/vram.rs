//! 显存水位门（4090 为共享卡：同机常有推理等其它显存负载）。
//!
//! 策略：CUDA 任务（RouteGrid）执行前查 NVML free VRAM，低于阈值即拒单
//! （Status::resource_exhausted，带当前水位），绝不与宿主机其它负载抢显存。
//! NVML 不可用的机器（Mac/无 NVIDIA）→ 放行（开发/CI 环境不设卡）。

use anyhow::{Context, Result};
use std::sync::Mutex;

#[derive(Debug, Clone)]
pub struct VramInfo {
    pub total_mb: u64,
    pub free_mb: u64,
    pub used_mb: u64,
}

pub struct VramGate {
    pub min_free_mb: u64,
    nvml: Mutex<Option<nvml_wrapper::Nvml>>,
}

impl VramGate {
    pub fn new(min_free_mb: u64) -> Self {
        let nvml = nvml_wrapper::Nvml::init().ok();
        Self {
            min_free_mb,
            nvml: Mutex::new(nvml),
        }
    }

    /// 当前显存水位（NVML 不可用 → None）
    pub fn probe(&self) -> Option<VramInfo> {
        let guard = self.nvml.lock().ok()?;
        let nvml = guard.as_ref()?;
        let device = nvml.device_by_index(0).ok()?;
        let mem = device.memory_info().ok()?;
        Some(VramInfo {
            total_mb: mem.total / (1024 * 1024),
            free_mb: mem.free / (1024 * 1024),
            used_mb: mem.used / (1024 * 1024),
        })
    }

    /// CUDA 任务准入检查：通过返回当前水位；不足或半初始化（NVML 在但设备缺失）返回 Err。
    /// NVML 完全不可用 → Ok(None)（放行，非 NVIDIA 环境不设卡）。
    pub fn check(&self) -> Result<Option<VramInfo>, String> {
        let Some(info) = self.probe() else {
            return Ok(None); // 无 NVML：开发/CI 环境放行
        };
        if info.free_mb < self.min_free_mb {
            return Err(format!(
                "显存水位不足: free {}MB < 阈值 {}MB（total {}MB，其它负载占用中）——拒绝执行，请稍后重试或下调 --min-free-vram-mb",
                info.free_mb, self.min_free_mb, info.total_mb
            ));
        }
        Ok(Some(info))
    }
}

/// 启动时初始化报告
pub fn init_report(gate: &VramGate) -> String {
    match gate.probe() {
        Some(info) => format!(
            "CUDA GPU 显存 {}/{}MB free，准入阈值 {}MB",
            info.free_mb, info.total_mb, gate.min_free_mb
        ),
        None => "NVML 不可用（无 NVIDIA GPU），显存门不生效".into(),
    }
}

/// 由 NVML 句柄意外失效时重建（驱动重载/容器迁移）
pub fn refresh(gate: &VramGate) -> Result<()> {
    let mut guard = gate
        .nvml
        .lock()
        .map_err(|_| anyhow::anyhow!("NVML 锁中毒"))?;
    if guard.is_none() {
        *guard = Some(nvml_wrapper::Nvml::init().context("NVML 初始化失败")?);
    }
    Ok(())
}
