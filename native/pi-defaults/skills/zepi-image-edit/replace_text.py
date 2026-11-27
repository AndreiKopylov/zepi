import argparse
import os
import sys
from collections import Counter
from pathlib import Path

from PIL import Image, ImageChops, ImageFilter, ImageFont, ImageOps


INK_DELTA = 22
EXPANSION_DELTA = 8
MIN_FONT_SIZE = 8


def ink_mask(crop, background):
    delta = ImageChops.difference(crop, Image.new('RGB', crop.size, background))
    r, g, b = delta.split()
    return ImageChops.lighter(ImageChops.lighter(r, g), b)


def match_font(text, target):
    fonts = Path(os.environ['WINDIR']) / 'Fonts'
    size = target.size
    best = None
    target = target.resize((min(512, target.width), target.height), Image.Resampling.NEAREST)
    area = target.histogram()[255]
    for name in ('segoeui.ttf', 'segoeuib.ttf', 'segoeuil.ttf', 'calibri.ttf', 'arial.ttf', 'arialbd.ttf'):
        path = fonts / name
        if not path.is_file():
            continue
        for px in range(max(6, int(size[1] * .85)), int(size[1] * 1.5) + 1):
            font = ImageFont.truetype(str(path), px, layout_engine=ImageFont.Layout.BASIC)
            mask = font.getmask(text, mode='L')
            glyph = Image.frombytes('L', mask.size, bytes(mask)).point(lambda p: 255 if p >= 64 else 0)
            if not (bounds := glyph.getbbox()):
                continue
            glyph = glyph.crop(bounds)
            width_error = abs(glyph.width / size[0] - 1)
            height_error = abs(glyph.height / size[1] - 1)
            if width_error + height_error > .45:
                continue
            shape = glyph.resize(target.size, Image.Resampling.NEAREST)
            difference = ImageChops.difference(shape, target).histogram()[255]
            similarity = 1 - difference / (area + shape.histogram()[255])
            score = similarity - width_error * 1.4 - height_error * .45
            if best is None or score > best[0]:
                best = score, font
    if best is None or best[0] < .60:
        raise ValueError('The selected region does not reliably match one line of the supplied old text.')
    return best[1]


def rasterize_line(font, text, contrast):
    raster, (_, top) = font.getmask2(text, mode='L', anchor='ls')
    alpha = Image.frombytes('L', raster.size, bytes(raster))
    visible = alpha.point(lambda p: 255 if round(p * contrast / 255) > INK_DELTA else 0).getbbox()
    if visible is None:
        raise ValueError('The text has no visible glyphs at this size and color.')
    return alpha, top, visible


def contains(outer, inner):
    l, t, r, b = outer
    x0, y0, x1, y1 = inner
    return l <= x0 < x1 <= r and t <= y0 < y1 <= b


def outside_strips(outer, inner):
    x0, y0, x1, y1 = outer
    l, t, r, b = inner
    return [(x0, y0, x1, t), (x0, b, x1, y1), (x0, t, l, b), (r, t, x1, b)]


def can_expand(source, box, expanded, background):
    if not contains((0, 0, *source.size), expanded):
        return False
    for strip in outside_strips(expanded, box):
        if strip[0] < strip[2] and strip[1] < strip[3]:
            extra = source.crop(strip)
            if extra.getchannel('A').getextrema() != (255, 255) or ink_mask(extra.convert('RGB'), background).getextrema()[1] > EXPANSION_DELTA:
                return False
    return True


