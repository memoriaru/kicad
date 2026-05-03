# KiCad 元件库文件格式设计规范

> 基于 KiCad 源码 (eeschema/) 分析，用于指导 kicad-json5 元件库 (.kicad_sym) 导入/导出设计

## 1. 文件概述

`.kicad_sym` 是 KiCad 7+ 引入的元件库文件格式（替代旧版 `.lib`），采用 S-expression 格式。
每个文件可包含多个元件符号定义。

### 1.1 文件命名

```
<library_name>.kicad_sym        ← 例如: MCU_Microchip_ATmega.kicad_sym
```

### 1.2 版本历史

| 版本日期 | 对应 KiCad | 关键变更 |
|---------|-----------|---------|
| 20200126 | 6.0 (预览) | 初始格式，引脚 alternate 定义 |
| 20210619 | 6.0 | 引脚上划线语法 `~...~` → `~{...}` |
| 20220914 | 7.0 | unit 显示名，property 不再保存 ID |
| 20231120 | 8.0 | 清理与标准化 |
| 20240529 | 8.99/9.0 | 嵌入文件支持 (embedded_files) |
| 20241004 | 9.0 | hide/use_boolean 改为 `(hide yes)` 格式 |
| 20250318 | 9.99 | `~` 不再表示空文本 |
| 20250901 | 10.0 | 堆叠引脚表示 (pin range: 1-8) |
| 20251024 | 10.0 | property 格式更新 (do_not_autoplace, show_name) |

## 2. 数据模型层级

```
LIB_SYMBOL (元件符号定义)
  ├── extends → LIB_SYMBOL (继承/派生)
  ├── Properties (属性/字段)
  │     ├── Reference (必须)
  │     ├── Value (必须)
  │     ├── Footprint (可选)
  │     ├── Datasheet (可选)
  │     ├── Description (可选)
  │     ├── ki_keywords (内部)
  │     ├── ki_fp_filters (内部)
  │     └── 自定义属性
  ├── Draw Items (图形项，按 unit/convert 组织)
  │     ├── LIB_PIN (引脚)
  │     ├── LIB_SHAPE_ARC (弧线)
  │     ├── LIB_SHAPE_CIRCLE (圆)
  │     ├── LIB_SHAPE_RECT (矩形)
  │     ├── LIB_SHAPE_POLY (多段线)
  │     ├── LIB_SHAPE_BEZIER (贝塞尔曲线)
  │     ├── LIB_TEXT (文本)
  │     └── LIB_TEXTBOX (文本框)
  └── Unit/Convert 结构
        ├── Unit 0 = 共享图形 (所有 unit 共有)
        ├── Unit 1..N = 各独立 unit
        ├── Convert 1 = 标准风格 (Style A)
        └── Convert 2 = De Morgan 风格 (Style B)
```

## 3. S-expression 文件结构

### 3.1 文件顶层

```
(kicad_symbol_lib
  (version "YYYYMMDD")
  (generator "name")
  (generator_version "X.Y")
  (symbol "SymbolName1" ...)
  (symbol "SymbolName2" ...)
  ...
)
```

**注意**: 根元素是 `(kicad_symbol_lib)`，不是 `(kicad_sch)` 中的 `(lib_symbols)`。

### 3.2 完整 Symbol 结构

```
(symbol "SymbolName"
  [power global|local]                           ← 电源符号标记
  [extends "ParentName"]                         ← 继承/派生
  [pin_numbers (hide yes)]                       ← 隐藏引脚编号
  [pin_names (offset N) [(hide yes)]]            ← 引脚名称设置
  [exclude_from_sim yes|no]                      ← 仿真排除
  [in_bom yes|no]                                ← BOM 包含
  [on_board yes|no]                              ← PCB 包含
  [in_pos_files yes|no]                          ← v10+, 位置文件包含
  [duplicate_pin_numbers_are_jumpers yes|no]      ← v10+, 跳线引脚
  (property "Reference" "R" ...)
  (property "Value" "10k" ...)
  (property "Footprint" "Lib:Footprint" ...)
  (property "Datasheet" "~" ...)
  ...更多 property...
  (symbol "SymbolName_0_1" ...)                  ← 共享体图形
  (symbol "SymbolName_1_1" ...)                  ← Unit1 StyleA (引脚+图形)
  [symbol "SymbolName_1_2" ...]                  ← Unit1 StyleB (De Morgan)
  [symbol "SymbolName_2_1" ...]                  ← Unit2 StyleA
  ...
  [embedded_fonts yes|no]                        ← v9+
)
```

