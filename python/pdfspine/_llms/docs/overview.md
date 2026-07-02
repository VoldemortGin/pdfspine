# pdfspine — Overview (是什么 / 解决什么)

> 面向 AI 消费的文档。所有签名、行为均核对自真实源码与运行验证。pip 安装名 `pdfspine`，import 名 `pdfspine`。

## 一句话定位

**pdfspine 是 PyMuPDF（`fitz`）的纯 Rust 重实现，通过 PyO3 暴露 Python API。** 它不依赖 MuPDF/pdfium 这类 C 库，是自包含的 abi3 wheel；许可证为 **Apache-2.0**（PyMuPDF 是 AGPL-3.0），便于闭源/SaaS/宽松许可项目使用。

- 引擎：纯 Rust（PDF 解析 / 渲染 / 文本 / OCR 全部 Rust 实现），`_core.abi3.so` 是编译产物。
- 目标：**“PyMuPDF 有的我们都要”**——按 PyMuPDF 1.24.x 基线做符号级对齐（见 PARITY.md / COMPAT.toml）。
- 当前覆盖：约 **88.7%（682/769）** 的 PyMuPDF 1.24 公开 API 已实现且有测试；其余为 deferred（21）或 out-of-scope（66）。

## 核心概念（对象模型）

pdfspine 的对象模型与 PyMuPDF 一一对应。顶层 import 即可拿到所有公开类型：

| 类型 | 作用 | 怎么拿到 |
|---|---|---|
| `Document` | 一个打开的文档（PDF 或被转成单/多页 PDF 的图片） | `pdfspine.open(...)` |
| `Page` | 一页 | `doc[i]` / `doc.load_page(i)` / `for page in doc` |
| `TextPage` | 一页的文本快照（多次抽取复用，避免重复解析） | `page.get_textpage()` |
| `Pixmap` | 一块像素位图（渲染结果） | `page.get_pixmap(...)` |
| `DisplayList` | 一页的渲染指令流（可重复 `get_pixmap`） | `page.get_displaylist()` |
| `Annot` / `Widget` | 注释 / 表单字段控件 | `page.annots()` / `page.widgets()` |
| `Shape` | 矢量绘图累加器（draw → finish → commit） | `page.new_shape()` |
| `TableFinder` / `Table` | 表格检测与抽取 | `page.find_tables()` |
| `TextWriter` | 高级文本排版写入器 | `pdfspine.TextWriter(rect)` |
| `Link` / `Outline` | 链接 / 书签节点 | `page.links()` / `doc.outline` |
| `Font` / `Colorspace` | 字体度量+程序句柄 / 颜色空间 | `pdfspine.Font(...)` / `pdfspine.csRGB` |
| 几何值类型 | `Rect` `IRect` `Point` `Matrix` `Quad` | `pdfspine.Rect(...)` 等，PyMuPDF 兼容语义 |

几何类型（`pdfspine.geometry`）是与 PyMuPDF 完全兼容的值类型：支持运算符（`*` 变换、`|` 并、`&` 交、`+`/`-`）、属性（`width`/`height`/`tl`/`br`/`quad`/`irect`…）和方法（`transform`/`intersect`/`include_rect`…）。坐标系与 PyMuPDF 一致（原点左上，y 向下）。

## fitz / pymupdf 兼容层（关键）

pdfspine 自带 PyMuPDF 兼容 shim，让大量现有 `import fitz` 代码**几乎不改**即可运行：

- 子模块 **始终可用**，不污染全局名：
  ```python
  import pdfspine.fitz as fitz        # 或 from pdfspine import pymupdf
  doc = fitz.open("in.pdf")
  ```
- 想让裸 `import fitz` / `import pymupdf` 解析到 shim，需**显式调用一次**：
  ```python
  import pdfspine
  pdfspine.install_fitz_shim()        # 之后 import fitz -> pdfspine 的 shim
  import fitz
  ```
- **默认不抢占全局名**：未调用 `install_fitz_shim()` 时，`import fitz` 不会被 pdfspine 接管，因此可与真正的 PyMuPDF 共存。`install_fitz_shim()` 用 `dict.setdefault` 注册，**绝不覆盖**已先导入的真 PyMuPDF。
- shim 额外提供 PyMuPDF 的异常名别名（映射到 pdfspine 的类型层级）：`FileDataError = PdfSyntaxError`、`EmptyFileError = PdfSyntaxError`、`mupdf_display_errors = PdfError`，以及内建 `FileNotFoundError`。
- shim 里**尚未实现**的 PyMuPDF 名不会抛 `AttributeError`，而是抛 `PdfUnsupportedError`（带提示，指向 parity matrix）。

## OCR（PaddleOCR + Tesseract）

