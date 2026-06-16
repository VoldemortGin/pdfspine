#!/usr/bin/env python3
"""Rendering differential harness — oxide-pdf vs fitz (PyMuPDF) page rasters.

oxide-pdf has a tiny-skia renderer (``Page.get_pixmap``); its *visual* output had
never been compared to fitz. This harness renders the same page(s) at the same
geometry (fixed DPI) with both engines and measures perceptual similarity
(SSIM + MAE) so we know objectively how close oxide's renderer is to fitz.

Design notes
------------
* **No PNG decode, no PIL/numpy.** Neither the project venv nor ``.venv-oracle``
  has Pillow/numpy/skimage, and we are forbidden to install anything. Both
  engines expose the rasterised page as a flat RGB byte buffer
  (``Pixmap.samples``, ``width``, ``height``, ``n``). We compare those raw
  buffers directly. SSIM and MAE are implemented in pure Python (operating on a
  downsampled grayscale buffer so it stays fast without numpy).

* **License + crash isolation.** fitz (AGPL) must never share our interpreter,
  so it is rendered by a ``--render-oracle`` worker invoked via
  ``.venv-oracle``. oxide is itself rendered in a ``--render-oxide`` subprocess
  so a Rust panic/abort/hang surfaces as a non-zero exit or timeout instead of
  killing the run. This mirrors ``conformance/run_validation.py`` /
  ``conformance/oracle_extract.py``.

* Each worker writes a tiny header line (JSON: w,h,n) followed by raw bytes to a
  temp file; the orchestrator reads them back. Workers also optionally save a
  PNG via the engine's native ``save`` for human inspection (gitignored cache).

Usage
-----
    # orchestrator (run in the project venv, which can import oxide_pdf):
    python conformance/gt/render_diff.py \
        --manifest conformance/gt/corpus-born/manifest.json \
        --manifest conformance/gt/corpus-eurlex/manifest.json \
        --corpus fixtures/corpus \
        --dpi 150 --pages 1 \
        --report conformance/gt/RENDER-REPORT.md \
        --json   conformance/gt/render-results.json

    # internal worker modes (invoked by the orchestrator, not by hand):
    python      conformance/gt/render_diff.py --render-oxide  <pdf> <page> <dpi> <out> [png]
    .venv-oracle/bin/python conformance/gt/render_diff.py --render-oracle <pdf> <page> <dpi> <out> [png]
"""

from __future__ import annotations

import argparse
import json
import math
import os
import random
import struct
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]

# --------------------------------------------------------------------------- #
# Raw-raster container (engine-agnostic)
# --------------------------------------------------------------------------- #


def _write_raster(out_path: str, w: int, h: int, n: int, samples: bytes) -> None:
    """Header line (JSON) + raw bytes. Avoids any image-codec dependency."""
    header = json.dumps({"w": w, "h": h, "n": n}).encode("utf-8")
    with open(out_path, "wb") as fh:
        fh.write(struct.pack("<I", len(header)))
        fh.write(header)
        fh.write(samples)


def _read_raster(path: str) -> tuple[int, int, int, bytes]:
    with open(path, "rb") as fh:
        (hlen,) = struct.unpack("<I", fh.read(4))
        header = json.loads(fh.read(hlen).decode("utf-8"))
        samples = fh.read()
    return header["w"], header["h"], header["n"], samples


# --------------------------------------------------------------------------- #
# Worker: oxide-pdf renderer (runs in THIS venv)
# --------------------------------------------------------------------------- #


def _render_oxide(pdf: str, page_idx: int, dpi: float, out: str, png: str | None) -> int:
    import oxide_pdf

    doc = oxide_pdf.open(pdf)
    if page_idx >= len(doc):
        sys.stderr.write(f"no such page {page_idx} (len={len(doc)})")
        return 3
    page = doc[page_idx]
    try:
        pm = page.get_pixmap(dpi=dpi)
    except TypeError:
        # dpi kwarg unsupported on this build — fall back to an explicit matrix.
        from oxide_pdf import Matrix  # type: ignore

        s = dpi / 72.0
        pm = page.get_pixmap(matrix=Matrix(s, 0, 0, s, 0, 0))
    samples = bytes(pm.samples)
    w, h, n = pm.width, pm.height, pm.n
    if n == 4:  # drop alpha if any
        samples, n = _drop_alpha(samples, w, h), 3
    _write_raster(out, w, h, n, samples)
    if png:
        try:
            pm.save(png)
        except Exception:  # noqa: BLE001 — PNG is for humans, never load-bearing
            pass
    return 0


