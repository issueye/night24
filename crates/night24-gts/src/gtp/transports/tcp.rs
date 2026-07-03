//! TCP socket transport
//!
//! This transport uses TCP sockets for GTP communication,
//! enabling network-based plugin communication.

use crate::gtp::frame::Frame;
use crate::gtp::transport::{StreamTransport, Transport};
use std::io;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

/// TCP socket transport
///
/// Wraps a TCP connection for GTP frame exchange.
pub struct TcpTransport {
    inner: StreamTransport<TcpStream, TcpStream>,
    stream: TcpStream,
}

impl TcpTransport {
    /// Connect to a TCP server at the given address
    ///
    /// # Example
    ///
    /// ```no_run
    /// use gts::gtp::transports::TcpTransport;
    ///
    /// let transport = TcpTransport::connect("localhost:9000").unwrap();
    /// ```
    pub fn connect<A: ToSocketAddrs>(addr: A) -> io::Result<Self> {
        let stream = TcpStream::connect(addr)?;

        // Clone for read and write (TcpStream supports this)
        let read_stream = stream.try_clone()?;
        let write_stream = stream.try_clone()?;

        Ok(Self {
            inner: StreamTransport::new(read_stream, write_stream),
            stream,
        })
    }

    /// Create a transport from an existing TCP stream
    pub fn from_stream(stream: TcpStream) -> io::Result<Self> {
        let read_stream = stream.try_clone()?;
        let write_stream = stream.try_clone()?;

        Ok(Self {
            inner: StreamTransport::new(read_stream, write_stream),
            stream,
        })
    }

    /// Set read timeout
    pub fn set_read_timeout(&self, dur: Option<Duration>) -> io::Result<()> {
        self.stream.set_read_timeout(dur)
    }

    /// Set write timeout
    pub fn set_write_timeout(&self, dur: Option<Duration>) -> io::Result<()> {
        self.stream.set_write_timeout(dur)
    }

    /// Get the peer address
    pub fn peer_addr(&self) -> io::Result<std::net::SocketAddr> {
        self.stream.peer_addr()
    }

    /// Get the local address
    pub fn local_addr(&self) -> io::Result<std::net::SocketAddr> {
        self.stream.local_addr()
    }
}

impl Transport for TcpTransport {
    fn send_frame(&mut self, frame: &Frame) -> io::Result<()> {
        self.inner.send_frame(frame)
    }

    fn recv_frame(&mut self) -> io::Result<Frame> {
        self.inner.recv_frame()
    }

    fn close(&mut self) -> io::Result<()> {
        self.inner.close()?;
        self.stream.shutdown(std::net::Shutdown::Both)
    }

    fn is_alive(&self) -> bool {
        self.inner.is_alive() && self.stream.peer_addr().is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gtp::frame::Frame;
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn test_tcp_transport_roundtrip() {
        // Start a simple echo server
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let server_thread = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut transport = TcpTransport::from_stream(stream).unwrap();

            // Echo back the frame
            let frame = transport.recv_frame().unwrap();
            transport.send_frame(&frame).unwrap();
        });

        // Client
        thread::sleep(Duration::from_millis(50)); // Give server time to start
        let mut client = TcpTransport::connect(addr).unwrap();

        let frame = Frame::hello("h1".to_string(), Some("test".to_string()));
        client.send_frame(&frame).unwrap();

        let response = client.recv_frame().unwrap();
        assert_eq!(response.frame_type, "hello");
        assert_eq!(response.id, "h1");

        client.close().unwrap();
        server_thread.join().unwrap();
    }

    #[test]
    fn test_tcp_transport_peer_addr() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            drop(stream); // Accept and drop
        });

        thread::sleep(Duration::from_millis(50));
        let transport = TcpTransport::connect(addr).unwrap();

        let peer = transport.peer_addr().unwrap();
        assert_eq!(peer, addr);
    }
}
