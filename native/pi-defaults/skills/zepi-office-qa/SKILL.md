---
name: zepi-office-qa
description: Visually check a generated or edited deck, document, or spreadsheet before reporting it done — render to PDF or images, open the result, and run a concrete defect checklist. Use after any Office file is created or modified, and when the user asks why a slide looks wrong.
---

# Visual check of Office files

Code that runs is not a document that looks right. Slides are the worst case: text overflow, overlaps,
and broken alignment are invisible in `python-pptx` and obvious on screen.

## Render

`& "$env:ZEPI_ROOT\zepi.exe" office render` is the only renderer you need (there is no bare `zepi` command on PATH): it runs Zepi's own engine, the same one the
built-in viewer uses, so the images match what the user sees on screen. No LibreOffice, no Office, no
network.

**Do not render with LibreOffice or Office.** A `soffice` install on the machine is not a reason to use
it: it renders a spreadsheet differently from the viewer, takes seconds per file and needs
reflection tricks to produce a PNG. Check for it and use it only when `office render` itself failed and
say so in the answer.

```powershell
& "$env:ZEPI_ROOT\zepi.exe" office render "deck.pptx" --dpi 150
```

- pptx → `slide-N.png`, docx → `page-N.png`, xlsx → `sheet-N.png`; a sheet too big for one readable
  image comes as `sheet-N-top.png`, `sheet-N-middle.png`, `sheet-N-bottom.png` (sampled windows across
  the whole sheet, the row numbers on each tell which part it is).
- `--pages 1-3,5` selects a subset: slides, pages, or sheets for xlsx. `--dpi 300` when details matter;
  for xlsx a lower `--dpi` fits more rows into one image.
- **xlsx: `--cells` renders any box you name**, instead of the automatic windows. Use it when a sheet
  is too big to read whole or when the user asks about a place in it, and to look at what you just
  changed:
  ```powershell
  office render "s.xlsx" --cells B10:E40        # that box
  office render "s.xlsx" --cells A100:          # from row 100 to the end of the used range
  office render "s.xlsx" --cells :E40           # from the start to E40
  office render "s.xlsx" --cells A:E            # columns A..E, every used row
  office render "s.xlsx" --pages 2 --cells A1:D30   # sheet 2 only
  ```
  A single reference works too (`--cells B7`). An unknown column or sheet is an error, not a guess:
  read `sheet-N.png` once or the values with Python before naming a box you have not seen.
- `--out DIR` picks the folder for the images when a specific place is wanted; the JSON prints the
  folder it used.
- Formulas written by a script carry no cached value, so a cell may show empty until opened in Excel;
  check values programmatically as well when the file was written by openpyxl.

A deck renders in well under a second per slide. If the command fails, read the JSON error instead of
guessing: it names the format, the page or the missing file.

## Self-check before blaming the environment

```powershell
& "$env:ZEPI_PYTHON" -c "import pptx, docx, openpyxl; print(pptx.__version__)"
& "$env:ZEPI_ROOT\zepi.exe" office render "sample.pptx" --pages 1 --out "$env:TEMP\zepi-selfcheck"
```

Two lines, and you know whether documents work: the first proves the runtime, the second the renderer. If
`ZEPI_PYTHON` is empty, use `"$env:ZEPI_ROOT\deps\python\python.exe"`.

## Look at the result

1. Open the produced PDF in Zepi's viewer (`office_viewer` page navigation works with up/down).
2. Look at every page, in order, at full size. Zoom is for the details, full size is for the layout.
3. For a deck, compare slides against each other: titles at the same height, margins equal, one accent.

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
