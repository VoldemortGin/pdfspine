# pdfspine — Gotchas（陷阱与边界）

> 都是真实代码/配置/运行验证得出的注意点。出问题先按这里排查。

## 1. 需要预编译 wheel（这是 Rust 扩展）

- pdfspine 的核心是 Rust 编译产物 `pdfspine/_core.abi3.so`。`import pdfspine` 实际加载这个二进制。
- **abi3 wheel，要求 CPython ≥ 3.11**（`requires-python = ">=3.11"`，maturin `features = ["pyo3/abi3-py311", ...]`）。3.11 下限是 `Pixmap` 零拷贝 buffer 协议（`bf_getbuffer`/`bf_releasebuffer` 稳定 ABI 槽，CPython 3.11 才有）所要求。
- 从源码构建需要 C/asm 编译器（`cc`/`clang`，Windows 上是 MSVC Build Tools，含 `ml64.exe`）——因为内置 PaddleOCR 依赖 `tract`，构建期会编译目标架构的汇编 kernel。**预编译 wheel 不需要这些。**
- 跨平台 wheel 见 GitHub Release。`maturin develop`/`maturin build --release` 自行构建。

## 2. dev 构建版本号是 `0.0.0`

- `pdfspine.__version__` 取安装时 wheel 元数据；本地/editable/dev 构建显示 `0.0.0`，**正式版本由 CI 按 git tag 注入**。看到 `0.0.0` 不是 bug。
- `pdfspine.version` 是 fitz 形状三元组 `(__version__, __version__, None)`，不是单字符串。

## 3. fitz / pymupdf shim 默认不接管全局名

- **默认 `import fitz` 不会解析到 pdfspine。** 必须显式 `pdfspine.install_fitz_shim()`，且要在首次 `import fitz` 之前调用。
  - 这是为了与真 PyMuPDF 共存（collision-safe）。
- 不想动全局名时，直接 `import pdfspine.fitz as fitz` / `from pdfspine import pymupdf`，这两个子模块永远可用。
- `install_fitz_shim()` 用 `dict.setdefault`：若 `sys.modules["fitz"]` 已被真 PyMuPDF 占用，**shim 不会覆盖它**，真 PyMuPDF 赢。
- shim 里未实现的 PyMuPDF 名访问时抛 `PdfUnsupportedError`（不是 `AttributeError`）。

## 4. OCR：引擎和模型都在 wheel 里

- 一个 `pip install pdfspine` 装的 wheel **既有 OCR 代码，也内嵌了模型** —— 开箱即全功能 OCR、离线可跑，**不需要任何单独数据包、不需要 `[ocr]` extra**。
- `engine="paddle"` 用 PP-OCRv5 ONNX 模型（det/rec + PP-LCNet_x1_0 textline-ori，~28MB，支持繁中/日文），已随 wheel 装到 `site-packages/pdfspine/_models/`。`[ocr]`/`[all]` extra 现为**向后兼容空壳**（`pip install pdfspine[ocr]` 仍可解析，但不再拉任何东西；旧的 `pdfspine-ocr-models` 数据包已弃用）。
- 默认 `engine="tesseract"` 还需要**系统安装的 tesseract 二进制**（不在 wheel 里）。
- 缺引擎/缺模型/未知 engine → 抛 `PdfUnsupportedError`（带清晰提示）。
- 模型解析顺序：`PDFSPINE_OCR_MODELS` 环境变量（显式覆盖）→ wheel 内嵌的 `pdfspine/_models` → 旧 `pdfspine_ocr_models` 伴随包（兼容）→ 源码树 `ocrspine/models`（开发回退）→ 否则报错。可设 `os.environ["PDFSPINE_OCR_MODELS"]` 显式指定（须在调用 OCR 前；内部会镜像到引擎实际读取的 `OCRSPINE_MODELS`）。
- 注意：对**已有文本层**的 born-digital PDF 调 OCR 通常没意义；OCR 针对扫描件/图片页。
- `pil_save`/`pil_tobytes` 需要 `Pillow`；numpy 互操作需要 `numpy`——这两者都不是默认依赖。

## 5. 未实现的能力会显式报错（不是静默错值）

- deferred（21 个，计划做）和 out-of-scope（66 个，v1 不做）符号调用时抛 `PdfUnsupportedError`。
- 最大的 out-of-scope 块：`Story` / `Xml` / `Archive`（HTML/CSS → PDF 排版引擎，整块不做）。还有部分渲染期 `Tools` 旋钮、Widget JavaScript 钩子、数字签名**创建**。
- 已知 deferred（节选）：`Document.add_layer`/`get_layers`/`get_oc`/`get_ocmd`/`set_ocmd`/`set_layer_ui_config`/`switch_layer`/`insert_file`/`FormFonts`；`Page.write_text`/`insert_font`/`remove_rotation`/`run`/`extend_textpage`/`refresh`；`DisplayList.run`/`get_textpage`；`Annot.get_textbox`；`Pixmap.warp`；`Tools.set_annot_stem`/`set_subset_fontnames`。
- 权威清单：包内随附的 PARITY 概览，或仓库 `COMPAT.toml`（per-symbol）。

