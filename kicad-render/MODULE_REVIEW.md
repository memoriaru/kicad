# KiCad Renderer 模块核对报告

本文档记录 Rust 移植代码与原始 JS 实现 (ecad-viewer.pc.js) 的对比核对。

## 1. Color 模块 (`render_core/color.rs`)

**JS 类名**: `N`

### 结构对比

| 特性 | JS | Rust | 状态 |
|------|-----|------|------|
| RGBA 存储 | `this.r, this.g, this.b, this.a` | `pub r, g, b, a: f64` | ✅ |
| 值范围 | 0-1 | 0-1 | ✅ |
| `new(r,g,b)` | ✅ | `new(r,g,b)` | ✅ |
| `with_alpha()` | ✅ | `with_alpha(r,g,b,a)` | ✅ |
| `from_rgb()` | ✅ | `from_rgb(r,g,b)` 0-255 | ✅ |
| `to_css()` | 返回 hex 或 rgba | `to_css()` 返回 hex 或 rgba | ✅ |
| `copy()` | ✅ | `copy()` | ✅ |
| `grayscale()` | ✅ | `grayscale()` | ✅ |
| `mix()` | ✅ | `mix(other, ratio)` | ✅ |
| `set_alpha()` | ✅ | `set_alpha(a)` | ✅ |

### 预定义颜色

| 颜色 | JS | Rust | 状态 |
|------|-----|------|------|
| black | ✅ | `black()` | ✅ |
| white | ✅ | `white()` | ✅ |
| red | ✅ | `red()` | ✅ |
| green | ✅ | `green()` | ✅ |
| blue | ✅ | `blue()` | ✅ |
| yellow | ✅ | `yellow()` | ✅ |
| cyan (Ref/Value) | `#51FF9F` | `cyan()` = rgb(81,255,159) | ✅ |
| dark_green (wire) | `#008400` | `dark_green()` | ✅ |

---

## 2. Point 模块 (`render_core/types.rs`)

**JS 类名**: `v`

### 结构对比

| 特性 | JS | Rust | 状态 |
|------|-----|------|------|
| x, y 存储 | `this.x, this.y` | `pub x, y: f64` | ✅ |
| `new(x,y)` | ✅ | `new(x,y)` | ✅ |
| `zero()` | ✅ | `zero()` | ✅ |
| `set(x,y)` | ✅ | `set(x,y)` | ✅ |
| `copy()` | ✅ | `copy()` | ✅ |
| `add()` | ✅ | `add(&Point)` + 运算符重载 | ✅ |
| `sub()` | ✅ | `sub(&Point)` + 运算符重载 | ✅ |
| `mul(scalar)` | ✅ | `mul(scalar)` + 运算符重载 | ✅ |
| `multiply(p)` | 元素乘 | `multiply(&Point)` | ✅ |
| `dot()` | ✅ | `dot(&Point)` | ✅ |
| `length()` | ✅ | `length()` | ✅ |
| `length_sq()` | ✅ | `length_sq()` | ✅ |
| `normalize()` | ✅ | `normalize()` | ✅ |
| `rotate(angle)` | ✅ | `rotate(angle)` | ✅ |
| `rotate_around()` | ✅ | `rotate_around(center, angle)` | ✅ |
| `distance_to()` | ✅ | `distance_to(&Point)` | ✅ |

---

## 3. Angle 模块 (`render_core/types.rs`)

**JS 类名**: `re`

### 结构对比

| 特性 | JS | Rust | 状态 |
|------|-----|------|------|
| radians 存储 | `this.radians` | `pub radians: f64` | ✅ |
| `new(radians)` | ✅ | `new(radians)` | ✅ |
| `from_degrees()` | ✅ | `from_degrees(degrees)` | ✅ |
| `degrees()` | ✅ | `degrees()` | ✅ |
| `is_horizontal()` | ✅ | `is_horizontal()` | ✅ |
| `normalize()` | ✅ | `normalize()` | ✅ |
| `rotate_point()` | ✅ | `rotate_point(&Point)` | ✅ |
| `rotate_point_around()` | ✅ | `rotate_point_around()` | ✅ |

### AngleExt trait
为 f64 添加 `normalize_angle()` 方法，确保角度在 [0, 2π) 范围内。

---

