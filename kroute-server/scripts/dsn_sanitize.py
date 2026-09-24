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
