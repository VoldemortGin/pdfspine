#!/usr/bin/env python3
"""把 paddle2onnx 导出的 PP-OCRv5 ONNX 转成 tract 0.21 可解析的形态。

# 背景

PaddleOCR 的 PP-OCRv5 权重经 [Paddle2ONNX] 转成 ONNX 后,图里带了两类
tract 0.21 解析不了的东西:

1. **非法的动态维度名**:paddle2onnx 写出的 symbolic 维度名形如
   ``DynamicDimension.0`` —— 含一个非法的 ``.``;更糟的是 ``value_info`` 里还有
   ``floor(floor(DynamicDimension.2/2 - 1/2)/2) + 1`` 这种把维度算式直接当成名字
   的字符串。tract 把维度名当标识符解析,遇到 ``.`` / ``(`` 直接报错。
2. **多余的 shape 提示**:``graph.value_info`` 与 output 上残留的、由上面那些算式
   组成的形状提示,tract 既解析不了、也不需要(它自己会做 shape inference)。

# 修复(确定性、可重复)

对每个原始 ONNX:

1. **重命名所有非法的 symbolic 维度名**:把任何含 ``.`` / ``(`` / 空白的 dim_param
   映射成一个合法标识符(``d0``、``d1`` …,按"首次出现顺序"分配,保证确定性)。
   合法的名字(纯字母数字下划线、且不以数字开头)原样保留。固定维度(``dim_value``)
   不动。
2. **清空 ``graph.value_info``**:``del graph.value_info[:]`` —— 删掉所有中间张量的
   形状提示(含 ``floor(...)`` 算式)。
3. **清空每个 output 的 shape 维度提示**:tract 会自行推断 output 形状,残留的
   symbolic 维度只会添乱。

修复后,tract 0.21 能 ``model_for_read → into_optimized → into_runnable → run``,
det / rec 仍保持动态输入(``[N,3,H,W]`` / ``[N,3,48,W]``),cls 保持 ``[N,3,80,160]``。

# 用法

    python scripts/strip_onnx_dims.py IN.onnx OUT.onnx

或批量(对一组 ``IN:OUT`` 对):

    python scripts/strip_onnx_dims.py a_in.onnx:a_out.onnx b_in.onnx:b_out.onnx

幂等 + 确定性:同样的输入永远产出 byte-identical 的输出,可重复运行。这是
**生成工具 + 可复现记录**:仓库里签入的是本脚本的【输出】,构建/运行时绝不跑它
(CI 零额外依赖)。

# 附:识别字典的烘焙(``dict`` 子命令)

PP-OCRv5 的识别字典(``ppocr_keys_v5.txt``,18383 行字符)需要烘焙成与本仓
``CharTable::load()`` 对齐的形态,才能 index-align 到 rec 的输出类轴:

    line 0      = "blank"   (CTC blank 占位,运行时被置空)
    line 1..N   = 18383 个字符(原始字典逐行照搬)
    last line   = " "       (use_space_char=True 时 PaddleOCR 追加的空格类)

烘焙后共 ``1 + 18383 + 1 = 18385`` 行,恰等于 rec 模型的输出类数(spike 实测
``[1, T, 18385]``)。用法:

    python scripts/strip_onnx_dims.py dict RAW_keys.txt BAKED_keys.txt

[Paddle2ONNX]: https://github.com/PaddlePaddle/Paddle2ONNX
"""

from __future__ import annotations

import re
import sys

import onnx
from onnx import TensorShapeProto


# 合法的 ONNX symbolic 维度名:首字符为字母或下划线,其余为字母数字下划线。
# tract 把维度名当作标识符;含 '.'、'('、空白、或算式的名字都解析不了。
_LEGAL_DIM = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")


def _is_legal_dim_param(name: str) -> bool:
    """该 symbolic 维度名是否是 tract 能接受的合法标识符。"""
    return bool(_LEGAL_DIM.match(name))


def _collect_illegal_dim_params(graph: onnx.GraphProto) -> dict[str, str]:
    """扫描 input / output / value_info,按首次出现顺序为每个非法 symbolic 维度名
    分配一个确定性的合法替换名(``d0``、``d1`` …)。

    返回 ``{原名: 新名}``。合法名不入表(原样保留),保证只改该改的。
    """
    mapping: dict[str, str] = {}

    def visit(value_infos) -> None:
        for vi in value_infos:
            ttype = vi.type.tensor_type
            if not ttype.HasField("shape"):
                continue
            for dim in ttype.shape.dim:
                # 只看 symbolic 维度(dim_param);固定维度(dim_value)不动。
                if dim.HasField("dim_param") and dim.dim_param:
                    name = dim.dim_param
                    if not _is_legal_dim_param(name) and name not in mapping:
                        mapping[name] = f"d{len(mapping)}"

    # 顺序固定:input → output → value_info,保证分配确定性。
    visit(graph.input)
    visit(graph.output)
    visit(graph.value_info)
    return mapping


