import os
import re
import runpy
import unittest
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont


SKILL = Path(__file__).resolve().parents[2] / 'native/pi-defaults/skills/zepi-image-edit'
replace_text = runpy.run_path(str(SKILL / 'replace_text.py'))['replace_text']
SEGOE_UI = Path(os.environ.get('WINDIR', 'C:/Windows')) / 'Fonts/segoeui.ttf'
OLD = 'Ква-ква-ква'
NEW = 'Кря-кря-кря'
LONGER = 'Бла-бла-бла-бла'
TIGHT = (0, 0, 133, 40)


def visible_bounds(image):
    return image.getchannel('B').point(lambda p: 255 if p <= 230 else 0).getbbox()


@unittest.skipUnless(SEGOE_UI.is_file(), 'Requires Windows Segoe UI')
class ReplaceTextTest(unittest.TestCase):
    def crop(self, height, width=150):
        image = Image.new('RGBA', (width, height), 'white')
        font = ImageFont.truetype(str(SEGOE_UI), 19, layout_engine=ImageFont.Layout.BASIC)
        ImageDraw.Draw(image).text((16, 24), OLD, font=font, fill='#cd6e3c', anchor='ls')
        return image

    def test_aligns_visible_edge_and_keeps_descenders(self):
        before = self.crop(40)
        box = (0, 0, *before.size)
        after, applied, _ = replace_text(before, box, OLD, NEW)
        self.assertEqual(applied, box)
        b, a = visible_bounds(before), visible_bounds(after)
        self.assertEqual(a[:2], b[:2])
        self.assertGreater(a[3], b[3])

    def test_refuses_clipping(self):
        for crop, text in [(self.crop(26), NEW), (self.crop(40), NEW * 4)]:
            with self.assertRaisesRegex(ValueError, 'does not fit'):
                replace_text(crop, (0, 0, *crop.size), OLD, text)

    def test_offers_and_applies_only_the_approved_expansion(self):
        source = self.crop(40, width=220)
        with self.assertRaisesRegex(ValueError, 'does not fit') as error:
            replace_text(source, TIGHT, OLD, LONGER)
        msg = str(error.exception)
        approved = tuple(map(int, re.search(r'--expand-to (\d+(?: \d+){3})', msg).group(1).split()))
        self.assertGreater(approved[2], TIGHT[2])
        self.assertIn('--font-size ', msg)
        patch, applied, size = replace_text(source, TIGHT, OLD, LONGER, expand_to=approved)
        self.assertEqual((applied, size), (approved, 19))
        self.assertGreater(visible_bounds(patch)[2], TIGHT[2])
        with self.assertRaisesRegex(ValueError, 'approved expansion no longer matches'):
            replace_text(source, TIGHT, OLD, LONGER, expand_to=(0, 0, approved[2] + 1, 40))
        source.putpixel((TIGHT[2], 18), (100, 110, 120, 255))
        with self.assertRaisesRegex(ValueError, 'approved expansion no longer matches'):
            replace_text(source, TIGHT, OLD, LONGER, expand_to=approved)

    def test_rejects_obstructed_expansion_and_applies_approved_size(self):
        source = self.crop(40, width=220)
        source.putpixel((TIGHT[2], 18), (100, 110, 120, 255))
        with self.assertRaisesRegex(ValueError, 'No safe expansion') as error:
            replace_text(source, TIGHT, OLD, LONGER)
        self.assertNotIn('--expand-to ', str(error.exception))
        size = int(re.search(r'--font-size (\d+)', str(error.exception)).group(1))
        patch, applied, font_size = replace_text(source, TIGHT, OLD, LONGER, font_size=size)
        self.assertEqual((applied, font_size), (TIGHT, size))
        self.assertEqual(patch.size, source.crop(TIGHT).size)
        self.assertEqual(visible_bounds(patch)[0], visible_bounds(source.crop(TIGHT))[0])


if __name__ == '__main__':
    unittest.main()
