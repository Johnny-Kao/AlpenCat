import ctypes, json, os
from pathlib import Path
import _cffi_backend

out=Path(os.environ.get("BENCH_OUT","artifacts"))
lib=ctypes.CDLL(_cffi_backend.__file__)
names=[
 "runtime_cffi_route_calls",
 "runtime_cffi_fast_routes",
 "runtime_cffi_legacy_routes",
 "runtime_cffi_shadow_checks",
 "runtime_cffi_shadow_mismatches",
 "runtime_cffi_backup_fallbacks",
]
stats={}
for name in names:
    fn=getattr(lib,name)
    fn.restype=ctypes.c_ulonglong
    stats[name]=int(fn())
(out/"swiss-counters.json").write_text(json.dumps(stats,indent=2))
print(json.dumps(stats,indent=2))
