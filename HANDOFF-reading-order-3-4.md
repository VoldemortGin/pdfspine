# HANDOFF — 阅读顺序阶段 3/4（2026-09-05 晚，因配额收尾，状态 WIP）

分支 `worktree-agent-ae07f5282e4af72f5`，基于 main `21d636a`。**没有任何代码改动被提交**：
配额告罄时实现 agent 尚未开始编辑（`git diff` 为空），因此本文件是唯一产物。
证据与脚本在 `/Volumes/ExternalSSD/tmp/ro34/`（不入库），上一轮资产在 `/Volumes/ExternalSSD/tmp/readorder/`。

## 1. 基线复现（冻结 wheel = main `21d636a`，oracle PyMuPDF 1.28.2）

| 语料 | pdfspine | fitz | 与任务给定基线 |
|---|---|---|---|
| PMC 7 篇 lev / f1 / jaccard / order | 0.7439 / 0.7808 / 0.6101 / 0.9600 | 0.7445 / 0.7809 / 0.6125 / 0.9605 | 逐位一致 |
| PMC212689 lev / order | 0.7003 / 0.7456 | 0.7050 / 0.7492 | 逐位一致 |
| born 6 篇 lev / order | 0.9803 / 1.0000 | 同 | 逐位一致 |
| EUR-Lex 40 篇 | **未跑完**（23:22 启动，收尾时被终止） | — | 任务给定 0.9372 / 0.9773（fitz 0.9396 / 0.9800） |
| govinfo FR 页眉误排 / 碎片化页 / 均块数 | 64/2492 / 530 / 1.58（`ro34/fr-header-base.json`） | 64/2517 / 0 / 1.02（`ro34/fr-header-fitz.json`） | 逐位一致 |
| 300 文档 digest | **未跑完**（只生成 49/300 篇即被终止，已删除，必须用 `.venv-base` 重新生成） | — | — |

结论：pmc/born/FR 基线在本 worktree 逐位复现；eurlex 与 digest 基线需按 §5 的命令补跑（约 30 min）。

诊断 agent 被中止前核实的一条**关键事实**：FR 的 64 个误排页在 pdfspine 与 fitz 上是**同一集合**（交集 64，双方各自独有 0），
即"误排 ≤ 64"已经是平局、不是 pdfspine 的失分；FR 上真正的差距只有两项——页眉带碎片化（530 页 vs 0，均块数 1.58 vs 1.02）
与页眉检出页数少 25（2492 vs 2517，碎片匹配不到正则）。两者都指向 D4（§6），不指向块序。检出少的 25 页是单向的（pdfspine 独有集为空），约每期 2 页且每对里有一页是 page index 2
（各期封面带页）：页眉块被排到 `y < 60` 带之下或被合并，文本并未丢失（集合文件 `ro34/diag/t3_*.json`）。EUR-Lex `32008L0048_DE`（27 页，
0.965 vs 0.983）与 PMC212689（p0 pdfspine 22 个文本块 vs fitz 18，碎片更多）的逐页定位未完成。

## 2. 环境与脚本（全部已写好，可直接复用）

- 构建：`export CARGO_TARGET_DIR=/Volumes/Cargo/target/pdfspine-ro34 CARGO_BUILD_JOBS=4 TMPDIR=/Volumes/ExternalSSD/tmp; unset CONDA_PREFIX`
- venv（收尾时已删除，需重建）：`python3.12 -m venv .venv-task && . .venv-task/bin/activate && pip install "maturin>=1.12,<2" pytest hypothesis "ruff==0.14.14" pymupdf ocrspine-models && maturin develop --release`
- `ro34/build_all.sh`：建 `.venv-task` + 从 HEAD 打冻结 wheel 到 `ro34/wheels-base/`（wheel 仍在）并装进 `.venv-base`。
  重建 `.venv-base` 只需：`python3.12 -m venv .venv-base && .venv-base/bin/pip install pytest hypothesis ocrspine-models pymupdf ro34/wheels-base/pdfspine-*.whl`。
- `ro34/mk_variant_venv.sh <tag>`：从 worktree 当前源码打 release wheel 到 `ro34/wheels-<tag>/`，装进 `.venv-<tag>`，并把 `git diff` 存为 `ro34/variant-<tag>.diff`。
- `ro34/score.sh <venv-python> <tag> [gt|fr|digest|all]`：GT（pmc, born, eurlex）→ `ro34/gt-<c>-<tag>.{json,md}`；FR 页眉 → `ro34/fr-header-<tag>.json`；300 文档三投影摘要 → `ro34/digest-<tag>/`。
- `ro34/summarize.py <tag> [<tag2>]`：语料均值表 + PMC212689 + FR 摘要；给第二个 tag 时输出逐篇 Δ（|Δ| ≥ 0.0001）。
- `ro34/ro_compare.py <digest-a> <digest-b> [out.json]`：300 文档逐页差分（纯块置换 / 块集合变化）；`ro_threeway.py` 三方归因（需改路径常量 `RO`）。
- 设计规格全文（给实现 agent 的 spec）：`ro34/stage3-spec.md`、`ro34/stage4-spec.md`。

