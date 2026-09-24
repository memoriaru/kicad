//! GPU adapter 枚举（feature = "gpu"）。
//!
//! 只做 health 报告（当前跑在哪个后端），不参与求解——生产 4090 在 WSL2 下
//! 无 Vulkan ICD（只透传 CUDA/DX12），求解路径 09-08 定案 cudarc CUDA；
//! 原三档后端（Metal/lavapipe/Vulkan）的 WGSL 求解路径留在 kicad-cdb
//! gpu-router feature（自研 A* 时代遗产，随 09-01 freerouting 定案退役）。

#[derive(Debug, Clone)]
pub struct AdapterInfo {
    pub name: String,
    pub backend: String,
}

#[cfg(feature = "gpu")]
pub fn enumerate_adapters() -> Vec<AdapterInfo> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
    instance
        .enumerate_adapters(wgpu::Backends::all())
        .iter()
        .map(|a| {
            let info = a.get_info();
            AdapterInfo {
                name: info.name,
                backend: format!("{:?}", info.backend),
            }
        })
        .collect()
}

#[cfg(not(feature = "gpu"))]
pub fn enumerate_adapters() -> Vec<AdapterInfo> {
    Vec::new()
}
