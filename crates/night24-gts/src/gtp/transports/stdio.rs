//! Stdio transport using stdin/stdout
//!
//! This transport is primarily used for plugin processes that communicate
//! with the parent process via standard input/output streams.

use crate::gtp::transport::StreamTransport;
use std::io;

/// Transport using stdin/stdout
///
/// This is the default transport for GTP plugin communication.
/// Plugins read frames from stdin and write frames to stdout.
/// Logging should go to stderr to avoid interfering with the protocol.
pub type StdioTransport = StreamTransport<io::Stdin, io::Stdout>;

/// Create a new stdio transport using the process's stdin/stdout
pub fn create_stdio_transport() -> StdioTransport {
    StreamTransport::new(io::stdin(), io::stdout())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gtp::Transport;

    #[test]
    fn test_stdio_transport_type() {
        // Just verify that the type compiles and can be created
        // (We can't actually test stdin/stdout in unit tests)

        // Verify it implements Transport
        fn accepts_transport<T: Transport>(_t: T) {}

        let transport = create_stdio_transport();
        accepts_transport(transport);
    }
}
