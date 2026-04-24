# kicad-render JS→Rust 移植审计报告

> 原始源码（两个互为补充的仓库）:
> - `demo/kicanvas/src/` — KiCanvas 原始项目
> - `demo/ecad-viewer/packages/ecad-viewer-app/src/` — ecad-viewer（基于 kicanvas，有少量差异）
> 两者核心渲染逻辑一致，ecad-viewer 额外有 `lib_pin.ts` 等文件
> 关键目录: `viewers/schematic/` (原理图渲染), `kicad/` (数据模型), `graphics/` (渲染器)

## 0. 原始源码文件索引

| 文件 | 对应功能 |
|------|---------|
| `viewers/schematic/painter.ts` | SchematicPainter 主调度器 + 所有子 Painter |
| `viewers/schematic/painters/base.ts` | BaseSchematicPainter, determine_stroke/fill |
| `viewers/schematic/painters/pin.ts` | PinPainter, PinShapeInternals, PinLabelInternals |
| `viewers/schematic/painters/symbol.ts` | LibSymbolPainter, SchematicSymbolPainter, get_symbol_transform |
| `viewers/schematic/painters/label.ts` | NetLabelPainter, GlobalLabelPainter, HierarchicalLabelPainter |
| `viewers/schematic/layers.ts` | LayerNames 枚举, LayerSet |
| `viewers/base/view-layers.ts` | ViewLayer, ViewLayerSet (图层渲染顺序) |
| `viewers/base/painter.ts` | DocumentPainter, ItemPainter 基类 |
| `viewers/drawing-sheet/painter.ts` | DrawingSheetPainter (边框+标题栏) |
| `kicad/theme.ts` | SchematicTheme 完整颜色定义 |
| `kicad/schematic.ts` | 原理图数据模型 |
| `kicad/drawing-sheet.ts` | DrawingSheet 数据模型 |
| `graphics/renderer.ts` | Renderer 接口 |
| `graphics/canvas2d.ts` | Canvas2D 渲染实现 |

---

## 1. 坐标系概述

### KiCad 原理图坐标系（事实标准）
- 原点 (0,0) 在纸张**左上角**
- X 向右增加，Y **向下**增加（Y-DOWN，同 SVG）
- 单位：mm
- 纸张范围：`(0,0)` → `(paper_width, paper_height)`

### KiCad 器件库坐标系
- 器件库图形使用 **Y-UP** 约定（正 Y 向上）
- Pin 角度：0°=Right, 90°=Up, 180°=Left, 270°=Down
- Pin position（at x y angle）= 引脚连接点（wire 连接端）

### SVG 坐标系
- 原点在**左上角**，Y 向下（Y-DOWN）
- 与 KiCad 原理图坐标系一致

---

## 2. JS 渲染链路（KiCanvas ecad-viewer.pc.js）

### 2.1 全局坐标变换
- **无全局 Y-flip**
- Camera 矩阵 = translation + zoom（无 Y 翻转）
- 原理图坐标直接映射到屏幕坐标

### 2.2 器件 Symbol 变换（`get_symbol_transform` / `Ah`）
```javascript
// 0°: [1,  0] = 单位阵 + Y-flip
//     [0, -1]
// 90°: [0, -1]
//      [-1, 0]
// 180°: [-1, 0]
//       [ 0, 1]
// 270°: [ 0, 1]
//       [ 1, 0]
```
- 矩阵内置 Y-flip（库 Y-UP → 原理图 Y-DOWN 转换）
- Mirror Y: 翻转 a,b 元素
- Mirror X: 翻转 c,d 元素
- 应用方式：`translation(position) * matrix` 变换库局部坐标到原理图坐标

### 2.3 Pin 变换（`apply_symbol_transformations`）
```javascript
// 1. 逐步旋转 pin 位置和方向（非矩阵，step-by-step）
// 2. Mirror pin 位置和方向
// 3. 最终位置计算：
let i = symbol_pos * (1, -1);           // (px, -py)
pin_pos = (pin_pos + i) * (1, -1);      // (px + ex, py - ey)
```
- 等效于：`translation(symbol_pos) * Y-flip * pin_local_pos`
- Pin 方向也经过旋转变换

