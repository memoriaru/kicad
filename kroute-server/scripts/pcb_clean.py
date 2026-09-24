# 清除现有走线/过孔（保留布局、封装、zone）——移植自 fr-route.sh
# 用法: python3 pcb_clean.py <board.kicad_pcb>
import re, sys

path = sys.argv[1]
txt = open(path).read()


def strip_blocks(txt, keyword):
    out, i, n = [], 0, len(txt)
    pat = re.compile(r'\n\t\(' + keyword + r'[\s\n]')
    while True:
        m = pat.search(txt, i)
        if not m:
            out.append(txt[i:])
            break
        out.append(txt[i:m.start() + 1])
        depth, j = 0, m.start() + 1
        while j < n:
            c = txt[j]
            if c == '(':
                depth += 1
            elif c == ')':
                depth -= 1
                if depth == 0:
                    break
            j += 1
        i = j + 1  # 只跳过 ')'；换行留给下一轮匹配（模式自带前导 \n）
    return ''.join(out)


before = txt.count('(segment') + txt.count('(via')
txt = strip_blocks(txt, 'segment')
txt = strip_blocks(txt, 'via')
open(path, 'w').write(txt)
print(f"[pcb-clean] 已删除 {before} 个走线/过孔块")
