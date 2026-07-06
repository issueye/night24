use std::fs;
use std::io::{Read, Write};

use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;

use super::super::helpers::*;
use crate::object::{new_error, CallContext, Object};

pub(crate) fn gzip_module() -> Object {
    module(vec![
        ("compress", native("gzip.compress", gzip_compress)),
        ("decompress", native("gzip.decompress", gzip_decompress)),
        (
            "compressFileSync",
            native("gzip.compressFileSync", gzip_compress_file_sync),
        ),
        (
            "decompressFileSync",
            native("gzip.decompressFileSync", gzip_decompress_file_sync),
        ),
    ])
}

pub(crate) fn gzip_compress(ctx: &mut CallContext, args: &[Object]) -> Object {
    let Some(value) = args.first() else {
        return new_error(ctx.pos.clone(), "gzip.compress requires value");
    };
    let data = match bytes_from_object(ctx, "gzip.compress", value) {
        Ok(data) => data,
        Err(err) => return err,
    };
    match gzip_compress_bytes(&data) {
        Ok(bytes) => make_buffer(bytes),
        Err(e) => new_error(ctx.pos.clone(), format!("gzip.compress: {}", e)),
    }
}

pub(crate) fn gzip_decompress(ctx: &mut CallContext, args: &[Object]) -> Object {
    let Some(value) = args.first() else {
        return new_error(ctx.pos.clone(), "gzip.decompress requires value");
    };
    let data = match bytes_from_object(ctx, "gzip.decompress", value) {
        Ok(data) => data,
        Err(err) => return err,
    };
    match gzip_decompress_bytes(&data) {
        Ok(bytes) => bytes_result(ctx, "gzip.decompress", bytes, args.get(1)),
        Err(e) => new_error(ctx.pos.clone(), format!("gzip.decompress: {}", e)),
    }
}

pub(crate) fn gzip_compress_file_sync(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "gzip.compressFileSync", args);
    let src = match reader.required_string(0, "source path") {
        Ok(src) => src,
        Err(err) => return err,
    };
    let dst = match reader.required_string(1, "destination path") {
        Ok(dst) => dst,
        Err(err) => return err,
    };
    match fs::read(&src).and_then(|data| {
        gzip_compress_bytes(&data)
            .map_err(std::io::Error::other)
            .and_then(|compressed| fs::write(&dst, compressed))
    }) {
        Ok(_) => Object::Undefined,
        Err(e) => new_error(ctx.pos.clone(), format!("gzip.compressFileSync: {}", e)),
    }
}

pub(crate) fn gzip_decompress_file_sync(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "gzip.decompressFileSync", args);
    let src = match reader.required_string(0, "source path") {
        Ok(src) => src,
        Err(err) => return err,
    };
    let dst = match reader.required_string(1, "destination path") {
        Ok(dst) => dst,
        Err(err) => return err,
    };
    match fs::read(&src).and_then(|data| {
        gzip_decompress_bytes(&data)
            .map_err(std::io::Error::other)
            .and_then(|decompressed| fs::write(&dst, decompressed))
    }) {
        Ok(_) => Object::Undefined,
        Err(e) => new_error(ctx.pos.clone(), format!("gzip.decompressFileSync: {}", e)),
    }
}

pub(crate) fn gzip_compress_bytes(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data).map_err(|e| e.to_string())?;
    encoder.finish().map_err(|e| e.to_string())
}

pub(crate) fn gzip_decompress_bytes(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut decoder = GzDecoder::new(data);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out).map_err(|e| e.to_string())?;
    Ok(out)
}

pub(crate) fn bytes_to_latin1_string(bytes: &[u8]) -> String {
    bytes.iter().map(|b| char::from(*b)).collect()
}

pub(crate) fn latin1_string_to_bytes(value: &str) -> Vec<u8> {
    value.chars().map(|ch| ch as u32 as u8).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gzip_bytes_round_trip() {
        let input = b"night24 gzip round trip";
        let compressed = gzip_compress_bytes(input).unwrap();
        let decompressed = gzip_decompress_bytes(&compressed).unwrap();

        assert_eq!(decompressed, input);
    }
}

// ---------------------------------------------------------------------------
// terminal: deterministic CI-friendly ANSI helpers and session stubs.
// ---------------------------------------------------------------------------
