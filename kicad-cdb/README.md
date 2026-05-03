# kicad-cdb

AI 辅助电路设计的元件数据库，基于 SQLite，支持参数化查询、设计规则检查和 KiCad 符号导出。

## 功能

- **元件管理** — MPN、制造商、分类、封装、生命周期、数据手册链接
- **层级分类** — 树状分类体系（如 `passive > capacitor > ceramic`）
- **Pin 定义** — 引脚编号、名称、电气类型、复用功能
- **电气参数** — 数值/文本参数 + 单位，支持典型值标记
- **参数范围查询** — `capacitance>=1e-7`、`voltage>=25` 等过滤
- **全文搜索** — 跨描述和元数据搜索
- **仿真模型** — SPICE / IBIS / Verilog-AMS / S-parameter 模型存储
- **设计规则引擎** — 数学表达式求值，参数约束检查
- **供应链信息** — 供应商、SKU、价格梯度、库存、交期、MOQ
- **KiCad 导出** — 生成 `.kicad_sym` 符号库文件

## 安装

```bash
cargo build --release
# 二进制: target/release/cdb
```

## CLI 用法

```bash
cdb [OPTIONS] <COMMAND>
```

### 全局选项

| 选项 | 说明 |
|------|------|
| `--db <PATH>` | 数据库路径（默认 `components.db`，支持 `:memory:`） |

### 导入元件

```bash
# 导入单个元件（JSON 格式）
cdb import component.json

# 批量导入
cdb import components.json
```

导入 JSON 格式示例：

```json
{
  "mpn": "LM358",
  "manufacturer": "TI",
  "category": "amplifiers/opamps",
  "description": "Dual Operational Amplifier",
  "package": "SOIC-8",
  "kicad_symbol": "Amplifier_Operational:LM358",
  "pins": [
    { "number": "1", "name": "OUT_A", "electrical_type": "output" },
    { "number": "2", "name": "IN-_A", "electrical_type": "input" },
    { "number": "3", "name": "IN+_A", "electrical_type": "input" },
    { "number": "4", "name": "V-", "electrical_type": "power_in" },
    { "number": "5", "name": "IN+_B", "electrical_type": "input" },
    { "number": "6", "name": "IN-_B", "electrical_type": "input" },
    { "number": "7", "name": "OUT_B", "electrical_type": "output" },
    { "number": "8", "name": "V+", "electrical_type": "power_in" }
  ],
  "parameters": [
    { "name": "supply_voltage_max", "value_numeric": 32.0, "unit": "V" },
    { "name": "gain_bandwidth", "value_numeric": 1.1e6, "unit": "Hz", "typical": true }
  ]
}
```

### 查询元件

```bash
# 按分类
cdb query --category "capacitors"

# 按参数范围
cdb query --param "capacitance>=1e-7"
cdb query --param "voltage>=25"

# 按封装
cdb query --package "SOIC-8"

# 全文搜索
cdb query --search "STM32F4"

# 仅显示有库存
cdb query --in-stock

# 组合过滤
cdb query --category "capacitors" --param "voltage>=25" --search "ceramic"
```

### 查看元件详情

```bash
cdb show LM358
```

### 设计规则检查

```bash
cdb check --rule "power_dissipation" --params "voltage=5,current=0.1" --candidate "resistance=50"
```

### 导出 KiCad 符号库

```bash
# 导出单个元件
cdb export --mpn LM358 --output lm358.kicad_sym

# 导出整个分类
cdb export --category "opamps" --output opamps.kicad_sym
```

### 其他命令

```bash
# 列出所有分类
cdb categories

# 导入仿真模型
cdb import-model --mpn LM358 --model-type spice --path lm358.cir --format spice3
```

## 数据库结构

SQLite，主要表：

| 表 | 说明 |
|----|------|
| `categories` | 层级分类（id, name, parent_id） |
| `components` | 元件主表（mpn, manufacturer, package, lifecycle） |
| `pins` | 引脚定义（pin_number, name, electrical_type） |
| `parameters` | 电气参数 EAV 模式（name, value_numeric, unit） |
| `simulation_models` | 仿真模型（model_type, model_text, format） |
| `design_rules` | 设计规则（condition_expr, formula_expr） |
| `supply_info` | 供应链（supplier, sku, price_breaks, stock） |
| `reference_circuits` | 参考电路（topology, circuit_json） |

## 项目结构

```
kicad-cdb/
├── src/
│   ├── bin/cdb.rs       # CLI 入口
│   ├── db.rs            # 数据库操作
│   ├── models.rs        # 数据结构定义
│   ├── schema.rs        # 表结构定义
│   ├── query.rs         # 查询引擎
│   ├── rules.rs         # 设计规则引擎
│   ├── import.rs        # JSON 导入（单条/批量/upsert）
│   └── lib.rs           # 库入口
├── Cargo.toml
└── tests/               # 集成测试
    ├── schema_test.rs   # Schema 与 CRUD 测试
    └── rules_test.rs    # 规则引擎测试
```

## 依赖

- `rusqlite` — SQLite（bundled）
- `serde` / `serde_json` — 序列化
- `clap` — CLI 参数解析
- `anyhow` — 错误处理

## License

MIT
