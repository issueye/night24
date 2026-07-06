use std::fs;

use super::super::helpers::*;
use crate::object::{new_error, str_obj, CallContext, Object};

pub(crate) fn csv_module() -> Object {
    module(vec![
        ("parse", native("csv.parse", csv_parse)),
        ("stringify", native("csv.stringify", csv_stringify)),
        (
            "readFileSync",
            native("csv.readFileSync", csv_read_file_sync),
        ),
        (
            "writeFileSync",
            native("csv.writeFileSync", csv_write_file_sync),
        ),
    ])
}

#[derive(Clone)]
pub(crate) struct CsvOptions {
    header: bool,
    comma: char,
    comment: Option<char>,
    fields_per_record: i64,
    trim_leading_space: bool,
}

pub(crate) fn csv_options(
    ctx: &CallContext,
    name: &str,
    value: Option<&Object>,
) -> Result<CsvOptions, Object> {
    let mut opts = CsvOptions {
        header: true,
        comma: ',',
        comment: None,
        fields_per_record: 0,
        trim_leading_space: false,
    };
    let Some(value) = value else {
        return Ok(opts);
    };
    if matches!(value, Object::Undefined | Object::Null) {
        return Ok(opts);
    }
    let Object::Hash(hash) = value else {
        return Err(new_error(
            ctx.pos.clone(),
            format!("{}: options must be an object", name),
        ));
    };
    let hash = hash.borrow();
    if let Some(value) = hash.get("header") {
        match value {
            Object::Boolean(b) => opts.header = *b,
            _ => {
                return Err(new_error(
                    ctx.pos.clone(),
                    format!("{}: options.header must be a boolean", name),
                ))
            }
        }
    }
    if let Some(value) = hash.get("comma") {
        opts.comma = csv_single_char(ctx, name, "comma", value)?;
    }
    if let Some(value) = hash.get("comment") {
        opts.comment = Some(csv_single_char(ctx, name, "comment", value)?);
    }
    if let Some(value) = hash.get("fieldsPerRecord") {
        match value {
            Object::Number(n) => opts.fields_per_record = *n as i64,
            _ => {
                return Err(new_error(
                    ctx.pos.clone(),
                    format!("{}: options.fieldsPerRecord must be a number", name),
                ))
            }
        }
    }
    if let Some(value) = hash.get("trimLeadingSpace") {
        match value {
            Object::Boolean(b) => opts.trim_leading_space = *b,
            _ => {
                return Err(new_error(
                    ctx.pos.clone(),
                    format!("{}: options.trimLeadingSpace must be a boolean", name),
                ))
            }
        }
    }
    Ok(opts)
}

pub(crate) fn csv_single_char(
    ctx: &CallContext,
    name: &str,
    option: &str,
    value: &Object,
) -> Result<char, Object> {
    let Object::String(s) = value else {
        return Err(new_error(
            ctx.pos.clone(),
            format!("{}: options.{} must be a string", name, option),
        ));
    };
    let mut chars = s.chars();
    let Some(ch) = chars.next() else {
        return Err(new_error(
            ctx.pos.clone(),
            format!("{}: options.{} must be a single character", name, option),
        ));
    };
    if chars.next().is_some() {
        return Err(new_error(
            ctx.pos.clone(),
            format!("{}: options.{} must be a single character", name, option),
        ));
    }
    Ok(ch)
}

pub(crate) fn csv_parse(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "csv.parse", args);
    let text = match reader.required_string(0, "text") {
        Ok(text) => text,
        Err(err) => return err,
    };
    let opts = match csv_options(ctx, "csv.parse", args.get(1)) {
        Ok(opts) => opts,
        Err(err) => return err,
    };
    let records = match parse_csv_records(&text, &opts) {
        Ok(records) => records,
        Err(e) => return new_error(ctx.pos.clone(), format!("csv.parse: {}", e)),
    };
    csv_records_to_object(records, opts.header)
}

