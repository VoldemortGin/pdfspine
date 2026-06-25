# PRD：坐标基准统一（MediaBox → CropBox）—— 消除跨通道空间偏移

状态：待实施
关联：源自 2025-06 空间一致性调研（见本文「调研依据」）。修复后发 pdfspine 0.0.6。

---

## 1. 背景 / 问题

pdfspine 把所有提取元素统一到 PyMuPDF device space（origin 左上、y 向下、`/Rotate` 已应用、单位 PDF point），且全仓库只有一个 user→device 变换函数 `page_transform`（`crates/pdf-text/src/layout.rs:200-209`）。**但坐标基准（page_transform 的 box 参数）分裂成两套**：

| 通道 | 基准 | 证据 |
|---|---|---|
| 数字文字（glyph/span/word/char）| **MediaBox** | `layout.rs:112` |
| 矢量 drawing path | **MediaBox** | `tables.rs:380-407`、`edit/drawings.rs:83` |
| 矢量表格 cell（由 words+drawings 构建）| **MediaBox** | `api/tables.rs:186-200` |
| 页面渲染（pixmap）| **CropBox** | `render.rs:199-208` |
| SVG | **CropBox** | `svg.rs:108` |
| OCR 框 | **CropBox** | `ocr/integration.rs` |
| 图像表格 cell | **CropBox** | `image_table.rs:169` |

`page_transform` 的平移项 `e,f` 取自 box 的 `(-x0, y1)`（`layout.rs:204-207`），所以当 **CropBox ≠ MediaBox**（页面有裁切边距）时，两组通道的 device 原点相差一个 crop 偏移 `(cropbox.x0 - mediabox.x0, …)`。

**后果**：同一页上，数字文字层的 bbox 与 OCR 层 / 渲染像素 / 图像表格 bbox **不在同一原点，直接叠加会错位**（偏移量 = crop 边距）。典型受影响场景：混合型 PDF（部分页有文本层、部分页是扫描件需 OCR），跨"文字层 ↔ OCR/渲染层"对齐时错位。

另注：文字层 clip 用的是 CropBox（`layout.rs:57,121-133,186-192`），但坐标原点用 MediaBox —— **clip 基准与坐标基准本身就不一致**，这也是该统一的一部分。

## 2. 目标

统一坐标基准，使所有提取通道在 **CropBox ≠ MediaBox** 的页上也共享同一原点，跨层 bbox 可直接叠加/对齐、零 crop 偏移。`crop == media`（最常见）的页保持坐标不变（回归保护）。

## 3. 方案：全部统一到 CropBox

PyMuPDF 自身的 device space 即以 **CropBox** 为基准（并裁掉 crop 外内容）。把数字文字 / 矢量 / drawing 的 `page_transform` 基准从 MediaBox 改为 CropBox，与渲染/OCR 对齐 —— 同时让坐标基准与既有的 CropBox clip 基准一致。

### 改动点
1. **数字文字**：`layout.rs:112` —— `page_transform` 的 box 参数 MediaBox → CropBox。
2. **矢量表格 / drawings**：`tables.rs:380-407`、`api/tables.rs:197`、`edit/drawings.rs:83` —— 同改 CropBox。
3. **clip 与坐标基准一致化**：`layout.rs:57,121-133,186-192` —— 文字层已用 CropBox clip，坐标改 CropBox 后两者基准统一（消除现有不一致）。
4. **公开 API 矩阵核对**：`pagetree.rs:303-338` 的 `transformation_matrix` / `rotation_matrix` / `derotation_matrix` 本就基于 CropBox（镜像 fitz）。改完后 `page.derotation_matrix()` 应能精确反变换提取出的文字 bbox（当前因基准不同不保证互逆）—— 加互逆精确性测试锁定。

### 不改
- `render.rs` / `svg.rs` / `ocr/integration.rs` / `image_table.rs` —— 已是 CropBox 基准，是对齐目标。
- OCR 像素 → device 的 `÷(dpi/72)` 无 y-flip 机制（`ocr/integration.rs:85-99`）—— 正确，不动。

## 4. 验证

- **新增 fixture**：CropBox ≠ MediaBox 的页（带裁切边距），断言：
  - 同位置的「数字文字 bbox」与「OCR/渲染像素 bbox」在同一原点（零 crop 偏移）。
  - 跨层叠加对齐正确。
- **回归保护**：`crop == media` 页坐标不变；既有 Python 736 + Rust 全套测试不破。
- **旋转 × crop 组合**：调整 `COORD-ROT-MEDIABOX`（`docs/test-case-catalog.md:1027`）为 CropBox 基准，确认 `/Rotate` + crop 同时存在时坐标正确。
- **API 互逆**：`transformation_matrix` ↔ 提取 bbox 经 `derotation_matrix` 反变换的精确互逆测试。

## 5. 风险 / 注意

- **行为变更（crop≠media 页）**：数字文字坐标会从 MediaBox 基变为 CropBox 基 —— 这是修 bug，但 pdfspine 0.0.5 已发布，下游若依赖旧 MediaBox 坐标需知会。**发 0.0.6 + CHANGELOG 标注**（crop≠media 页的文字坐标基准变更，属修正性 breaking）。
- **crop == media（最常见）**：零影响。
- **fitz 对齐**：确认改后与 PyMuPDF 在 crop≠media 页上的 `get_text` 坐标行为一致（pdfspine 以 fitz 兼容为契约）。

## 6. 范围外（另议，不在本 PRD）

调研同时发现的次要几何缺口，本 PRD 不处理，记此备查：
- `words.rs:98-103` 词间距用 raw device-x（旋转/竖排词边界不严谨，行聚类本身正确）。
- 竖排（wmode 1）advance 近似（`/W2`/`/DW2` 未落地）。
- OCR 框无真实 baseline、per-char cell 均分近似（`ocr/integration.rs:101-169`）。
- 非 90° 倍数 `/Rotate` 当 0 处理。

## 7. 交付物

- `pdf-text/layout.rs` + `tables.rs` + `api/tables.rs` + `edit/drawings.rs` 基准统一 CropBox。
- CropBox≠MediaBox fixture + 跨层对齐断言 + API 互逆测试。
- `COORD-ROT-MEDIABOX` 调整为 CropBox 基准。
- CHANGELOG 标注 + 发 pdfspine **0.0.6**。

## 调研依据

源自空间一致性只读调研（grep `page_transform(` 全 call sites + `model.rs` 坐标系注释 + `layout.rs` 变换/阅读顺序 + `ocr/integration.rs` 像素→device）。核心正机制：单一 `page_transform`（`layout.rs:200`）+ 「四角投影取轴对齐外包」的 `Rect::transform`（`geom/rect.rs:207-213`）+ glyph 级 `Trm`（`interp.rs:1321`）+ 阅读顺序 XY-cut 分列 + content-order `seq` 排序。唯一实质缺口即本 PRD 处理的 MediaBox/CropBox 双基准。
