# 交接：Rust 文本层发布完整字形几何 + line/span 聚合修正

> **重启入口（2026-09-05）：**先读 `docs/PRD-NEXT.md` §0 的当前优先队列，
> 再读本文件 §12 和 `conformance/COVERAGE-REPORT.md` / 专项 geometry reports。
> 仓库是 `/Users/linhan/startup/spine/pdfspine`；`HEAD`、`origin/main`、`v0.7.1`
> 均为 `9da7ca6`，旧 `v0.7.0` 保持 `36ed734`。A–G 已完成，不能再按
> `glyph-geometry-continue` / `9912a23` 的历史状态续做。§1–§11 只保留设计和过程史。
>
> `0.7.0` 发布 glyph geometry，`0.7.1` 仅同步文档。PyPI 正式未锁版本安装已
> 独立验证选择 `0.7.1`、自动安装 `ocrspine-models==0.0.3`，并实测
> `Tf 1` / `Tm 12` 得 `size == rendered_size == 12`、`declared_size == 1`。
> GitHub Release `v0.7.1` 已 published、非 draft、非 prerelease，六个附件齐全。
> 当前新增 coverage report 和本轮 PRD/HANDOFF 整理在 commit 前都只是工作树成果。
>
> 下一步依序是：修主 CI 离线 wheel smoke 的模型依赖准备；持久化并联合采集
> Rust/Python coverage；继续优化 rawdict geometry 成本；用独立语料实验 span
> 首字形代表问题。不得无证据改 F 阈值。更完整口径见 `docs/PRD-NEXT.md` §0。

---

## 0. 任务目标（原始诉求）

让 **Rust 文本解析层直接对外发布完整的字形几何信息**，使下游
**不再需要靠 bbox 反推字号，也不再需要自己修复交错字符**：

`declared_font_size` / `rendered_font_size` / `text_matrix` / `ctm` / `render_matrix` /
`baseline_vector` / 变换后的 glyph bbox（旋转 quad，不是轴对齐近似）/ `painting_order` / `reading_order`。

外加：修正 line/span 聚合 —— 变换矩阵、文字方向、基线差异超过与字号相关的容差时，
不应放进同一个视觉 span。

用户后续追加的总原则（优先级最高）：
> **能暴露出来的数据尽可能多暴露出来，且在文档里说明，这样别的大模型调用这个库时知道怎么用。**

---

## 1. 已完成

### commit `95f7205` — `fix(pdf-render): use the true text rendering matrix for SVG glyphs` ✅ 已落地

SVG 后端此前用 `glyph.size`（纯 `Tfs` 标量）当字形矩阵的线性部分，
导致相对真实 `Trm` 的缩放因子**恒为 1.0**（位置对、字号/朝向错，图表文字失真）。
光栅后端 `render.rs` 早就在用 `TextRun.trms[i]` 了，只有 SVG 落下。

- `crates/pdf-render/src/svg.rs`：`write_glyph_outlines` 改用 `run.trms[i]`（新增私有 helper
  `glyph_trm`，与 `render.rs` 的 `has_trms` 回退逻辑同构）；守卫从 `size` 有限性换成
  `matrix.determinant()` 有限且非零。
- 顺带修了 `write_text_fallback`（无嵌入字体程序时的 `<text>` 回退）：
  `m = [1/size, 0, 0, -1/size, 0, 0] * trm`。数学上是纯归一化，**恒等情形逐字节不变**
  （`Trm = [F,0,0,F,ox,oy]` 时结果正是原来的 `[1,0,0,-1,ox,oy]`），风险极低。
- 新增测试 `SVGTRM-001..004`（`crates/pdf-render/tests/svg.rs`），已登记
  `docs/test-case-catalog.md:2504-2525`。四条都**先以正确的理由失败**过：
  `e/f`（位置）修复前后完全一致，只有线性部分错。

| ID | 内容流 | 断言矩阵 | 修复前 |
|---|---|---|---|
| `SVGTRM-001` | `q 2 0 0 2 0 0 cm BT /F1 12 Tf 20 100 Td (A) Tj ET Q` | `[24,0,0,24,40,200]` | `[12,0,0,12,40,200]` |
| `SVGTRM-002` | `q 0 1 -1 0 150 20 cm BT /F1 20 Tf 50 50 Td (A) Tj ET Q` | `[0,20,-20,0,100,70]` | `[20,0,0,20,100,70]` |
| `SVGTRM-003` | `BT /F1 10 Tf 50 Tz 20 100 Td (A) Tj ET` | `[5,0,0,10,20,100]` | `[10,0,0,10,20,100]` |
| `SVGTRM-004` | 同 002 但字体无 `FontFile`（`<text>` 回退） | `[0,1,1,0,100,70]` + `font-size="20"` | `[1,0,0,-1,100,70]` |

门：`fmt` ✅ / `test -p pdf-render` 17/17 ✅ / `test -p pdf-api` ✅ /
`clippy -p pdf-render --all-features -D warnings` 见 §6 已知项。

### 未提交的工作树改动 — Rust 内部携带字形几何（约 90% 完成）

见 `git status`。**`cargo test --workspace --all-features` 在写本文时正在跑，已过部分全绿。**

已改：
- `crates/pdf-text/src/model.rs`
  - `PositionedGlyph` 新增 4 字段（**用户空间**）：`text_matrix` / `ctm` / `render_matrix` / `cell`。
  - `Char` 新增：`matrix`(设备空间) / `quad`(设备空间真四点) / `rendered_size` / `seq` / `synthetic`。
  - `Span` 新增：`rendered_size` / `matrix` / `text_matrix` / `ctm` / `dir` / `quad` / `seq`。
  - 新增 `pub fn rendered_font_size(m: &Matrix) -> f64` = `sqrt(|det|)`，退化/非有限返回 `0.0`，不 panic。
