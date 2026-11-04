---
name: zepi-deck-build
description: Build or edit a PowerPoint deck with the bundled python-pptx — outlines, slides from a template's layouts, text boxes, tables, native charts, Gantt timelines, images, speaker notes and footnotes, plus the python-pptx traps that silently break files. Use for any task that produces or modifies a .pptx.
---

# Building a deck

Design rules live in the `zepi-deck-design` skill; visual verification in `zepi-office-qa`. This skill is the
mechanics: how to write the file correctly with `python-pptx` on the bundled runtime.

## Process

1. Read every input (docx, xlsx, pptx, notes) and collect the real numbers. Never invent a figure: mark
   estimates as estimates.
2. Write the outline first, in Markdown, one block per slide: composition (from `zepi-deck-design`), title,
   the one sentence, the data it needs, the source.
3. Generate the deck from the template with a script. Keep the script, so the next revision is a re-run,
   not a re-decoration.
4. Verify structure programmatically (reopen, count slides, check text not empty).
5. Render and look at it (`zepi-office-qa`). Fix, do not "improve" what is already readable.

## Starting from a template

```python
import shutil
from pptx import Presentation
from pptx.util import Inches, Pt

shutil.copyfile("sample.pptx", "out.pptx")
prs = Presentation("out.pptx")
for i, layout in enumerate(prs.slide_layouts):
    print(i, layout.name, [(p.placeholder_format.idx, p.placeholder_format.type) for p in layout.placeholders])
slide = prs.slides.add_slide(prs.slide_layouts[1])
slide.shapes.title.text = "Revenue grows 18%"
slide.placeholders[1].text_frame.text = "One idea per slide"
prs.save("out.pptx")
```

Slide size comes from the template: `prs.slide_width`, `prs.slide_height` are EMU (914400 per inch).
Never hardcode 13.333 × 7.5 inches — check first.

## Text and layout

```python
box = slide.shapes.add_textbox(Inches(0.6), Inches(1.4), Inches(6.0), Inches(1.2))
frame = box.text_frame
frame.word_wrap = True
frame.text = "First paragraph"
para = frame.add_paragraph()
para.text = "Second"
para.font.size = Pt(16)
para.space_after = Pt(6)
box.text_frame.margin_left = 0    # align box text with the edge
```

python-pptx does **not** recompute text size: overflow is invisible in code and obvious on the slide.
Estimate before writing: characters per line ≈ box_width_pt / (0.55 × font_pt); lines ≈ ceil(chars /
chars_per_line); needed height ≈ lines × 1.25 × font_pt + 2 × padding. If it does not fit, cut words or
split the slide — do not shrink below 12 pt.

## Tables

```python
rows, cols = 4, 3
shape = slide.shapes.add_table(rows, cols, Inches(0.6), Inches(2.0), Inches(8.0), Inches(1.6))
table = shape.table
table.columns[0].width = Inches(4.0)
table.rows[0].height = Inches(0.4)
for c, text in enumerate(["Option", "img/s", "Cost"]):
    table.cell(0, c).text = text
table.cell(1, 0).text = "RTX 2080 Ti"
table.cell(1, 1).text = "2,58"
```

Row heights are minimums: PowerPoint grows a row when the text needs it, which pushes the table over the
footer. Keep cell text short, and put units in the header.

## Native charts

```python
from pptx.chart.data import CategoryChartData
from pptx.enum.chart import XL_CHART_TYPE, XL_LEGEND_POSITION

data = CategoryChartData()
data.categories = ["April", "May", "June"]
data.add_series("Rollout", (1, 1, 0))
chart = slide.shapes.add_chart(XL_CHART_TYPE.COLUMN_CLUSTERED, Inches(0.6), Inches(1.6),
                               Inches(6.0), Inches(3.4), data).chart
chart.has_title = True
chart.chart_title.text_frame.text = "Rollout takes two months"
chart.has_legend = False
plot = chart.plots[0]
plot.has_data_labels = True
plot.data_labels.font.size = Pt(10)
chart.value_axis.has_major_gridlines = False
chart.value_axis.maximum_scale = 2.0
```

A Gantt chart is a stacked bar with an invisible first series carrying the offset:

```python
data = CategoryChartData()
data.categories = ["Rollout", "Form analysis", "Risks"]
data.add_series("offset", (0, 2, 4))          # hidden
data.add_series("duration", (2, 2, 1))
chart = slide.shapes.add_chart(XL_CHART_TYPE.BAR_STACKED, ...).chart
chart.plots[0].series[0].format.fill.background()   # hide the offset series
```

Set `chart.font.size`, axis label sizes and `chart.chart_title` explicitly: the default palette is dated
and the frame is heavy. Colours come from the template's accent, as `RGBColor(0x1F, 0x4E, 0x79)` — never
a `"#` prefix.

## Images, notes, footnotes

```python
slide.shapes.add_picture("logo.png", Inches(11.5), Inches(0.4), width=Inches(1.2))
slide.notes_slide.notes_text_frame.text = "Say out loud: 30 seconds"
foot = slide.shapes.add_textbox(Inches(0.6), Inches(6.9), Inches(9.0), Inches(0.3))
foot.text_frame.text = "* the chart covers the main exam window only"
foot.text_frame.paragraphs[0].font.size = Pt(9)
```

Notes hold the reasoning the presenter needs, including the decisions you made and open questions.
Mark them by slide number when the user must check something ("Option 3 — chose A6000 because…").

## Editing an existing deck

- Text: set `paragraph.runs[0].text` and clear the remaining runs, so inline formatting survives. Setting
  `paragraph.text` wipes runs but keeps paragraph style.
- Slides: no API for reordering; move entries of `prs.slides._sldIdLst` and fix relationships, or rebuild
  the file from the template with the desired order.
- Deletion leaves orphaned media; that is acceptable for small files, noticeable in large ones.
- Never round-trip a deck you only needed to read.

## Deliver

Save as a new file unless the user asked to overwrite, then report: which file, how many slides, what
you changed, what you could not verify, and the open questions for the user.
