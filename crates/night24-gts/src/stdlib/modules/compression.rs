use super::super::helpers::*;
use super::compress_gzip::{
    bytes_to_latin1_string, gzip_compress_bytes, gzip_decompress_bytes, latin1_string_to_bytes,
};
use crate::object::{new_error, str_obj, CallContext, Object};

pub(crate) fn compression_module() -> Object {
    module(vec![
        (
            "gzipCompress",
            native("compression.gzipCompress", compression_gzip_compress),
        ),
        (
            "gzipDecompress",
            native("compression.gzipDecompress", compression_gzip_decompress),
        ),
    ])
}

pub(crate) fn compression_gzip_compress(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "gzipCompress", args);
    let value = match reader.required_string(0, "data") {
        Ok(value) => value,
        Err(err) => return err,
    };
    match gzip_compress_bytes(value.as_bytes()) {
        Ok(bytes) => str_obj(bytes_to_latin1_string(&bytes)),
        Err(e) => new_error(ctx.pos.clone(), format!("gzipCompress: {}", e)),
    }
}

pub(crate) fn compression_gzip_decompress(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "gzipDecompress", args);
    let value = match reader.required_string(0, "data") {
        Ok(value) => value,
        Err(err) => return err,
    };
    match gzip_decompress_bytes(&latin1_string_to_bytes(&value)) {
        Ok(bytes) => str_obj(String::from_utf8_lossy(&bytes).into_owned()),
        Err(e) => new_error(ctx.pos.clone(), format!("gzipDecompress: {}", e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gzip_bytes_round_trip_through_latin1_string() {
        let compressed = gzip_compress_bytes(b"night24").unwrap();
        let encoded = bytes_to_latin1_string(&compressed);
        let decoded = gzip_decompress_bytes(&latin1_string_to_bytes(&encoded)).unwrap();

        assert_eq!(decoded, b"night24");
    }
}