- `crates/pdf-text/src/interp.rs`：`emit_glyph_into` 把已经算出的 `trm` / `tm` / `ctm` / `cell` 留下来。
- `crates/pdf-text/src/layout.rs`：`DevGlyph` 携带 `render_matrix`(= `Trm · page_transform`) /
  `quad` / `text_matrix` / `ctm` / `seq`；`build_line` 填充 Span/Char 的新字段。
- `crates/pdf-text/tests/glyph_geometry.rs`（**新文件，298 行**）：`GLYPHGEO-001..009`，9 条全过。
- 若干既有测试文件因结构体加字段而做的机械适配。
- `crates/pdf-ocr/src/integration.rs`、`crates/pdf-render/src/text.rs`、
  `crates/pdf-text/src/texttrace.rs`：同样是构造 `PositionedGlyph` 处的机械补字段。

---

## 2. 原始接手清单（历史；现均已处理，见 §12）

### T1. 清理（5 分钟）
- **删掉 `crates/pdf-text/tests/zz_size_probe.rs`** —— 临时探针（只打印
  `size_of::<PositionedGlyph/Char/Span>()`）。删之前先跑一次
  `cargo test -p pdf-text --test zz_size_probe -- --nocapture` 把三个数字记进 commit body。
- 跑完整门（§5），确认全绿。
- 把上面这坨未提交改动提成 **Commit 1**：
  `feat(pdf-text): carry the full glyph rendering geometry`

### T2. 对外发布（**最关键的缺口**）
`crates/pdf-text/src/serialize.rs` 与 `crates/py-bindings/src/lib.rs` **一行都还没改** ——
新字段目前只存在于 Rust 结构体里，**Python 侧完全看不到**。这是任务的核心诉求，必须做完。

需要发布的键（既有键语义**一律不变**）：

**span 层**（`dict` / `rawdict` / `json` / `rawjson`）
| 新键 | 类型 | 值 |
|---|---|---|
| `declared_size` | float | `Tf` 声明值（与现有 `size` 同值，显式命名） |
| `rendered_size` | float | `rendered_font_size(matrix)` |
| `matrix` | 6-tuple | **设备空间** render matrix（span 首字形） |
| `text_matrix` | 6-tuple | `Tm`，**用户空间原始值** |
| `ctm` | 6-tuple | CTM，**用户空间原始值** |
| `dir` | 2-tuple | span 基线方向单位向量（设备空间） |
| `quad` | 8-tuple | span 的方向包络 |
| `seq` | int | painting order |

**char 层**（`rawdict` / `rawjson`）：`matrix`(6) / `quad`(8) / `rendered_size` / `seq` / `synthetic`(bool)

**line 层**：`number`(int，页内 reading order 下标) / `seq`(int，既有 `Line.seq`)

**block 层**：新增 `seq`(既有 `Block.seq`，图像块为 `usize::MAX`)；`number` 已存在，只补文档。

改动点（已侦察确认的行号，基线版本）：
`dict_span`(serialize.rs:673) / `dict_line`(:663) / `text_block`(:654) /
`json_span`(:869) / `json_line`(:850) / `json_block`(:797) / `xml_span`(:1169)；
Python 转换 `crates/py-bindings/src/lib.rs:594-654`。

⚠️ `dict`/`rawdict` 的二选一陷阱：`dict_span` 在 `raw` 时把 `text` 置空填 `chars`，否则反之；
py 层 `lib.rs:640-653` 靠 `span.chars.is_empty()` 判断输出哪个。

### T3. `to_xml` 的 `<char quad>` 改成真 quad
现在填的是 **bbox 派生的轴对齐四点**；PyMuPDF 1.28.2 实测 fitz 填**真实平行四边形**
（斜切探针下四点不构成矩形）。用新的 `Char.quad` 替换。顺序 `ul.x ul.y ur.x ur.y ll.x ll.y lr.x lr.y`（与 fitz 一致）。
改完把 `serialize.rs:1010` 附近"已声明的 PyMuPDF 偏差"注释里这条**删掉**（不再是偏差）。

### T4. Python 测试 + 文档
- `python/tests/`：`PYGEO-001` 起。
- `docs/` 相应页 + **`python/pdfspine/_llms/docs/api.md`**（给大模型看的，是本次重点交付物）。
  文档要求见 §4。
- 全部登记 `docs/test-case-catalog.md`。

### T5. span 聚合收紧（独立 commit，**必须拿语料数据**）
现判据（`build_line`, layout.rs:1416 基线行号）只有
`font` + `size`(1e-6) + `color` + `flags` 四项，**完全没有几何判据** —— 这就是"交错字符"的来源。

新增分割判据（相邻字形之间，任一命中即断开 span），三条容差**全部相对化**（分母 `rendered_size`）、
提为具名常量：
1. **线性部分**：`render_matrix` 的 `(a,b,c,d)` 逐项差 ÷ `rendered_size` 超阈值
   （一条判据同时覆盖缩放 / 旋转 / 斜切 / `Tz`）
2. **基线方向**：`dir` 夹角，复用既有 `dir_matches` 口径（`dot > 0.996`，约 5°）
3. **基线位置**：`|Δcross|` ÷ `rendered_size` 超阈值

**容差取值必须有数据依据**：在 300 文档语料上统计这三个量的实际分布，找自然分界点。

✅ 已确认**不会**打散正常上下标 —— `flags::SUPERSCRIPT` 早就参与 `can_merge`，
正常上下标现在就已是独立 span。真正风险方向是反的（过度分割正常文本），容差往"够松"一侧靠。

正反两组测试都要有：该分的分（不同缩放/旋转/基线的相邻字符）、不该分的不分（同一行正常文本、正常上下标）。

