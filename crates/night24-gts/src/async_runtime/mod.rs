/// Async runtime: completion-queue coordination between the single-threaded
/// VM and background workers.
///
/// ## Architecture
///
/// Async work is resolved through a thread-safe completion queue rather than
/// a poll-based event loop:
/// - [`AsyncCompletionQueue`]: a `Mutex`+`Condvar` queue collecting owned
///   results from background workers (`std::thread::spawn` or a tokio runtime).
/// - [`VirtualMachine::wait_async`](crate::object::VirtualMachine::wait_async):
/// drains the queue on the VM thread and settles the matching `Promise`s, so
///   all `Object` manipulation stays single-threaded.
/// - [`Promise`](crate::object::Promise): the language-level async primitive,
///   resolved/rejected by the drain loop.
///
/// With the `tokio` feature (default), [`TokioRuntime`] provides a standalone
/// multi-threaded runtime wrapper for callers that need one (e.g. the HTTP
/// client builds its own). The VM itself does not own a tokio runtime.
///
/// ## Design Principles
///
/// - **Single-threaded VM**: all `Object`/`Rc<RefCell<..>>` work happens on the
///   VM thread; workers exchange only owned (`Send`) data via the queue.
/// - **Feature-gated**: the tokio dependency is opt-in via the `tokio` feature.
pub mod completion;

#[cfg(feature = "tokio")]
pub mod tokio_rt;

// Re-export the native runtime as the default
pub use completion::{
    AsyncCompletion, AsyncCompletionData, AsyncCompletionId, AsyncCompletionQueue,
    AsyncCompletionResult, AsyncCompletionSender, AsyncHttpResponse,
};

#[cfg(feature = "tokio")]
pub use tokio_rt::TokioRuntime;