def replace_text(source, box, old, new, *, expand_to=None, font_size=None):
    l, t, r, b = box
    crop = source.crop(box)
    if crop.getchannel('A').getextrema() != (255, 255):
        raise ValueError('The selected background must be opaque.')
    rgb = crop.convert('RGB')
    w, h = rgb.size
    edges = [(0, 0, w, 1), (0, h - 1, w, h), (0, 1, 1, h - 1), (w - 1, 1, w, h - 1)]
    background = Counter(pixel for edge in edges for pixel in rgb.crop(edge).get_flattened_data()).most_common(1)[0][0]
    delta = ink_mask(rgb, background)
    if any(delta.crop(edge).getextrema()[1] > INK_DELTA for edge in edges):
        raise ValueError('The selection touches other content or the background is not uniform. Select just the text with a margin.')
    mask = delta.point(lambda p: 255 if p > INK_DELTA else 0)
    if (old_bounds := mask.getbbox()) is None:
        raise ValueError('No contrasting text found in the selection.')
    cutoff, peaks = 255, 0
    count = mask.histogram()[255]
    for level, frequency in reversed(list(enumerate(delta.histogram()))):
        peaks += frequency
        if peaks >= max(1, count // 8):
            cutoff = level
            break
    samples = [pixel for pixel, distance in zip(rgb.get_flattened_data(), delta.get_flattened_data()) if distance >= cutoff]
    color = tuple(round(sum(channel) / len(samples)) for channel in zip(*samples))
    target = delta.point(lambda p: 255 if p >= max(INK_DELTA + 1, int(cutoff * .32)) else 0)
    text_bounds = target.getbbox()
    font = match_font(old, target.crop(text_bounds))
    missing = bytes(font.getmask('\uffff'))
    if any(bytes(font.getmask(char)) == missing for char in set(new) if not char.isspace()):
        raise ValueError('The matched font does not support the replacement text.')
    contrast = max(abs(c - bg) for c, bg in zip(color, background))
    _, old_top, old_ink = rasterize_line(font, old, contrast)
    left = l + old_bounds[0]
    baseline = t + old_bounds[1] - (old_top + old_ink[1])

    def layout(selected_font):
        alpha, top, visible = rasterize_line(selected_font, new, contrast)
        x, y = left - visible[0], baseline + top
        # Check all antialiased pixels, not just the visible edge used for alignment.
        bounds = alpha.getbbox()
        x0, y0, x1, y1 = bounds
        return alpha.crop(bounds), (x + x0, y + y0, x + x1, y + y1)

    size = font.size
    if font_size is not None:
        if not MIN_FONT_SIZE <= font_size < size:
            raise ValueError(f'Choose a font size from {MIN_FONT_SIZE} px up to but below {size} px.')
        size = font_size
    glyphs, bounds = layout(font if font_size is None else font.font_variant(size=size))
    applied = box
    if not contains(box, bounds):
        if font_size is not None:
            raise ValueError(f'The replacement does not fit at the approved size ({size} px).')
        x0, y0, x1, y1 = bounds
        needed = (min(l, x0), min(t, y0), max(r, x1), max(b, y1))
        safe = can_expand(source, box, needed, background)
        if expand_to is not None:
            if not safe or expand_to != needed:
                raise ValueError('The approved expansion no longer matches what fits safely. Request new options.')
            applied = needed
        else:
            options = [f'The replacement does not fit at the original size ({size} px). No files written.']
            if safe:
                growth = ', '.join(f'{side} by {abs(a - b)} px' for side, a, b in zip(('left', 'top', 'right', 'bottom'), box, needed) if a != b)
                options.append(f'With approval: expand {growth} on the flat background with --expand-to {" ".join(map(str, needed))}.')
            else:
                options.append('No safe expansion on the current background.')
            for smaller in range(size - 1, MIN_FONT_SIZE - 1, -1):
                try:
                    _, candidate = layout(font.font_variant(size=smaller))
                except ValueError:
                    continue
                if contains(box, candidate):
                    options.append(f'With approval: keep the selection and use --font-size {smaller} ({smaller} px).')
                    break
            raise ValueError('\n'.join(options))
    elif expand_to is not None:
        raise ValueError('The text now fits the original selection. Omit --expand-to.')

    x, y = applied[:2]
    result = source.crop(applied)
    cleared = Image.composite(Image.new('RGB', rgb.size, background), rgb, mask.filter(ImageFilter.MaxFilter(7)))
    result.paste(cleared, (l - x, t - y))
    result.paste((*color, 255), (bounds[0] - x, bounds[1] - y), glyphs)
    return result, applied, size


def save_verified(source, result, output, box, icc_profile):
    strips = outside_strips((0, 0, *source.size), box)
    with output.open('xb') as file:
        try:
            result.save(file, format='PNG', icc_profile=icc_profile)
            file.flush()
            with Image.open(output) as saved:
                decoded = saved.convert('RGBA')
                if decoded.size != source.size or any(decoded.crop(s).tobytes() != source.crop(s).tobytes() for s in strips):
                    raise ValueError('Pixels outside the selected region changed.')
        except BaseException:
            file.close()
            output.unlink()
            raise


def main():
    parser = argparse.ArgumentParser(description='Replace one line of text on a flat background.')
    parser.add_argument('source', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--box', type=int, nargs=4, required=True, metavar=('LEFT', 'TOP', 'RIGHT', 'BOTTOM'))
    parser.add_argument('--old', required=True)
    parser.add_argument('--text', required=True)
    parser.add_argument('--preview', type=Path)
    approval = parser.add_mutually_exclusive_group()
    approval.add_argument('--expand-to', type=int, nargs=4, metavar=('LEFT', 'TOP', 'RIGHT', 'BOTTOM'), help='Apply an approved expanded box')
    approval.add_argument('--font-size', type=int, help='Apply an approved smaller font size in pixels')
    args = parser.parse_args()
    try:
        if args.output.suffix.lower() != '.png' or args.output.exists():
            raise ValueError('Choose a new output path with a .png extension.')
        if args.preview and (args.preview.exists() or args.preview.resolve() == args.output.resolve()):
            raise ValueError('Choose a separate new path for the preview.')
        if any(not value.strip() or len(value.splitlines()) != 1 for value in (args.old, args.text)):
            raise ValueError('Both old and new text must be nonempty, single-line strings.')
        with Image.open(args.source) as opened:
            if opened.format not in ('PNG', 'JPEG', 'WEBP') or getattr(opened, 'n_frames', 1) != 1:
                raise ValueError('Use a static PNG, JPEG, or WebP source.')
            icc_profile = opened.info.get('icc_profile') if opened.mode in ('RGB', 'RGBA') else None
            source = ImageOps.exif_transpose(opened).convert('RGBA')
        l, t, r, b = args.box
        if not (0 <= l < r <= source.width and 0 <= t < b <= source.height and r - l >= 3 and b - t >= 3):
            raise ValueError('The selection must be a valid pixel box within the oriented image.')
        patch, applied, size = replace_text(
            source, tuple(args.box), args.old, args.text,
            expand_to=tuple(args.expand_to) if args.expand_to is not None else None,
            font_size=args.font_size,
        )
        if args.preview:
            w, h = patch.size
            preview = Image.new('RGB', (w, h * 2 + 4), '#b0b0b0')
            preview.paste(source.crop(applied), (0, 0))
            preview.paste(patch, (0, h + 4))
            with args.preview.open('xb') as file:
                preview.save(file, format='PNG')
        result = source.copy()
        result.paste(patch, applied[:2])
        save_verified(source, result, args.output, applied, icc_profile)
        print(f'Saved {args.output} ({size} px); decoded pixels outside {applied} are unchanged.')
    except (OSError, ValueError) as error:
        sys.exit(str(error))


if __name__ == '__main__':
    main()
