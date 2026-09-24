# KiCad Python 导出 Specctra DSN——移植自 fr-route.sh
# 用法: <kicad_python> dsn_export.py <board.kicad_pcb> <out.dsn>
import pcbnew, sys

b = pcbnew.LoadBoard(sys.argv[1])
ok = pcbnew.ExportSpecctraDSN(b, sys.argv[2])
print("[dsn-export]", "OK" if ok else "FAIL")
sys.exit(0 if ok else 1)
