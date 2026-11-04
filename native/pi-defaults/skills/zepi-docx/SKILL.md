---
name: zepi-docx
description: Read and edit Word documents with python-docx on the bundled runtime — styles, numbering, tables, headers and footers, page breaks, images, find-and-replace across runs, and merging documents. Use for any task whose input or output is a .docx file, including extracting text or tables, filling a template, fixing formatting, and assembling a report from several sources.
---

# Word documents

The runtime is `$env:ZEPI_PYTHON` (`%ZEPI_ROOT%\deps\python\python.exe`), with `python-docx` and
`Pillow` installed. Never run `pip install`; the package set is fixed and offline.

## Read

```python
from docx import Document
doc = Document("report.docx")

for style, text in [(p.style.name, p.text) for p in doc.paragraphs]:
    if text.strip():
        print(style, "|", text)

for table in doc.tables:                       # rows, columns, merged cells
    for row in table.rows:
        print([cell.text for cell in row.cells])

print(doc.core_properties.title, doc.core_properties.author)
print([s.name for s in doc.styles if s.type == 1])   # paragraph styles
```

Text lives in runs: a paragraph is a list of runs, each with its own bold/italic/font. `p.text`
joins them for reading; writing `p.text = "..."` collapses the whole paragraph into one plain run and
loses inline formatting inside it.

## Edit

```python
# replace text while keeping the paragraph's formatting
for p in doc.paragraphs:
    if "OLD" in p.text:
        p.runs[0].text = p.text.replace("OLD", "NEW")
        for run in p.runs[1:]:
            run.text = ""

# add a paragraph with a style the template defines
doc.add_paragraph("Total: $2,052,800", style="Heading 1")

# tables
table = doc.add_table(rows=1, cols=3, style="Table Grid")
header = table.rows[0].cells
for cell, value in zip(header, ["Parameter", "Value", "Source"]):
    cell.text = value
row = table.add_row().cells
row[0].text = "Rent"

# images and page breaks
doc.add_picture("chart.png", width=Inches(5.5))
doc.add_page_break()

# headers, footers, page numbers
section = doc.sections[0]
section.header.paragraphs[0].text = "Acme · GovTech"
section.footer.paragraphs[0].text = "Page "
doc.save("out.docx")
```

Traps that silently produce a wrong file:

- `Document("x.docx")` **rewrites** the file on `save` with the same name; save to a new name unless the
  user asked to overwrite, and tell them which file changed.
- `add_table` without a `style` produces a borderless table, and style names must exist in the
  document (a template's "Table Grid" may be localised (for example "Tabelle Gitter" in German)). Check `[s.name for s in doc.styles]`.
- `add_heading` uses built-in Heading styles, which a corporate template may not define; prefer
  `add_paragraph(..., style=<existing name>)`.
- Merged cells appear repeatedly in `row.cells`; writing to a merged cell writes the same cell several
  times. Deduplicate by `cell._tc`.
- Lists and numbering are stored outside the paragraph; adding bullets by typing "•" is not a list.
  Use `style="List Bullet"` (or the template's list style) instead.
- Fields (page numbers, dates) are not text; python-docx cannot add them. Say so instead of faking.
- Tracked changes and comments survive reading but cannot be authored; if the file has revisions, tell
  the user rather than saving a silently flattened copy.
- `.docm` loses macros in python-docx. Refuse to save over it.

## Verify before reporting done

1. Reopen the saved file and assert the paragraphs, tables and section texts you touched.
2. Render and look: `& "$env:ZEPI_ROOT\zepi.exe" office render "out.docx" --dpi 110`, then read the
   produced `page-N.png` files. Word's own layout is the reference; Zepi's renderer shows breaks,
   overlaps and missing images.
3. Report what changed, in which file, and anything you could not verify.

## Merging and templates

- `docxcompose` is not installed. To merge, copy the body elements of the source into the target:
  `for element in source.element.body: target.element.body.insert(-1, element)` (styles may differ —
  check the result visually).
- To fill a template, open it, replace runs in place, and save under a new name. Do not rebuild the
  document from scratch: the template carries fonts, headers and page setup.
