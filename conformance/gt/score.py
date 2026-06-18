#!/usr/bin/env python3
"""Decomposed, ground-truth text scorer for pdfspine conformance.

PURE stdlib. No network, no PDF libraries, no I/O. This is the shared scoring
core used by ``run_gt.py`` to score an extractor's output (``hyp``) against TRUE
ground truth (``ref``).

It decomposes "how good is this extraction" into independent axes so we can tell
*why* a number is low:

- **content** (precision / recall / F1 / Jaccard): which words were extracted,
  regardless of order. Multiset-based so spurious or duplicated tokens are
  penalized.
- **order**: reading-ORDER agreement among the tokens both sides share, isolated
  from content errors (so a perfect-vocabulary-but-shuffled extraction scores
  high on content but low on order — the multi-column reading-order failure mode).
- **lev**: the headline normalized token-level edit similarity (sequence-level),
  matching the convention in ``conformance/run_validation.py``.

Run the offline self-test (any Python, pure stdlib)::

    .venv/bin/python conformance/gt/score.py
"""

from __future__ import annotations

import difflib
import re
import unicodedata
from collections import Counter

# --------------------------------------------------------------------------- #
# Normalization
# --------------------------------------------------------------------------- #
_WS = re.compile(r"\s+")

# Ligatures that NFKC does NOT decompose (they are not compatibility-decomposable
# in Unicode, so we expand them explicitly to match extractor conventions).
_LIGATURES = {
    "ﬀ": "ff",   # ﬀ
    "ﬁ": "fi",   # ﬁ
    "ﬂ": "fl",   # ﬂ
    "ﬃ": "ffi",  # ﬃ
    "ﬄ": "ffl",  # ﬄ
    "ﬅ": "st",   # ﬅ  (long-s + t)
    "ﬆ": "st",   # ﬆ
}
_LIG_RE = re.compile("|".join(map(re.escape, _LIGATURES)))

_SOFT_HYPHEN = "­"

# A hyphen at the end of a line, followed by a newline (optionally with trailing
# spaces) and a word continuation, is a line-break hyphenation: "exam-\nple".
# We de-hyphenate by joining the two fragments. We require a word char on each
# side so we don't eat real dashes like "well-\nknown" intent? -> standard PDF
# extraction de-hyphenation joins these too; we follow that (join unconditionally
# when a word char precedes the hyphen and follows the break).
_LINEBREAK_HYPHEN = re.compile(r"(\w)[-‐]\s*\n\s*(\w)")

# Token splitter operates on already-normalized (single-spaced) text.
_TOKEN_CAP = 50_000

# CJK / Japanese-kana ranges whose characters are written WITHOUT spaces, so a
# whitespace split would lump a whole run into one token and make the metrics
# meaningless. We treat each such character as its own token (the natural unit
# for CJK accuracy). Latin/space-delimited text is unaffected because none of
# these ranges overlap ASCII or Latin scripts.
#
#   U+3000–U+303F  CJK symbols & punctuation (、。「」 etc; incl. ideographic space)
#   U+3040–U+309F  Hiragana
#   U+30A0–U+30FF  Katakana
#   U+4E00–U+9FFF  CJK Unified Ideographs
def _is_cjk_char(ch: str) -> bool:
    cp = ord(ch)
    return (
        0x3000 <= cp <= 0x303F  # CJK symbols & punctuation
        or 0x3040 <= cp <= 0x309F  # Hiragana
        or 0x30A0 <= cp <= 0x30FF  # Katakana
        or 0x4E00 <= cp <= 0x9FFF  # CJK Unified Ideographs
    )


def normalize_text(s: str, *, lowercase: bool = False) -> str:
    """Canonicalize text for scoring.

    Steps, in order:

    1. Unicode NFKC normalization.
    2. Expand common ligatures NFKC leaves intact (ﬁ→fi, ﬂ→fl, ﬀ→ff, ﬃ→ffi, ﬄ→ffl).
    3. De-hyphenate line-break hyphenation ("exam-\\nple" → "example"): a hyphen
       between two word characters across a newline is removed and the fragments
       joined. Done BEFORE soft-hyphen stripping and whitespace collapse so the
       newline is still present to anchor on.
    4. Strip soft hyphens (U+00AD) anywhere.
    5. Collapse every whitespace run to a single space; strip ends.

    Case is preserved by default (we score case-sensitively); pass
    ``lowercase=True`` to fold case.
    """
    if not s:
        return ""
    s = unicodedata.normalize("NFKC", s)
    s = _LIG_RE.sub(lambda m: _LIGATURES[m.group(0)], s)
    s = _LINEBREAK_HYPHEN.sub(r"\1\2", s)
    s = s.replace(_SOFT_HYPHEN, "")
    s = _WS.sub(" ", s).strip()
    if lowercase:
        s = s.lower()
    return s


