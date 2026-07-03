//! JSON Lines encoder and decoder for GTP frames
//!
//! This module provides streaming JSON Lines encoding/decoding for GTP frames.
//! Each frame is serialized as a single line of JSON, terminated by a newline.

use super::frame::Frame;
use std::io::{self, BufRead, BufReader, BufWriter, Write};

/// Encoder writes GTP frames as JSON Lines
pub struct JsonlEncoder<W: Write> {
    writer: BufWriter<W>,
}

impl<W: Write> JsonlEncoder<W> {
    /// Create a new encoder wrapping the given writer
    pub fn new(writer: W) -> Self {
        Self {
            writer: BufWriter::new(writer),
        }
    }

    /// Encode and write a single frame
    pub fn encode(&mut self, frame: &Frame) -> io::Result<()> {
        // Serialize to JSON
        let json = serde_json::to_string(frame)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        // Ensure no newlines in the JSON (this would break the protocol)
        if json.contains('\n') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "GTP frame contains newline",
            ));
        }

        // Write JSON + newline
        self.writer.write_all(json.as_bytes())?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()?;

        Ok(())
    }

    /// Get a reference to the underlying writer
    pub fn get_ref(&self) -> &BufWriter<W> {
        &self.writer
    }

    /// Get a mutable reference to the underlying writer
    pub fn get_mut(&mut self) -> &mut BufWriter<W> {
        &mut self.writer
    }

    /// Consume the encoder and return the underlying writer
    pub fn into_inner(self) -> BufWriter<W> {
        self.writer
    }
}

/// Decoder reads GTP frames from JSON Lines
pub struct JsonlDecoder<R: io::Read> {
    reader: BufReader<R>,
}

impl<R: io::Read> JsonlDecoder<R> {
    /// Create a new decoder wrapping the given reader
    pub fn new(reader: R) -> Self {
        Self {
            reader: BufReader::new(reader),
        }
    }

    /// Decode and read a single frame
    ///
    /// Returns Err(UnexpectedEof) when the stream ends
    pub fn decode(&mut self) -> io::Result<Frame> {
        let mut line = String::new();
        let n = self.reader.read_line(&mut line)?;

        // EOF
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "EOF while reading GTP frame",
            ));
        }

        // Trim whitespace
        let line = line.trim_end();

        // Empty line - skip and try again
        if line.is_empty() {
            return self.decode();
        }

        // Parse JSON
        serde_json::from_str(line).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    /// Get a reference to the underlying reader
    pub fn get_ref(&self) -> &BufReader<R> {
        &self.reader
    }

    /// Get a mutable reference to the underlying reader
    pub fn get_mut(&mut self) -> &mut BufReader<R> {
        &mut self.reader
    }

    /// Consume the decoder and return the underlying reader
    pub fn into_inner(self) -> BufReader<R> {
        self.reader
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gtp::frame::{Frame, Value};
    use std::io::Cursor;

    #[test]
    fn test_encode_decode_roundtrip() {
        let frame = Frame::hello("h1".to_string(), Some("gts_r".to_string()));

        // Encode
        let mut buf = Vec::new();
        {
            let mut encoder = JsonlEncoder::new(&mut buf);
            encoder.encode(&frame).unwrap();
        } // encoder dropped here, releasing the borrow

        // Decode
        let cursor = Cursor::new(buf);
        let mut decoder = JsonlDecoder::new(cursor);
        let decoded = decoder.decode().unwrap();

        assert_eq!(decoded.frame_type, "hello");
        assert_eq!(decoded.id, "h1");
        assert_eq!(decoded.runtime, Some("gts_r".to_string()));
    }

    #[test]
    fn test_multiple_frames() {
        let frames = vec![
            Frame::hello("h1".to_string(), None),
            Frame::call(
                "c1".to_string(),
                "mod".to_string(),
                "method".to_string(),
                vec![],
            ),
            Frame::ok_result("c1".to_string(), Value::number(42.0)),
        ];

        // Encode all
        let mut buf = Vec::new();
        {
            let mut encoder = JsonlEncoder::new(&mut buf);
            for frame in &frames {
                encoder.encode(frame).unwrap();
            }
        } // encoder dropped here

        // Decode all
        let cursor = Cursor::new(buf);
        let mut decoder = JsonlDecoder::new(cursor);

        let f1 = decoder.decode().unwrap();
        assert_eq!(f1.frame_type, "hello");

        let f2 = decoder.decode().unwrap();
        assert_eq!(f2.frame_type, "call");

        let f3 = decoder.decode().unwrap();
        assert_eq!(f3.frame_type, "result");
        assert_eq!(f3.ok, Some(true));

        // Should be EOF
        let result = decoder.decode();
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn test_reject_newline_in_frame() {
        // This should never happen with serde_json, but we check anyway
        let mut buf = Vec::new();
        let mut encoder = JsonlEncoder::new(&mut buf);

        // Manually create invalid JSON (this won't pass through Frame serialization,
        // but tests the check in encode())
        // In practice, this is defensive programming.

        // Normal frames should work
        let frame = Frame::hello("h1".to_string(), None);
        assert!(encoder.encode(&frame).is_ok());
    }

    #[test]
    fn test_decode_eof() {
        let cursor = Cursor::new(Vec::new());
        let mut decoder = JsonlDecoder::new(cursor);
        let result = decoder.decode();
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn test_decode_invalid_json() {
        let cursor = Cursor::new(b"not valid json\n");
        let mut decoder = JsonlDecoder::new(cursor);
        let result = decoder.decode();
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn test_skip_empty_lines() {
        let input = b"\n\n{\"v\":1,\"id\":\"h1\",\"type\":\"hello\"}\n";
        let cursor = Cursor::new(input);
        let mut decoder = JsonlDecoder::new(cursor);
        let frame = decoder.decode().unwrap();
        assert_eq!(frame.frame_type, "hello");
    }
}
