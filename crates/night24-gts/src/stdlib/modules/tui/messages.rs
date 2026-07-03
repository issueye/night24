//! Event-message constructors (key / resize / tick / mouse / raw).
//!
//! Migrated verbatim from `tui_legacy` so the update-loop contract is
//! unchanged. These build the `{type, ...}` hash objects the user's `update`
//! function pattern-matches on.

#![allow(dead_code)]

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::object::{bool_obj, num_obj, str_obj, HashData, Object};

pub fn key_message(name: &str, raw: &str) -> Object {
    let hash = Rc::new(RefCell::new(HashData::default()));
    hash.borrow_mut().set("type", str_obj("key"));
    hash.borrow_mut().set("key", str_obj(name));
    if !raw.is_empty() {
        hash.borrow_mut().set("raw", str_obj(raw));
    }
    Object::Hash(hash)
}

pub fn text_message(value: &str, raw: &str) -> Object {
    let hash = Rc::new(RefCell::new(HashData::default()));
    hash.borrow_mut().set("type", str_obj("text"));
    hash.borrow_mut().set("text", str_obj(value));
    if !raw.is_empty() {
        hash.borrow_mut().set("raw", str_obj(raw));
    }
    Object::Hash(hash)
}

pub fn resize_message(cols: i32, rows: i32, stable: bool) -> Object {
    let hash = Rc::new(RefCell::new(HashData::default()));
    hash.borrow_mut().set("type", str_obj("resize"));
    hash.borrow_mut().set("cols", num_obj(cols as f64));
    hash.borrow_mut().set("rows", num_obj(rows as f64));
    hash.borrow_mut().set("stable", bool_obj(stable));
    Object::Hash(hash)
}

pub fn tick_message() -> Object {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0);
    let hash = Rc::new(RefCell::new(HashData::default()));
    hash.borrow_mut().set("type", str_obj("tick"));
    hash.borrow_mut().set("timeMs", num_obj(ms));
    Object::Hash(hash)
}

pub fn raw_message(raw: String) -> Object {
    let hash = Rc::new(RefCell::new(HashData::default()));
    hash.borrow_mut().set("type", str_obj("raw"));
    hash.borrow_mut().set("raw", str_obj(raw));
    Object::Hash(hash)
}