### 2.4 Pin stem 计算（`qm.stem`）
```javascript
// n = wire连接点（外端）, e = pin长度
// up:    p0=(n.x, n.y-e),  dir=(0,1)    // p0 在 n 下方(Y-UP), dir 朝上
// down:  p0=(n.x, n.y+e),  dir=(0,-1)   // p0 在 n 上方, dir 朝下
// left:  p0=(n.x-e, n.y),  dir=(1,0)    // p0 在 n 左边, dir 朝右
// right: p0=(n.x+e, n.y),  dir=(-1,0)   // p0 在 n 右边, dir 朝左
```
- **position = 外端（wire连接点）**
- **p0 = 内端（body连接点）**
- **dir = 从 p0 指向 position 的方向**

### 2.5 Pin 图形绘制（`qm.draw`）
- line: p0 → position 直线
- inverted: 圆(circle at p0 + dir*radius) + 线(p0+dir*2*size → position)
- clock: 线 + clock_notch 三角形
- inverted_clock: 圆 + 线 + clock_notch
- clock_low/edge_clock_high: 线 + clock_notch + low_tri
- input_low: 线 + low_tri
- output_low: 线 + output_low 符号
- non_logic: 线 + X 交叉

### 2.6 Pin name/number 文字定位
```javascript
R = pin_names_offset;  // 通常 0
R > 0 → name: place_inside, number: place_above
R <= 0 → name: place_above, number: place_below
```
- `place_above/below/inside` 根据 pin 方向计算偏移
- text offset = 0.6096 * text_offset_ratio(0.15) ≈ 0.091

### 2.7 Wire 渲染
```javascript
this.gfx.line(new Re(r.pts, stroke_width, theme.wire))
```
- 直接用原理图坐标，无额外变换
- theme.wire = 绿色 #008400

### 2.8 Junction 渲染
```javascript
this.gfx.circle(new at(r.at.position, (r.diameter || 1) / 2, theme.junction))
```
- 实心圆，radius = diameter/2（默认 0.9144/2 = 0.4572mm）
- theme.junction = 黑色填充

### 2.9 Label 渲染
- **Local (NetLabelPainter/hh)**: 纯文字 + 短连接线
- **Global (GlobalLabelPainter/ph)**: 文字 + 框 + 箭头形状
- **Hierarchical (HierarchicalLabelPainter/dh)**: 文字 + 三角/箭头形状
- 颜色：`theme.label_local`, `theme.label_global`, `theme.label_hier`

### 2.10 Drawing Sheet 渲染
- 外框：`(0,0)` → `(width, height)` 的矩形
- 内框：inset 后的矩形
- Title block：右下角区域
- 直接在原理图坐标中渲染，无 Y-flip

### 2.11 白色背景
```javascript
this.ctx2d.fillStyle = this.background_color.to_css(); // white
this.ctx2d.fillRect(0, 0, canvas.width, canvas.height);
```
- Canvas 全屏白色填充，在所有内容之前

### 2.12 图层 Z-Order（原始源码 `layers.ts` + `view-layers.ts`）

**添加顺序**（front-to-back，先添加=后渲染=在上层）:
```typescript
// layers.ts constructor 中按此顺序 add:
interactive    // 最上层（添加最前）
marks          // DNP标记
symbol_field   // Reference/Value 属性文字
label          // 标签
junction       // 节点
wire           // 导线+总线
symbol_foreground  // Symbol 描边+Pin文字
notes          // 独立文本/矩形
bitmap
symbol_pin     // Pin 线段
symbol_background  // Symbol 填充
drawing_sheet  // 边框+标题栏
grid           // 网格
```

**渲染顺序**（`in_display_order()` 反转=back-to-front）:
```
grid → drawing_sheet → symbol_background → symbol_pin → bitmap
→ notes → symbol_foreground → wire → junction → label
→ symbol_field → marks → interactive → overlay
```

**对比 Rust 当前 z-index**:
```
Notes(1) < Symbol.Background(5) < Wire(10) < Bus(11) < Symbol.Pin(20)
< Symbol.Foreground(25) < Junctions(30) < Labels(35) < DrawingSheet(40)
< Grid(45) < Interactive(100)
```
⚠️ Rust 的 DrawingSheet(40) 和 Grid(45) 在最上层，但 JS 中它们在最底层

