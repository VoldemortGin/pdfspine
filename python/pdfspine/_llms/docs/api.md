# pdfspine — Public API（真实签名 + 契约）

> 签名核对自 `python/pdfspine/*.pyi`（PEP 561 stubs）+ `inspect.signature` 运行核对。带 `*` 的关键字参数表示**仅限关键字**。PyMuPDF 风格的 camelCase 别名（如 `getToC`/`insertPDF`/`getPixmap`/`newPage`）大多保留，下文标注“fitz 兼容别名”。

## 顶层入口 `pdfspine`

```python
import pdfspine

pdfspine.__version__: str                       # 安装时 wheel 元数据；dev 构建为 "0.0.0"
pdfspine.version: tuple[str, str, None]         # fitz 形状 (VersionBind, VersionFitz, None)
pdfspine.identity_matrix() -> tuple[float,...]  # (1,0,0,1,0,0)
pdfspine.install_fitz_shim() -> None            # 让裸 import fitz/pymupdf 解析到 shim（幂等，不覆盖已导入的真 PyMuPDF）
```

### `open()` — 打开文档

```python
pdfspine.open(
    filename: str | os.PathLike[str] | None = None,
    *,
    stream: bytes | None = None,
    filetype: str | None = None,
) -> Document
```

契约（核对自 `document.py::open`）：
- 位置参数传路径；`stream=` 传内存字节。
- **无参** `open()` → 新建一个空 PDF（0 页），对应 `fitz.open()`。
- 光栅图片输入（`.png .jpg .jpeg .jpe .jfif .bmp .gif .webp .tif .tiff`）会被透明转成单/多页 PDF。判定依据：`filetype=`、路径后缀、或内容嗅探（字节不以 `%PDF` 开头时尝试按图片解码）。
- 重解析在 Rust 核心里释放 GIL。
- 失败：路径不存在抛 `FileNotFoundError`；坏 PDF 抛 `PdfSyntaxError`；加密相关抛 `PdfPasswordError`；不支持的输入抛 `PdfUnsupportedError`。

> 注意：图片→PDF 依赖 `_core.image_to_pdf`，需要带 OCR/图片特性编译的 wheel（发布 wheel 已含）。

## `Document`

构造一般经由 `pdfspine.open()`。支持 `len(doc)`、`doc[i]`、`for page in doc`、`with pdfspine.open(...) as doc:`。

### 生命周期 / 基本信息
```python
doc.page_count: int            # == len(doc)
doc.load_page(index=0) -> Page
doc[index] -> Page             # 等价 load_page，支持负索引
doc.pages(*args, **kwargs) -> Iterator[Page]
for page in doc                # __iter__
doc.reload_page(page) -> Page
doc.close() -> None
doc.is_pdf: bool
doc.is_repaired: bool          # 打开时是否触发了修复
doc.is_closed: bool
doc.is_dirty: bool
doc.is_reflowable: bool
doc.is_fast_webaccess: bool
doc.name: str | None           # 来源路径；stream/新建文档为 None
doc.metadata: dict[str, str]   # title/author/subject/keywords/creator/producer/format/...
doc.version_count: int
doc.chapter_count: int
doc.last_location: tuple[int, int]
```

### 加密
```python
doc.is_encrypted: bool
doc.needs_pass: bool
doc.permissions: int                       # PDF_PERM_* 位掩码
doc.authenticate(password: str | bytes) -> bool
```

### 文本 / 抽取（文档级便捷）
```python
doc.get_page_text(pno, option="text", *, flags=None, sort=False) -> str | list | dict
doc.get_page_pixmap(pno, **kw) -> Pixmap
doc.get_page_images(pno, full=False) -> list[tuple]
doc.get_page_fonts(pno, full=False) -> list[tuple]
doc.search_page_for(pno, text, **kw) -> list
doc.extract_image(xref) -> dict[str, Any]   # {"image": bytes, "ext": str, "width", "height", ...}; 别名 extractImage
```