## 4. Matrix 模块 (`render_core/matrix.rs`)

**JS 类名**: `fe`

### 矩阵格式
```
| a  c  e |
| b  d  f |
| 0  0  1 |
```
对应 `[a, b, c, d, e, f]`

### 结构对比

| 特性 | JS | Rust | 状态 |
|------|-----|------|------|
| elements 数组 | `[a,b,c,d,e,f]` | `[f64; 6]` | ✅ |
| `identity()` | ✅ | `identity()` | ✅ |
| `translation(tx,ty)` | ✅ | `translation(tx,ty)` | ✅ |
| `scaling(sx,sy)` | ✅ | `scaling(sx,sy)` | ✅ |
| `uniform_scaling(s)` | ✅ | `uniform_scaling(s)` | ✅ |
| `rotation(angle)` | ✅ | `rotation(angle)` | ✅ |
| `rotation_around()` | ✅ | `rotation_around(angle, center)` | ✅ |
| `copy()` | ✅ | `copy()` | ✅ |
| `transform(point)` | ✅ | `transform(&Point)` | ✅ |
| `multiply(other)` | ✅ | `multiply(&Matrix)` | ✅ |
| `multiply_self()` | ✅ | `multiply_self(&Matrix)` | ✅ |
| `pre_multiply()` | ✅ | `pre_multiply(&Matrix)` | ✅ |
| `determinant()` | ✅ | `determinant()` | ✅ |
| `inverse()` | ✅ | `inverse()` | ✅ |
| `translate_self()` | ✅ | `translate_self(tx,ty)` | ✅ |
| `scale_self()` | ✅ | `scale_self(sx,sy)` | ✅ |
| `rotate_self()` | ✅ | `rotate_self(angle)` | ✅ |
| `to_svg_matrix()` | ✅ | `to_svg_matrix()` | ✅ |
| `get_rotation()` | ✅ | `get_rotation()` / `rotation_angle()` | ✅ |
| `get_scale()` | ✅ | `get_scale()` | ✅ |
| `get_translation()` | ✅ | `get_translation()` | ✅ |
| `scale_factor()` | ✅ | `scale_factor()` | ✅ |
| `is_identity()` | ✅ | `is_identity()` | ✅ |

---

## 5. BoundingBox 模块 (`render_core/bbox.rs`)

**JS 类名**: `ne`

### 结构对比

| 特性 | JS | Rust | 状态 |
|------|-----|------|------|
| x, y, w, h 存储 | `this.x,y,w,h` | `pub x,y,w,h: f64` | ✅ |
| `new(x,y,w,h)` | ✅ | `new(x,y,w,h)` | ✅ |
| `empty()` | ✅ | `empty()` 使用 NaN 标记 | ✅ |
| `from_min_max()` | ✅ | `from_min_max()` | ✅ |
| `from_points()` | ✅ | `from_points()` | ✅ |
| `is_empty()` | ✅ | `is_empty()` | ✅ |
| `expand_point()` | ✅ | `expand_point(x,y)` | ✅ |
| `expand()` | ✅ | `expand(&BoundingBox)` | ✅ |
| `min_x/max_x/min_y/max_y()` | ✅ | ✅ | ✅ |
| `center()` | ✅ | `center()` | ✅ |
| `width()/height()` | ✅ | `width()` / `height()` | ✅ |
| `with_padding()` | ✅ | `with_padding(padding)` | ✅ |

### 修复的问题
- `empty()` 使用 `NaN` 而非 `f64::MAX/MIN`，避免首次扩展时的逻辑错误

---

## 6. Graphics 基元 (`render_core/graphics.rs`)

### Stroke 结构

| 特性 | JS | Rust | 状态 |
|------|-----|------|------|
| width | ✅ | `width: f64` | ✅ |
| color | ✅ | `color: Color` | ✅ |
| style (solid/dash/dot) | ✅ | `style: StrokeStyle` | ✅ |

### Fill 结构

| 特性 | JS | Rust | 状态 |
|------|-----|------|------|
| `none()` | ✅ | `none()` | ✅ |
| `solid(color)` | ✅ | `solid(Color)` | ✅ |

### Circle 基元