### 2.13 完整主题颜色（`kicad/theme.ts` SchematicTheme）
```typescript
interface SchematicTheme {
    background: Color;        // 白色 (canvas底色)
    wire: Color;              // 导线 #008400 绿
    bus: Color;               // 总线
    junction: Color;          // 节点 实心黑
    pin: Color;               // Pin 线段颜色
    pin_name: Color;          // Pin 名称文字颜色
    pin_number: Color;        // Pin 编号文字颜色
    reference: Color;         // Reference 文字颜色 (如 "R1")
    value: Color;             // Value 文字颜色 (如 "10k")
    fields: Color;            // 其他属性文字颜色
    label_local: Color;       // 本地标签颜色
    label_global: Color;      // 全局标签颜色
    label_hier: Color;        // 层次标签颜色
    component_body: Color;    // 器件 body 填充色
    component_outline: Color; // 器件描边色
    note: Color;              // 独立文本/图形颜色
    no_connect: Color;        // 未连接标记
    worksheet: Color;         // 图纸边框+文字颜色
    grid: Color;              // 网格
    // ... 其他
}
```

### 2.14 Drawing Sheet 渲染（`drawing-sheet/painter.ts`）

**JS 不是硬编码的！** 它使用 `DrawingSheet` 数据模型（从 `.kicad_wks` 文件解析）：
- `LinePainter`: 绘制线段，支持锚点(ltcorner/rbcorner/lbcorner/rtcorner)和重复
- `RectPainter`: 绘制矩形，同上
- `TbTextPainter`: 绘制标题栏文字，支持字号/粗体/斜体/对齐

**Rust 当前做法**：硬编码外框+内框+标题栏，不读取 `.kicad_wks`

### 2.15 Property 渲染（`painter.ts` PropertyPainter）

Reference/Value 文字的完整渲染逻辑：
```typescript
// 1. 使用 SchField 类处理属性
const schfield = new SchField(text, {
    position: parent.at.position.multiply(10000),  // symbol 位置
    transform: matrix,                             // symbol 变换矩阵
    is_symbol: true,
});

// 2. 属性位置需要逆变换
let rel_position = p.at.position.multiply(10000)
    .sub(schfield.parent.position);
rel_position = matrix.inverse().transform(rel_position);
rel_position = rel_position.add(schfield.parent.position);

// 3. 颜色选择
switch (p.name) {
    case "Reference": color = theme.reference; break;
    case "Value": color = theme.value; break;
    default: color = theme.fields;
}
```

**Rust 当前做法**：固定偏移 ±2.54mm，完全忽略 IR 中属性的位置和变换

---

## 3. Rust 渲染链路（kicad-render）

### 3.1 全局坐标变换（main.rs）⚠️ 错误
```rust
let flip_scale = Matrix::new([scale, 0.0, 0.0, -scale, 0.0, 0.0]);
svg_renderer.set_transform(&flip_scale);
```
- **使用了全局 Y-flip**：`(x,y) → (x*scale, -y*scale)`
- JS 没有全局 Y-flip

### 3.2 器件 Symbol 变换（symbol_painter.rs）⚠️ 错误
```rust
pub fn transform(&self) -> Matrix {
    let mut matrix = Matrix::translation(self.symbol.position.x, self.symbol.position.y);
    matrix = matrix.multiply(&Matrix::rotation(rotation_rad));  // 标准逆时针旋转
    // mirror...
    matrix
}
```
- **使用 `Matrix::rotation()`**，不含 Y-flip
- JS 使用 `get_symbol_transform()`，内置 Y-flip

### 3.3 get_symbol_transform 函数 ✅ 已实现但未使用
```rust
pub fn get_symbol_transform(rotation: i32, mirror: &Mirror) -> Matrix {
    let (a, b, c, d) = match rotation % 360 {
        0 => (1.0, 0.0, 0.0, -1.0),   // Y-flip ✓
        90 => (0.0, -1.0, -1.0, 0.0),  // ✓
        180 => (-1.0, 0.0, 0.0, 1.0),  // ✓
        270 => (0.0, 1.0, 1.0, 0.0),   // ✓
        _ => (1.0, 0.0, 0.0, -1.0),
    };
    // mirror handling...
}
```
- 逻辑与 JS 一致
- 但 **未被 symbol_painter.transform() 调用**

