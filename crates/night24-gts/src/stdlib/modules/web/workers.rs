use std::cell::{Cell, RefCell};
use std::fs;
use std::rc::Rc;

use super::super::signal::ctrlc_set_flag;
use super::request::{poll_active_streams, web_handle_request, ActiveStreams, WebRequestOutcome};
use super::WebApp;
use crate::object::{new_error, num_obj, CallContext, HashData, Object};
use crate::stdlib::helpers::{ArgReader, ObjectBuilder, ObjectView};

/// The shared handle a worker thread receives from the spawner.
struct WebWorkerCtx {
    /// The single bound listener shared by all workers (accept-ready model).
    server: std::sync::Arc<tiny_http::Server>,
    /// Set to true by `app.close()` or Ctrl+C to ask workers to exit.
    shutdown: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// This worker's id (0-based), for logging.
    id: usize,
}

thread_local! {
    static WEB_WORKER_CTX: RefCell<Option<WebWorkerCtx>> = const { RefCell::new(None) };
}

pub(super) fn signal_current_worker_shutdown() {
    WEB_WORKER_CTX.with(|c| {
        if let Some(wctx) = c.borrow().as_ref() {
            wctx.shutdown
                .store(true, std::sync::atomic::Ordering::Relaxed);
            wctx.server.unblock();
        }
    });
}

/// Bind a tiny_http server and serve requests.
///
/// Options (`{count, workers}`):
/// - `{count: N}` - serial mode. Process N requests in a loop on the calling
///   thread, then return. This is the original single-threaded behaviour and
///   remains the default for tests that rely on shared in-memory state.
/// - `{workers: N}` (N >= 2) - concurrent mode. Spawn N worker threads, each
///   running its own independent VM that re-loads the script to rebuild the
///   route table, and serves requests from the shared listener in parallel.
///   `listen` blocks until `app.close()` or Ctrl+C.
/// - `{}` or omitted - defaults to `{workers: 1}`, i.e. a long-running single
///   worker (serial semantics, but blocks indefinitely instead of returning
///   after one request).
///
/// `workers` takes precedence over `count` when both are given.
pub(super) fn web_listen(ctx: &mut CallContext, app: &Rc<WebApp>, args: &[Object]) -> Object {
    // ---- Worker intercept -------------------------------------------------
    // If this thread is a prefork worker, it is re-executing the user's script
    // top-level. The `app.listen(...)` call must NOT bind again or spawn more
    // workers; instead it enters the shared accept loop. The WebApp here is the
    // worker's own freshly-built instance (independent routes), which is
    // exactly what we want each worker to use when dispatching requests.
    let worker_jump = WEB_WORKER_CTX.with(|c| c.borrow().is_some());
    if worker_jump {
        return web_listen_worker(ctx, app);
    }

    let reader = ArgReader::new(ctx, "web.listen", args);
    let port = match reader.required_number(0, "port") {
        Ok(v) => v as u16,
        Err(e) => return e,
    };

    // Parse options. Defaults: long-running single worker. Explicit `count`
    // keeps the bounded serial behavior used by unit tests.
    let mut count: usize = 1;
    let mut workers: usize = 1;
    if let Some(opts) = reader.object_view(1) {
        let opts = ObjectView::new(&opts);
        if let Some(n) = opts.number("count") {
            count = n as usize;
            workers = 0;
        }
        if let Some(n) = opts.number("workers") {
            workers = n as usize;
        }
    }

    let bind = format!("0.0.0.0:{}", port);
    let server = match tiny_http::Server::http(bind.as_str()) {
        Ok(s) => s,
        Err(e) => return new_error(ctx.pos.clone(), format!("web.listen: {}", e)),
    };
    let bound_port = match server.server_addr() {
        tiny_http::ListenAddr::IP(addr) => addr.port(),
    };

    let result_obj = ObjectBuilder::new()
        .set("port", num_obj(bound_port as f64))
        .into_shared();

    if workers >= 2 {
        // Concurrent prefork path.
        web_listen_concurrent(ctx, app, server, workers, result_obj)
    } else if workers == 1 {
        // Long-running single worker: block until close/shutdown.
        *app.server.borrow_mut() = Some(server);
        let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        *app.shutdown_signal.borrow_mut() = Some(shutdown.clone());
        web_listen_serial(ctx, app, /*count=*/ usize::MAX, Some(shutdown));
        *app.server.borrow_mut() = None;
        *app.shutdown_signal.borrow_mut() = None;
        Object::Hash(result_obj)
    } else {
        // Original serial path: serve `count` requests then return.
        *app.server.borrow_mut() = Some(server);
        web_listen_serial(ctx, app, count, None);
        *app.server.borrow_mut() = None;
        Object::Hash(result_obj)
    }
}

