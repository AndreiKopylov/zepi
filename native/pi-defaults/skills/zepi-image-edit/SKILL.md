---
name: zepi-image-edit
description: Make a targeted image edit from a written request and an optional attached crop. Use to replace a label, change a diagram block, or retouch a region without changing the rest of the image.
---

# Edit an image region

## Identify the change

- Use the attachment to identify the region and the user's description to decide what to change. Copy replacement text verbatim, including punctuation and case. Keep unmentioned text, arrows, borders, and layout as they are.
- Zepi's image viewer adds an `Image source` note to the attachment for the agent. It contains the original path and the pixel box after `PIL.ImageOps.exif_transpose`. Check that the current source region matches the attachment visually; small JPEG decoder rounding differences are normal. If the content no longer matches, ask for a new selection.
- The attached pixel box is the boundary for raster edits. Pass its four coordinates unchanged to the helper. Do not widen or move the box to make an edit fit. If the edit needs to go outside, ask the user for a new selection or explicit approval before making that edit.
- For a manually pasted crop without a source note, find the original in the workspace or use the path the user supplied. Locate the crop using surrounding features. If the file or target is ambiguous, ask for the missing path or location instead of guessing. If the user only wants a revised crop, don't require the full image.

## Make the smallest edit

1. **Editable diagram:** If the matching Mermaid, SVG, or draw.io source is available, change the corresponding element in a copy of the source. If a renderer is available, export and inspect the result. Don't silently redraw unrelated parts or claim to have exported an image without rendering it.
2. **Simple raster edit:** For one line of text on an opaque, flat background, use the bundled `replace_text.py` next to this `SKILL.md` before writing any custom code. Give it the old text read from the attachment, the exact new text, and the pixel box. It picks a nearby system font, refuses uncertain matches or clipping, and verifies the PNG after writing. Read its optional before/after PNG once to check the visual result. If the user delegated the wording, choose a shorter phrase that fits at the original size, using the same box. If the user supplied exact text and it does not fit, keep the text as given and offer only the fit options reported by the helper. If the selection includes unrelated content, ask for a tighter selection. For a simple fill or shape, use bundled Pillow only; do not install packages. Never fill over a detailed background with a solid rectangle.
3. **Retouch or redraw:** Use an image-editing tool only if one is configured. Supply the crop or mask it actually needs and ask before sending an image to a separate external service. If the tool isn't available, say what is blocked rather than pretending a vision model can return edited pixels.

## Run the text replacement

Use the actual absolute path of this skill's directory, and a new path for each PNG:

```powershell
& $env:ZEPI_PYTHON 'C:\path\to\zepi-image-edit\replace_text.py' 'C:\work\image.jpg' 'C:\work\image-edited.png' --box 11 17 378 63 --old 'Old label' --text 'New label' --preview 'C:\temp\label-preview.png'
```

The four coordinates are separate integers. The helper aligns the visible left edge and checks that the entire rendered line fits before pasting it. The preview shows the same area before and after. This helper is for one line on a flat background, not OCR or exact font recovery.

If the text does not fit, the helper writes no files. It can report the exact safe expansion at the original size, the largest smaller size that fits the same selection, or both. Describe those options and ask the user to choose. After approval, rerun with the same source, original `--box`, and text; add the reported `--expand-to LEFT TOP RIGHT BOTTOM` or `--font-size SIZE` for the chosen option. Do not guess dimensions or use either flag without approval. If the helper reports no suitable option, ask for a new selection or different wording.

## Verify and deliver

- For a full-image raster result, work in source pixel coordinates, not viewport pixels. Use a nonempty in-bounds `(left, top, right, bottom)` crop with exclusive right and bottom edges. Edit the crop, check that its size still matches the box, then paste it back into a copy at `(left, top)` without rescaling the original. Even if a model returns a whole image, take only the verified patch that aligns with the selected box.
- Save a new PNG (e.g. `diagram-edited.png`), never re-encode the result as JPEG if the pixels outside the box must stay identical. `replace_text.py` verifies the saved pixels outside the original selection or the explicitly approved expanded box, including alpha. For custom edits, reopen the PNG and compare `crop(...).tobytes()` in the same color mode on the strips above, below, left, and right of the box. If pixels differ, fix the output before delivering it.
- Inspect the edited area for exact text, clipping, and seams at the edges. Report the output path and what was verified; if you couldn't preview it, say so. Don't overwrite the original.