### 3.4 Pin 方向向量（pin_painter.rs）⚠️ 错误
```rust
pub fn direction(&self) -> Point {
    match self {
        Right => (1.0, 0.0),   // ✓
        Up    => (0.0, -1.0),  // ✗ JS 是 (0, 1)
        Left  => (-1.0, 0.0),  // ✓
        Down  => (0.0, 1.0),   // ✗ JS 是 (0, -1)
    }
}
```
- 当前使用 Y-DOWN 约定，但库坐标系是 Y-UP
- 需要与库坐标系一致：Up=(0,1), Down=(0,-1)

### 3.5 Pin position 约定 ⚠️ 差异
- **Rust**: `position` = 库的 `(at x y angle)` = **body端**（内端）
- **JS**: `n` (position) = **wire连接点**（外端），`p0` = body端
- Rust 的 `end_position()` 计算外端 = position + direction * length

### 3.6 Wire 渲染 ✅ 正确
- 坐标直接从 IR 传入，无变换
- 颜色正确：#008400

### 3.7 Junction 渲染 ✅ 正确
- 实心圆，使用 diameter 字段
- 颜色：黑色填充

### 3.8 Label 渲染 ⚠️ 简化
- JS 的 Label 有复杂的形状（框+箭头+三角）
- Rust 只有简单的箭头形状
- 文字定位简化处理
- **Label shape 旋转使用 cos/sin，未考虑 Y-DOWN 坐标系**

### 3.9 Drawing Sheet 渲染 ⚠️ 被全局 Y-flip 影响
- 渲染逻辑正确（外框+内框+标题栏）
- 但经过全局 Y-flip 后位置错误：标题栏翻到右上角

### 3.10 白色背景 ⚠️ viewBox 负坐标
- SVG `<rect>` 在 viewBox 正常时正确
- 当前 viewBox 使用负 Y 坐标，可能渲染异常

---

## 4. 错误汇总与根因分析

### 错误 1：全局 Y-flip 导致 Drawing Sheet 位置错误
**根因**：main.rs 使用 `Matrix([s,0,0,-s,0,0])` 全局翻转 Y 轴
**现象**：标题栏出现在 SVG 顶部而非底部，白色背景可能显示异常
**JS 做法**：无全局 Y-flip，器件级 Y-flip 在 get_symbol_transform 中处理

### 错误 2：Symbol transform 缺少 Y-flip
**根因**：symbol_painter.transform() 用 Matrix::rotation() 而非 get_symbol_transform()
**现象**：当前被全局 Y-flip 掩盖；去掉全局 Y-flip 后器件方向会错
**修复**：使用 get_symbol_transform() 替换 Matrix::rotation()

### 错误 3：Pin 方向向量使用 Y-DOWN 而非 Y-UP
**根因**：库坐标系是 Y-UP，但 direction() 使用 Y-DOWN 约定
**现象**：当前被全局 Y-flip 双重翻转掩盖；修复后 pin 方向会反
**修复**：Up→(0,1), Down→(0,-1)

### 错误 4：Pin position 是 body 端，JS 的 position 是 wire 端
**根因**：KiCad IR 的 pin.at 是 body 端，JS stem() 的 n 是 wire 端
**现象**：pin 线段方向与 JS 一致（body→wire），但 wire 端/端点标记位置可能不同
**影响**：目前影响不大，因为 wire 连接点通过 end_position() 正确计算

### 错误 5：Label shape 旋转未适配 Y-DOWN
**根因**：shape 偏移使用 cos/sin 计算，在 Y-DOWN 中 Y 分量需取反
**现象**：Global/Hierarchical label 箭头方向可能不对
**修复**：Y 偏移量取反，或在绘制时考虑坐标系

### 错误 6：Drawing Sheet 背景色渲染
**根因**：viewBox 使用负 Y 范围，白色 `<rect>` 可能不被正确显示
**现象**：用户看到黑色背景
**修复**：移除全局 Y-flip 后 viewBox 全部为正坐标

---

## 5. 修复方案

### Step 1: main.rs — 移除全局 Y-flip
```rust
// 改前: Matrix::new([scale, 0.0, 0.0, -scale, 0.0, 0.0])
// 改后: Matrix::new([scale, 0.0, 0.0, scale, 0.0, 0.0])

// viewBox 改为正值:
let view_x = 0.0;
let view_y = 0.0;
let view_w = paper_w * scale;
let view_h = paper_h * scale;
```

