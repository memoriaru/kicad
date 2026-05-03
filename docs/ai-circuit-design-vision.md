---
name: AI 辅助电路设计愿景与架构
description: 用户对 AI 辅助电路设计的完整构想：SQLite 元件库 + 设计规则 Skill + kicad-json5 编译管线，三层知识体系打通从设计意图到出图的全流程
type: project
originSessionId: 422e4240-a5d2-4575-98be-2332704728d8
---
# AI 辅助电路设计 — 架构构想

## 核心理念

数据驱动的硬件设计：用结构化数据库作为 Single Source of Truth，通过工具链自动生成所有下游产物（原理图、PCB、元件库）。让 AI 能"看见"元件、"理解"规则、"输出"图纸。

## 三层知识体系

```
Layer 3: 设计意图 (Design Intent)
  → AI 理解需求，拆解为功能模块

Layer 2: 设计知识 (Design Knowledge)
  → 拓扑库 / 公式库 / 规则库（结构化的可执行设计约束）

Layer 1: 元件数据 (Component Data)
  → SQLite: 参数 / 封装 / 符号 / 供应商（AI 可查询的事实基础）
```

## 五个断裂点及解法

| 断裂点 | 问题 | 解法 |
|--------|------|------|
| 元件不可见 | AI 只看到文字，看不到参数 | SQLite 元件库，结构化 params JSON |
| 设计规则不成体系 | 知识散落在经验/文档中 | 可执行 skill 集合（带约束的函数） |
| 知识到图纸的鸿沟 | 无程序化出图通道 | kicad-json5 编译器 |
| 拓扑选型无形式化 | 无法从需求推导拓扑 | 拓扑库 + 选型公式 |
| 参数计算不可追溯 | 选型依据不可查 | 每个决策记录计算过程和约束 |

## 优先级

1. **P0**: SQLite 元件库 schema + 导入工具（让 AI 看得见元件）
2. **P1**: 设计规则 skill 集（电源/信号完整性/EMC，让 AI 能决策）
3. **P2**: JSON5 设计描述格式扩展（支持设计约束，让 AI 能表达）
4. **P3**: kicad-json5 自动生成完整工程（让 AI 能出图）

## 关联项目

- kicad-json5: 编译管线核心（已完成 sch 导出，待做 sym/pcb）
- 设计规范文档: docs/kicad-sch-design-notes.md, kicad-sym-design-notes.md, kicad-pcb-design-notes.md

## 现有方案对比：atopile 的能力边界

### atopile 能做什么

atopile 采用"模板 + 约束"模式，对**拓扑已知的标准化模块**表现优秀：

```
输入: vin=12V, vout=5V, iout=2A
  ↓ 公式链自动推导
电感值 = (Vout × (1 - Vout/Vin)) / (fsw × ΔIL × Iout)
输出电容 ≥ Iout × D / (2 × fsw × ΔVripple)
反馈电阻 = 上下分压比设定 Vout
  ↓ 约束检查
check Inductor.isat >= Iout × 1.3
check Cap.voltage_rating >= Vout × 2
```

覆盖的模块类型：Buck / Boost / Buck-Boost / LDO（含热分析、效率计算、PCB 布局约束）。

### atopile 做不了什么

非标准 IC 无法纳入约束体系，原因有三：

| 卡点 | 示例 |
|------|------|
| **引脚功能复杂** | MCU 100+ pin，每 pin 有 3-5 个复用功能，不能用几个参数概括 |
| **外围电路依赖场景** | 同一颗 MCU 做电机控制 vs 做 WiFi 网关，外围完全不同 |
| **没有通用选型公式** | 不像 LDO 的 `Vdrop = Vin - Vout`，IC 选型依赖大量隐性工程判断 |

```
atopile 模式:  一个模板 → 一种拓扑 → 固定参数推导
我们的目标:    一个知识库 → 任意元件 → 按需组合约束
```

### 启示

atopile 的约束表达方式（interface / check / calculate）值得借鉴，但需要从"模板驱动"升级为"数据驱动"：
- atopile: 设计者手动写 .ato 模板描述拓扑
- 我们的方案: AI 从 SQLite 元件库查询参数 + 从规则库选择约束 + 自动生成设计

**Why:** 用户希望从 AI 辅助设计的角度重新思考硬件工具链，不只是做格式转换，而是打通从设计意图到出图的全流程。

**How to apply:** 所有后续 kicad-json5 的功能扩展（PCB 导出、元件库生成等）都应围绕"让 AI 能驱动"这个目标来设计。SQLite 元件库是整个系统的地基，应最先启动。
