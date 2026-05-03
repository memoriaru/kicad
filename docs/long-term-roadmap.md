# AI 辅助电路设计 — 远期路线图

对照 `ai-circuit-design-vision.md` 核心理念，盘点各模块实现状态与待完成项。

## 现状总览

| 模块 | 状态 | 完成度 |
|------|------|--------|
| kicad-json5 (原理图编译) | 可用 | 85% |
| kicad-render (SVG 渲染) | 可用 | 75% |
| kicad-cdb (元件数据库) | 华秋 API 集成完成 | 60% |
| kicad-symgen (符号/封装生成) | 框架就绪 | 35% |
| 设计规则 Skill 系统 | 原型 | 15% |
| 拓扑选型与参数推导 | 基础分析 | 20% |

---

## Layer 1: 元件数据 (Component Data)

> 目标：SQLite 作为 Single Source of Truth，AI 可查询参数、封装、符号、供应商。

### 已完成

- [x] SQLite schema 设计（8 表：categories, components, pins, parameters, simulation_models, design_rules, supply_info, reference_circuits）
- [x] ComponentDb 基础 CRUD（创建/查询/导入）
- [x] 参数范围查询（`capacitance>=1e-7` 语法）
- [x] 全文搜索（描述/元数据）
- [x] 元件分类树（层级 category）
- [x] 供应商信息存储（SKU/价格/库存/交期）

### 已完成（续）

- [x] **华秋 EDA API 集成**：搜索/详情/供应链三个端点，关键词搜索 + MPN 按需拉取
- [x] **CLI fetch/hqsearch 命令**：`cdb fetch --mpn ...` 一键拉取参数+pin+符号到 SQLite
- [x] **.kicad_sym 解析**：从华秋下载的符号文件中自动提取 pin number/name/type
- [x] **参数结构化解析**：华秋 API `attrShortName` → 标准化参数名，数值+单位分离

### 未完成

- [ ] **数据批量导入**：CSV/Excel 批量导入元件数据
- [ ] **Datasheet 解析**：从 PDF datasheet 自动提取参数填入 parameters 表
- [ ] **Pin 功能映射**：MCU 等复杂 IC 的引脚复用功能结构化存储（alt_functions 当前为纯文本）
- [ ] **KiCad 符号关联**：components.kicad_symbol / kicad_footprint 字段关联到实际 .kicad_sym 文件
- [ ] **仿真模型管理**：simulation_models 表结构已有，但无导入/验证流程
- [ ] **元件生命周期追踪**：lifecycle 字段存在但无自动更新机制
- [ ] **参数标准化**：统一单位、命名规范（如 `capacitance` vs `Cap` vs `C`）
- [ ] **数据库版本迁移**：schema 变更时自动迁移机制

---

## Layer 2: 设计知识 (Design Knowledge)

> 目标：可执行的 skill 集（电源/信号完整性/EMC），AI 能决策选型。

### 已完成

- [x] 规则引擎核心（EvalContext：四则运算、变量替换、比较运算）
- [x] 公式求值（`l_min = (vout * (1 - vout / vin)) / (fsw * 0.3 * iout)`）
- [x] 约束检查（`L_value >= l_min * 0.8`）
- [x] ComponentDb.apply_rule() 接口

### 未完成

- [ ] **内置 Skill 库**：按功能域组织的设计规则集合
  - [ ] 电源域：Buck/Boost/LDO 选型与外围计算
  - [ ] 信号完整性：阻抗匹配、走线长度约束
  - [ ] EMC：去耦电容数量/位置规则、滤波器截止频率
  - [ ] 热设计：功耗、热阻、温升计算
  - [ ] 时序：建立/保持时间裕量
- [ ] **Skill 输入/输出契约**：每个 Skill 声明需要哪些输入参数、输出哪些计算结果
- [ ] **Skill 链式调用**：一个 Skill 的输出自动成为下一个 Skill 的输入（如 Buck 电感选型 → 电感饱和电流检查）
- [ ] **条件分支**：根据输入条件选择不同设计路径（如 `if iout > 2A then use Buck else use LDO`）
- [ ] **多方案比较**：同一需求生成多个候选方案并排序
- [ ] **设计决策追溯**：记录每步计算的输入参数、公式、结果和约束检查

