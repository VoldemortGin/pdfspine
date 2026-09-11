"""Exercise the packaged replay module, stub, documentation and typed targets."""

from importlib.resources import files

import pdfspine as pdf

package = files("pdfspine")
assert package.joinpath("replay.py").is_file()
assert package.joinpath("_core.pyi").is_file()
assert package.joinpath("_llms/docs/replay.md").is_file()
doc = pdf.open()
page = doc.new_page(width=20, height=20)
page.insert_text((2, 10), "A", fontsize=8)
record = page.get_displaylist()
events = []
record.run(pdf.ReplayDevice(events.append), pdf.Matrix(), None)
assert events[0].kind == "begin" and events[-1].kind == "end"
empty = pdf.open()
text = empty.new_page(width=20, height=20).get_textpage()
record.run(pdf.ReplayDevice.for_textpage(text), pdf.Matrix(), None)
assert "A" in text.extractText()
expected = record.get_pixmap(alpha=True)
target = pdf.Pixmap(pdf.csRGB, pdf.IRect(0, 0, 20, 20), True)
target.clear_with(0)
doc.close()
record.run(pdf.ReplayDevice.for_pixmap(target), pdf.Matrix(), None)
assert target.samples == expected.samples
print("packaged replay callback/TextPage/Pixmap smoke passed")
