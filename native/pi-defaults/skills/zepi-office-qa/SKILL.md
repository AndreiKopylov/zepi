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

```powershell
& "$env:ZEPI_ROOT\zepi.exe" office render "deck.pptx" --dpi 150
```

- pptx → `slide-N.png`, docx → `page-N.png`, `--pages 1-3,5` for a subset, `--dpi 300` when details matter.
- Without `--out` the images go to `%LOCALAPPDATA%\Zepi\render\<file name>\`, overwritten on the next
  render; folders untouched for a week are deleted on the next run. Pass `--out` only when the images
  belong to the answer, for example when the user asked to see them.
- Never write renders into the project being edited: that is the user's working copy, not scratch space.
- **xlsx has no built-in rasteriser.** If the machine happens to have LibreOffice or Office, use it;
   Locate it instead of assuming an install path:
   ```powershell
   $soffice = (Get-Command soffice -ErrorAction SilentlyContinue).Source
   if (-not $soffice) { $soffice = @("$env:ProgramFiles\LibreOffice\program\soffice.exe", "${env:ProgramFiles(x86)}\LibreOffice\program\soffice.exe") | Where-Object { Test-Path $_ } | Select-Object -First 1 }
   ```
   An empty $soffice means no LibreOffice on this machine: skip the visual check and say so.
  otherwise do the structural check below and say plainly that the visual check was skipped.

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

Typography

- Fonts are the template's, not a silent fallback to a default face.
- No line with a single orphaned word, no hyphenated breaks in titles.
- Numbers formatted consistently: same decimal mark, same units, thin spaces for thousands.
- Footnotes and sources are readable at print size (>= 8 pt).

Content

- Title says the takeaway; the slide supports it with evidence.
- Every chart has a title, labelled axes or units, a readable number of categories and a source.
- Every figure on screen exists in the source data or is marked as an estimate.
- Nothing says "Lorem", "TODO", "[название]" or keeps template placeholder text.

## Report

State what was checked and how: "rendered to PDF, 9 slides, looked at all pages; fixed overlap on slide
4". If rendering is impossible (xlsx without LibreOffice): "visual check skipped — no rasteriser for this format;
verified structure only". Never claim a deck looks right without having looked at it.

## Programmatic fallback

When rendering is impossible, check the structure at least: reopen the file, assert the expected number of
slides, that titles are non-empty, that shape bounds stay inside `prs.slide_width/height`, and that text
length fits the estimated line budget (`zepi-deck-build` has the formula).
