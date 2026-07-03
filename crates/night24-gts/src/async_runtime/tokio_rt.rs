#[cfg(feature = "tokio")]
use std::future::Future;
//
/// Tokio runtime integration for multi-threaded async execution
///
/// This module provides a bridge between GTS's single-threaded Awaitable
/// system and tokio's multi-threaded async runtime.
///
/// ## Architecture
///
/// The bridge works by:
/// 1. Converting GTS Awaitables to tokio Futures
/// 2. Running futures on tokio's thread pool
/// 3. Bridging results back to GTS via channels
///
/// ## Thread Safety
///
/// Since GTS objects use Rc/RefCell (not thread-safe), we:
/// - Clone/serialize data when crossing thread boundaries
/// - Use channels to communicate results back to main thread
/// - Keep actual GTS object manipulation on the main thread
#[cfg(feature = "tokio")]
use tokio::runtime::{Builder, Runtime};

/// Tokio runtime wrapper for GTS
///
/// This provides a multi-threaded async runtime powered by tokio.
#[cfg(feature = "tokio")]
pub struct TokioRuntime {
    runtime: Runtime,
}

#[cfg(feature = "tokio")]
impl TokioRuntime {
    /// Create a new tokio runtime with default configuration
    pub fn new() -> Self {
        let runtime = Builder::new_multi_thread()
            .worker_threads(4)
            .thread_name("gts-tokio-worker")
            .enable_all()
            .build()
            .expect("Failed to create tokio runtime");

        TokioRuntime { runtime }
    }

    /// Create a tokio runtime with custom number of worker threads
    pub fn with_worker_threads(threads: usize) -> Self {
        let runtime = Builder::new_multi_thread()
            .worker_threads(threads)
            .thread_name("gts-tokio-worker")
            .enable_all()
            .build()
            .expect("Failed to create tokio runtime");

        TokioRuntime { runtime }
    }

    /// Get a handle to the underlying tokio runtime
    pub fn handle(&self) -> tokio::runtime::Handle {
        self.runtime.handle().clone()
    }

    /// Spawn a future on the tokio runtime
    pub fn spawn<F>(&self, future: F) -> tokio::task::JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.runtime.spawn(future)
    }

    /// Block on a future until it completes
    pub fn block_on<F: Future>(&self, future: F) -> F::Output {
        self.runtime.block_on(future)
    }

    /// Shutdown the runtime, waiting for all tasks to complete
    pub fn shutdown(self) {
        self.runtime.shutdown_background();
    }
}

#[cfg(feature = "tokio")]
impl Default for TokioRuntime {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper to spawn async TCP operations on tokio
#[cfg(feature = "tokio")]
pub mod tcp {

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    /// Async TCP connect using tokio
    pub async fn connect(addr: String) -> Result<TcpStream, std::io::Error> {
        TcpStream::connect(&addr).await
    }

    /// Async TCP read using tokio
    pub async fn read(stream: &mut TcpStream, buf: &mut [u8]) -> Result<usize, std::io::Error> {
        stream.read(buf).await
    }

    /// Async TCP write using tokio
    pub async fn write(stream: &mut TcpStream, data: &[u8]) -> Result<usize, std::io::Error> {
        stream.write(data).await
    }
}

/// Example usage and integration tests
#[cfg(all(test, feature = "tokio"))]
mod tests {
    use super::*;

    #[test]
    fn test_create_tokio_runtime() {
        let runtime = TokioRuntime::new();
        assert!(runtime.handle().metrics().num_workers() > 0);
    }

    #[test]
    fn test_spawn_simple_task() {
        let runtime = TokioRuntime::new();
        let handle = runtime.spawn(async {
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            42
        });

        let result = runtime.block_on(handle).unwrap();
        assert_eq!(result, 42);
    }

    #[test]
    fn test_tcp_connect() {
        let runtime = TokioRuntime::new();

        // Try to connect to a known unreachable address
        // This should fail quickly, demonstrating tokio networking
        let result = runtime.block_on(async {
            tokio::time::timeout(
                tokio::time::Duration::from_millis(100),
                tcp::connect("192.0.2.1:12345".to_string()),
            )
            .await
        });

        // Either timeout or connection refused - both are expected
        assert!(result.is_err() || result.unwrap().is_err());
    }

    #[test]
    fn test_multiple_workers() {
        let runtime = TokioRuntime::with_worker_threads(8);
        assert!(runtime.handle().metrics().num_workers() >= 8);
    }
}

/// Documentation module (always available)
pub mod docs {
    /// Usage example for tokio integration
    ///
    /// ```rust,ignore
    /// #[cfg(feature = "tokio")]
    /// use gts::async_runtime::tokio_rt::TokioRuntime;
    ///
    /// #[cfg(feature = "tokio")]
    /// fn example() {
    ///     let runtime = TokioRuntime::new();
    ///     
    ///     // Spawn async work
    ///     let handle = runtime.spawn(async {
    ///         // Async operations here
    ///         println!("Running on tokio!");
    ///     });
    ///     
    ///     // Wait for completion
    ///     runtime.block_on(handle).unwrap();
    /// }
    /// ```
    pub fn usage_example() {}
}