/// Serial request loop: accept and handle up to `count` requests on the
/// calling thread. When `count == usize::MAX` and a `shutdown` flag is given,
/// loops until the flag is set (long-running single-worker mode).
fn web_listen_serial(
    ctx: &mut CallContext,
    app: &Rc<WebApp>,
    count: usize,
    shutdown: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
) {
    let infinite = count == usize::MAX;
    let mut served: usize = 0;
    let pending_responses = Rc::new(Cell::new(0usize));
    let active_streams: ActiveStreams = Rc::new(RefCell::new(Vec::new()));
    loop {
        poll_active_streams(&active_streams);
        if !infinite && served >= count {
            if pending_responses.get() == 0 {
                break;
            }
            if active_streams.borrow().is_empty() {
                ctx.vm().wait_async();
                ctx.vm().drain_async_completions();
            } else {
                std::thread::sleep(std::time::Duration::from_millis(10));
                ctx.vm().drain_async_completions();
                poll_active_streams(&active_streams);
            }
            continue;
        }
        if let Some(flag) = shutdown.as_ref() {
            if flag.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }
        }
        ctx.vm().drain_async_completions();
        let request = {
            let guard = app.server.borrow();
            let srv = match guard.as_ref() {
                Some(s) => s,
                None => break,
            };
            // Use recv_timeout so we can periodically check the shutdown flag.
            let timeout = std::time::Duration::from_millis(100);
            match srv.recv_timeout(timeout) {
                Ok(Some(r)) => r,
                Ok(None) => {
                    ctx.vm().drain_async_completions();
                    continue;
                }
                Err(_) => {
                    ctx.vm().drain_async_completions();
                    continue;
                }
            }
        };
        match web_handle_request(
            ctx,
            app,
            request,
            Some(pending_responses.clone()),
            Some(active_streams.clone()),
        ) {
            Ok(WebRequestOutcome::Responded) => served += 1,
            Ok(WebRequestOutcome::Pending) => served += 1,
            Err(_e) => served += 1,
        }
    }
}

/// Worker-side accept loop. Called from a worker thread when its re-executed
/// script reaches `app.listen(...)`. The thread-local `WEB_WORKER_CTX` carries
/// the shared listener and shutdown flag. The `app` argument is the worker's
/// own freshly-built app (with its own independent copy of the route table),
/// so dispatch uses the worker's handlers - which is exactly the parallelism we
/// want.
fn web_listen_worker(ctx: &mut CallContext, app: &Rc<WebApp>) -> Object {
    let wctx = WEB_WORKER_CTX.with(|c| {
        c.borrow().as_ref().map(|w| WebWorkerCtx {
            server: w.server.clone(),
            shutdown: w.shutdown.clone(),
            id: w.id,
        })
    });
    let wctx = match wctx {
        Some(w) => w,
        None => return new_error(ctx.pos.clone(), "web.listen: worker context missing"),
    };

    let timeout = std::time::Duration::from_millis(100);
    let active_streams: ActiveStreams = Rc::new(RefCell::new(Vec::new()));
    loop {
        if wctx.shutdown.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }
        ctx.vm().drain_async_completions();
        poll_active_streams(&active_streams);
        let request = match wctx.server.recv_timeout(timeout) {
            Ok(Some(r)) => r,
            Ok(None) => {
                ctx.vm().drain_async_completions();
                poll_active_streams(&active_streams);
                continue;
            }
            Err(_) => break, // listener gone
        };
        if let Err(_e) = web_handle_request(ctx, app, request, None, Some(active_streams.clone())) {
            // Handler threw; web_handle_request already responded 500.
        }
    }
    Object::Undefined
}

