# kroute-server 镜像（多阶段）
# 构建上下文 = 仓库根（.dockerignore 已白名单必要文件）：
#   docker build -t kroute-server:0.1.0 -f kroute-server/Dockerfile .
#
# 生产（Windows 4090）：WSL2 内 docker-ce + nvidia-container-toolkit，
#   NVIDIA_DRIVER_CAPABILITIES 必须含 graphics 才注入 Vulkan ICD（wgpu 才能枚举到 4090）。
# 本镜像自带 mesa lavapipe（CPU 实现的 Vulkan），无 GPU 也能跑通 wgpu 全链路冒烟。

# ---------------------------------------------------------------- builder
FROM rust:1-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends protobuf-compiler && rm -rf /var/lib/apt/lists/*

# cudarc 构建期走预生成绑定（CUDARC_CUDA_VERSION），运行期 dlopen 容器内 libcuda/libnvrtc
ENV CUDARC_CUDA_VERSION=12060

WORKDIR /src
COPY kroute-server/Cargo.toml kroute-server/build.rs ./kroute-server/
COPY kroute-server/proto ./kroute-server/proto
COPY kroute-server/src ./kroute-server/src
COPY kroute-server/scripts ./kroute-server/scripts
COPY kicad-cdb/Cargo.toml ./kicad-cdb/
COPY kicad-cdb/src ./kicad-cdb/src
COPY kicad-json5/Cargo.toml ./kicad-json5/
COPY kicad-json5/src ./kicad-json5/src
COPY kicad-symgen/Cargo.toml ./kicad-symgen/
COPY kicad-symgen/src ./kicad-symgen/src

# 先只编译依赖（利用层缓存：改业务代码不用重拉依赖）
RUN cd kroute-server && mkdir -p src tests && \
    echo "pub mod backends;pub mod cuda;pub mod gpuinfo;pub mod proto;pub mod server;pub mod store;pub mod vram;pub mod wavefront;pub mod wavefront_wgpu;" > src/lib.rs && \
    echo "fn main(){}" > src/main.rs && echo "" > tests/integration.rs && \
    cargo build --release --features "gpu cuda"

# 拷贝真实源码重新编译
COPY kroute-server/Cargo.toml kroute-server/build.rs ./kroute-server/
COPY kroute-server/proto ./kroute-server/proto
COPY kroute-server/src ./kroute-server/src
COPY kroute-server/scripts ./kroute-server/scripts
RUN cd kroute-server && touch src/main.rs && cargo build --release --features "gpu cuda"

# ---------------------------------------------------------------- runtime
# 基础镜像用本地 ubuntu（Docker Hub 间歇不可达），CUDA 组件最小化自带：
#   - libnvrtc（redist 包，nvrtc 运行时编译 PTX）
#   - libcuda 由 nvidia-container-toolkit 在 --gpus all 时注入（仅运行期需要）
FROM ubuntu:24.04

# nvrtc 12.6（cudarc nvrtc feature 运行时 dlopen；libnvrtc.so → .12 建链保证 dlopen 名字可解析）
COPY kroute-server/vendor/cuda_nvrtc.tar.xz /tmp/cuda_nvrtc.tar.xz
RUN apt-get update && apt-get install -y --no-install-recommends xz-utils && rm -rf /var/lib/apt/lists/* \
    && mkdir -p /usr/local/cuda-12.6/lib64 \
    && tar -xf /tmp/cuda_nvrtc.tar.xz -C /tmp \
    && cp /tmp/cuda_nvrtc-linux-x86_64-*/lib/libnvrtc*.so* /usr/local/cuda-12.6/lib64/ \
    && ln -sf libnvrtc.so.12 /usr/local/cuda-12.6/lib64/libnvrtc.so \
    && rm -rf /tmp/cuda_nvrtc* \
    && echo "/usr/local/cuda-12.6/lib64" > /etc/ld.so.conf.d/cuda.conf \
    && ldconfig

ENV DEBIAN_FRONTEND=noninteractive

# openjdk-17：freerouting 1.9 要求 Java 17+
# libvulkan1 + mesa-vulkan-drivers：lavapipe（CPU Vulkan，无 GPU 冒烟）；NVIDIA ICD 由 container toolkit 注入
# libgl1 等：KiCad AppImage 内 pcbnew 的运行时依赖
RUN apt-get update && apt-get install -y --no-install-recommends \
        openjdk-17-jre-headless \
        python3 \
        curl \
        ca-certificates \
        libvulkan1 \
        mesa-vulkan-drivers \
        vulkan-tools \
        libgl1 \
        libglu1-mesa \
        libxkbcommon0 \
        fontconfig \
    && rm -rf /var/lib/apt/lists/*

# KiCad 10 AppImage：ubuntu 源的 kicad 7 读不了 KiCad 10 格式的板，
# 官方无 10.x PPA → 用 AppImage 解包（--appimage-extract 不依赖 FUSE）
ARG KICAD_APPIMAGE_URL=https://github.com/KiCad/kicad-source-mirror/releases/download/10.0.5/kicad-10.0.5-x86_64.AppImage.tar
RUN mkdir -p /opt/kicad10 \
    && curl -sL "$KICAD_APPIMAGE_URL" -o /tmp/kicad.tar \
    && tar -xf /tmp/kicad.tar -C /opt/kicad10 \
    && cd /opt/kicad10 && ./*.AppImage --appimage-extract >/dev/null \
    && rm -f ./*.AppImage /tmp/kicad.tar

# Xvfb + full JRE：freerouting 1.9 启动即取 getScreenSize()（AWT headful），
# headless JRE 缺 libawt_xawt 且容器无 DISPLAY → 用虚拟 X server 供DISPLAY=:99
RUN apt-get update && apt-get install -y --no-install-recommends \
        xvfb \
        openjdk-17-jre \
    && rm -rf /var/lib/apt/lists/*

# freerouting jar：GPLv3 独立项目, 构建期从官方 Release 下载(不随仓库分发)。
# 可用 FREEROUTING_VERSION 覆盖; 离线环境可预置 jar 后用 --build-arg 跳过下载。
ARG FREEROUTING_VERSION=1.9.0
RUN mkdir -p /opt/freerouting \
    && curl -fL -o /opt/freerouting/freerouting-${FREEROUTING_VERSION}.jar \
      "https://github.com/freerouting/freerouting/releases/download/v${FREEROUTING_VERSION}/freerouting-${FREEROUTING_VERSION}.jar"

COPY --from=builder /src/kroute-server/target/release/kroute-server /usr/local/bin/kroute-server

ENV KROUTE_FR_JAR=/opt/freerouting/freerouting-1.9.0.jar \
    KROUTE_KICAD_PYTHON=/opt/kicad10/squashfs-root/usr/bin/python3.11 \
    KROUTE_JAVA=/usr/bin/java \
    CUDARC_CUDA_VERSION=12060 \
    NVIDIA_DRIVER_CAPABILITIES=compute,utility,graphics

# nvml-wrapper 硬编码 dlopen "libnvidia-ml.so"（无后缀），toolkit 只注入 .so.1 → 建链
RUN ln -sf /usr/lib/x86_64-linux-gnu/libnvidia-ml.so.1 /usr/lib/x86_64-linux-gnu/libnvidia-ml.so

RUN mkdir -p /data/jobs
WORKDIR /data
EXPOSE 50051

# 起 Xvfb 供 freerouting 的 AWT headful 调用，再交棒服务本体（"$@" 透传覆盖 CMD）
COPY kroute-server/entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
