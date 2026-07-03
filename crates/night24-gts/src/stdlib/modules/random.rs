use super::super::helpers::*;
use crate::object::{bool_obj, new_error, num_obj, str_obj, CallContext, Object};

pub(crate) fn random_module() -> Object {
    module(vec![
        ("int", native("random.int", random_int)),
        ("float", native("random.float", random_float)),
        ("bool", native("random.bool", random_bool)),
        ("pick", native("random.pick", random_pick)),
        ("sample", native("random.sample", random_sample)),
        ("shuffle", native("random.shuffle", random_shuffle)),
        ("hex", native("random.hex", random_hex)),
        ("base64", native("random.base64", random_base64)),
        (
            "alphanumeric",
            native("random.alphanumeric", random_alphanumeric),
        ),
        ("alpha", native("random.alpha", random_alpha)),
        ("numeric", native("random.numeric", random_numeric)),
        ("uuid", native("random.uuid", random_uuid)),
        ("uuidv4", native("random.uuid", random_uuid)),
        ("bytes", native("random.bytes", random_bytes)),
    ])
}

pub(crate) fn random_int(ctx: &mut CallContext, args: &[Object]) -> Object {
    let min = match required_number(ctx, "random.int", args, 0, "min") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let max = match required_number(ctx, "random.int", args, 1, "max") {
        Ok(v) => v,
        Err(e) => return e,
    };
    // `partial_cmp ... != Some(Less)` preserves the original `!(min < max)`
    // semantics for NaN (any NaN operand fails the bound check), while avoiding
    // a negated comparison on a partially-ordered type.
    if min.partial_cmp(&max) != Some(std::cmp::Ordering::Less) {
        return new_error(ctx.pos.clone(), "random.int: min must be less than max");
    }
    let span = (max - min) as u64;
    match bounded_random_u64(ctx, "random.int", span) {
        Ok(value) => num_obj(min + (value as f64)),
        Err(err) => err,
    }
}

pub(crate) fn random_float(ctx: &mut CallContext, args: &[Object]) -> Object {
    let min = match required_number(ctx, "random.float", args, 0, "min") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let max = match required_number(ctx, "random.float", args, 1, "max") {
        Ok(v) => v,
        Err(e) => return e,
    };
    // See random.int: preserve the NaN-fails-the-bound-check behaviour.
    if min.partial_cmp(&max) != Some(std::cmp::Ordering::Less) {
        return new_error(ctx.pos.clone(), "random.float: min must be less than max");
    }
    match read_random_u64(ctx, "random.float") {
        Ok(raw) => {
            let frac = raw as f64 / u64::MAX as f64;
            num_obj(min + frac * (max - min))
        }
        Err(err) => err,
    }
}

pub(crate) fn random_bool(ctx: &mut CallContext, _args: &[Object]) -> Object {
    let mut buf = [0u8; 1];
    match fill_random(ctx, "random.bool", &mut buf) {
        Ok(()) => bool_obj(buf[0] & 1 == 1),
        Err(err) => err,
    }
}

pub(crate) fn random_pick(ctx: &mut CallContext, args: &[Object]) -> Object {
    let arr = match args.first() {
        Some(Object::Array(a)) => a.clone(),
        Some(_) => return new_error(ctx.pos.clone(), "random.pick: argument must be an array"),
        None => return new_error(ctx.pos.clone(), "random.pick requires array"),
    };
    let len = arr.borrow().elements.len();
    if len == 0 {
        return Object::Null;
    }
    match bounded_random_u64(ctx, "random.pick", len as u64) {
        Ok(idx) => arr.borrow().elements[idx as usize].clone(),
        Err(err) => err,
    }
}

pub(crate) fn random_sample(ctx: &mut CallContext, args: &[Object]) -> Object {
    let arr = match args.first() {
        Some(Object::Array(a)) => a.clone(),
        Some(_) => {
            return new_error(
                ctx.pos.clone(),
                "random.sample: first argument must be an array",
            )
        }
        None => return new_error(ctx.pos.clone(), "random.sample requires array and count"),
    };
    let count = match required_number(ctx, "random.sample", args, 1, "count") {
        Ok(v) => v,
        Err(e) => return e,
    };
    if count < 0.0 {
        return new_error(ctx.pos.clone(), "random.sample: count must be non-negative");
    }
    let mut elements = arr.borrow().elements.clone();
    let take = (count as usize).min(elements.len());
    // Fisher-Yates partial shuffle over the first `take` positions.
    for i in 0..take {
        let span = (elements.len() - i) as u64;
        match bounded_random_u64(ctx, "random.sample", span) {
            Ok(j) => elements.swap(i, i + j as usize),
            Err(err) => return err,
        }
    }
    elements.truncate(take);
    array(elements)
}

