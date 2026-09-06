# pdfspine 阅读顺序：根因与根治方案

诊断日期 2026-09-03 · 代码基线 `75a1ace`（v0.6.1）；仓库 HEAD 已前进到 `db8bfe9`（**仅 `conformance/` 下的分数刷新，无代码改动**，故本文结论不受影响）· 本文只做设计，未改任何实现代码

---

## 0. 一句话

`b6c027a`（2026-08-03，*fix(compat): align page order and text extraction with PyMuPDF*）在一次
13 文件 `+1786/−243` 的大杂烩提交里，把块排序从**几何序**静默换成了 **content-stream 绘制序**：

```diff
- region_lines.sort_by(|a, b| a.bbox.y0.total_cmp(&b.bbox.y0));
+ region_lines.sort_by_key(|line| line.seq);
```

commit message 对此**只字未提**，也没有任何 before/after 指标。结果是：几何 XY-cut 仍然正确地切出了
栏区域，但这些区域之间的先后改由"谁先被画"决定。在"绘制序 ≠ 阅读序"的排版上（多内容流拼版、
翻转 CTM、右栏先画）直接产出错误的阅读顺序。

**这不是"我们不如 PyMuPDF"，而是一次有明确引入点的回归。**

---

## (a) 现状：几何序与绘制序在代码里的分布

### A1. 用几何序的路径

| 位置 | 内容 |
|---|---|
| `layout.rs:531` `group_lines` | glyph → line：基线聚类（`LINE_TOL_FRAC=0.5`），cluster 内按前进轴排序 |
| `layout.rs:854` `detect_page_gutters` | 页面级竖带检测（1pt 分箱占用直方图，`min_gap = max(0.4·typ_size, 2.0)`） |
| `layout.rs:998` `split_on_gutter` | 一条基线 run 按竖带切成每栏一段 |
| `layout.rs:1900` `cut_lines` | 递归 XY-cut，产出 region 列表 |
| `layout.rs:1990` `find_column_cut` | 列切口（`min_x_gut = max(1.2·typ_h, 0.03·region_w)`） |
| `layout.rs:2059` `column_gutter` | 扫描线覆盖山谷（`tol = max(⌊0.1·行数⌋, 1)`） |
| `layout.rs:2165` `widest_y_gutter` | 横带间隙（`REGION_BAND_GAP_FRAC = 1.3`） |
| `tables.rs:478` | 表格排序：**纯 `(y0, x0)`**，完全独立的平行管线 |
| `py-bindings/src/lib.rs:427` `sorted_plain_text` | `sort=True` 时按 `(y0, x0)` 重排 line |
| `py-bindings/src/lib.rs:516/527` | `sort=True` 时 blocks/words 按 `(y, x)` 重排 |

### A2. 用绘制序（`seq`）的路径 —— 冲突就在这里

`Line.seq` = 该行**最早绘制**的 glyph 索引；`Block.seq` = 块内最小 line seq（`make_text_block`, 2239）。

**冲突点 1 — `layout.rs:1600`（region *内部*）**

```rust
region_lines.sort_by_key(|line| line.seq);
let mut region_blocks = Vec::new();
group_region_paragraphs(region_lines, &mut region_blocks);
```

作用：决定**同一个 region 内行的先后**，进而喂给 `group_region_paragraphs`(1720) 做段落切分。

隐患：`group_region_paragraphs` 计算 `baseline_step` 时取了 `.abs()`（1749-1751），**向上跳与向下跳
无法区分**。所以栏内绘制顺序倒错的 PDF，不仅行序错，段落切分也会跟着错（倒序被当成正常段内步进）。

**冲突点 2 — `layout.rs:1618`（region *之间*）—— 主犯**

```rust
if side_by_side[region_index] {
    let order_key = region_blocks.iter().map(|b| b.seq).min().unwrap_or(usize::MAX);
    order_groups.push((order_key, region_blocks));          // 并排栏：原子组，key = min(seq)
} else {
    order_groups.extend(region_blocks.into_iter().map(|b| (b.seq, vec![b])));   // ← 独立真 bug
}
order_groups.sort_by_key(|(order_key, _)| *order_key);
```

两处问题：

1. **region 之间按绘制序排**。`cut_lines` 几何切分的成果在这一行被丢弃。
2. **非 `side_by_side` 的 region 被拆成逐块**参与全局排序 —— 一根柱子会被邻柱**逐行插花**。
   这是一个与主犯独立的真 bug，`side_by_side` 判据（`regions_are_side_by_side`, 1711，
   `COLUMN_REGION_OVERLAP_FRAC = 0.5`）只覆盖"水平不相交且垂直重叠 ≥50%"的情形，漏网面很大。

**冲突点 3 — `layout.rs:2292` `order_blocks` 的文档谎报**

```rust
fn order_blocks(blocks: &mut [Block]) {
    for (i, b) in blocks.iter_mut().enumerate() { b.number = i; }   // 只编号，不排序
}
```

它的 doc comment 说"[`group_blocks_columned`] performs the content-sequence sort … Image blocks are
appended afterwards, matching the prior `seq == usize::MAX` behavior"。但图像块在
`textpage_core:279-294` 于 `group_blocks_columned` **返回之后**才 push，**从未进入 `order_groups`**，
`seq = usize::MAX` 实际从未参与过任何排序。`model.rs:296` 的 `Block::seq` 文档同样错误。

### A3. `cut_lines` 产出的 region 顺序 —— 半对

递归遍历本身是深度优先、左优先/上优先：X-cut 先左后右（1929/1933）、
`split_y_bands`(2190) 先按 `y0` 排序故 Y-cut 严格 top-first、叶子 push 全在正确位置。

**唯一结构性错位：`spanning` 被 push 在 left 和 right 之间**

