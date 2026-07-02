# pdfspine — Recipes（可运行最小示例）

> 每个示例都按真实 API 写，绝大多数不依赖任何外部 PDF 文件（用 `pdfspine.open()` 现造）。所有非 OCR 示例已在已装 wheel 上实跑通过。

## 0. 30 秒 hello-world：造个 PDF、提取文本

```python
import pdfspine

# 无参 open() = 新建空 PDF（0 页）
doc = pdfspine.open()
page = doc.new_page(width=300, height=200)          # 默认 A4，这里给小尺寸
page.insert_text((50, 100), "Hello pdfspine", fontsize=18)

print(len(doc), "page(s)")                           # 1 page(s)
print(repr(page.get_text()))                         # 'Hello pdfspine\n'
print(page.search_for("Hello"))                      # [Rect(50.0, 85.6, 91.0..., 103.6)]
```

## 1. 打开已有 PDF 并遍历页面

```python
import pdfspine

doc = pdfspine.open("input.pdf")          # 路径；或 pdfspine.open(stream=pdf_bytes)
print(doc.page_count, doc.metadata.get("format"))

for page in doc:                          # 等价 for i in range(len(doc)): page = doc[i]
    text = page.get_text()                # 默认 "text"
    print(f"page {page.number}: {len(text)} chars")

doc.close()                               # 或用 with pdfspine.open(...) as doc:
```

## 2. 文本抽取的各种形态

```python
import pdfspine

doc = pdfspine.open()
page = doc.new_page(width=300, height=200)
page.insert_text((50, 100), "Hello pdfspine", fontsize=18)

page.get_text("text")     # 纯文本（默认）
page.get_text("words")    # [(x0,y0,x1,y1,'Hello',block,line,word), ...]
page.get_text("blocks")   # 块级 [(x0,y0,x1,y1,text,block_no,block_type), ...]
page.get_text("dict")     # {"blocks":[...], "width":..., "height":...}
page.get_text("json")     # 同 dict 的 JSON 字符串
page.get_text("html")     # 'html' / 'xhtml' / 'xml' 也支持

# 复用 TextPage 避免重复解析
tp = page.get_textpage()
tp.extractText()
tp.extractWORDS()
tp.search("Hello")        # list[Rect]
```

实跑核对输出：`words[0] == (50.0, 85.6, 91.0..., 103.6, 'Hello', 0, 0, 0)`；`dict` 顶层键 `['blocks','height','width']`。

## 3. 渲染页面为图片（PNG）

```python
import pdfspine

doc = pdfspine.open()
page = doc.new_page(width=300, height=200)
page.insert_text((50, 100), "Hello pdfspine", fontsize=18)

pix = page.get_pixmap(dpi=150)            # 或 matrix=pdfspine.Matrix(2, 2)（2x = 144 DPI）
print(pix.width, pix.height, pix.n, pix.colorspace)   # 625 417 3 DeviceRGB

pix.save("page1.png")                     # 按扩展名写盘
png_bytes = pix.tobytes("png")            # 也可拿字节（以 b'\x89PNG' 开头）

# 局部渲染 + 透明通道
clip = pdfspine.Rect(0, 0, 150, 100)
pix2 = page.get_pixmap(dpi=150, clip=clip, alpha=True)
```

numpy 零拷贝（需 `numpy`）：
```python
import numpy as np
pix = page.get_pixmap(dpi=150)
arr = np.frombuffer(pix.samples, dtype=np.uint8).reshape(pix.height, pix.width, pix.n)
# 或 arr = np.asarray(pix)（走 __array_interface__）
```

## 4. 用 fitz shim 直接替换 PyMuPDF

不污染全局名（推荐）：
```python
import pdfspine.fitz as fitz             # 等价 from pdfspine import pymupdf as fitz
doc = fitz.open("input.pdf")
text = doc[0].get_text("dict")
rect = fitz.Rect(0, 0, 100, 100)         # fitz.Rect 就是 pdfspine.Rect
```

让裸 `import fitz` 解析到 shim（一次性 opt-in，须在首次 import fitz 之前）：
```python
import pdfspine
pdfspine.install_fitz_shim()             # 幂等；不覆盖已先导入的真 PyMuPDF
import fitz                              # 现在 -> pdfspine 的 shim
print(fitz.open is pdfspine.open)        # True
print(fitz.FileDataError.__name__)       # 'PdfSyntaxError'（异常名别名）
```