def tokenize(s: str, *, lowercase: bool = False) -> list[str]:
    """Tokenize AFTER :func:`normalize_text` (normalize internally).

    Latin / space-delimited text is whitespace-split, byte-for-byte identical to
    the historical behaviour. CJK characters (Unified Ideographs, CJK
    punctuation, Hiragana/Katakana — see :func:`_is_cjk_char`) are each emitted
    as their own one-character token, because CJK is written without spaces and a
    plain whitespace split would collapse an entire run into a single token. The
    two are interleaved correctly: a Latin word run between CJK chars stays one
    token, each CJK char becomes its own.
    """
    norm = normalize_text(s, lowercase=lowercase)
    if not norm:
        return []
    # Fast path: no CJK -> identical to the original ``norm.split(" ")``.
    if not any(_is_cjk_char(ch) for ch in norm):
        return norm.split(" ")
    # Mixed/CJK path: split on whitespace, then break out CJK chars from each
    # whitespace token into individual tokens, preserving order.
    tokens: list[str] = []
    for chunk in norm.split(" "):
        if not chunk:
            continue
        buf: list[str] = []
        for ch in chunk:
            if _is_cjk_char(ch):
                if buf:
                    tokens.append("".join(buf))
                    buf = []
                tokens.append(ch)
            else:
                buf.append(ch)
        if buf:
            tokens.append("".join(buf))
    return tokens


# --------------------------------------------------------------------------- #
# Content metrics (order-independent)
# --------------------------------------------------------------------------- #
def content_scores(hyp: str, ref: str, *, lowercase: bool = False) -> dict:
    """Token multiset precision/recall/F1 + token-set Jaccard.

    Precision/recall/F1 use **multiset** (``Counter``) intersection: the number of
    overlapping tokens counts multiplicity, so spurious or duplicated hypothesis
    tokens reduce precision and missing repetitions reduce recall. Jaccard is the
    classic **set** overlap (vocabulary-level), matching ``run_validation.jaccard``.

    ``hyp`` is the extractor output, ``ref`` is ground truth. Returns
    ``{"precision","recall","f1","jaccard","n_hyp","n_ref"}`` with all ratios in
    [0,1]. Empty/empty → all 1.0; one empty, other not → 0.0.
    """
    ht = tokenize(hyp, lowercase=lowercase)
    rt = tokenize(ref, lowercase=lowercase)
    n_hyp, n_ref = len(ht), len(rt)

    if n_hyp == 0 and n_ref == 0:
        return {"precision": 1.0, "recall": 1.0, "f1": 1.0, "jaccard": 1.0,
                "n_hyp": 0, "n_ref": 0}
    if n_hyp == 0 or n_ref == 0:
        return {"precision": 0.0, "recall": 0.0, "f1": 0.0, "jaccard": 0.0,
                "n_hyp": n_hyp, "n_ref": n_ref}

    hc, rc = Counter(ht), Counter(rt)
    overlap = sum((hc & rc).values())  # multiset intersection size
    precision = overlap / n_hyp
    recall = overlap / n_ref
    f1 = (2 * precision * recall / (precision + recall)) if (precision + recall) else 0.0

    hs, rs = set(ht), set(rt)
    union = len(hs | rs)
    jaccard = (len(hs & rs) / union) if union else 1.0

    return {
        "precision": precision,
        "recall": recall,
        "f1": f1,
        "jaccard": jaccard,
        "n_hyp": n_hyp,
        "n_ref": n_ref,
    }