| 特性 | JS | Rust | 状态 |
|------|-----|------|------|
| center, radius | ✅ | `center: Point, radius: f64` | ✅ |
| fill | ✅ | `fill: Fill` | ✅ |
| stroke | ✅ | `stroke: Option<Stroke>` | ✅ |
| `transform()` | ✅ | `transform(&Matrix)` | ✅ |
| `bbox()` | ✅ | `bbox()` | ✅ |

### Arc 基元

| 特性 | JS | Rust | 状态 |
|------|-----|------|------|
| center, radius | ✅ | ✅ | ✅ |
| start_angle, end_angle | ✅ | 弧度制 | ✅ |
| stroke | ✅ | `stroke: Stroke` | ✅ |
| `transform()` | ✅ | `transform(&Matrix)` | ✅ |
| `start_point()` | ✅ | `start_point()` | ✅ |
| `end_point()` | ✅ | `end_point()` | ✅ |
| `bbox()` | ✅ | `bbox()` | ✅ |

### Polyline 基元

| 特性 | JS | Rust | 状态 |
|------|-----|------|------|
| points[] | ✅ | `points: Vec<Point>` | ✅ |
| stroke | ✅ | `stroke: Stroke` | ✅ |
| `transform()` | ✅ | `transform(&Matrix)` | ✅ |
| `bbox()` | ✅ | `bbox()` | ✅ |
| `length()` | ✅ | `length()` | ✅ |

### Polygon 基元

| 特性 | JS | Rust | 状态 |
|------|-----|------|------|
| points[] | ✅ | `points: Vec<Point>` | ✅ |
| fill | ✅ | `fill: Fill` | ✅ |
| stroke | ✅ | `stroke: Option<Stroke>` | ✅ |
| `transform()` | ✅ | `transform(&Matrix)` | ✅ |
| `bbox()` | ✅ | `bbox()` | ✅ |
| `is_closed()` | ✅ | `is_closed()` | ✅ |

### Bezier 基元

| 特性 | JS | Rust | 状态 |
|------|-----|------|------|
| start, control1, control2, end | ✅ | ✅ | ✅ |
| stroke | ✅ | `stroke: Stroke` | ✅ |
| `transform()` | ✅ | `transform(&Matrix)` | ✅ |
| `bbox()` | ✅ | `bbox()` | ✅ |
| `to_svg_path()` | ✅ | `to_svg_path()` | ✅ |

---

## 7. Layer 系统 (`layer/mod.rs`)

### LayerId

| 图层 | Z-Index | JS | Rust |
|------|---------|-----|------|
| Grid | 45 | ✅ | `grid()` |
| DrawingSheet | 40 | ✅ | `drawing_sheet()` |
| Notes | 25 | ✅ | `notes()` |
| Wire | 10 | ✅ | `wire()` |
| Junctions | 35 | ✅ | `junctions()` |
| Labels | 30 | ✅ | `labels()` |
| Symbol.Background | 5 | ✅ | `symbol_background()` |
| Symbol.Pin | 20 | ✅ | `symbol_pin()` |
| Symbol.Foreground | 15 | ✅ | `symbol_foreground()` |
| Interactive | 100 | ✅ | `interactive()` |

### LayerElement

| 类型 | JS | Rust | 状态 |
|------|-----|------|------|
| Circle | ✅ | `LayerElementType::Circle` | ✅ |
| Arc | ✅ | `LayerElementType::Arc` | ✅ |
| Polyline | ✅ | `LayerElementType::Polyline` | ✅ |
| Polygon | ✅ | `LayerElementType::Polygon` | ✅ |
| Bezier | ✅ | `LayerElementType::Bezier` | ✅ |
| Text | ✅ | `LayerElementType::Text` | ✅ |

### LayerSet

| 方法 | JS | Rust | 状态 |
|------|-----|------|------|
| `add_layer()` | ✅ | `add_layer(LayerId)` | ✅ |
| `get_layer()` | ✅ | `get_layer(&LayerId)` | ✅ |
| `get_layer_mut()` | ✅ | `get_layer_mut(&LayerId)` | ✅ |
| `render()` | 按 z-order | `render(&mut dyn Renderer)` | ✅ |

---

## 8. Renderer trait 和 SvgRenderer (`renderer/mod.rs`, `renderer/svg.rs`)

### Renderer trait

