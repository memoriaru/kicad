# 净化 DSN 非 ASCII 字符（FR 对 PN 里 Ω/μ 弹警告且不进布线）——移植自 fr-route.sh
# 用法: python3 dsn_sanitize.py <file.dsn>
import re
import sys

path = sys.argv[1]
txt = open(path).read()


def fix(val):
    if all(ord(c) < 128 for c in val):
        return val
    val = val.replace('Ω', 'Ohm').replace('μ', 'u').replace('µ', 'u')
    val = re.sub(r'[^\x00-\x7F]+', '', val)
    return val


txt, n1 = re.subn(r'\(PN "([^"]*)"\)',
                  lambda m: '(PN "%s")' % fix(m.group(1)), txt)
txt, n2 = re.subn(r'\(PN ([^")\s]+)\)',
                  lambda m: '(PN %s)' % fix(m.group(1)), txt)
open(path, 'w').write(txt)
print("[dsn-sanitize] 净化 PN 字段 %d 处（引号 %d + 裸值 %d）" % (n1 + n2, n1, n2))

# ---- 边缘 keepout 注入(一块实板 实证: FR 不认 KiCad board edge clearance
# 0.5 规则, 全局解 12 条贴边)。口径: keepout=禁布线**环带**(非内缩全板!):
#   外框(boundary path)→ 内缩 m 的四边条; 孔/无铜矩形(KiCad 已导出
#   keepout "" polygon signal)→ 外扩 m 的四边条。语法照抄 KiCad 原生
#   keepout(polygon signal 0 + 空格坐标对 + 首尾重复点), 插 structure 段
#   boundary 之后。坐标数值=DSN 原单位(KiCad 导出 1000/mm), argv[2] 同制。
#   注: 脚本经 include_str! 烘焙进 kroute-server 二进制, 改后必须重编译 serve) ----
try:
    EDGE_MARGIN = float(sys.argv[2]) if len(sys.argv) > 2 else 500.0

    def rect_of(body):
        nums = [float(v) for v in re.findall(r'-?\d+(?:\.\d+)?', body)]
        xs, ys = nums[0::2], nums[1::2]
        return (min(xs), min(ys), max(xs), max(ys))

    rects = []   # (rect, 'outer'|'hole')
    mb = re.search(r'\(boundary\s*\(path\s+\S+\s+[\d.]+\s+([^()]*)\)', txt)
    if mb:
        rects.append((rect_of(mb.group(1)), 'outer'))
    for mk in re.finditer(r'\(keepout\s+""\s*\(polygon\s+signal\s+[\d.]+\s+([^()]*)\)\)', txt):
        rects.append((rect_of(mk.group(1)), 'hole'))

    def strip(name, x0, y0, x1, y1, out):
        return ('(keepout "%s" (polygon signal 0  %g %g %g %g %g %g %g %g %g %g))'
                % (name, x0, y0, x1, y0, x1, y1, x0, y1, x0, y0))

    ko_lines, n = [], 0
    for (x0, y0, x1, y1), kind in rects:
        m = EDGE_MARGIN
        if kind == 'outer':   # 四边条内缩
            bands = [(x0, y0, x0 + m, y1), (x1 - m, y0, x1, y1),
                     (x0, y0, x1, y0 + m), (x0, y1 - m, x1, y1)]
        else:                 # 孔外扩环: 左右全高条 + 上下中段条
            bands = [(x0 - m, y0 - m, x0, y1 + m), (x1, y0 - m, x1 + m, y1 + m),
                     (x0, y0 - m, x1, y0), (x0, y1, x1, y1 + m)]
        for bx0, by0, bx1, by1 in bands:
            ko_lines.append(strip('edge_margin_%d' % n, bx0, by0, bx1, by1, kind))
            n += 1
    if ko_lines:
        ins = '\n    ' + '\n    '.join(ko_lines)
        txt2, nb = re.subn(r'(\(boundary\s*\(path\s+\S+\s+[\d.]+\s+[^()]*\)\s*\))',
                           lambda mm: mm.group(1) + ins, txt, count=1)
        if nb:
            txt = txt2
            print('[dsn-sanitize] 边缘 keepout 注入 %d 条(margin %g DSN 单位)' % (n, EDGE_MARGIN))
        else:
            print('[dsn-sanitize] 无 boundary 插入点, keepout 注入跳过')
except Exception as e:
    print('[dsn-sanitize] keepout 注入跳过: %r' % (e,))
open(path, 'w').write(txt)
