# 拓扑提取器开发任务

> 为 AI 提供原理图的语义化理解能力

## 目标

从 KiCad 原理图 IR 中提取电路拓扑结构，生成 AI 友好的语义化摘要。

## 核心概念

### 什么是电路拓扑？

```
拓扑 = 元件之间的连接关系（不关心位置，只关心"谁连着谁"）

原理图文件：          拓扑提取：
─────────────────────────────────────────────────
R1 @ (100, 50)        VCC ── R1 ── LED1 ── GND
LED1 @ (150, 50)
Wire (120,50)-(140,50)
...                   ↓
                      {
                        path: ["VCC", "R1", "LED1", "GND"],
                        type: "series",
                        purpose: "LED指示电路"
                      }
```

### 对 AI 的意义

- AI 不需要理解坐标、旋转角度
- AI 理解"信号从哪来，到哪去"
- AI 可以识别"这是一个电压调节器"
- Token 高效，信息密度高

---

## 任务清单

### Phase 1: 基础数据结构 (P0) ✅ 已完成

#### Task 1.1: 定义拓扑核心类型
- [x] 创建 `src/topology/mod.rs`
- [x] 创建 `src/topology/types.rs`
- [x] 定义 `TopologyNode` 枚举
- [x] 定义 `TopologyEdge` 结构
- [x] 定义 `CircuitTopology` 主结构

```rust
// 实现的结构
pub enum TopologyNode {
    Component { reference: String, kind: ComponentKind, lib_id: String, value: Option<String> },
    Net { name: String, kind: NetKind },
    Pin { component: String, pin: String },
}

pub enum ComponentKind {
    Resistor, Capacitor, Inductor, Diode, Transistor,
    Ic, Connector, Power, Crystal, Switch, Fuse, Unknown,
}

pub enum NetKind {
    Power, Ground, Signal, Bus,
}
```

#### Task 1.2: 定义拓扑摘要结构
- [x] 创建 `src/topology/summary.rs`
- [x] 定义 `TopologySummary` (给 AI 的精简信息)
- [x] 定义 `PowerDomain`
- [x] 定义 `SignalPath`
- [x] 定义 `FunctionalModule`
- [x] 实现 `to_json5()` 方法
- [x] 实现 `to_text_summary()` 方法

---

### Phase 2: 拓扑提取器 (P0) ✅ 已完成

#### Task 2.1: 构建连接图
- [x] 创建 `src/topology/extractor.rs`
- [x] 实现 `TopologyExtractor::new(schematic: &Schematic)`
- [x] 实现 `build_connection_graph()` - 从 IR 构建连接关系
- [x] 实现 `get_adjacency_list()` - 获取邻接表表示

#### Task 2.2: 电源网络识别
- [x] 实现 `identify_power_nets()` - 识别电源/地网络
- [x] 定义电源网络命名规则 (VCC, 3V3, 5V, +12V, VIN, VOUT...)
- [x] 定义地网络命名规则 (GND, AGND, DGND, GNDA, GNDD, VSS...)
- [x] 实现 `extract_power_domains()` - 提取电源域
- [x] 实现 `extract_voltage()` - 从网络名提取电压值

#### Task 2.3: 信号路径提取
- [x] 实现 `extract_signal_paths()` - 提取主要信号路径
- [x] 识别输入/输出标签
- [x] 追踪从输入到输出的路径

#### Task 2.4: 元件分类
- [x] 创建 `src/topology/classify.rs`
- [x] 实现 `classify_component(lib_id: &str) -> ComponentKind`
- [x] 实现 `classify_net(net_name: &str) -> NetKind`
- [x] 基于 lib_id 前缀和关键词分类

---

### Phase 3: 功能模块识别 (P1) ✅ 已完成

#### Task 3.1: 定义模块模式
- [x] 创建 `src/topology/patterns.rs`
- [x] 定义 `ModulePattern` 结构
- [x] 定义 `ConnectionPattern` 枚举

#### Task 3.2: 模块识别器
- [x] 实现 `identify_modules()` - 识别功能模块
- [x] 支持的模式：
  - [x] I2C 上拉电阻
  - [x] LED 指示灯
  - [x] 去耦电容
  - [x] 电压调节器 (检测 Power 类型元件)
  - [ ] 晶振电路 (待增强)
  - [ ] 复位电路 (待增强)

---

### Phase 4: JSON5 输出 (P1) ✅ 已完成

#### Task 4.1: 拓扑序列化
- [x] 为拓扑类型实现 JSON5 输出
- [x] 实现 `TopologySummary::to_json5()` - 生成 AI 友好的输出

#### Task 4.2: CLI 集成
- [x] 添加 `--topology` / `-t` 命令行选项
- [x] 添加 `--format topology` 选项
- [x] 输出拓扑摘要而非原始 JSON5

---

### Phase 5: 测试与验证 (P0) ✅ 已完成

#### Task 5.1: 单元测试
- [x] 测试电源网络识别
- [x] 测试地网络识别
- [x] 测试元件分类 (电阻、电容、IC、连接器、电源)
- [x] 测试电压提取
- [x] 测试模式匹配
- [x] 测试空原理图处理
- [x] 测试 Wire 连接推断 (Point, UnionFind)

#### Task 5.2: 集成测试
- [x] 使用 WCH-LinkE-R0-1v3 原理图测试
- [x] 验证拓扑摘要的准确性
- [x] 验证电源域提取 (+3V3)
- [x] 验证地网络识别 (GND)
- [x] 验证信号路径提取 (40+ 信号)
- [x] 验证功能模块识别 (LED 指示灯)

---

## 文件结构

```
src/topology/
├── mod.rs           # 模块入口 ✅
├── types.rs         # 核心类型定义 ✅
├── summary.rs       # 摘要结构 ✅
├── extractor.rs     # 拓扑提取器 ✅
├── patterns.rs      # 模块模式识别 ✅
└── classify.rs      # 元件分类 ✅
```

---

## 使用方法

```bash
# 提取拓扑摘要
kicad-json5 schematic.kicad_sch --topology

# 或使用 --format
kicad-json5 schematic.kicad_sch --format topology

# 输出到文件
kicad-json5 schematic.kicad_sch -t -o topology.json5
```

---

## 验收标准

### MVP (Phase 1-2) ✅ 已实现

```bash
# 输入
kicad-json5 schematic.kicad_sch --topology

# 输出
{
  // 电源域
  power_domains: [
    { name: "3.3V", voltage: "3.3", consumers: ["U1", "R1", "C1"], sources: [] }
  ],
  ground_nets: ["GND"],

  // 信号路径
  signal_paths: [
    { name: "SDA", direction: "bidirectional", from: "U1.21", to: "J1.3" }
  ],

  // 功能模块
  modules: [
    { type: "power_decoupling", purpose: "电源去耦 (2 个电容)", components: ["C1", "C2"], target: "U1" }
  ],

  // 元件统计
  component_summary: {
    total: 15,
    by_type: { "ic": 2, "resistor": 5, "capacitor": 8 }
  },

  // 网络连接
  net_components: {
    "3.3V": ["U1", "R1", "C1"],
    "GND": ["U1", "C1", "C2"]
  }
}
```

---

## 参考资料

- KiCad 原理图格式：已有 IR 实现
- 电路拓扑分析：基于图论的电路分析方法
- 电源网络识别：基于命名规则 + 网络类型