### T6. `span["size"]` 的 parity 决策（**独立实验分支，用数据定**）
见 §3① —— 这是一个**已证实的 parity bug**。
做法：先让 T2 的新键落地，再单开实验分支把 `Span.size` 改成 rendered，
用 300 文档语料 + GT 实测；**数据好就落成独立 commit，数据差就只在文档里标注 parity 差异**。
注意它会连带影响：`build_line` 的 `eff_size` 词切分阈值、`to_html` 的 `font-size`、`to_xml` 的 `<font size=>`。

### T7. 性能实测
每字形多带 3 个 `Matrix` + 1 个 `Rect` ≈ **+176 B/glyph**，`PositionedGlyph` 大小翻倍。
用 EUR-Lex `32006L0112_EN` 或 govinfo 大件测 `dict`/`rawdict`/`text` 三种抽取的耗时与峰值 RSS。
**用户已决定：不加 `textflags` 按需开关，无条件携带**（原则是尽可能多暴露）。
除非实测严重退化才回头考虑。

### T8. rebase
另一个 agent 在 `worktree-agent-a1f48229c382fa88a` 上做双列对照表修复
（给 `is_table_dominant` 加 `is_two_column_record_grid` 路径，加了 `LAYOUT-ORDER-004/005/006`）。
写本文时**尚未合入 `origin/main`**（`origin/main` 仍是 `75a1ace`）。
预期最后要 rebase 到它之上，**冲突热点：`layout.rs` 与 `docs/test-case-catalog.md`**。

---

## 3. 已确证的事实（**直接用，不要重新发现**）

### ① PyMuPDF 1.28.2 oracle 实测：`span["size"]` 是**渲染**字号

公式 `sqrt(|a·d − b·c|)`（MuPDF `fz_matrix_expansion`），a,b,c,d 是 `Trm` 线性部分。

| 探针 | Trm 线性部分 | fitz `span["size"]` |
|---|---|---|
| `Tf 1` + `Tm 12 0 0 12` | (12,0,0,12) | **12**（不是 1） |
| `cm 2` + `Tf 12` | (24,0,0,24) | **24**（不是 12） |
| `Tm 20 0 0 10` | (20,0,0,10) | **√200 = 14.142136** |
| `Tf 12` + `50 Tz` | (6,0,0,12) | **√72 = 8.485281** |
| 斜切 `Tm 12 0 6 12` | (12,0,6,12) | **12**（旋转/斜切不变） |
| 判别探针 `(3,4,−8,6)` | — | **7.070711** = `√\|3·6−4·(−8)\|`，决定性 |

**G 之前 pdfspine 的内部 `Span.size` 与公开 `span["size"]` 都是 `Tf` 声明值**
（`interp.rs` 的 `size: ts.font_size`）。G 的最终公开语义见 §12.5。
所以那个"89.7% 双峰分布"**不是风格差异，是这个键本身错了**；
下游被迫用 bbox 反推字号，正是这个 bug 的下游症状。

### ② fitz 的 span 切分比我们**激进**（所以收紧聚合是向 parity 靠拢，不是偏离）
- 同一行内两段不同 `Tm` 缩放 → fitz 拆成**两个 line**
- 正常 + 旋转 90° → fitz 拆成**两个 block**
- 我们连 span 都不拆

**因果链**：我们的 `can_merge` 判的是 `size`(declared)。`Tf 1` + 两个不同 `Tm` 的情形，
declared size **都是 1** → 不拆。**把 `size` 改成 rendered，span 聚合的一大半问题自动消失** ——
①和②是同一个 bug 的两面。

### ③ fitz 没有矩阵可对齐
`rawdict` 和 `get_texttrace()` **都没有**任何矩阵字段。
唯一暴露真 quad 的通道是 `get_text("xml")` 的 `<char quad="ul.x ul.y ur.x ur.y ll.x ll.y lr.x lr.y">`。
**矩阵命名我们自己定，没有既有 fitz 约定要守。**

⚠️ fitz 内部有**两套字号语义**：`rawdict` 用 `sqrt(|det|)`，`get_texttrace()` 用 `|(a,b)|`
（x 基向量长度）。二者只在共形矩阵下一致，在非各向同性缩放与 `Tz` 下分道扬镳。
**不要用同一个名字承载两种语义。**

### ④ 仓内代码事实
- `pdf_core::geom::Matrix` = PyMuPDF 兼容 `[a,b,c,d,e,f]`，`(x,y) → (a·x+c·y+e, b·x+d·y+f)`；
  `Matrix::concat(m1,m2)` / `m1 * m2` = **先 m1 后 m2**。有 `determinant()`。
- `pdf_core::geom::Quad` **已存在**：`{ ul, ur, ll, lr }` + `from_rect` / `rect()` / `transform()`。
  **复用它，不要新造类型。**
- `TextRun` 在 **`crates/pdf-text/src/renderops.rs`**，不在 pdf-render（pdf-render 只是消费者）。
  它的 `trms: Vec<Matrix>` 与 `glyphs`/`gids` 三者索引严格一致；空 = legacy run。
- `interp.rs` 里 `Trm = params · Tm · CTM`，`params = [Tfs·Th, 0, 0, Tfs, 0, Trise]`，
  `origin = (0,0)·Trm`。横排 cell `[0, desc, w0, asc]`，竖排 cell 含 `−v` 位移。
- `Line.seq` / `Block.seq` **已存在**，语义就是**绘制顺序** → `painting_order` 复用它，不新造。
- **`reading_order` 有坑**：`layout.rs` 的 region 间排序键是 content-stream **绘制序**
  （`b6c027a` 的静默回归），不是几何阅读序。`PYTEXT-010` 与 `LAYOUT-ORDER-002` 两条测试
  断言互相矛盾。**本任务不根治**，`number` 只承诺"引擎当前实际使用的顺序下标 /
  与 `get_text("text")` 输出顺序一致"，并在文档写明局限。
  根治方案见 §7 的独立文档。

---

## 4. 设计决定（已拍板，照做）

