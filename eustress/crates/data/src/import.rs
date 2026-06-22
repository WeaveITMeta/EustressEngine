//! File import (Data Platform P6) — CSV and JSON/JSONL → [`Frame`], with
//! per-column dtype inference and unit extraction from header suffixes.
//!
//! Requires the `import` feature (`csv` + `serde_json`). This is the columnar
//! half of an import; routing it through the Eustress Parameters connector
//! taxonomy (`DataSourceType`) and applying consent / anonymization happens at
//! the engine boundary (plan §3.5 / Subsystem D) — the leaf just turns bytes
//! into a typed `Frame`.

use std::io::Read;

use crate::{frame_from_columns, ColumnData, ColumnDtype, ColumnSpec, DataError, Frame, Result};

/// Parse a CSV (with header row) into a [`Frame`]. Per column: integers →
/// `I64`, else numbers → `F64`, else `true`/`false` → `Bool`, else `Str`. An
/// empty cell is a null. A header of the form `name (unit)` sets the column's
/// unit symbol (e.g. `temp (C)` → name `temp`, unit `C`).
pub fn frame_from_csv<R: Read>(rdr: R) -> Result<Frame> {
    let mut reader = csv::ReaderBuilder::new().has_headers(true).from_reader(rdr);
    let headers: Vec<(String, Option<String>)> = reader
        .headers()
        .map_err(|e| DataError::Schema(format!("csv headers: {e}")))?
        .iter()
        .map(parse_header)
        .collect();
    let ncols = headers.len();
    if ncols == 0 {
        return Err(DataError::Schema("csv: no header columns".into()));
    }

    let mut cols: Vec<Vec<Option<String>>> = vec![Vec::new(); ncols];
    for rec in reader.records() {
        let rec = rec.map_err(|e| DataError::Schema(format!("csv row: {e}")))?;
        if rec.len() != ncols {
            return Err(DataError::Schema(format!(
                "csv row has {} fields, expected {ncols}",
                rec.len()
            )));
        }
        for (i, field) in rec.iter().enumerate() {
            cols[i].push(if field.is_empty() {
                None
            } else {
                Some(field.to_string())
            });
        }
    }

    let mut columns = Vec::with_capacity(ncols);
    for ((name, unit), cells) in headers.into_iter().zip(cols) {
        let (dtype, data) = infer_csv_column(&cells);
        let mut spec = ColumnSpec::new(name, dtype);
        if let Some(u) = unit {
            spec = spec.with_unit(u);
        }
        columns.push((spec, data));
    }
    frame_from_columns(columns)
}

/// Split a header like `temp (C)` into `("temp", Some("C"))`; a bare header
/// returns `(header, None)`.
fn parse_header(h: &str) -> (String, Option<String>) {
    let h = h.trim();
    if h.ends_with(')') {
        if let Some(open) = h.rfind('(') {
            let name = h[..open].trim();
            let unit = h[open + 1..h.len() - 1].trim();
            if !name.is_empty() && !unit.is_empty() {
                return (name.to_string(), Some(unit.to_string()));
            }
        }
    }
    (h.to_string(), None)
}

fn infer_csv_column(cells: &[Option<String>]) -> (ColumnDtype, ColumnData) {
    let present: Vec<&str> = cells.iter().flatten().map(|s| s.trim()).collect();
    let any = !present.is_empty();

    if any && present.iter().all(|s| s.parse::<i64>().is_ok()) {
        let v = cells
            .iter()
            .map(|o| o.as_ref().and_then(|s| s.trim().parse::<i64>().ok()))
            .collect();
        return (ColumnDtype::I64, ColumnData::I64(v));
    }
    if any && present.iter().all(|s| s.parse::<f64>().is_ok()) {
        let v = cells
            .iter()
            .map(|o| o.as_ref().and_then(|s| s.trim().parse::<f64>().ok()))
            .collect();
        return (ColumnDtype::F64, ColumnData::F64(v));
    }
    if any
        && present
            .iter()
            .all(|s| matches!(s.to_ascii_lowercase().as_str(), "true" | "false"))
    {
        let v = cells
            .iter()
            .map(|o| o.as_ref().map(|s| s.trim().eq_ignore_ascii_case("true")))
            .collect();
        return (ColumnDtype::Bool, ColumnData::Bool(v));
    }
    (ColumnDtype::Str, ColumnData::Str(cells.to_vec()))
}