# --------------------------------------------------------------------------- #
# Worker: fitz renderer (runs ONLY under .venv-oracle)
# --------------------------------------------------------------------------- #


def _render_oracle(pdf: str, page_idx: int, dpi: float, out: str, png: str | None) -> int:
    import fitz  # PyMuPDF

    doc = fitz.open(pdf)
    if page_idx >= doc.page_count:
        sys.stderr.write(f"no such page {page_idx} (count={doc.page_count})")
        return 3
    page = doc[page_idx]
    scale = dpi / 72.0
    pm = page.get_pixmap(matrix=fitz.Matrix(scale, scale), colorspace=fitz.csRGB, alpha=False)
    samples = bytes(pm.samples)
    w, h, n = pm.width, pm.height, pm.n
    if n == 4:
        samples, n = _drop_alpha(samples, w, h), 3
    _write_raster(out, w, h, n, samples)
    if png:
        try:
            pm.save(png)
        except Exception:  # noqa: BLE001
            pass
    return 0


def _drop_alpha(samples: bytes, w: int, h: int) -> bytes:
    out = bytearray(w * h * 3)
    mv = memoryview(samples)
    j = 0
    for i in range(0, len(mv), 4):
        out[j] = mv[i]
        out[j + 1] = mv[i + 1]
        out[j + 2] = mv[i + 2]
        j += 3
    return bytes(out)


# --------------------------------------------------------------------------- #
# Image similarity (pure Python, operate on downsampled grayscale)
# --------------------------------------------------------------------------- #

# Cap the working grayscale image to this many pixels on the long side. SSIM is
# robust to mild downsampling, and this keeps pure-Python math fast even for
# A4@150dpi (~1275x1650). The same target is applied to both images so they end
# up identically sized.
GRAY_MAX_DIM = 512