### 矩阵与几何约定
- **Matrix** = 6 元组 `(a,b,c,d,e,f)`，PDF/PyMuPDF **行向量**约定
  `(x,y) → (a·x+c·y+e, b·x+d·y+f)`。与 `pdfspine.geometry.Matrix`、`identity_matrix()`、
  dict 图像块的 `transform` 键一致。
- **Quad** = 8 元组 `(ul.x,ul.y,ur.x,ur.y,ll.x,ll.y,lr.x,lr.y)`，与 `pdf_core::geom::Quad`、
  `search_for(quads=True)`、`get_text("xml")` 的 `<char quad>` 一致。
- dict 内一律**裸元组**（不包 `Matrix`/`Quad` 对象），与既有 `bbox`/`origin`/`dir` 及 fitz 一致。

### 坐标空间的刻意不对称（**最容易踩，必须写进文档**）
- `matrix` / `quad` → **设备空间**，与既有 `bbox`/`origin` 同基准。
- `text_matrix` / `ctm` → **PDF 用户空间的内容流原始值**，不叠 page transform ——
  它们的用途是**对回 PDF 源**，叠了就没法对了。
- 三条不变量（必须逐字出现在文档里，并写成测试）：
  1. `(0,0)·matrix == origin`
  2. `quad` 的外接矩形 `== bbox`
  3. `matrix = params · text_matrix · ctm · page_transform`，`params = [Tfs·Th, 0, 0, Tfs, 0, Trise]`

### `python/pdfspine/_llms/docs/api.md` 的标准（给大模型看的，本次重点交付物）
- 每个新键给出：类型、单位、坐标空间、退化值、**一个具体数值例子**。
- 一小节讲清**三个坐标空间**（PDF 用户空间 / 设备空间 / text space 的 glyph cell）及各键归属。
- 写出上面三条不变量。
- 写清 `rendered_size` = `sqrt(|det|)`（fitz rawdict 语义），并注明与 `get_texttrace()` 的
  `|(a,b)|` **不同**。
- 写清 `size` 目前是 **declared（`Tf` 原值）**、**fitz 的是 rendered**，这是已知 parity 差异，
  要字号请用 `rendered_size`。措辞中立准确，别承诺会改。
- 讲清 `seq`(painting order) 与 `number`(reading order) 的区别，带上 `number` 的已知局限。
- 一段可运行示例：从 rawdict 取一个 char 的 `matrix`/`quad`，验证不变量。

---

## 5. 环境与命令

```bash
unset CONDA_PREFIX                       # 每条命令前都要
# cargo 用绝对路径，不要 export PATH（沙箱会拒整条命令）
/Users/linhan/.cargo/bin/cargo fmt --all -- --check
/Users/linhan/.cargo/bin/cargo clippy --workspace --all-targets --all-features -- -D warnings
/Users/linhan/.cargo/bin/cargo test --workspace --all-features
```

Python 侧（worktree 自带 `.venv`，py3.12；**主仓 `.venv` 不要动**）：
```bash
unset CONDA_PREFIX
env VIRTUAL_ENV="$PWD/.venv" \
    PATH="$PWD/.venv/bin:/Users/linhan/.local/bin:/Users/linhan/.cargo/bin:/usr/bin:/bin" \
    maturin develop --release --uv       # 注意 --uv，否则报 "Failed to find pip"
.venv/bin/pytest python/tests
```

长命令一律 `nohup ... > 日志 2>&1 &` + 轮询，别撞 10 分钟超时。

---

## 6. 已知项 / 坑

- **`clippy` 特性子集假阳性**：`cargo clippy -p pdf-render --all-features` 会报
  `crates/pdf-core/src/document.rs:986` 的 `clippy::never_loop`。
  这是**特性组合产物**（唯一的 `continue` 藏在 `#[cfg(feature = "encryption")]` 里，
  单包编译时被 cfg 掉）。
  ✅ **`cargo clippy --workspace --all-targets --all-features` 是绿的**（零 error 零 warning），
  正式门不受影响，**不要去"修"它**。
- `interp.rs` 有一处**属性错位**：孤立的 `#[allow(clippy::too_many_arguments)]` 挂到了只有 1 个
  参数的 `normalize_cjk_radicals` 上；真正需要它的 `emit_glyph_into` 另有一份。别被误导。
- `layout.rs` 有两处 **doc comment 错位**：`build_line` 的 doc 挂在 `drop_phantom_whitespace` 上、
  `along_span` 的 doc 挂在 `spacing_along` 上。别被误导，也别顺手改（最小改动）。
- `svg.rs` 有一条与实现不一致的注释（写 "flipping y" 但 `ny` 并没取负，实际靠外层 `<g>` 做 y-flip）。

---

## 7. 原环境 scratchpad 资产（历史；当前已用可追踪清单取代）

`/private/tmp/claude-502/-Users-linhan-startup-spine/b8ac4adb-94a4-4e9b-9547-3a9e21ef1393/scratchpad/`

- `reading-order-root-cause.md` —— **阅读顺序根因与根治方案（413 行）**。本任务不根治，
  但 `reading_order` 的文档措辞要参考它。**这份很重要，另一台电脑上没有的话不要凭空重写。**
- `oracle_size/` —— §3① 的 PyMuPDF 探针原始 PDF 与 dump。
- `corpus_diff/` —— 300 文档语料回路：`run_engine.py` → `compare.py` → `classify2.py`。
  `out/fitz/` 300 份**已缓存**，只需跑我们这侧。
  历史基线：**over 334/83、under 332/160**。
- `wt-ab/DESIGN.md` —— 本文档的早期草案（内容已并入本文）。
- `conformance/gt/run_gt.py`（在仓库里）—— born / CJK / Arabic / EUR-Lex 子集。
  历史基线：EUR-Lex mean lev 0.9248 / mean order 0.9771；born 1.0000 / 0.9803。