- OCR **引擎代码已编进 wheel**（纯 Rust PaddleOCR，PP-OCRv5 det/rec + PP-LCNet_x1_0 textline-ori；以及 Tesseract 适配器）。
- **~28MB PP-OCRv5 ONNX 模型也已内嵌进 wheel**（装后位于 `site-packages/pdfspine/_models/`）。所以一个**裸 `pip install pdfspine` 即全功能 OCR、离线可跑**——不需要任何单独数据包，也不需要 `[ocr]` extra。PP-OCRv5 相比 v4 还支持繁体中文 / 日文。`[ocr]`/`[all]` extra 现为**向后兼容空壳**（仍可解析，但不再拉任何依赖）；旧的 `pdfspine-ocr-models` 数据包已弃用。
- 运行时模型解析顺序（见 `document.py::_ensure_ocr_models_env`）：
  1. 环境变量 `PDFSPINE_OCR_MODELS`（用户显式覆盖）；
  2. wheel 内嵌的 `pdfspine/_models`（默认，开箱即用）；
  3. 已安装的旧 `pdfspine_ocr_models` 伴随包（向后兼容）；
  4. 源码树内 `ocrspine/models`（开发回退）；
  5. 都没有 → 抛 `PdfUnsupportedError`。
- 入口：`page.get_textpage_ocr(...)`（返回 OCR `TextPage`）、`doc.pdfocr_save(...)` / `doc.pdfocr_tobytes(...)`（生成“可搜索三明治” PDF）。`engine="paddle"` 走纯 Rust PaddleOCR（CJK 更强）；默认 `engine="tesseract"`（需系统 tesseract 二进制）。GPU/CPU 由 tract 运行时自适应，无需区分。

## 与 PyMuPDF 的关系与差异

- **API 形状一致**：同名类、同名方法、同名常量（`TEXT_*`/`PDF_ANNOT_*`/`PDF_ENCRYPT_*`…）、PyMuPDF 风格的 camelCase 别名（`getToC`/`insertPDF`/`getPixmap`…）大多保留。
- **异常体系是 pdfspine 自有**：根类 `PdfError`，子类 `PdfSyntaxError`/`PdfPasswordError`/`PdfUnsupportedError`/`PdfDecodeError`/`PdfLimitError`/`PdfRedactionError`。PyMuPDF 名通过 shim 别名映射。
- **未实现的会显式报错**：deferred / out-of-scope 符号调用时抛 `PdfUnsupportedError`（不是静默错值）。最大的 out-of-scope 块是 `Story`/`Xml`/`Archive`（HTML/CSS→PDF 排版引擎，整块不做）。
- **版本号**：`pdfspine.__version__` 来自安装时 wheel 元数据（CI 按 git tag 定版）；本地/dev 构建显示 `0.0.0`。`pdfspine.version` 是 fitz 形状的三元组 `(VersionBind, VersionFitz, None)`。

## 能力范围速查（参考 PARITY.md）

- **读**：open（文件/字节）、坏 PDF 修复、加密 PDF（RC4 / AES-128 / AES-256，R2–R6）。
- **文本**：`get_text`（`text/words/blocks/dict/rawdict/json/rawjson/html/xhtml/xml`）、`search_for`、`TextPage`、字体/图片清单。文本抽取已达 fitz parity，Arabic/RTL 更强。
- **表格**：`find_tables`（含合并单元格）→ `extract()` / `to_markdown()` / `to_html()`。
- **编辑/保存**：完整保存 + 字节级增量保存、垃圾回收、页插入/删除/复制/移动/select、`insert_pdf` 合并、metadata/XMP、TOC、链接、写入加密。
- **注释/表单/redaction**：常见注释类型（带 `/AP` 外观流）、AcroForm 读/填/flatten + `Widget`、**破坏性** redaction（真正删除内容）。
- **渲染**：`get_pixmap`（矢量+文本+图片+渐变，tiny-skia 光栅化）、`Pixmap`（buffer-protocol/numpy 零拷贝）、`DisplayList`、`get_svg_image`。near-parity，~1.74× 更快。
- **图片**：把 PNG/JPEG/TIFF/GIF/BMP/WEBP 当文档打开、`convert_to_pdf`、图片 XObject 解码、`extract_image`。
- **Markdown → PDF（pdfspine 原创扩展，非 PyMuPDF API）**：`pdfspine.markdown_to_pdf()` 把 CommonMark + GFM（表格/删除线/任务列表）渲染成新 PDF——纯 Rust 确定性排版引擎；图片仅本地路径/`data:` URI（不发网络）；**中文必须传 `cjk_font=`**（否则渲染成 `?`，见 gotchas）。
- **图层**：OCG 读写（`get_ocgs` / `add_ocg` / `set_layer`）。
- **OCR**：见上。
- **CLI**：`pdfspine info / text / render / merge / split / pages / images / toc`。

明确不做：数字签名**创建**；HTML/CSS→PDF（Story/Xml/Archive——`markdown_to_pdf` 是独立的原创 Markdown 排版引擎，不经 HTML/CSS，不改变这一边界）；部分渲染期旋钮。
