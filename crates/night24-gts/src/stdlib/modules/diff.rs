use super::super::helpers::*;
use crate::object::{str_obj, CallContext, Object};

pub(crate) fn diff_module() -> Object {
    module(vec![
        ("lines", native("diff.lines", diff_lines)),
        ("unified", native("diff.unified", diff_unified)),
    ])
}

pub(crate) fn diff_lines(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "diff.lines", args);
    let old = match reader.required_string(0, "old") {
        Ok(value) => value,
        Err(err) => return err,
    };
    let new = match reader.required_string(1, "new") {
        Ok(value) => value,
        Err(err) => return err,
    };
    array(
        line_diff(&old, &new)
            .into_iter()
            .map(|entry| {
                module(vec![
                    ("kind", str_obj(entry.kind)),
                    ("value", str_obj(entry.value)),
                ])
            })
            .collect(),
    )
}

pub(crate) fn diff_unified(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "diff.unified", args);
    let old = match reader.required_string(0, "old") {
        Ok(value) => value,
        Err(err) => return err,
    };
    let new = match reader.required_string(1, "new") {
        Ok(value) => value,
        Err(err) => return err,
    };
    let old_name = match args.get(2) {
        Some(Object::String(value)) => value.to_string(),
        _ => "old".to_string(),
    };
    let new_name = match args.get(3) {
        Some(Object::String(value)) => value.to_string(),
        _ => "new".to_string(),
    };
    let mut out = format!("--- {}\n+++ {}\n", old_name, new_name);
    for entry in line_diff(&old, &new) {
        let prefix = match entry.kind.as_str() {
            "add" => '+',
            "remove" => '-',
            _ => ' ',
        };
        out.push(prefix);
        out.push_str(&entry.value);
        out.push('\n');
    }
    str_obj(out)
}

pub(crate) struct LineDiffEntry {
    kind: String,
    value: String,
}

pub(crate) fn line_diff(old: &str, new: &str) -> Vec<LineDiffEntry> {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();
    let mut lcs = vec![vec![0usize; new_lines.len() + 1]; old_lines.len() + 1];
    for i in (0..old_lines.len()).rev() {
        for j in (0..new_lines.len()).rev() {
            lcs[i][j] = if old_lines[i] == new_lines[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }

    let mut out = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    while i < old_lines.len() && j < new_lines.len() {
        if old_lines[i] == new_lines[j] {
            out.push(LineDiffEntry {
                kind: "equal".to_string(),
                value: old_lines[i].to_string(),
            });
            i += 1;
            j += 1;
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            out.push(LineDiffEntry {
                kind: "remove".to_string(),
                value: old_lines[i].to_string(),
            });
            i += 1;
        } else {
            out.push(LineDiffEntry {
                kind: "add".to_string(),
                value: new_lines[j].to_string(),
            });
            j += 1;
        }
    }
    while i < old_lines.len() {
        out.push(LineDiffEntry {
            kind: "remove".to_string(),
            value: old_lines[i].to_string(),
        });
        i += 1;
    }
    while j < new_lines.len() {
        out.push(LineDiffEntry {
            kind: "add".to_string(),
            value: new_lines[j].to_string(),
        });
        j += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// log: deterministic level formatting without side effects.
// ---------------------------------------------------------------------------
