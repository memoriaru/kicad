# KiCad 原理图文件格式设计规范

> 基于 KiCad 源码 (eeschema/) 分析，用于指导 kicad-json5 转换器的设计

## 1. 数据模型层级

```
SCHEMATIC
  └── SCH_SCREEN (原理图页面)
        ├── SCH_SYMBOL (元件实例)
        │     └── SCH_PIN (引脚实例)
        ├── SCH_LINE (wire/bus)
        ├── SCH_JUNCTION (连接点)
        ├── SCH_LABEL / SCH_GLOBALLABEL / SCH_HIERLABEL (网络标签)
        ├── SCH_NO_CONNECT (未连接标记)
        ├── SCH_SHEET (层次化子图纸)
        └── ...
  └── LIB_SYMBOL (库符号定义)
        ├── LIB_PIN (引脚定义)
        └── 图形元素 (arc, circle, polyline, rectangle, text)
  └── CONNECTION_GRAPH (连接图)
        └── CONNECTION_SUBGRAPH (子图)
              ├── m_driver (驱动器: 具名网络项)
              ├── m_members (跟随者: 连接项)
              └── m_driver_connection (网络连接对象)
```

## 2. TRANSFORM 坐标变换

### 2.1 矩阵定义

KiCad 使用 2×2 矩阵 `{x1, y1, x2, y2}`:

```
x' = x1 * x + y1 * y
y' = x2 * x + y2 * y
```

### 2.2 旋转矩阵

| 角度 | 矩阵参数 | 含义 |
|------|---------|------|
| 0° | `(1, 0, 0, 1)` | 恒等变换 |
| 90° | `(0, 1, -1, 0)` | 顺时针 90° |
| 180° | `(-1, 0, 0, -1)` | 180° 翻转 |
| 270° | `(0, -1, 1, 0)` | 逆时针 90° (顺时针 270°) |

### 2.3 Pin 绝对位置计算

```
absolute_pos = component_pos + TransformCoordinate(pin_local_pos)
```

示例：引脚本地坐标 `(7.62, 2.54)`，元件在 `(100, 50)` 旋转 90°:
```
x' = 0 * 7.62 + 1 * 2.54 = 2.54
y' = -1 * 7.62 + 0 * 2.54 = -7.62
absolute = (100 + 2.54, 50 + -7.62) = (102.54, 42.38)
```

### 2.4 Pin 方向定义

| 旋转值 | 方向 | 引脚线延伸方向 | GetPinRoot() 偏移 |
|--------|------|--------------|------------------|
| 0 | PIN_RIGHT | 向右 (+x) | (+length, 0) |
| 90 | PIN_UP | 向上 (-y) | (0, -length) |
| 180 | PIN_LEFT | 向左 (-x) | (-length, 0) |
| 270 | PIN_DOWN | 向下 (+y) | (0, +length) |

**关键**: Pin 的 `(at x y rotation)` 指定的是**连接点**(wire 连接端)。
引脚线从连接点**延伸到 body**，方向由 rotation 决定。

## 3. S-expression 文件结构

### 3.1 顶层元素 (kicad_sch 的子元素，按解析顺序)

```
(kicad_sch
  (version "YYYYMMDD")
  (generator "name")
  (generator_version "X.Y")
  (uuid "...")
  (paper "A4" [width height])
  (title_block ...)
  (lib_symbols ...)
  (symbol ...)         ← 元件实例，可多个
  (image ...)
  (sheet ...)          ← 层次化子图纸
  (junction ...)
  (no_connect ...)
  (bus_entry ...)
  (polyline ...)       ← 注意：不是 (wire)，KiCad 内部用 SCH_LINE
  (wire ...)
  (bus ...)
  (arc ...)
  (circle ...)
  (rectangle ...)
  (bezier ...)
  (rule_area ...)
  (text ...)
  (label ...)
  (global_label ...)
  (hierarchical_label ...)
  (directive_label ...)
  (text_box ...)
  (table ...)
  (group ...)
  (sheet_instances ...)   ← v10+
  (embedded_fonts ...)    ← v9+
)
```

**注意**: `(net ...)` 不是 `(kicad_sch)` 的有效子元素！网络通过 wire + label 推断。

### 3.2 lib_symbol 结构