**未完成的基线测量**：语料侧 300 文档已跑完（`out/pdfspine`），
但 `compare.py`/`classify2.py` 汇总、GT 四子集、性能/内存基线**都还没出数**
（跑基线的 agent 被 watchdog 判停在 EUR-Lex 子集）。接手后 T5/T6/T7 需要这些数据，得重跑。

---

## 8. commit 规范

Conventional Commit，body 写清动机与数据，末尾加：

```
Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01FrgNdzHnsp6pGgLyL4HfTy
```

提交时**只 add 自己改的路径，绝不 `git add -A`**。

---

## 9. 历史进度更新（其中陈旧状态已被 §10–§12 取代）

### 9.1 已推送到 `origin/worktree-agent-ab0626e7f9bd0c95d` 的 4 个 commit

| commit | 内容 | 验证状态 |
|---|---|---|
| `95f7205` | SVG 后端改用真 `Trm`（`SVGTRM-001..004`） | 门全绿 |
| `5b728d3` | Rust 层携带完整字形几何（`GLYPHGEO-001..009`） | fmt / clippy 零警告 / test 1682-0 |
| `16c30e0` | 阅读顺序根因 + 本交接文档入库 | — |
| `d05ff28` | **字段贯通到 Python 侧**（+829） | **仅 `cargo check` 零 error；全量门与实测证据未跑完** |

**§2 的 T2/T3/T4 已由 `d05ff28` 基本完成**（10 个新键全部发布）：

- span：`declared_size` / `rendered_size` / `matrix` / `text_matrix` / `ctm` / `dir` / `quad` / `seq`
- char（rawdict）：`matrix` / `quad` / `rendered_size` / `seq` / `synthetic`
- line：`number`(阅读序) / `seq`(绘制序)；block：`seq`

⚠️ 但 **`d05ff28` 的全量门尚未跑完**，且**还没有"真实 PDF 读回新字段"的实测证据**。
接手第一件事：跑完整门 + 打印一份真实 PDF 的 `rawdict`，验证
`(0,0)·matrix == origin` 与 `quad` 外接矩形 `== bbox`。

### 9.2 主仓 main 已前进 —— 需要 rebase

主仓 main 现在是 `aaee2a9 fix(pdf-text): read a two-column correlation table row by row`
（在 `layout.rs` 的 `is_table_dominant` 旁加了 `is_two_column_record_grid` 路径，
并加了 `LAYOUT-ORDER-004/005/006`）。

`git rebase aaee2a9`（主仓 main 在同一个 repo 里，`git fetch` 拿不到）。
**预期冲突点**：`layout.rs`、`layout_unit.rs`、`docs/test-case-catalog.md`（两边都加了测试 ID，**保留双方**）。

⚠️ **本分支已经 push 过**。rebase 会重写历史，之后需要 `--force-with-lease`。
如果另一台电脑已经 checkout 了这个分支，force push 前务必先协调。

### 9.3 rebase 后要用的新基线（双列修复之后）

- 300 文档语料：仍是 **over 334/83、under 332/160**（双列修复对它逐位无影响）
- GT EUR-Lex：`0.9287/0.9486、0.9811/0.9852` ← **原文如此，四个数字的确切含义
  （mean/median × lev/order）未经核实，别直接当验收标准，自己重跑一遍拿确定口径**
- GT born / CJK / Arabic：逐位不变
- GT PMC 7 篇 order：`0.939 / 0.996`

### 9.4 剩余工作 —— 准确的当前状态

**已完成，不要重做**（证据见 §10 与 `conformance/GLYPH-GEOMETRY-REPORT.md`）：

- ~~A · 跑完整门 + Python 实测证据~~ ✅ 全绿，含真实 PDF 读回与三条不变量
- ~~B · rebase 到 `aaee2a9`~~ ✅ 已在远端 feature 历史完成，9 个 commit 零冲突。
  **rebase 后复跑全量测试仍全绿**，其中
  `LAYOUT-ORDER-004/005/006`（双列对照表）与 `GLYPHGEO-001..015` 同时通过 ——
  两边虽同改 `layout.rs`，但双列修复动的是 **region 选择策略**、本次动的是
  **`DevGlyph` 几何携带 + `build_line` 字段填充**，不在同一条决策路径上，故不冲突。

**当时仍未做**（现状见 §12）：

| 项 | 内容 | 验收口径 / 已知基线 |
|---|---|---|
| **F** | **span 聚合收紧**（用户诉求中唯一未落地的一块，最大工作量） | 见下 §9.4.1 |
| C | 300 文档语料 over/under 不退化 | 基线 **over 334/83、under 332/160**；回路已提交在 `conformance/corpus-diff/`，本机语料与两侧缓存均缺，需按 README 同批重建 |
| D | GT 子集不退化 | **EUR-Lex lev 0.9287/0.9486、order 0.9811/0.9852**（双列修复后新基线，mean/median 口径请在 `run_gt.py` 输出里自行确认）；**born / CJK / Arabic 逐位不变**；**PMC 7 篇 order 0.939/0.996** |
| E | 性能：大文档抽取耗时 + 峰值 RSS | 无历史基线，需先在改动前测一次做对照。`PositionedGlyph` 已从 168 → **344 字节**（+176 = 3×Matrix + Rect），`Char` 56 → 184，需确认大文档上的实际代价 |
| G | `size` declared-vs-rendered 的 parity 决策 | fitz 报 rendered（`sqrt(\|det\|)`，§3① 有完整探针表），我们报 declared。改动会连带影响 `build_line` 的词切分阈值、`to_html` 的 `font-size`、`to_xml` 的 `<font size=>`，**必须用 C/D 的语料实测决定**，不要凭判断改 |

C/D/E 是计算密集型验证，适合在算力空闲的机器上跑。F 需要边改边验（改容差就得重跑 C）。

