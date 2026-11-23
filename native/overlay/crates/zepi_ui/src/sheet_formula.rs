use gridcraft_calc::{Calc, Key};
use gridcraft_core::CellRef;
use gridcraft_model::{CalcMode, Workbook};

pub(crate) fn fill_missing_formulas(wb: &mut Workbook) {
    if wb.calc.mode == CalcMode::Manual {
        return;
    }
    let missing: Vec<Key> = wb
        .sheets
        .iter()
        .enumerate()
        .flat_map(|(index, sheet)| {
            sheet.cells.iter().filter_map(move |(at, cell)| {
                (cell.formula.is_some() && cell.value.is_empty()).then_some((index, at))
            })
        })
        .collect();
    if !missing.is_empty() {
        // A fresh graph evaluates only the missing formulas and reads saved results as inputs.
        Calc::new().cells_changed(wb, &missing);
    }
}

pub(crate) fn recalculate_cells(
    calc: &mut Calc,
    wb: &mut Workbook,
    sheet: usize,
    cells: impl IntoIterator<Item = CellRef>,
) {
    let changed: Vec<Key> = cells.into_iter().map(|at| (sheet, at)).collect();
    recalculate_changes(calc, wb, &changed);
}

pub(crate) fn recalculate_changes(calc: &mut Calc, wb: &mut Workbook, changed: &[Key]) {
    if !changed.is_empty()
        && (!calc.graph.is_empty()
            || changed.iter().any(|&(sheet, at)| {
                wb.sheet(sheet)
                    .and_then(|s| s.cell(at))
                    .is_some_and(|cell| cell.formula.is_some())
            }))
    {
        calc.cells_changed(wb, changed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gridcraft_core::Value;
    use gridcraft_model::{Cell, Formula};

    #[test]
    fn calculates_uncached_formulas_and_uses_saved_results_as_inputs() {
        let mut wb = Workbook::new();
        let sheet = wb.sheet_mut(0).unwrap();
        let cached = CellRef::new(1, 1);
        let profit = CellRef::new(1, 3);
        let total = CellRef::new(2, 3);
        let mut revenue = Cell::formula(Formula::new("UnsupportedTestFunction()"));
        revenue.value = Value::Number(50_000.0);
        sheet.set_cell(cached, revenue);
        sheet.set_value(CellRef::new(1, 2), Value::Number(32_000.0));
        sheet.set_cell(profit, Cell::formula(Formula::new("B2-C2")));
        sheet.set_cell(total, Cell::formula(Formula::new("SUM(D2:D2)")));

        let bytes = gridcraft_xlsx::write_xlsx(&wb).unwrap();
        let (mut wb, _) = gridcraft_xlsx::read_xlsx(&bytes).unwrap();
        wb.calc.mode = CalcMode::Manual;
        fill_missing_formulas(&mut wb);
        assert_eq!(wb.sheet(0).unwrap().value(profit), Value::Empty);

        wb.calc.mode = CalcMode::Automatic;
        fill_missing_formulas(&mut wb);
        let sheet = wb.sheet(0).unwrap();
        assert_eq!(sheet.value(cached), Value::Number(50_000.0));
        assert_eq!(sheet.value(profit), Value::Number(18_000.0));
        assert_eq!(sheet.value(total), Value::Number(18_000.0));
    }
}
