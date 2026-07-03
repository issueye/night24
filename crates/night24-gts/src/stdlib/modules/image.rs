use super::super::helpers::*;
use crate::object::{new_error, CallContext, Object};

pub(crate) fn image_module() -> Object {
    module(vec![("info", native("image.info", image_info))])
}

pub(crate) fn image_info(ctx: &mut CallContext, args: &[Object]) -> Object {
    match required_string(ctx, "image.info", args, 0, "path") {
        Ok(_path) => new_error(
            ctx.pos.clone(),
            "image module: basic placeholder - full implementation requires external library",
        ),
        Err(e) => e,
    }
}
