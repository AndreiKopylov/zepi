---
name: zepi-office-docs
description: Create, read, and edit Word (.docx), Excel (.xlsx), and PowerPoint (.pptx) files with the bundled Python runtime. Use whenever a task involves an Office document as input or output, including extracting text or numbers from a document, building a deck in the style of a template, editing an existing report or spreadsheet, and adding charts, tables, or speaker notes.
---

# Office documents

Zepi ships a pinned Python runtime with the document libraries installed. Use it instead of any
system Python: the system one may be missing, may be the Windows Store stub, or may lack packages.

## Runtime

```powershell
python -c "import pptx, docx, openpyxl, PIL, pypdf; print(pptx.__version__)"   # resolves to the bundled one
& $env:ZEPI_PYTHON --version                                                  # explicit path
```

`python` inside agent commands is the bundled runtime: `%ZEPI_ROOT%\deps\python\python.exe`.
Available: `python-pptx`, `python-docx`, `openpyxl`, `Pillow`, `pypdf`, `lxml`.

Never run `pip install`. The runtime is offline and its package set is fixed. If a task needs a
library that is missing, say so and find a way with what is installed.

## Reading

```python
from docx import Document
for p in Document("report.docx").paragraphs:      # tables: .tables
    if p.text.strip(): print(p.style.name, p.text)

from openpyxl import load_workbook
wb = load_workbook("plan.xlsx", data_only=True)    # data_only=True gives computed values
for row in wb["Расчет загрузки"].iter_rows(values_only=True): print(row)

from pptx import Presentation
prs = Presentation("deck.pptx")
print(prs.slide_width, prs.slide_height, [l.name for l in prs.slide_layouts])
for i, slide in enumerate(prs.slides):
    for shape in slide.shapes:
        if shape.has_text_frame: print(i, shape.name, shape.text_frame.text)
    if slide.has_notes_slide: print("notes:", slide.notes_slide.notes_text_frame.text)
```

`pypdf` for PDFs: `from pypdf import PdfReader; "".join(p.extract_text() for p in PdfReader(f).pages)`.

## Building a deck in the style of a template

Copy the template first, then work inside it so masters, layouts, theme, and fonts stay intact:

```python
import shutil
from pptx import Presentation
shutil.copyfile("sample.pptx", "out.pptx")
prs = Presentation("out.pptx")
layout = next(l for l in prs.slide_layouts if "Title and Content" in l.name)
slide = prs.slides.add_slide(layout)
slide.shapes.title.text = "Заголовок"
slide.placeholders[1].text_frame.text = "Первый пункт"
slide.notes_slide.notes_text_frame.text = "Что сказать вслух"
prs.save("out.pptx")
```

Rules that cost the most time when ignored:

- Set slide size from the template (`prs.slide_width/height`), never assume 16:9 inches.
- Text overflow is invisible in code: keep font sizes from the template placeholders, shorten the text,
  and check by rendering (see below) instead of guessing.
- Charts must be native: `slide.shapes.add_chart(XL_CHART_TYPE.COLUMN_CLUSTERED, ...)` with
  `CategoryChartData`. Do not paste a screenshot of a chart unless the chart type has no native form.
- Tables: `shapes.add_table(rows, cols, left, top, width, height)`; set column widths, then cell text.
- Delete unused template slides by editing the slide id list, and drop orphaned media afterwards,
  otherwise the file grows and shows stale slides.

## Editing existing files

- Word: rewrite `paragraph.runs[0].text` and clear the rest of the runs to preserve formatting, or use
  `paragraph.text = ...` only when the whole paragraph may lose its inline styling. Body left
  untouched keeps styles, numbering, headers, and footers.
- Excel: `load_workbook(path)` without `data_only`, write cells, then `wb.save`. Formulas stay formulas
  (`ws["B4"] = "=SUM(B2:B3)"`); `data_only=True` reads cached values but must never be saved, or every
  formula is replaced by its last value.
- Never round-trip a file you did not need to change. Save to a new name when the original matters,
  then tell the user which file changed.

## Verify before reporting done

1. Reopen the saved file with the matching library and assert the shapes/cells/paragraphs you touched.
2. Render and look at it: `& "$env:ZEPI_ROOT\zepi.exe" office render <file> --dpi 150`, then open the
   PNGs. Zepi's own renderer needs no LibreOffice and no Office.
3. Report exactly what changed, in which file, and anything you could not verify.

## Limits

- Spreadsheets (xlsx) have no built-in rasteriser: check the cells programmatically, or render with
  LibreOffice/Office only if the machine has one.
   Locate it instead of assuming an install path:
   ```powershell
   $soffice = (Get-Command soffice -ErrorAction SilentlyContinue).Source
   if (-not $soffice) { $soffice = @("$env:ProgramFiles\LibreOffice\program\soffice.exe", "${env:ProgramFiles(x86)}\LibreOffice\program\soffice.exe") | Where-Object { Test-Path $_ } | Select-Object -First 1 }
   ```
   An empty $soffice means no LibreOffice on this machine: skip the visual check and say so.
- Password-protected or macro-enabled (`.docm`, `.xlsm`) files: python-docx/openpyxl refuse or drop
  macros. Tell the user instead of silently saving a stripped copy.
