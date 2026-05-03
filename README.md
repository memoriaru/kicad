# kicad-rs

Rust 工具链，为 AI 辅助电路设计优化 KiCad 工作流。

## 子项目

| 项目 | 说明 | 版本 |
|------|------|------|
| [kicad-json5](kicad-json5/) | KiCad S-expression ↔ JSON5 双向编译器 + 拓扑提取 | 0.8.5 |
| [kicad-render](kicad-render/) | KiCad 原理图 SVG 渲染器（KiCanvas 1:1 移植） | 0.2.3 |

## kicad-json5

将 KiCad `.kicad_sch` 文件与 JSON5 双向转换，为 LLM 提供结构清晰、token 高效的中间表示。

```bash
# S-expression → JSON5
kicad-json5 schematic.kicad_sch -o schematic.json5

# JSON5 → KiCad（自动插入 PWR_FLAG）
kicad-json5 schematic.json5 -o schematic.kicad_sch --power-flags

# 提取电路拓扑
kicad-json5 schematic.kicad_sch -f topology
```

核心特性：标准 Device 库嵌入、自动 wire/label 生成、PWR_FLAG 插入、No-Connect 标记、拓扑提取。

详见 [kicad-json5/README.md](kicad-json5/README.md) | [English](kicad-json5/README_EN.md)

## kicad-render

将 `.kicad_sch` 文件渲染为高保真 SVG 矢量图，渲染逻辑从 KiCanvas JS 1:1 移植到 Rust。

```bash
kicad-render schematic.kicad_sch -o output.svg
```

也支持作为库调用，可选编译为 WASM 在浏览器中渲染。

详见 [kicad-render/README.md](kicad-render/README.md) | [English](kicad-render/README_EN.md)

## 工具链配合

```
.kicad_sch → kicad-json5 → JSON5（AI 可读写）→ kicad-json5 → .kicad_sch
                          ↓
                     kicad-render → SVG 可视化
```

## 致谢

- [KiCanvas](https://github.com/theacodes/kicanvas) — kicad-render 的 Painter 层架构和渲染算法来源
- [ecad-viewer](https://github.com/AbijahKecadan/ecad-viewer) — 基于 KiCanvas 的 KiCad 文件在线查看器

## License

MIT
