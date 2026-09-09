# corpus-diff — word-boundary差分回路

对同一批 PDF 跑 pdfspine 与 PyMuPDF，对齐词序列，统计 **over-split**（我们把一个词拆碎）
与 **under-split**（我们该拆没拆），并把每个事件关联到字体/内容流特征。

`get_text` 的词切分改动（`WORD_GAP_FRAC`、tracking 扣减、span 聚合等）**极其容易**在某类
PDF 上悄悄退化，单元测试看不见 —— 这套回路就是用来兜住它的。

## Clean-room 边界（重要）

PyMuPDF 是 AGPL，本仓库是 Apache-2.0。按 `conformance/REPORT.md` 与 `docs/BENCHMARKS.md`
的既有声明：**oracle 只在本地当差分参照运行，其输出永不提交** —— 只提交相似度/统计数字。

因此工作输出 `out/`、本目录根部的 `summary*.json`、`corpus.txt` 全部**不入库**
（见 `.gitignore`）。仓库只保留驱动脚本，以及 `baselines/` 下不含 oracle 原始输出的
可移植语料清单和聚合验收值。

## 用法

```bash
# 0) 准备语料（仓库自带取语料脚本；born/CJK/Arabic 是合成的，完全可再生）
python conformance/fetch_corpus.py            # Tier-1 → fixtures/corpus/
python conformance/gt/fetch_eurlex.py         # EUR-Lex
python conformance/gt/fetch_govinfo.py        # govinfo
python conformance/gt/born_digital.py         # born-digital（合成）

# 1) 精确重建已冻结语料；缺文件或 SHA-256 不同会立即失败
python conformance/corpus-diff/build_corpus.py \
  --manifest conformance/corpus-diff/baselines/glyph-geometry-2026-09-05b-manifest.json

# 仓库内 fixture 被有意改动（sha256 过期）时：不要手改旧 manifest，用 --refresh-stale
# 冻结一份继承旧文件的新 manifest（只更新过期条目，旧哈希留在 previous_*，顶层 supersedes 指回旧文件）
python conformance/corpus-diff/build_corpus.py \
  --manifest conformance/corpus-diff/baselines/glyph-geometry-2026-09-05b-manifest.json \
  --refresh-stale \
  --freeze conformance/corpus-diff/baselines/glyph-geometry-<date>-manifest.json

# 只有有意刷新基线时才按目录重新选样，并写出新的可入库 manifest
python conformance/corpus-diff/build_corpus.py --cap 300 \
  --root fixtures \
  --root conformance/gt/corpus-born \
  --root conformance/gt/corpus-cjk \
  --root conformance/gt/corpus-arabic \
  --root conformance/gt/corpus-eurlex \
  --root conformance/gt/corpus-robustness \
  --root conformance/gt/corpus-fintabnet \
  --freeze conformance/corpus-diff/baselines/new-manifest.json

# 2) 两侧抽取（各自最多 180s/文件；已存在的 json 会跳过，重跑前先把 out/<engine> 挪走）
python conformance/corpus-diff/run_engine.py fitz     /path/to/python-with-pymupdf
python conformance/corpus-diff/run_engine.py pdfspine /path/to/python-with-pdfspine
# 第三个参数可选，可把同一 engine 的不同版本隔离到不同输出目录
python conformance/corpus-diff/run_engine.py pdfspine /path/to/baseline-python baseline

# 3) 差分 + 归类
python conformance/corpus-diff/compare.py     # → summary.json + stdout 汇总表
python conformance/corpus-diff/compare.py fitz baseline summary-baseline.json
python conformance/corpus-diff/stream_flags.py
python conformance/corpus-diff/font_extra.py
python conformance/corpus-diff/classify2.py   # → over-split 事件按机制归类

# 4) F 阶段：量测当前 span 内已合并 glyph seam（原始结果仍不入库）
python conformance/corpus-diff/sample_span_seams.py  # → span-seams.json
```

`compare.py` 的 stdout 汇总表就是验收口径；`classify2.py` 把 over-split 分成
`newline_cut`（同一词被切成两行）/ `space_insert`（text 模式也插了空格）/
`words_only`（text 模式完整、仅 words 模式拆）/ `unknown`，并与
`stream_flags.json`（`Tc≠0`、Descent 符号等页级特征）交叉。

`baselines/glyph-geometry-2026-09-05b-manifest.json` 是当前可入库的精确 300 文档清单；每项
记录仓库相对路径、source ID、上游 ID、大小和 SHA-256，顶层记录来源 URL/生成器及版本。
它由 `--refresh-stale` 从 `glyph-geometry-2026-09-05-manifest.json` 派生：`32e6232` 改了
`fixtures/typeset/typeset-lo-slide.pdf`，只有这一条的 sha256/size 更新（旧值保留在该条目的
`previous_sha256` / `previous_size`），其余 299 条逐字节相同；顶层 `supersedes` 记录旧文件名和
旧指纹 `87804b5a…`。旧 manifest 原样保留，仍是 `*-summary.json` 与 C–G 报告所引用的语料指纹
（用 `git show 32e6232^:fixtures/typeset/typeset-lo-slide.pdf` 可取回旧 fixture 复核）。
同目录的 `*-summary.json` 保存可入库的聚合验收值，不包含 oracle 原始输出。

## 已知基线

2026-09 在 300 文档语料上记录过：**over 334/83、under 332/160**。原始清单、清单指纹、
斜杠两侧的版本映射和 oracle 版本均未随报告保留，因此该数字不能作为新清单的逐值验收线。
新运行以同一指纹清单上的 baseline/current/oracle 三侧对照为准。
改动 `get_text` 词切分相关逻辑后应复跑，**不得退化**。

`glyph-geometry-2026-09-05-summary.json` 的 `word_boundary` 是旧指纹 `87804b5a…` 上的 C 阶段
验收值。2026-09-05 在派生的 `…-05b` 清单上用 PyPI `pdfspine==0.7.1` wheel 对 PyMuPDF 1.28.2
复跑，全部聚合值逐项相同：over 137（alpha 73 / punct 17）、under 329（alpha 184）、mixed 130、
1887 页、1246 content-equal 页、双侧 0 错误；与本机 post-G 缓存逐文档对比 299/300 JSON 一致，
仅 `typeset-lo-slide.pdf` 的词框坐标随行距变化（text 与词串不变）。

## 注意

- `run_engine.py` 会跳过已存在的输出 json，所以复跑前要把 `out/<engine>/` 挪走，否则拿到的是旧数据。
- fitz 侧的缓存与语料是一一对应的：换了语料就必须重跑 fitz 侧，不能沿用别的机器的缓存。