# --------------------------------------------------------------------------- #
# Order metric (content-independent)
# --------------------------------------------------------------------------- #
def order_score(hyp: str, ref: str, *, lowercase: bool = False) -> float:
    """Reading-ORDER agreement in [0,1], isolated from content errors.

    Definition: align the ``hyp`` and ``ref`` token sequences with
    ``difflib.SequenceMatcher`` and collect the tokens that match (the "common"
    tokens, with their positions on each side). SequenceMatcher's matching blocks
    are themselves order-preserving, so the matched tokens are *already* a common
    subsequence — i.e. they appear in the same relative order on both sides. The
    score is therefore::

        order = (number of matched tokens) / (number of distinct shared tokens
                 that COULD be aligned, accounting for multiplicity)

    Concretely we measure how many of the tokens both sequences share (by
    multiset, ``sum((Counter(hyp) & Counter(ref)).values())``) were placed by the
    alignment into an order-consistent common subsequence. When the two token
    multisets are equal but permuted, the longest order-preserving alignment is
    shorter than the full set, so the score drops below 1 — exactly isolating an
    ordering disagreement from a vocabulary disagreement.

    Edge cases: if there are no shared tokens at all, order is undefined and we
    return 1.0 (no ordering claim to violate — content metrics already capture the
    total mismatch). Empty/empty → 1.0.
    """
    ht = tokenize(hyp, lowercase=lowercase)
    rt = tokenize(ref, lowercase=lowercase)
    if not ht and not rt:
        return 1.0
    if not ht or not rt:
        return 1.0  # nothing shared; order is undefined, don't double-penalize

    if len(ht) > _TOKEN_CAP:
        ht = ht[:_TOKEN_CAP]
    if len(rt) > _TOKEN_CAP:
        rt = rt[:_TOKEN_CAP]

    # Multiset of shared tokens: the maximum number of tokens any common
    # subsequence could contain (the denominator). This is order-agnostic.
    shared = sum((Counter(ht) & Counter(rt)).values())
    if shared == 0:
        return 1.0

    # matching_blocks gives the order-preserving aligned runs; their total size is
    # the length of the longest order-consistent common subsequence found by the
    # alignment. matched <= shared always.
    sm = difflib.SequenceMatcher(None, ht, rt, autojunk=False)
    matched = sum(b.size for b in sm.get_matching_blocks())

    return matched / shared


# --------------------------------------------------------------------------- #
# Headline sequence metric
# --------------------------------------------------------------------------- #
def lev_ratio(hyp: str, ref: str, *, lowercase: bool = False) -> float:
    """Normalized token-level edit similarity in [0,1] (1.0 == identical).

    ``difflib.SequenceMatcher.ratio()`` over the token lists (not raw chars), the
    same approach and cap as ``conformance/run_validation.levenshtein_ratio``.
    Combines both content and order errors into one headline number.
    """
    ht = tokenize(hyp, lowercase=lowercase)
    rt = tokenize(ref, lowercase=lowercase)
    if not ht and not rt:
        return 1.0
    if not ht or not rt:
        return 0.0
    ht = ht[:_TOKEN_CAP]
    rt = rt[:_TOKEN_CAP]
    return difflib.SequenceMatcher(None, ht, rt, autojunk=False).ratio()


# --------------------------------------------------------------------------- #
# Combined
# --------------------------------------------------------------------------- #
def score_all(hyp: str, ref: str, *, lowercase: bool = False) -> dict:
    """All metrics in one pass.

    ``hyp`` = extractor output, ``ref`` = ground truth. Returns::

        {"lev", "precision", "recall", "f1", "jaccard", "order", "n_hyp", "n_ref"}
    """
    content = content_scores(hyp, ref, lowercase=lowercase)
    return {
        "lev": lev_ratio(hyp, ref, lowercase=lowercase),
        "precision": content["precision"],
        "recall": content["recall"],
        "f1": content["f1"],
        "jaccard": content["jaccard"],
        "order": order_score(hyp, ref, lowercase=lowercase),
        "n_hyp": content["n_hyp"],
        "n_ref": content["n_ref"],
    }