## 3. 阶段 3 设计（已定稿，未实现）—— 几何 band → column → y

以 `layout.rs` 为准，函数名对应现有代码（`group_blocks_columned` 1800、`cut_lines` 2235、`emit_column_cut` 2299、`partition_spanning` 2332、`find_column_cut` 2420）。

1. **`find_column_cut` 改按整条谷带分类**。`column_gutter` 返回 `(宽度, 中点)`，取 `lo = at − w/2, hi = at + w/2`：
   `x0 < lo − 0.5 && x1 > hi + 0.5` 才是 spanning（覆盖整条谷带）；否则按 bbox 中心 ≤ at 归左/右。
   动机：OJ 右栏序号 "(32)" 起点落在谷带内（`column_gutter` 的 tol 放行），旧的中点判据把它判成 spanning；
   新判据下它是右栏行。这一条取代上一轮"逐行判据"失败的原因（root-cause 文档变体 c）。
2. **`cut_lines`：合法 column cut 永远优先于 band cut**。删除 `prefer_x = xg >= yg`。理由：column cut 有结构性保证
   （两侧 substantial、跨谷行 ≤ 10 %），band gap 只是空白宽度；段落间距宽于栏距时旧逻辑把双栏横切成 band
   （`32011L0083_BG p10`）。上下两半栏结构不同的页，跨谷行超过 tol → 没有 column cut → 仍先 Y-cut。
   `cut_lines` 不再返回 `bool`（"根是否 X-cut"判别式连同其唯一消费者一起删除）。
3. **`emit_column_cut`：spanning band 划分行**（取代 `partition_spanning` 与三条过滤规则、
   `SPANNING_COLUMN_LINE_MIN_WIDTH_FRAC`、`SPANNING_MARGIN_ROW_MAX_HEIGHT`）。
   spanning 行按 `split_y_bands`（min_gap = 1.3·typ_h）分成 band（自上而下）；每条栏行的行号 = 它位于其下方的 band 数
   （`line.y0 ≥ band.y1 − 0.5·line.height` 计一次；与 spanning 碎片同一行的栏行归上一行，FR 页眉碎片因此聚在页首）。
   发射顺序：row 0 左、row 0 右、band 0、row 1 左、row 1 右、band 1 … 。页眉/页脚天然是 row 0 为空 / 末行为空的 band。
   同时用 `const SPANNING_BANDS_PARTITION_ROWS: bool` 保留 **float 语义**分支（中间 band 夹在整根左栏与整根右栏之间，
   即现状），一行翻转即可打第二个 wheel。
4. **`group_blocks_columned`：region 全部原子、按 XY-cut 的 DFS 序**。删 `regions_are_side_by_side`、
   `COLUMN_REGION_OVERLAP_FRAC`、`side_by_side`、`order_groups`、`root_column_cut`；region 内仍 `sort_by_key(seq)`（阶段 4 才改）。
   `is_table_dominant` 的单 region 路径不变。更新 `order_blocks` 与 `model.rs` `Block::seq` 的 doc comment。
5. **测试**：`readorder_008` 只改注释；新增 `readorder_009`（谷带内起笔的 "(32)" 留在右栏）、`readorder_010`
   （通栏标题划分两行：L上→R上→标题→L下→R下）、`readorder_011`（无栏结构页、页脚先画 → 几何序）、
   `readorder_012`（标题 band 后画 + 段距 > 栏距的双栏 → 标题→整左栏→整右栏）。
   `PYTEXT-010`（`python/tests/test_text.py:463`）改为 `Right < Left < Bottom`（顶 band 先、共基线块内保留绘制序），
   `blocks`/`sorted` 断言不动。`LAYOUT-ORDER-002`、`compat_block_*`、`layout_column_regression_*` 预期原样通过。

### 目标语义的取舍（已决定，可被数据推翻）

- **全页几何序**（含无栏结构页），绘制序只保留在"同一 block 内共基线片段"（root-cause 文档 C1）。
  代价：`sort=False` 在页脚先画的单栏页上与 fitz 不同；300 文档 digest 会出现大量纯置换页，需归因为"绘制序 ≠ 几何序"。
  备选（root-cause 文档阶段 2 建议）："无 column cut 的页保留绘制序"——若 digest 变化面不可接受可退到此项。
- **中间 spanning band 的语义未定，靠数据**：行划分（通栏小标题正确、期刊中部通栏图注错误）vs float（相反）。
  PMC（PLoS 三栏、图注被 GT 排除）与 EUR-Lex（OJ 中间通栏行极少）会给出答案。

## 4. 变体计划与判断标准（最多 3 个）

| 变体 | 内容 | 建法 |
|---|---|---|
| V1 | §3 全部，`SPANNING_BANDS_PARTITION_ROWS = true` | 实现 spec → `mk_variant_venv.sh v1` |
| V2 | 同 V1 但 `= false`（float） | 翻 const → `mk_variant_venv.sh v2` |
| V3（仅当 V1/V2 互有胜负） | 单行 band 划分行、多行 band float | 小改 `emit_column_cut` |

