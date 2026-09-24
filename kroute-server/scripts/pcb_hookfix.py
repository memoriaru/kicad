#!/usr/bin/env python3
"""pcb-hookfix — SES 回导/pcbnew 保存后的 pcb 文本规范化器.

痛点(2026-09-11 v36 实测): ses_import 后 pcbnew 保存的文件与 kicad-json5
生成物格式漂移, 下游解析器(board_check/layout_audit/文本手术脚本)逐一踩坑:
  1. (property "Reference" 换行 "U1"     <-> 单行 (property "Reference" "U1")
  2. pad 内 (net "1V2") 无编号            <-> 原生 (net 4 "1V2")
  3. (at 48.8 52.0) 尾零                 <-> (at 48.8 52) —— 文本 replace 锚定失配
  4. footprint 主 at 的 rot 字段时有时无
本工具把上述漂移**归一**到工具链标准形态(幂等, 可重复执行):
  - property 四主属性单行化
  - pad 内 net 行补回编号(查头部 net 声明表)
  - at/size/start/end 坐标去尾零
用法: python3 pcb_hookfix.py <board.kicad_pcb>   (原地规范化)
管线位置: dsn_export → freerouting → ses_import → **pcb_hookfix** → placement 重放 → DRC/内审
"""
import re, sys

path = sys.argv[1]
txt = open(path).read()
orig_len = len(txt)
changes = []

# ── 1. 头部 net 声明表 ──
hdr_end = txt.find('(footprint')
net_by_name = {}
for m in re.finditer(r'\(net (\d+) "([^"]+)"\)', txt[:hdr_end if hdr_end > 0 else len(txt)]):
    net_by_name[m.group(2)] = m.group(1)
if not net_by_name:
    # SES 回导谱系: 无编号表. 从 pad 引用集合重建 (GND 惯例 0 之后的从 1 起按名排序)
    names = sorted(set(re.findall(r'\(pad "[^"]*"\s+\S+\s+\S+\s*\n\s*\(at [^)]*\)[\s\S]{0,200}?\(net "([^"]+)"\)', txt)))
    decls = []
    for idx, name in enumerate(names, start=1):
        net_by_name[name] = str(idx)
        decls.append(f'\t(net {idx} "{name}")')
    ins = txt.find('(footprint')
    txt = txt[:ins] + '\n'.join(decls) + '\n' + txt[ins:]
    changes.append(f'重建 net 声明表 {len(names)} 条')

# ── 2. property 四主属性单行化 ──
n = 0
def prop_fold(m):
    global n
    n += 1
    return f'(property "{m.group(1)}" "{m.group(2)}"'
txt2 = re.sub(r'\(property "(Reference|Value|Footprint|Datasheet)"\s*\n\s*"([^"]*)"', prop_fold, txt)
if n: changes.append(f'property 单行化 {n} 处')
txt = txt2

# ── 3. pad 内 net 行补编号 ──
n = 0
def net_fix(m):
    global n
    name = m.group(1)
    num = net_by_name.get(name)
    if num is None:
        return m.group(0)
    n += 1
    return f'(net {num} "{name}")'
# 只处理 pad 块内的 (net "X") —— 以 (pad 起点括号配对扫描, 避免误改头部声明
out = []
i = 0
while True:
    j = txt.find('(pad ', i)
    if j < 0:
        out.append(txt[i:]); break
    out.append(txt[i:j])
    depth, k = 0, j
    while True:
        c = txt[k]
        if c == '(': depth += 1
        elif c == ')':
            depth -= 1
            if depth == 0: break
        k += 1
    blk = txt[j:k+1]
    blk2 = re.sub(r'\(net "([^"]+)"\)', net_fix, blk)
    out.append(blk2)
    i = k + 1
txt = ''.join(out)
if n: changes.append(f'pad net 补编号 {n} 处')

# ── 4. 坐标去尾零 (仅 at/size/start/end 行内, 不碰 stroke width 等) ──
n = 0
def trim_zero(m):
    global n
    line = m.group(0)
    line2 = re.sub(r'(\d+)\.0+(?=[\s)])', r'\1', line)
    line2 = re.sub(r'(\d+\.\d*?)0+(?=[\s)])', r'\1', line2)
    if line2 != line: n += 1
    return line2
txt2 = re.sub(r'\((?:at|size|start|end) [^\n]+', trim_zero, txt)
txt = txt2
if n: changes.append(f'坐标去尾零 {n} 行')

open(path, 'w').write(txt)
print(f'[pcb-hookfix] {path}')
for c in changes:
    print('  -', c)
if not changes:
    print('  - 已是规范形态 (幂等)')