### 保存 / 序列化
```python
doc.save(filename, *, garbage=0, deflate=False, incremental=False,
         encryption=None, owner_pw=None, user_pw=None, permissions=..., **_) -> None
doc.tobytes(*, garbage=0, deflate=False, incremental=False, encryption=None,
            owner_pw=None, user_pw=None, permissions=..., **_) -> bytes
doc.write(...) -> bytes                      # 同 tobytes 参数
doc.ez_save(filename, **kwargs) -> None      # 便捷默认（garbage+deflate）
doc.saveIncr(filename=None) -> None          # 增量保存到原文件（无 save_incremental 同名方法；增量也可用 save(..., incremental=True)）
doc.can_save_incrementally() -> bool
doc.convert_to_pdf(from_page=0, to_page=-1, rotate=0) -> bytes
```
- `garbage`：0–4，垃圾回收等级；`deflate=True` 压缩流；`incremental=True` 仅追加（字节级增量）。
- `encryption` 取 `PDF_ENCRYPT_*` 常量，配合 `owner_pw`/`user_pw`/`permissions`。

### 页操作
```python
doc.new_page(pno=-1, width=595, height=842) -> Page      # 别名 newPage；默认 A4 点尺寸
doc.insert_page(pno, text=None, fontsize=11, width=595, height=842,
                fontname="helv", fontfile=None, color=None, **kw) -> int
doc.insert_pdf(docsrc, from_page=None, to_page=None, start_at=None, **_) -> None   # 别名 insertPDF
doc.delete_page(pno=-1) -> None
doc.delete_pages(*args, **kw) -> None
doc.select(pages: Sequence[int]) -> None     # 仅保留并按给定顺序重排
doc.copy_page(pno, to=-1) -> None
doc.move_page(pno, to=-1) -> None
doc.fullcopy_page(pno, to=-1) -> None
doc.page_xref(pno) -> int
doc.page_cropbox(pno) -> Rect
doc.page_mediabox(pno) -> Rect
```

### TOC / 大纲 / 链接解析
```python
doc.get_toc(simple=True) -> list[list]       # 别名 getToC；[[level, title, page], ...]
doc.set_toc(toc) -> None                     # 别名 setToC
doc.outline -> Outline | None
doc.get_outline_xrefs() -> list[int]
doc.resolve_link(uri="", *, chapters=0) -> int | None
doc.resolve_names() -> dict[str, dict]
```

### Metadata / XMP / 页面状态
```python
doc.set_metadata(metadata: dict) -> None     # 别名 setMetadata
doc.get_xml_metadata() -> str
doc.set_xml_metadata(xml) -> None
doc.del_xml_metadata() -> None
doc.pagelayout / set_pagelayout(s)
doc.pagemode / set_pagemode(s)
doc.language / set_language(language=None)
doc.markinfo / set_markinfo(dict) -> bool
doc.need_appearances(value=None) -> bool | None
doc.get_sigflags() -> int
```

### 页标签 (page labels)
```python
doc.get_page_label(pno) -> str
doc.get_label(pno) -> str
doc.get_page_labels() -> list[dict]
doc.get_page_numbers(label, only_one=False) -> list[int]
doc.set_page_labels(labels: Sequence[dict]) -> None
```

### 表单 (AcroForm)
```python
doc.is_form_pdf: bool                         # 别名 isFormPDF
doc.form_field_names() -> list[str]
doc.form_fill(name, value) -> None
doc.form_flatten() -> None
```

### 嵌入文件
```python
doc.embfile_add(name, buffer, filename=None, ufilename=None, desc=None, **_) -> None
doc.embfile_get(name) -> bytes
doc.embfile_del(name) -> None
doc.embfile_names() -> list[str]
doc.embfile_count() -> int
doc.embfile_info(name) -> dict
doc.embfile_upd(item, buffer=None, filename=None, ufilename=None, desc=None, **_) -> None
# camelCase 别名: embfileAdd/embfileGet/embfileDel/embfileNames/embfileCount/embfileInfo
```

