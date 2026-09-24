# 布局指纹（uuid+坐标）——移植自 fr-route.sh 的 layout_dump
# SES 回导会把元件「搬回」SES 记录的旧位置；回导前后指纹不一致 = SES 与当前布局不一致 → 必须报错。
# 用法: python3 layout_dump.py <board.kicad_pcb>  （stdout = 排序后的指纹行）
import re
import sys

txt = open(sys.argv[1]).read()
rows = []
# 括号配对逐块扫描，兼容 header/property 两种序列化格式；
# 坐标数值归一：KiCad 保存会把 27.700000 重写为 27.7，文本比较会误报布局变化
for m in re.finditer(r'\(footprint [\s"\n]', txt):
    s = m.start() + 1
    d, e = 0, len(txt)
    for j in range(s, len(txt)):
        if txt[j] == '(':
            d += 1
        elif txt[j] == ')':
            d -= 1
            if d == 0:
                e = j + 1
                break
    blk = txt[s:e]
    u = re.search(r'\(uuid "([^"]+)"\)', blk)
    a = re.search(r'\(at (-?[\d.]+) (-?[\d.]+)(?: (-?[\d.]+))?\)', blk)
    if u and a:
        rows.append("%s %.4f %.4f %.1f" % (u.group(1), float(a.group(1)), float(a.group(2)), float(a.group(3) or 0)))
print('\n'.join(sorted(rows)))