### Step 2: symbol_painter.rs — 使用 get_symbol_transform
```rust
pub fn transform(&self) -> Matrix {
    let rot_mirror = get_symbol_transform(self.symbol.rotation, &self.symbol.mirror);
    let translation = Matrix::translation(self.symbol.position.x, self.symbol.position.y);
    translation.multiply(&rot_mirror)
}
```

### Step 3: pin_painter.rs — 方向向量改为 Y-UP
```rust
Right => (1.0, 0.0),   // 不变
Up    => (0.0, 1.0),    // Y-UP: 正 Y 向上
Left  => (-1.0, 0.0),   // 不变
Down  => (0.0, -1.0),   // Y-UP: 负 Y 向下
```

### Step 4: 验证
- 编译通过
- 生成 SVG，确认：
  - 白色背景覆盖全页
  - 标题栏在右下角
  - 器件方向正确
  - Pin 指向正确（与 wire 对齐）
  - 导线位置正确
  - Junction 在正确位置

---

## 6. IR→SVG 数据映射审计

### 6.1 IR 类型全览

| IR 类型 | 关键字段 | 是否被 bridge.rs 使用 | 遗漏字段 |
|---------|---------|---------------------|----------|
| `Wire` | start, end, stroke | start, end ✅ stroke ❌ | stroke.width 被忽略 |
| `Junction` | position, diameter | 两者 ✅ | — |
| `Label` | text, position, label_type, shape, **effects** | 前4者 ✅ effects ❌ | effects.font.size 被硬编码 |
| `SymbolInstance` | lib_id, position, mirror, unit, reference, value, **properties_ext** | 前6者 ✅ properties_ext ❌ | 属性位置/字体未使用 |
| `Symbol` (库) | graphics, units, pin_names_hidden, **pin_name_offset**, properties | 前3者 ✅ 后两者 ❌ | pin_name_offset 影响文字位置 |
| `PinGraphic` | position, length, shape, pin_type, name, number, **name_effects**, **number_effects** | 前6者 ✅ 后两者 ❌ | 引脚文字字号未使用 |
| `Arc` | start, mid, end, stroke, fill | 全部 ✅ (via calculate_arc_params) | — |
| `Circle` | center, radius, stroke, fill | 全部 ✅ | — |
| `Rectangle` | start, end, stroke, fill | 全部 ✅ | — |
| `Polyline` | points, stroke, fill | 全部 ✅ | — |
| `Text` | text, position, **effects** | text, position ✅ effects 部分 | effects.font.size 被部分使用 |
| `Property` | name, value, position, effects, hide | — (未直接使用) | 器件属性位置完全未使用 |
| `NoConnect` | position | 未渲染 | 整个类型未实现 |
| `Bus` | points, stroke | 未渲染 | 整个类型未实现 |
| `BusEntry` | position, size | 未渲染 | 整个类型未实现 |

### 6.2 IR Stroke 颜色缺失问题

**IR `Stroke` 结构体（graphic.rs）**：
```rust
pub struct Stroke {
    pub width: f64,
    pub stroke_type: StrokeType,
    // ⚠️ 没有 color 字段！
}
```

**JS stroke 颜色决定逻辑**（`determine_stroke`）：
```javascript
// 1. 优先使用 IR 的 stroke.color（如果存在）
// 2. 如果没有，按图层选择主题色：
//    - Symbol:Foreground → theme.component_outline
//    - 其他图层 → theme.note
let color = e.stroke?.color ?? theme_for_layer;
```

**Rust stroke 颜色（bridge.rs `convert_stroke`）**：
```rust
pub fn convert_stroke(s: &ir::Stroke) -> Stroke {
    let width = if s.width > 0.0 { s.width } else { constants::LINE_WIDTH };
    Stroke { width, color: Color::black(), style }  // ⚠️ 硬编码黑色
}
```

**差异**：
- IR Stroke 没有 color 字段 → 这是 kicad-json5 解析器的遗漏
- JS 能从 IR 获取 stroke.color → JS 的解析器可能更完整
- Rust 硬编码黑色 → 对于 symbol body 应该用 `theme.component_outline`（深红 #840000）
- **修复**：按图层类型选择默认颜色，或在 IR 中补全 stroke.color

