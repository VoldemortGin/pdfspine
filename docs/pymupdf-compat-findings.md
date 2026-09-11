# PRD：pdfspine 与 PyMuPDF 行为差异修复

- 状态：Draft v0.1
- 日期：2026-08-03
- 发现环境：pdfspine 0.5.0 / PyMuPDF 1.28.0 / macOS 26.5.1 (Apple M4 Max) / Python 3.12
- 发现场景：中文财务年报（友邦人寿 2010–2025 年度信息披露报告，16 份 PDF，1311 页）的 RAG 语料解析
- 报告人：Claude Code（受 aia GraphRAG 调研项目委托）

## Problem Statement

pdfspine 以「drop-in-shaped、88.7% PyMuPDF 公开 API 已实现并测试」为核心卖点。
在把一条既有 PyMuPDF 解析流水线切换到 pdfspine 时，发现三处行为差异。
三者都**不抛异常、不报警告**，而是静默产出不同结果——对下游 RAG/检索场景，
表现为「答案页码错误」「整页正文消失」，且很难归因到解析层。

按影响排序：

| 编号 | 问题 | 严重度 | 静默失败 |
|---|---|---|---|
| P0 | 页面索引整体偏移，`d[0]` 返回 PyMuPDF 的 `d[1]` | 阻断 | 是 |
| P1 | `get_text('blocks')` 分块粒度与 PyMuPDF 差异巨大 | 高 | 是 |
| P2 | `get_text('text', sort=True)` 中 `sort` 不生效 | 中 | 是 |

---

## P0：页面索引偏移（阻断级）

### 现象

`page_count` 与 PyMuPDF 一致，但按下标取页得到的内容整体错位一页。
内容稀少的封面页（约 35–60 字符、6 个 word）似乎被跳过。

### 复现

```python
import fitz
import pdfspine.fitz as ps

F = "友邦人寿2020年年度信息披露报告.pdf"
a, b = fitz.open(F), ps.open(F)
print(a.page_count, b.page_count)          # 77 77   —— 页数一致
print(repr(a[0].get_text()[:40]))          # '友邦人寿保险有限公司 2020 年年度信息披露报告…'  （封面）
print(repr(b[0].get_text()[:40]))          # '- 2 - 目 录 一、公司简介 ……'                （目录，实为第 2 页）
```

### 实测范围

在 16 份 PDF 上对比 `d[0]` 的文本：

- **10/16 文件首页内容不一致**
- 其中 8 份为明显偏移：PyMuPDF 首页 35–60 字符（封面），pdfspine 首页 1351–2527 字符（目录）
- 另 2 份（2023 年报等）仅有 2 字符级差异，属另一类小差异，不在此问题范围

受影响文件的共同特征：**首页为纯封面，文本极少**。
不受影响的文件（2010/2011/2012/2013/2024/2025）首页本身即含较多文本。

### 期望行为

`doc[i]` 必须与 PyMuPDF 的 `doc[i]` 指向同一物理页，无论该页文本多寡；
空白页或近空白页也必须占据其索引位置。

### 影响

1. 任何页码溯源全部错位——财务/法务场景中，引用页码错误等同于引用失效。
2. 依赖 `doc[i]` 做分页处理的流水线（分块、缓存、增量更新）全部错位，且不报错。
3. 与 `page_count` 一致这一点会强化误判：调用方通常用页数是否相等来做健全性检查，而该检查会通过。

### 建议

- 排查页树遍历是否跳过了无 `/Contents` 或内容极少的页对象。
- 增加回归用例：构造「首页为纯封面/空白页」的 fixture，断言 `doc[i]` 与 PyMuPDF 逐页文本哈希一致。
- 建议在 conformance 套件中加入「逐页文本哈希序列一致性」这一整体断言，而不只断言 `page_count`。

---

## P1：`get_text('blocks')` 分块粒度差异（高）

### 现象

同一页，PyMuPDF 返回行/段级块，pdfspine 返回覆盖整页的巨块。

### 复现

```python
F, PAGE = "2010年度信息披露报告_final version_110415.pdf", 55   # 物理第 56 页
# PyMuPDF ：33 个非空块，单块高度约 10–22pt，形如 (74,111,195,121)
# pdfspine： 2 个非空块，其中一块 bbox=(74,111,522,804) 覆盖整页，含 828 字符
```

### 期望行为

块粒度应与 PyMuPDF 大致一致（行/段级）。若刻意采用不同的聚合策略，
应在文档中显式标注，并提供获取细粒度块的等价途径。