```rust
// cut_lines:1929-1933（cut_column_subtree:1974-1978 同构）
cut_column_subtree(lines, &left, width, height, out);
if !spanning.is_empty() {
    cut_spanning(lines, &spanning, width, height, out);      // ← 通栏页眉/页脚夹在两栏中间
}
cut_column_subtree(lines, &right, width, height, out);
```

通栏页眉本该在两栏**之前**，通栏页脚本该在两栏**之后**。目前被 1618 的 seq 排序完全掩盖，是死代码。

### A4. 一条被实测证伪的直觉

> "regions 顺序就是阅读顺序，seq 排序是掩盖 spanning bug 的死代码，删掉即可。"

**错。seq 排序在双栏页上是承重墙。** 实测（Python 逐行移植 `cut_lines` 全家，
在 EUR-Lex 2816 页上验证移植保真度：32014R0596_EN 710 个 region 只有 1 个跨块）：

`cut_lines` 顶层选轴的判据是 `prefer_x = xg >= yg`（1916-1920）—— **段落间距经常宽于栏间白带**，
于是双栏页顶层走 Y-cut，把整页横切成 band，再在每个 band 内 X-cut，
产出 **L-R-L-R 带状交错**的 region 序列。

```
32011L0083_BG p10  x_gut=23.6  y_gut=24.3  TOP_AXIS=Y
   header(seq4419) | 左上(0) | 右上(2135) | 左下(1498) | 右下(3575)
PMC212689 p1       x_gut=29.3  y_gut=None  TOP_AXIS=X
   x0=54(seq33) | x0=228(seq1394) | x0=402(seq0)     ← 最右栏先画
```

**两个语料的差别恰好落在顶层轴向上**：PMC 顶层 X-cut（region 序可信、seq 乱），
EUR-Lex 顶层 Y-cut（region 序不可信、seq 是唯一救星）。

实测代价（EUR-Lex 40 篇 / 2816 页）：

| 变体 | mean lev | mean order |
|---|---|---|
| main | **0.9248** | **0.9771** |
| 单纯 region 保序 | 0.9121 | 0.9639 |
| region 保序 + spanning 分区 | 0.8907 | 0.9415 |

32011L0083 全家崩塌：_EN lev −0.1098、_IT −0.0861、_BG −0.0633、_EL −0.0630、_FR −0.0616。

---

## (b) PyMuPDF 的实际语义（凡标注"oracle 实测"者均在 PyMuPDF 1.27.2 上验证过）

### B1. `sort=False`（默认）—— 不是纯绘制序，也不是纯几何序

- **oracle 实测**：`PYTEXT-010` 的合成页（绘制序 `Bottom(y=100) → Right(y=700,x=400) → Left(y=700,x=72)`）
  PyMuPDF 返回 `'Bottom block\nRight block\nLeft block\n'` —— **绘制顺序**。
  且 `Right`/`Left` 共享基线，被合并进**同一个 block**，所以排 block 元组也救不回来。
- **oracle 实测**：`PMC176547.pdf`（6 页合刊，8 个内容流 + 翻转 CTM 拼版，绘制序 ≠ 阅读序）
  PyMuPDF 的 order = **1.0000**，而我们是 0.5214 —— fitz 在这里**做了几何重排**。
- **oracle 实测**：EUR-Lex / PLoS 双栏与三栏页，fitz 稳定输出"整栏连续、栏按几何左→右"。

**结论：MuPDF 的 stext 在构建 block/line 时沿绘制顺序累积，但随后跑了 segmentation /
reading-order pass；该 pass 在有可辨识的页级栏结构时做几何重排，在退化页（如那个三块合成页）
上保留累积顺序。** 我们没有逐行核对 MuPDF 源码来精确刻画触发条件——这是本文档最大的未知项，
也是 (e) 阶段 2 的前置调研任务。

### B2. `sort=True`

PyMuPDF 按 block bbox 的 `(y0, x0)` 全局重排。我们已对齐（`sorted_plain_text` 按 `(y0,x0)` 排 line；
blocks/words 按 `(y,x)` 排元组）。`PYTEXT-010` 后半段断言
`Left < Right < Bottom` 在两边都通过。**这一路没有问题，本次不动。**

### B3. 我们自己的测试套件里，两条断言互相矛盾

| 测试 | 位置 | 锁死什么 | 几何序能否通过 |
|---|---|---|---|
| `PYTEXT-010` | `python/tests/test_text.py:444` | `sort=False` 必须还原**绘制序**（`Bottom < Right < Left`） | **否** |
| `LAYOUT-ORDER-002` | `crates/pdf-text/tests/layout_unit.rs:340` | 按 **row-major 绘制**（L1,R1,L2,R2）却断言 **column-major 输出**（L1,L2,R1,R2） | **是** |

两条都是 `b6c027a` 新增/改写的。**同一个 commit 同时锁死了"必须是绘制序"和"必须是几何列序"**
—— 这说明当时并没有形成统一的目标语义。这也是本次回归能潜伏 4 个月的根本原因：
没有任何一条测试表达"多栏页的栏序必须几何正确"。

---

## (c) 建议的目标语义

核心主张：**把"结构组装"与"阅读顺序"彻底分层，绘制序只允许出现在最底层。**

| 层次 | 目标语义 | 现状 | 是否需改 |
|---|---|---|---|
| glyph → line | 纯几何（基线聚类 + 前进轴排序） | 几何 | 否 |
| line → block（段落切分） | 纯几何（基线步进 + 缩进），**带符号**，不取 `.abs()` | 依赖 seq 输入序 | **是** |
| block 在 region 内的先后 | 几何 `y`（top-first） | `seq` | **是** |
| region 之间的先后 | 几何阅读顺序 `band → column → y` | `seq` | **是** |
| 同基线相邻 cell 是否合并成一个 block | 保持现状（fitz 兼容） | 已对齐 | 否 |
| `sort=True` | 全局 `(y0, x0)` 重排 | 已对齐 | 否 |