pub(crate) fn parse_csv_records(text: &str, opts: &CsvOptions) -> Result<Vec<Vec<String>>, String> {
    let mut records = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut chars = text.chars().peekable();
    let mut in_quotes = false;
    let mut at_line_start = true;
    let mut at_field_start = true;
    while let Some(ch) = chars.next() {
        if at_line_start && !in_quotes && opts.comment == Some(ch) {
            for next in chars.by_ref() {
                if next == '\n' {
                    break;
                }
            }
            at_line_start = true;
            at_field_start = true;
            continue;
        }
        if in_quotes {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(ch);
            }
            continue;
        }
        if at_field_start && opts.trim_leading_space && ch == ' ' {
            continue;
        }
        if at_field_start && ch == '"' {
            in_quotes = true;
            at_field_start = false;
            at_line_start = false;
            continue;
        }
        if ch == opts.comma {
            row.push(field);
            field = String::new();
            at_field_start = true;
            at_line_start = false;
            continue;
        }
        if ch == '\n' || ch == '\r' {
            if ch == '\r' && chars.peek() == Some(&'\n') {
                chars.next();
            }
            row.push(field);
            field = String::new();
            csv_check_record_len(&row, opts.fields_per_record)?;
            records.push(row);
            row = Vec::new();
            at_line_start = true;
            at_field_start = true;
            continue;
        }
        field.push(ch);
        at_field_start = false;
        at_line_start = false;
    }
    if in_quotes {
        return Err("unterminated quoted field".into());
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        csv_check_record_len(&row, opts.fields_per_record)?;
        records.push(row);
    }
    Ok(records)
}

pub(crate) fn csv_check_record_len(row: &[String], fields_per_record: i64) -> Result<(), String> {
    if fields_per_record > 0 && row.len() as i64 != fields_per_record {
        Err(format!(
            "wrong number of fields: expected {}, got {}",
            fields_per_record,
            row.len()
        ))
    } else {
        Ok(())
    }
}

pub(crate) fn csv_records_to_object(records: Vec<Vec<String>>, header: bool) -> Object {
    if !header {
        return array(
            records
                .into_iter()
                .map(|row| array(row.into_iter().map(str_obj).collect()))
                .collect(),
        );
    }
    let Some(headers) = records.first() else {
        return array(Vec::new());
    };
    let mut rows = Vec::new();
    for record in records.iter().skip(1) {
        let mut row = ObjectBuilder::new();
        for (idx, key) in headers.iter().enumerate() {
            row.insert(
                key.clone(),
                str_obj(record.get(idx).cloned().unwrap_or_default()),
            );
        }
        rows.push(row.build());
    }
    array(rows)
}

pub(crate) fn csv_stringify(ctx: &mut CallContext, args: &[Object]) -> Object {
    let Some(rows) = args.first() else {
        return new_error(ctx.pos.clone(), "csv.stringify requires rows");
    };
    let opts = match csv_options(ctx, "csv.stringify", args.get(1)) {
        Ok(opts) => opts,
        Err(err) => return err,
    };
    let records = match csv_rows_from_object(ctx, "csv.stringify", rows, opts.header) {
        Ok(records) => records,
        Err(err) => return err,
    };
    str_obj(write_csv_records(&records, opts.comma))
}