def _rename_dim_params(value_infos, mapping: dict[str, str]) -> None:
    """把一组张量(input/output)的 symbolic 维度名按 ``mapping`` 重命名。"""
    for vi in value_infos:
        ttype = vi.type.tensor_type
        if not ttype.HasField("shape"):
            continue
        for dim in ttype.shape.dim:
            if dim.HasField("dim_param") and dim.dim_param in mapping:
                dim.dim_param = mapping[dim.dim_param]


def _clear_shape(value_info) -> None:
    """清空一个张量的 shape 维度提示(保留 elem_type)。

    tract 会自行推断 output 形状;残留的 symbolic/算式维度只会添乱,删掉最稳。
    """
    ttype = value_info.type.tensor_type
    ttype.ClearField("shape")
    # 给一个空 shape(rank 未知),让消费方走纯推断路径。
    ttype.shape.CopyFrom(TensorShapeProto())


def strip_model(model: onnx.ModelProto) -> onnx.ModelProto:
    """对一个已加载的 ONNX 模型做就地 strip,返回同一个(已修改的)对象。

    三步(见模块 docstring):重命名非法维度名 → 清空 value_info → 清空 output shape。
    确定性:对同一输入产出 byte-identical 的输出。
    """
    graph = model.graph

    # 1) 重命名所有非法的 symbolic 维度名(仅 input/output —— value_info 整体删掉)。
    mapping = _collect_illegal_dim_params(graph)
    _rename_dim_params(graph.input, mapping)
    _rename_dim_params(graph.output, mapping)

    # 2) 清空所有中间张量的形状提示(含 floor(...) 算式)。
    del graph.value_info[:]

    # 3) 清空每个 output 的 shape 维度提示(让 tract 自行推断)。
    for out in graph.output:
        _clear_shape(out)

    return model


def strip_file(src: str, dst: str) -> None:
    """读 ``src`` ONNX,strip,写到 ``dst``。"""
    model = onnx.load(src)
    strip_model(model)
    onnx.save(model, dst)
    print(f"stripped {src} -> {dst}")


def bake_dict(src: str, dst: str) -> None:
    """把原始 PP-OCRv5 识别字典烘焙成 ``CharTable`` 对齐的形态(见模块 docstring)。

    line 0 = "blank"、中间 = 原字典逐行、last line = 单个空格,共 N+2 行。
    确定性:对同一输入产出 byte-identical 的输出。
    """
    with open(src, "r", encoding="utf-8", newline="") as f:
        raw = f.read()
    chars = raw.split("\n")
    # 去掉文件末尾换行带来的空元素(原字典本身不含空行)。
    if chars and chars[-1] == "":
        chars.pop()
    baked = ["blank"] + chars + [" "]
    out = "\n".join(baked) + "\n"
    with open(dst, "w", encoding="utf-8", newline="") as f:
        f.write(out)
    print(f"baked dict {src} -> {dst} ({len(baked)} 行 = 1 blank + {len(chars)} 字符 + 1 space)")


def _parse_pairs(args: list[str]) -> list[tuple[str, str]]:
    """把命令行参数解析成 ``[(src, dst), ...]``。

    支持两种形式:
      * 两个位置参数:``IN.onnx OUT.onnx``;
      * 一组 ``IN:OUT`` 对(用于批量)。
    """
    if len(args) == 2 and ":" not in args[0]:
        return [(args[0], args[1])]
    pairs: list[tuple[str, str]] = []
    for arg in args:
        if ":" not in arg:
            raise SystemExit(f"批量模式下每个参数须为 'IN:OUT',得到:{arg!r}")
        src, dst = arg.rsplit(":", 1)
        pairs.append((src, dst))
    return pairs


def main(argv: list[str]) -> int:
    if len(argv) < 2:
        print(__doc__)
        print("用法: python scripts/strip_onnx_dims.py IN.onnx OUT.onnx")
        print("  或: python scripts/strip_onnx_dims.py IN1:OUT1 IN2:OUT2 ...")
        print("  字典: python scripts/strip_onnx_dims.py dict RAW_keys.txt BAKED_keys.txt")
        return 2
    # ``dict`` 子命令:烘焙识别字典。
    if argv[1] == "dict":
        if len(argv) != 4:
            raise SystemExit("用法: python scripts/strip_onnx_dims.py dict RAW_keys.txt BAKED_keys.txt")
        bake_dict(argv[2], argv[3])
        return 0
    for src, dst in _parse_pairs(argv[1:]):
        strip_file(src, dst)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