### 3.3 字段顺序规则（严格遵守）

在 `(symbol ...)` 内，**必须按以下顺序**:

1. `power` / `extends`
2. `pin_numbers`
3. `pin_names`
4. `exclude_from_sim`
5. `in_bom`
6. `on_board`
7. `in_pos_files` (v10+)
8. `duplicate_pin_numbers_are_jumpers` (v10+)
9. `property` (按定义顺序)
10. `symbol` (unit 子块，按 unit/convert 排序)
11. `embedded_fonts` (v9+，父级末尾)

**违反顺序会导致 KiCad 解析错误！**

## 4. Unit 子块命名规则

### 4.1 命名格式

```
<SymbolName>_<unit>_<convert>
```

- `unit`: 0 = 共享图形，1..N = 各独立 unit
- `convert`: 1 = 标准风格，2 = De Morgan 风格

### 4.2 示例

```
(symbol "ATmega328P"
  ...
  (symbol "ATmega328P_0_1" ...)     ← 所有 unit 共享的体图形
  (symbol "ATmega328P_1_1" ...)     ← Unit 1 (端口A), Style A
  (symbol "ATmega328P_2_1" ...)     ← Unit 2 (端口B), Style A
  (symbol "ATmega328P_3_1" ...)     ← Unit 3 (电源), Style A
)
```

### 4.3 Unit 0 的意义

Unit 0 包含**所有 unit 共有的图形元素**（如元件体矩形框）。这些图形在放置时会显示在每个 unit 实例上。

## 5. Property 属性格式

### 5.1 标准格式

```
(property "name" "value"
  (at x y [rotation])
  (effects
    (font (size width height) [thickness] [italic] [bold] [face "name"])
    [justify left|right|top|bottom|mirror]
  )
  [hide yes]
  [show_name yes]                    ← v10+
  [do_not_autoplace yes]             ← v10+
  [private]                          ← v9+，私有属性
)
```

### 5.2 标准属性

| 属性名 | 说明 | 默认值 |
|-------|------|-------|
| `Reference` | 元件参考号 (R?, C?, U?) | 必须 |
| `Value` | 元件值/型号 | 必须 |
| `Footprint` | 关联封装 | 可选 |
| `Datasheet` | 数据手册链接 | 可选 |
| `Description` | 描述 | 可选 |
| `ki_keywords` | 搜索关键词 | 内部自动生成 |
| `ki_fp_filters` | 封装过滤器 | 内部自动生成 |

### 5.3 属性可见性

| 情况 | 格式 |
|------|------|
| 显示 | 无额外标记 |
| 隐藏 | `(hide yes)` |
| 显示属性名 | `(show_name yes)` (v10+) |
| 不自动布局 | `(do_not_autoplace yes)` (v10+) |
| 私有 | `(private)` (v9+) |

## 6. Pin 引脚格式

### 6.1 完整格式

```
(pin <electrical_type> <shape>
  (at x y <0|90|180|270>)            ← 必须 3 个值
  (length N)
  [hide yes]                          ← 隐藏引脚
  [name "name"
    (effects (font (size w h)))]
  [number "number"
    (effects (font (size w h)))]
  [alternate "alt_name" <alt_type> <alt_shape>]  ← 备用引脚定义
)
```

### 6.2 引脚电气类型 (electrical_type)

| 类型 | 说明 |
|------|------|
| `input` | 输入 |
| `output` | 输出 |
| `bidirectional` | 双向 |
| `tri_state` | 三态 |
| `passive` | 被动 |
| `unspecified` | 未指定 |
| `power_in` | 电源输入 |
| `power_out` | 电源输出 |
| `open_collector` | 开路集电极 |
| `open_emitter` | 开路发射极 |
| `no_connect` | 不连接 |
| `free` | 自由 |

### 6.3 引脚形状 (shape)

| 形状 | 说明 | 图形 |
|------|------|------|
| `line` | 普通线 | ───── |
| `inverted` | 反相 (圆圈) | ──○── |
| `clock` | 时钟 (三角) | ▷──── |
| `inverted_clock` | 反相时钟 | ──○▷── |
| `input_low` | 低电平输入 | ──┘── |
| `clock_low` | 低电平时钟 | ──┘▷── |
| `output_low` | 低电平输出 | ──┐── |
| `edge_clock_high` | 边沿时钟高 | ──┐▷── |
| `non_logic` | 非逻辑 (×) | ──×── |