pub(crate) fn csv_rows_from_object(
    ctx: &mut CallContext,
    name: &str,
    rows: &Object,
    header: bool,
) -> Result<Vec<Vec<String>>, Object> {
    let Object::Array(arr) = rows else {
        return Err(new_error(
            ctx.pos.clone(),
            format!("{}: rows must be an array", name),
        ));
    };
    let rows = arr.borrow();
    if rows.elements.is_empty() {
        return Ok(Vec::new());
    }
    if matches!(rows.elements.first(), Some(Object::Array(_))) {
        let mut out = Vec::new();
        for row in &rows.elements {
            let Object::Array(arr) = row else {
                return Err(new_error(
                    ctx.pos.clone(),
                    format!("{}: rows must be all arrays or all objects", name),
                ));
            };
            out.push(arr.borrow().elements.iter().map(object_to_text).collect());
        }
        return Ok(out);
    }
    let mut headers = Vec::<String>::new();
    for row in &rows.elements {
        if let Object::Hash(hash) = row {
            for (key, _) in &hash.borrow().entries {
                if !headers.contains(key) {
                    headers.push(key.clone());
                }
            }
        }
    }
    headers.sort();
    let mut out = Vec::new();
    if header {
        out.push(headers.clone());
    }
    for (idx, row) in rows.elements.iter().enumerate() {
        let Object::Hash(hash) = row else {
            return Err(new_error(
                ctx.pos.clone(),
                format!("{}: row {} must be an object", name, idx),
            ));
        };
        let hash = hash.borrow();
        out.push(
            headers
                .iter()
                .map(|key| hash.get(key).map(object_to_text).unwrap_or_default())
                .collect(),
        );
    }
    Ok(out)
}

pub(crate) fn write_csv_records(records: &[Vec<String>], comma: char) -> String {
    let mut out = String::new();
    for row in records {
        for (idx, field) in row.iter().enumerate() {
            if idx > 0 {
                out.push(comma);
            }
            out.push_str(&csv_escape_field(field, comma));
        }
        out.push('\n');
    }
    out
}

pub(crate) fn csv_escape_field(field: &str, comma: char) -> String {
    if field.contains(comma) || field.contains('"') || field.contains('\n') || field.contains('\r')
    {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_string()
    }
}

pub(crate) fn csv_read_file_sync(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "csv.readFileSync", args);
    let path = match reader.required_string(0, "path") {
        Ok(path) => path,
        Err(err) => return err,
    };
    match fs::read_to_string(&path) {
        Ok(text) => {
            let opts = args.get(1).cloned();
            let parse_args = match opts {
                Some(opts) => vec![str_obj(text), opts],
                None => vec![str_obj(text)],
            };
            csv_parse(ctx, &parse_args)
        }
        Err(e) => new_error(ctx.pos.clone(), format!("csv.readFileSync: {}", e)),
    }
}

pub(crate) fn csv_write_file_sync(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "csv.writeFileSync", args);
    let path = match reader.required_string(0, "path") {
        Ok(path) => path,
        Err(err) => return err,
    };
    let Some(rows) = args.get(1) else {
        return new_error(ctx.pos.clone(), "csv.writeFileSync requires rows");
    };
    let opts = args.get(2).cloned();
    let stringify_args = opts.map_or_else(|| vec![rows.clone()], |opts| vec![rows.clone(), opts]);
    let text = csv_stringify(ctx, &stringify_args);
    if matches!(text, Object::Error(_)) {
        return text;
    }
    match fs::write(&path, object_to_text(&text)) {
        Ok(_) => Object::Undefined,
        Err(e) => new_error(ctx.pos.clone(), format!("csv.writeFileSync: {}", e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_options() -> CsvOptions {
        CsvOptions {
            header: true,
            comma: ',',
            comment: None,
            fields_per_record: 0,
            trim_leading_space: false,
        }
    }

    #[test]
    fn parse_csv_records_handles_quoted_fields() {
        let records = parse_csv_records(
            "name,note\nAlice,\"hello, \"\"night\"\"\"\n",
            &default_options(),
        )
        .unwrap();

        assert_eq!(
            records,
            vec![
                vec!["name".to_string(), "note".to_string()],
                vec!["Alice".to_string(), "hello, \"night\"".to_string()],
            ]
        );
    }

    #[test]
    fn write_csv_records_escapes_quoted_fields() {
        let records = vec![vec![
            "name".to_string(),
            "hello, \"night\"".to_string(),
            "line\nbreak".to_string(),
        ]];

        assert_eq!(
            write_csv_records(&records, ','),
            "name,\"hello, \"\"night\"\"\",\"line\nbreak\"\n"
        );
    }
}

// ---------------------------------------------------------------------------
// template: focused Go-template-like interpolation and common funcs.
// ---------------------------------------------------------------------------
