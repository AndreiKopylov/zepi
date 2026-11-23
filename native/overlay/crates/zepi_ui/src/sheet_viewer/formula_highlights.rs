use std::collections::HashMap;
use std::ops::Range;

use gridcraft_core::RangeRef;
use gridcraft_formula::lexer::{Tok, tokenize};
use gridcraft_formula::{Expr, MAX_FORMULA_LEN, SheetSel, parse, same_name};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FormulaHighlight {
    pub chars: Range<usize>,
    pub cells: RangeRef,
    pub index: usize,
}

pub(super) fn formula_highlights(text: &str, sheet: &str) -> Vec<FormulaHighlight> {
    let Some(body) = text.strip_prefix('=') else {
        return Vec::new();
    };
    if body.len() > MAX_FORMULA_LEN {
        return Vec::new();
    }
    let Ok(tokens) = tokenize(body) else {
        return Vec::new();
    };
    let tokens: Vec<_> = tokens.iter().filter(|t| t.tok != Tok::Space).collect();
    let chars: Vec<_> = body
        .char_indices()
        .map(|(byte, _)| byte)
        .chain([body.len()])
        .collect();
    let char_at = |byte| chars.partition_point(|&b| b < byte) + 1;
    let token = |i: usize| tokens.get(i).map(|t| &t.tok);
    let mut highlights = Vec::new();
    let mut colors = HashMap::new();
    let mut i = 0;
    while i < tokens.len() {
        let start = tokens[i].start;
        if matches!(token(i), Some(Tok::Sheet(..))) {
            i += 1;
        }
        if !matches!(token(i), Some(Tok::Word(_))) {
            i += 1;
            continue;
        }
        i += 1;
        if matches!(token(i), Some(Tok::Colon | Tok::Trim(_))) {
            i += 1;
            if matches!(token(i), Some(Tok::Sheet(..))) {
                i += 1;
            }
            if matches!(token(i), Some(Tok::Word(_) | Tok::Number(_))) {
                i += 1;
            }
        }
        if matches!(token(i), Some(Tok::Op("#"))) {
            i += 1;
        }
        let end = tokens[i - 1].end;
        // shortcut: direct references only, resolve names and computed references when tracing dependencies.
        let Ok(Expr::Ref(reference)) = parse(&body[start..end]) else {
            continue;
        };
        if !matches!(reference.sheet, SheetSel::Current)
            && !matches!(&reference.sheet, SheetSel::Named(name) if same_name(name, sheet))
        {
            continue;
        }
        let cells = reference.range();
        let next = colors.len();
        let index = *colors.entry(cells).or_insert(next);
        highlights.push(FormulaHighlight {
            chars: char_at(start)..char_at(end),
            cells,
            index,
        });
    }
    highlights
}

#[cfg(test)]
mod tests {
    use super::*;
    use gridcraft_core::CellRef;

    #[test]
    fn highlights_direct_references_with_stable_colors_and_character_offsets() {
        let text = "=IF(A1=\"не B2\",SUM($B$2:C3),'Лист 1'!$A$1)+Other!D4";
        let refs = formula_highlights(text, "лист 1");
        let labels: Vec<String> = refs
            .iter()
            .map(|h| {
                text.chars()
                    .skip(h.chars.start)
                    .take(h.chars.len())
                    .collect()
            })
            .collect();
        assert_eq!(labels, ["A1", "$B$2:C3", "'Лист 1'!$A$1"]);
        assert_eq!(refs.iter().map(|h| h.index).collect::<Vec<_>>(), [0, 1, 0]);
        assert_eq!(
            refs[1].cells,
            RangeRef::new(CellRef::new(1, 1), CellRef::new(2, 2))
        );
        assert_eq!(formula_highlights("=B4-C4+", "Sheet1").len(), 2);
        assert!(formula_highlights("B4-C4", "Sheet1").is_empty());
    }
}
