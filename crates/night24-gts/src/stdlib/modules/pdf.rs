use super::super::helpers::*;
use crate::object::{new_error, CallContext, Object};

pub(crate) fn pdf_module() -> Object {
    module(vec![("info", native("pdf.info", pdf_info))])
}

pub(crate) fn pdf_info(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "pdf.info", args);
    match reader.required_string(0, "path") {
        Ok(_path) => new_error(
            ctx.pos.clone(),
            "pdf module: basic placeholder - full implementation requires external library",
        ),
        Err(e) => e,
    }
}

// ---------------------------------------------------------------------------
// net/ws/client + net/ws/server: WebSocket (RFC 6455) over blocking TCP
// (@std/net/ws/client, @std/net/ws/server)
// ---------------------------------------------------------------------------
