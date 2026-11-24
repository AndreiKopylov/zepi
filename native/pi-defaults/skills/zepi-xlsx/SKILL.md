---
name: zepi-xlsx
description: Read, analyze and edit Excel workbooks with openpyxl on the bundled runtime — formulas, cached values, styles, column widths, merged cells, autofilters, freeze panes and charts — plus the data_only trap that destroys formulas on save. Use for any task whose input or output is a .xlsx file, including extracting numbers, filling a model, reformatting a report, and building a summary sheet.
---

# Excel workbooks

Run scripts with `& $env:ZEPI_PYTHON script.py`; the bundled runtime includes `openpyxl`.
If it is unavailable, tell the user rather than falling back to system Python. No `pip install`.

## Formulas and saved values

`data_only=True` reads the last **saved results**, not the formulas, and never recalculates. Use it
for reading only. Saving that workbook can replace formulas with cached values or erase them when
the cache is missing. Open without `data_only` for edits:

```python
from openpyxl import load_workbook

values = load_workbook("model.xlsx", data_only=True)        # analysis only, never saved
book = load_workbook("model.xlsx")                          # editing
```

For a formula cell, `None` means no saved result is available; it does not mean zero. A saved result
can also be stale. Zepi can calculate supported formulas with no saved result when opening or
rendering automatic-calculation workbooks. This does not update the file's cache during rendering.

## Read

```python
wb = load_workbook("plan.xlsx", data_only=True)
print(wb.sheetnames)
ws = wb["Load calculation"]
print(ws.dimensions, ws.max_row, ws.max_column)
for row in ws.iter_rows(min_row=1, max_row=5, values_only=True):
    print(row)
print(ws["B4"].value, ws["B4"].number_format)
print(ws.merged_cells.ranges)                               # merged blocks
for name, defined in wb.defined_names.items():               # named ranges
    print(name, defined.attr_text)
```

## Edit

```python
wb = load_workbook("plan.xlsx")
ws = wb.active

ws["B4"] = "=SUM(B2:B3)"                  # formula stays a formula
ws["C2"] = 1234.5
ws["C2"].number_format = "#,##0 ₽"        # width/format of the cell, not of the text

ws.column_dimensions["B"].width = 24
ws.row_dimensions[1].height = 22
ws.freeze_panes = "A2"
ws.auto_filter.ref = "A1:F20"
ws.merge_cells("A1:F1")

from openpyxl.styles import Font, PatternFill, Alignment
ws["A1"].font = Font(bold=True, size=14)
ws["A1"].alignment = Alignment(horizontal="center", vertical="center")
ws["A1"].fill = PatternFill("solid", fgColor="FFEEEE")

from openpyxl.chart import BarChart, Reference
chart = BarChart()
chart.add_data(Reference(ws, min_col=2, min_row=1, max_row=10), titles_from_data=True)
ws.add_chart(chart, "H2")

wb.save("out.xlsx")
```

Traps:

- openpyxl does not write calculated results for formulas. Do not replace formulas with numbers just
  to populate a missing cache.
- An openpyxl round trip can change or drop charts. If the source has charts, save to a new path and
  verify that the expected charts and their source ranges survived. Zepi's spreadsheet PNG shows the
  cell grid, not the embedded charts. Do not overwrite the original to work around this limitation.
- `ws.merge_cells` keeps only the top-left value; the rest is dropped. Write before merging.
- Copying a cell with `ws["B2"] = ws["B1"].value` loses the style; copy `font`, `fill`,
  `number_format`, `alignment` and `border` explicitly, or use `copy(cell._style)` on the new cell.
- Iterating `ws.rows` on a large sheet is slow and memory heavy; `iter_rows(values_only=True)` is the
  cheap path, and `read_only=True` helps for reading only.
- `.xlsm` macros are dropped on save; refuse instead of silently stripping them.

## Verify before reporting done

1. **Formula:** Reopen the output without `data_only`. Check the formula text and any changed input
   values in their expected cells on the intended sheet.
2. **Number:** Compute the expected result independently from the inputs and compare it with the
   result available in the workbook or viewer. If no result is available, report the expected value
   and that the workbook result could not be verified. A cache read is not a recalculation.
3. **Layout:** Follow `zepi-office-qa` with `--pages` for the changed sheet and `--cells` for the
   changed range. Inspect the PNG for clipped text and bad formatting, not as proof of the math.
4. Report what changed, in which sheet and cells, and anything you could not verify.
