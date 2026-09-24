//! CUDA 接入自检（feature = "cuda"）。
//!
//! 路线定案（2026-09-07 台式机实测）：WSL2 只透传 CUDA/DX12，无 Vulkan ICD，
//! 所以 4090 GPU 并行走 cudarc driver API + nvrtc 运行时编译。
//! 本模块 = 接入运行测试的最小闭环：设备枚举 + vectorAdd kernel 真实执行 + 回读校验。

/// 自检 kernel：1M float 向量加，覆盖 alloc / H2D / launch / D2H 全链路
#[allow(dead_code)] // cuda feature 专用
const VEC_ADD_CUDA: &str = r#"
extern "C" __global__ void vec_add(const float* a, const float* b, float* c, int n) {
    int i = blockIdx.x * blockDim.x + threadIdx.x;
    if (i < n) c[i] = a[i] + b[i];
}
"#;

#[derive(Debug, Clone, Default)]
pub struct DeviceInfo {
    pub name: String,
    /// compute capability，如 "8.9"
    pub cc: String,
    pub selftest_ok: bool,
    pub selftest_ms: u64,
    /// 失败原因（成功为空）
    pub selftest_err: String,
}

pub struct CudaReport {
    pub devices: Vec<DeviceInfo>,
    pub summary: String,
}

#[cfg(feature = "cuda")]
pub fn self_test() -> CudaReport {
    // cudarc 在驱动库缺失时是 panic 不是 Err（lib.rs 的 dynamic-load 失败路径），
    // 无 GPU 环境必须接住降级，否则 panic 会炸掉 tokio worker
    let outcome = std::panic::catch_unwind(run_all);
    match outcome {
        Ok(Ok(devices)) => {
            let ok = devices.iter().filter(|d| d.selftest_ok).count();
            let summary = if devices.is_empty() {
                "ERR: 无可见 CUDA 设备（--gpus all / 驱动注入？）".to_string()
            } else {
                format!("OK {ok}/{} devices passed vec_add selftest", devices.len())
            };
            CudaReport { devices, summary }
        }
        Ok(Err(e)) => CudaReport {
            devices: Vec::new(),
            summary: format!("ERR: {e:#}"),
        },
        Err(_) => CudaReport {
            devices: Vec::new(),
            summary: "ERR: CUDA 驱动库不可用（无 GPU 机器上属预期）".to_string(),
        },
    }
}

#[cfg(not(feature = "cuda"))]
pub fn self_test() -> CudaReport {
    CudaReport {
        devices: Vec::new(),
        summary: "disabled (built without cuda feature)".into(),
    }
}

#[cfg(feature = "cuda")]
fn run_all() -> anyhow::Result<Vec<DeviceInfo>> {
    use cudarc::driver::result;
    use cudarc::driver::sys::CUdevice_attribute as Attr;

    result::init()?;
    let count = result::device::get_count()?;
    let mut out = Vec::new();
    for i in 0..count {
        let name = result::device::get_name(i as i32)?;
        let dev = result::device::get(i as i32)?;
        // SAFETY: 属性枚举值与设备句柄均合法，无悬垂
        let (major, minor) = unsafe {
            (
                result::device::get_attribute(
                    dev,
                    Attr::CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MAJOR,
                )?,
                result::device::get_attribute(
                    dev,
                    Attr::CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MINOR,
                )?,
            )
        };
        let (selftest_ok, selftest_ms, selftest_err) = device_vec_add(i as i32);
        out.push(DeviceInfo {
            name,
            cc: format!("{major}.{minor}"),
            selftest_ok,
            selftest_ms,
            selftest_err,
        });
    }
    Ok(out)
}

/// 在指定设备上跑 1M 向量加并回读校验。
#[cfg(feature = "cuda")]
fn device_vec_add(ordinal: i32) -> (bool, u64, String) {
    use cudarc::driver::{CudaContext, LaunchConfig};
    use cudarc::nvrtc::compile_ptx;
    use std::time::Instant;

    let t0 = Instant::now();
    let run = || -> anyhow::Result<()> {
        use cudarc::driver::PushKernelArg;
        const N: usize = 1 << 20;
        let a: Vec<f32> = (0..N).map(|i| i as f32 * 0.5).collect();
        let b: Vec<f32> = (0..N).map(|i| i as f32 * 2.0).collect();
        let expect: Vec<f32> = a.iter().zip(&b).map(|(x, y)| x + y).collect();

        let ptx = compile_ptx(VEC_ADD_CUDA)?;
        let ctx = CudaContext::new(ordinal as usize)?;
        let stream = ctx.default_stream();
        let module = ctx.load_module(ptx)?;
        let func = module.load_function("vec_add")?;

        let mut dev_a = stream.alloc_zeros::<f32>(N)?;
        let mut dev_b = stream.alloc_zeros::<f32>(N)?;
        let mut dev_c = stream.alloc_zeros::<f32>(N)?;
        stream.memcpy_htod(&a, &mut dev_a)?;
        stream.memcpy_htod(&b, &mut dev_b)?;
        let n_i = N as i32;

        let cfg = LaunchConfig::for_num_elems(N as u32);
        unsafe {
            stream
                .launch_builder(&func)
                .arg(&dev_a)
                .arg(&dev_b)
                .arg(&mut dev_c)
                .arg(&n_i)
                .launch(cfg)?;
        }
        stream.synchronize()?;

        let mut c: Vec<f32> = vec![0.0; N];
        stream.memcpy_dtoh(&dev_c, &mut c)?;
        anyhow::ensure!(c == expect, "vec_add 回读校验失败");
        Ok(())
    };
    match run() {
        Ok(()) => (true, t0.elapsed().as_millis() as u64, String::new()),
        Err(e) => (false, t0.elapsed().as_millis() as u64, format!("{e:#}")),
    }
}