判断标准（硬约束，逐篇列出 ≤ 0.005 的退化并解释）：PMC order ≥ 0.9600 且目标 ≥ 0.9605；PMC212689 ≥ 0.7456 目标 ≥ 0.749；
EUR-Lex lev ≥ 0.9372、order ≥ 0.9773；born 逐位不变；FR 误排 ≤ 64；300 文档变化逐类归因（`ro_compare.py` +
一个"仅原子化"或"仅新分类"的诊断 wheel 做三方对照，见 `readorder/mk_atomic_variant.sh` 的做法）。
`typeset-lo-slide.pdf` 条目已过期（另一 agent 在修），digest 会记 SHA_MISMATCH，排除并说明。

## 5. 下一步具体命令

```
# 0. 环境（≈10 min）
cd <worktree>; sh /Volumes/ExternalSSD/tmp/ro34/build_all.sh          # 或按 §2 手工重建 .venv-task/.venv-base
# 1. 补齐基线（≈30 min，可后台）
/Volumes/ExternalSSD/tmp/ro34/score.sh <wt>/.venv-base/bin/python base all
# 2. 实现阶段 3：把 ro34/stage3-spec.md 交给 developer-opus；门禁 fmt/clippy/test 全绿后
/Volumes/ExternalSSD/tmp/ro34/mk_variant_venv.sh v1 && /Volumes/ExternalSSD/tmp/ro34/score.sh <wt>/.venv-v1/bin/python v1 all
python3.12 /Volumes/ExternalSSD/tmp/ro34/summarize.py base v1
python3.12 /Volumes/ExternalSSD/tmp/ro34/ro_compare.py /Volumes/ExternalSSD/tmp/ro34/digest-base /Volumes/ExternalSSD/tmp/ro34/digest-v1 /Volumes/ExternalSSD/tmp/ro34/compare-v1.json
# 3. 翻 const 打 v2，同样打分；选优后 commit `feat(text): reading order stage 3 ...`
# 4. 阶段 4：ro34/stage4-spec.md（region 内按主方向 cross 排行、行内按 seq；保留 .abs()），单独 commit
# 5. D4：见 §6；单独 commit；无收益则回滚并记录数据
```

## 6. 阶段 4 与 D4 的设计要点

- **阶段 4**（`ro34/stage4-spec.md`）：region 内取主方向 `(wmode, dir)`，按 `paragraph_line_metrics().baseline_origin · (−dir.1, dir.0)`
  稳定排序，`LINE_TOL_FRAC × max(size)` 内视为同一行，行内按 `seq`（保住 `PYTEXT-010` 的共基线块）。
  `group_region_paragraphs` 的 `.abs()` 保留（主方向下步进恒非负，abs 已惰性；混合方向仍靠它），不按 root-cause 文档去掉。
  风险：column cut 失败的混栏叶子 region 从 seq 序变成逐行交错；阶段 3 的"column cut 优先"应使此类叶子变少，用 EUR-Lex
  `32019R0881_*`、`32011L0083_PL` 验证（变体 a 当年就在这里退化）。
- **D4（FR 页眉碎片化，530 页 vs fitz 0）**：根因在行级 `split_on_gutter`（1094）：页眉整行在栏距 x 处恰有词间空格即被切。
  方案：让 `detect_page_gutters`（950）返回谷带 `(lo, hi)` 而非中点；`split_on_gutter` 仅当本 run 自己的空白覆盖谷带
  （`gap ≥ 0.8·(hi − lo)`，即两侧 glyph 没有侵入谷带）才切。合并的 L1+R1 行 gap ≥ 谷宽会切；页眉的普通词距 ≪ 谷宽不切。
  `is_heading`（大字号）保护不变。验收：`fr-header-*.json` 的 `header_band_fragmented_pages` 与 `misplaced` 下降，
  EUR-Lex/born/PMC 不退化，`layout_e2e_003/004/005` 通过。上一轮诊断说"不要碰行级路径"是针对块序回归的定位，与此正交。

## 7. 已知风险

1. 全页几何序改变 `sort=False` 在退化页上的 fitz 兼容性；`PYTEXT-010` 必须重写，README/兼容性文档若声称"块序 = 绘制序"需同步。
2. `column_gutter` 的 tol（10 % 行数）允许的跨谷行在 V1 下每条都划分行；期刊中部通栏表行/图注可能把栏流切成行序。
3. `is_table_dominant` 根路径与两列对照表逐行读取（`aaee2a9`）不受影响，但阶段 4 的行排序会让"按列绘制的表"变行序（预期正确，需 `test_tables`/`compat_block_008/010` 验证）。
4. 残差诊断（EUR-Lex `32008L0048_*` 四篇各 −0.02 order、PMC212689 −0.0036、FR 剩余 64 页 p1/p4/p5）尚未做出结论，
   诊断 agent 被中止时只写了 `ro34/diag/_probe.py`；这三项决定阶段 3 之后还差什么，建议先做。