### 影响

块级 bbox 是版面处理的基本单位。粒度不一致会让所有「按区域过滤/保留」的逻辑失效。

本项目的实际事故：流水线用 `find_tables()` 得到表格 bbox，再删除与之相交的块，
以避免表格内容重复出现。在 PyMuPDF 上只删掉表格区域内的几个小块；
在 pdfspine 上，那个覆盖整页的巨块必然与表格相交，**整页正文被删除**——
2010 年报 p56 由 866 字符降至 123 字符，且无任何报错。

（注：该项目自身的「相交即删」判据也过于粗糙，已改为按面积占比判定；
但块粒度差异会把此类问题的后果放大一个数量级。）

### 建议

- 对齐块聚合策略，或在 `docs/gotchas.md` 中明确列出该差异及迁移注意事项。
- 增加 conformance 用例：对比两库在同一页的块数量与 bbox 分布，容忍阈值内比对。

---

## P2：`get_text('text', sort=True)` 中 `sort` 不生效（中）

### 现象

`sort=True` 在 `option='blocks'` 时生效，在 `option='text'` 时不生效；
PyMuPDF 两种模式下均生效。

### 复现

测试页：2024 年报物理第 29 页「六、偿付能力信息」。
该表为两列版式（左标签、右数值），PDF 按列写入，同一视觉行的 `y0` 严格相等（如 118.629）。
判据：标签之后是否紧邻其对应数值（5 个指标）。

```
                                  PyMuPDF   pdfspine
get_text('text')                    3/5       3/5
get_text('text', sort=True)         5/5       3/5      ← 差异点
get_text('blocks', sort=True)       5/5       5/5
```

### 期望行为

`sort=True` 在所有支持该参数的 `option` 下语义一致：按 `(y, x)` 重排后再序列化。

### 影响

两列版式（财务报表中极常见）在未排序时会输出
`标签A 标签B 数值A 数值B`，标签与数值的对应关系丢失，
下游 LLM 极易错配（把 12,752,275 当作核心资本）。
调用方按 PyMuPDF 经验传入 `sort=True` 会以为已规避，实际未生效。

### 建议

- 让 `text` 模式复用与 `blocks` 模式相同的排序路径。
- 增加 conformance 用例：两列版式 fixture，断言 `sort=True` 下两库输出的 token 顺序一致。

---

## 附：一个可选增强（非缺陷）

`sort` 是按 `(y, x)` 精确排序。实践中不少 PDF 同一视觉行的 `y` 存在零点几的抖动，
精确排序会把一行拆成两行。可考虑提供带容差的分行选项（如 `get_text('layout')`，
按 y 容差分行、行内按 x 排序）。本项目自行实现了词级 + 3pt 容差的版本，
在该语料上把两列表格的标签-数值紧邻率由 3/5 提升至 5/5。

不建议将 `sort` 默认值改为 `True`——PyMuPDF 默认为 `False`，
改默认会破坏 drop-in 兼容承诺，而兼容性正是 pdfspine 的核心价值主张。

## 附：刻意偏离 PyMuPDF 的语义（非缺陷）

以下行为 pdfspine 与 MuPDF / PyMuPDF 不同，属有意为之，不作为兼容缺陷处理，也不会为了 drop-in 一致而回退。
第 1–3 条是 OCG 可见性，按 ISO 32000-1 实现，判定代码在 `crates/pdf-core/src/ocg.rs`（模块头
"Deliberate divergences from MuPDF / PyMuPDF" 注释是权威说明）；第 4 条是 `sort=False` 的文本块顺序，
判定代码在 `crates/pdf-text/src/layout.rs`，设计与 300 文档实测见 `docs/reading-order-root-cause.md`。
测试 id 见 `docs/test-case-catalog.md`。