## 5. 编辑、合并、保存

```python
import pdfspine

a = pdfspine.open("a.pdf")
b = pdfspine.open("b.pdf")

a.insert_pdf(b)                          # 把 b 全部追加到 a 后面
a.select([0, 2, 1])                      # 仅保留并重排为 第1、3、2 页
a.delete_page(0)                         # 删第 1 页

a.save("merged.pdf", garbage=4, deflate=True)   # 全量保存 + GC + 压缩
data = a.tobytes(garbage=3, deflate=True)        # 拿字节
a.save("inc.pdf", incremental=True)              # 增量保存（仅追加；需写回原文件）
```

加密保存：
```python
doc.save("secure.pdf",
         encryption=pdfspine.PDF_ENCRYPT_AES_256,
         owner_pw="owner", user_pw="user",
         permissions=pdfspine.PDF_PERM_PRINT | pdfspine.PDF_PERM_COPY)
```

打开加密 PDF：
```python
doc = pdfspine.open("secure.pdf")
if doc.needs_pass:
    assert doc.authenticate("user")      # True 表示通过
```

## 6. 表格抽取

```python
import pdfspine

doc = pdfspine.open("form.pdf")
page = doc[0]
tf = page.find_tables()                  # TableFinder
print(len(tf.tables), "table(s)")
for t in tf.tables:
    print(t.row_count, "x", t.col_count)
    print(t.extract())                   # list[list[str]]
    print(t.to_markdown())               # Markdown 表
    print(t.to_html())                   # HTML（保留合并单元格）
```
（在 `fixtures/corpus/irs-fw9.pdf` 第 1 页实跑：找到 4 个表，首表 4x2。）

## 7. 注释 + 破坏性 redaction

```python
import pdfspine

doc = pdfspine.open()
page = doc.new_page()
page.insert_text((72, 72), "Confidential SSN: 123-45-6789", fontsize=12)

# 高亮一段
for q in page.search_for("Confidential", quads=True):
    page.add_highlight_annot(q)

# 真正抹掉 SSN（破坏性）
for r in page.search_for("123-45-6789"):
    page.add_redact_annot(r, fill=(0, 0, 0))
applied = page.apply_redactions()        # 返回处理的 redaction 数，内容被真正删除
doc.save("redacted.pdf", garbage=4)
```

## 8. 表单（AcroForm）填写与扁平化

```python
import pdfspine

doc = pdfspine.open("form.pdf")
if doc.is_form_pdf:
    print(doc.form_field_names())
    doc.form_fill("name", "Alice")       # 按字段名填值
    doc.form_flatten()                   # 把表单值烘焙成静态内容
doc.save("filled.pdf")

# 也可遍历控件
for w in doc[0].widgets():
    print(w.field_name, w.field_type_string, w.field_value)
```

## 9. 矢量绘图（Shape）

```python
import pdfspine

doc = pdfspine.open()
page = doc.new_page(width=200, height=200)
shape = page.new_shape()
shape.draw_rect(pdfspine.Rect(20, 20, 180, 180))
shape.draw_circle(pdfspine.Point(100, 100), 60)
shape.finish(color=(0, 0, 1), fill=(0.9, 0.9, 1), width=2)
shape.commit()
doc.save("shapes.pdf")
```

## 10. TOC / 书签

```python
import pdfspine

doc = pdfspine.open("book.pdf")
print(doc.get_toc())                     # [[level, title, page], ...]

# 写入新 TOC
doc.set_toc([[1, "Chapter 1", 1], [2, "Section 1.1", 2], [1, "Chapter 2", 5]])
doc.save("book-toc.pdf")
```

## 11. OCR（`pip install pdfspine` 即全功能，无需额外步骤）

OCR 引擎（纯 Rust PaddleOCR PP-OCRv5）**和 ~28MB 模型都已内嵌进 wheel**（装后位于 `site-packages/pdfspine/_models/`），一个裸 `pip install pdfspine` 即全功能 OCR、离线可跑，**无需单独数据包、无需 `[ocr]` extra**。`engine="paddle"` 走 PP-OCRv5（支持繁中/日文）；默认 `engine="tesseract"` 还需系统 tesseract 二进制。