---

## Layer 3: 设计意图 → 出图 (Design Intent → Output)

> 目标：AI 从需求推导电路拓扑，自动生成完整 KiCad 工程。

### 已完成

- [x] JSON5 → .kicad_sch 双向编译
- [x] 自动 wire 布线（Manhattan 路由）
- [x] 自动 label 生成（global_label/hierarchical_label）
- [x] PWR_FLAG 自动插入（flat 原理图）
- [x] No-Connect 标记生成
- [x] 层次原理图（sheet）JSON5 定义 → .kicad_sch
- [x] SVG 原理图渲染
- [x] 标准 Device 库嵌入（R/C/L/D/LED/NTC）
- [x] 拓扑提取（电源域、信号路径、功能模块识别）

### 未完成

#### kicad-json5 编译器

- [ ] **PCB 输出**：.kicad_pcb 格式生成（布局、走线、铜皮）
- [ ] **符号库输出**：.kicad_sym 格式完整生成（当前仅嵌入预定义符号）
- [ ] **封装库输出**：.kicad_mod 格式生成
- [ ] **ERC 集成**：生成后自动调用 KiCad ERC 并解析结果
- [ ] **DRC 集成**：PCB 生成后自动调用 KiCad DRC
- [ ] **网表生成**：Netlist 格式输出供 PCB 工具使用
- [ ] **BOM 生成**：从 components 自动输出 BOM（CSV/TSV）
- [ ] **3D 模型关联**：元器件 3D 模型路径自动匹配

#### kicad-symgen 符号/封装生成

- [ ] **从 cdb 自动生成**：查询 kicad-cdb → 自动生成 .kicad_sym + .kicad_mod
- [ ] **更多封装模板**：当前有 DIP/SOIC/SOT，缺少 QFP/QFN/BGA/WLCSP/DFN 等
- [ ] **符号图形丰富化**：当前符号为简单矩形+pin，缺少 op-amp 三角形、MCU 功能分区等
- [ ] **智能引脚布局**：按功能分组（电源/地/IO/通信/时钟）
- [ ] **批量库生成**：从元件列表批量生成完整 .kicad_sym 库文件

#### kicad-render SVG 渲染

- [ ] **层次原理图渲染**：sheet symbol 内部子图的递归渲染
- [ ] **交互式 SVG**：点击 sheet symbol 跳转到子图
- [ ] **Wire 路由优化**：当前 Manhattan 路由可能产生重叠/交叉
- [ ] **ERC 错误标记可视化**：在 SVG 上标注 ERC 错误位置
- [ ] **PDF 导出**：高质量 PDF 输出

---

## 跨层集成（核心断裂点）

### 断裂点 1：元件不可见 → 元件库

- [x] **在线元件搜索**：`cdb hqsearch "74hc04"` → 华秋 API → 返回候选列表
- [x] **按需拉取**：`cdb fetch --mpn MCP6444T-E/ST --mfg-id 4901` → 参数+pin+符号一键入库
- [ ] **AI 查询接口**：让 Claude/LLM 通过自然语言查询元件库
  - 示例：`"找一个 100mA LDO，输入 5V 输出 3.3V"` → SQL 查询 → 返回候选列表
- [ ] **元件推荐**：根据设计规则自动推荐满足约束的元件
- [ ] **参数对比**：多个候选元件的参数并排对比

### 断裂点 2：设计规则不成体系 → Skill 集

- [ ] **Skill 注册机制**：可插拔的规则模块，声明适用场景
- [ ] **自然语言 → Skill 映射**：AI 从需求描述选择合适的 Skill
- [ ] **Skill 组合编排**：多个 Skill 协作完成复杂设计（电源树分析 = LDO Skill + 去耦 Skill + 热设计 Skill）

### 断裂点 3：知识到图纸的鸿沟 → 编译管线

- [ ] **端到端管线**：需求 → 拓扑选型 → 参数计算 → 元件选型 → JSON5 → .kicad_sch
- [ ] **增量更新**：修改需求后只更新受影响的部分，不重新生成全部
- [ ] **设计空间探索**：自动生成多个设计方案供比较

