//! @std/gtp/client - GTP client module
//!
//! Provides functions to connect to GTP servers over various transports.

use crate::gtp::frame::{Frame, Value};
use crate::gtp::transport::Transport;
use crate::gtp::transports::TcpTransport;
use crate::object::{bool_obj, new_error, num_obj, str_obj, CallContext, HashData, Object};
use std::cell::RefCell;
use std::rc::Rc;

/// Create the @std/gtp/client module
pub fn gtp_client_module() -> Object {
    let mut exports = HashData::default();

    exports.set("connectTcp", native_fn("gtp.connectTcp", gtp_connect_tcp));
    exports.set("connect", native_fn("gtp.connect", gtp_connect));

    Object::Hash(Rc::new(RefCell::new(exports)))
}

/// Helper to create a native function
fn native_fn(name: &str, f: fn(&mut CallContext, &[Object]) -> Object) -> Object {
    use crate::object::Builtin;
    Object::Builtin(Rc::new(Builtin {
        name: name.to_string(),
        func: Rc::new(f),
        extra: None,
    }))
}

/// Connect to a GTP server via TCP
fn gtp_connect_tcp(ctx: &mut CallContext, args: &[Object]) -> Object {
    // Get address argument
    let addr = match args.first() {
        Some(Object::String(s)) => s.as_ref().clone(),
        Some(other) => {
            return new_error(
                ctx.pos.clone(),
                format!(
                    "gtp.connectTcp: expected string address, got {}",
                    other.inspect()
                ),
            );
        }
        None => {
            return new_error(ctx.pos.clone(), "gtp.connectTcp: missing address argument");
        }
    };

    // Connect
    let transport = match TcpTransport::connect(&addr) {
        Ok(t) => t,
        Err(e) => {
            return new_error(
                ctx.pos.clone(),
                format!("gtp.connectTcp: failed to connect to {}: {}", addr, e),
            );
        }
    };

    // Create connection object
    create_gtp_connection_object(Box::new(transport))
}

/// Auto-detect transport from URL and connect
fn gtp_connect(ctx: &mut CallContext, args: &[Object]) -> Object {
    let url = match args.first() {
        Some(Object::String(s)) => s.as_ref().clone(),
        Some(other) => {
            return new_error(
                ctx.pos.clone(),
                format!("gtp.connect: expected string URL, got {}", other.inspect()),
            );
        }
        None => {
            return new_error(ctx.pos.clone(), "gtp.connect: missing URL argument");
        }
    };

    // Parse URL scheme
    if url.starts_with("tcp://") {
        let addr = url.strip_prefix("tcp://").unwrap();
        let transport = match TcpTransport::connect(addr) {
            Ok(t) => t,
            Err(e) => {
                return new_error(
                    ctx.pos.clone(),
                    format!("gtp.connect: failed to connect to {}: {}", addr, e),
                );
            }
        };
        create_gtp_connection_object(Box::new(transport))
    } else if url.starts_with("ws://") || url.starts_with("wss://") {
        new_error(
            ctx.pos.clone(),
            "gtp.connect: WebSocket transport not yet implemented",
        )
    } else {
        new_error(
            ctx.pos.clone(),
            format!("gtp.connect: unsupported URL scheme: {}", url),
        )
    }
}

/// State key for storing the transport in the connection object
const GTP_CONN_KEY: &str = "__gtp_transport__";

/// Create a GTP connection object
fn create_gtp_connection_object(transport: Box<dyn Transport>) -> Object {
    use crate::object::Builtin;

    let conn = Rc::new(RefCell::new(transport));
    let obj = Rc::new(RefCell::new(HashData::default()));

    // Store the transport reference in the object's internal state
    obj.borrow_mut().set(
        GTP_CONN_KEY,
        Object::String(Rc::new("internal".to_string())), // Placeholder
    );

    // Add methods using Builtin
    let c = conn.clone();
    obj.borrow_mut().set(
        "call",
        Object::Builtin(Rc::new(Builtin {
            name: "gtp.call".to_string(),
            func: Rc::new(move |ctx, args| gtp_call(ctx, &c, args)),
            extra: None,
        })),
    );

    let c = conn.clone();
    obj.borrow_mut().set(
        "send",
        Object::Builtin(Rc::new(Builtin {
            name: "gtp.send".to_string(),
            func: Rc::new(move |ctx, args| gtp_send(ctx, &c, args)),
            extra: None,
        })),
    );

    let c = conn.clone();
    obj.borrow_mut().set(
        "recv",
        Object::Builtin(Rc::new(Builtin {
            name: "gtp.recv".to_string(),
            func: Rc::new(move |ctx, args| gtp_recv(ctx, &c, args)),
            extra: None,
        })),
    );

    let c = conn.clone();
    obj.borrow_mut().set(
        "close",
        Object::Builtin(Rc::new(Builtin {
            name: "gtp.close".to_string(),
            func: Rc::new(move |_ctx, _args| {
                let _ = c.borrow_mut().close();
                Object::Undefined
            }),
            extra: None,
        })),
    );

    let c = conn.clone();
    obj.borrow_mut().set(
        "isAlive",
        Object::Builtin(Rc::new(Builtin {
            name: "gtp.isAlive".to_string(),
            func: Rc::new(move |_ctx, _args| bool_obj(c.borrow().is_alive())),
            extra: None,
        })),
    );

    Object::Hash(obj)
}

