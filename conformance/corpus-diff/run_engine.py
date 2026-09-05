"""驱动：python run_engine.py <fitz|pdfspine> <python-interpreter> [output-label]

对 corpus.txt 每个文件起子进程跑 extract_one.py（超时 180s），输出到 out/<engine>/<idx>.json。
"""

import os
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
engine, py = sys.argv[1], sys.argv[2]
label = sys.argv[3] if len(sys.argv) > 3 else engine
outdir = HERE / "out" / label
outdir.mkdir(parents=True, exist_ok=True)
env = dict(os.environ)
env.pop("CONDA_PREFIX", None)
env["PYTHONHASHSEED"] = "0"

files = [line for line in (HERE / "corpus.txt").read_text().split("\n") if line.strip()]
log = open(HERE / f"run_{label}.log", "w")
t0 = time.time()
for i, f in enumerate(files):
    out = outdir / f"{i:03d}.json"
    if out.exists():
        continue
    t = time.time()
    try:
        r = subprocess.run(
            [py, str(HERE / "extract_one.py"), engine, f, str(out)],
            env=env,
            capture_output=True,
            text=True,
            timeout=180,
        )
        status = f"rc={r.returncode}"
        if r.returncode != 0:
            status += " stderr=" + r.stderr[-300:].replace("\n", " | ")
    except subprocess.TimeoutExpired:
        status = "TIMEOUT"
        out.write_text(
            '{"engine":"%s","path":"%s","pages":{},"error":"TIMEOUT","fonts":{}}'
            % (engine, f)
        )
    log.write(f"{i:03d} {time.time() - t:6.1f}s {status} {f}\n")
    log.flush()
log.write(f"DONE total {time.time() - t0:.1f}s\n")
log.close()
print("DONE", label)
