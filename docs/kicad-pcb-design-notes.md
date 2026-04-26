# KiCad PCB 文件格式设计规范

> 基于 KiCad 源码 (pcbnew/) 分析，用于指导 kicad-json5 PCB 导出功能的设计

## 1. 数据模型层级

```
BOARD
  ├── BOARD_DESIGN_SETTINGS (设计规则)
  ├── BOARD_STACKUP (层叠结构)
  ├── FOOTPRINT (封装实例)
  │     ├── PAD (焊盘)
  │     │     ├── NETINFO_ITEM (网络关联)
  │     │     └── pinfunction / pintype
  │     ├── FP_SHAPE (封装图形: line/arc/circle/poly/text)
  │     ├── FP_TEXT (封装文本: Reference, Value, 自定义)
  │     ├── FP_ZONE (封装内铜区)
  │     └── FP_3DMODEL (3D 模型)
  ├── PCB_TRACK / PCB_ARC / PCB_VIA (走线/弧线/过孔)
  ├── ZONE (铜区/禁止区)
  ├── BOARD_CONNECTED_ITEM (所有连接项的基类)
  │     ├── NETINFO_ITEM (网络定义)
  │     └── NETCLASS (网络类)
  ├── PCB_SHAPE (板级图形)
  ├── PCB_TEXT / PCB_TEXTBOX (板级文本)
  ├── DIMENSION (标注)
  └── BOARD_ITEM (其他板级元素)
```

## 2. S-expression 文件结构

### 2.1 顶层元素 (kicad_pcb 的子元素，按解析顺序)

```
(kicad_pcb
  (version "YYYYMMDD")
  (generator "name")
  (generator_version "X.Y")
  (general
    (thickness N)
    (legacy_teardowns no)              ← v9+
  )
  (paper "A4" [width height])
  (title_block ...)
  (layers ...)
  (setup ...)
  (properties ...)
  (net [netcode] "netname") ...        ← 网络声明列表
  (net_class "name" "description" ...)
  (gr_circle ...)
  (gr_arc ...)
  (gr_poly ...)
  (gr_line ...)
  (gr_rect ...)
  (gr_text ...)
  (gr_text_box ...)                    ← v9+
  (dimension ...)
  (footprint ...)                      ← 封装实例，可多个
  (segment ...)
  (arc ...)                            ← v9+
  (via ...)
  (zone ...)
  (group ...)
  (embedded_fonts ...)                 ← v9+
)
```

### 2.2 版本映射

| 特性 | V7 (20221219) | V8 (20231120) | V9 (20250114) | V10 (20260306) |
|------|:---:|:---:|:---:|:---:|
| generator_version | 7.0 | 8.0 | 9.0 | 10.0 |
| embedded_fonts | - | - | 支持 | 支持 |
| (arc ...) 走线 | - | - | 支持 | 支持 |
| (gr_text_box ...) | - | - | 支持 | 支持 |
| legacy_teardrops | - | - | general 内 | general 内 |
| 属性 properties | 支持 | 支持 | 支持 | 支持 |

## 3. 层模型 (layers)

### 3.1 层定义格式

```
(layers
  (0 "F.Cu" signal)
  (1 "In1.Cu" signal)
  (2 "In2.Cu" signal)
  ...
  (31 "B.Cu" signal)
  (32 "B.Adhes" user)
  (33 "F.Adhes" user)
  (34 "B.Paste" user)
  (35 "F.Paste" user)
  (36 "B.SilkS" user)
  (37 "F.SilkS" user)
  (38 "B.Mask" user)
  (39 "F.Mask" user)
  (40 "Dwgs.User" user)
  (41 "Cmts.User" user)
  (42 "Eco1.User" user)
  (43 "Eco2.User" user)
  (44 "Edge.Cuts" user)
  (45 "Margin" user)
  (46 "B.CrtYd" user)
  (47 "F.CrtYd" user)
  (48 "B.Fab" user)
  (49 "F.Fab" user)
  (user [layer_number] "name" user)     ← 自定义层
)
```

### 3.2 层分类

| 类别 | 层号范围 | 说明 |
|------|---------|------|
| 铜层 | 0-31 | F.Cu(0), In1-In30.Cu(1-30), B.Cu(31) |
| 辅助层 | 32-49 | 丝印、阻焊、粘贴、注释、边框等 |
| 用户层 | 自定义 | user 关键字定义 |

