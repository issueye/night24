use super::*;

pub(crate) fn now_millis() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0)
}

thread_local! {
    static RUNTIME_ARGV: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

pub(crate) fn set_runtime_argv(argv: Vec<String>) {
    RUNTIME_ARGV.with(|cell| {
        *cell.borrow_mut() = argv;
    });
}

pub(crate) fn runtime_argv_snapshot() -> Vec<String> {
    RUNTIME_ARGV.with(|cell| cell.borrow().clone())
}

// ---------------------------------------------------------------------------
// timers: forwards to the global timer builtins (setTimeout/setInterval/
// sleepAsync) and provides the synchronous sleep plus no-op clear* / microtask
// helpers expected by the Go `@std/timers` surface.
//
// The Rust runtime executes timers inline on the calling thread, so
// clear* are effectively no-ops and queueMicrotask runs the callback
// immediately — this preserves the observable ordering of a script that
// finishes before any async work escapes.
// ---------------------------------------------------------------------------