pub(crate) fn random_shuffle(ctx: &mut CallContext, args: &[Object]) -> Object {
    let arr = match args.first() {
        Some(Object::Array(a)) => a.clone(),
        Some(_) => return new_error(ctx.pos.clone(), "random.shuffle: argument must be an array"),
        None => return new_error(ctx.pos.clone(), "random.shuffle requires array"),
    };
    let mut elements = arr.borrow().elements.clone();
    let len = elements.len();
    for i in (1..len).rev() {
        match bounded_random_u64(ctx, "random.shuffle", (i + 1) as u64) {
            Ok(j) => elements.swap(i, j as usize),
            Err(err) => return err,
        }
    }
    array(elements)
}

pub(crate) fn random_length_bounded(
    ctx: &mut CallContext,
    name: &str,
    args: &[Object],
    label: &str,
    max: u32,
) -> Result<u32, Object> {
    match required_number(ctx, name, args, 0, label) {
        Ok(n) => {
            if n < 0.0 || n > max as f64 {
                Err(new_error(
                    ctx.pos.clone(),
                    format!("{}: {} must be in range [0, {}]", name, label, max),
                ))
            } else {
                Ok(n as u32)
            }
        }
        Err(e) => Err(e),
    }
}

pub(crate) fn random_hex(ctx: &mut CallContext, args: &[Object]) -> Object {
    let count = match random_length_bounded(ctx, "random.hex", args, "byte count", 1024) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let mut buf = vec![0u8; count as usize];
    if let Err(err) = fill_random(ctx, "random.hex", &mut buf) {
        return err;
    }
    str_obj(hex_encode_bytes(&buf))
}

pub(crate) fn random_base64(ctx: &mut CallContext, args: &[Object]) -> Object {
    let count = match random_length_bounded(ctx, "random.base64", args, "byte count", 1024) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let mut buf = vec![0u8; count as usize];
    if let Err(err) = fill_random(ctx, "random.base64", &mut buf) {
        return err;
    }
    str_obj(base64_std_encode(&buf))
}

pub(crate) fn random_charset_string(
    ctx: &mut CallContext,
    name: &str,
    args: &[Object],
    charset: &[u8],
) -> Object {
    let length = match random_length_bounded(ctx, name, args, "length", 1024) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let span = charset.len() as u64;
    let mut out = String::with_capacity(length as usize);
    for _ in 0..length {
        match bounded_random_u64(ctx, name, span) {
            Ok(idx) => out.push(charset[idx as usize] as char),
            Err(err) => return err,
        }
    }
    str_obj(out)
}

pub(crate) const ALPHA_NUMERIC: &[u8] =
    b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

pub(crate) const ALPHA: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";

pub(crate) const NUMERIC: &[u8] = b"0123456789";

pub(crate) fn random_alphanumeric(ctx: &mut CallContext, args: &[Object]) -> Object {
    random_charset_string(ctx, "random.alphanumeric", args, ALPHA_NUMERIC)
}

pub(crate) fn random_alpha(ctx: &mut CallContext, args: &[Object]) -> Object {
    random_charset_string(ctx, "random.alpha", args, ALPHA)
}

pub(crate) fn random_numeric(ctx: &mut CallContext, args: &[Object]) -> Object {
    random_charset_string(ctx, "random.numeric", args, NUMERIC)
}

pub(crate) fn random_uuid(ctx: &mut CallContext, _args: &[Object]) -> Object {
    let mut buf = [0u8; 16];
    if let Err(err) = fill_random(ctx, "random.uuid", &mut buf) {
        return err;
    }
    buf[6] = (buf[6] & 0x0f) | 0x40;
    buf[8] = (buf[8] & 0x3f) | 0x80;
    str_obj(format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7], buf[8], buf[9], buf[10], buf[11], buf[12], buf[13], buf[14], buf[15]
    ))
}

pub(crate) fn random_bytes(ctx: &mut CallContext, args: &[Object]) -> Object {
    let size = match required_number(ctx, "random.bytes", args, 0, "size") {
        Ok(v) => v,
        Err(e) => return e,
    };
    if !(0.0..=1_048_576.0).contains(&size) {
        return new_error(
            ctx.pos.clone(),
            "random.bytes: size must be in range [0, 1048576]",
        );
    }
    let mut buf = vec![0u8; size as usize];
    if let Err(err) = fill_random(ctx, "random.bytes", &mut buf) {
        return err;
    }
    array(buf.into_iter().map(|b| num_obj(b as f64)).collect())
}

// ---------------------------------------------------------------------------
// regexp: RE2-based escape / matchAll / split.
// ---------------------------------------------------------------------------