# --------------------------------------------------------------------------- #
# Offline self-test
# --------------------------------------------------------------------------- #
def _selftest() -> None:
    EPS = 1e-9

    # 1. Identical strings -> everything perfect.
    s = "The quick brown fox jumps over the lazy dog"
    r = score_all(s, s)
    assert abs(r["lev"] - 1.0) < EPS, r
    assert abs(r["f1"] - 1.0) < EPS, r
    assert abs(r["jaccard"] - 1.0) < EPS, r
    assert abs(r["order"] - 1.0) < EPS, r
    assert abs(r["precision"] - 1.0) < EPS and abs(r["recall"] - 1.0) < EPS, r

    # 2. Same tokens, swapped order -> content perfect, order & lev degraded.
    ref = "a b c d"
    hyp = "b a d c"
    r = score_all(hyp, ref)
    assert abs(r["jaccard"] - 1.0) < EPS, r
    assert abs(r["f1"] - 1.0) < EPS, r          # multiset identical
    assert abs(r["recall"] - 1.0) < EPS, r
    assert abs(r["precision"] - 1.0) < EPS, r
    assert r["order"] < 1.0, r                  # ORDER isolates the shuffle
    assert r["lev"] < 1.0, r                    # sequence metric also drops
    assert r["order"] >= 0.0, r

    # 3. Spurious duplicate tokens -> recall perfect, precision penalized.
    ref = "a b c"
    hyp = "a b c x x"
    r = score_all(hyp, ref)
    assert abs(r["recall"] - 1.0) < EPS, r      # all ref tokens present
    assert r["precision"] < 1.0, r              # multiset penalizes spurious x x
    # 3 overlap / 5 hyp tokens = 0.6
    assert abs(r["precision"] - 0.6) < EPS, r

    # 3b. Duplicated real token is still spurious under multiset precision.
    r2 = content_scores("a a b c", "a b c")
    assert abs(r2["recall"] - 1.0) < EPS, r2
    assert abs(r2["precision"] - 0.75) < EPS, r2  # 3 overlap / 4 hyp
    assert abs(r2["jaccard"] - 1.0) < EPS, r2     # set-level identical

    # 4. Normalization: ligature + soft hyphen + line-break de-hyphenation.
    got = normalize_text("ﬁle so­ft de-\nhyphen")
    assert got == "file soft dehyphen", repr(got)

    # 4b. Each ligature individually.
    assert normalize_text("ﬀ ﬁ ﬂ ﬃ ﬄ") == "ff fi fl ffi ffl", normalize_text("ﬀ ﬁ ﬂ ﬃ ﬄ")
    # 4c. Whitespace runs collapse; NFKC folds full-width.
    assert normalize_text("a\t\n  b\r\nc") == "a b c"
    assert normalize_text("ＡＢＣ") == "ABC"
    # 4d. Case preserved by default, foldable on request.
    assert normalize_text("MixedCase") == "MixedCase"
    assert normalize_text("MixedCase", lowercase=True) == "mixedcase"

    # 5. tokenize normalizes internally.
    assert tokenize("ﬁle  test\n") == ["file", "test"]
    assert tokenize("") == []

    # 5b. CJK tokenization: each ideograph / CJK punctuation is its own token,
    # while interleaved Latin runs stay whole; Latin-only input is unaffected.
    assert tokenize("春天来了") == ["春", "天", "来", "了"], tokenize("春天来了")
    # Ideographic full stop 。(U+3002) is in the CJK-punctuation range NFKC does
    # NOT fold, so it splits as its own token.
    assert tokenize("鸟儿歌唱。绿色") == ["鸟", "儿", "歌", "唱", "。", "绿", "色"], \
        tokenize("鸟儿歌唱。绿色")
    # The fullwidth comma ，(U+FF0C) NFKC-folds to ASCII ',', so it is not a CJK
    # char; bracketed by ideographs it still becomes its own one-char token.
    assert tokenize("春天，鸟儿") == ["春", "天", ",", "鸟", "儿"], tokenize("春天，鸟儿")
    # Mixed CJK + Latin: "abc春x" -> latin run "abc", char "春", latin run "x".
    assert tokenize("abc春x def") == ["abc", "春", "x", "def"], tokenize("abc春x def")
    # Latin-only MUST be byte-identical to the historical whitespace split.
    latin = "The quick brown fox jumps over the lazy dog"
    assert tokenize(latin) == normalize_text(latin).split(" "), tokenize(latin)
    # CJK scoring sanity: one wrong char of four -> high but <1 lev/f1.
    rc = score_all("春天来了", "春天来啦")
    assert 0.0 < rc["f1"] < 1.0 and 0.0 < rc["lev"] < 1.0, rc
    assert rc["n_ref"] == 4 and rc["n_hyp"] == 4, rc

    # 6. Disjoint vocab -> content zero but order not double-penalized.
    r = score_all("x y z", "a b c")
    assert r["f1"] == 0.0 and r["jaccard"] == 0.0, r
    assert r["order"] == 1.0, r  # no shared tokens -> order undefined -> 1.0

    # 7. Empty handling.
    r = score_all("", "")
    assert r["lev"] == 1.0 and r["f1"] == 1.0 and r["order"] == 1.0, r
    r = score_all("", "a b c")
    assert r["lev"] == 0.0 and r["f1"] == 0.0 and r["recall"] == 0.0, r

    # 8. Order metric quantitative sanity: a clean block-swap of two halves.
    ref = "1 2 3 4 5 6"
    hyp = "4 5 6 1 2 3"
    o = order_score(hyp, ref)
    # Best order-preserving alignment keeps one half (3 of 6 shared) -> 0.5.
    assert abs(o - 0.5) < EPS, o
    c = content_scores(hyp, ref)
    assert abs(c["f1"] - 1.0) < EPS and abs(c["jaccard"] - 1.0) < EPS, c

    print("score.py self-test OK")


if __name__ == "__main__":
    _selftest()
