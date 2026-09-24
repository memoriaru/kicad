# vendor/

构建期下载的二进制（**不入 git**，Docker 构建上下文需要）。

## cuda_nvrtc.tar.xz

NVIDIA CUDA redist 的 nvrtc 组件（cudarc nvrtc feature 运行时 dlopen 用）。


```bash
curl -L -o cuda_nvrtc.tar.xz \
  "https://developer.download.nvidia.com/compute/cuda/redist/cuda_nvrtc/linux-x86_64/cuda_nvrtc-linux-x86_64-12.6.85-archive.tar.xz"
```

版本对应关系见 `https://developer.download.nvidia.com/compute/cuda/redist/redistrib_12.6.3.json`。