| # | 场景 | PyMuPDF 行为 | pdfspine 行为 | 依据 | 覆盖测试 |
|---|---|---|---|---|---|
| 1 | OCMD `/OCGs [A B] /P /AllOn`（或 `/AnyOff`），成员状态不一 | MuPDF 1.28 对 AllOn / AnyOff 的求值有误 | 按规范求值：AllOn 需全部 ON 才可见，AnyOff 任一 OFF 即可见 | ISO 32000-1 §8.11.2.2 | `OCG-VIS-OCMD-POLICIES`、`OCG-INTERP-OCMD-POLICY`、`OCG-WRITE-OCMD` |
| 2 | OCMD 带 `/VE` 可见性表达式 | 忽略 `/VE`，只按 `/OCGs` + `/P` | 求值 `/VE`（`/And` / `/Or` / `/Not` 可嵌套），且 `/VE` 优先于 `/OCGs` + `/P` | ISO 32000-1 §8.11.2.2 | `OCG-VIS-OCMD-VE`、`OCG-INTERP-OCMD-VE`、`OCG-VIS-USAGE-OCMD` |
| 3 | OCG 在活动配置中 OFF（或 `/BaseState /OFF`），但 `/Usage /View /ViewState /ON`，且活动配置的 `/AS` 有 `/Event /View`、`/Category` 含 `/View` 的条目列出该 OCG | 隐藏——MuPDF 完全忽略 `/AS`（`pdf-layer.c` 有 FIXME 承认应处理） | 显示——usage application dict 决定状态；面板 override OFF 仍可压过它 | ISO 32000-1 §8.11.4.4 | `OCG-VIS-USAGE-AS-PROMOTE`、`OCG-VIS-USAGE-AS-CONFIG`、`OCG-VIS-USAGE-OVERRIDE` |
| 4 | `get_text`（`sort=False`，默认）的文本块顺序 | 按 content-stream 绘制序返回块，不做几何重排（阶段 2 黑盒探针在 1.28.2 逐一实测；1.28 的 opt-in `TEXT_SEGMENT` 也只在栏内分段，栏与栏之间仍按绘制序） | 按页面几何阅读序返回块：递归 XY-cut（band 上→下、column 左→右、合法 column cut 优先于横向 band cut、通栏 spanning band 划分行、region 全原子），绘制序只保留在同一 block 内的共基线片段 | 几何阅读序设计（`docs/reading-order-root-cause.md`）；`b6c027a` 曾把块序静默改成绘制序是一次回归，本次改回并锁死 | `readorder_009`…`readorder_012`、`PYTEXT-010`（`Right < Left < Bottom`）、`LAYOUT-ORDER-002` |

说明：

- 第 3 条之外的 `/Usage` 判定与 PyMuPDF 一致：`/ViewState /OFF` 无条件隐藏（面板 override ON 也不例外）；
  `/Print` / `/Export` 不参与；`/AS` 只读活动配置（`/D` 或选中的 `/Configs[n]`）；`get_ocgs()` /
  `layer_ui_configs()` / `ocg_state()` 只反映配置 ON/OFF（`OCG-VIS-USAGE-VIEWSTATE-OFF`、
  `OCG-VIS-USAGE-PRINT-EXPORT`、`OCG-VIS-USAGE-REPORT`；判定表其余各行已用真 PyMuPDF 1.27.2 逐行核对，
  `oc=` 写入侧由 `PYOCG-046` / `PYOCG-047` 双向 oracle 覆盖）。
- `/Intent` 筛选已与真实 PyMuPDF **1.28.2** 的 64 组合矩阵核对（2026-09-10）：
  仅活动配置的非空 `/Intent` 启用筛选；配置缺省或 `[]` 不筛选，也不从 `/D` 继承到
  缺省 Intent 的备用配置。OCG 缺省 Intent 按 `/View`，显式 `[]` 则不匹配任何配置，
  包括 `/All`。单名称或数组中任一同名匹配即可，任一侧的 `/All` 可通配另一侧的名称。
  不匹配隐藏优先于面板 ON 与 `/AS` 显示提升；匹配本身不把 OFF 层变 ON。
  `get_ocgs()` / `layer_ui_configs()` / `ocg_state()` 继续只报告配置／面板状态。
  覆盖：`OCG-VIS-INTENT-MATRIX`、`OCG-VIS-INTENT-STATE`、`OCG-VIS-INTENT-CONFIG`、
  `OCG-VIS-INTENT-INDIRECT`、`OCG-INTERP-INTENT`（文字、图像与渲染绘制指令）。

## 验证方式

修复后建议以本语料回归：16 份中文财务年报 PDF，断言

1. 逐页文本哈希序列与 PyMuPDF 完全一致（覆盖 P0）；
2. 每页块数量与 PyMuPDF 的差异在阈值内（覆盖 P1）；
3. 2024 年报 p29 在 `text`/`blocks` 两种 option 下 `sort=True` 结果一致（覆盖 P2）。


## DisplayList 文字快照的返回类型与资源边界（2026-09-10）

