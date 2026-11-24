---
name: zepi-office-qa
description: Visually check a generated or edited deck, document, or spreadsheet before reporting it done — render to PNG with Zepi, open the images, and check for visible defects. Use after any Office file is created or modified, and when the user asks why a slide looks wrong.
---

# Visual check of Office files

Code that runs is not a document that looks right. Slides are the worst case: text overflow, overlaps,
and broken alignment are invisible in `python-pptx` and obvious on screen.

## Render with Zepi

The editor sets `ZEPI_EXE` to the running Zepi executable. Do not guess a path from `ZEPI_ROOT` or
fall back to LibreOffice or Microsoft Office. The native renderer does not need Python.

```powershell
if (-not $env:ZEPI_EXE -or -not (Test-Path -LiteralPath $env:ZEPI_EXE -PathType Leaf)) {
    throw 'The Zepi Office renderer is unavailable in this session.'
}
$out = Join-Path $env:TEMP ('zepi-render-' + [guid]::NewGuid())
$render = & $env:ZEPI_EXE office render 'deck.pptx' --out $out --dpi 150 | ConvertFrom-Json
if (-not $render.ok) { throw "Office render failed: $($render.error)" }
if (-not $render.data.files) { throw 'Office render returned no images.' }
$render.data.files | ForEach-Object { Join-Path $render.data.out $_ }
```

- `--pages 1-3,5` selects slides, pages, or sheets (counting from 1). A selection outside the
  document's range fails instead of returning incomplete results.
- For spreadsheets, use `--cells B10:E40` to inspect the changed range on each selected sheet.
  Open bounds (`A100:`, `:E40`), column ranges (`A:E`), and single cells (`B7`) also work.
- PPTX produces `slide-N.png`, Word `page-N.png`, and Excel `sheet-N.png`. A large sheet may
  produce sampled `-top`, `-middle`, and `-bottom` windows rather than the full sheet.
- Use a fresh output directory for each run so stale PNGs cannot be mistaken for new results.

## Look at the result

1. Open the PNGs listed by this run with the image read tool, in order. Zoom in or render again
   with a higher `--dpi` if details are unreadable.
2. Check the changed pages, slides, or cells against the checklist. For a deck, compare alignment
   and typography across slides. Sampled windows do not verify the entire sheet.
3. If the command fails or the images cannot be opened, state what blocked visual verification.

## Defect checklist

Layout

- Text fits inside its shape; nothing runs past the slide edge or under the footer.
- No overlap between title, content, footnotes and page numbers.
- Margins are equal on all sides and consistent across slides.
- Tables do not grow past the footer (a grown row is a silent overflow).
- Images keep their aspect ratio; no stretching, no unexpected crop of faces or labels.
- For a sheet: no cell shows `######` (a number too wide for its column), wrapped text is not
  clipped mid-word, column widths look deliberate, and nothing meant for one cell spilled into a
  neighbor's territory.

Typography

- Fonts are the template's, not a silent fallback to a default face.
- No line with a single orphaned word, no hyphenated breaks in titles.
- Numbers formatted consistently: same decimal mark, same units, thin spaces for thousands.
- Footnotes and sources are readable at print size (>= 8 pt).

Content

- Title says the takeaway; the slide supports it with evidence.
- Every chart has a title, labelled axes or units, a readable number of categories and a source.
- Every figure on screen exists in the source data or is marked as an estimate.
- Nothing says "Lorem", "TODO", "[title]" or keeps template placeholder text.

## Report

State what was checked and how: "rendered to images, 9 slides, looked at all pages; fixed overlap on slide
4"; "rendered sheet 1, checked columns C-E for truncated numbers". Never claim a deck looks right without
having looked at it.

## Programmatic fallback

When the render fails, check the structure at least: reopen the file, assert the expected number of
slides, that titles are non-empty, that shape bounds stay inside `prs.slide_width/height`, and that text
length fits the estimated line budget (`zepi-deck-build` has the formula).
