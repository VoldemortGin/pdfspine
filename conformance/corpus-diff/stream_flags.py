"""fitz 侧：为语料每一页提取内容流特征（Tf 尺寸、Tc/Tw/Tz、BT 数、TJ 大位移），输出 stream_flags.json"""
import json
import re
from pathlib import Path

import fitz

HERE = Path(__file__).resolve().parent
files = [l for l in (HERE / "corpus.txt").read_text().split("\n") if l.strip()]
out = {}
RE_TF = re.compile(rb"/[^\s/\[\]()<>]+\s+(-?(?:\d+\.?\d*|\.\d+))\s+Tf")
RE_TC = re.compile(rb"(-?(?:\d+\.?\d*|\.\d+))\s+Tc")
RE_TW = re.compile(rb"(-?(?:\d+\.?\d*|\.\d+))\s+Tw")
RE_TZ = re.compile(rb"(-?(?:\d+\.?\d*|\.\d+))\s+Tz")
for i, f in enumerate(files):
    try:
        doc = fitz.open(f)
    except Exception as e:  # noqa: BLE001
        out[f] = {"error": repr(e)}
        continue
    pages = {}
    for pno in range(min(len(doc), 20)):
        try:
            cs = doc[pno].read_contents()
        except Exception as e:  # noqa: BLE001
            pages[str(pno)] = {"error": repr(e)}
            continue
        tf = [float(x) for x in RE_TF.findall(cs)]
        tc = [float(x) for x in RE_TC.findall(cs)]
        tw = [float(x) for x in RE_TW.findall(cs)]
        tz = [float(x) for x in RE_TZ.findall(cs)]
        pages[str(pno)] = {
            "tf_le1": any(abs(x) <= 1.0 for x in tf),
            "tf_min": min(tf) if tf else None,
            "tc_nonzero": any(abs(x) > 1e-6 for x in tc),
            "tc_max": max((abs(x) for x in tc), default=0.0),
            "tw_nonzero": any(abs(x) > 1e-6 for x in tw),
            "tz_non100": any(abs(x - 100) > 1e-6 for x in tz),
            "bt": cs.count(b"BT"),
            "tj_arrays": cs.count(b"TJ"),
            "xobj_do": len(re.findall(rb"/\S+\s+Do", cs)),
        }
    out[f] = pages
(HERE / "stream_flags.json").write_text(json.dumps(out))
print("done", len(out))