**注意**: 铜层标记为 `signal`，辅助层标记为 `user`。

## 4. Setup 配置

```
(setup
  (stackup ...)                         ← 层叠结构
  (pad_to_mask_clearance N)
  (solder_mask_min_width N)
  (pad_to_paste_clearance N)
  (pad_to_paste_clearance_ratio N)
  (aux_axis_origin x y)
  (grid_origin x y)
  (pcbplotparams ...)
  (net_api_count N)                     ← v10+
)
```

### 4.1 层叠结构 (stackup)

```
(stackup
  (layer "F.Cu" (type copper))
  (layer "dielectric 1"
    (type core)
    (thickness N)
    (material "...") (epsilon_r N) (loss_tangent N))
  (layer "In1.Cu" (type copper))
  ...
  (layer "B.Cu" (type copper))
  (copper_finish "None")
  (dielectric_constraints no)
)
```

## 5. Footprint 封装格式

### 5.1 基本结构

```
(footprint "library:footprint_name"
  [locked]
  [placed]
  (layer "F.Cu" | "B.Cu")
  (at x y [rotation])                  ← rotation 可选，默认 0
  (uuid "...")
  [property "key" "value"]             ← v9+，可多个
  [tedit "hex_timestamp"]
  [descr "description")]
  [tags "tag1 tag2")]
  [path "/uuid/path")]
  [autoplace_cost90 N]
  [autoplace_cost180 N]
  [solder_mask_margin N]
  [solder_paste_margin N]
  [solder_paste_ratio N]
  [clearance N]
  [zone_connect 0|1|2]
  [thermal_relief_gap N]
  [thermal_relief_width N]
  [attr smd | virtual | exclude_from_pos_files | exclude_from_bom]
  (fp_text ...)
  (fp_line ...)
  (fp_arc ...)
  (fp_circle ...)
  (fp_rect ...)
  (fp_poly ...)
  (pad ...)
  (zone ...)                            ← 封装内铜区
  (model "path/to/3dmodel"
    (at xyz)
    (scale xyz)
    (rotate xyz)
  )
  (group ...)
)
```

### 5.2 封装文本 (fp_text)

```
(fp_text reference|value|user "text"
  (at x y [rotation])
  (layer "F.SilkS")
  (uuid "...")
  (effects (font (size w h) (thickness t)) [justify])
  [hide]
)
```

### 5.3 封装图形 (fp_line / fp_arc / fp_circle / fp_rect / fp_poly)

```
(fp_line (start x1 y1) (end x2 y2)
  (stroke (width N) (type solid|dashed|dotted|dash_dot))
  (layer "F.SilkS")
  (uuid "...")
)

(fp_arc (start cx cy) (mid mx my) (end ex ey)
  (stroke (width N) (type solid))
  (layer "...")
  (uuid "...")
)

(fp_circle (center cx cy) (end ex ey)
  (stroke (width N) (type solid))
  (fill none|solid)
  (layer "...")
  (uuid "...")
)

(fp_rect (start x1 y1) (end x2 y2)
  (stroke (width N) (type solid))
  (fill none|solid)
  (layer "...")
  (uuid "...")
)

(fp_poly (pts (xy x1 y1) (xy x2 y2) ...)
  (stroke (width N) (type solid))
  (fill none|solid)
  (layer "...")
  (uuid "...")
)
```

## 6. Pad 焊盘格式

### 6.1 焊盘结构

```
(pad "number" thru_hole|smd|connect|np_thru_hole
  circle|rect|oval|trapezoid|roundrect|custom
  (at x y [rotation])
  (size width height)
  [layers "layer1" "layer2" ...]
  [rect_delta dx dy]                    ← 仅 trapezoid
  [roundrect_rratio N]                  ← 仅 roundrect
  [chamfer_ratio N]                     ← v9+
  [chamfer top_left|top_right|bottom_left|bottom_right]  ← v9+
  [property pad_prop_bga|pad_prop_fiducial|pad_prop_testpoint|pad_prop_heatsink|pad_prop_castellated]
  (drill [oval] [dx dy] diameter)       ← thru_hole/np_thru_hole
  (paste_options ...)                   ← v9+
  (net netcode "netname")               ← 网络关联
  (pinfunction "name")                  ← v8+，引脚功能
  (pintype passive|input|output|bidirectional|power_in|power_out|...)  ← v8+
  (uuid "...")
  (solder_mask_margin N)
  (solder_paste_margin N)
  (solder_paste_margin_ratio N)
  (clearance N)
  (zone_connect 0|1|2)
  (thermal_relief_gap N)
  (thermal_relief_width N)
  (custom_shape ...)                    ← 仅 custom 类型
  (options ...)                         ← 仅 custom 类型
)
```