/// Parse newline-delimited JSON objects (JSONL) into a [`Frame`]. Columns are
/// the first-seen union of object keys; a missing or `null` field is a null.
/// Per column: all integers → `I64`, all numbers → `F64`, all bools → `Bool`,
/// else `Str` (non-string JSON values are stringified).
pub fn frame_from_jsonl<R: Read>(rdr: R) -> Result<Frame> {
    use std::io::BufRead;

    let mut rows: Vec<serde_json::Map<String, serde_json::Value>> = Vec::new();
    for line in std::io::BufReader::new(rdr).lines() {
        let line = line?;
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        match serde_json::from_str::<serde_json::Value>(t)
            .map_err(|e| DataError::Schema(format!("jsonl parse: {e}")))?
        {
            serde_json::Value::Object(m) => rows.push(m),
            _ => return Err(DataError::Schema("jsonl: each line must be a JSON object".into())),
        }
    }

    let mut keys: Vec<String> = Vec::new();
    for r in &rows {
        for k in r.keys() {
            if !keys.iter().any(|e| e == k) {
                keys.push(k.clone());
            }
        }
    }

    let mut columns = Vec::with_capacity(keys.len());
    for key in &keys {
        let vals: Vec<Option<&serde_json::Value>> = rows
            .iter()
            .map(|r| r.get(key).filter(|v| !v.is_null()))
            .collect();
        let (dtype, data) = infer_json_column(&vals);
        columns.push((ColumnSpec::new(key.clone(), dtype), data));
    }
    frame_from_columns(columns)
}

fn infer_json_column(vals: &[Option<&serde_json::Value>]) -> (ColumnDtype, ColumnData) {
    let present: Vec<&serde_json::Value> = vals.iter().flatten().copied().collect();
    let any = !present.is_empty();

    if any && present.iter().all(|v| v.is_i64() || v.is_u64()) {
        let v = vals
            .iter()
            .map(|o| o.and_then(|v| v.as_i64().or_else(|| v.as_u64().map(|u| u as i64))))
            .collect();
        return (ColumnDtype::I64, ColumnData::I64(v));
    }
    if any && present.iter().all(|v| v.is_number()) {
        let v = vals.iter().map(|o| o.and_then(|v| v.as_f64())).collect();
        return (ColumnDtype::F64, ColumnData::F64(v));
    }
    if any && present.iter().all(|v| v.is_boolean()) {
        let v = vals.iter().map(|o| o.and_then(|v| v.as_bool())).collect();
        return (ColumnDtype::Bool, ColumnData::Bool(v));
    }
    let v = vals
        .iter()
        .map(|o| {
            o.map(|v| match v {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            })
        })
        .collect();
    (ColumnDtype::Str, ColumnData::Str(v))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_infers_types_units_and_nulls() {
        let csv = "t (s),count,ok,label\n0.0,10,true,a\n0.5,20,false,\n,30,true,c\n";
        let frame = frame_from_csv(csv.as_bytes()).unwrap();
        assert_eq!(frame.n_cols(), 4);
        assert_eq!(frame.n_rows(), 3);

        let t = frame.specs().find(|s| s.name == "t").unwrap();
        assert_eq!(t.dtype, ColumnDtype::F64);
        assert_eq!(t.unit.as_deref(), Some("s"), "unit parsed from 't (s)'");
        assert_eq!(
            frame.specs().find(|s| s.name == "count").unwrap().dtype,
            ColumnDtype::I64
        );
        assert_eq!(
            frame.specs().find(|s| s.name == "ok").unwrap().dtype,
            ColumnDtype::Bool
        );

        match frame.column("t").unwrap() {
            ColumnData::F64(v) => assert_eq!(v[2], None, "empty cell → null"),
            _ => panic!("t should be F64"),
        }
        match frame.column("label").unwrap() {
            ColumnData::Str(v) => {
                assert_eq!(v[0].as_deref(), Some("a"));
                assert_eq!(v[1], None);
            }
            _ => panic!("label should be Str"),
        }
    }

    #[test]
    fn jsonl_infers_columns_and_missing_fields_are_null() {
        let jsonl = concat!(
            "{\"t\":0.0,\"n\":1,\"ok\":true,\"name\":\"a\"}\n",
            "{\"t\":0.5,\"n\":2,\"ok\":false,\"name\":\"b\"}\n",
            "{\"t\":1.0,\"n\":3}\n"
        );
        let frame = frame_from_jsonl(jsonl.as_bytes()).unwrap();
        assert_eq!(frame.n_rows(), 3);
        assert_eq!(frame.n_cols(), 4);
        assert_eq!(
            frame.specs().find(|s| s.name == "n").unwrap().dtype,
            ColumnDtype::I64
        );
        assert_eq!(
            frame.specs().find(|s| s.name == "t").unwrap().dtype,
            ColumnDtype::F64
        );
        match frame.column("ok").unwrap() {
            ColumnData::Bool(v) => assert_eq!(v[2], None, "missing field → null"),
            _ => panic!("ok should be Bool"),
        }
    }
}
