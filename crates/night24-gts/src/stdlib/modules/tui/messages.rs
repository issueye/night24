//! Event-message constructors (key / resize / tick / mouse / raw).
//!
//! Migrated verbatim from `tui_legacy` so the update-loop contract is
//! unchanged. These build the `{type, ...}` hash objects the user's `update`
//! function pattern-matches on.

#![allow(dead_code)]

use std::time::{SystemTime, UNIX_EPOCH};

use crate::object::{bool_obj, num_obj, str_obj, Object};
use crate::stdlib::helpers::ObjectBuilder;

pub fn key_message(name: &str, raw: &str) -> Object {
    let mut builder = ObjectBuilder::new()
        .set("type", str_obj("key"))
        .set("key", str_obj(name));
    if !raw.is_empty() {
        builder.insert("raw", str_obj(raw));
    }
    builder.build()
}

pub fn text_message(value: &str, raw: &str) -> Object {
    let mut builder = ObjectBuilder::new()
        .set("type", str_obj("text"))
        .set("text", str_obj(value));
    if !raw.is_empty() {
        builder.insert("raw", str_obj(raw));
    }
    builder.build()
}

pub fn resize_message(cols: i32, rows: i32, stable: bool) -> Object {
    ObjectBuilder::new()
        .set("type", str_obj("resize"))
        .set("cols", num_obj(cols as f64))
        .set("rows", num_obj(rows as f64))
        .set("stable", bool_obj(stable))
        .build()
}

pub fn tick_message() -> Object {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0);
    ObjectBuilder::new()
        .set("type", str_obj("tick"))
        .set("timeMs", num_obj(ms))
        .build()
}

pub fn raw_message(raw: String) -> Object {
    ObjectBuilder::new()
        .set("type", str_obj("raw"))
        .set("raw", str_obj(raw))
        .build()
}
