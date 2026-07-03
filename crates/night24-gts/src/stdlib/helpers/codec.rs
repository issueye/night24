use super::*;

/// Shared codec file-write helper: stringify `value`, then write to `path`.
/// Returns Undefined on success or an Error; the error prefix mirrors the
/// Go original (it belongs to the stringify step on serialization failure).
pub(crate) fn codec_write_file(
    ctx: &mut CallContext,
    module: &str,
    args: &[Object],
    node_label: &str,
    stringify: fn(&Object) -> Result<String, String>,
) -> Object {
    let path = match required_string(ctx, &format!("{}.writeFileSync", module), args, 0, "path") {
        Ok(p) => p,
        Err(e) => return e,
    };
    let value = match args.get(1) {
        Some(v) => v,
        None => {
            return new_error(
                ctx.pos.clone(),
                format!("{}.writeFileSync requires {}", module, node_label),
            )
        }
    };
    match stringify(value) {
        Ok(text) => match fs::write(&path, text) {
            Ok(()) => Object::Undefined,
            Err(e) => new_error(ctx.pos.clone(), format!("{}.writeFileSync: {}", module, e)),
        },
        Err(msg) => new_error(ctx.pos.clone(), msg),
    }
}

// ---------------------------------------------------------------------------
// toml
// ---------------------------------------------------------------------------