### 6.2 焊盘类型

| 类型 | 说明 | 钻孔 |
|------|------|------|
| `thru_hole` | 通孔焊盘 | 必须有 `(drill ...)` |
| `smd` | 表面贴装 | 无钻孔 |
| `connect` | 连接焊盘 (边缘连接器) | 无钻孔 |
| `np_thru_hole` | 非电镀孔 | 必须有 `(drill ...)` |

### 6.3 焊盘形状

| 形状 | 说明 | 必需参数 |
|------|------|---------|
| `circle` | 圆形 | `(size diameter)` |
| `rect` | 矩形 | `(size width height)` |
| `oval` | 椭圆/圆角矩形 | `(size width height)` |
| `roundrect` | 圆角矩形 | `(size w h)` + `(roundrect_rratio N)` |
| `trapezoid` | 梯形 | `(size w h)` + `(rect_delta dx dy)` |
| `custom` | 自定义 | `(custom_shape ...)` + `(options ...)` |

### 6.4 钻孔 (drill)

```
(drill diameter)                        ← 圆形钻孔
(drill oval dx dy)                      ← 椭圆钻孔
(drill dx dy)                           ← 偏心钻孔 (offset)
(drill oval diameter)                   ← 椭圆 + 偏移
```

### 6.5 焊盘层

常用层组合：

| 焊盘类型 | layers |
|---------|--------|
| SMD 顶面 | `"F.Cu" "F.Paste" "F.Mask"` |
| SMD 底面 | `"B.Cu" "B.Paste" "B.Mask"` |
| 通孔 | `"*.Cu" "*.Mask"` 或 `"F.Cu" "B.Cu"` 等 |
| 连接器 | 特定铜层 |

**注意**: `*` 通配符可匹配所有铜层。

## 7. 走线元素

### 7.1 线段 (segment)

```
(segment
  (start x y)
  (end x y)
  (width N)
  (layer "F.Cu")
  (net netcode)
  (uuid "...")
  [locked]
)
```

### 7.2 弧线 (arc) — v9+

```
(arc
  (start x y)
  (mid x y)
  (end x y)
  (width N)
  (layer "F.Cu")
  (net netcode)
  (uuid "...")
  [locked]
)
```

### 7.3 过孔 (via)

```
(via
  (at x y)
  (size diameter)
  (drill diameter)
  [layers "layer1" "layer2"]            ← v8+，盲/埋孔
  (net netcode)
  (uuid "...")
  [locked]
  [type blind|micro]                    ← v9+
  (teardrops ...)                       ← v9+
)
```

**注意**:
- v7 过孔层通过 `(layers "F.Cu" "B.Cu")` 固定为通孔
- v8+ 支持任意层对: `(layers "F.Cu" "In1.Cu")` 为盲孔
- v9+ `type` 属性: `blind` (盲孔), `micro` (微孔)

## 8. Zone 铜区格式

```
(zone
  [locked]
  (net netcode)                         ← 0 = 未分配
  (net_name "name")
  (layers "layer1" "layer2" ...)
  (uuid "...")
  [hatch edge|full|none pitch orientation]
  [priority N]
  [connect_pads yes|no (clearance N)]
  [min_thickness N]
  [filled_areas_thickness no]
  [fill yes|no (mode solid|hatch) (hatch_thickness N) (hatch_gap N) (hatch_orientation N) (hatch_smoothing_level N) (hatch_smoothing_value N) (hatch_border_algorithm hatch|thermal)]
  [keepout (tracks yes|no) (vias yes|no) (pads yes|no) (copperpour yes|no) (footprints yes|no)]
  (polygon
    (pts
      (xy x1 y1)
      (xy x2 y2)
      ...
    )
  )
  [filled_polygon ...]
  [fill_segments ...]
)
```

### 8.1 Zone 类型

| 类型 | 说明 |
|------|------|
| 铜区 | `(net N)` + `(fill yes)` |
| 禁止区 | `(keepout ...)` 或 `(keepout_tracks ...)` 等 |
| 规则区 | 特定设计规则区域 |