### C1. 绘制序唯一应当保留的位置

**同一条基线上、同一个 block 内部**的片段先后。这是 `PYTEXT-010` 真正在测的东西
（`Right`/`Left` 共享基线且合并进同一 block），也是 fitz 的实际行为。
把绘制序限制在这一层，`PYTEXT-010` 与几何阅读顺序就不再冲突。

### C2. 需要事先定夺的产品问题

pdfspine 的既定目标是 **PyMuPDF 兼容**。那么当"fitz 的行为"与"版面上正确的阅读顺序"分歧时，
以哪个为准？三个选项：

- **选项 1 · 复刻 MuPDF 的 segmentation**。兼容性最好，但需要先逐行读 MuPDF 的
  `stext-para.c` / `stext-device.c`（源码已抓到 scratchpad）。工作量最大，结果最可预期。
- **选项 2 · 自研几何阅读顺序，接受与 fitz 的边界差异**。工作量中等，但需要重写 `PYTEXT-010`
  并显式记录"已知差异"，且每个差异都要有语料证据支撑。
- **选项 3 · 维持现状 + 打补丁**（如已验证的"顶层 X-cut 页用几何序、其余用 seq"）。
  见 (e) 阶段 1.5。工作量最小，但判别式 keys on 内部实现细节，长期会漂移。

**我的建议：选项 1**，理由是 `b6c027a` 的教训恰恰是"自己发明语义又不写下来"。
复刻 MuPDF 至少让"正确"有一个可查证的定义。

---

## (d) 需要改的函数 / 需要重写的测试

### D1. 函数

| 函数 | 位置 | 改什么 |
|---|---|---|
| `cut_lines` | `layout.rs:1900` | ① `spanning` 按 y 拆 above/middle/below，above 排两栏前、below 排两栏后；② **X/Y 优先级**：某层能做出合法 column cut 时，禁止祖先层用 Y-gutter 横切两栏（现状 `prefer_x = xg >= yg` 只是间隙宽度比大小，不是结构性保证） |
| `cut_column_subtree` | `layout.rs:1959` | 同 ①（与 `cut_lines` 同构，必须同步改） |
| `find_column_cut` | `layout.rs:1990` | 当 `spanning` 在纵向真正分割了区域时**拒绝 X-cut**，让 Y-cut 接手 |
| `group_blocks_columned` | `layout.rs:1550` | 核心：region 间排序键改为几何；**所有 region 一律原子**（修掉"非 side_by_side 被拆成逐块 seq"的真 bug） |
| `group_region_paragraphs` | `layout.rs:1720` | `baseline_step` 去掉 `.abs()`（1749-1751），区分向上/向下跳 |
| `regions_are_side_by_side` | `layout.rs:1711` | 连同 `COLUMN_REGION_OVERLAP_FRAC`(102) **删除**（其唯一目的是阻止 seq 排序交错两栏） |
| `order_blocks` | `layout.rs:2292` | 修正 doc comment（它不排序；图像 `seq` 从未参与排序） |
| `Block::seq` 文档 | `model.rs:296` | 同上，图像默认 `usize::MAX` 并非"驱动排序" |

**不要碰**：`split_on_gutter`(998) / `detect_page_gutters`(854) 这条**行级**路径。
`2451922` 只在这里加了 `touching` 判据（`WORD_GAP_FRAC = 0.15`），与块序无关，
且实测使 PMC212689 **+0.0009**。它是无辜的。

### D2. 测试

**必须先统一的两条矛盾断言：**

- `PYTEXT-010`（`python/tests/test_text.py:444`）—— 按 (c)/C1 收窄为
  "同基线同 block 内部保留绘制序"，页级块序改为断言几何顺序。
  **注意：现有断言在 oracle 上是真的**，重写前必须先确认 MuPDF 在该页究竟为何不重排
  （是"没有可辨识栏结构"还是别的条件），否则会写出一条与 fitz 不符的新断言。
- `LAYOUT-ORDER-002`（`layout_unit.rs:340`）—— 已经是目标语义（row-major 绘制 → column-major 输出），
  **保留，并提升为规格性测试**。

**需要复核、预期不挂但必须跑的：**
`readorder_001`~`005`、`cropclip_001`~`003`（`reading_order_round1.rs`）；
`layout_e2e_003/004`（gutter 断行，与本次正交）、`layout_e2e_005`（列优先，目标语义下更强）；
`layout_column_regression_001`~`004`（`layout.rs` 内联 `mod tests`）；
`compat_block_*` 全族（压在 `group_region_paragraphs` 路径上）。

**必须新增的（现在一条都没有）：**

1. **三栏 + 内容流逆序**（先画第三栏），断言输出 A→B→C。这正是 PMC212689 的形态，
   **不锁住必然再次退化**。
2. **通栏页眉 + 两栏 + 通栏页脚**，断言 header → 左栏 → 右栏 → footer。锁 `spanning` 分区。
3. **顶层 Y-cut 的双栏页**（段落间距 > 栏间距），断言仍是整栏连续。锁 X/Y 优先级。
4. **翻转 CTM / 多内容流拼版页**，断言几何序。这是 fitz 赢 PMC176547 的机制。

**测试可见性陷阱**：`fixtures/corpus/` 里的 PLoS / cdc-mmwr / Federal Register
**`cargo test` 完全看不见**，只在 `conformance/` 与 `python/tests/test_longtail1{0,2}.py` 跑得到。
新增测试必须做成 `tests/common/mod.rs` 的**合成 fixture**，否则等于没锁。

---

## (e) 分阶段实施与验证口径

