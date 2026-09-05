"""构建语料清单：worktree 测试 PDF + 主仓 conformance/fixtures 语料，上限 300。"""
from pathlib import Path

HERE = Path(__file__).resolve().parent
CAP = 300

wt = [Path(p) for p in (HERE / "worktree_pdfs.txt").read_text().split("\n") if p.strip()]
main = [Path(p) for p in (HERE / "mainrepo_pdfs.txt").read_text().split("\n") if p.strip()]

wt_names = {p.name for p in wt}
keep: list[Path] = list(wt)
fintab: list[Path] = []
for p in main:
    s = str(p)
    if "/fuzz/" in s:
        continue
    if ("/fixtures/born/" in s or "/fixtures/typeset/" in s) and p.name in wt_names:
        continue  # 与 worktree 同名 fixture，去重
    if "corpus-fintabnet" in s:
        fintab.append(p)
    else:
        keep.append(p)

room = CAP - len(keep)
keep.extend(fintab[: max(room, 0)])
(HERE / "corpus.txt").write_text("\n".join(str(p) for p in keep) + "\n")
print("worktree:", len(wt), "main-nonfintab:", len(keep) - len(wt) - min(room, len(fintab)),
      "fintabnet kept:", min(room, len(fintab)), "of", len(fintab), "total:", len(keep))
