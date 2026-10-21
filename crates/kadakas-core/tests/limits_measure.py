#!/usr/bin/env python3
"""Runs one scenario of limits_measure.rs several times, each run in its own process under
/usr/bin/time -l, and prints the median of every number (wall, user+sys CPU, maximum resident set
size of the whole process, and the numbers of the RESULT lines).

  limits_measure.py BINARY RUNS SCENARIO "ARGS"        e.g. ... bin 5 shape "terrain 158"

BINARY is the test binary (cargo prints it with --no-run; the same for the ui-core and props
tests). Environment LM_TEST_NAME is the name of the test function (default: scenario)."""
import os, re, statistics, subprocess, sys

binary, runs, scen, scen_args = sys.argv[1], int(sys.argv[2]), sys.argv[3], sys.argv[4] if len(sys.argv) > 4 else ""
test = os.environ.get("LM_TEST_NAME", "scenario")
env = dict(os.environ, LM=scen, LM_ARGS=scen_args, LC_ALL="C", LANG="C")
rows, procs = [], []
for _ in range(runs):
    p = subprocess.run(["/usr/bin/time", "-l", binary, "--ignored", "--exact", test, "--nocapture"],
                       env=env, capture_output=True, text=True)
    err = p.stderr
    def g(pat):
        m = re.search(pat, err)
        return float(m.group(1)) if m else 0.0
    m = re.search(r"([\d.]+)\s+real\s+([\d.]+)\s+user\s+([\d.]+)\s+sys", err)
    real, user, sys_ = (float(m.group(i)) for i in (1, 2, 3)) if m else (0, 0, 0)
    procs.append(dict(real=real, cpu=user + sys_, maxrss_mb=g(r"(\d+)\s+maximum resident set size") / 1048576))
    res = [l[7:] for l in p.stdout.splitlines() if l.startswith("RESULT ")]
    if p.returncode != 0 and not res:
        res = ["CRASH rc=%d %s" % (p.returncode, (p.stdout.strip().splitlines() or [""])[-1][:100] + " | " + " ".join(err.strip().splitlines()[:3])[:300])]
    rows.append(res)
def med(vals):
    return statistics.median(vals)
print(f"## {scen} {scen_args}  (runs={runs}; uptime: {subprocess.run(['uptime'],capture_output=True,text=True).stdout.strip().split('load averages:')[-1].strip()})")
print("process: wall=%.2fs cpu(user+sys)=%.2fs maxrss=%.0f MB" % (med([p['real'] for p in procs]), med([p['cpu'] for p in procs]), med([p['maxrss_mb'] for p in procs])))
nlines = max(len(r) for r in rows)
for i in range(nlines):
    lines = [r[i] for r in rows if i < len(r)]
    first = lines[0].split()
    out = [first[0]]
    def parse(l):
        # "raw_wall_s=1 cpu_s=2 peak_heap_mb=3": the keys after a prefixed wall_s take its prefix.
        d, pre = {}, ""
        for t in l.split()[1:]:
            k, v = t.split("=", 1) if "=" in t else (t, None)
            m = re.match(r"^(.+_)wall_s$", k)
            if k == "wall_s":
                pre = ""
            elif m:
                pre = m.group(1)
            elif k in ("cpu_s", "peak_heap_mb", "kept_heap_mb"):
                k = pre + k
            d[k] = v
        return d
    toks = [parse(l) for l in lines]
    for key in toks[0]:
        vals = [t.get(key) for t in toks]
        v0 = vals[0]
        if v0 is None:
            out.append(key); continue
        try:
            nums = [float(v) for v in vals if v is not None]
            m = med(nums)
            out.append(f"{key}={m:.4g}" if abs(m) < 1e5 and m != int(m) else f"{key}={int(m)}")
        except ValueError:
            out.append(f"{key}={v0}")
    print(" ".join(out))
