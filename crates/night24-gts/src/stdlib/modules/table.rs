use super::super::helpers::*;
use crate::object::{new_error, str_obj, CallContext, Object};

pub(crate) fn table_module() -> Object {
    module(vec![("render", native("table.render", table_render))])
}

pub(crate) fn table_render(ctx: &mut CallContext, args: &[Object]) -> Object {
    let Some(rows_obj) = args.first() else {
        return new_error(ctx.pos.clone(), "table.render requires rows");
    };
    let Object::Array(rows) = rows_obj else {
        return new_error(ctx.pos.clone(), "table.render: rows must be an array");
    };
    let rows_ref = rows.borrow();
    let mut headers = table_headers(args.get(1));
    if headers.is_empty() {
        headers = infer_table_headers(&rows_ref.elements);
    }
    let mut matrix = Vec::new();
    if !headers.is_empty() {
        matrix.push(headers.clone());
    }
    for row in &rows_ref.elements {
        matrix.push(table_row_cells(row, &headers));
    }
    str_obj(render_ascii_table(&matrix, !headers.is_empty()))
}

pub(crate) fn table_headers(value: Option<&Object>) -> Vec<String> {
    match value {
        Some(Object::Array(arr)) => arr
            .borrow()
            .elements
            .iter()
            .map(object_to_text)
            .collect::<Vec<_>>(),
        Some(Object::Hash(hash)) => match hash.borrow().get("headers") {
            Some(Object::Array(arr)) => arr
                .borrow()
                .elements
                .iter()
                .map(object_to_text)
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

pub(crate) fn infer_table_headers(rows: &[Object]) -> Vec<String> {
    for row in rows {
        if let Object::Hash(hash) = row {
            return hash
                .borrow()
                .entries
                .iter()
                .map(|(key, _)| key.clone())
                .collect();
        }
    }
    Vec::new()
}

pub(crate) fn table_row_cells(row: &Object, headers: &[String]) -> Vec<String> {
    match row {
        Object::Array(arr) => arr.borrow().elements.iter().map(object_to_text).collect(),
        Object::Hash(hash) if !headers.is_empty() => {
            let hash = hash.borrow();
            headers
                .iter()
                .map(|key| hash.get(key).map(object_to_text).unwrap_or_default())
                .collect()
        }
        _ => vec![object_to_text(row)],
    }
}

pub(crate) fn render_ascii_table(rows: &[Vec<String>], has_header: bool) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    let mut widths = vec![0usize; columns];
    for row in rows {
        for (idx, cell) in row.iter().enumerate() {
            widths[idx] = widths[idx].max(strip_ansi(cell).chars().count());
        }
    }
    let border = table_border(&widths);
    let mut out = String::new();
    out.push_str(&border);
    out.push('\n');
    for (idx, row) in rows.iter().enumerate() {
        out.push_str(&table_row(row, &widths));
        out.push('\n');
        if idx == 0 && has_header {
            out.push_str(&border);
            out.push('\n');
        }
    }
    out.push_str(&border);
    out
}

pub(crate) fn table_border(widths: &[usize]) -> String {
    let mut out = String::from("+");
    for width in widths {
        out.push_str(&"-".repeat(*width + 2));
        out.push('+');
    }
    out
}

pub(crate) fn table_row(row: &[String], widths: &[usize]) -> String {
    let mut out = String::from("|");
    for (idx, width) in widths.iter().enumerate() {
        let cell = row.get(idx).cloned().unwrap_or_default();
        let pad = width.saturating_sub(strip_ansi(&cell).chars().count());
        out.push(' ');
        out.push_str(&cell);
        out.push_str(&" ".repeat(pad + 1));
        out.push('|');
    }
    out
}

// ---------------------------------------------------------------------------
// validation: small schema validator plus common predicate helpers.
// ---------------------------------------------------------------------------
