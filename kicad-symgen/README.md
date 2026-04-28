# kicad-symgen

KiCad 元件符号 (.kicad_sym) 和封装 (.kicad_mod) 生成器，为 AI 辅助电路设计优化。

与 [kicad-cdb](../kicad-cdb) 元件数据库配合使用，也可独立接受 JSON5 输入。

## 安装

```bash
cd kicad-symgen
cargo build --release
# binary: target/release/symgen
```

## 用法

### 生成符号 (.kicad_sym)

```bash
# 从 JSON5 输入（AI 工作流推荐）
symgen symbol --input fp6277.json5 --output custom.kicad_sym

# 从元件数据库生成
symgen symbol --db components.db --mpn FP6277 --output custom.kicad_sym

# 批量导出整个分类
symgen symbol --db components.db --category IC --output custom.kicad_sym
```

JSON5 输入示例：

```json5
{
  mpn: "FP6277",
  reference_prefix: "U",
  description: "2A synchronous boost converter",
  package: "SOT-23-6",
  kicad_footprint: "Package_TO_SOT_SMD:SOT-23-6",
  pins: [
    { number: "1", name: "LX", electrical_type: "power_out" },
    { number: "2", name: "GND", electrical_type: "power_in" },
    { number: "3", name: "EN", electrical_type: "input" },
    { number: "4", name: "FB", electrical_type: "input" },
    { number: "5", name: "VCC", electrical_type: "power_in" },
    { number: "6", name: "SW", electrical_type: "power_out" },
  ],
}
```

### 生成封装 (.kicad_mod)

```bash
symgen footprint --package DIP-8 --output DIP-8.kicad_mod
symgen footprint --package SOIC-8 --pitch 1.27 --output SOIC-8.kicad_mod
symgen footprint --package SOT-23-6 --output SOT-23-6.kicad_mod
symgen footprint --package TSSOP-20 --output TSSOP-20.kicad_mod
```

支持的封装类型：DIP, SIP, SOIC, SOP, MSOP, TSSOP, QFP, LQFP, TQFP, QFN, DFN, SOT-23, SOT-223, Pin Header, DIP Socket。

### 生成库表

```bash
symgen lib-table --dir ./libraries
# 生成 sym-lib-table 和 fp-lib-table
```

### 批量生成

```bash
symgen batch --db components.db --category IC --output-dir ./libraries --with-footprints
```

## 智能 Pin 布局

符号生成使用智能 pin 布局算法：

- **电源 pin (VCC/VDD)** → 元件体顶部
- **地线 pin (GND/VSS)** → 元件体底部
- **输入 pin** → 左侧
- **输出 pin** → 右侧
- **双向 pin** → 按 pin_group 分配

自动为 lib_id 添加 `custom:` 前缀，解决 KiCad ERC 的 `lib_symbol_issues` 警告。

## 架构

```
kicad-symgen/
  src/
    main.rs                 CLI 入口 (clap)
    model.rs                核心数据模型
    input.rs                输入适配器 (DB/JSON5)
    symbol/
      layout.rs             智能 Pin 布局算法
      sexpr.rs              .kicad_sym S-expression 输出
    footprint/
      pad.rs                焊盘位置计算
      outline.rs            丝印/装配/阻焊层轮廓
      sexpr.rs              .kicad_mod S-expression 输出
      templates/
        dip.rs              DIP/SIP 通孔模板
        soic.rs             SOIC/TSSOP/SOP SMD 模板
        sot.rs              SOT-23/SOT-223 模板
    lib_table.rs            sym-lib-table / fp-lib-table 生成
```

## 工具链配合

| 工具 | 功能 |
|------|------|
| [kicad-cdb](../kicad-cdb) | 元件数据库 (SQLite)，存储 MPN/pin/参数 |
| **kicad-symgen** | 从 DB/JSON5 生成符号和封装文件 |
| [kicad-json5](../kicad-json5) | .kicad_sch ↔ JSON5 双向编译 |