```
(symbol "lib_id"
  [power]
  [pin_names (offset N) (hide yes)]
  [pin_numbers (hide yes)]
  (exclude_from_sim yes/no)
  (in_bom yes/no)
  (on_board yes/no)
  (in_pos_files yes/no)                           ← v10+
  (duplicate_pin_numbers_are_jumpers yes/no)       ← v10+
  (property "Reference" "value" ...)
  (property "Value" "value" ...)
  ...
  (symbol "name_0_1" ...graphics...)               ← 体图形
  (symbol "name_1_1" ...pins...)                   ← 引脚位置
  (embedded_fonts no)                               ← v9+，父级
)
```

**字段顺序**: 布尔标志必须在 unit 子符号之前。

### 3.3 Pin S-expression 格式

```
(pin <type> <shape>
  (at x y <0|90|180|270>)     ← 必须有 3 个值
  (length N)
  (name "name" (effects ...))
  (number "number" (effects ...))
)
```

### 3.4 Symbol 实例格式

```
(symbol
  (lib_id "Library:SymbolName")
  (at x y <0|90|180|270>)       ← 必须有 3 个值
  [mirror x|y]
  (unit N)
  (exclude_from_sim yes/no)
  (in_bom yes/no)
  (on_board yes/no)
  (dnp yes/no)
  (uuid "...")
  (property "Reference" "value" ...)
  (property "Value" "value" ...)
  (pin "number" (uuid "..."))
  ...
  (instances ...)
)
```

## 4. 连接图模型 (CONNECTION_GRAPH)

### 4.1 网络名驱动优先级

```
GLOBAL           ← 最高优先级 (global_label, 电源符号)
GLOBAL_POWER_PIN
LOCAL_POWER_PIN
LOCAL_LABEL
HIER_LABEL
SHEET_PIN
PIN              ← 最低优先级
NONE
```

多个连接项在同一子图时，优先级最高的成为 `m_driver`，其名称成为网络名。

### 4.2 连接规则

- 共享同一几何点的 wire 端点、pin 连接点、label、junction 属于同一子图
- 子图之间通过相同 net name 的 global_label 或 hierarchical_label 合并
- 电源 pin (power_in/power_out) 隐式创建全局网络

## 5. 版本差异

| 特性 | V7 (20221219) | V8 (20231120) | V9 (20250114) | V10 (20260306) |
|------|:---:|:---:|:---:|:---:|
| generator_version | 7.0 | 8.0 | 9.0 | 10.0 |
| embedded_fonts | - | - | lib_symbol + 文件级 | lib_symbol + 文件级 |
| in_pos_files | - | - | - | lib_symbol 级 |
| duplicate_pin_numbers_are_jumpers | - | - | - | lib_symbol 级 |
| show_name (property) | - | - | - | property 级 |
| do_not_autoplace (property) | - | - | - | property 级 |
| sheet_instances | - | - | - | 文件级 |
| pin_numbers hide | `(pin_numbers hide)` | `(pin_numbers hide)` | `(pin_numbers (hide yes))` | `(pin_numbers (hide yes))` |
| top-level (net) | 不支持 | 不支持 | 不支持 | 不支持 |

## 6. kicad-json5 转换器设计要点

### 6.1 已实现
- V7-V10 多版本输出，版本自适应
- 默认符号模板 (IC/R/C/L/D/LED/Generic)
- Pin 坐标变换 (KiCad 顺时针旋转矩阵)
- Wire 自动生成 (L 形连线)
- 正确的 (embedded_fonts no) 父级放置

### 6.2 待优化
- **Wire 路由**: 当前 L 形链式连接，可优化为总线轨道式
- **Junction 自动插入**: wire 交叉点需要 junction 标记
- **Net label 放置**: 自动在 wire 端点放置 net name label
- **层次化图纸**: 多页原理图支持
- **图形元素**: 更多 lib_symbol 图形 (bezier, text_box)

### 6.3 格式严格遵守
- `(at x y rotation)` 必须有 3 个值
- Pin rotation 必须是 0/90/180/270
- `(embedded_fonts no)` 在父级 symbol 后、关闭 `)` 前
- 布尔标志在 unit 子符号之前
- 不输出 `(net ...)` 作为 kicad_sch 子元素
