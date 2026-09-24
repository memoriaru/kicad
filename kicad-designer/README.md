# kicad-designer

AI-driven circuit design orchestrator — unified CLI + MCP Server.

AI 驱动电路设计编排器，统一 CLI 和 MCP Server 接口，编排四个子库完成从需求到原理图的全流程。

## Architecture / 架构

```
                ┌─────────────────────┐
                │   kicad-designer     │  ← AI 编排层
                │   MCP Server + CLI   │
                └──┬──┬──┬──┬─────────┘
                   │  │  │  │
      ┌────────────┘  │  │  └──────────────┐
      ▼               ▼  ▼                  ▼
kicad-cdb      kicad-json5      kicad-symgen   kicad-render
(数据层)        (编译层)         (生成层)       (渲染层)
```

## Features / 功能

- **Unified CLI** (`kdesign`) — 统一命令行接口，整合设计/查询/验证全流程
- **MCP Server** — AI 可通过 MCP 协议直接调用设计能力
- **End-to-end workflow** — 需求 → 拓扑选型 → 参数计算 → 元件选型 → 原理图生成 → ERC 验证

## CLI Commands / 命令

```bash
# MCP Server for AI integration
kdesign serve

# Topology suggestion
kdesign suggest --vin 12 --vout 3.3 --iout 2

# Generate schematic from template
kdesign design --template buck --params "vin=12,vout=3.3,iout=2" -o output.kicad_sch

# Run design pipeline
kdesign pipeline buck --params "vin=12,vout=3.3,iout=2,fsw=500000"

# Design space exploration
kdesign explore --vin 12 --vout 3.3 --iout 2

# ERC / DRC
kdesign erc --input schematic.kicad_sch
kdesign drc --input board.kicad_pcb
```

## Status / 状态

**状态**：CLI 与 MCP Server 已可用（原理图生成/查询/验证/布线编排全流程），PCB 文本手术与 v35 布线编排为活跃开发区。

## License

Apache License 2.0
