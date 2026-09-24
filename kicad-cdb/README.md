# kicad-cdb

AI 辅助电路设计的核心引擎库：SQLite 元件数据库 + 布局/布线/DRC 引擎 + 设计知识库。

> CLI 入口见 [kicad-designer](https://github.com/memoriaru/kicad/tree/main/kicad-designer)（`kdesign`）；本 crate 为库形态，供其与你的代码调用。

## 功能模块

| 模块 | 说明 |
| --- | --- |
| **元件数据库**（`db`/`schema`） | MPN/制造商/分类/封装/pin/参数/供应链，层级分类，参数范围查询，全文搜索 |
| **自动布线器**（`router`） | 波前扩展网格布线，3D Dijkstra，net-aware 撕扯重试，差分对与 SerDes 等长 |
| **GPU 布线**（`gpu_router`，可选） | wgpu compute shader 波前扩展（`--features gpu`，默认开启；`cuda` 档另计） |
| **布局引擎**（`layout_engine`） | SA Sequence Pair 全局布局 + 电源流向链检测 + 连接器外设分组 |
| **DRC/ERC**（`drc`/`erc`） | 间距/宽度/孔径等全套设计规则检查 + 电气规则检查 |
| **规则引擎**（`rules`） | 数学表达式求值与参数约束检查 |
| **设计技能**（`skills`） | 93 条可执行设计规则 + 元件推荐 + 管线编排 |
| **BOM/电源树**（`bom`/`power_tree`） | 物料清单生成与电源拓扑推导 |
| **组合设计**（`composition`/`design`） | 子电路组合成板、IC 板级原理图生成 |
| **HQ API**（`hqapi`） | 华秋元件库在线拉取（pin/参数/数据手册） |
| **符号/封装生成**（`symgen`/`footprint`） | KiCad 符号库导出、封装元数据提取 |

## 数据库生命周期

**Schema 即模板**：库代码内建 migrations，首次打开任意路径即自动建全表——
仓库不携带任何 `.db` 文件，每个项目用自己的数据库实例：

```rust
use kicad_cdb::db::Db;

// 路径来自 CDB_PATH 环境变量（工具链约定），或任意你的路径
let db = Db::open("myproject.db")?;   // 首次打开自动建表
```

数据填充走 HQ API 在线拉取、CSV/JSON 导入（见 `hqapi`/`csv_import` 模块），
或 `:memory:` 直接起内存库做测试。

内置模板资产（cwd 相对加载，或 `import-templates` 入库）：
- `ic-templates/` — 27 个 IC 核心模板（CH340/LM358/INA219 等）
- `templates/` — 9 个电源拓扑模板（boost/buck/SEPIC…）

## 库用法示例

```rust
use kicad_cdb::db::Db;

let db = Db::open_in_memory()?;

db.insert_category(&kicad_cdb::models::Category {
    name: "passive/capacitor".into(),
    ..Default::default()
})?;

// 参数范围查询：耐压 ≥ 25V 的电容
let hits = db.query_components("category:passive/capacitor voltage>=25", 10)?;
```

## 工具链

| crate | 说明 |
| --- | --- |
| [kicad-json5](https://github.com/memoriaru/kicad/tree/main/kicad-json5) | .kicad_sch/.kicad_pcb ↔ JSON5 双向编译 |
| [kicad-symgen](https://github.com/memoriaru/kicad/tree/main/kicad-symgen) | 符号/封装参数化生成 |
| [kicad-render](https://github.com/memoriaru/kicad/tree/main/kicad-render) | 原理图/PCB SVG 渲染（KiCanvas 移植） |
| [kicad-cdb](https://github.com/memoriaru/kicad/tree/main/kicad-cdb) | 本 crate：元件库 + 布局/布线/DRC 引擎 |

## License

MIT