### 6.4 引脚方向与旋转

| rotation 值 | 方向 | 连接点位置 | 引脚线延伸方向 |
|------------|------|-----------|--------------|
| 0 | PIN_RIGHT | 右端 | 向右 (+x) |
| 90 | PIN_UP | 上端 | 向上 (-y) |
| 180 | PIN_LEFT | 左端 | 向左 (-x) |
| 270 | PIN_DOWN | 下端 | 向下 (+y) |

**关键**: `(at x y rotation)` 指定的是**连接点**(wire 连接端)。
引脚线从连接点向元件体方向延伸，方向由 rotation 决定。

### 6.5 备用引脚定义 (alternate)

```
(pin bidirectional line
  (at -7.62 7.62 0)
  (length 2.54)
  (name "PA0"
    (effects (font (size 1.27 1.27))))
  (number "1"
    (effects (font (size 1.27 1.27))))
  (alternate "ADC0" input line)        ← 备用功能
  (alternate "AREF" passive line)      ← 另一个备用功能
)
```

一个引脚可以有多个 alternate 定义，每个定义包含名称、电气类型和形状。

### 6.6 堆叠引脚 (v10+)

v10 支持引脚范围表示法，用于总线连接器等场景：

```
(pin "1-8" input line ...)             ← 展开为 pin1 到 pin8
```

## 7. 图形元素格式

### 7.1 通用结构

所有图形元素共享以下属性：

```
(元素类型
  ...几何参数...
  (stroke
    (width N)
    (type default|dash|dot|dash_dot)   ← v8+
  )
  (fill
    (type none|outline|background|color|hatch|reverse_hatch|cross_hatch)
    [color r g b a]                     ← v9+, 仅 color 类型
  )
)
```

### 7.2 矩形 (rectangle)

```
(rectangle
  (start x1 y1)
  (end x2 y2)
  (stroke (width N) (type default))
  (fill (type none))
)
```

### 7.3 多段线 (polyline)

```
(polyline
  (pts
    (xy x1 y1)
    (xy x2 y2)
    (xy x3 y3)
    ...
  )
  (stroke (width N) (type default))
  (fill (type none))                    ← 可选，闭合时有效
)
```

### 7.4 弧线 (arc)

```
(arc
  (start cx cy)                         ← 圆心
  (mid mx my)                           ← 中间点 (确定弧度)
  (end ex ey)                           ← 终点
  (stroke (width N) (type default))
  (fill (type none))
)
```

### 7.5 圆 (circle)

```
(circle
  (center cx cy)
  (radius N)
  (stroke (width N) (type default))
  (fill (type none))
)
```

### 7.6 贝塞尔曲线 (bezier)

```
(bezier
  (pts
    (xy x1 y1)                          ← 起点
    (xy x2 y2)                          ← 控制点 1
    (xy x3 y3)                          ← 控制点 2
    (xy x4 y4)                          ← 终点
  )
  (stroke (width N) (type default))
  (fill (type none))
)
```

### 7.7 文本 (text)

```
(text "content"
  (at x y [rotation])
  (effects
    (font (size w h) [thickness] [italic] [bold] [face "name"])
    [justify left|right|top|bottom|mirror]
  )
)
```

### 7.8 文本框 (text_box) — v8+

```
(text_box "content"
  (at x y [rotation])
  (size width height)
  (stroke (width N) (type default))
  (fill (type none))
  (effects ...)
)
```

## 8. 坐标与单位

### 8.1 内部单位

- 库文件内部使用 **mil** (1/1000 英寸) 为基本单位
- 常用间距: 2.54mm = 100mil, 1.27mm = 50mil
- 坐标原点: 元件符号的参考点 (通常在元件体中心)

### 8.2 S-expression 中的单位

文件中以 **毫米 (mm)** 表示坐标值，KiCad 内部转换为 mil 运算。

### 8.3 坐标系

- 原点: 元件符号参考点
- X 轴: 向右为正
- Y 轴: **向上为正** (与 PCB 相反！)
- 旋转: KiCad 使用 **顺时针** 旋转矩阵

## 9. 继承/派生 (extends)

### 9.1 格式

```
(symbol "DerivedSymbol"
  (extends "BaseSymbol")              ← 继承父元件
  (property "Value" "10k" ...)       ← 覆盖属性
  ...                                  ← 可添加/覆盖图形项
)
```

### 9.2 继承规则