## 6. 与真 PyMuPDF 的差异点

- **异常类型不同名**：pdfspine 用 `PdfError` 体系，不是 PyMuPDF 的 `FileDataError` 等。shim 提供别名（`FileDataError = PdfSyntaxError`、`EmptyFileError = PdfSyntaxError`、`mupdf_display_errors = PdfError`），但若你**直接** `import pdfspine`（不经 shim），就该 catch `pdfspine.PdfError` 系列。
- **覆盖率不是 100%**：约 88.7%。迁移前用 `COMPAT.toml` 核对你依赖的符号。
- **渲染/文本是 near-parity 而非逐字节相同**：渲染 SSIM ~0.945；文本在 born-digital 上 parity，Arabic/RTL 更好。像素级/字节级完全一致不要假设。
- **redaction 是破坏性的**：`apply_redactions()` 真正删除被覆盖内容，不可逆。
- **camelCase 别名存在但建议用 snake_case**：`getToC`/`insertPDF`/`getPixmap`/`newPage` 等保留以兼容旧代码，新代码用 `get_toc`/`insert_pdf`/`get_pixmap`/`new_page`。

## 7. open() 的行为细节

- `pdfspine.open()` 无参 = 新建**空** PDF（0 页），不是报错。要加页用 `doc.new_page(...)`。
- 字节流：`open(stream=data)`。若 `data` 不以 `%PDF` 开头，会**尝试按图片解码**；明确是 PDF 时确保字节正确，或显式不传 `filetype`。
- 图片输入依赖 `_core.image_to_pdf`/图片编解码——发布 wheel（含 `ocr`/图片特性）有；用 `--no-default-features` 自行精简编译的库可能没有。
- 路径用 `str` 或 `os.PathLike` 均可（内部 `os.fspath`），跨平台安全。

## 8. 坐标系与几何

- 坐标系同 PyMuPDF：原点左上、y 向下、单位为 point（1/72 inch）。
- `Rect` 等是值类型：`*` 是变换、`|` 是并、`&` 是交（与 PyMuPDF 一致），别误用成数值乘法。
- `page.get_pixmap` 的 `matrix` 与 `dpi` 二选一；`Matrix(2,2)` ≈ 144 DPI（基准 72 DPI）。

## 9. 类型信息齐全

- 包带 `py.typed` 和全套 `.pyi` stubs（`__init__.pyi`/`document.pyi`/`geometry.pyi`/`helpers.pyi`/`constants.pyi`/`_core.pyi`），mypy/IDE 可直接用。LLM 生成代码时优先参照这些 stub 的真实签名。

## 10. `markdown_to_pdf()`（原创扩展）的边界

- **中文/CJK 默认渲染成 `?`（最常踩！）**：默认 Base-14 字体（Helvetica/Courier）没有 CJK 字形，不传 `cjk_font=` 时中/日/韩字符逐个退化为 `?` ——**不报错，静默退化**。必须传一个 CJK 字体文件（TTF/OTF/TTC 路径或字节）：macOS 如 `/System/Library/Fonts/Hiragino Sans GB.ttc`，Windows 如 `C:\Windows\Fonts\msyh.ttc`，或任意 Noto Sans CJK。`cjk_font` 是**逐字符回退**：主字体能编码的字符用主字体，编码不了的落到 `cjk_font`。
- 同理，WinAnsi 0x80–0x9F 区的排版字符（智能引号 “ ” ‘ ’、em-dash —）默认编码不了，无 `cjk_font` 时也退化 `?`。
- `font=` 是**单一字面**：传用户 TTF 后 bold/italic 不再切换粗/斜变体（同一字体绘制，样式退化）；默认 Base-14 时 bold/italic 正常。
- 标题内、表格单元格内的图片会被**丢弃**（段落/列表里的图片正常）。
- 表格跨页按**行**分页：单行本身不拆分，续页**不重复表头**。
- HTML 块 / 内联 HTML 被**忽略**（既不渲染也不报错）。
- 链接渲染为蓝字，**没有可点击的 link annotation**（v1）。
- 图片只接受本地路径 + `data:` URI；远程 URL 被拒，**绝不发网络请求**。相对路径需要 `base_dir=`（传文件路径时默认取该文件的父目录）。
- `md_or_path` 歧义：字符串若恰好是**现存文件**且后缀 ∈ {"", ".md", ".markdown", ".txt"} 会被当文件读；其余（含不存在的路径）当 Markdown 文本。
