"""Public replay validation must leave targets and device lifecycle intact."""

import pdfspine as p
import pytest


@pytest.mark.parametrize("mode", ["callback", "textpage", "pixmap"])
def test_wrong_area_fails_before_effects_and_same_device_remains_usable(mode):
    source = p.open()
    page = source.new_page(width=60, height=60)
    page.insert_text((10, 25), "NEW", fontsize=8)
    page.draw_rect((5, 35, 25, 50), fill=(1, 0, 0))
    record = page.get_displaylist()
    events = []
    old = p.open()
    old.new_page(width=60, height=60).insert_text((10, 15), "OLD", fontsize=8)
    text = old[0].get_textpage()
    original_core = text._tp
    original_text = text.extractRAWDICT()
    pixmap = p.Pixmap(3, (0, 0, 60, 60), True)
    pixmap.clear_with(0)
    pixmap.set_dpi(111, 222)
    before = pixmap.samples
    pointer = pixmap.samples_ptr
    geometry = (pixmap.width, pixmap.height, pixmap.x, pixmap.y)
    device = {
        "callback": lambda: p.ReplayDevice(events.append),
        "textpage": lambda: p.ReplayDevice.for_textpage(text),
        "pixmap": lambda: p.ReplayDevice.for_pixmap(pixmap),
    }[mode]()

    with pytest.raises(ValueError, match="area must contain four coordinates"):
        record.run(device, None, (0, 0, 30))
    assert events == []
    assert text._tp is original_core and text.extractRAWDICT() == original_text
    assert pixmap.samples == before and pixmap.samples_ptr == pointer
    assert (pixmap.width, pixmap.height, pixmap.x, pixmap.y) == geometry
    assert (pixmap.xres, pixmap.yres) == (111, 222)

    # A failed validation must not leave the run token held for any target mode.
    assert record.run(device, None, None) is None
    if mode == "callback":
        assert events[0].kind == "begin" and events[-1].kind == "end"
        assert len(events) > 2
    elif mode == "textpage":
        assert text.extractText().split() == ["OLD", "NEW"]
    else:
        assert pixmap.samples != before
        assert pixmap.pixel(10, 40) == (255, 0, 0, 255)


@pytest.mark.parametrize(
    "factory,bad_target,message",
    [
        (p.ReplayDevice, None, "callback must be callable"),
        (p.ReplayDevice.for_pixmap, object(), "target must be a Pixmap"),
    ],
)
def test_replay_factories_reject_incompatible_public_inputs(
    factory, bad_target, message
):
    with pytest.raises(TypeError, match=message):
        factory(bad_target)