/// Concurrent (prefork-style) listen path, run on the main thread.
///
/// 1. Wrap the bound listener in `Arc` and store it (plus a shutdown flag) on
///    the app so `app.close()` can signal workers.
/// 2. Spawn `workers` threads. Each thread:
///    - Sets the thread-local worker context (shared server + shutdown).
///    - Builds an independent `Session` (its own VM, globals, module cache).
///    - Re-runs the user's script. Its top-level statements rebuild the route
///      table; the final `app.listen(...)` is intercepted and becomes the
///      worker's accept loop.
/// 3. Install a Ctrl+C handler that flips the shutdown flag.
/// 4. Join all workers, then clean up.
///
/// Each worker's VM is single-threaded and owns its `Object` graph, so the
/// non-`Send` constraint on `Object` is never violated: live `Object`s never
/// cross a thread boundary. Only the `tiny_http::Server` (which is
/// `Send + Sync`) is shared.
fn web_listen_concurrent(
    ctx: &mut CallContext,
    app: &Rc<WebApp>,
    server: tiny_http::Server,
    workers: usize,
    result_obj: Rc<RefCell<HashData>>,
) -> Object {
    use crate::runtime::Session;

    let shared = std::sync::Arc::new(server);
    let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

    // Publish on the app so app.close() / Ctrl+C can reach workers.
    *app.shared_server.borrow_mut() = Some(shared.clone());
    *app.shutdown_signal.borrow_mut() = Some(shutdown.clone());

    // Locate the script source so each worker can re-run its top-level.
    // bootstrap_source holds the entry script path set by run_file_with_options.
    let script_path = ctx.env.borrow().vm.bootstrap_source.borrow().clone();
    let script_source = if script_path.is_empty() {
        None
    } else {
        fs::read_to_string(&script_path).ok()
    };
    let (script_path, script_source) = match script_source {
        Some(src) => (script_path, src),
        None => {
            // Can't reload - fall back to a single worker on this thread using
            // the already-bound server via the serial loop.
            web_listen_serial(ctx, app, usize::MAX, Some(shutdown.clone()));
            *app.shared_server.borrow_mut() = None;
            *app.shutdown_signal.borrow_mut() = None;
            return Object::Hash(result_obj);
        }
    };
    let script_pathbuf = std::path::PathBuf::from(&script_path);

    // Install Ctrl+C handler (best-effort; ignored if a handler is already set).
    let shutdown_for_sig = shutdown.clone();
    let _ = ctrlc_set_flag(shutdown_for_sig);

    // Spawn workers.
    let mut handles = Vec::with_capacity(workers);
    for id in 0..workers {
        let shared = shared.clone();
        let shutdown = shutdown.clone();
        let script_source = script_source.clone();
        let script_pathbuf = script_pathbuf.clone();
        let handle = std::thread::Builder::new()
            .name(format!("gts-web-worker-{}", id))
            .spawn(move || {
                // Publish the worker context for this thread so the re-executed
                // script's app.listen() is intercepted.
                WEB_WORKER_CTX.with(|c| {
                    *c.borrow_mut() = Some(WebWorkerCtx {
                        server: shared.clone(),
                        shutdown: shutdown.clone(),
                        id,
                    });
                });
                // Each worker gets a fully independent VM + globals + module
                // cache. Re-running the script rebuilds the route table inside
                // this isolated VM; the final listen() call becomes our accept
                // loop via web_listen_worker.
                let session = Session::new();
                let _ = session.run_source(&script_source, &script_pathbuf);
            })
            .expect("spawn web worker");
        handles.push(handle);
    }

    // Wait for all workers to finish (they exit on shutdown).
    for h in handles {
        let _ = h.join();
    }

    // Clean up.
    *app.shared_server.borrow_mut() = None;
    *app.shutdown_signal.borrow_mut() = None;
    Object::Hash(result_obj)
}