## 9. 网络 (Net) 系统

### 9.1 网络声明

```
(net 0 "")                              ← 空网络
(net 1 "VCC")
(net 2 "GND")
(net 3 "SDA")
...
```

**注意**: 网络 0 始终为空字符串，表示未连接。netcode 从 1 开始递增。

### 9.2 网络类 (net_class)

```
(net_class "Default"
  "This is the default net class."
  (clearance N)
  (trace_width N)
  (via_diameter N)
  (via_drill N)
  (diff_pair_width N)
  (diff_pair_gap N)
  "net1" "net2" "net3" ...              ← 归属该类的网络名列表
)
```

### 9.3 网络关联方式

- **走线/过孔**: `(net netcode)` — 使用数字编码
- **焊盘**: `(net netcode "netname")` — 编码 + 名称
- **铜区**: `(net netcode)` + `(net_name "name")` — 两者都有

## 10. 坐标与单位

### 10.1 单位

- PCB 文件中所有坐标单位为 **毫米 (mm)**
- 角度为 **度 (degrees)**
- 与原理图不同（原理图使用 mils/mm 混合）

### 10.2 坐标系

- 原点: 左上角为 `(0, 0)`
- X 轴: 向右为正
- Y 轴: **向下为正** (KiCad 屏幕/PCB 坐标)
- 旋转: 逆时针为正（0°/90°/180°/270°）

### 10.3 封装坐标变换

封装内元素的绝对坐标：

```
absolute = footprint_pos + rotate(local_pos, footprint_rotation)
```

旋转遵循标准逆时针旋转矩阵（与原理图的顺时针不同）。

## 11. kicad-json5 PCB 导出设计要点

### 11.1 最小可行导出 (MVP)

生成可被 KiCad 打开的 PCB 文件，需包含：

1. **层定义** — 标准 2 层 (F.Cu + B.Cu) 或 4 层
2. **网络声明** — 从原理图/连接关系提取
3. **Footprint 放置** — 封装 + 位置 + 旋转
4. **Pad 网络** — 每个 pad 关联正确的 netcode
5. **Board 边框** — Edge.Cuts 层的矩形或轮廓

### 11.2 JSON5 IR 扩展

PCB 导出需要在 IR 层新增：

```
PcbData
  ├── layers: Vec<LayerDef>
  ├── nets: Vec<NetDef>               ← netcode + name
  ├── footprints: Vec<PcbFootprint>
  │     ├── lib_id: String
  │     ├── position: (f64, f64, f64) ← x, y, rotation
  │     ├── layer: String
  │     ├── pads: Vec<PcbPad>
  │     │     ├── number: String
  │     │     ├── net: Option<usize>  ← netcode
  │     │     ├── pad_type: PadType
  │     │     └── shape: PadShape
  │     └── model: Option<String>
  ├── tracks: Vec<PcbTrack>
  ├── vias: Vec<PcbVia>
  ├── zones: Vec<PcbZone>
  └── board_outline: Vec<(f64, f64)>  ← Edge.Cuts 轮廓
```

### 11.3 导出流程

```
JSON5 → IR → PcbIR → S-expression Generator → .kicad_pcb
```

### 11.4 格式严格遵守

- `(at x y rotation)` — rotation 可选，默认 0
- `(pad "number" type shape ...)` — number 是字符串
- `(net netcode "name")` — pad 中使用编码+名称
- `(segment ...)` 中 `(net netcode)` — 仅编码
- 网络编码 0 保留给空网络
- 所有坐标使用毫米
- 铜层标记 `signal`，辅助层标记 `user`
- `(layers ...)` 必须在 `(setup ...)` 之前
- `(net ...)` 声明在 `(setup ...)` 之后、元素之前

### 11.5 与原理图格式的差异

| 方面 | 原理图 (.kicad_sch) | PCB (.kicad_pcb) |
|------|-------------------|-----------------|
| 坐标单位 | mm (部分 mils) | mm |
| Y 轴方向 | 向下为正 | 向下为正 |
| 旋转方向 | **顺时针** | **逆时针** |
| 网络表示 | 隐式 (wire+label) | **显式** (netcode) |
| 元件引用 | lib_id + symbol | footprint + pad |
| 层概念 | 无 | 核心概念 |
| 文件层级 | 多文件 (层次化) | 单文件 |
