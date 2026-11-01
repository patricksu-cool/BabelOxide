"""Small, bounded regression checks run with the packaged BabelDOC runtime.

No provider, credentials, network, model download or user PDF is used here.
The full paper mono/dual comparison is performed separately by compare_rust_pdf.
"""

import importlib.metadata
import unittest
from types import SimpleNamespace

import pymupdf

from babeldoc.format.pdf.document_il.il_version_1 import Box
from babeldoc.format.pdf.document_il.midend.typesetting import Typesetting
from babeldoc.format.pdf.document_il.utils.raster_geometry import with_pixel_budget
from babeldoc.format.pdf.document_il.utils.raster_geometry import with_target_long_edge


class Unit:
    """Controlled short Latin glyph followed by an em-height CJK glyph."""

    font_size = 12
    char = None
    width = 12
    is_space = False
    is_hung_punctuation = False
    is_cannot_appear_in_line_end_punctuation = False
    mixed_character_blacklist = True

    def __init__(self, text, height):
        self.text, self.height = text, height
        self.is_cjk_char = text == '中'
        self.box = None

    def relocate(self, x, y, scale):
        placed = Unit(self.text, self.height)
        placed.box = Box(x=x, y=y, x2=x + self.width * scale, y2=y + self.height * scale)
        return placed

    def try_get_unicode(self):
        return self.text


class EngineRegressionTests(unittest.TestCase):
    def test_engine_identity(self):
        self.assertEqual(importlib.metadata.version('babeldoc'), '0.6.4')

    def test_oversized_page_is_bounded_and_coordinates_round_trip(self):
        with pymupdf.open() as document:
            page = document.new_page(width=12000, height=18000)
            # Exercise the production helper with a deliberately small budget.
            geometry = with_pixel_budget(page, 72, 200_000, normalize_rotation=False)
            self.assertLessEqual(geometry.pixel_width * geometry.pixel_height, 200_000)
            self.assertLess(geometry.render_dpi, 72)
            for axis in ('x', 'y'):
                self.assertAlmostEqual(geometry.px_len_to_pt(geometry.pt_len_to_px(123, axis), axis), 123)

    def test_normal_page_preserves_geometry_and_rotation(self):
        with pymupdf.open() as document:
            page = document.new_page(width=612, height=792)
            page.set_rotation(90)
            geometry = with_target_long_edge(page, 72, 800, normalize_rotation=True)
            self.assertEqual((geometry.pixel_width, geometry.pixel_height), (612, 792))
            self.assertEqual(geometry.render_dpi, 72)
            self.assertEqual(page.rotation, 90)

    def test_mixed_latin_cjk_lines_cannot_collapse_to_short_glyph_height(self):
        layout = Typesetting.__new__(Typesetting)
        layout.font_mapper = SimpleNamespace(base_font=SimpleNamespace(char_lengths=lambda *_: [12]))
        units, fits = layout._layout_typesetting_units(
            [Unit('a', 5), Unit('b', 5), Unit('中', 12)],
            Box(x=0, y=0, x2=24, y2=80), 1.0, 1.2,
            SimpleNamespace(first_line_indent=False), use_english_line_break=False,
        )
        self.assertTrue(fits)
        self.assertEqual(len(units), 3)
        self.assertGreaterEqual(units[0].box.y - units[2].box.y, 12 * 1.2 - 1e-6)
        self.assertLessEqual(units[2].box.y2, units[0].box.y)


if __name__ == '__main__':
    unittest.main(verbosity=2)