### 清理 / 脱敏 / 烘焙
```python
doc.scrub(*, attached_files=False, clean_pages=False, embedded_files=False,
          hidden_text=False, javascript=False, metadata=False, redactions=False,
          redact_images=0, remove_links=False, reset_fields=False,
          reset_responses=False, thumbnails=False, xml_metadata=False, **_) -> None
doc.bake(*, annots=True, widgets=True, **_) -> None   # 把注释/控件烘焙进页面内容
doc.subset_fonts(*args, **kwargs) -> int
```

### OCG / 图层（已实现部分）
```python
doc.get_ocgs() -> dict[int, dict]            # 别名 getOCGs
doc.layer_ui_configs() -> list[dict]
doc.ocg_state(xref) -> bool
doc.get_layer(config=0) -> dict[str, list[int]]
doc.set_layer(config=0, *, on=None, off=None, locked=None, **_) -> None
doc.add_ocg(name, config=None, *, on=True, intent="View", usage=None, **_) -> int
doc.set_oc(xref, ocg) -> None
```
> deferred（调用抛 `PdfUnsupportedError`）：`add_layer` / `get_layers` / `get_oc` / `get_ocmd` / `set_ocmd` / `set_layer_ui_config` / `switch_layer` / `insert_file` / `FormFonts`。

### Journalling（撤销/重做）
```python
doc.journal_enable() / journal_is_enabled() -> bool
doc.journal_save_state() / journal_can_undo() / journal_can_redo() -> bool
doc.journal_can_do() -> dict[str, bool]
doc.journal_undo() / journal_redo() -> bool
```

### 低层 xref / COS（高级）
```python
doc.xref_length() -> int
doc.xref_object(xref) -> str
doc.xref_get_key(xref, key) -> Any
doc.xref_get_keys(xref) -> tuple
doc.xref_set_key(xref, key, value: str) -> None
doc.xref_is_stream(xref) / is_stream(xref) -> bool
doc.xref_stream(xref) -> bytes
doc.xref_stream_raw(xref) -> bytes
doc.xref_is_font(xref) / xref_is_image(xref) / xref_is_xobject(xref) -> bool
doc.xref_copy(source, target, *, keep=...) -> None
doc.pdf_catalog() -> int
doc.pdf_trailer(compressed=False, ascii=False) -> str
doc.get_new_xref() -> int
doc.update_object(xref, text, page=...) -> None
doc.update_stream(xref, stream=..., new=False, compress=False) -> None
doc.get_char_widths(xref, ...) -> list[tuple[int, float]]
doc.extract_font(...) ; doc.get_page_xobjects(pno) -> list[tuple]
```

## `Page`

### 几何 / 盒子
```python
page.number: int
page.rect -> Rect          ;  page.bound() -> Rect
page.mediabox / cropbox / artbox / bleedbox / trimbox -> Rect
page.mediabox_size -> Point ; page.cropbox_position -> Point
page.rotation: int
page.transformation_matrix / rotation_matrix / derotation_matrix -> Matrix
page.xref: int ;  page.parent -> Document | None
page.is_image_only: bool
page.set_rotation(rotation) -> None              # 别名 setRotation
page.set_mediabox/set_cropbox/set_artbox/set_bleedbox/set_trimbox(rect) -> None
```