/// Call a remote method
fn gtp_call(
    ctx: &mut CallContext,
    conn: &Rc<RefCell<Box<dyn Transport>>>,
    args: &[Object],
) -> Object {
    // Parse arguments: module, method, [args]
    let module = match args.first() {
        Some(Object::String(s)) => s.as_ref().clone(),
        _ => return new_error(ctx.pos.clone(), "gtp.call: missing module argument"),
    };

    let method = match args.get(1) {
        Some(Object::String(s)) => s.as_ref().clone(),
        _ => return new_error(ctx.pos.clone(), "gtp.call: missing method argument"),
    };

    let call_args = args.get(2..).unwrap_or(&[]).to_vec();

    // Convert Object args to GTP Values
    let gtp_args: Vec<Value> = call_args.iter().map(object_to_gtp_value).collect();

    // Generate frame ID
    let id = format!(
        "call-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );

    // Create call frame
    let frame = Frame::call(id.clone(), module, method, gtp_args);

    // Send
    if let Err(e) = conn.borrow_mut().send_frame(&frame) {
        return new_error(ctx.pos.clone(), format!("gtp.call: send failed: {}", e));
    }

    // Receive result
    let result_frame = match conn.borrow_mut().recv_frame() {
        Ok(f) => f,
        Err(e) => return new_error(ctx.pos.clone(), format!("gtp.call: recv failed: {}", e)),
    };

    // Check result
    if result_frame.ok == Some(true) {
        result_frame
            .result
            .as_ref()
            .map(gtp_value_to_object)
            .unwrap_or(Object::Undefined)
    } else if let Some(err) = result_frame.error {
        new_error(ctx.pos.clone(), format!("{}: {}", err.name, err.message))
    } else {
        new_error(ctx.pos.clone(), "gtp.call: invalid result frame")
    }
}

/// Send a raw frame
fn gtp_send(
    ctx: &mut CallContext,
    _conn: &Rc<RefCell<Box<dyn Transport>>>,
    _args: &[Object],
) -> Object {
    // TODO: Parse frame from object
    new_error(ctx.pos.clone(), "gtp.send: not yet implemented")
}

/// Receive a raw frame
fn gtp_recv(
    ctx: &mut CallContext,
    conn: &Rc<RefCell<Box<dyn Transport>>>,
    _args: &[Object],
) -> Object {
    match conn.borrow_mut().recv_frame() {
        Ok(frame) => {
            // Convert frame to object
            // TODO: Full frame to object conversion
            let obj = Rc::new(RefCell::new(HashData::default()));
            obj.borrow_mut()
                .set("type", str_obj(frame.frame_type.clone()));
            obj.borrow_mut().set("id", str_obj(frame.id.clone()));
            Object::Hash(obj)
        }
        Err(e) => new_error(ctx.pos.clone(), format!("gtp.recv: {}", e)),
    }
}

// ============================================================================
// Type conversion helpers
// ============================================================================

/// Convert Object to GTP Value
fn object_to_gtp_value(obj: &Object) -> Value {
    match obj {
        Object::Undefined => Value::undefined(),
        Object::Null => Value::null(),
        Object::Boolean(b) => Value::boolean(*b),
        Object::Number(n) => Value::number(*n),
        Object::String(s) => Value::string(s.as_ref().clone()),
        Object::Array(arr) => {
            let items: Vec<Value> = arr
                .borrow()
                .elements
                .iter()
                .map(object_to_gtp_value)
                .collect();
            Value::array(items)
        }
        Object::Hash(_h) => {
            let fields = std::collections::HashMap::new();
            // Note: HashData doesn't expose iteration, so this is simplified
            // In a full implementation, we'd need to iterate over fields
            Value::object(fields)
        }
        _ => Value::string(format!("<{}>", obj.type_tag())),
    }
}

/// Convert GTP Value to Object
fn gtp_value_to_object(val: &Value) -> Object {
    match val.value_type.as_str() {
        "undefined" => Object::Undefined,
        "null" => Object::Null,
        "boolean" => {
            if let Some(serde_json::Value::Bool(b)) = &val.v {
                bool_obj(*b)
            } else {
                Object::Undefined
            }
        }
        "number" => {
            if let Some(ref special) = val.special {
                match special.as_str() {
                    "NaN" => num_obj(f64::NAN),
                    "Infinity" => num_obj(f64::INFINITY),
                    "-Infinity" => num_obj(f64::NEG_INFINITY),
                    _ => Object::Undefined,
                }
            } else if let Some(serde_json::Value::Number(n)) = &val.v {
                num_obj(n.as_f64().unwrap_or(0.0))
            } else {
                Object::Undefined
            }
        }
        "string" => {
            if let Some(serde_json::Value::String(s)) = &val.v {
                str_obj(s.clone())
            } else {
                Object::Undefined
            }
        }
        // TODO: array, object, bytes, resource, error
        _ => Object::Undefined,
    }
}