本机 PyMuPDF 1.28.2 的 `DisplayList.get_textpage(flags=3)` 返回底层
`mupdf.FzStextPage`，需再用 `pymupdf.TextPage(...)` 包装。pdfspine 直接返回已有
公开 `pdfspine.TextPage`，提供 `extractText/DICT/RAWDICT/JSON/WORDS/search` 等方法；
这是刻意的可用性归一化，不仿造 native handle。

- DisplayList 创建时配对保存语义记录和绘制指令；文字包括不可见/裁剪模式文本。
  字体名称在对应 Form 的资源上下文解析，图像以记录 identity 关联，命名颜色空间
  在各次绘制的上下文解析。图像编码流与必要解码资源独立复制，首次需要图片时才解码。
- `flags=3` 默认不包含图片；`flags=7` 保留图片；`flags=11` 禁止自动插空格。
  后续提取沿用 TextPage 创建 flags。源内容、字体、图像修改及文档关闭不改变文字快照。
- `Page.get_displaylist(annots=True)` 默认包含可见 FreeText/widget 的当前 `/AP /N`
  外观；`annots=False` 排除。考虑 `/AS`、`/Rect`、`/BBox`、`/Matrix`、隐藏标志与 `/OC`。
  这修正了旧 DisplayList 忽略注释的行为；直接 `Page.get_pixmap` 路径没有改动。
- 独立快照保证限于新 TextPage 语义/图片资源。既有 DisplayList raster 对源 ICC、
  palette、mask 等资源的依赖未在本轮改为深快照，不能把文字关闭后存活测试当作
  所有 raster 资源编辑后的隔离证明。旧字体 bbox/版面排序及不支持的颜色模型也未改写。

可执行对照在 `python/tests/test_displaylist_textpage.py`；比较文本、字体、origin、
页面尺寸、图片 bbox 与实际 RGB 像素，未声称不同引擎字形 bbox 完全一致。

命名颜色空间的补充边界：Form 内 inline `/CS /CS1` 的局部 RGB/灰度资源与
PyMuPDF 对照一致，包括 `/F /Fl` 压缩流。图像 XObject 的 `/ColorSpace /CS1`
由 pdfspine 文字快照按当前资源上下文解析；本机 PyMuPDF 1.28.2 对该 XObject
写法报 unknown colorspace 并忽略图片，因此此项仅有本引擎实际像素回归，
不记作 oracle 相等案例。

## 注释 ID stem 与图标名称（2026-09-10）

`TOOLS.set_annot_stem(stem: str | None = None) -> str` 使用进程级配置，默认
`fitz`；`None` 查询，字符串设置并返回前 50 个 Unicode 标量字符，空字符串有效。
它影响之后创建的普通注释 `/NM`（`<stem>-A<n>`）和 widget（`<stem>-W<n>`），
在目标页所有现存 `/NM` 中寻找最小空缺编号。已有 ID 不改写；删除后空缺可复用，
保存重开仍保留 ID。`Tools()` 实例共享同一配置。

本机 PyMuPDF 1.28.2 对照覆盖 ASCII/空 stem、截断、编号空缺、页面隔离和 widget。
pdfspine 明确拒绝 bytes/list 等非字符串，不复刻 oracle 接受部分可切片类型的偶然行为；
非 ASCII `/NM` 使用合法 PDF Unicode 文本编码，不复刻 oracle 的损坏编码。

`Annot.info['id']` 读取 `/NM`，`info['name']` 读取图标 `/Name`（如 Note/Help）；
这是对旧 Python 读取行为的纠正。Rust 既有 `AnnotInfo.name`/`Annot::name()` 仍表示
`/NM`，新增 `icon_name()` 读取图标。既有 `set_info(name=...)` 继续作为本项目扩展
写 `/NM`，结果通过 `info['id']` 和 `Page.load_annot(id)` 读取；此 setter 参数并非
PyMuPDF 接口，设置图标仍用 `Annot.set_name()`。

创建路径的 ID 扫描与 page `/Annots` 追加共享互斥锁，自动 Popup 和显式新建 Popup
追加也使用该锁，widget 的 `/AcroForm /Fields` 注册受同锁保护。外观生成在锁外，
配置读锁不跨越文档访问。此保证限定这些创建路径，不是任意 xref 编辑的事务承诺。
回归见 `python/tests/test_tools_annot_stem.py` 与 `pdf-edit` 注释模块并发测试。
`Tools.set_subset_fontnames` 仍未实现。
