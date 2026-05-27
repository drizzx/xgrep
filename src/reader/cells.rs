//! Per-sheet cell parse: takes a calamine sheet range + formula range, applies
//! the configured layer set, and emits CellRecord values. Pure logic — no I/O.

use std::collections::HashSet;

use calamine::{Data, DataType, Range};
use chrono::Timelike as _;

use crate::cell::to_a1;
use crate::config::{Layer, LayerSet};
use crate::reader::CellRecord;

pub struct SheetParseInput<'a> {
    pub sheet_name: &'a str,
    pub range: &'a Range<Data>,
    pub formulas: Option<&'a Range<String>>,
    pub layers: LayerSet,
    pub hidden_rows: &'a HashSet<u32>,
    pub hidden_cols: &'a HashSet<u32>,
    pub include_hidden: bool,
}

pub fn parse_sheet(input: &SheetParseInput) -> Vec<CellRecord> {
    let mut out = Vec::new();
    let wants_formula = input.layers.contains(LayerSet::FORMULA);
    let wants_cached = input.layers.contains(LayerSet::CACHED);
    let wants_display = input.layers.contains(LayerSet::DISPLAY);

    for (row, col, data) in input.range.cells() {
        if data.is_empty() { continue; }
        if !input.include_hidden {
            if input.hidden_rows.contains(&(row as u32)) { continue; }
            if input.hidden_cols.contains(&(col as u32)) { continue; }
        }
        let a1 = to_a1(row as u32, col as u32);

        let formula_text: Option<String> = input
            .formulas
            .and_then(|f| f.get_value((row as u32, col as u32)).cloned())
            .filter(|s| !s.is_empty());

        if let Some(ftxt) = formula_text {
            if wants_formula {
                out.push(CellRecord {
                    sheet: input.sheet_name.to_owned(),
                    cell: a1.clone(),
                    layer: Layer::Formula,
                    text: ftxt,
                });
            }
            if wants_cached {
                let cached = display_value(data);
                if !cached.is_empty() {
                    out.push(CellRecord {
                        sheet: input.sheet_name.to_owned(),
                        cell: a1,
                        layer: Layer::Cached,
                        text: cached,
                    });
                }
            }
        } else if wants_display {
            let text = display_value(data);
            if !text.is_empty() {
                out.push(CellRecord {
                    sheet: input.sheet_name.to_owned(),
                    cell: a1,
                    layer: Layer::Display,
                    text,
                });
            }
        }
    }
    out
}

fn display_value(d: &Data) -> String {
    match d {
        Data::Empty => String::new(),
        Data::String(s) => s.clone(),
        Data::Float(f) => format_number(*f),
        Data::Int(i) => i.to_string(),
        Data::Bool(b) => b.to_string(),
        Data::DateTime(dt) => {
            if let Some(ndt) = dt.as_datetime() {
                let t = ndt.time();
                if t.hour() == 0 && t.minute() == 0 && t.second() == 0 {
                    ndt.format("%Y-%m-%d").to_string()
                } else {
                    ndt.format("%Y-%m-%d %H:%M:%S").to_string()
                }
            } else {
                dt.to_string()
            }
        }
        Data::DateTimeIso(s) => s.clone(),
        Data::DurationIso(s) => s.clone(),
        Data::Error(e) => format!("#{e:?}"),
    }
}

fn format_number(f: f64) -> String {
    if f.fract() == 0.0 && f.abs() < 1e15 {
        format!("{}", f as i64)
    } else {
        let s = format!("{:.10}", f);
        let trimmed = s.trim_end_matches('0').trim_end_matches('.');
        trimmed.to_owned()
    }
}
