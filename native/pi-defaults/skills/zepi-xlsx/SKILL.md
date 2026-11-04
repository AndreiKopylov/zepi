---
name: zepi-xlsx
description: Read, analyze and edit Excel workbooks with openpyxl on the bundled runtime — formulas, cached values, styles, column widths, merged cells, autofilters, freeze panes and charts — plus the data_only trap that destroys formulas on save. Use for any task whose input or output is a .xlsx file, including extracting numbers, filling a model, reformatting a report, and building a summary sheet.
---

# Excel workbooks

Runtime: `$env:ZEPI_PYTHON` (`%ZEPI_ROOT%\deps\python\python.exe`) with `openpyxl`. No `pip install`, no
network; the installed set is fixed.

## The one rule that matters

`load_workbook(path, data_only=True)` returns **cached values** (what Excel computed last save) and, if you
then `save`, every formula in the workbook is replaced by its last value. Read with
`data_only=True` for analysis, but **reload without it for any file you intend to save**:

```python
from openpyxl import load_workbook

values = load_workbook("model.xlsx", data_only=True)        # analysis only, never saved
book = load_workbook("model.xlsx")                          # editing
```

If a cell shows `None` with `data_only=True`, the file was never opened by Excel: the cache is empty.
Say so instead of reporting zero.

## Read

```python
wb = load_workbook("plan.xlsx", data_only=True)
print(wb.sheetnames)
ws = wb["Расчет загрузки"]
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

chart = wb.create_chartsheet() if False else None   # charts on a normal sheet instead:
from openpyxl.chart import BarChart, Reference
chart = BarChart()
chart.add_data(Reference(ws, min_col=2, min_row=1, max_row=10), titles_from_data=True)
ws.add_chart(chart, "H2")

wb.save("out.xlsx")
```

Traps:

- Formulas written by openpyxl have **no cached value**. Excel and LibreOffice recalculate on open, but a
  program reading `data_only=True` sees `None`. If the user needs values without opening the file, compute
  them in Python and write numbers (or state that a recalculation is needed).
- Charts do not survive a plain `load_workbook` + `save` round trip in all cases; if the workbook already
  has charts, keep the original and edit in place, then check the result visually.
- `ws.merge_cells` keeps only the top-left value; the rest is dropped. Write before merging.
- Copying a cell with `ws["B2"] = ws["B1"].value` loses the style; copy `font`, `fill`,
  `number_format`, `alignment` and `border` explicitly, or use `copy(cell._style)` on the new cell.
- Iterating `ws.rows` on a large sheet is slow and memory heavy; `iter_rows(values_only=True)` is the
  cheap path, and `read_only=True` helps for reading only.
- `.xlsm` macros are dropped on save; refuse instead of silently stripping them.

## Verify before reporting done

1. Reopen the saved file (`load_workbook`, no `data_only`) and assert the cells, formulas and sheet names
   you touched.
2. If the numbers matter, read them again with `data_only=True` **in a fresh workbook opened by Excel or
   LibreOffice**, or recompute the changed formulas in Python and compare.
3. Excel has no built-in rasteriser in Zepi; render with LibreOffice only if it happens to be installed.
   Locate it instead of assuming an install path:
   ```powershell
   $soffice = (Get-Command soffice -ErrorAction SilentlyContinue).Source
   if (-not $soffice) { $soffice = @("$env:ProgramFiles\LibreOffice\program\soffice.exe", "${env:ProgramFiles(x86)}\LibreOffice\program\soffice.exe") | Where-Object { Test-Path $_ } | Select-Object -First 1 }
   ```
   An empty $soffice means no LibreOffice on this machine: skip the visual check and say so.
   Otherwise say that the visual check was skipped.
4. Report what changed, in which sheet and cells, and anything you could not verify.