### 文本抽取
```python
page.get_text(option="text", *, clip=None, flags=None, textpage=None, sort=False) -> str | list | dict
#   option ∈ {"text","words","blocks","dict","rawdict","json","rawjson","html","xhtml","xml"}
page.get_text_words(*, clip=None, flags=None, sort=False) -> list[tuple]
page.get_text_blocks(*, clip=None, flags=None, sort=False) -> list[tuple]
page.get_textbox(rect, *, textpage=None) -> str
page.get_text_selection(p1, p2, clip=None) -> str
page.get_textpage(flags=None, clip=None) -> TextPage
page.search_for(needle, *, hit_max=0, quads=False, clip=None, flags=None, textpage=None)
#   -> list[Rect]（默认）或 list[Quad]（quads=True）
page.get_texttrace() -> list[dict]
```
- `words` 每条 = `(x0, y0, x1, y1, word, block_no, line_no, word_no)`（已运行核对）。
- `dict` 顶层键 = `{"blocks", "width", "height"}`（已运行核对）。
- `flags` 取 `TEXT_*` / `TEXTFLAGS_*` 常量位掩码。

### 清单 / 资源
```python
page.get_fonts(full=False) -> list[tuple]        # 别名 getImages 等见下
page.get_images(full=False) -> list[tuple]       # 别名 getImages
page.get_xobjects() -> list[tuple]
page.get_image_rects(...) -> list[Rect]
page.get_image_info(...) -> list[dict]
page.get_image_bbox(name_or_xref, ...) -> Rect
page.get_drawings(**_) -> list[dict]             # 别名 getDrawings
page.get_cdrawings(**_) -> list[dict]            # 别名 getCdrawings
```

### 内容流
```python
page.get_contents() -> list[int]                 # 内容流对象号
page.read_contents() -> bytes                    # 解码+拼接后的内容流字节
page.set_contents(xref) -> None
page.clean_contents(...) -> None
page.wrap_contents() -> None
page.delete_image(name_or_xref, ...) -> None
page.replace_image(name_or_xref, *, filename=None, stream=None, pixmap=None, **kw) -> None
#   需 stream=（JPEG 字节）或 filename=
```

### 渲染
```python
page.get_pixmap(*, matrix=None, dpi=None, colorspace=None, alpha=False, clip=None) -> Pixmap
#   别名 getPixmap；matrix 与 dpi 二选一（dpi 优先级见 fitz 语义）
page.get_displaylist() -> DisplayList            # 别名 getDisplayList
page.get_svg_image(matrix=None, *, text_as_path=False, **_) -> str   # 别名 getSVGimage
```

### 表格
```python
page.find_tables(*, strategy="lines", line_max_thickness=..., snap_tolerance=...,
                 min_line_length=..., clip=None, **_) -> TableFinder   # 别名 findTables
```

### 链接
```python
page.first_link -> Link | None
page.links(kinds=None) -> Iterator[Link]
page.get_links() -> list[dict]                   # 别名 getLinks
page.insert_link(link: dict) -> None
page.update_link(link: dict) -> None
page.delete_link(link: dict) -> None
```

### 文本/图片/绘图写入
```python
page.insert_text(point, text, *, fontname="helv", fontsize=11, color=None, fontfile=None, **_) -> int
page.insert_textbox(rect, text, *, fontname="helv", fontsize=11, color=None, align=0, fontfile=None, **_) -> float
page.insert_image(rect, *, stream=None, filename=None, pixmap=None, width=0, height=0, **_) -> None
#   提供 stream=（图片字节，JPEG 自动识别）或 filename= 或 pixmap=
page.draw_line(p1, p2, *, color=None, width=1, **_)        # 及 draw_rect/draw_circle/draw_oval/draw_bezier/draw_polyline
page.new_shape() -> Shape                                  # 别名 newShape
page.show_pdf_page(rect, src: Document, pno=0, ...) -> str
```
- camelCase 别名：`insertText`/`insertTextbox`/`insertImage`/`drawLine`/`drawRect`/`drawCircle`/`drawOval`/`drawBezier`/`drawPolyline`。