### 阶段 0 · 修语料（前置，不改代码）

PMC 12 篇里 **5 篇配对错误**，不修则所有 order 数字都不可信：

| 文档 | 问题 |
|---|---|
| `PMC176547` / `PMC193606` / `PMC193607` / `PMC212688` | 抽取文本 md5 **完全相同**（`a0a40d14353a`，29814 字符，6 页）——同一份 *PLoS Biology* Vol1 Iss1 *Research Digest* 合刊，四个 nxml 各只是其中一篇 synopsis |
| `PMC176548` | 另一份合刊，同类问题（GT 2715 字符 vs 抽取 29731，f1 仅 0.173） |

成因：`pmc_fetch.py` 对 synopsis 类 PMC ID 下载了整本合刊 PDF。

**动作**：修 `pmc_fetch.py`；把这 5 篇移出 head-to-head 统计。

**为什么关键**：剔除后，干净 7 篇的差距 100% 集中在一篇上——

```
order  Δ合计 −0.1497   PMC212689 = −0.1498 (100.1%)   其余 6 篇 = +0.0001
lev    Δ合计 −0.1414   PMC212689 = −0.1420 (100.4%)   其余 6 篇 = +0.0006
```

**干净篇目上 pdfspine 与 PyMuPDF 阅读顺序打平。** 原报告"输 6 篇、最惨 −0.4786"是坏语料的伪影。

### 阶段 1 · `spanning` 分区（零风险，已验证）

只改 `cut_lines` / `cut_column_subtree` 的 spanning push 位置。此时 seq 排序仍在
→ 纯死代码修正。**已实测：`cargo test -p pdf-text` 252 全绿，PMC 三篇分数逐位不变。**

验证口径：全绿 + 全语料零 diff。**若出现任何 diff，说明对 spanning 的理解有误，立即停下。**

### 阶段 1.5 · （可选）止血补丁

若业务上急需 PMC212689 这类页面正确，可先上"顶层 X-cut 页用 region 下标、其余页用 `min(seq)`"
+ "所有 region 一律原子"。**已实测全部门绿**：

| | main | 补丁 | fitz |
|---|---|---|---|
| PMC mean order | 0.8947 | **0.9750** | 0.9754 |
| PMC212689 | 0.5994 | **0.7434** | 0.7492 |
| EUR-Lex mean lev | 0.9248 | 0.9246 | 0.9292 |
| EUR-Lex mean order | 0.9771 | 0.9769 | 0.9821 |
| born 6 篇 | — | **逐位相同** | — |

门：`cargo test --workspace` 1666 passed / 0 failed；`clippy -D warnings` 通过；`fmt --check` 通过；
`pytest python/tests` 772 passed / 66 skipped / 0 failed（含 `PYTEXT-010` 与 `compat_block_*`）。
patch 在 `scratchpad/reg/fixE.patch`（`+91/−63`），已装好的解释器 `scratchpad/reg/wt-fixE/.venv/bin/python`。

**明确标注为止血**：判别式依赖"顶层选了哪个轴"这个内部实现细节，切分策略一变行为就漂移，
且没有任何测试会捕捉到。若采用，必须在代码注释与 issue 中写明失效条件，并挂在阶段 3 的前置。

### 阶段 2 · 定夺目标语义（调研，不改代码）

1. 读 MuPDF `stext-para.c` / `stext-device.c`（源码已在 `scratchpad/source_fitz_*.c`、
   `stext-device-1.27.2.c`），刻画 reading-order pass 的**触发条件**。
2. 用合成探针在 oracle 上验证该刻画（沿用 `PYTEXT-010` 的 `_raw_content_pdf` 手法）。
3. 产出一份"目标语义规格"，据此重写 `PYTEXT-010`、提升 `LAYOUT-ORDER-002`。

**这一步不产出分数，但它是阶段 3 的唯一合法输入。** 跳过它就会重蹈 `b6c027a` 的覆辙。

### 阶段 3 · 几何阅读顺序（主体重构）

按阶段 2 的规格实现 `band → column → y`，同时修 `cut_lines` 的 X/Y 优先级
（能做出合法 column cut 就禁止祖先层横切）。删 `side_by_side` 全套。

### 阶段 4 · region 内行序改几何 y

含 `group_region_paragraphs` 的 `baseline_step` 去 `.abs()`。
**改动面最大、`compat_block_*` 全族压在这条路径上**，单独一个阶段，可延后。

### 各阶段统一验证口径

| 语料 | 位置 | 看什么 | 当前基线 |
|---|---|---|---|
| PMC 干净 7 篇 | `conformance/gt/corpus-pmc/` | order；**唯一硬指标是 PMC212689** | 0.5994（fitz 0.7492，v0.5.0 0.6454） |
| EUR-Lex 40 篇 | `conformance/gt/corpus-eurlex/` | mean lev / order **不得低于基线** | 0.9248 / 0.9771 |
| born 6 篇 | `conformance/gt/corpus-born/` | 逐位不变（该语料全单页无页眉，**对本议题无诊断力**） | 1.0000 / 0.9803 |
| **govinfo FR 子集** | `conformance/gt/corpus-govinfo/` | **`spanning` 修复的唯一有效验收集**：running header 的秩 → 0 | 现状 **39/166 页（23%）**把通栏 header 排进正文中间，fitz **0/166** |
| 300 文档语料 | `conformance/` | 差分为主，`cargo test` 看不见这些文档 | — |
| 单元/集成 | `cargo test --workspace` + `pytest` + `clippy -D warnings` + `fmt --check` | 全绿 | 1666 / 772 |

**注意**：`spanning` 修复**不要用 EUR-Lex 的 lev 验收**——EUR-Lex 的 `gt_text` 把 running header
整个剥掉了，页眉排在页首还是页尾都是多余 token，收益结构上为 0（实测 −0.001）。

