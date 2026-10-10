//! SYLK (`.slk`) tables as used by Warcraft III (e.g. `UnitData.slk`).
//!
//! The first row holds column headers and the first column holds row keys
//! (for example unit raw codes). Records handled:
//! `ID` (ignored), `B` (bounds), `C` (cell), `E` (end). Other records are ignored.
//!
//! In a `C` record an omitted `X` means the previous `C` record's column plus one,
//! and an omitted `Y` means the previous record's row.

use std::collections::BTreeMap;
use std::fmt;

use crate::{decode_text, Error};

/// Largest row or column number accepted, to bound allocation on bad input.
const MAX_COORD: usize = 100_000;

/// A cell value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// Quoted or non-numeric text (`""` inside quotes is unescaped to `"`).
    Text(String),
    /// Integer literal.
    Int(i64),
    /// Real literal.
    Real(f64),
}

impl Value {
    /// Returns the text if this is a `Text` value.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Text(s) => Some(s),
            _ => None,
        }
    }

    /// Returns the integer if this is an `Int` value.
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            _ => None,
        }
    }

    /// Returns the number as `f64` for `Real` and `Int` values.
    pub fn as_real(&self) -> Option<f64> {
        match self {
            Value::Real(r) => Some(*r),
            Value::Int(i) => Some(*i as f64),
            Value::Text(_) => None,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Text(s) => f.write_str(s),
            Value::Int(i) => write!(f, "{i}"),
            Value::Real(r) => write!(f, "{r}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct RowData {
    key: String,
    /// Aligned with `Slk::columns`; `None` for empty cells.
    cells: Vec<Option<Value>>,
}

/// A parsed SLK table: named columns and keyed rows.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Slk {
    columns: Vec<String>,
    column_index: BTreeMap<String, usize>,
    rows: Vec<RowData>,
    row_index: BTreeMap<String, usize>,
}

/// A borrowed view of one row of an [`Slk`].
#[derive(Debug, Clone, Copy)]
pub struct Row<'a> {
    slk: &'a Slk,
    data: &'a RowData,
}

impl<'a> Row<'a> {
    /// The row key (the value of the first column).
    pub fn key(self) -> &'a str {
        &self.data.key
    }

    /// The value in the given column, if present.
    pub fn get(self, column: &str) -> Option<&'a Value> {
        let idx = *self.slk.column_index.get(column)?;
        self.data.cells[idx].as_ref()
    }

    /// All columns of this row in table order, with `None` for empty cells.
    pub fn iter(self) -> impl Iterator<Item = (&'a str, Option<&'a Value>)> {
        self.slk
            .columns
            .iter()
            .map(String::as_str)
            .zip(self.data.cells.iter().map(Option::as_ref))
    }
}

impl Slk {
    /// Parses SYLK text. Parsing stops at the `E` record.
    pub fn parse(src: &str) -> Result<Slk, Error> {
        let src = src.strip_prefix('\u{feff}').unwrap_or(src);
        let mut cells: BTreeMap<(usize, usize), Value> = BTreeMap::new();
        let (mut bound_x, mut bound_y) = (0, 0);
        let (mut last_x, mut last_y) = (0_usize, 1_usize);

        for (i, line) in src.lines().enumerate() {
            let line_no = i + 1;
            if line.trim().is_empty() {
                continue;
            }
            let fields = split_fields(line);
            let mut x = None;
            let mut y = None;
            let mut k = None;
            for field in &fields[1..] {
                let field = field.trim();
                let mut chars = field.chars();
                match chars.next() {
                    Some('X') => x = Some(parse_coord(chars.as_str(), line_no)?),
                    Some('Y') => y = Some(parse_coord(chars.as_str(), line_no)?),
                    Some('K') => k = Some(parse_value(chars.as_str())),
                    _ => {}
                }
            }
            match fields[0].trim() {
                "E" => break,
                "B" => {
                    bound_x = x.unwrap_or(bound_x);
                    bound_y = y.unwrap_or(bound_y);
                }
                "C" => {
                    let col = x.unwrap_or(last_x + 1);
                    let row = y.unwrap_or(last_y);
                    last_x = col;
                    last_y = row;
                    if let Some(value) = k {
                        cells.insert((row, col), value);
                    }
                }
                _ => {}
            }
        }

        let max_x = cells
            .keys()
            .map(|&(_, x)| x)
            .max()
            .unwrap_or(0)
            .max(bound_x);
        let max_y = cells
            .keys()
            .map(|&(y, _)| y)
            .max()
            .unwrap_or(0)
            .max(bound_y);

        let mut slk = Slk::default();
        for x in 1..=max_x {
            let name = cells.get(&(1, x)).map(Value::to_string).unwrap_or_default();
            slk.push_column(name);
        }
        for y in 2..=max_y {
            let Some(key) = cells.get(&(y, 1)) else {
                continue;
            };
            let key = key.to_string();
            if slk.row_index.contains_key(&key) {
                return Err(Error::DuplicateRowKey { key });
            }
            let row_cells = (1..=max_x).map(|x| cells.get(&(y, x)).cloned()).collect();
            slk.push_row(key, row_cells);
        }
        Ok(slk)
    }

