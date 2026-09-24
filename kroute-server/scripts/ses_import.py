# SES 回导 + 保存——移植自 fr-route.sh
# 用法: <kicad_python> ses_import.py <board.kicad_pcb> <in.ses>
import pcbnew
import sys

b = pcbnew.LoadBoard(sys.argv[1])
ok = pcbnew.ImportSpecctraSES(b, sys.argv[2])
if ok:
    pcbnew.SaveBoard(sys.argv[1], b)
    print("[ses-import] SES 回导+保存: OK")
else:
    print("[ses-import] SES 回导: FAIL")
    sys.exit(1)
