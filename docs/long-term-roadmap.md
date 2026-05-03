# AI 辅助电路设计 — 远期路线图

对照 `ai-circuit-design-vision.md` 核心理念，盘点各模块实现状态与待完成项。

## 现状总览

| 模块 | 状态 | 完成度 |
|------|------|--------|
| kicad-json5 (原理图编译) | 可用 | 85% |
| kicad-render (SVG 渲染) | 可用 | 75% |
| kicad-cdb (元件数据库) | CLI --json + MCP Server + 代码重构 | 90% |
| kicad-symgen (符号/封装生成) | 独立 CLI，智能布局 + 封装模板 | 45% |
| 设计规则 Skill 系统 | Pipeline 链式调用 + 决策追溯 | 75% |
| 拓扑选型与参数推导 | 9 拓扑 + 选型引擎 | 55% |

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
- [x] **多赋值公式支持**：分号分隔的多步计算（`duty = 1 - vin/vout; c_out_min = iout * duty / (fsw * ripple_v)`）
- [x] **condition_expr 门控**：规则前置条件求值，不满足时跳过
- [x] **内置 Skill 库（46 条规则）**：
  - 电源域：Buck (5) / Boost (6) / Buck-Boost (3) / Inverting (4) / SEPIC (5) / Charge Pump (3) / Flyback (7) / LDO (4)
  - 共享辅助：电容纹波/电压裕量、电感饱和/降额、热功耗/结温、效率检查 (6)
  - LED：限流电阻 (1)
  - 热设计：功耗计算、结温估算 (2)
- [x] **CLI rules 命令**：`cdb rules --seed` / `cdb rules` / `cdb rules --apply <rule> --params ... --candidate ...`
- [x] **Pipeline 链式调用**：4 条内置 pipeline（buck/boost/ldo/led），上游输出自动流入下游规则
- [x] **设计决策追溯**：DesignLog 结构化记录每步的 inputs/formula/outputs/check/passed/skipped
- [x] **Pipeline CLI**：`cdb pipeline buck --params "vin=12,vout=3.3,iout=2,fsw=500000,ripple_ratio=0.3,ripple_v=0.05" [--json]`

### 未完成

- [ ] **Skill 输入/输出契约**：每个 Skill 声明需要哪些输入参数、输出哪些计算结果（当前 parameters/output_params 字段已有但未被 CLI 严格校验）
- [ ] **条件分支**：根据输入条件选择不同设计路径（如 `if iout > 2A then use Buck else use LDO`）
- [ ] **多方案比较**：同一需求生成多个候选方案并排序
- [ ] **信号完整性 Skill**：阻抗匹配、走线长度约束
- [ ] **EMC Skill**：去耦电容数量/位置规则、滤波器截止频率
- [ ] **时序 Skill**：建立/保持时间裕量

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
- [x] **拓扑模板系统**：JSON 格式的电路拓扑描述（components + connections + layout）
- [x] **9 个内置拓扑模板**：LDO / Buck / Boost / Buck-Boost / Inverting / SEPIC / Charge Pump / Flyback / LED
- [x] **端到端管线**：`cdb design --template <name> --vin --vout --iout -o output.kicad_sch`
- [x] **拓扑选型引擎**：`cdb suggest --vin --vout --iout [--isolated]` 基于 Vin/Vout/Iout/隔离需求推荐拓扑
- [x] **lib_symbols 自动生成**：为拓扑中的 Device:R/C/L/D 和 custom:IC 自动创建符号定义

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

- [x] **cdb → symgen 数据桥接**：`cdb export --format spec --mpn X -o X.json5` → `symgen symbol --input X.json5 -o X.kicad_sym`
- [x] **独立 crate 架构**：kicad-symgen 与 kicad-cdb 零交叉依赖，通过 JSON5 spec 文件协作
- [x] **智能引脚布局**：按电气类型自动分类（电源→顶部，地→底部，输入→左侧，输出→右侧）
- [x] **封装模板**：DIP/SOIC/TSSOP/SOP/MSOP/SOT-23/SOT-223
- [x] **更多封装模板**：QFP/QFN/BGA/DFN 等已添加
- [ ] **符号图形丰富化**：op-amp 三角形、MCU 功能分区等特殊图形
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
- [x] **拓扑选型**：`cdb suggest --vin 12 --vout 3.3 --iout 2` → 推荐 Buck/Boost/LDO/SEPIC 等
- [x] **AI 查询接口**：CLI `--json` 模式 + MCP Server (`cdb serve`)，9 个 tools，Claude 直接查询元件库
- [ ] **元件推荐**：根据设计规则自动推荐满足约束的元件
- [ ] **参数对比**：多个候选元件的参数并排对比

