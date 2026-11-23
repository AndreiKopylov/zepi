use std::collections::BTreeMap;
use std::sync::Arc;

use gridcraft_core::{CellRef, RangeRef, Value};
use gridcraft_formula::adjust::{Edit, adjust, shift_relative};
use gridcraft_model::{Cell, Formula, Sheet, Workbook};

use super::CellChange;

pub(super) type SheetChanges = Vec<(usize, Vec<CellChange>)>;

pub(super) fn shifted_formula(
    formula: Arc<Formula>,
    source: CellRef,
    target: CellRef,
) -> Arc<Formula> {
    if source != target
        && let Some(expr) = formula.expr.as_deref().cloned()
    {
        let dr = i64::from(target.row) - i64::from(source.row);
        let dc = i64::from(target.col) - i64::from(source.col);
        return Arc::new(Formula {
            dynamic: formula.dynamic,
            ..Formula::from_expr(shift_relative(expr, dr, dc))
        });
    }
    formula
}

pub(super) fn fill_cells(
    sheet: &Sheet,
    range: RangeRef,
    down: bool,
) -> anyhow::Result<Vec<CellChange>> {
    if (down && range.height() == 1) || (!down && range.width() == 1) {
        return Ok(Vec::new());
    }
    validate_range(sheet, range)?;
    let mut changes = Vec::new();
    for at in range.iter() {
        let source = if down {
            CellRef::new(range.start.row, at.col)
        } else {
            CellRef::new(at.row, range.start.col)
        };
        if at == source {
            continue;
        }
        let mut cell = sheet.cell(source).cloned().unwrap_or_default();
        if let Some(formula) = cell.formula.take() {
            cell.formula = Some(shifted_formula(formula, source, at));
            cell.value = Value::Empty;
        }
        let next = (!cell.is_blank()).then_some(cell);
        if sheet.cell(at) != next.as_ref() {
            changes.push((at, sheet.cell(at).cloned(), next));
        }
    }
    Ok(changes)
}

pub(super) fn move_cells(
    wb: &Workbook,
    sheet_index: usize,
    source: RangeRef,
    target: CellRef,
) -> anyhow::Result<(RangeRef, SheetChanges)> {
    let dr = i64::from(target.row) - i64::from(source.start.row);
    let dc = i64::from(target.col) - i64::from(source.start.col);
    let end = source
        .end
        .offset(dr, dc)
        .ok_or_else(|| anyhow::anyhow!("The cut range does not fit here. Nothing was moved."))?;
    let dest = RangeRef::new(target, end);
    if dest == source {
        return Ok((dest, Vec::new()));
    }
    let sheet = wb
        .sheet(sheet_index)
        .ok_or_else(|| anyhow::anyhow!("The cut sheet is no longer available."))?;
    validate_range(sheet, source)?;
    validate_range(sheet, dest)?;
    let edit = Edit::Move {
        from: source,
        to_row: target.row,
        to_col: target.col,
    };
    for name in &wb.names {
        let host = name
            .scope
            .and_then(|i| wb.sheet(i))
            .map_or("", |s| s.name.as_str());
        if let Ok(expr) = gridcraft_formula::parse_name(&name.formula) {
            let original = gridcraft_formula::print(&expr);
            let adjusted = gridcraft_formula::print(&adjust(expr, host, &sheet.name, &edit));
            anyhow::ensure!(
                original == adjusted,
                "The cut would change a defined name. Nothing was moved."
            );
        }
    }

    // Read both ranges before applying any changes, including when the ranges overlap.
    let mut relocated: BTreeMap<CellRef, Option<Cell>> = sheet
        .cells
        .iter_range(source)
        .chain(sheet.cells.iter_range(dest))
        .map(|(at, _)| (at, None))
        .collect();
    for (at, cell) in sheet.cells.iter_range(source) {
        let moved = at.offset(dr, dc).expect("the destination was checked");
        let cell =
            adjust_cell(cell, &sheet.name, &sheet.name, &edit).unwrap_or_else(|| cell.clone());
        relocated.insert(moved, Some(cell));
    }

    let mut changes = Vec::new();
    for (index, host) in wb.sheets.iter().enumerate() {
        let mut edits = if index == sheet_index {
            std::mem::take(&mut relocated)
        } else {
            BTreeMap::new()
        };
        for (at, cell) in host.cells.iter() {
            if !edits.contains_key(&at)
                && let Some(cell) = adjust_cell(cell, &host.name, &sheet.name, &edit)
            {
                edits.insert(at, Some(cell));
            }
        }
        let edits: Vec<CellChange> = edits
            .into_iter()
            .filter_map(|(at, new)| {
                let old = host.cell(at);
                (old != new.as_ref()).then(|| (at, old.cloned(), new))
            })
            .collect();
        if !edits.is_empty() {
            changes.push((index, edits));
        }
    }
    Ok((dest, changes))
}

fn adjust_cell(cell: &Cell, host: &str, target: &str, edit: &Edit) -> Option<Cell> {
    let formula = cell.formula.as_ref()?;
    let expr = adjust(formula.expr()?, host, target, edit);
    let moved = Formula::from_expr(expr);
    (moved.text != formula.text).then(|| Cell {
        formula: Some(Arc::new(Formula {
            array: formula.array,
            dynamic: formula.dynamic,
            ..moved
        })),
        ..cell.clone()
    })
}

fn validate_range(sheet: &Sheet, range: RangeRef) -> anyhow::Result<()> {
    // shortcut: only ordinary cells, extend the move history before moving sheet objects.
    anyhow::ensure!(
        sheet.protection.is_none()
            && !sheet.merges.iter().any(|r| r.intersects(&range))
            && !sheet
                .tables
                .iter()
                .any(|table| table.range.intersects(&range))
            && !sheet.spill_ranges.values().any(|r| r.intersects(&range))
            && !sheet.cells.iter().any(|(_, cell)| {
                cell.formula
                    .as_ref()
                    .and_then(|f| f.array)
                    .is_some_and(|r| r.intersects(&range))
            })
            && !sheet
                .cell_pictures
                .keys()
                .chain(sheet.comments.keys())
                .chain(sheet.hyperlinks.keys())
                .any(|at| range.contains(*at)),
        "This operation needs ordinary cells without protection, merges, tables, arrays, or cell objects."
    );
    Ok(())
}