#### 9.4.1 span 聚合收紧 —— 设计与验收

现判据（`build_line`，`can_merge`）只有 `font` + `size`(1e-6) + `color` + `flags` 四项，
**完全没有几何判据** —— 这就是"交错字符"的来源。

新增分割判据（相邻字形之间，任一命中即断开 span），三条容差**全部相对化**
（分母 `rendered_size`）、提为具名常量以便调节：

1. **线性部分**：`render_matrix` 的 `(a,b,c,d)` 逐项差 ÷ `rendered_size` 超阈值
   —— 一条判据同时覆盖缩放 / 旋转 / 斜切 / `Tz`
2. **基线方向**：`dir` 夹角，复用既有 `dir_matches` 口径（`dot > 0.996`，约 5°）
3. **基线位置**：`|Δcross|` ÷ `rendered_size` 超阈值

**容差取值必须有数据依据**：在 300 文档语料上统计这三个量的真实分布，找自然分界点，
把直方图或分位数写进报告，不要拍脑袋取值。

**方向已由 oracle 确认**（§3②）：fitz 切得比我们**更激进**（同行不同 `Tm` 缩放 → 拆成两个
*line*；正常 + 旋转 90° → 拆成两个 *block*），所以收紧是**向 parity 靠拢**，不是偏离。
建议**只在 span 级收紧**，不动 line/block 切分 —— 这样 `get_text("text")` 的输出不变，
风险面最小，也正好对应用户原话"不应该放进同一个视觉 span"。

✅ **正常上下标不会被打散**：`flags::SUPERSCRIPT` 早就参与 `can_merge`，
正常上下标现在就已是独立 span。真正的风险方向是反的（**过度分割正常文本**），
所以容差要往"够松"一侧靠。

**正反两组测试都要有**：该分的分（相邻字符不同缩放 / 旋转 / 基线）、
不该分的不分（同一行正常文本、正常上下标）。合成 fixture，精确断言。

**验收**：C 的 over/under 不得退化（这是最灵敏的指标 —— 过度分割会直接抬高 over），
D 的四个子集不得退化，全量门全绿。**退化就停下汇报，不要调容差硬凑数字。**


### 9.5 历史环境坑：pre-push hook 推不上去

一次 pre-push hook 运行 `quality_gate.py` 时，pytest 显示解释器为
**`/opt/anaconda3/bin/python3`**，并发生以下版本断言失败：

```
assert pdfspine.VersionBind == pdfspine.__version__
AssertionError: assert '0.6.1' == '0.6.0'
```

worktree 自己的 `.venv` 中 `0.6.1 == 0.6.1`。当时据此推测 hook 选错了解释器或导入路径，
但没有保存足以证明完整因果链的诊断记录；因此不能把该推测当作已确认根因，也不能据此
断言所有 worktree push 都会失败。后续若复现，应先记录 hook 实际解释器、`sys.path`、
导入模块路径和环境变量，再修选择逻辑。

---

## 10. 验证结果 —— **推翻 §9.1 与 §9.4-A 的「未验证」标注**

§9 写于验证跑完之前。实际上**全量门与端到端实测都已跑完，且全部通过**。
§9.4 的 A 项（"跑完整门 + Python 实测证据"）**已完成，不需要再做**。

### 10.1 全量门（全绿）

| 门 | 结果 |
|---|---|
| `cargo fmt --all` | 干净 |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | **零 warning 零 error** |
| `cargo test --workspace --all-features` | **1687 passed / 0 failed** |
| `maturin develop --release --uv` | 成功（`pdfspine-0.6.1-cp311-abi3-macosx_11_0_arm64`） |
| `pytest python/tests` | **780 passed / 66 skipped / 0 failed** |

### 10.2 新增测试

- Rust `GLYPHGEO-010..015`（`crates/pdf-text/tests/glyph_geometry.rs` 共 15 条全绿）
- Python `PYGEO-001..009`（`python/tests/test_glyph_geometry.py`）
- `PYTEXT-003` / `PYTEXT-006` 的精确键集合断言已补上新键

### 10.3 原始发布版真实 PDF 实测（pre-G；`fixtures/born/pangrams.pdf`，`get_text("rawdict")`）

```
block[0]  {'number': 0, 'type': 0, 'bbox': (72.0, 62.4, 393.444, 116.4), 'seq': 0}
line[0]   {'wmode': 0, 'dir': (1.0, 0.0), 'bbox': (72.0, 62.4, 312.768, 74.4),
           'number': 0, 'seq': 0}

span[0]   size          12.0          declared_size 12.0
          rendered_size 12.0          flags         0
          font          'Helvetica'   color         0
          ascender      0.8           descender     -0.2
          origin        (72.0, 72.0)
          bbox          (72.0, 62.4, 312.768, 74.4)
          matrix        (12.0, 0.0, 0.0, -12.0, 72.0, 72.0)
          text_matrix   (1.0, 0.0, 0.0, 1.0, 72.0, 720.0)
          ctm           (1.0, 0.0, 0.0, 1.0, 0.0, 0.0)
          dir           (1.0, 0.0)
          quad          (72.0, 62.4, 312.768, 62.4, 72.0, 74.4, 312.768, 74.4)
          seq           0

chars[0]  origin (72.0, 72.0)   bbox (72.0, 62.4, 79.332, 74.4)   c 'T'
          matrix (12.0, 0.0, 0.0, -12.0, 72.0, 72.0)
          quad   (72.0, 62.4, 79.332, 62.4, 72.0, 74.4, 79.332, 74.4)
          rendered_size 12.0   seq 0   synthetic False
```

**三条不变量在真实 PDF 上全部通过**：
1. `(0,0)·matrix == origin` → `(72.0, 72.0)` ✅（span 与 char 均通过）
2. `quad` 外接矩形 == `bbox` ✅（bbox 侧有 1e-14 级浮点噪声，`isclose` 通过）
3. `matrix == params · text_matrix · ctm · page_transform` → 复合结果
   `(12.0, 0.0, 0.0, -12.0, 72.0, 72.0)` 与 `matrix` 逐值相同 ✅