### 断裂点 2：设计规则不成体系 → Skill 集

- [x] **46 条电源域规则**：覆盖 Buck/Boost/Buck-Boost/Inverting/SEPIC/Charge Pump/Flyback/LDO 全拓扑
- [x] **规则引擎增强**：多赋值公式、条件门控
- [x] **Pipeline 链式调用**：buck/boost/ldo/led 四条 pipeline，上游输出自动流入下游
- [x] **设计决策追溯**：DesignLog JSON 格式记录每步计算
- [ ] **Skill 注册机制**：可插拔的规则模块，声明适用场景
- [ ] **自然语言 → Skill 映射**：AI 从需求描述选择合适的 Skill
- [ ] **Skill 组合编排**：多个 Skill 协作完成复杂设计（电源树分析 = LDO Skill + 去耦 Skill + 热设计 Skill）

### 断裂点 3：知识到图纸的鸿沟 → 编译管线

- [x] **端到端管线**：需求 → 拓扑选型 → 参数计算 → 元件选型 → JSON5 → .kicad_sch
- [ ] **增量更新**：修改需求后只更新受影响的部分，不重新生成全部
- [ ] **设计空间探索**：自动生成多个设计方案供比较

### 断裂点 4：拓扑选型无形式化 → 拓扑库

- [x] **拓扑模板库（9 个）**：
  - Buck converter（电感/电容选型、占空比检查、续流二极管）
  - Boost converter（电感/电容选型、开关电压应力）
  - Buck-Boost（升降压，非反相）
  - Inverting（反相输出）
  - SEPIC（升降压非反相，耦合电容）
  - Charge Pump（电荷泵，无电感）
  - Flyback（反激隔离，变压器匝比、RCD 吸收）
  - LDO（线性稳压，压差/功耗/效率）
  - LED（限流电阻）
- [x] **拓扑匹配**：`cdb suggest` 从需求自动推荐合适拓扑并给出效率/评分
- [x] **拓扑参数化**：给定拓扑模板和需求参数，自动生成含正确连线的 .kicad_sch

### 断裂点 5：参数计算不可追溯 → 决策记录

- [x] **设计日志格式**：DesignLog 结构化 JSON（pipeline_name, user_inputs, steps[], passed/failed/skipped）
- [x] **Pipeline CLI**：`cdb pipeline <name> --params "..." --json` 输出完整决策链
- [ ] **回溯机制**：从最终设计反向查看每个参数的计算依据
- [ ] **变更影响分析**：修改某个参数后，标记所有受影响的下游决策

---

## 优先级排序（建议）

| 优先级 | 任务 | 状态 | 理由 |
|--------|------|------|------|
| **P0** | ~~元件数据导入工具~~ → ✅ 华秋 API 集成完成 | ✅ 已完成 | 搜索+拉取+pin 提取+10颗核心IC |
| **P0** | ~~常用元件库填充~~ | ✅ 已完成 | 10颗核心IC: opamp/buck/LDO/MCU/CAN/ESD |
| **P1** | ~~内置 Skill 库（5 条核心规则）~~ | ✅ 已完成 | buck/ldo/cap/led 规则 + Rules CLI |
| **P1** | ~~拓扑模板库（3 个模板）~~ | ✅ 已完成 | ldo/buck/led 模板 + design.rs 编排器 |
| **P1** | ~~端到端演示：5V→3.3V LDO~~ | ✅ 已完成 | `cdb design --template ldo` 全流程 |
| **P2** | ~~电源域完整 Skill 框架~~ | ✅ 已完成 | 46 条规则覆盖 8 种 DC-DC 拓扑 |
| **P2** | ~~6 个新拓扑模板~~ | ✅ 已完成 | boost/buckboost/inverting/sepic/chargepump/flyback |
| **P2** | ~~拓扑选型引擎~~ | ✅ 已完成 | `cdb suggest --vin --vout --iout` |
| **P3** | ~~Track C: IC 核心模板 + 模块化组合~~ | ✅ 已完成 | 8 个 IC 模板 + 华秋 API pin 获取 + CCD 全板组合验证 |
| **P3** | ~~Skill 链式调用 + 设计决策追溯~~ | ✅ 已完成 | 4 条 pipeline + DesignLog JSON 输出 |
| **P3** | ~~kicad-symgen 与 cdb 集成~~ | ✅ 已完成 | 三 crate 独立架构 + JSON5 spec 桥接 |
| **P3** | ~~AI 查询接口（自然语言 → SQL）~~ | ✅ 已完成 | CLI --json + MCP Server (9 tools) + service.rs 重构 |
| **P4** | ~~封装模板补全（QFP/QFN/BGA）~~ | ✅ 已完成 | kicad-symgen QFP/QFN/DFN/BGA 模板 |
| **P4** | PCB 输出 (.kicad_pcb) | 未开始 | 最大的单项工程 |
| **P4** | ERC/DRC 集成 | 未开始 | 需要 KiCad CLI 或 headless 运行 |
| **P5** | 交互式 SVG / PDF 导出 | 未开始 | 用户体验提升，非核心功能 |

