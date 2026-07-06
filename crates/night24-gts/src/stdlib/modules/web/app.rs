use std::cell::RefCell;
use std::rc::Rc;

use super::routing::{web_register_route, web_use, web_ws, WebRoute};
use super::workers::{signal_current_worker_shutdown, web_listen};
use crate::object::{CallContext, Object};
use crate::stdlib::helpers::{native, ObjectBuilder};

/// App state: ordered routes + a tiny_http server bound on listen().
///
/// - `server`: used by the serial path (`count: N`). Set on listen, cleared on
///   return.
/// - `shared_server`: used by the concurrent path (`workers: N`). An `Arc` so
///   multiple worker threads can call `recv_timeout` on the same listener.
/// - `shutdown_signal`: set by `app.close()` (or Ctrl+C) to ask all workers to
///   exit their accept loops. `None` when running serially.
pub(super) struct WebApp {
    pub(super) routes: RefCell<Vec<WebRoute>>,
    pub(super) server: RefCell<Option<tiny_http::Server>>,
    pub(super) shared_server: RefCell<Option<std::sync::Arc<tiny_http::Server>>>,
    pub(super) shutdown_signal: RefCell<Option<std::sync::Arc<std::sync::atomic::AtomicBool>>>,
}

const WEB_APP_STATE_KEY: &str = "__web_app__";

pub(super) fn web_create_app(_ctx: &mut CallContext, _args: &[Object]) -> Object {
    let app = Rc::new(WebApp {
        routes: RefCell::new(Vec::new()),
        server: RefCell::new(None),
        shared_server: RefCell::new(None),
        shutdown_signal: RefCell::new(None),
    });
    let obj = ObjectBuilder::new()
        .set(WEB_APP_STATE_KEY, ObjectBuilder::new().build())
        .into_shared();

    // Register HTTP-method route helpers: get/post/put/patch/delete/all.
    for method in ["get", "post", "put", "patch", "delete", "all"] {
        let m = method.to_string();
        let a = app.clone();
        let upper = m.to_ascii_uppercase();
        obj.borrow_mut().set(
            m.as_str(),
            native("web.route", move |ctx, args| {
                web_register_route(ctx, &a, &upper, args)
            }),
        );
    }

    let a = app.clone();
    obj.borrow_mut().set(
        "use",
        native("web.use", move |ctx, args| web_use(ctx, &a, args)),
    );

    let a = app.clone();
    obj.borrow_mut().set(
        "ws",
        native("web.ws", move |ctx, args| web_ws(ctx, &a, args)),
    );

    let a = app.clone();
    obj.borrow_mut().set(
        "listen",
        native("web.listen", move |ctx, args| web_listen(ctx, &a, args)),
    );

    let a = app.clone();
    obj.borrow_mut().set(
        "close",
        native("web.close", move |_ctx, _args| {
            // Serial path: just drop the owned server (original behaviour).
            *a.server.borrow_mut() = None;
            // Concurrent path on the MAIN thread: signal workers via the app's
            // published shutdown flag + unblock any parked recv().
            if let Some(flag) = a.shutdown_signal.borrow().as_ref() {
                flag.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            if let Some(srv) = a.shared_server.borrow().as_ref() {
                srv.unblock();
            }
            // Concurrent path inside a WORKER thread: this app is the worker's
            // own instance, so its shutdown_signal is None. Reach for the
            // shared shutdown flag published via thread-local instead, so
            // `app.close()` called from a handler stops all workers.
            signal_current_worker_shutdown();
            Object::Undefined
        }),
    );

    Object::Hash(obj)
}