另：`rendered_size == sqrt(|det|) == 12.0` ✅；
`get_text("xml")` 的 `<char quad>` 与 rawdict `quad` **逐值相同** ✅（T3 生效）。

### 10.4 原始发布版合成 PDF 补充实测（pre-G）

- `BT /F1 1 Tf 12 0 0 12 100 700 Tm (Hi) Tj ET`
  → `size == declared_size == 1.0`、**`rendered_size == 12.0`**、
  `matrix == (12,0,0,-12,100,92)`、`text_matrix == (12,0,0,12,100,700)`；
  char `"H"` 的 `quad == (100, 82.4, 106, 82.4, 100, 94.4, 106, 94.4)`
  —— **这正是"下游不必再靠 bbox 反推字号"的证据**：declared 1.0 而 rendered 12.0。
- 斜切 `Tm 12 0 6 12` → char `matrix == (12.0, 0.0, 6.0, -12.0, 100.0, 92.0)`，
  `quad == (104.8, 82.4, 110.8, 82.4, 98.8, 94.4, 104.8, 94.4)`
  —— 真平行四边形，`ll.x − ul.x == −6`
- 旋转 90° `Tm 0 12 -12 0` → `dir == (0.0, -1.0)`、
  `matrix == (0.0, -12.0, -12.0, -0.0, 100.0, 92.0)`
- `page.transformation_matrix` 实测 `(1.0, 0.0, 0.0, -1.0, -0.0, 792.0)`

### 10.5 quad 角命名语义 —— ✅ 已复核

`layout.rs` 的 `DevGlyph::new` 里，glyph cell 的 quad 现在是**先在字形自身坐标系里命名四角**
（`ul` = ascender-left）再做变换，而不是 `Quad::from_rect(&g.cell)` 之后再变换。

原因：cell 在 text space 是 y 向上（`y0` 是 descender），`from_rect` 会把 `ul` 取成 descender 角，
经设备空间 y 翻转后 `ul` 实际落在**视觉下方** —— 正立文本的 `ul`/`ll` 会颠倒。

修正后 `quad.rect()` 不变，因此 `bbox` 与所有既有输出**逐字节不变**（XML golden 仍绿）。
已用 PyMuPDF 1.28.2 对正立、斜切、内容流旋转 90° 三种自生成 PDF 复核。
MuPDF 同样先在 glyph 的 y-up 坐标框架中命名 `ul/ur/ll/lr` 再变换；旋转 90° 后
`ul -> ur` 沿旋转后的基线向上，`ul -> ll` 向右，不会按最终视觉位置重新命名。
`PYGEO-009` 固化此拓扑，可复现双引擎脚本为
`conformance/probe_glyph_quad_corners.py`。两边 Helvetica fallback 的竖直 metrics 不同，
所以绝对 quad 数值不相等；探针对比的是归一化边方向。

同一探针另发现页面字典 `/Rotate` 的坐标基准差异：PyMuPDF 1.28.2 的文本 XML
仍用未旋转页坐标，pdfspine 会应用 page transform。此项与角命名无关，现作为已知
rotated-page 差异记录在文本提取文档中。

### 10.6 后续状态

§9.4 的 C–G 已在本机继续执行；最终结果见 §12。

## 11. 历史续接环境状态（2026-09-05；已被顶部入口取代）

- `git fetch origin` 后 glyph 远端 ref 从旧 `3de8240` 更新到 `9912a23`。新历史包含
  `aaee2a9`、等价的 9 个 rebased glyph commits、rebase 后验证记录，以及已提交的
  `conformance/corpus-diff/`。`range-diff` 确认旧、新 9 个 glyph commits 逐一等价。
  本地 `glyph-geometry-continue` 已同步并跟踪该远端 ref；旧尖端由本地分支
  `glyph-geometry-pre-sync-3de8240` 保留。`origin/main` 仍为 `75a1ace`，未修改、未 push。
- **B 已完成**：`aaee2a9` 是 `9912a23` 的祖先，rebase 后 1690 个 Rust 测试及双方
  定向测试的持久证据见 §9.4 与验证报告。
- `.venv-oracle` 已按 PyMuPDF 1.28.2 + `pdfminer.six` 重建；`.venv` 是搬迁环境，`activate` 内仍带旧
  路径，使用时显式设置 `VIRTUAL_ENV`，并确保 `~/.cargo/bin` 在 Homebrew cargo 前，
  才会采用仓库要求的 rustc 1.96。以该方式 `maturin develop --release --uv` 已成功。
- 本节记录的是同步刚完成时的环境快照。随后恢复的精确语料、缓存和验收结果见 §12；
  不应继续按此处“尚未下载/缓存缺失”的旧状态安排工作。

## 12. 当前验收状态（2026-09-05）

| 项 | 状态 | 持久证据 |
|---|---|---|
| A | ✅ 完成 | §10；全量 Rust / Python 门与真实 PDF 读回 |
| B | ✅ 完成 | `aaee2a9` 是 `9912a23` 祖先；rebase 后 1690 Rust 测试全绿 |
| C | ✅ 完成 | `conformance/GLYPH-GEOMETRY-CORPUS-REPORT.md`；精确 300 件 manifest 下 baseline/current/F/G 均 0/300 word/text/font 差异 |
| D | ✅ 完成 | `conformance/GLYPH-GEOMETRY-GT-REPORT.md`；30 件固定子集逐文档分数和字符数完全一致 |
| E | ✅ 已测量并缓解 | `conformance/GLYPH-GEOMETRY-PERFORMANCE-REPORT.md`；优化后仍有显著 rawdict 时间/RSS 成本 |
| F | ✅ 完成 | `conformance/GLYPH-GEOMETRY-SPAN-REPORT.md`；阈值、8523 个新边界、负例和结构代价均已固化 |
| G | ✅ 完成 | `conformance/GLYPH-GEOMETRY-SIZE-PARITY-REPORT.md`；完整 300 件差分、专项门和最终统一门通过 |