### 注释 (Annot)
```python
page.add_text_annot(point, text, *, icon="Note", **_) -> Annot
page.add_freetext_annot(rect, text, *, fontsize=11, text_color=None, fill_color=None, align=0, **_) -> Annot
page.add_highlight_annot(quads=None, *, start=None, stop=None, clip=None, **_) -> Annot
page.add_underline_annot(...) / add_strikeout_annot(...) / add_squiggly_annot(...)   # 同 highlight 签名
page.add_rect_annot(rect, *, color=None, fill=None, **_) -> Annot
page.add_circle_annot(rect, *, color=None, fill=None, **_) -> Annot
page.add_line_annot(p1, p2, *, color=None, **_) -> Annot
page.add_polygon_annot(points, *, color=None, fill=None, **_) -> Annot
page.add_polyline_annot(points, *, color=None, **_) -> Annot
page.add_ink_annot(handwriting, *, color=None, **_) -> Annot
page.add_stamp_annot(rect, *, stamp="Approved", **_) -> Annot # stamp 取图章名字符串，如 "Approved"
page.add_file_annot(point, buffer, filename, *, ufilename=None, desc=None, icon=None, **_) -> Annot
page.add_redact_annot(quad, *, text=None, fill=None, **_) -> Annot
page.annots(types=None) -> Iterator[Annot]
page.annot_xrefs() -> list[int] ; page.annot_names() -> list[str]
page.first_annot -> Annot | None    # 别名 firstAnnot
page.delete_annot(annot: Annot | int) -> None    # 别名 deleteAnnot
page.apply_redactions(*args, **kwargs) -> int     # 别名 applyRedactions；破坏性删除
```
- 所有 `add_*` 都有 camelCase 别名（`addTextAnnot`/`addHighlightAnnot`/…）。

### 表单控件 (Widget)
```python
page.widgets() -> list[Widget]
page.first_widget -> Widget | None   # 别名 firstWidget
```

### OCR（页级）
```python
page.get_textpage_ocr(flags=3, language="eng", dpi=72, full=True,
                      tessdata=None, engine="tesseract") -> TextPage   # 别名 getTextPageOCR
```
- `engine="paddle"` → 纯 Rust PaddleOCR（需模型，见 OCR 一节）；`engine="tesseract"`（默认）→ 需系统 tesseract。
- 引擎不可用/未知 → `PdfUnsupportedError`。

## `TextPage`

由 `page.get_textpage()` / `page.get_textpage_ocr()` 得到。一次解析、多次抽取。
```python
tp.extractText() -> str
tp.extractWORDS() -> list[tuple] ; tp.extractBLOCKS() -> list[tuple]
tp.extractDICT() / extractRAWDICT() -> dict
tp.extractJSON() / extractRAWJSON() -> str
tp.extractHTML() / extractXHTML() / extractXML() -> str
tp.extractTextbox(rect) -> str
tp.extractSelection(a, b) -> str
tp.search(needle, quads=False) -> list[Rect] | list[Quad]
tp.extractIMGINFO() -> list[dict]
tp.poolsize() -> int
tp.rect -> Rect
```

## `Pixmap`（渲染位图，原生类型）

```python
pdfspine.Pixmap(colorspace: int | str, irect, alpha=False)   # 构造空 pixmap
# 通常经 page.get_pixmap(...) / displaylist.get_pixmap(...) 获得
```
属性：`width`/`w`、`height`/`h`、`n`（通道数）、`alpha`、`stride`、`irect`、`colorspace`（如 `"DeviceRGB"`）、`samples`(bytes)、`samples_mv`(memoryview)、`samples_ptr`(int)、`size`、`x`/`y`、`xres`/`yres`、`is_monochrome`、`is_unicolor`、`__array_interface__`（numpy 零拷贝）。