---

## 附录 A · 二分证据

同一份主仓 `run_gt.py` + 同一 oracle，只换 `--python`：

| commit | PMC176547 | PMC212688 | **PMC212689** |
|---|---|---|---|
| v0.4.0 `209d779` | 1.0000 | 0.9910 | 0.6454 |
| v0.5.0 `c386b27` = `b6c027a^` | 1.0000 | 0.9910 | **0.6454** |
| **`b6c027a`** | 0.5214 | 0.6600 | **0.5985** ← 跌点 |
| `bdf8d1d` / `d7135f0` | 0.5214 | 0.6600 | 0.5985 |
| `2451922` | 0.5214 | 0.6600 | 0.5994（**+0.0009**） |
| main `75a1ace` | 0.5214 | 0.6600 | 0.5994 |

`b6c027a` 对干净 7 篇是**一笔交易**：6 篇小赚（lev +0.0006~+0.0029），1 篇大亏（−0.046）。
mean order 0.9455 → 0.9391。

## 附录 B · PMC212689 失效形态

difflib 匹配块尺寸直方图 `237,160,144,128,116,110,104,99,87…`，
**1872/1920 匹配 token 落在 size≥10 的块里，size=1 碎块仅 11 个**
→ **块级整段挪位，不是左右栏逐行交错**。

反向 n-gram 跳变：我们 6 次、fitz 3 次；其中 `gt@504` 与 `gt@1720` **两边都跳且 delta 相同**
（版式本身的歧义，非我方缺陷），**pdfspine 独有的只有 3 处**。

## 附录 C · order 指标的读法

```python
shared  = sum((Counter(ht) & Counter(rt)).values())        # 分母 = 多重集交集，与顺序无关
matched = sum(b.size for b in difflib.SequenceMatcher(None, ht, rt, autojunk=False).get_matching_blocks())
return matched / shared                                     # 任一为空或 shared==0 → 1.0
```

- 两家都打 `page.get_text("text")` 逐页 `"\n".join`，**都没传 `sort=True`** → 对比公平。
- **抽取内容缺失几乎不惩罚 order**（分母同步缩小）→ order 只在 content 分数也高时可信。
- **敏感度与 |GT| 成反比**：284 token 的 GT 上一次整块调换吃掉 47.9 分；9219 token 上只值约 2 分。
- `_TOKEN_CAP = 50000`，PMC 最大 GT 9219 token，**截断未触发**。
- GT 口径（`jats_text.py`）只含标题 + 摘要 + 正文，**排除**参考文献/表格/图注/公式/脚注/`<xref>`；
  抽取端却全抽 → PMC 上两家 lev 都只有 0.5 上下是**地板效应，不是抽取质量**。

## 附录 D · 与本议题正交、建议单独立项

1. **EUR-Lex 的真正失分点：2 列对照表按栏读 vs GT 按行读。** 32014R0596 家族 8 篇的
   −0.013~−0.017 lev，**98~100% 来自附录 p55–60**；32011L0083 家族 74~90% 来自最后 2~3 页。
   这是"多栏阅读顺序"与"表格行序"的语义冲突，**栏序全对也拿不回分**。
   方向：放宽 `is_table_dominant`（现要求 `TABLE_MIN_CELLS_PER_BASELINE = 3`，2 列对照表永远进不去）
   或对"两栏且行行对齐"的 region 走 row-major。**EUR-Lex 上 ROI 远高于本文所有条目。**
2. **`jaccard` 系统性小输**：干净 7 篇负 6，且 76% 差距来自 PMC212689 之外的篇目
   （各篇 −0.001~−0.002）。词表级差异，与阅读顺序无关。
3. **首字下沉行合并缺陷**：`PMC212689 p0` 块 `(54.0,151.7,203.0,201.6)` 输出为
   `'Cedaormlieesstt iicnanteodv a1t0io,0n0s0. Iyte waarss ago'` —— 两条基线被逐字符交织。
   推测首字下沉大写 "C"（跨 3 行）撑大行 bbox 后按 x 排序所致。全语料 17 页仅此 1 例。
4. **通栏行被沿切割线物理切成碎片**：Federal Register 上页脚被切 5 片散落；
   页眉页脚碎片数是 fitz 的 1.8~1.9 倍。修 spanning 时应一并处理。

## 附录 E · 复现资产（scratchpad，主仓工作区未动）

- `verify/repro.py` —— 按 `run_gt.py` 口径复现单篇打分（GT / pdfspine / fitz / score_all）
- `reg/fixE.patch`（止血补丁 `+91/−63`）、`reg/fixD.patch`、`reg/layout_stepA.rs`、`reg/layout_stepB.rs`
- `reg/wt-{v040,v050,b6c,bdf,before,at,fix,fixB,fixE,probe}/` —— 各版本 worktree + 已装好的解释器
  （`target-*` 已删，解释器仍可用；需重新编译时要重建 target）
- `reg/gt-pmc-*.json`、`gt-eurlex-{fixB,fixD,fixE}.json`、`gt-born-fixE.json`
- `span/xycut.py` —— `cut_lines` 全家的 Python 逐行移植（`mode="buggy"` / `"fixed"`）
- `ord/*.sidebyside.txt` —— 双引擎块序并排
- `source_fitz_stext-{device,para,output}.c`、`stext-device-1.27.2.c` —— 阶段 2 调研用的 MuPDF 源码

## 2026-09-05 修复记录（阶段 1 + 阶段 1.5 落地，阶段 2 黑盒结论）

