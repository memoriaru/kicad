# kicad-json5

KiCad 原理图 S-expression 与 JSON5 的双向编译器，同时支持电路拓扑提取。为 AI 辅助电路设计提供人机友好的中间表示。

## 为什么用 JSON5？

- **人类可读** — 支持注释、无引号键名、尾逗号
- **Token 高效** — 比 S-expression 少 40-50% 的 token
- **AI 友好** — 结构清晰，LLM 可直接理解和生成
- **双向转换** — `.kicad_sch` ↔ `.json5` 完整 round-trip

## 安装

```bash
cargo build --release
# 二进制: target/release/kicad-json5
```

## CLI 用法

### 基本转换

```bash
# S-expression → JSON5
kicad-json5 schematic.kicad_sch -o schematic.json5

# JSON5 → S-expression
kicad-json5 schematic.json5 -o schematic.kicad_sch

# 输出到 stdout
kicad-json5 schematic.kicad_sch
```

### 全部选项

| 选项 | 说明 |
|------|------|
| `-o, --output <FILE>` | 输出文件路径 |
| `-f, --format <FORMAT>` | 输出格式：`json5`、`sexpr`、`topology` |
| `-i, --indent <N>` | 缩进空格数（默认 2） |
| `--kicad-version <V>` | 目标 KiCad 版本：7, 8, 9, 10+ |
| `--no-comments` | JSON5 输出不含注释 |
| `--validate` | 仅验证，不输出 |
| `--debug-ast` | 打印 AST 用于调试 |
| `--power-flags` | JSON5→Sexpr 时自动插入 PWR_FLAG |
| `-v, --verbose` | 详细输出 |

### 示例

```bash
# JSON5 → KiCad，自动插入 PWR_FLAG 和 NC 标记
kicad-json5 carrier-board.json5 -o carrier-board.kicad_sch --power-flags

# 提取电路拓扑
kicad-json5 schematic.kicad_sch -f topology -o topology.json

# 仅验证
kicad-json5 schematic.kicad_sch --validate
```

## JSON5 输入格式

```json5
{
  // 原理图元数据
  version: "20241129",
  generator: "kicad-json5",

  title_block: {
    title: "CCD Carrier Board",
    date: "2026-04-01",
    paper: "A3",
  },

  // 元件实例
  components: [
    {
      ref: "U1",
      lib_id: "Device:R",
      value: "10k",
      position: { x: 100, y: 50, rotation: 0 },
      pins: {
        "1": { net_id: 1, net_name: "VCC" },
        "2": { net_id: 5, net_name: "SDA" },
      },
    },
    {
      ref: "U3",
      lib_id: "custom:ME2802",
      value: "ME2802",
      position: { x: 39.37, y: 110.49, rotation: 0 },
      pins: {
        "1": { net_name: "GND" },
        "2": { net_name: "VOUT_5V" },
        "3": { net_name: "VBAT" },
        "4": { nc: true },   // No-Connect 标记
      },
    },
  ],

  // 网络定义
  nets: [
    { id: 0, name: "GND" },
    { id: 1, name: "VCC" },
    { id: 5, name: "SDA" },
  ],
}
```

## 核心特性

### 标准 Device 库嵌入

内置 KiCad 标准 Device 库常用符号（R, C, L, D, LED, NTC），生成 `.kicad_sch` 时自动嵌入 lib_symbol 定义，无需外部库文件即可通过 ERC。

### 自动标签生成

JSON5→S-expression 时，自动为每个网络连接点生成 `global_label`，放置在引脚连接点的世界坐标位置，确保 KiCad 正确识别网络连通性。

### PWR_FLAG 自动插入

配合 `--power-flags` 选项，自动检测需要电源标记的网络（有 `power_in` 引脚但无 `power_out` 驱动），插入符合 KiCad 官方 `power.kicad_sym` 规范的 PWR_FLAG 符号。

### No-Connect 标记

引脚声明 `"nc": true` 后，在对应引脚位置生成 `(no_connect ...)` 元素，消除 KiCad ERC 的 `pin_not_connected` 警告。

### 拓扑提取

`-f topology` 模式提取电路语义信息：
- 电源域和地线网络识别
- 信号路径追踪
- 功能模块识别（上拉电阻、去耦电容、LED 指示等）

## 项目结构

```
kicad-json5/
├── src/
│   ├── lexer/              # S-expression 分词器
│   │   ├── mod.rs
│   │   ├── token.rs
│   │   └── scanner.rs
│   ├── parser/             # 解析器
│   │   ├── mod.rs
│   │   ├── ast.rs
│   │   ├── schematic.rs    # S-expression → IR
│   │   ├── json5_parser.rs # JSON5 → IR
│   │   └── s_expr_parser.rs
│   ├── ir/                 # 中间表示
│   │   ├── schematic.rs    # 原理图整体结构
│   │   ├── component.rs    # 元件/引脚实例
│   │   ├── net.rs          # 网络/标签/NC
│   │   └── symbol.rs       # 库符号定义
│   ├── codegen/            # 代码生成
│   │   ├── json5_gen.rs    # IR → JSON5
│   │   ├── sexpr_gen.rs    # IR → S-expression
│   │   └── standard_symbols.rs  # 嵌入式标准符号
│   ├── topology/           # 拓扑分析
│   ├── lib.rs
│   ├── main.rs             # CLI 入口
│   └── error.rs
├── docs/                   # 设计文档
│   ├── kicad-sch-design-notes.md
│   ├── topology-evaluation.md
│   └── ...
└── tests/
```

## 依赖

- `clap` — CLI 参数解析
- `serde` / `serde_json` / `serde_json5` — 序列化
- `uuid` — UUID 生成
- `thiserror` / `anyhow` — 错误处理

## License

MIT