---

## 技术债务

- [x] ~~kicad-json5：`snap_to_grid` 和 `get_standard_pin_positions` dead code~~ — 已删除
- [x] ~~kicad-render：`_h_align` / `_v_align` 未使用参数~~ — 已删除参数并更新调用点
- [x] ~~kicad-render：painter 层裸 `unwrap()`~~ — 已替换为 `expect("...")`
- [x] ~~kicad-render：pin_painter 重复 Y-flip 逻辑~~ — 已提取 `correct_vertical_anchor()` 辅助函数
- [x] ~~kicad-cdb：`unwrap()` 裸用~~ — 已替换为 `expect()`/`context()`
- [x] ~~kicad-symgen：magic number~~ — 已提取为模块级常量
- [x] ~~所有 crate：裸 `unwrap()` 替换为 `expect()`/`context()`~~ — 生产代码已清理
- [x] ~~kicad-cdb：hqapi 集成测试（需要网络，当前仅有单元测试）~~ — 已完成（下载测试 + 测试 DB 切换为正式 DB）
- [x] ~~kicad-cdb：query.rs 未使用变量警告~~ — 已加 `_` 前缀
- [x] ~~kicad-symgen：独立 CLI，不依赖 workspace 其他 crate~~ — 已确认零 workspace 依赖
- [x] ~~kicad-cdb：hqapi::ApiError 死代码~~ — 已删除
- [x] ~~kicad-cdb：pipeline.rs Context 未使用导入~~ — 已删除
- [x] ~~cdb.rs 代码重复~~ — 提取 service.rs，消除 6x 参数解析、4x SQL、2x 查询过滤链重复
- [ ] 所有 crate：缺少 CI/CD 配置

---

## 设计规则完整清单（46 条）

### 共享辅助 (6)

| 规则 | 说明 |
|------|------|
| `cap_voltage_derating` | 电容电压降额 |
| `cap_ripple_current` | 电容纹波电流 (三角波近似) |
| `inductor_saturation_check` | 电感饱和电流检查 |
| `inductor_derating` | 电感饱和电流降额 (20% 裕量) |
| `thermal_dissipation` | 线性器件功耗计算 |
| `thermal_ja_rise` | 结温计算 (Tj = Ta + P*Rja) |
| `efficiency_linear` | 线性稳压器效率 |

### Buck (5)

| 规则 | 说明 |
|------|------|
| `buck_inductor_selection` | 最小电感量 |
| `buck_input_capacitor` | 输入电容 |
| `buck_output_capacitor` | 输出电容 |
| `buck_duty_cycle` | 占空比检查 (≤90%) |
| `buck_inductor_ripple` | 电感纹波电流验证 |
| `buck_catch_diode` | 续流二极管反向电压 |

### LDO (4)

| 规则 | 说明 |
|------|------|
| `ldo_dropout_check` | 压差检查 |
| `ldo_power_dissipation` | 功耗计算 |
| `ldo_efficiency` | 效率估算 |
| `ldo_output_cap` | 输出电容估算 |

### Boost (6)

