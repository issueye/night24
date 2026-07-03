use super::super::helpers::*;
use crate::object::{new_error, num_obj, str_obj, CallContext, Object};

pub(crate) fn hash_module() -> Object {
    module(vec![
        ("adler32", native("hash.adler32", hash_adler32)),
        ("crc32", native("hash.crc32", hash_crc32)),
        ("crc64", native("hash.crc64", hash_crc64)),
        ("fnv1a", native("hash.fnv1a", hash_fnv1a)),
        (
            "adler32Number",
            native("hash.adler32Number", hash_adler32_number),
        ),
        ("crc32Number", native("hash.crc32Number", hash_crc32_number)),
    ])
}

pub(crate) fn hash_input(
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

pub(crate) fn hash_adler32(ctx: &mut CallContext, args: &[Object]) -> Object {
    match hash_input(ctx, "hash.adler32", args) {
        Ok(bytes) => str_obj(format!("{:08x}", adler32(&bytes))),
        Err(err) => err,
    }
}

pub(crate) fn hash_crc32(ctx: &mut CallContext, args: &[Object]) -> Object {
    match hash_input(ctx, "hash.crc32", args) {
        Ok(bytes) => str_obj(format!("{:08x}", crc32_ieee(&bytes))),
        Err(err) => err,
    }
}

pub(crate) fn hash_crc64(ctx: &mut CallContext, args: &[Object]) -> Object {
    match hash_input(ctx, "hash.crc64", args) {
        Ok(bytes) => str_obj(format!("{:016x}", crc64_iso(&bytes))),
        Err(err) => err,
    }
}

pub(crate) fn hash_fnv1a(ctx: &mut CallContext, args: &[Object]) -> Object {
    match hash_input(ctx, "hash.fnv1a", args) {
        Ok(bytes) => str_obj(format!("{:016x}", fnv1a_64(&bytes))),
        Err(err) => err,
    }
}

pub(crate) fn hash_adler32_number(ctx: &mut CallContext, args: &[Object]) -> Object {
    match hash_input(ctx, "hash.adler32Number", args) {
        Ok(bytes) => num_obj(adler32(&bytes) as f64),
        Err(err) => err,
    }
}

pub(crate) fn hash_crc32_number(ctx: &mut CallContext, args: &[Object]) -> Object {
    match hash_input(ctx, "hash.crc32Number", args) {
        Ok(bytes) => num_obj(crc32_ieee(&bytes) as f64),
        Err(err) => err,
    }
}

/// Adler-32 checksum (RFC 1950).
fn adler32(data: &[u8]) -> u32 {
    const MOD: u32 = 65521;
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for &byte in data {
        a = (a + byte as u32) % MOD;
        b = (b + a) % MOD;
    }
    (b << 16) | a
}

/// CRC-32 IEEE (polynomial 0xEDB88320), same as Go's crc32.ChecksumIEEE.
fn crc32_ieee(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xffffffff;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xedb88320;
            } else {
                crc >>= 1;
            }
        }
    }
    crc ^ 0xffffffff
}

/// CRC-64 ISO (polynomial 0xD800000000000000), matching Go's crc64 ISO table.
fn crc64_iso(data: &[u8]) -> u64 {
    let table = crc64_iso_table();
    let mut crc: u64 = 0xffff_ffff_ffff_ffff;
    for &byte in data {
        crc = table[((crc ^ byte as u64) & 0xff) as usize] ^ (crc >> 8);
    }
    crc ^ 0xffff_ffff_ffff_ffff
}

pub(crate) fn crc64_iso_table() -> [u64; 256] {
    const POLY: u64 = 0xd800_0000_0000_0000;
    let mut table = [0u64; 256];
    for i in 0..256u32 {
        let mut crc = i as u64;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ POLY;
            } else {
                crc >>= 1;
            }
        }
        table[i as usize] = crc;
    }
    table
}

/// FNV-1a 64-bit hash.
fn fnv1a_64(data: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf29ce484222325;
    const PRIME: u64 = 0x100000001b3;
    let mut hash = OFFSET;
    for &byte in data {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

// ---------------------------------------------------------------------------
// random: cryptographically secure RNG helpers (matches Go's crypto/rand).
// ---------------------------------------------------------------------------