代码基线 `a45b66d`（main，含 09-03 之后落地的 `aaee2a9` 两列对照表逐行读取）· oracle PyMuPDF 1.28.2 ·
机器与 5 个并行 agent 共享 CPU（本节数字全部是确定性的抽取/打分结果，不含时间基准）。
提交的是下文的**变体 e**；变体 a–d 的数据保留在"变体对比"一节供后续阶段使用。

### 改了什么（`crates/pdf-text/src/layout.rs`，`+151/−26`）

1. **阶段 1 · `spanning` 分区**：`cut_lines` / `cut_column_subtree` 共用新的 `emit_column_cut`，
   把一次 column cut 的 spanning 行按几何拆成 `above / middle / below`（`partition_spanning`），
   `above` 排在两栏之前、`below` 排在两栏之后。判据：spanning 行的 `y1 ≤ 栏体顶 + 半行` 为 `above`、
   `y0 ≥ 栏体底 − 半行` 为 `below`，其余 `middle`。"栏体"的上下界只由两类过滤后的栏行决定：
   - 只数**宽度 ≥ 所在栏 50%** 的栏行（`SPANNING_COLUMN_LINE_MIN_WIDTH_FRAC`）。OJ 首页左上角的页码
     "PL / L 304/64" 与右上角的日期都落在栏内、位于居中标题之上；若把它们算作栏体，`32011L0083_PL p0`
     的标题块会被判成 `middle` 而排到整个左栏之后（变体 b 实测该篇 lev/order 各 −0.010）。
     FR 的竖排边注（x 18–23、高 130pt）同理会阻止页脚成为 `below`。
   - **不计入含 spanning 行的"页眉/页脚行"**（高度 ≤ 2 个典型行高的 band，`SPANNING_MARGIN_ROW_MAX_HEIGHT`）。
     FR 的页眉被行级 `split_on_gutter` 切成逐栏碎片（附录 D4），落在栏内的宽碎片
     "Friday, January 2, 2026 / Contents" 会把栏体顶抬到页眉自己的行，使跨栏的另一半碎片变成
     `middle`（变体 d 实测 FR 误排 331/2492，变体 b/e 为 64）。
   - 反过来不能用"没有栏行在其上方"这种逐行判据（变体 b/c）：`column_gutter` 的 tol 会把 OJ 右栏首行的
     序号 "(32)" 当作跨栏行，它与正文首行同高，逐行判据把它判成 `above` 而排到左栏之前（`32011L0083_PL p4`）。
2. **阶段 1.5 · 止血补丁**：`cut_lines` 返回根节点是否做了 column cut；`group_blocks_columned`
   在**根为 X-cut** 的页上按 region 下标（几何 DFS 序）排序且每个 region 原子；**其余页保持
   `b6c027a` 的原逻辑逐字节不变**（side-by-side region 原子按 `min(seq)`，其它 region 逐块按 `seq`）。
   与本文 (e) 阶段 1.5 的描述有一处**有意偏离**：没有采用"所有 region 一律原子"。实测该变体（变体 a）
   在根为 Y-cut 的页上把混栏叶子 region 整体搬动：`32011L0083_EL p10` 中 region
   {ιβ), εμπορικών, β), τη γεωγραφική} 横跨两栏（band 内 column cut 失败），原子化后右栏的 `β)…` 两块被
   拖到左栏 `ιγ)…` 之前；逐块 `seq` 排序恰好在修复这类叶子。EUR-Lex 上变体 a 的
   `32019R0881_{BG,DE,EL,PL}` order 各 −0.013~−0.016、`32011L0083_PL` −0.010，均由此机制造成。
3. 文档修正：`order_blocks` / `Block::seq` 的 doc comment 不再声称图像块参与排序；`cut_lines` 的
   doc comment 不再说"最终顺序由 seq 决定"。
4. 新增合成 fixture 测试（`crates/pdf-text/tests/reading_order_round1.rs`）：
   `readorder_006`（三栏、先画第三栏 → A→B→C，锁 PMC212689 形态）、
   `readorder_007`（通栏页眉+两栏+通栏页脚、页眉页脚最后画 → header→左→右→footer，锁 spanning 分区）、
   `readorder_008`（段落间距 > 栏间距的根 Y-cut 双栏页、按阅读序绘制 → 整栏连续，锁"Y-root 保留 seq"这条取舍）。
   `LAYOUT-ORDER-002`、`PYTEXT-010`、`compat_block_*` 全部原样通过。

### 基线复现（修复前，worktree `a45b66d`）

| 语料 | 本次复现（pdfspine） | 09-03 报告 | 一致？ |
|---|---|---|---|
| PMC 干净 7 篇 | lev 0.7243 / f1 0.7808 / jaccard 0.6101 / order 0.9391；PMC212689 lev 0.5630 / order 0.5994 | 0.724 / 0.781 / 0.610 / 0.939；0.563 / 0.599 | 逐位一致（磁盘上的 `corpus-pmc/manifest.json` 已只含干净 7 篇，即阶段 0 已在语料侧完成） |
| born 6 篇 | 0.9803 / 0.9803 / 0.9652 / 1.000 | 同 | 逐位一致 |
| EUR-Lex 40 篇 | lev 0.9371 / f1 0.9706 / jaccard 0.9293 / order 0.9772（fitz 0.9396 / 0.9704 / 0.9284 / 0.9800） | `GT-REPORT.md` 中 9 篇不同 | **不一致，已解释**：`32011L0083_*`、`32014R0596_*` 8 篇在 09-03 之后被 `aaee2a9`（两列对照表逐行读取，即附录 D1）抬高；`32006L0112_BG` 仅 pdfminer 末位 ±0.001。本文 (a)/A4 引用的 "0.9248 / 0.9771" 来自诊断时的 scratch 打分口径，与 `run_gt.py` 的 40 篇均值不可直接比较。 |
| govinfo FR | 12 期（2026-01-02…01-20，`fetch_govinfo.py --collections FR --per 12`），2492 个带 running header 的页：**850 页（34.1%）**把页眉排进正文之后；fitz 64/2517（2.5%） | 39/166（23%）；fitz 0/166 | 样本不同（本文诊断时的 166 页语料未保留），比例同量级 |

