# kroute-server

gRPC 布线服务器：把 KiCad PCB 的自动布线/布局优化做成可远程调用的服务。

三层后端体系：
- **wavefront** — 自研波前扩展网格布线（CPU Dijkstra 参照/回退 + wgpu compute shader GPU 档 + 可选 CUDA 档），net-aware、支持过孔代价与批量多网求解；
- **freerouting 管线** — 清线 → DSN 导出 → 净化 → [freerouting](https://github.com/freerouting/freerouting) 求解 → SES 回导 → 布局指纹校验；
- **noop** — 冒烟后端，原样返回输入板。

## gRPC API（`proto/kroute.proto`）

```protobuf
service KRoute {
  rpc Health(HealthReq) returns (HealthReply);              // 就绪探测（各后端可用性）
  rpc SubmitJob(SubmitReq) returns (JobHandle);             // 提交后台任务（幂等：同 payload 复用）
  rpc Watch(JobRef) returns (stream Progress);              // 订阅任务进度（server-stream）
  rpc FetchResult(JobRef) returns (FetchReply);             // 拉取结果（板文件/DRC/日志）
  rpc Cancel(JobRef) returns (CancelReply);                 // 取消任务
  rpc ListJobs(ListReq) returns (ListReply);                // 任务列表
  rpc RouteGrid(RouteGridReq) returns (RouteGridReply);     // 同步网格布线（单目标）
  rpc RouteGridBatch(RouteGridBatchReq) returns (RouteGridBatchReply); // 批量多网网格布线
  rpc LayoutOptimize(LayoutOptimizeReq) returns (LayoutOptimizeReply); // SA 布局优化
}
```

## 快速开始

```sh
cargo run --release -- serve --port 50051   # 默认 CPU/wgpu 后端
```

另开终端，用内置 CLI 客户端：

```sh
cargo run -- health --port 50051
cargo run -- submit --board board.kicad_pcb --backend wavefront
cargo run -- watch <job-id>
cargo run -- fetch <job-id> --out result/
```

## 后端与环境

| 后端 | feature | 说明 |
| --- | --- | --- |
| `wavefront`（CPU Dijkstra） | 默认 | 永远可用，同时是全部 GPU 引擎的对拍参照 |
| `wavefront`（wgpu） | `gpu`（默认） | compute shader 波前扩展；无 GPU 环境配 lavapipe（mesa）可全链路冒烟 |
| CUDA | `cuda` | cudarc 绑定，需构建期准备 NVRTC（见 `vendor/README.md`） |
| freerouting | — | 需 Java + freerouting jar（GPLv3，见下） |
| `noop` | — | 冒烟/联通性测试 |

跨后端浮点不位级一致：确定性口径**仅限同机同后端**；跨机结果只做并集/择优。

## freerouting 依赖

freerouting jar 为 **GPLv3** 独立项目，本仓库不捆绑分发。获取方式：

```sh
curl -L -o freerouting.jar \
  https://github.com/freerouting/freerouting/releases/latest/download/freerouting.jar
```

通过环境变量/参数指定 `java` 可执行文件与 jar 路径后，FR 后端即可用。
`scripts/` 内含 DSN 导出/净化/SES 回导所需的 KiCad pcbnew Python 脚本。
Docker 镜像在构建期自动下载 jar（见 `Dockerfile`）。

## 测试

```sh
cargo test                          # 默认档（含 wgpu 编译）
cargo test --no-default-features    # 纯 CPU 档（CI 使用）
cargo test -- --ignored             # 真实 freerouting 管线端到端（需本机 java+jar）
```

任务运行产物写入 `jobs/`（已被 gitignore），含板文件与日志，属运行时数据。

## 工具链

依赖 [kicad-cdb](https://github.com/memoriaru/kicad/tree/main/kicad-cdb) 与
[kicad-json5](https://github.com/memoriaru/kicad/tree/main/kicad-json5)。

## License

MIT；freerouting jar 为外部 GPLv3 组件，按上述方式独立获取，不随本仓库分发。