### 断裂点 4：拓扑选型无形式化 → 拓扑库

- [ ] **拓扑模板库**：常见电路拓扑的结构化描述
  - Buck converter（输入范围、输出电压、电流能力、效率）
  - LDO（压差、PSRR、噪声）
  - I2C 上拉（总线电容、速率、电阻计算）
  - LED 驱动（正向电压、电流、限流电阻）
- [ ] **拓扑匹配**：从需求（`5V→3.3V, 500mA`）自动选择合适拓扑
- [ ] **拓扑参数化**：给定拓扑模板和需求参数，自动填充外围元件值

### 断裂点 5：参数计算不可追溯 → 决策记录

- [ ] **设计日志格式**：记录每步决策的结构化格式
  ```json5
  {
    step: "buck_inductor_selection",
    rule: "inductor_sizing",
    inputs: { vin: 12, vout: 5, iout: 2, fsw: 500e3, delta_il: 0.3 },
    formula: "L = (Vout × (1 - Vout/Vin)) / (fsw × ΔIL × Iout)",
    result: { l_min: 5.83e-6 },
    check: "L_value >= l_min * 0.8",
    selected: "SRN6045-6R8Y",
    passed: true
  }
  ```
- [ ] **回溯机制**：从最终设计反向查看每个参数的计算依据
- [ ] **变更影响分析**：修改某个参数后，标记所有受影响的下游决策

---

## 优先级排序（建议）

| 优先级 | 任务 | 理由 |
|--------|------|------|
| **P0** | ~~元件数据导入工具~~ → ✅ 华秋 API 集成完成 | 地基已打通：搜索+拉取+pin 提取 |
| **P0** | AI 查询接口（自然语言 → SQL） | 让 AI 真正"看见"元件 |
| **P1** | 内置 Skill 库（电源域 5 条核心规则） | 最常见的设计场景，验证规则引擎可用性 |
| **P1** | 拓扑模板库（Buck/LDO/LED 3 个模板） | 验证"需求 → 拓扑 → 参数"链路 |
| **P1** | 端到端演示：`5V→3.3V LDO` 全流程 | 从需求到出图的最小闭环 |
| **P1** | 常用元件库填充（20-50 颗核心 IC） | 用 fetch 批量拉取，让数据库真正可用 |
| **P2** | kicad-symgen 与 cdb 集成 | 消除手动创建符号的瓶颈 |
| **P2** | 封装模板补全（QFP/QFN/BGA） | 覆盖更多常见封装 |
| **P2** | 设计决策追溯系统 | 提升设计可审计性 |
| **P3** | PCB 输出 (.kicad_pcb) | 最大的单项工程，依赖 PCB 格式设计笔记 |
| **P3** | ERC/DRC 集成 | 需要 KiCad CLI 或 headless 运行 |
| **P3** | 交互式 SVG / PDF 导出 | 用户体验提升，非核心功能 |

---

## 技术债务

- [x] ~~kicad-json5：`snap_to_grid` 和 `get_standard_pin_positions` dead code~~ — 已删除
- [x] ~~kicad-render：`_h_align` / `_v_align` 未使用参数~~ — 已删除参数并更新调用点
- [x] ~~kicad-render：painter 层裸 `unwrap()`~~ — 已替换为 `expect("...")`
- [x] ~~kicad-render：pin_painter 重复 Y-flip 逻辑~~ — 已提取 `correct_vertical_anchor()` 辅助函数
- [x] ~~kicad-cdb：`unwrap()` 裸用~~ — 已替换为 `expect()`/`context()`
- [x] ~~kicad-symgen：magic number~~ — 已提取为模块级常量
- [x] ~~所有 crate：裸 `unwrap()` 替换为 `expect()`/`context()`~~ — 生产代码已清理
- [ ] kicad-cdb：hqapi 集成测试（需要网络，当前仅有单元测试）
- [ ] kicad-cdb：测试覆盖仍不够完善（规则引擎和查询 API）
- [ ] kicad-symgen：未与 workspace 其他 crate 建立依赖关系
- [ ] 所有 crate：缺少 CI/CD 配置
