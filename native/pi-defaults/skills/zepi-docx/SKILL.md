---
name: zepi-docx
description: Read and edit Word documents with python-docx on the bundled runtime — styles, numbering, tables, headers and footers, page breaks, images, and targeted replacements. Use for any task whose input or output is a .docx file, including extracting text or tables, filling a template, fixing formatting, and assembling a report from several sources.
---

# Word documents

Run scripts with `& $env:ZEPI_PYTHON script.py`; the bundled runtime includes `python-docx` and `Pillow`.
If it is unavailable, tell the user rather than falling back to system Python. No `pip install`.

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

## Edit a placeholder in an existing document

Find the intended paragraph first. This example changes one single-line placeholder in one plain
text run. Do not apply it to tracked changes. If a match spans runs or is inside a hyperlink or
field, inspect it separately; never flatten the paragraph to make the replacement work.

```python
from docx import Document

doc = Document("report.docx")
p = doc.paragraphs[0]  # Select the intended body paragraph
old, new = "{{COMPANY}}", "Acme"
matches = [run for run in p.runs if old in run.text]
if (
    p.text.count(old) != 1
    or len(matches) != 1
    or p._p.xpath(".//w:fldChar | .//w:fldSimple | .//w:instrText")
    or matches[0]._r.xpath("*[not(self::w:rPr or self::w:t)]")
):
    raise ValueError("Expected one placeholder in a plain text run; inspect the paragraph.")

expected = p.text.replace(old, new, 1)
run = matches[0]
run.text = run.text.replace(old, new, 1)
assert p.text == expected
doc.save("out.docx")
```

## Add content to a new document

```python
from docx import Document
from docx.shared import Inches

doc = Document()

# add a paragraph with a style the document defines
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

# new header and footer
section = doc.sections[0]
section.header.paragraphs[0].text = "Acme · GovTech"
section.footer.paragraphs[0].text = "Draft"
doc.save("new.docx")
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
2. Follow `zepi-office-qa` to render the changed pages and open the PNGs. Check pagination, overlaps,
   and missing images.
3. Report what changed, in which file, and anything you could not verify.

## Merging and templates

- Do not merge documents by moving or copying body XML: links, images, styles, and numbering can
  depend on parts outside that body. If plain-text assembly is acceptable, create a new document from
  the extracted text and state what will not carry over. Otherwise report that a faithful merge is not
  covered by this workflow.
- To fill a template, make targeted edits and save under a new name. Do not rebuild the document
  from scratch: the template carries fonts, headers and page setup.