### 6.3 IR Fill 颜色映射

**JS fill 颜色决定逻辑**（`determine_fill`）：
```javascript
switch (fill_type) {
    case "none":       → null（不填充）
    case "background": → theme.component_body  // 淡黄色
    case "outline":    → theme.component_outline // 深红色（仅描边）
    case "color":      → e.fill.color  // 使用 IR 指定的颜色
}
```

**Rust fill 颜色（bridge.rs `convert_fill`）**：
```rust
match f.fill_type {
    FillType::None | FillType::Outline → Fill::none(),
    FillType::Background → Fill::solid(Color::from_rgb(255, 255, 238)),  // #ffffee 硬编码
    FillType::Color → { /* 使用 f.color */ }
}
```

**差异**：
- `Outline` 填充类型：JS 用 `component_outline` 色填充，Rust 不填充 ⚠️
- `Background` 填充颜色硬编码一致 ✅（但应从 theme 读取）
- `Color` 类型使用 IR 颜色 ✅

### 6.4 Reference/Value 属性位置

**JS 做法**：
- 从 `SymbolInstance.properties_ext` 读取每个属性的 `position` 和 `effects`
- 属性的位置和字体直接来自 `.kicad_sch` 文件中的 property 定义
- 属性经过 symbol transform（旋转、镜像、Y-flip）后渲染

**Rust 做法**：
- `convert_symbol()` 只读取 `component.reference` 和 `component.value` 字符串
- Reference 固定在 symbol position 上方 2.54mm
- Value 固定在 symbol position 下方 2.54mm
- **完全忽略了 IR 中属性的精确位置和字体设置**

**差异影响**：
- Reference/Value 位置不精确（固定偏移 vs 实际位置）
- 字体大小可能不对（硬编码 vs IR 中定义的字号）
- 属性旋转被忽略

### 6.5 Pin 文字字号

**JS 做法**：
```javascript
// 使用 IR 的 pin name/number effects
y && _n.draw(e, o, r.position, y, i.name.effects, e.state.stroke)
V && _n.draw(e, c, r.position, V, i.number.effects, e.state.stroke)
```

**Rust 做法**：
```rust
// PinGraphic 中没有 name_effects/number_effects 字段
// paint_pin_name / paint_pin_number 使用硬编码 font_size: 1.27
```

**差异**：pin 文字字号来自 IR 的 `PinGraphic.name_effects.font.size`，Rust 硬编码 1.27mm

### 6.6 Label 字体字号

**JS 做法**：从 IR `Label.effects.font.size` 读取

**Rust 做法**（bridge.rs）：
```rust
Label {
    font_size: 1.27,  // TEXT_SIZE default — 硬编码
}
```

**差异**：IR `Label.effects.font.size` 被忽略，使用硬编码值

### 6.7 Label shape 渲染细节

**JS GlobalLabel shape（完整点列表）**：
```javascript
// 输入型：右侧突出的箭头
// [起点, 上角, 右上角, 右下角, 下角, 终点]
// 输出型：左侧突出的箭头
// 双向型：两侧突出
```

**JS HierarchicalLabel shape**：
```javascript
// 输出：右向三角 (5点)
// 输入：左向三角 (6点)
// 双向：菱形 (5点)
// 被动：矩形 (6点)
```

**Rust 做法**：
- 只有简单的 V 形箭头（Input/Output/Bidirectional 共用近似逻辑）
- 无完整的框+箭头形状
- **差异较大**，但不影响功能识别

### 6.8 Text 图形元素旋转

**JS 做法**：
```javascript
// 根据方向设置文字角度
switch (orientation) {
    case "up": case "down": text_angle = 90°; break;
    case "left": case "right": text_angle = 0°; break;
}
```

**Rust 做法**（symbol_painter.rs `paint_body`）：
```rust
GraphicElement::Text(ir_text) => {
    let pos = transform.transform(&Point::new(ir_text.position.0, ir_text.position.1));
    // ⚠️ 文字的旋转角度 ir_text.position.2 被忽略
    // ⚠️ effects.font.bold/italic 被忽略
}
```

**差异**：Text 元素的旋转角度和字体样式未使用

### 6.9 SVG 输出特性

