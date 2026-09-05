# corpus-diff — word-boundary差分回路

对同一批 PDF 跑 pdfspine 与 PyMuPDF，对齐词序列，统计 **over-split**（我们把一个词拆碎）
与 **under-split**（我们该拆没拆），并把每个事件关联到字体/内容流特征。

`get_text` 的词切分改动（`WORD_GAP_FRAC`、tracking 扣减、span 聚合等）**极其容易**在某类
PDF 上悄悄退化，单元测试看不见 —— 这套回路就是用来兜住它的。

## Clean-room 边界（重要）

PyMuPDF 是 AGPL，本仓库是 Apache-2.0。按 `conformance/REPORT.md` 与 `docs/BENCHMARKS.md`
的既有声明：**oracle 只在本地当差分参照运行，其输出永不提交** —— 只提交相似度/统计数字。

因此 `out/`、`summary*.json`、`corpus.txt` 全部**不入库**（见 `.gitignore`）。
本目录只有驱动脚本。

## 用法

```bash
# 0) 准备语料（仓库自带取语料脚本；born/CJK/Arabic 是合成的，完全可再生）
python conformance/fetch_corpus.py            # Tier-1 → fixtures/corpus/
python conformance/gt/fetch_eurlex.py         # EUR-Lex
python conformance/gt/fetch_govinfo.py        # govinfo
python conformance/gt/born_digital.py         # born-digital（合成）

# 1) 生成 corpus.txt（每行一个 PDF 绝对路径；机器之间不通用，各自生成）
python conformance/corpus-diff/build_corpus.py

# 2) 两侧抽取（各自最多 180s/文件；已存在的 json 会跳过，重跑前先把 out/<engine> 挪走）
python conformance/corpus-diff/run_engine.py fitz     /path/to/python-with-pymupdf
python conformance/corpus-diff/run_engine.py pdfspine /path/to/python-with-pdfspine

# 3) 差分 + 归类
python conformance/corpus-diff/compare.py     # → summary.json + stdout 汇总表
python conformance/corpus-diff/classify2.py   # → over-split 事件按机制归类
```

`compare.py` 的 stdout 汇总表就是验收口径；`classify2.py` 把 over-split 分成
`newline_cut`（同一词被切成两行）/ `space_insert`（text 模式也插了空格）/
`words_only`（text 模式完整、仅 words 模式拆）/ `unknown`，并与
`stream_flags.json`（`Tc≠0`、Descent 符号等页级特征）交叉。

## 已知基线

2026-09 在 300 文档语料上：**over 334/83、under 332/160**。
改动 `get_text` 词切分相关逻辑后应复跑，**不得退化**。

## 注意

- `run_engine.py` 会跳过已存在的输出 json，所以复跑前要把 `out/<engine>/` 挪走，否则拿到的是旧数据。
- fitz 侧的缓存与语料是一一对应的：换了语料就必须重跑 fitz 侧，不能沿用别的机器的缓存。