FR 的页眉判定：`get_text("blocks")` 中 `y0 < 60` 且匹配 `Federal Register|Vol\. \d+, No\. \d+ /` 的**第一个**块；
"误排" = 有正文块（`y0 ≥ 页眉 y1`）排在它前面。pdfspine 会把页眉沿栏切割线切成 2~4 块（附录 D4：
530/2492 页碎片化，fitz 0），因此按碎片匹配，否则 pdfspine 侧漏检 400+ 页。

### 修复后（变体 e，同一套脚本、同一 oracle）

| | 修复前 | **修复后** | fitz |
|---|---|---|---|
| PMC 7 篇 mean lev / f1 / jaccard / order | 0.7243 / 0.7808 / 0.6101 / 0.9391 | **0.7439 / 0.7808 / 0.6101 / 0.9600** | 0.7445 / 0.7809 / 0.6125 / 0.9605 |
| PMC212689 lev / order | 0.5630 / 0.5994 | **0.7003 / 0.7456** | 0.7050 / 0.7492 |
| PMC 其余 6 篇 | — | 四位小数逐篇不变 | — |
| born 6 篇 | 0.9803 / 0.9803 / 0.9652 / 1.000 | 逐位不变 | 同 |
| EUR-Lex 40 篇 mean lev / f1 / jaccard / order | 0.9371 / 0.9706 / 0.9293 / 0.9772 | **0.9372 / 0.9706 / 0.9293 / 0.9773** | 0.9396 / 0.9704 / 0.9284 / 0.9800 |
| 　EUR-Lex 单篇变化 | — | `32006L0112_BG` lev +0.0046 / order +0.0047；`32011L0083_PL` +0.0002；其余 7 篇 ±0.0003 以内 | — |
| govinfo FR 页眉误排页数 | 850 / 2492（34.1%） | **64 / 2492（2.6%）** | 64 / 2517（2.5%） |

FR 剩余 64 页与 fitz 的 64 页几乎重合：各期 p1（"The FEDERAL REGISTER (ISSN…)" 信息页）与 p4/p5（Contents / CFR
Parts 页）等根为 Y-cut 的页，本补丁不触碰。页眉碎片化（530 页）未变，属附录 D4。
已知残余：OJ 右栏的序号列 "(32) (33) (34)…" 被 `column_gutter` 的 tol 判成跨栏行，作为 `middle` region 整体排在
左栏之后、右栏正文之前，序号与其段落分离（基线按块 seq 时是交错正确的）；只影响单 token，阶段 3 的 band 交错应一并处理。

### 300 文档冻结清单（`corpus-diff/baselines/glyph-geometry-2026-09-05-manifest.json`，SHA-256 逐一校验，每篇前 20 页，共 1887 页）

text / dict / rawdict 三种投影的逐页 SHA-256：**42/300 篇、285/1887 页变化**（三种投影同页同变）。按目录：EUR-Lex 17 篇、
robustness(govdocs1) 23 篇、`fixtures/corpus/usgs-fs20183024` 1 篇、fintabnet 1 篇。归因用了**第三个诊断构建**
（"仅 region 原子、不改序、不分区"，即变体 a 减去几何序）做三方对照：

| 机制 | 页数 | 说明 |
|---|---|---|
| 根 X-cut 页：region 序由 `min(seq)` 改为几何 DFS 序 + spanning 分区 | 285 | 283 页在"仅原子"构建上与基线逐字节相同（不是原子化引起），2 页两者皆变；Y-root 页的排序代码路径未变 |
| 　其中纯块置换（块集合不变，只换顺序） | 284 | EUR-Lex：OJ 的 running header（最后绘制）从页尾移到页首（如 `32006L0112_BG p1-3`），首页标题块在页眉之后、正文之前；govdocs1：NTSB 表单、USGS 地图标注等"绘制序与几何无关"的页按栏几何序输出（`govdocs1-00007 p6`、`-00012 p0`） |
| 　其中块集合变化（同一行集被切成不同块） | 1 | `32013R0575_DE p16`：两条脚注原为 1 块，现为 2 块（text 也变）。原因：`cut_spanning` 现在分别作用于 above/middle/below 子集，子集各自计算 `typical_line_height` 与 band 阈值 |
| 与变体 b 的差异 | 47 页 | 全部来自分区判据的改进（标题块/页眉碎片的归属），241 页与变体 b 相同 |

### 变体对比（后续阶段的数据）

| 变体 | 内容 | PMC mean order（fitz 0.9605） | PMC212689 lev / order（fitz 0.7050 / 0.7492） | FR 误排（fitz 64/2517） | EUR-Lex 40 篇 mean lev / order（基线 0.9371 / 0.9772，fitz 0.9396 / 0.9800） | 300 文档变化 |
|---|---|---|---|---|---|---|
| a | 本文 (e) 原版阶段 1.5：所有 region 原子 + X-root 几何序 + 逐行 spanning 分区 | 0.9596 | 0.6982 / 0.7434 | 64 / 2492 | 0.9355 / 0.9755（`32019R0881_*` −0.013~−0.016，`32011L0083_PL` −0.010） | 83 篇 / 519 页（231 页仅由原子化引起，全是 Y-root 页） |
| b | a 去掉 Y-root 页的原子化 | 0.9596 | 0.6982 / 0.7434 | 64 / 2492 | 0.9369 / 0.9770（`32011L0083_PL` −0.010：首页标题块排到左栏之后） | 41 篇 / 287 页 |
| c | b + 逐行判据只数宽栏行 | 未跑完 | — | — | `32011L0083_PL p4` 的 "(32)" 排到左栏之前，判据作废 | — |
| d | b + 栏体上下界（宽栏行） | 0.9600 | 0.7003 / 0.7456 | **331 / 2492**（页眉碎片抬高栏体顶） | 未跑 | 42 篇 / 285 页 |
| **e（提交）** | d + 含 spanning 行的页眉/页脚行不计入栏体 | **0.9600** | **0.7003 / 0.7456** | **64 / 2492** | **0.9372 / 0.9773** | 42 篇 / 285 页 |

