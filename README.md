# kicad-render

KiCad 原理图 SVG 渲染器，从 KiCanvas JS 原理图渲染器 1:1 移植到 Rust。将 `.kicad_sch` 文件渲染为高质量 SVG 矢量图。

## 功能

- **高保真渲染** — 1:1 复现 KiCanvas JS 渲染逻辑，视觉效果与 KiCad 一致
- **完整元素支持** — 导线、元件符号、引脚、结点、标签、文本、No-Connect 标记、图框
- **KiCad 主题色** — 使用 KiCad 官方默认配色
- **精确尺寸** — 所有度量匹配 KiCad 内部单位（mm）
- **WASM 支持** — 可编译为 WebAssembly 在浏览器中渲染

## 安装

```bash
cargo build --release
# 二进制: target/release/kicad-render
```

## CLI 用法

```bash
# 基本用法 — 输出到同名 .svg 文件
kicad-render schematic.kicad_sch

# 指定输出路径
kicad-render schematic.kicad_sch -o output.svg
```

CLI 会解析原理图文件，打印统计信息（导线数、元件数、结点数等），然后渲染为带正确 viewBox 和白色背景的 SVG（3x 缩放）。

## 库用法

```rust
use kicad_render::SchematicRenderer;
use kicad_json5::parser::schematic::SchematicParser;

// 解析 KiCad 原理图
let schematic = SchematicParser::parse_file("schematic.kicad_sch")?;

// 渲染为 SVG
let renderer = SchematicRenderer::new(&schematic);
let svg = renderer.render_to_svg()?;

// 保存
std::fs::write("output.svg", svg)?;
```

## 架构

```
kicad-render/
├── src/
│   ├── lib.rs                  # 库入口
│   ├── main.rs                 # CLI 入口
│   ├── render_core/            # 基础类型和图元
│   │   ├── point.rs            # Point, Matrix
│   │   └── color.rs            # 颜色定义
│   ├── renderer/               # 渲染器 trait 和 SVG 实现
│   │   ├── mod.rs              # Renderer trait
│   │   └── svg_renderer.rs     # SVG 后端
│   ├── schematic_renderer.rs   # 渲染编排器
│   ├── painter/                # 专用绘制器
│   │   ├── pin_painter.rs      # 引脚渲染
│   │   ├── wire_painter.rs     # 导线/总线渲染
│   │   ├── symbol_painter.rs   # 元件符号渲染
│   │   ├── label_painter.rs    # 标签渲染（local/global/hierarchical）
│   │   └── junction_painter.rs # 结点渲染
│   ├── layer.rs                # 图层管理
│   ├── text.rs                 # 文本渲染和标记处理
│   ├── bridge.rs               # kicad-json5 IR → 渲染类型转换
│   └── constants.rs            # 渲染常量和 KiCad 主题色
├── MODULE_REVIEW.md            # JS→Rust 移植对比审计
├── MIGRATION_AUDIT.md          # 移植变更记录
└── tests/
```

### 渲染管线

```
.kicad_sch → kicad-json5 Parser → IR → Bridge → SchematicRenderer
                                                  ├── WirePainter
                                                  ├── SymbolPainter
                                                  ├── PinPainter
                                                  ├── LabelPainter
                                                  └── JunctionPainter
                                              → SVG 输出
```

## Features

| Feature | 默认 | 说明 |
|---------|------|------|
| `svg` | 启用 | SVG 渲染后端 |
| `wasm` | 禁用 | WebAssembly + Canvas 后端 |

启用 WASM：

```bash
cargo build --features wasm --target wasm32-unknown-unknown
```

## 依赖

- `kicad-json5` — KiCad S-expression 解析（本地依赖）
- `thiserror` / `anyhow` — 错误处理
- `wasm-bindgen` / `web-sys` / `js-sys` — WASM 支持（可选）

## 与 kicad-json5 的关系

`kicad-render` 使用 `kicad-json5` 作为解析引擎。`kicad-json5` 负责 S-expression → IR 的转换，`kicad-render` 的 `bridge` 模块将 IR 转为渲染类型，再由各 Painter 绘制。

```
kicad-json5 (解析/编译)  →  kicad-render (渲染/可视化)
```