| SVG 特性 | 当前状态 | 备注 |
|---------|---------|------|
| 基本图形（circle/polyline/polygon/path） | ✅ | — |
| 文字渲染（含 XML 转义） | ✅ | — |
| 上标/下标/上划线（tspan） | ✅ | `^{}`/`_{}`/`~{}` 标记 |
| Stroke 缩放 | ✅ | width * scale_factor |
| Font size 缩放 | ✅ | font_size * scale_factor |
| Dash 样式 | ✅ | stroke-dasharray |
| Arc → SVG path（A 命令） | ✅ | large-arc/sweep flags |
| Bezier → SVG path（C 命令） | ✅ | — |
| `<g>` 分组 | ❌ | 所有元素平铺 |
| text-anchor / 对齐 | ❌ | 文字始终左对齐 |
| font-weight / font-style | ❌ | bold/italic 未输出 |
| stroke-linecap/linejoin | ❌ | 未设置 |
| opacity | ❌ | 未支持 |
| 文字旋转 | ❌ | SVG `<text>` 无 transform |

### 6.10 Arc 三点→圆心转换

**IR 存储**：`start(x,y)`, `mid(x,y)`, `end(x,y)` 三个点

**`calculate_arc_params()` 算法**：
1. 用两条弦的垂直平分线交点求圆心 `(cx, cy)`
2. 半径 = 圆心到任一点的距离
3. 起始角 = atan2(y1-cy, x1-cx)
4. 终止角 = atan2(y3-cy, x3-cx)

**⚠️ 潜在问题**：
- 中间点 `mid` 仅用于求圆心，**不用于判断弧的走向**
- 当弧跨越 ±π 边界时，start_angle 和 end_angle 的差值可能不正确
- 需要验证：大弧 vs 小弧、顺时针 vs 逆时针

---

## 7. IR 数据映射缺失汇总

### 高优先级（影响渲染正确性）

| # | 遗漏 | 影响 | 修复建议 |
|---|------|------|---------|
| 1 | Stroke 无 color 字段 | symbol body 线条全黑，应为深红 | IR 补 color 或按图层选默认色 |
| 2 | convert_stroke 硬编码黑色 | 所有 symbol body 线条颜色错误 | 按 `outline_color` 传参 |
| 3 | Reference/Value 位置固定偏移 | 属性位置与 KiCad 不一致 | 使用 properties_ext 的位置 |
| 4 | 全局 Y-flip | drawing sheet 错位+背景异常 | 见第5节修复方案 |

### 中优先级（影响渲染精度）

| # | 遗漏 | 影响 | 修复建议 |
|---|------|------|---------|
| 5 | Label effects.font.size 被忽略 | 字号固定 1.27mm | 使用 IR 的 font.size |
| 6 | Pin name/number effects 被忽略 | 引脚文字字号固定 | 使用 IR 的 effects |
| 7 | Text 元素旋转被忽略 | symbol body 文字方向可能不对 | 使用 position.2 作为旋转角 |
| 8 | Fill Outline 类型处理不同 | JS 填充 outline 色，Rust 不填充 | 对齐 JS 逻辑 |

### 低优先级（不影响基本功能）

| # | 遗漏 | 影响 | 修复建议 |
|---|------|------|---------|
| 9 | font bold/italic 未输出 | 文字样式缺失 | SVG 添加 font-weight/font-style |
| 10 | text-anchor 未设置 | 文字对齐不精确 | 根据 justify 设置 text-anchor |
| 11 | Label shape 简化 | global/hierarchical 标签形状不完整 | 移植 JS 完整形状 |
| 12 | NoConnect/Bus/BusEntry 未实现 | 部分图形缺失 | 后续移植 |

---

## 8. 后续优化（非阻塞）

| 项目 | 当前状态 | JS 做法 | 优先级 |
|------|---------|---------|--------|
| Pin shape 详细渲染 | 只有 Line/Dot/Clock | 8种完整形状 | 中 |
| Label 形状（Global/Hierarchical） | 简化箭头 | 完整框+箭头 | 中 |
| Pin name/number 精确定位 | 简化偏移 | place_above/below/inside | 低 |
| Bus 渲染 | 未实现 | BusPainter | 低 |
| NoConnect 渲染 | 未实现 | NoConnectPainter | 低 |
| BusEntry 渲染 | 未实现 | BusEntryPainter | 低 |
