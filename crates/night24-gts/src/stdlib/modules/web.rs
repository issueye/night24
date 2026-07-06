use super::super::helpers::*;
use crate::object::Object;

mod app;
mod helpers;
mod request;
mod response;
mod routing;
mod workers;
mod ws;

use app::web_create_app;
use app::WebApp;
use helpers::{web_json_helper, web_static_helper, web_text_helper};

pub(crate) fn web_module() -> Object {
    module(vec![
        ("createApp", native("web.createApp", web_create_app)),
        ("json", native("web.json", web_json_helper)),
        ("text", native("web.text", web_text_helper)),
        ("static", native("web.static", web_static_helper)),
    ])
}

// ============================================================================
// @std/signal - OS signal handling
// ============================================================================