| 规则 | 说明 |
|------|------|
| `boost_duty_cycle` | 占空比 (D=1-Vin/Vout) |
| `boost_inductor_selection` | 最小电感量 |
| `boost_inductor_ripple` | 电感纹波验证 |
| `boost_output_capacitor` | 输出电容 |
| `boost_switch_voltage` | 开关管电压应力 |
| `boost_diode_voltage` | 输出二极管反向电压 |

### Buck-Boost (3)

| 规则 | 说明 |
|------|------|
| `buckboost_duty_cycle` | 占空比 (D=Vout/(Vin+Vout)) |
| `buckboost_inductor_selection` | 最小电感量 |
| `buckboost_output_capacitor` | 输出电容 |

### Inverting (4)

| 规则 | 说明 |
|------|------|
| `inverting_duty_cycle` | 占空比 |
| `inverting_inductor_selection` | 最小电感量 |
| `inverting_output_capacitor` | 输出电容 |
| `inverting_diode_voltage` | 二极管反向电压 (Vin+\|Vout\|) |

### SEPIC (5)

| 规则 | 说明 |
|------|------|
| `sepic_duty_cycle` | 占空比 |
| `sepic_inductor_selection` | 最小电感量 |
| `sepic_coupling_cap` | 耦合电容 |
| `sepic_coupling_cap_voltage` | 耦合电容耐压 |
| `sepic_output_capacitor` | 输出电容 |

### Charge Pump (3)

| 规则 | 说明 |
|------|------|
| `chargepump_flying_cap` | 飞跨电容 |
| `chargepump_flying_cap_voltage` | 飞跨电容耐压 |
| `chargepump_output_cap` | 输出电容 |

### Flyback (7)

| 规则 | 说明 |
|------|------|
| `flyback_duty_cycle` | 占空比 (≤75%) |
| `flyback_transformer_turns` | 最小匝比 |
| `flyback_primary_inductance` | 初级最小电感 (DCM 边界) |
| `flyback_primary_peak_current` | 初级峰值电流 + 饱和检查 |
| `flyback_snubber_rcd_cap` | RCD 吸收电容 |
| `flyback_snubber_resistor` | RCD 吸收电阻 |
| `flyback_output_capacitor` | 输出电容 |

### LED (1)

| 规则 | 说明 |
|------|------|
| `led_current_resistor` | 限流电阻计算 |

---

## 拓扑模板完整清单（9 个）

| 模板 | 元件数 | 网络数 | 关键 Skill |
|------|--------|--------|-----------|
| `ldo` | 3 (regulator, c_in, c_out) | 3 (VIN, GND, VOUT) | ldo_dropout, cap_derating |
| `buck` | 5 (controller, l_out, c_in, c_out, d_catch) | 4 (VIN, GND, SW, VOUT) | buck_inductor, buck_input_cap |
| `boost` | 5 (controller, l_in, c_in, c_out, d_out) | 4 (VIN, GND, SW, VOUT) | boost_inductor, boost_output_cap |
| `buckboost` | 4 (controller, l_main, c_in, c_out) | 5 (VIN, GND, SW1, SW2, VOUT) | buckboost_inductor |
| `inverting` | 5 (controller, l_main, c_in, c_out, d_catch) | 5 (VIN, GND, SW, -VOUT, COM) | inverting_inductor |
| `sepic` | 7 (controller, l1, l2, c_coupling, c_in, c_out, d_out) | 5 (VIN, GND, SW, CS, VOUT) | sepic_inductor, sepic_coupling_cap |
| `chargepump` | 4 (controller, c_fly, c_in, c_out) | 5 (VIN, GND, C+, C-, VOUT) | chargepump_flying_cap |
| `flyback` | 7 (controller, t1, c_in, c_out, d_out, r_snub, c_snub) | 8 (VIN, GND, SW, ...) | flyback_transformer, flyback_snubber |
| `led` | 2 (r_limit, led) | 3 (VIN, GND, N_LED) | led_current_resistor |

---

## P3: Track C — 模块化组合设计

### 设计理念

Track A（自动管线）适合公式驱动的电源拓扑，Track B（对话驱动）适合复杂系统设计。Track C 在两者之间：