def _to_gray_downsampled(
    w: int, h: int, n: int, samples: bytes, max_dim: int = GRAY_MAX_DIM
) -> tuple[int, int, list[float]]:
    """Downsample an RGB(/gray) buffer to a small grayscale float list.

    Returns (gw, gh, pixels) where pixels is row-major length gw*gh in [0,255].

    For speed (pure Python, no numpy) each output cell averages a small fixed
    set of probe pixels within the source cell rather than the full box — this
    retains anti-aliasing fidelity well enough for perceptual SSIM while keeping
    the work O(gw*gh*probes) instead of O(w*h).
    """
    scale = max(1, math.ceil(max(w, h) / max_dim))
    gw = max(1, w // scale)
    gh = max(1, h // scale)
    mv = memoryview(samples)
    stride = w * n
    out = [0.0] * (gw * gh)
    # Probe offsets within a source cell (corners + center of the scale×scale box).
    if scale == 1:
        probes_y = (0,)
        probes_x = (0,)
    else:
        q = max(1, scale // 4)
        probes_y = (0, scale // 2, scale - 1, q, scale - 1 - q)
        probes_x = probes_y
    for gy in range(gh):
        y0 = gy * scale
        for gx in range(gw):
            x0 = gx * scale
            acc = 0
            cnt = 0
            for dy in probes_y:
                yy = y0 + dy
                if yy >= h:
                    continue
                rowbase = yy * stride
                for dx in probes_x:
                    xx = x0 + dx
                    if xx >= w:
                        continue
                    base = rowbase + xx * n
                    if n >= 3:
                        acc += 299 * mv[base] + 587 * mv[base + 1] + 114 * mv[base + 2]
                    else:
                        acc += 1000 * mv[base]
                    cnt += 1
            out[gy * gw + gx] = acc / (cnt * 1000) if cnt else 0.0
    return gw, gh, out


def _resize_gray(gw: int, gh: int, px: list[float], tw: int, th: int) -> list[float]:
    """Nearest-neighbour resize of a grayscale buffer to (tw, th)."""
    if gw == tw and gh == th:
        return px
    out = [0.0] * (tw * th)
    for y in range(th):
        sy = min(gh - 1, int(y * gh / th))
        for x in range(tw):
            sx = min(gw - 1, int(x * gw / tw))
            out[y * tw + x] = px[sy * gw + sx]
    return out


def mae_similarity(a: list[float], b: list[float]) -> tuple[float, float]:
    """Mean absolute error over [0,255] and a 0..1 similarity (1 - mae/255)."""
    if not a:
        return 255.0, 0.0
    s = 0.0
    for i in range(len(a)):
        s += abs(a[i] - b[i])
    mae = s / len(a)
    return mae, max(0.0, 1.0 - mae / 255.0)


def ssim(a: list[float], b: list[float], w: int, h: int, win: int = 7) -> float:
    """Windowed SSIM (Wang et al. 2004) over two equal-size grayscale buffers.

    Uniform (box) windows of side ``win``; means/variances/covariance per window;
    returns the mean SSIM across all windows. Pure Python, no numpy.
    """
    if w < win or h < win:
        # too small to window — fall back to a single global SSIM
        win = min(w, h)
        if win < 2:
            return 1.0 if a == b else 0.0
    c1 = (0.01 * 255) ** 2
    c2 = (0.03 * 255) ** 2
    total = 0.0
    count = 0
    npx = win * win
    step = max(1, win // 2)  # 50% overlap keeps it cheap but representative
    for y0 in range(0, h - win + 1, step):
        for x0 in range(0, w - win + 1, step):
            sa = sb = saa = sbb = sab = 0.0
            for yy in range(y0, y0 + win):
                row = yy * w
                for xx in range(x0, x0 + win):
                    va = a[row + xx]
                    vb = b[row + xx]
                    sa += va
                    sb += vb
                    saa += va * va
                    sbb += vb * vb
                    sab += va * vb
            mu_a = sa / npx
            mu_b = sb / npx
            var_a = saa / npx - mu_a * mu_a
            var_b = sbb / npx - mu_b * mu_b
            cov = sab / npx - mu_a * mu_b
            num = (2 * mu_a * mu_b + c1) * (2 * cov + c2)
            den = (mu_a * mu_a + mu_b * mu_b + c1) * (var_a + var_b + c2)
            total += num / den if den else 1.0
            count += 1
    return total / count if count else 1.0


def _mean(px: list[float]) -> float:
    return sum(px) / len(px) if px else 0.0


# --------------------------------------------------------------------------- #
# Orchestration
# --------------------------------------------------------------------------- #


def _run_worker(
    python_exe: str, mode: str, pdf: str, page: int, dpi: float, out: str, png: str | None, timeout: float
) -> tuple[bool, str]:
    cmd = [python_exe, str(Path(__file__).resolve()), mode, pdf, str(page), str(dpi), out]
    if png:
        cmd.append(png)
    env = dict(os.environ)
    env.pop("CONDA_PREFIX", None)
    try:
        proc = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout, env=env)
    except subprocess.TimeoutExpired:
        return False, f"timeout after {timeout}s"
    except Exception as exc:  # noqa: BLE001
        return False, f"{type(exc).__name__}: {exc}"
    if proc.returncode != 0:
        msg = (proc.stderr or proc.stdout or "").strip().replace("\n", " ")
        return False, f"exit {proc.returncode}: {msg[:200]}"
    return True, ""


def _near_blank(px: list[float]) -> bool:
    """True if the grayscale render is (near) a flat field — likely a failed draw."""
    if not px:
        return True
    mu = _mean(px)
    var = sum((v - mu) ** 2 for v in px) / len(px)
    return var < 4.0  # std < 2 gray levels => essentially uniform


def compare_one(
    name: str,
    pdf: str,
    corpus: str,
    page: int,
    dpi: float,
    py_oxide: str,
    py_oracle: str,
    cache_dir: Path,
    timeout: float,
    save_png: bool,
) -> dict:
    rec: dict = {"name": name, "corpus": corpus, "pdf": pdf, "page": page}
    if not Path(pdf).exists():
        rec["error"] = "pdf-missing"
        return rec

    tag = f"{corpus}__{name}__p{page}"
    ox_raw = cache_dir / f"{tag}.oxide.bin"
    or_raw = cache_dir / f"{tag}.fitz.bin"
    ox_png = str(cache_dir / f"{tag}.oxide.png") if save_png else None
    or_png = str(cache_dir / f"{tag}.fitz.png") if save_png else None

    ok_ox, err_ox = _run_worker(py_oxide, "--render-oxide", pdf, page, dpi, str(ox_raw), ox_png, timeout)
    ok_or, err_or = _run_worker(py_oracle, "--render-oracle", pdf, page, dpi, str(or_raw), or_png, timeout)

    if not ok_ox:
        rec["error"] = f"oxide: {err_ox}"
        rec["oxide_failed"] = True
    if not ok_or:
        rec["error"] = (rec.get("error", "") + f" | fitz: {err_or}").strip(" |")
        rec["fitz_failed"] = True
    if not (ok_ox and ok_or):
        return rec

    try:
        ow, oh, on, osamp = _read_raster(str(ox_raw))
        fw, fh, fn, fsamp = _read_raster(str(or_raw))
    except Exception as exc:  # noqa: BLE001
        rec["error"] = f"raster-read: {type(exc).__name__}: {exc}"
        return rec

    rec["oxide_size"] = [ow, oh]
    rec["fitz_size"] = [fw, fh]
    rec["size_dw"] = ow - fw
    rec["size_dh"] = oh - fh

    # Downsample both to small grayscale, then force identical target dims.
    ogw, ogh, opx = _to_gray_downsampled(ow, oh, on, osamp)
    fgw, fgh, fpx = _to_gray_downsampled(fw, fh, fn, fsamp)
    tw, th = min(ogw, fgw), min(ogh, fgh)
    opx = _resize_gray(ogw, ogh, opx, tw, th)
    fpx = _resize_gray(fgw, fgh, fpx, tw, th)

    mae, mae_sim = mae_similarity(opx, fpx)
    s = ssim(opx, fpx, tw, th)
    rec["ssim"] = round(s, 4)
    rec["mae"] = round(mae, 2)
    rec["mae_sim"] = round(mae_sim, 4)
    rec["oxide_mean_gray"] = round(_mean(opx), 1)
    rec["fitz_mean_gray"] = round(_mean(fpx), 1)
    rec["oxide_near_blank"] = _near_blank(opx)
    rec["fitz_near_blank"] = _near_blank(fpx)

    # Heuristic cause guess for divergences.
    # ink_gap > 0 means oxide is LIGHTER than fitz (drew less ink); a large gap
    # on a matching-size page is the signature of missing glyphs / body text.
    ink_gap = rec["oxide_mean_gray"] - rec["fitz_mean_gray"]
    rec["ink_gap"] = round(ink_gap, 1)
    size_off = abs(rec["size_dw"]) + abs(rec["size_dh"])
    if rec["oxide_near_blank"] and not rec["fitz_near_blank"]:
        rec["cause"] = "oxide near-blank — renderer drew (almost) nothing"
    elif size_off > 6:
        rec["cause"] = f"page-box mismatch — size delta {rec['size_dw']}x{rec['size_dh']}px"
    elif s < 0.7 and ink_gap > 6:
        rec["cause"] = f"oxide drew much less ink (+{ink_gap:.0f} gray) — missing glyphs / body text not rendered"
    elif s < 0.7 and ink_gap < -6:
        rec["cause"] = f"oxide drew much more ink ({ink_gap:.0f} gray) — over-dark / fill or color差异"
    elif s < 0.7:
        rec["cause"] = "low SSIM at matching size — glyph positioning / vector ops / color差异"
    elif s < 0.9:
        rec["cause"] = "moderate divergence — partial glyph/vector/AA differences"
    else:
        rec["cause"] = "good parity"
    return rec


def _load_manifest(path: Path) -> list[dict]:
    data = json.loads(path.read_text())
    entries = data if isinstance(data, list) else data.get("docs") or data.get("entries") or []
    out = []
    corpus = path.parent.name
    for e in entries:
        if not isinstance(e, dict) or "pdf" not in e:
            continue
        out.append({"name": e.get("name") or Path(e["pdf"]).stem, "pdf": e["pdf"], "corpus": corpus})
    return out


def _load_corpus_dir(path: Path) -> list[dict]:
    corpus = path.name
    return [
        {"name": p.stem, "pdf": str(p), "corpus": corpus}
        for p in sorted(path.glob("*.pdf"))
    ]


def _sample(entries: list[dict], k: int | None, seed: int) -> list[dict]:
    if k is None or k <= 0 or len(entries) <= k:
        return entries
    rng = random.Random(seed)
    return sorted(rng.sample(entries, k), key=lambda e: e["name"])


def main(argv: list[str]) -> int:
    # ---- internal worker dispatch (must come first) ----
    if argv and argv[0] in ("--render-oxide", "--render-oracle"):
        mode = argv[0]
        pdf, page_s, dpi_s, out = argv[1], argv[2], argv[3], argv[4]
        png = argv[5] if len(argv) > 5 else None
        page = int(page_s)
        dpi = float(dpi_s)
        if mode == "--render-oxide":
            return _render_oxide(pdf, page, dpi, out, png)
        return _render_oracle(pdf, page, dpi, out, png)

    ap = argparse.ArgumentParser(description="oxide-vs-fitz rendering differential harness")
    ap.add_argument("--manifest", action="append", default=[], help="manifest.json (entries with a 'pdf' path)")
    ap.add_argument("--corpus", action="append", default=[], help="directory of bare PDFs")
    ap.add_argument("--dpi", type=float, default=150.0)
    ap.add_argument("--pages", type=int, default=1, help="how many leading pages per doc (0/1)")
    ap.add_argument("--sample", type=int, default=0, help="cap docs per corpus (0 = all)")
    ap.add_argument("--seed", type=int, default=1234)
    ap.add_argument("--timeout", type=float, default=120.0, help="per-render wall-clock timeout (s)")
    ap.add_argument("--python", default=sys.executable, help="interpreter that can import oxide_pdf")
    ap.add_argument(
        "--oracle-python",
        default=str(ROOT / ".venv-oracle" / "bin" / "python"),
        help="interpreter that can import fitz (PyMuPDF)",
    )
    ap.add_argument("--cache", default=str(ROOT / "conformance" / "gt" / "cache" / "render"))
    ap.add_argument("--no-png", action="store_true", help="skip saving inspection PNGs")
    ap.add_argument("--report", required=True)
    ap.add_argument("--json", dest="json_out", required=True)
    args = ap.parse_args(argv)

    cache_dir = Path(args.cache)
    cache_dir.mkdir(parents=True, exist_ok=True)

    # Gather + per-corpus sample.
    by_corpus: dict[str, list[dict]] = {}
    for m in args.manifest:
        entries = _load_manifest(Path(m))
        if entries:
            by_corpus.setdefault(entries[0]["corpus"], []).extend(entries)
    for c in args.corpus:
        entries = _load_corpus_dir(Path(c))
        if entries:
            by_corpus.setdefault(entries[0]["corpus"], []).extend(entries)

    if not by_corpus:
        sys.stderr.write("no input PDFs found via --manifest/--corpus\n")
        return 2

    sampled: dict[str, list[dict]] = {}
    sample_log: dict[str, dict] = {}
    for corpus, entries in by_corpus.items():
        chosen = _sample(entries, args.sample or None, args.seed)
        sampled[corpus] = chosen
        sample_log[corpus] = {"total": len(entries), "sampled": len(chosen)}

    oracle_ok = Path(args.oracle_python).exists()
    n_pages = max(1, args.pages)
    save_png = not args.no_png

    records: list[dict] = []
    t0 = time.time()
    total_docs = sum(len(v) for v in sampled.values())
    done = 0
    for corpus, entries in sampled.items():
        for e in entries:
            for page in range(n_pages):
                rec = compare_one(
                    e["name"], e["pdf"], corpus, page, args.dpi,
                    args.python, args.oracle_python, cache_dir, args.timeout, save_png,
                )
                records.append(rec)
            done += 1
            ssim_str = "n/a"
            last = records[-1]
            if "ssim" in last:
                ssim_str = f"{last['ssim']:.3f}"
            print(f"[{done}/{total_docs}] {corpus}/{e['name']}: ssim={ssim_str} {last.get('error','')}")

    elapsed = time.time() - t0

    # ---- aggregate ----
    def agg(recs: list[dict]) -> dict:
        ss = sorted(r["ssim"] for r in recs if "ssim" in r)
        ma = sorted(r["mae_sim"] for r in recs if "mae_sim" in r)
        n = len(ss)
        return {
            "n_docs": len({(r["corpus"], r["name"]) for r in recs}),
            "n_compared": n,
            "n_errors": sum(1 for r in recs if r.get("error")),
            "ssim_mean": round(sum(ss) / n, 4) if n else None,
            "ssim_median": round(ss[n // 2], 4) if n else None,
            "mae_sim_mean": round(sum(ma) / len(ma), 4) if ma else None,
        }

    overall = agg(records)
    per_corpus = {c: agg([r for r in records if r["corpus"] == c]) for c in sampled}

    scored = [r for r in records if "ssim" in r]
    worst = sorted(scored, key=lambda r: r["ssim"])[:10]

    payload = {
        "generated": time.strftime("%Y-%m-%dT%H:%M:%S%z"),
        "dpi": args.dpi,
        "pages_per_doc": n_pages,
        "method": "raw RGB sample buffers -> downsampled grayscale -> windowed SSIM + MAE (pure Python; no PNG decode)",
        "oracle_available": oracle_ok,
        "elapsed_s": round(elapsed, 1),
        "sample_log": sample_log,
        "overall": overall,
        "per_corpus": per_corpus,
        "worst": worst,
        "records": records,
    }
    Path(args.json_out).write_text(json.dumps(payload, indent=2))
    _write_report(Path(args.report), payload)
    print(f"\nWrote {args.report} and {args.json_out} ({elapsed:.0f}s, {total_docs} docs)")
    return 0


def _verdict(overall: dict, per_corpus: dict, records: list[dict]) -> str:
    m = overall.get("ssim_mean")
    if m is None:
        return "No documents could be scored (no successful oxide+fitz render pairs)."
    blanks = sum(1 for r in records if r.get("oxide_near_blank") and not r.get("fitz_near_blank"))
    boxbug = sum(1 for r in records if abs(r.get("size_dw", 0)) + abs(r.get("size_dh", 0)) > 6)
    parts = []
    if m >= 0.95:
        parts.append(f"AT/NEAR PARITY — mean SSIM {m:.3f}. Renderer matches fitz closely (AA/hinting aside).")
    elif m >= 0.90:
        parts.append(f"CLOSE — mean SSIM {m:.3f}. Broadly faithful with localized differences.")
    elif m >= 0.75:
        parts.append(f"PARTIAL — mean SSIM {m:.3f}. Recognizable but with systematic gaps.")
    else:
        parts.append(f"DIVERGENT — mean SSIM {m:.3f}. Substantial rendering differences.")
    if blanks:
        parts.append(f"{blanks} doc(s) render near-blank in oxide while fitz draws content (renderer failure).")
    if boxbug:
        parts.append(f"{boxbug} doc(s) have a page-box / size mismatch (>6px).")
    return " ".join(parts)


def _write_report(path: Path, p: dict) -> None:
    a = []
    a.append("# oxide-pdf vs fitz — Rendering Differential\n")
    a.append(f"_Generated {p['generated']} · DPI {p['dpi']:.0f} · {p['pages_per_doc']} page(s)/doc · "
             f"oracle_available={p['oracle_available']} · {p['elapsed_s']:.0f}s_\n")
    a.append(f"**Method:** {p['method']}\n")
    a.append("SSIM is 0..1 (1 = identical). AA / hinting / sub-pixel differences mean an exact "
             "match is not expected; SSIM ≳0.90 indicates good visual parity.\n")

    a.append("## Verdict\n")
    a.append(_verdict(p["overall"], p["per_corpus"], p["records"]) + "\n")

    o = p["overall"]
    a.append("## Aggregate (overall)\n")
    a.append("| docs | compared | errors | SSIM mean | SSIM median | MAE-sim mean |")
    a.append("|---|---|---|---|---|---|")
    a.append(f"| {o['n_docs']} | {o['n_compared']} | {o['n_errors']} | {o['ssim_mean']} | "
             f"{o['ssim_median']} | {o['mae_sim_mean']} |\n")

    a.append("## Per-corpus\n")
    a.append("| corpus | sampled/total | compared | errors | SSIM mean | SSIM median | MAE-sim mean |")
    a.append("|---|---|---|---|---|---|---|")
    for c, st in p["sample_log"].items():
        cc = p["per_corpus"].get(c, {})
        a.append(f"| {c} | {st['sampled']}/{st['total']} | {cc.get('n_compared','-')} | "
                 f"{cc.get('n_errors','-')} | {cc.get('ssim_mean','-')} | "
                 f"{cc.get('ssim_median','-')} | {cc.get('mae_sim_mean','-')} |")
    a.append("")

    a.append("## Worst ~10 divergences (lowest SSIM)\n")
    a.append("| corpus/doc | page | SSIM | MAE | oxide size | fitz size | Δw×Δh | cause guess |")
    a.append("|---|---|---|---|---|---|---|---|")
    for r in p["worst"]:
        a.append(
            f"| {r['corpus']}/{r['name']} | {r['page']} | {r['ssim']} | {r['mae']} | "
            f"{r['oxide_size'][0]}×{r['oxide_size'][1]} | {r['fitz_size'][0]}×{r['fitz_size'][1]} | "
            f"{r['size_dw']}×{r['size_dh']} | {r['cause']} |"
        )
    a.append("")

    errs = [r for r in p["records"] if r.get("error")]
    if errs:
        a.append("## Render errors / skips\n")
        a.append("| corpus/doc | page | error |")
        a.append("|---|---|---|")
        for r in errs:
            a.append(f"| {r['corpus']}/{r['name']} | {r.get('page','-')} | {r['error']} |")
        a.append("")

    path.write_text("\n".join(a))


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
