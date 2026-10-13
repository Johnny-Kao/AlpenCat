#!/usr/bin/env python3
"""Nonprivileged Linux OS resource transition smoke: actual scheduler affinity."""
import json,os,pathlib,time
out=pathlib.Path("native-batch-results");out.mkdir(exist_ok=True)
data={"kind":"real_linux_os_affinity_event","simulated_hfi":False,"supported":False}
if not hasattr(os,"sched_getaffinity") or not hasattr(os,"sched_setaffinity"):
    data["reason"]="affinity API unavailable"
else:
    original=os.sched_getaffinity(0)
    data["original_affinity_count"]=len(original)
    try:
        if len(original)<2:
            data["reason"]="one available CPU; transition unavailable"
        else:
            chosen={min(original)}
            os.sched_setaffinity(0,chosen)
            observed=os.sched_getaffinity(0)
            assert observed==chosen,(observed,chosen)
            data["restricted_affinity_count"]=len(observed)
            data["supported"]=True
    except OSError as e:
        data["reason"]=f"OS denied affinity change: {e}"
    finally:
        os.sched_setaffinity(0,original)
        data["restored_affinity_count"]=len(os.sched_getaffinity(0))
        assert os.sched_getaffinity(0)==original
# Observe cgroup but never attempt privileged writes.
for path in ("/sys/fs/cgroup/cpu.max","/sys/fs/cgroup/cpuset.cpus.effective","/proc/pressure/cpu"):
    try:data[path]=pathlib.Path(path).read_text()[:1500]
    except OSError as e:data[path]=f"UNAVAILABLE: {e}"
(out/"os-event.json").write_text(json.dumps(data,indent=2)+"\n")
print(json.dumps(data,indent=2))