方法：
```python
pix.save(filename, output=None) -> None          # 按扩展名或 output 写盘
pix.tobytes(output="png") -> bytes                # "png"/"jpg"/... 编码字节
pix.pil_save(filename, format=None, **kwargs) -> None   # 需 Pillow
pix.pil_tobytes(format="png", **kwargs) -> bytes        # 需 Pillow
pix.pixel(x, y) -> tuple[int, ...] ; pix.set_pixel(x, y, value)
pix.set_rect(irect, color) -> bool ; pix.set_alpha(value) ; pix.clear_with(value=0)
pix.invert_irect(irect=None) ; pix.copy() -> Pixmap ; pix.shrink(factor)
pix.set_origin(x, y) ; pix.set_dpi(xres, yres)
pix.tint_with(black=0, white=...) ; pix.gamma_with(gamma)
pix.color_count() -> int ; pix.color_topusage() -> tuple[float, bytes]
pix.digest() -> bytes
# numpy: numpy.frombuffer(pix.samples, ...) 或 numpy.array(pix)（经 __array_interface__）
```
- pdfocr 导出：`pix.pdfocr_save(...)` / `pix.pdfocr_tobytes(...)`（PARITY 列为已实现）。
- deferred：`warp`。

## `DisplayList`
```python
dl = page.get_displaylist()
dl.rect -> Rect ; len(dl) -> int
dl.get_pixmap(*, matrix=None, dpi=None, colorspace=None, alpha=False, clip=None) -> Pixmap
```
> deferred：`run` / `get_textpage`（device-callback 回放）。

## `TableFinder` / `Table`
```python
tf = page.find_tables()
tf.tables -> list[Table] ; len(tf) ; tf[i] ; for t in tf
t.bbox -> Rect ; t.row_count : int ; t.col_count : int
t.header -> list ; t.rows -> list[float] ; t.cols -> list[float]
t.cells -> list[list[Rect | None]]
t.spans -> list[tuple[int,int,int,int,Rect]]    # 合并单元格
t.extract() -> list[list]                        # 行×列文本
t.to_markdown() -> str                           # 别名 toMarkdown
t.to_html() -> str                               # 保留合并单元格
```

## `Annot`
关键属性/方法（核对自 `document.pyi`）：
```python
a.rect -> Rect ; a.type -> tuple[int, str] ; a.xref : int
a.info -> dict[str,str] ; a.colors -> dict ; a.opacity : float ; a.flags : int
a.border -> dict ; a.line_ends -> tuple[int,int] ; a.blendmode -> str|None
a.vertices -> list[Point] ; a.has_appearance : bool ; a.has_ap() -> bool
a.set_rect(rect) ; a.set_colors(colors=None, *, stroke=..., fill=...) ; a.set_opacity(o)
a.set_border(border=None, *, width=None) ; a.set_line_ends(s,e) ; a.set_blendmode(m)
a.set_name(n) ; a.set_open(b) ; a.set_flags(f)
a.set_info(info=None, *, content=None, title=None, name=None)
a.update(**_) -> bool                            # 重生成外观流
a.set_rotation(r) ; a.popup_rect -> Rect ; a.set_popup(rect)
a.apn_bbox() -> Rect ; a.apn_matrix() -> Matrix
a.file_info() -> dict ; a.get_file() -> bytes ; a.update_file(...)
a.next -> Annot | None
a.get_textpage(clip=None, flags=...) -> TextPage ; a.get_text(option="text", ...) 
# camelCase 别名: setColors/setRect/setOpacity/setBorder/setInfo/setFlags
```
> deferred：`get_textbox`。

## `Widget`
```python
w.rect -> Rect ; w.xref : int
w.field_type : int ; w.field_type_string : str
w.field_name : str|None ; w.field_label : str|None ; w.field_value (可读写)
w.field_flags : int ; w.choice_values -> list[str] ; w.button_states -> list[str]
w.border_color/fill_color -> list[float]|None ; w.border_style/border_width/border_dashes
w.text_color/text_font/text_fontsize/text_maxlen/text_format
w.button_caption ; w.field_display : int ; w.is_signed : bool|None ; w.rb_parent : int|None
w.on_state() -> str|None ; w.reset() ; w.update(value=...) -> None
w.next -> Widget | None
```