- 子符号自动继承父符号的所有图形、引脚、属性
- 子符号可以覆盖属性值
- 子符号可以添加新的图形项
- `extends` 必须在 symbol 内的第一个字段（power 之后）
- 父符号必须在同一库文件中或已被加载

### 9.3 典型用法

- 电阻系列: 基础电阻 → 特定阻值
- IC 系列: 基础型号 → 引脚兼容的升级型号
- 电源符号: 通用电源符号 → 特定电压

## 10. 电源符号

### 10.1 格式

```
(symbol "VCC"
  (power global)                       ← 全局电源标记
  ...
  (pin power_in line
    (at 0 0 90)
    (length 0)                         ← 长度通常为 0
    (hide yes)                         ← 隐藏
    (name "VCC"
      (effects (font (size 1.27 1.27))))
    (number "1"
      (effects (font (size 1.27 1.27))))
  )
)
```

### 10.2 电源符号特征

- `(power global)` 或 `(power local)` 标记
- 引脚类型为 `power_in` 或 `power_out`
- 引脚长度通常为 0 且隐藏
- 全局电源符号在所有原理图页面可见
- 电源引脚隐式创建全局网络

## 11. kicad-json5 库文件转换设计

### 11.1 JSON5 IR 扩展

库文件需要独立的 IR 结构：

```
LibFile
  ├── version: KicadVersion
  ├── symbols: Vec<LibSymbol>
  │     ├── name: String
  │     ├── power: Option<PowerType>
  │     ├── extends: Option<String>
  │     ├── pin_numbers_hide: bool
  │     ├── pin_names_offset: f64
  │     ├── pin_names_hide: bool
  │     ├── exclude_from_sim: bool
  │     ├── in_bom: bool
  │     ├── on_board: bool
  │     ├── in_pos_files: bool                         ← v10+
  │     ├── duplicate_pin_numbers_are_jumpers: bool     ← v10+
  │     ├── properties: Vec<Property>
  │     ├── units: Vec<UnitDef>
  │     │     ├── unit_id: u32
  │     │     ├── convert_id: u32
  │     │     ├── unit_name: Option<String>
  │     │     ├── graphics: Vec<GraphicItem>
  │     │     └── pins: Vec<Pin>
  │     └── embedded_fonts: bool                       ← v9+
```

### 11.2 导入流程 (.kicad_sym → JSON5)

```
.kicad_sym → S-expr Parser → LibIR → JSON5 Generator → .json5
```

关键点：
- 解析 `(kicad_symbol_lib)` 顶层
- 每个 `(symbol ...)` 转为一个 LibSymbol
- unit 子块按 `_unit_convert` 命名解析
- 图形元素按类型分发解析
- property 提取标准字段 + 自定义字段

### 11.3 导出流程 (JSON5 → .kicad_sym)

```
.json5 → JSON5 Parser → LibIR → S-expr Generator → .kicad_sym
```

### 11.4 与原理图中 lib_symbols 的区别

| 方面 | 库文件 (.kicad_sym) | 原理图内嵌 (lib_symbols) |
|------|-------------------|----------------------|
| 根元素 | `(kicad_symbol_lib)` | `(lib_symbols)` |
| symbol 完整度 | 完整定义 | 可能简化 (无 power 标记等) |
| 多符号 | 支持 (每文件多个) | 内嵌所有用到的符号 |
| extends | 支持 | 不支持 |
| version | 独立版本号 | 跟随原理图版本 |
| embedded_fonts | 支持 | 支持 |

### 11.5 格式严格遵守

- 字段顺序必须正确（第 3.3 节）
- `(at x y rotation)` — rotation 必须为 0/90/180/270
- Pin 的 name/number 必须有 effects
- `(embedded_fonts no)` 在父级 symbol 末尾
- Unit 子块命名: `<Name>_<unit>_<convert>`
- 不使用已废弃的 bare 关键字 (如 `hide` → `hide yes`)
- 所有图形元素必须有 stroke 定义
- 填充类型为 `none` 时仍需显式写出 `(fill (type none))`

### 11.6 常见陷阱

1. **字段顺序错误**: KiCad 解析器严格按顺序读取，乱序会报错
2. **Pin rotation 缺失**: `(at x y)` 缺少第三个值会解析失败
3. **Unit 命名错误**: 子块名必须与父 symbol 名完全匹配前缀
4. **power 标记遗漏**: 电源符号缺少 `(power global)` 导致网络推断失败
5. **hide 格式**: v7+ 必须用 `(hide yes)`，不能用 bare `hide`
6. **继承循环**: extends 链不能形成循环引用