### 12.1 C：可重建的 300 文档差分基线

历史 `over 334/83、under 332/160` 所依赖的私有文件列表、指纹、oracle 版本和斜杠列含义
均未保留，不能严格复现，也不能拿任意 300 件与其作数值比较。新基线固定在
`conformance/corpus-diff/baselines/glyph-geometry-2026-09-05-manifest.json`：每项含仓库相对路径、
source ID、大小和 PDF SHA-256，portable fingerprint 为
`87804b5a316632e3920ec198f24bdc26c50131a6344bd51dd522a858963e82c0`。语料构成为
FinTabNet 132、GovDocs1 100、EUR-Lex 41、fixtures 9、Arabic 9、born 6、CJK 3；共 1887 页。
后续 `32e6232`（typeset TS-12）改写了 `fixtures/typeset/typeset-lo-slide.pdf`，该 manifest 对
当前工作树已无法整体校验；现行清单是 `build_corpus.py --refresh-stale` 派生的
`glyph-geometry-2026-09-05b-manifest.json`（指纹 `6c7126de…`，只此一条更新，旧哈希留在
`previous_*`，顶层 `supersedes` 指回本文件）。本节及 C–G 报告引用的指纹仍指旧文件，旧文件不改。

同一清单上，`aaee2a9`、`9912a23`、post-F、post-G 的 extraction JSON 均逐文档一致。
固定 PyMuPDF 1.28.2 比较为 over 137、under 329、mixed 130，oracle/pdfspine 均零提取错误。
这些数字是新的可复现基线，不宣称重现旧 `334/332` 数字。

### 12.2 D：固定 GT 子集

born 6、CJK 3、Arabic 9、EUR-Lex 历史希腊文切片 5、PMC 历史 clean slice 7，共 30 件，
在 `aaee2a9`、`9912a23` 和 post-F 间逐文档 unrounded metric、字符数及 aggregate 完全一致。
PMC 通过新的官方匿名 `pmc-oa-opendata` S3 metadata 精确恢复，重现 order mean/median
`0.9391/0.9962`。EUR-Lex 验收只跑了历史 5 件 / 352 页切片，不是完整 40 件 / 2816 页。

### 12.3 E：性能结论

原 geometry build 在 118 页 EUR-Lex 样本上令 streamed rawdict 时间增加 85.9%、retained
rawdict 峰值 RSS 增加 141.6%。复用 Python key 与不可变 float 后，retained rawdict RSS 从约
699 MiB 降至 430 MiB；相对 pre-geometry 同窗口仍增加 59.0% streamed rawdict 时间、65.3%
retained rawdict 时间和 48.9% retained rawdict RSS。这是已量化并缓解的显著成本，不是零退化。
最终同输入复测中，F/G 相对已优化 E 快照增加 0.19% streamed、0.75% retained rawdict
时间，RSS 增加 1.031 MiB / 0.297 MiB；全部模式 elapsed 最大增幅 3.25%。这说明 F/G
没引入新的大幅退化，不改变上述相对 pre-geometry 的残余成本结论。

### 12.4 F：span 几何门

最终阈值为 linear `<= 0.05 + 1e-9`、baseline `<= 0.1 + 1e-9`、direction dot `> 0.996`；
奇异矩阵走有限线性部分精确相等且 absolute baseline `<= 1e-6`。冻结语料中新增 8523 个
span 边界，涉及 79/300 文档；1887 页、6812936 个字符的扁平几何投影完全不变。
420 个 alphabetic run 跨新 span 边界，三个变成逐字符 span，其中包含正常缩写 `HPA`；
leader-dot 长串也明显碎片化。这些是接受的真实结构代价，不能表述为“无过拆”。

### 12.5 G：`span["size"]` 决策

已接受把结构化 `DictSpan.size` 改为首字形的 `rendered_size`，同时保留内部 `Span.size` / `Tf`
供布局及 HTML/XML 既有语义使用。300 件上 5803856 个与 PyMuPDF 双向唯一匹配的 glyph 中，
5495174 个字号误差改善、308367 个不变、315 个恶化；mean absolute error 从 8.1822137 pt
降至 0.000114815 pt，最大 residual 为 0.6996063 pt（5.83005%）。315 件 residual 只出现在
两个 GovDocs1 文档的 41 个 span（文档 00018 / 00053），原因是 F 容许同 span 内最多约 5%
的相邻 geometry 变化，而公开 span size
只能取首字形值。候选值 100% 等于该 span 的 `rendered_size`；F/G 间 text、words、rawdict
除 span `size` 外的 1887 页投影全部一致。因此这是大幅 parity 改善，不是完美逐 glyph parity。
完整匹配、容差口径、315 件逐项归因和复现命令见
`conformance/GLYPH-GEOMETRY-SIZE-PARITY-REPORT.md`。

### 12.6 最终统一门

G 落地后的最终门全部通过：`cargo fmt` clean；clippy `-D warnings` clean；Rust workspace
all-features **1702 passed / 0 failed / 1 个显式 profiling ignored**；共享 maturin release build
成功；Python 在 `-W error --doctest-modules` 下 **814 passed / 63 skipped / 0 failed**；Ruff
format/check、mypy、cargo-deny 和四个 drift guard 均通过。首次 Python 运行的三项失败只来自
PyMuPDF 1.28 对旧 `fitz` compatibility import 写 stdout 的 deprecation warning；三个子进程入口
改用 `import pymupdf as fitz` 后复跑全绿，没有靠 skip 绕过。
