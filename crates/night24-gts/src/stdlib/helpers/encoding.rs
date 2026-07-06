use super::*;

pub(crate) fn base64_std_encode(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    let mut chunks = input.chunks_exact(3);
    for chunk in &mut chunks {
        let n = ((chunk[0] as u32) << 16) | ((chunk[1] as u32) << 8) | chunk[2] as u32;
        out.push(TABLE[((n >> 18) & 0x3f) as usize] as char);
        out.push(TABLE[((n >> 12) & 0x3f) as usize] as char);
        out.push(TABLE[((n >> 6) & 0x3f) as usize] as char);
        out.push(TABLE[(n & 0x3f) as usize] as char);
    }
    let rem = chunks.remainder();
    match rem.len() {
        1 => {
            let n = (rem[0] as u32) << 16;
            out.push(TABLE[((n >> 18) & 0x3f) as usize] as char);
            out.push(TABLE[((n >> 12) & 0x3f) as usize] as char);
            out.push('=');
            out.push('=');
        }
        2 => {
            let n = ((rem[0] as u32) << 16) | ((rem[1] as u32) << 8);
            out.push(TABLE[((n >> 18) & 0x3f) as usize] as char);
            out.push(TABLE[((n >> 12) & 0x3f) as usize] as char);
            out.push(TABLE[((n >> 6) & 0x3f) as usize] as char);
            out.push('=');
        }
        _ => {}
    }
    out
}

/// URL-safe base64 alphabet (`-_`) with no padding.
pub(crate) fn base64_url_encode(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(input.len() * 4 / 3 + 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((n >> 18) & 0x3f) as usize] as char);
        out.push(TABLE[((n >> 12) & 0x3f) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((n >> 6) & 0x3f) as usize] as char);
        }
        if chunk.len() > 2 {
            out.push(TABLE[(n & 0x3f) as usize] as char);
        }
    }
    out
}

pub(crate) fn base64url_encode_string(input: &[u8]) -> String {
    base64_url_encode(input)
}

pub(crate) fn base64_decode_into(
    table: &[Option<u8>; 256],
    name: &str,
    text: &str,
    ignore_padding: bool,
) -> Result<Vec<u8>, String> {
    let mut bits: u32 = 0;
    let mut shift: u32 = 0;
    let mut out = Vec::new();
    for ch in text.chars() {
        if ignore_padding && ch == '=' {
            continue;
        }
        let v = table[ch as usize].ok_or_else(|| format!("{}: invalid base64 data", name))?;
        bits = (bits << 6) | v as u32;
        shift += 6;
        if shift >= 8 {
            shift -= 8;
            out.push((bits >> shift) as u8);
        }
    }
    Ok(out)
}

pub(crate) fn base64_std_table() -> [Option<u8>; 256] {
    let mut t = [None; 256];
    for (i, c) in b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
        .iter()
        .enumerate()
    {
        t[*c as usize] = Some(i as u8);
    }
    t
}

pub(crate) fn base64_url_table() -> [Option<u8>; 256] {
    let mut t = [None; 256];
    for (i, c) in b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_"
        .iter()
        .enumerate()
    {
        t[*c as usize] = Some(i as u8);
    }
    t
}

/// Apply the optional `{asBuffer: true}` flag and render the result.
pub(crate) fn bytes_result(
    _ctx: &mut CallContext,
    _name: &str,
    bytes: Vec<u8>,
    opts: Option<&Object>,
) -> Object {
    let as_buffer = hash_bool_arg(opts, "asBuffer").unwrap_or(false);
    if as_buffer {
        make_buffer(bytes)
    } else {
        match String::from_utf8(bytes.clone()) {
            Ok(s) => str_obj(s),
            // Fall back to lossy conversion to preserve a string return type,
            // matching the Go behavior where non-UTF8 bytes become a string.
            Err(_) => str_obj(String::from_utf8_lossy(&bytes).into_owned()),
        }
    }
}

/// Build a Buffer-shaped Hash so that it round-trips through `bytes_from_object`.
pub(crate) fn make_buffer(bytes: Vec<u8>) -> Object {
    let elements: Vec<Object> = bytes.iter().map(|b| num_obj(*b as f64)).collect();
    let inner = array(elements);
    ObjectBuilder::new()
        .set(BUFFER_DATA_KEY, inner)
        .set("length", num_obj(bytes.len() as f64))
        .build()
}

// ---------------------------------------------------------------------------
// hex
// ---------------------------------------------------------------------------

pub(crate) fn hex_encode_bytes(input: &[u8]) -> String {
    const TABLE: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(input.len() * 2);
    for b in input {
        out.push(TABLE[(b >> 4) as usize] as char);
        out.push(TABLE[(b & 0x0f) as usize] as char);
    }
    out
}

pub(crate) fn hex_decode_bytes(name: &str, text: &str) -> Result<Vec<u8>, String> {
    if !text.len().is_multiple_of(2) {
        return Err(format!("{}: invalid hex data", name));
    }
    let mut out = Vec::with_capacity(text.len() / 2);
    let bytes = text.as_bytes();
    for chunk in bytes.chunks_exact(2) {
        let hi = hex_val(chunk[0]).ok_or_else(|| format!("{}: invalid hex data", name))?;
        let lo = hex_val(chunk[1]).ok_or_else(|| format!("{}: invalid hex data", name))?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

pub(crate) fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}