| 方法 | JS | Rust | 状态 |
|------|-----|------|------|
| `draw_circle()` | ✅ | `draw_circle(&Circle)` | ✅ |
| `draw_arc()` | ✅ | `draw_arc(&Arc)` | ✅ |
| `draw_polyline()` | ✅ | `draw_polyline(&Polyline)` | ✅ |
| `draw_polygon()` | ✅ | `draw_polygon(&Polygon)` | ✅ |
| `draw_bezier()` | ✅ | `draw_bezier(&Bezier)` | ✅ |
| `draw_text()` | ✅ | `draw_text(&Point, &str, f64, &Color)` | ✅ |
| `draw_line()` | 便捷方法 | `draw_line()` | ✅ |
| `draw_rect()` | 便捷方法 | `draw_rect()` | ✅ |
| `set_transform()` | ✅ | `set_transform(&Matrix)` | ✅ |
| `save()/restore()` | ✅ | `restore()` | ⚠️ 缺少 save |

### SvgRenderer

| 特性 | JS | Rust | 状态 |
|------|-----|------|------|
| 输出缓冲 | ✅ | `output: String` | ✅ |
| 变换栈 | ✅ | `transform_stack: Vec<Matrix>` | ✅ |
| `output()` | ✅ | `output()` | ✅ |
| 颜色转换 | ✅ | `color_to_svg()` | ✅ |
| Stroke 属性 | ✅ | `stroke_to_attrs()` | ✅ |
| Fill 属性 | ✅ | `fill_to_attrs()` | ✅ |

---

## 9. Painter 模块 (`painter/*.rs`)

### Painter trait

| 方法 | JS | Rust | 状态 |
|------|-----|------|------|
| `layers()` | 返回图层列表 | `layers() -> Vec<LayerId>` | ✅ |
| `bbox()` | 返回边界框 | `bbox() -> BoundingBox` | ✅ |
| `paint()` | 绘制到图层 | `paint(&mut LayerSet)` | ✅ |

### WirePainter

| 特性 | JS | Rust | 状态 |
|------|-----|------|------|
| WireSegment | start, end | `WireSegment` struct | ✅ |
| 颜色 | wire color | `color: Color` | ✅ |
| 宽度 | 0.1524mm (6 mils) | `width: 0.1524` | ✅ |

### JunctionPainter

| 特性 | JS | Rust | 状态 |
|------|-----|------|------|
| 位置 | ✅ | `position: Point` | ✅ |
| 直径 | 1.016mm (40 mils) | `diameter: 1.016` | ✅ |
| 填充圆 | ✅ | `Circle` with fill | ✅ |

### PinPainter

| 特性 | JS | Rust | 状态 |
|------|-----|------|------|
| PinGraphic | position, rotation, length | ✅ | ✅ |
| PinType | input/output/bidirectional/etc | ✅ | ✅ |
| PinShape | line/dot/clock/etc | ✅ | ✅ |
| PinOrientation | right/up/left/down | ✅ | ✅ |
| 变换矩阵 | symbol transform | `transform: Matrix` | ✅ |
| 绘制 pin body | ✅ | `paint_pin_body()` | ✅ |
| 绘制 pin shape | ✅ | `paint_pin_shape()` | ✅ |
| 绘制 pin name | ✅ | `paint_pin_name()` | ✅ |
| 绘制 pin number | ✅ | `paint_pin_number()` | ✅ |

### LabelPainter

| 特性 | JS | Rust | 状态 |
|------|-----|------|------|
| LabelType | Local/Global/Hierarchical | ✅ | ✅ |
| LabelShape | Input/Output/Bidirectional/etc | ✅ | ✅ |
| 文本渲染 | ✅ | `paint_label_text()` | ✅ |
| 形状渲染 | ✅ | `paint_label_shape()` | ✅ |

---

## 待改进项

1. **Renderer trait**: 添加 `save()` 方法以完整支持状态栈
2. **文本渲染**: 添加 KiCad 标记语法支持 (`^{}`, `_{}`, `~{}`)
3. **字体**: 实现矢量字体渲染 (StrokeGlyph)
4. **WASM**: 添加 Canvas 2D 后端支持

---

## 测试覆盖

- 单元测试: 所有核心模块都有测试
- 集成测试: `tests/svg_render_test.rs` 包含 9 个测试
- 输出验证: 生成有效 SVG 文件