## `Shape`（矢量绘图累加器）
流程：`shape = page.new_shape()` → `draw_*` → `finish(...)` → `commit()`。
```python
s.draw_line(p1,p2)->Point ; s.draw_rect(rect)->Rect ; s.draw_circle(c,r)->Point
s.draw_oval(rect)->Rect ; s.draw_bezier(p1,p2,p3,p4)->Point ; s.draw_polyline(points)->Point
s.draw_curve(points)->Point ; s.draw_quad(quad)->Point ; s.draw_curve3(p1,p2,p3)->Point
s.draw_sector(center,point,angle,fullSector=False)->Point
s.draw_squiggle(p1,p2,breadth=...)->Point ; s.draw_zigzag(p1,p2,breadth=...)->Point
s.finish(color=None, fill=None, width=1, dashes=None, even_odd=False, closePath=True, **_)
s.commit(overlay=True) -> None
s.insert_text(point, text, *, fontname="helv", fontsize=11, color=None, fontfile=None, **_) -> int
s.insert_textbox(rect, buffer, *, fontname="helv", fontsize=11, color=None, align=0, fontfile=None, **_) -> float
s.rect / s.width / s.height / s.x / s.y / s.doc / s.page
```

## `TextWriter`
```python
tw = pdfspine.TextWriter(page_rect, opacity=1, color=None)
tw.append(pos, text, font=None, fontsize=11, *, language=None, **_) -> tuple[TextWriter, Point]
tw.appendv(pos, text, font=None, fontsize=11, **_) -> tuple[TextWriter, Point]
tw.fill_textbox(rect, text, *, font=None, fontsize=11, align=0, **_) -> list[str]
tw.write_text(page, *, opacity=None, color=None, overlay=True, **_) -> None   # 别名 writeText
tw.text_rect -> Rect ; tw.last_point -> Point ; tw.clean_rtl(text) -> str
```

## `Font` / `Colorspace`
```python
pdfspine.Font(fontname=None, fontfile=None, fontbuffer=None, ...)
#   name/ascender/descender/bbox/glyph_count/flags/is_bold/is_italic/is_serif/is_monospaced
#   is_writable/buffer
#   glyph_advance(chr) ; has_glyph(chr) ; text_length(text, fontsize=11) ; char_lengths(text, fontsize=11)
#   glyph_name_to_unicode(name) ; unicode_to_glyph_name(ch) ; valid_codepoints() ; glyph_bbox(chr)
pdfspine.Base14_fontnames : tuple[str, ...]
pdfspine.csGRAY / csRGB / csCMYK : Colorspace        # 预建实例
pdfspine.Colorspace(type_: int)                      # type_ ∈ {CS_GRAY, CS_RGB, CS_CMYK}
#   cs.n : int ; cs.name : str ; cs.is_gray : bool
```

## 几何值类型 `pdfspine.geometry`

```python
# 这五个值类型在顶层 pdfspine 也可见: pdfspine.Rect/IRect/Point/Matrix/Quad
Point(*args) ; Rect(*args) ; IRect(*args) ; Matrix(*args) ; Quad(*args)
pdfspine.geometry.Identity         # IdentityMatrix 单例（在 geometry 子模块，不在顶层 pdfspine）
```
- `Rect`：`x0/y0/x1/y1`、`width`/`height`、`tl/tr/bl/br`(及 `top_left`…)、`quad`、`irect`、`round()`、`is_empty/is_valid/is_infinite`；方法 `normalize()`/`include_point(p)`/`include_rect(r)`/`intersect(r)`/`transform(m)`/`morph(p,m)`/`torect(r)`/`contains(x)`/`intersects(x)`/`get_area(unit="px")`；运算符 `r * m`（变换）、`r | x`（并）、`r & r2`（交）、`r + p`/`r - p`、`in`。
- `IRect`：整数版，另有 `.rect`。
- `Point`：`x/y`、`norm()`/`distance_to(...)`/`transform(m)`/`unit`/`abs_unit`，运算符 `+ - * /`、`abs()`。
- `Matrix`：`a..f`、`concat`/`invert`/`prerotate`/`prescale`/`preshear`/`pretranslate`/`is_rectilinear`、`~m`(逆)、`m * m2`。
- `Quad`：`ul/ur/ll/lr`、`rect`/`width`/`height`/`is_empty/is_infinite/is_convex/is_rectangular`、`transform(m)`/`morph(p,m)`。
- 工厂：`EMPTY_RECT()`/`EMPTY_IRECT()`/`EMPTY_QUAD()`/`INFINITE_RECT()`/`INFINITE_IRECT()`/`INFINITE_QUAD()`；纸张 `paper_sizes()`/`paper_size(s)`/`paper_rect(s)`。
- 类型别名：`point_like`/`rect_like`/`matrix_like`/`quad_like`。