- **Skill 驱动模块**：电源域、无源元件选型 → 公式计算，全自动
- **模板驱动模块**：IC 核心电路 → 从 datasheet 提取固定连接模式，半自动
- **模块组合**：通过 KiCad 原生 label 层级定义接口（local label = 模块私有，global label = 全局共享）

### 模块分类

| 模块类型 | 驱动方式 | 知识来源 | 当前状态 |
|---------|---------|---------|---------|
| 电源域 | Skill | 物理公式 | ✅ P2 完成 |
| IC 核心电路 | 模板 | Datasheet 典型应用 | 🔄 P3 进行中 |
| 接口转换 | 模板 | Datasheet 参考设计 | 待开始 |
| 信号链 | 混合 | Datasheet + 公式 | 待开始 |
| 保护/EMI | Skill | 经验公式 | 待开始 |

### IC 核心模板

**目标：** 为常见 IC 定义"典型应用电路模板"，包含：
- IC 本体（pin 映射从 cdb/华秋 API 获取）
- 必需外围元件（去耦电容、上拉/下拉、反馈网络等）
- 连接关系（哪些 pin 接哪些外围元件）
- 可调参数（容值、阻值范围）
- 接口声明（global label = 对外网络）

**已验证模板（8 个）：**

| 模板 | 类型 | 元件数 | 验证实例 | 来源 |
|------|------|--------|---------|------|
| RT9193-ADJ | LDO 稳压器 | 5 (IC+2C+2R) | 6 例 | ccd-power.json5 |
| EL7156 | 时钟驱动器 | 4 (IC+2C+1R) | 10 例 | ccd-clock.json5 |
| AO3408-low-side-switch | MOSFET 开关 | 2 (MOS+R) | 2 例 | ccd-control.json5 |
| NTC-divider | 温度采样 | 2 (NTC+R) | 1 例 | ccd-control.json5 |
| FP6277 | Boost 控制器 | 4 (IC+L+2C) | 1 例 | ccd-power-full.json5 |
| FP6276 | Boost 控制器 | 4 (IC+L+2C) | 1 例 | ccd-power-full.json5 |
| ME2802 | Boost 控制器 | 4 (IC+L+2C) | 1 例 | ccd-power-full.json5 |
| SY7208 | Boost 控制器 | 4 (IC+L+2C) | 1 例 | ccd-power-full.json5 |

**待扩展模板：**

| IC | 用途 | 使用次数 | 复杂度 | 验证来源 |
|----|------|---------|--------|---------|
| 74HC04 | 六反相器 | 1 | 低 | ccd-control.json5 |
| KAF-09001 | CCD 传感器 | 1 | 高 | ccd-sensor.json5 |

### 已完成的实施步骤

1. ✅ **IC 核心模板数据结构** — `ic_template.rs`：IcCoreTemplate + 公式求解器 + 参数依赖解析
2. ✅ **原理图生成** — `design.rs` 扩展 `generate_ic_schematic()`：IC + 外围元件 → .kicad_sch
3. ✅ **CLI 集成** — `cdb ic-design --template <name> --params <k=v> --nets <k=v> -o <file>`
4. ✅ **华秋 API pin 获取** — `ic_template::fetch_pins_from_hqapi(mpn)` 从华秋自动提取 pin 列表
5. ✅ **4 个 Boost IC 模板** — FP6277/FP6276/ME2802/SY7208，SOT-23-6 统一封装
6. ✅ **全板组合验证** — ccd-power-full.json5（10 模块 / 4 Boost + 6 LDO / 20 全局网络）
7. ✅ **Pipeline 链式调用** — pipeline.rs：buck/boost/ldo/led 四条 pipeline + DesignLog JSON
8. ✅ **symgen 与 cdb 集成** — 三 crate 独立架构 + `cdb export --format spec` JSON5 桥接

### 不同设计的模块组合示例

| 设计 | Skill 模块 | IC 模板 | 说明 |
|------|-----------|---------|------|
| CCD 承载板 | power(4路LDO+Buck) | EL7156×10, KAF-09001, 74HC04 | 已有原理图可验证 |
| STM32+RGB屏 | power(2路) | STM32F4, RGB驱动IC, LCD | 需新建模板 |
| 电机驱动板 | power+驱动 | 电机驱动IC, 栅极驱动 | 需新建模板 |
| FPGA板 | power(多路) | FPGA, DDR, 配置Flash | 需新建模板 |
