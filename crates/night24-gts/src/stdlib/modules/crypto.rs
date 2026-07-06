use super::super::helpers::*;
use crate::object::{bool_obj, new_error, num_obj, str_obj, CallContext, Object};

pub(crate) fn crypto_module() -> Object {
    module(vec![
        (
            "randomUUID",
            native("crypto.randomUUID", crypto_random_uuid),
        ),
        ("sha1", native("crypto.sha1", crypto_sha1)),
        ("sha256", native("crypto.sha256", crypto_sha256)),
        ("sha512", native("crypto.sha512", crypto_sha512)),
        ("hmac", native("crypto.hmac", crypto_hmac)),
        ("pbkdf2", native("crypto.pbkdf2", crypto_pbkdf2)),
        (
            "randomBytes",
            native("crypto.randomBytes", crypto_random_bytes),
        ),
        (
            "timingSafeEqual",
            native("crypto.timingSafeEqual", crypto_timing_safe_equal),
        ),
    ])
}

pub(crate) fn crypto_sha1(ctx: &mut CallContext, args: &[Object]) -> Object {
    match crypto_input(ctx, "crypto.sha1", args) {
        Ok(bytes) => str_obj(hex_encode_bytes(&sha1(&bytes))),
        Err(err) => err,
    }
}

pub(crate) fn crypto_sha256(ctx: &mut CallContext, args: &[Object]) -> Object {
    match crypto_input(ctx, "crypto.sha256", args) {
        Ok(bytes) => str_obj(hex_encode_bytes(&sha256(&bytes))),
        Err(err) => err,
    }
}

pub(crate) fn crypto_sha512(ctx: &mut CallContext, args: &[Object]) -> Object {
    match crypto_input(ctx, "crypto.sha512", args) {
        Ok(bytes) => str_obj(hex_encode_bytes(&sha512(&bytes))),
        Err(err) => err,
    }
}

pub(crate) fn crypto_input(
    ctx: &mut CallContext,
    name: &str,
    args: &[Object],
) -> Result<Vec<u8>, Object> {
    match args.first() {
        Some(value) => bytes_from_object(ctx, name, value),
        None => Err(new_error(
            ctx.pos.clone(),
            format!("{} requires value", name),
        )),
    }
}

pub(crate) fn crypto_hmac(ctx: &mut CallContext, args: &[Object]) -> Object {
    if args.len() < 3 {
        return new_error(
            ctx.pos.clone(),
            "crypto.hmac requires algorithm, key and value",
        );
    }
    let reader = ArgReader::new(ctx, "crypto.hmac", args);
    let algorithm = match reader.required_string(0, "algorithm") {
        Ok(value) => value,
        Err(err) => return err,
    };
    let key = match bytes_from_object(ctx, "crypto.hmac", &args[1]) {
        Ok(b) => b,
        Err(err) => return err,
    };
    let value = match bytes_from_object(ctx, "crypto.hmac", &args[2]) {
        Ok(b) => b,
        Err(err) => return err,
    };
    match hash_kind(&algorithm) {
        Some(kind) => str_obj(hex_encode_bytes(&hmac(kind, &key, &value))),
        None => new_error(
            ctx.pos.clone(),
            format!("crypto.hmac: unsupported hash algorithm {:?}", algorithm),
        ),
    }
}

pub(crate) fn crypto_pbkdf2(ctx: &mut CallContext, args: &[Object]) -> Object {
    if args.len() < 4 {
        return new_error(
            ctx.pos.clone(),
            "crypto.pbkdf2 requires password, salt, iterations and keyLength",
        );
    }
    let password = match bytes_from_object(ctx, "crypto.pbkdf2", &args[0]) {
        Ok(b) => b,
        Err(err) => return err,
    };
    let salt = match bytes_from_object(ctx, "crypto.pbkdf2", &args[1]) {
        Ok(b) => b,
        Err(err) => return err,
    };
    let iterations = match required_positive_int(ctx, "crypto.pbkdf2", &args[2], "iterations") {
        Ok(n) => n,
        Err(err) => return err,
    };
    let key_length = match required_positive_int(ctx, "crypto.pbkdf2", &args[3], "keyLength") {
        Ok(n) => n,
        Err(err) => return err,
    };
    let algorithm = match args.get(4) {
        Some(Object::String(s)) => s.as_str().to_string(),
        // Default per Go original.
        _ => "sha256".to_string(),
    };
    let kind = match hash_kind(&algorithm) {
        Some(kind) => kind,
        None => {
            return new_error(
                ctx.pos.clone(),
                format!("crypto.pbkdf2: unsupported hash algorithm {:?}", algorithm),
            )
        }
    };
    let derived = pbkdf2(kind, &password, &salt, iterations as u32, key_length);
    // pbkdf2 defaults to a lowercase hex string (matching the Go original's
    // hex.EncodeToString); only {asBuffer:true} returns a Buffer.
    let as_buffer = match args.get(5) {
        Some(Object::Hash(opts)) => ObjectView::new(&opts.borrow())
            .bool("asBuffer")
            .unwrap_or(false),
        _ => false,
    };
    if as_buffer {
        make_buffer(derived)
    } else {
        str_obj(hex_encode_bytes(&derived))
    }
}

pub(crate) fn crypto_random_uuid(ctx: &mut CallContext, _args: &[Object]) -> Object {
    let mut buf = [0u8; 16];
    if let Err(err) = fill_random(ctx, "crypto.randomUUID", &mut buf) {
        return err;
    }
    buf[6] = (buf[6] & 0x0f) | 0x40;
    buf[8] = (buf[8] & 0x3f) | 0x80;
    str_obj(format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7], buf[8], buf[9], buf[10], buf[11], buf[12], buf[13], buf[14], buf[15]
    ))
}

pub(crate) fn crypto_random_bytes(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "crypto.randomBytes", args);
    let size = match reader.required_number(0, "size") {
        Ok(v) => v,
        Err(e) => return e,
    };
    if size < 0.0 {
        return new_error(
            ctx.pos.clone(),
            "crypto.randomBytes: size must be non-negative",
        );
    }
    if size > 1_048_576.0 {
        return new_error(
            ctx.pos.clone(),
            "crypto.randomBytes: size must be <= 1048576",
        );
    }
    let mut buf = vec![0u8; size as usize];
    if let Err(err) = fill_random(ctx, "crypto.randomBytes", &mut buf) {
        return err;
    }
    array(buf.into_iter().map(|b| num_obj(b as f64)).collect())
}

pub(crate) fn crypto_timing_safe_equal(ctx: &mut CallContext, args: &[Object]) -> Object {
    if args.len() < 2 {
        return new_error(
            ctx.pos.clone(),
            "crypto.timingSafeEqual requires left and right",
        );
    }
    let left = match bytes_from_object(ctx, "crypto.timingSafeEqual", &args[0]) {
        Ok(b) => b,
        Err(err) => return err,
    };
    let right = match bytes_from_object(ctx, "crypto.timingSafeEqual", &args[1]) {
        Ok(b) => b,
        Err(err) => return err,
    };
    if left.len() != right.len() {
        return bool_obj(false);
    }
    // Constant-time compare.
    let mut diff: u8 = 0;
    for (a, b) in left.iter().zip(right.iter()) {
        diff |= a ^ b;
    }
    bool_obj(diff == 0)
}