### 阶段 2 · 黑盒探针结论（未读 MuPDF 源码，与 README 的 clean-room 声明一致）

用 `PYTEXT-010` 的 `_raw_content_pdf` 手法合成 9 个已知绘制序的单页 PDF，在 oracle（PyMuPDF 1.28.2）上跑
`get_text("text")`（默认 flags，`sort=False`）：

| 探针 | 绘制序 | fitz 输出 |
|---|---|---|
| 两栏、先画右栏 | R0..R7 L0..L7 | **R0..R7 L0..L7**（不重排） |
| 两栏 + 通栏页眉最后画 | L R H | L R **H**（页眉留在最后） |
| 通栏页脚最先画 | F L R | **F** L R |
| 三栏按 3,2,1 画 | C B A | **C B A** |
| 单栏两段、先画下段 | B A | **B A** |
| 两栏按行主序画（同基线） | L0 R0 L1 R1… | L0 R0 L1 R1…（且合成 1 个 block） |
| 两栏行主序 + 宽段落间距 | LA0 RA0 … | 行主序不变 |
| `PYTEXT-010` 形态 | BB RR LL | BB RR LL |
| 两栏各 2 行、先画右栏 | R L | R L |

**PyMuPDF 默认路径没有任何几何重排：块序 = 绘制序**（同基线片段合并进同一 block）。本文 (b)/B1 的
"fitz 做了几何重排"推断来自 `PMC176547`——一篇已被隔离的错配语料，不成立。1.28 提供 opt-in 的
`TEXT_SEGMENT`：探针 6（行主序）在该 flag 下变为整栏连续，但探针 1（先画右栏）仍输出 R 先于 L——
即使开启分段，栏与栏之间仍按绘制序。

**PMC212689 的真实机制**：p0 绘制序最前的 glyph 是右下角的 running footer
"Volume 1 | Issue 1 | Page 015"（x 455–558，落在右栏 region 内）。fitz 只把这一小块排到最前、其后按绘制序
（= 阅读序）输出三栏；`b6c027a` 用 region 级 `min(seq)` 把**整个右栏**带到最前——是 region 级 `min(seq)`
对单个早绘制 glyph run 的放大，不是"内容流整栏逆序"。

**对 (c)/C2 三选项的影响**：选项 1"复刻 MuPDF 的 segmentation"在默认路径上就是复刻绘制序，等于回到
`b6c027a` 的语义（只是粒度从 region 降到 block），无法解决行主序/翻转 CTM 的页；本次修复实际走的是选项 2
（自研几何序，限于 X-root 页）并已把与 fitz 的边界差异记录在上表。阶段 3 若推进，建议明确以"几何阅读序"为
目标语义，`PYTEXT-010` 收窄为"无可辨识栏结构的页保留绘制序"。

### 本次改变 / 未改变的取舍

- **改变**：`PRD-NEXT.md` P3-1r 的 "accepted won't-fix" 依据（"任何帮助 PMC212689 的改动都会回退 PMC212688 −0.19
  与 PMC176547 −0.02"）中的两篇正是阶段 0 隔离的错配语料；干净 7 篇上本次 PMC212689 +0.146 order、其余 6 篇
  四位不变。该标注的前提已不成立（由协调者改 PRD）。
- **未改变**：Y-root 页仍是 `b6c027a` 的绘制序语义（`PYTEXT-010` 原样通过，`readorder_008` 锁住）；region 内行序仍按
  `seq`（阶段 4）；页眉碎片化（D4）、首字下沉交织（D3）、两列对照表（D1，`aaee2a9` 已部分处理）均未触碰。
- **失效条件（止血补丁的自知之明）**：判别式是"根节点选了 X 轴"。`prefer_x = xg >= yg` 一旦改动（阶段 3 的 X/Y
  优先级），哪些页走几何序就会变，`readorder_006/007` 只锁 X-root 形态、`readorder_008` 只锁 Y-root 形态，
  两者之间的迁移没有测试覆盖。阶段 3 落地时应删除该判别式而不是叠加条件。spanning 分区的三条过滤规则各自
  对应一个语料形态（OJ 首页、OJ 序号列、FR 页眉碎片），没有统一原理，阶段 3 的 band 交错应取代它们。

### 门禁

`cargo fmt --check` ✓ · `cargo clippy --workspace --all-features -- -D warnings` ✓ · `cargo test --workspace --all-features`
1705 passed / 0 failed · `pytest -W error --doctest-modules python/pdfspine python/tests` 811 passed / 66 skipped。

### 复现资产（`/Volumes/ExternalSSD/tmp/readorder/`，不入库）

`ro_digest.py`（三投影逐页摘要）、`ro_compare.py` / `ro_threeway.py` / `ro_overlap_e.py`（差分与三方归因）、
`ro_fr_header.py`（FR 页眉秩）、`ro_probe.py` / `ro_probe2.py`（阶段 2 探针）、`gt-*-{before,after,b,d,e}.json`、
`compare-{after,b,d,e}.json`、`fr-header-{base,after,b,d,e,fitz}.json`、`govinfo-fr/`（12 期 FR PDF + manifest）。
逐页摘要目录与各变体 wheel 已按任务要求删除。