## 模块函数 `pdfspine.helpers`（顶层可见）
```python
get_text_length(text, fontname="helv", fontsize=11, encoding=0) -> float
image_profile(stream, keep_image=0) -> dict | None     # 探测图片元数据
get_pdf_now() -> str ; get_pdf_str(s) -> str
glyph_name_to_unicode(name) -> int ; unicode_to_glyph_name(ch) -> str
sRGB_to_rgb(srgb) -> tuple[int,int,int] ; sRGB_to_pdf(srgb) -> tuple[float,float,float]
planish_line(p1, p2) -> Matrix
recover_quad / recover_bbox_quad / recover_char_quad / recover_line_quad / recover_span_quad
ConversionHeader(output="text", filename="UNKNOWN") -> str ; ConversionTrailer(output) -> str
Base14_fontdict : dict[str, str]
# 日志: message(text="") ; log(text="", caller=...) ; set_messages(...) ; set_log(...)
```

## 异常层级（`pdfspine`）
```
PdfError                # 根
├── PdfSyntaxError      # 坏 PDF / 语法错误（shim: FileDataError, EmptyFileError 映射到此）
├── PdfPasswordError    # 密码 / 认证
├── PdfUnsupportedError # 未实现 / 不支持的能力（deferred / out-of-scope / 引擎缺失）
├── PdfDecodeError      # 解码失败
├── PdfLimitError       # 资源/尺寸限制
└── PdfRedactionError   # redaction 失败
```

## 常量 `pdfspine.constants`（顶层 re-export）
分组（完整名见 `constants.pyi`）：
- 颜色空间：`CS_GRAY` / `CS_RGB` / `CS_CMYK`
- 注释类型：`PDF_ANNOT_*`（`TEXT`/`HIGHLIGHT`/`SQUARE`/`REDACT`/…）+ 注释 flag `PDF_ANNOT_IS_*` + 线端 `PDF_ANNOT_LE_*`
- 文本 flag：`TEXT_*`（`TEXT_PRESERVE_LIGATURES`/`TEXT_DEHYPHENATE`/…）+ `TEXTFLAGS_*`（每种 get_text 模式的默认组合）
- 加密：`PDF_ENCRYPT_NONE` / `RC4_40` / `RC4_128` / `AES_128` / `AES_256` / `KEEP` / `UNKNOWN`
- 权限：`PDF_PERM_*`（`PRINT`/`MODIFY`/`COPY`/`ANNOTATE`/…）
- 表单控件：`PDF_WIDGET_TYPE_*` / `PDF_WIDGET_TX_FORMAT_*` / `PDF_FIELD_IS_*` / `PDF_BORDER_STYLE_*`
- 混合模式：`PDF_BM_*`；图章：`STAMP_*`；页标签：`PDF_PAGE_LABEL_*`
- redaction：`PDF_REDACT_TEXT_*` / `PDF_REDACT_IMAGE_*` / `PDF_REDACT_LINE_ART_*`
- 签名（只读语义）：`PDF_SIGNATURE_*`；token：`PDF_TOK_*`
- 版本：`VersionBind` / `VersionFitz` / `version` / `version_info`
