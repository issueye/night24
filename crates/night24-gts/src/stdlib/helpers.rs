//! Shared helpers for the native `@std/*` standard library modules.
//!
//! These are the common argument-coercion, buffer, json and glob primitives
//! reused across many `modules::*` files. Extracted from the original
//! monolithic `stdlib/mod.rs`.

#![allow(dead_code)]
use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf, MAIN_SEPARATOR_STR};
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

#[allow(unused_imports)]
use std::process::Command;
#[allow(unused_imports)]
use std::process::Stdio;

#[allow(unused_imports)]
use regex::Regex;

#[allow(unused_imports)]
use crate::ast::Position;
#[allow(unused_imports)]
use crate::object::{
    bool_obj, format_number, new_error, num_obj, str_obj, strict_equal, ArrayData, Builtin,
    CallContext, EnvRef, HashData, Object,
};
#[allow(unused_imports)]
use crate::VERSION;

mod args;
mod buffer;
mod codec;
mod core;
mod crypto;
mod encoding;
mod glob;
mod http;
mod json;
mod logic;
mod mime;
mod net;
mod object_builder;
mod path;
mod random;
mod runtime;
mod serde_value;
mod signal;
mod terminal;
mod text;

pub(crate) use args::*;
pub(crate) use buffer::*;
pub(crate) use codec::*;
pub(crate) use core::*;
pub(crate) use crypto::*;
pub(crate) use encoding::*;
pub(crate) use glob::*;
pub(crate) use http::*;
pub(crate) use json::*;
pub(crate) use logic::*;
pub(crate) use mime::*;
pub(crate) use net::*;
pub(crate) use object_builder::*;
pub(crate) use path::*;
pub(crate) use random::*;
pub(crate) use runtime::*;
pub(crate) use serde_value::*;
pub(crate) use signal::*;
pub(crate) use terminal::*;
pub(crate) use text::*;
