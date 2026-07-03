//! Transport trait and implementations
//!
//! This module defines the abstract `Transport` trait for sending and receiving
//! GTP frames, along with a generic `StreamTransport` implementation that works
//! with any `Read` + `Write` types.

use super::codec::{JsonlDecoder, JsonlEncoder};
use super::frame::Frame;
use std::io;

/// Transport abstraction for GTP communication
///
/// Implementations can use different underlying transports (stdio, TCP, WebSocket, etc.)
/// while providing a uniform interface for frame exchange.
pub trait Transport {
    /// Send a GTP frame
    fn send_frame(&mut self, frame: &Frame) -> io::Result<()>;

    /// Receive a GTP frame (blocks until a complete frame is received)
    fn recv_frame(&mut self) -> io::Result<Frame>;

    /// Close the transport
    fn close(&mut self) -> io::Result<()>;

    /// Check if the transport is still alive
    fn is_alive(&self) -> bool;
}

/// Generic stream-based transport implementation
///
/// This works with any types that implement `Read` and `Write`,
/// such as TcpStream, stdio pipes, or in-memory buffers.
pub struct StreamTransport<R: io::Read, W: io::Write> {
    encoder: JsonlEncoder<W>,
    decoder: JsonlDecoder<R>,
    alive: bool,
}

impl<R: io::Read, W: io::Write> StreamTransport<R, W> {
    /// Create a new stream transport
    pub fn new(reader: R, writer: W) -> Self {
        Self {
            encoder: JsonlEncoder::new(writer),
            decoder: JsonlDecoder::new(reader),
            alive: true,
        }
    }

    /// Get a reference to the encoder
    pub fn encoder(&self) -> &JsonlEncoder<W> {
        &self.encoder
    }

    /// Get a mutable reference to the encoder
    pub fn encoder_mut(&mut self) -> &mut JsonlEncoder<W> {
        &mut self.encoder
    }

    /// Get a reference to the decoder
    pub fn decoder(&self) -> &JsonlDecoder<R> {
        &self.decoder
    }

    /// Get a mutable reference to the decoder
    pub fn decoder_mut(&mut self) -> &mut JsonlDecoder<R> {
        &mut self.decoder
    }
}

impl<R: io::Read, W: io::Write> Transport for StreamTransport<R, W> {
    fn send_frame(&mut self, frame: &Frame) -> io::Result<()> {
        if !self.alive {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "Transport is closed",
            ));
        }
        self.encoder.encode(frame)
    }

    fn recv_frame(&mut self) -> io::Result<Frame> {
        if !self.alive {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "Transport is closed",
            ));
        }

        match self.decoder.decode() {
            Ok(frame) => Ok(frame),
            Err(e) => {
                // Mark as dead on EOF or other fatal errors
                if e.kind() == io::ErrorKind::UnexpectedEof {
                    self.alive = false;
                }
                Err(e)
            }
        }
    }

    fn close(&mut self) -> io::Result<()> {
        self.alive = false;
        Ok(())
    }

    fn is_alive(&self) -> bool {
        self.alive
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gtp::frame::Frame;
    use std::io::Cursor;

    #[test]
    fn test_stream_transport_bidirectional() {
        // Simulate a bidirectional stream (for testing, use two separate buffers)
        let mut write_buf = Vec::new();
        let read_buf = Vec::new();

        // First, write a frame
        {
            let mut transport = StreamTransport::new(Cursor::new(read_buf.clone()), &mut write_buf);

            let frame = Frame::hello("h1".to_string(), Some("gts_r".to_string()));
            transport.send_frame(&frame).unwrap();
        }

        // Now read it back
        {
            let mut transport = StreamTransport::new(Cursor::new(write_buf), Vec::new());

            let frame = transport.recv_frame().unwrap();
            assert_eq!(frame.frame_type, "hello");
            assert_eq!(frame.id, "h1");
        }
    }

    #[test]
    fn test_transport_close() {
        let mut transport = StreamTransport::new(Cursor::new(Vec::new()), Vec::new());

        assert!(transport.is_alive());
        transport.close().unwrap();
        assert!(!transport.is_alive());

        // Should error after close
        let result = transport.send_frame(&Frame::hello("h1".to_string(), None));
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::NotConnected);
    }

    #[test]
    fn test_recv_eof_marks_dead() {
        let mut transport = StreamTransport::new(
            Cursor::new(Vec::new()), // Empty = immediate EOF
            Vec::new(),
        );

        assert!(transport.is_alive());

        let result = transport.recv_frame();
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::UnexpectedEof);

        // Should be marked as dead
        assert!(!transport.is_alive());
    }
}