```python
import pdfspine

# 把扫描件（图片/无文本层 PDF）OCR 成可搜索文本
doc = pdfspine.open("scan.pdf")

# 页级：返回 OCR 后的 TextPage
tp = doc[0].get_textpage_ocr(dpi=150, engine="paddle", language="ch")
print(tp.extractText())

# 文档级：生成“可搜索三明治” PDF（原图 + 隐藏文本层）
doc.pdfocr_save("searchable.pdf", dpi=150, engine="paddle")
sandwich_bytes = doc.pdfocr_tobytes(dpi=150, engine="paddle")
```

显式指定模型目录（覆盖 wheel 内嵌的默认模型）：
```python
import os
os.environ["PDFSPINE_OCR_MODELS"] = "/path/to/models"   # 须在调用 OCR 前设置
```

## 12. 把图片当文档打开（需含图片特性的发布 wheel）

```python
import pdfspine

# 路径后缀识别（.png/.jpg/.jpeg/.bmp/.gif/.webp/.tif/.tiff）
doc = pdfspine.open("photo.png")         # 透明转成单页 PDF
print(doc.page_count, doc.is_pdf)        # 1 True

# 或字节 + filetype
with open("photo.jpg", "rb") as f:
    doc = pdfspine.open(stream=f.read(), filetype="jpg")

# 转成 PDF 字节
pdf_bytes = pdfspine.open("photo.png").convert_to_pdf()
```

## 13. Markdown → PDF（pdfspine 原创扩展）

`pdfspine.markdown_to_pdf()` 把 CommonMark + GFM（表格/删除线/任务列表）渲染成新 `Document`（确定性输出；图片仅本地路径/`data:` URI，不发网络）：

```python
import pdfspine

md = """# 报告标题

支持 **粗体** / *斜体* / `行内代码` / [链接](https://example.com)（蓝字）。

- 列表、嵌套列表
- [x] 任务列表（已完成项）

| 列 A | 列 B |
|---|---|
| 1 | 2 |
"""

doc = pdfspine.markdown_to_pdf(md)             # 返回 Document
doc.save("out.pdf")

# 也可直接传 .md/.markdown/.txt/无后缀 的现存文件路径
# （相对图片路径默认以该文件所在目录为基准；可用 base_dir= 覆盖）
doc = pdfspine.markdown_to_pdf("README.md")
```

**中文（CJK）必须传 `cjk_font=`** ——默认 Base-14 字体没有 CJK 字形，不传时中文渲染成 `?`（不报错）：

```python
doc = pdfspine.markdown_to_pdf(
    "# 中文标题\n\n正文**加粗**，表格、列表都支持。",
    cjk_font="/System/Library/Fonts/Hiragino Sans GB.ttc",  # macOS 自带；TTF/OTF/TTC 均可
    # Windows 可用 r"C:\Windows\Fonts\msyh.ttc"；或任意 Noto Sans CJK 文件
)
doc.save("cjk.pdf")
```
（实跑核对：不传 `cjk_font` 时 `get_text()` 返回 `'????\n?? abc\n'`；传 Hiragino/Songti/Arial Unicode 后返回 `'中文标题\n正文加粗。\n'`。）

页面选项：`page_width`/`page_height`（默认 A4 595.32×841.92 pt）、`margins`（单值或 `(top,right,bottom,left)`，默认 72）、`body_font_size`（默认 11，标题按比例缩放）、`font=`（用户 TTF/OTF 替换正文+标题字体）。

## 14. 命令行（CLI）

安装后提供 `pdfspine` 命令（也可 `python -m pdfspine`）。页码为 **1-based**，范围语法 `1-3,5,8-`。

```bash
pdfspine info report.pdf
pdfspine text report.pdf --pages 1-3 --format json -o out.json
pdfspine render report.pdf --dpi 200 -o images/        # 或 --zoom 2.0
pdfspine merge a.pdf b.pdf -o merged.pdf
pdfspine split report.pdf --ranges 1-3,4-6 -o parts/   # 默认每页一个文件
pdfspine pages report.pdf --select 1,3,5 -o subset.pdf # 可重排/重复
pdfspine images report.pdf -o imgs/
pdfspine toc report.pdf
```
（`pdfspine info fixtures/born/pangrams.pdf` 实跑：输出 format/is pdf/encrypted/page count 等。）
