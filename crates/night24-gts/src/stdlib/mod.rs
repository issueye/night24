//! Native standard library modules (`@std/*`).
//!
//! Each `@std/*` module lives in its own file under [`modules`], mirroring the
//! original Go layout in `gts/internal/stdlib/*.go`. Cross-cutting helpers
//! (argument coercion, buffers, json/glob primitives, ...) live in [`helpers`].
//! This file is now only the dispatch table that maps a module specifier to its
//! constructor plus the small public API consumed elsewhere in the crate.

pub mod gtp;
pub mod helpers;
pub mod modules;

use crate::object::Object;

// Re-export the public API that other crates/modules reach via
// `crate::stdlib::<name>` (kept stable during the monolith split).
pub(crate) use helpers::set_runtime_argv;
pub(crate) use modules::time::{format_epoch_ms_utc, ms_from_utc_parts, utc_parts_from_ms};

/// Load a native `@std/*` module by specifier.
pub fn load_native_module(spec: &str) -> Option<Object> {
    match spec {
        "@std/path" => Some(modules::path::path_module()),
        "@std/os" => Some(modules::os::os_module()),
        "@std/env" => Some(modules::env::env_module()),
        "@std/fs" => Some(modules::fs::fs_module()),
        "@std/json" => Some(modules::json::json_module()),
        "@std/time" => Some(modules::time::time_module()),
        "@std/encoding/base64" => Some(modules::encoding_base64::base64_module()),
        "@std/encoding/hex" => Some(modules::encoding_hex::hex_module()),
        "@std/hash" => Some(modules::hash::hash_module()),
        "@std/crypto" => Some(modules::crypto::crypto_module()),
        "@std/random" => Some(modules::random::random_module()),
        "@std/regexp" => Some(modules::regexp::regexp_module()),
        "@std/semver" => Some(modules::semver::semver_module()),
        "@std/collections" => Some(modules::collections::collections_module()),
        "@std/process" => Some(modules::process::process_module()),
        "@std/text" => Some(modules::text::text_module()),
        "@std/url" => Some(modules::url::url_module()),
        "@std/cache" => Some(modules::cache::cache_module()),
        "@std/timers" => Some(modules::timers::timers_module()),
        "@std/glob" => Some(modules::glob::glob_module()),
        "@std/color" => Some(modules::color::color_module()),
        "@std/diff" => Some(modules::diff::diff_module()),
        "@std/log" => Some(modules::log::log_module()),
        "@std/table" => Some(modules::table::table_module()),
        "@std/validation" => Some(modules::validation::validation_module()),
        "@std/encoding/csv" => Some(modules::encoding_csv::csv_module()),
        "@std/template" => Some(modules::template::template_module()),
        "@std/compression" => Some(modules::compression::compression_module()),
        "@std/compress/gzip" => Some(modules::compress_gzip::gzip_module()),
        "@std/terminal" => Some(modules::terminal::terminal_module()),
        "@std/cli" => Some(modules::cli::cli_module()),
        "@std/tui" => Some(modules::tui::tui_module()),
        "@std/toml" => Some(modules::toml::toml_module()),
        "@std/yaml" => Some(modules::yaml::yaml_module()),
        "@std/xml" => Some(modules::xml::xml_module()),
        "@std/markdown" => Some(modules::markdown::markdown_module()),
        "@std/schema" => Some(modules::schema::schema_module()),
        "@std/test" => Some(modules::test::test_module()),
        "@std/archive/zip" => Some(modules::archive_zip::archive_zip_module()),
        "@std/buffer" => Some(modules::buffer::buffer_module()),
        "@std/events" => Some(modules::events::events_module()),
        "@std/jwt" => Some(modules::jwt::jwt_module()),
        "@std/mime" => Some(modules::mime::mime_module()),
        "@std/net/ip" => Some(modules::net_ip::net_ip_module()),
        "@std/retry" => Some(modules::retry::retry_module()),
        "@std/stream" => Some(modules::stream::stream_module()),
        "@std/exec" => Some(modules::exec::exec_module()),
        "@std/http" | "@std/net/http/client" => {
            Some(modules::net_http_client::http_client_module())
        }
        "@std/rate-limit" => Some(modules::rate_limit::rate_limit_module()),
        "@std/prometheus" => Some(modules::prometheus::prometheus_module()),
        "@std/highlight" => Some(modules::highlight::highlight_module()),
        "@std/sse" => Some(modules::sse::sse_module()),
        #[cfg(feature = "db")]
        "@std/db" => Some(modules::db::db_module()),
        "@std/mail" => Some(modules::mail::mail_module()),
        "@std/net/socket/client" => Some(modules::net_socket_client::socket_client_module()),
        "@std/net/socket/server" => Some(modules::net_socket_server::socket_server_module()),
        "@std/runtime" => Some(modules::runtime::runtime_module()),
        "@std/image" => Some(modules::image::image_module()),
        "@std/pdf" => Some(modules::pdf::pdf_module()),
        "@std/net/ws/client" => Some(modules::net_ws_client::ws_client_module()),
        "@std/net/ws/server" => Some(modules::net_ws_server::ws_server_module()),
        "@std/net/http/server" => Some(modules::net_http_server::http_server_module()),
        "@std/web" => Some(modules::web::web_module()),
        "@std/express" => Some(modules::web::web_module()),
        "@std/signal" => Some(modules::signal::signal_module()),
        "@std/watch" => Some(modules::watch::watch_module()),
        "@std/async" => Some(modules::async_::async_module()),
        "@std/pty" => Some(modules::pty::pty_module()),

        // GTP modules - delegate to gtp submodule
        spec if spec.starts_with("@std/gtp/") => gtp::load_gtp_module(spec),

        _ => None,
    }
}
