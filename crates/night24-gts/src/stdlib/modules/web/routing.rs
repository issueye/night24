use std::rc::Rc;

use crate::object::{new_error, CallContext, Object};
use crate::stdlib::helpers::{is_callable, ArgReader};

use super::WebApp;

/// A registered route: method filter, path pattern (with `:param` segments),
/// and the ordered list of handler/middleware functions.
pub(super) struct WebRoute {
    pub(super) method: String,        // GET/POST/.../ALL/USE
    pub(super) segments: Vec<String>, // split path, each segment possibly ":name"
    pub(super) handlers: Vec<Object>,
    pub(super) websocket: bool,
}

/// `app.METHOD(path, ...handlers)` or `app.METHOD(path, handler)`.
pub(super) fn web_register_route(
    ctx: &mut CallContext,
    app: &Rc<WebApp>,
    method: &str,
    args: &[Object],
) -> Object {
    if args.len() < 2 {
        return new_error(
            ctx.pos.clone(),
            format!(
                "web.{} requires path and handler",
                method.to_ascii_lowercase()
            ),
        );
    }
    let path = match &args[0] {
        Object::String(s) => s.to_string(),
        _ => {
            return new_error(
                ctx.pos.clone(),
                format!("web.{}: path must be a string", method.to_ascii_lowercase()),
            )
        }
    };
    let handlers = callable_handlers(&args[1..]);
    if handlers.is_empty() {
        return new_error(
            ctx.pos.clone(),
            format!(
                "web.{}: handler must be a function",
                method.to_ascii_lowercase()
            ),
        );
    }
    app.routes.borrow_mut().push(WebRoute {
        method: method.to_string(),
        segments: split_route_path(&path),
        handlers,
        websocket: false,
    });
    Object::Undefined
}

/// `app.use([path], ...handlers)` registers middleware. Path defaults to "/".
pub(super) fn web_use(ctx: &mut CallContext, app: &Rc<WebApp>, args: &[Object]) -> Object {
    let mut path = "/".to_string();
    let mut start = 0;
    if let Some(Object::String(s)) = args.first() {
        path = s.to_string();
        start = 1;
    }
    let handlers = callable_handlers(&args[start..]);
    if handlers.is_empty() {
        return new_error(ctx.pos.clone(), "web.use requires a handler");
    }
    app.routes.borrow_mut().push(WebRoute {
        method: "USE".to_string(),
        segments: split_route_path(&path),
        handlers,
        websocket: false,
    });
    Object::Undefined
}

/// `app.ws(path, handler)` registers a WebSocket endpoint on the same web
/// listener. Handler signature: `(conn, req)`.
pub(super) fn web_ws(ctx: &mut CallContext, app: &Rc<WebApp>, args: &[Object]) -> Object {
    if args.len() < 2 {
        return new_error(ctx.pos.clone(), "web.ws requires path and handler");
    }
    let path = match &args[0] {
        Object::String(s) => s.to_string(),
        _ => return new_error(ctx.pos.clone(), "web.ws: path must be a string"),
    };
    let reader = ArgReader::new(ctx, "web.ws", args);
    let handler = match reader.required_callable(1, "handler") {
        Ok(handler) => handler,
        Err(err) => return err,
    };
    app.routes.borrow_mut().push(WebRoute {
        method: "GET".to_string(),
        segments: split_route_path(&path),
        handlers: vec![handler],
        websocket: true,
    });
    Object::Undefined
}

fn split_route_path(path: &str) -> Vec<String> {
    path.split('/')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

fn callable_handlers(args: &[Object]) -> Vec<Object> {
    args.iter().filter(|h| is_callable(h)).cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::str_obj;
    use crate::stdlib::helpers::native;

    #[test]
    fn split_route_path_ignores_empty_segments() {
        assert_eq!(split_route_path("/api/:id/"), vec!["api", ":id"]);
        assert_eq!(split_route_path("//"), Vec::<String>::new());
    }

    #[test]
    fn callable_handlers_filters_non_callables() {
        let handler = native("web.test", |_ctx, _args| Object::Undefined);
        let handlers = callable_handlers(&[str_obj("not-callable"), handler.clone(), Object::Null]);

        assert_eq!(handlers.len(), 1);
        assert!(matches!(&handlers[0], Object::Builtin(_)));
    }
}