    /// Parses SYLK bytes (UTF-8 or Windows-1252).
    pub fn parse_bytes(bytes: &[u8]) -> Result<Slk, Error> {
        Slk::parse(&decode_text(bytes))
    }

    /// Column headers in table order (the first is the row-key column).
    pub fn columns(&self) -> &[String] {
        &self.columns
    }

    /// The number of rows.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether the table has no rows.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The value at `row_key` and `column`, if the cell is non-empty.
    pub fn get(&self, row_key: &str, column: &str) -> Option<&Value> {
        let row = *self.row_index.get(row_key)?;
        let col = *self.column_index.get(column)?;
        self.rows[row].cells[col].as_ref()
    }

    /// The row with the given key.
    pub fn row(&self, row_key: &str) -> Option<Row<'_>> {
        let idx = *self.row_index.get(row_key)?;
        Some(Row {
            slk: self,
            data: &self.rows[idx],
        })
    }

    /// All rows in file order.
    pub fn rows(&self) -> impl Iterator<Item = Row<'_>> + '_ {
        self.rows.iter().map(move |data| Row { slk: self, data })
    }

    /// Merges `other` on top of `self`: non-empty cells in `other` replace
    /// cells in `self`. Unknown rows and columns are added; empty cells in
    /// `other` leave existing values alone.
    pub fn merge(&mut self, other: &Slk) {
        let map: Vec<usize> = other
            .columns
            .iter()
            .map(|name| self.column_or_insert(name))
            .collect();
        for orow in &other.rows {
            let row = self.row_or_insert(&orow.key);
            for (ocol, cell) in orow.cells.iter().enumerate() {
                if let Some(value) = cell {
                    self.rows[row].cells[map[ocol]] = Some(value.clone());
                }
            }
        }
    }

    fn push_column(&mut self, name: String) -> usize {
        let idx = self.columns.len();
        self.column_index.entry(name.clone()).or_insert(idx);
        self.columns.push(name);
        for row in &mut self.rows {
            row.cells.push(None);
        }
        idx
    }

    fn column_or_insert(&mut self, name: &str) -> usize {
        match self.column_index.get(name) {
            Some(&idx) => idx,
            None => self.push_column(name.to_owned()),
        }
    }

    fn push_row(&mut self, key: String, cells: Vec<Option<Value>>) -> usize {
        let idx = self.rows.len();
        self.row_index.insert(key.clone(), idx);
        self.rows.push(RowData { key, cells });
        idx
    }

    fn row_or_insert(&mut self, key: &str) -> usize {
        match self.row_index.get(key) {
            Some(&idx) => idx,
            None => {
                let cells = vec![None; self.columns.len()];
                self.push_row(key.to_owned(), cells)
            }
        }
    }
}

/// Splits a record on `;` outside double quotes. `""` inside quotes toggles
/// the state twice, so it does not end a quoted section.
fn split_fields(line: &str) -> Vec<&str> {
    let mut fields = Vec::new();
    let mut start = 0;
    let mut in_quote = false;
    for (i, ch) in line.char_indices() {
        match ch {
            '"' => in_quote = !in_quote,
            ';' if !in_quote => {
                fields.push(&line[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    fields.push(&line[start..]);
    fields
}

fn parse_coord(text: &str, line_no: usize) -> Result<usize, Error> {
    let n: usize = text.trim().parse().map_err(|_| Error::Slk {
        line: line_no,
        msg: format!("invalid coordinate {text:?}"),
    })?;
    if n == 0 || n > MAX_COORD {
        return Err(Error::Slk {
            line: line_no,
            msg: format!("coordinate {n} out of range"),
        });
    }
    Ok(n)
}

/// Parses the value after `K`: quoted text, a number, or bare text.
fn parse_value(text: &str) -> Value {
    if let Some(inner) = text.strip_prefix('"') {
        let inner = inner.strip_suffix('"').unwrap_or(inner);
        return Value::Text(inner.replace("\"\"", "\""));
    }
    let t = text.trim();
    if let Ok(i) = t.parse::<i64>() {
        return Value::Int(i);
    }
    let looks_numeric = t.chars().any(|c| c.is_ascii_digit())
        && t.chars().all(|c| c.is_ascii_digit() || "+-.eE".contains(c));
    if looks_numeric {
        if let Ok(r) = t.parse::<f64>() {
            return Value::Real(r);
        }
    }
    Value::Text(t.to_owned())
}
