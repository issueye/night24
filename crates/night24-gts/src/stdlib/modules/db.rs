use std::rc::Rc;

use super::super::helpers::*;
use crate::object::{bool_obj, new_error, num_obj, str_obj, CallContext, Object};

pub(crate) const DB_STATE_KEY: &str = "__db_conn__";

pub(crate) fn db_module() -> Object {
    module(vec![
        ("open", native("db.open", db_open)),
        (
            "drivers",
            array(vec![str_obj("sqlite"), str_obj("sqlite3")]),
        ),
    ])
}

pub(crate) fn db_open(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "db.open", args);
    let driver = match reader.required_string(0, "driver") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let dsn = match reader.required_string(1, "dsn") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let driver_lower = driver.to_ascii_lowercase();
    if driver_lower != "sqlite" && driver_lower != "sqlite3" {
        return new_error(
            ctx.pos.clone(),
            format!(
                "db.open: unsupported driver \"{}\" (Rust port supports sqlite only)",
                driver
            ),
        );
    }
    let conn = match rusqlite::Connection::open(&dsn) {
        Ok(c) => c,
        Err(e) => return new_error(ctx.pos.clone(), format!("db.open: {}", e)),
    };
    let conn = Rc::new(std::cell::UnsafeCell::new(conn));
    db_connection_object(conn, driver_lower, dsn)
}

pub(crate) fn db_connection_object(conn: DbConn, driver: String, dsn: String) -> Object {
    // Sentinel marker so callers can identify a connection handle if needed.
    let obj = ObjectBuilder::new()
        .set(DB_STATE_KEY, ObjectBuilder::new().build())
        .set("driver", str_obj(driver.clone()))
        .set("dsn", str_obj(dsn.clone()))
        .into_shared();

    let c = conn.clone();
    obj.borrow_mut().set(
        "exec",
        native("db.exec", move |ctx, args| db_exec(ctx, &c, args)),
    );
    let c = conn.clone();
    obj.borrow_mut().set(
        "query",
        native("db.query", move |ctx, args| db_query(ctx, &c, args)),
    );
    let c = conn.clone();
    obj.borrow_mut().set(
        "queryOne",
        native("db.queryOne", move |ctx, args| db_query_one(ctx, &c, args)),
    );
    let c = conn.clone();
    obj.borrow_mut().set(
        "prepare",
        native("db.prepare", move |ctx, args| db_prepare(ctx, &c, args)),
    );
    let c = conn.clone();
    obj.borrow_mut().set(
        "begin",
        native("db.begin", move |ctx, _args| db_begin(ctx, &c)),
    );
    let _c = conn.clone();
    obj.borrow_mut().set(
        "ping",
        native("db.ping", move |_ctx, _args| {
            // sqlite is in-process; ping always succeeds when the handle is open.
            bool_obj(true)
        }),
    );
    let c = conn.clone();
    obj.borrow_mut()
        .set("close", native("db.close", move |_ctx, _args| db_close(&c)));

    Object::Hash(obj)
}

type DbConn = Rc<std::cell::UnsafeCell<rusqlite::Connection>>;

// Safety: the GTS VM is single-threaded (synchronous tree-walker), so a single
// mutable borrow at a time is guaranteed by the interpreter's call discipline.
unsafe fn conn_ref(conn: &DbConn) -> &rusqlite::Connection {
    &*conn.get()
}

pub(crate) fn db_query_args(
    ctx: &mut CallContext,
    name: &str,
    args: &[Object],
) -> Result<(String, Vec<RusqlParam>), Object> {
    let reader = ArgReader::new(ctx, name, args);
    let query = reader.required_string(0, "query")?;
    let mut params = Vec::new();
    if let Some(arg) = args.get(1) {
        if let Object::Array(arr) = arg {
            for item in &arr.borrow().elements {
                params.push(object_to_sql_param(item));
            }
        } else {
            for item in &args[1..] {
                params.push(object_to_sql_param(item));
            }
        }
    }
    Ok((query, params))
}

pub(crate) enum RusqlParam {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
    Bool(bool),
}

pub(crate) fn object_to_sql_param(obj: &Object) -> RusqlParam {
    match obj {
        Object::Null | Object::Undefined => RusqlParam::Null,
        Object::Boolean(b) => RusqlParam::Bool(*b),
        Object::Number(n) => {
            if *n == n.trunc() && n.abs() < 9.007e15 {
                RusqlParam::Int(*n as i64)
            } else {
                RusqlParam::Real(*n)
            }
        }
        Object::String(s) => RusqlParam::Text(s.to_string()),
        _ => RusqlParam::Text(obj.inspect()),
    }
}

impl rusqlite::ToSql for RusqlParam {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        match self {
            RusqlParam::Null => Ok(rusqlite::types::Null.to_sql()?),
            RusqlParam::Int(i) => Ok((*i).into()),
            RusqlParam::Real(f) => Ok((*f).into()),
            RusqlParam::Text(s) => Ok(s.as_str().into()),
            RusqlParam::Bool(b) => Ok((*b as i64).into()),
        }
    }
}

pub(crate) fn to_sql_refs(params: &[RusqlParam]) -> Vec<&dyn rusqlite::ToSql> {
    params.iter().map(|p| p as &dyn rusqlite::ToSql).collect()
}

pub(crate) fn db_exec(ctx: &mut CallContext, conn: &DbConn, args: &[Object]) -> Object {
    let (query, params) = match db_query_args(ctx, "db.exec", args) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let refs = to_sql_refs(&params);
    let result = unsafe { conn_ref(conn).execute(&query, refs.as_slice()) };
    match result {
        Ok(affected) => ObjectBuilder::new()
            .set("rowsAffected", num_obj(affected as f64))
            .set("lastInsertId", num_obj(0.0))
            .build(),
        Err(e) => new_error(ctx.pos.clone(), format!("db.exec: {}", e)),
    }
}

pub(crate) fn db_query(ctx: &mut CallContext, conn: &DbConn, args: &[Object]) -> Object {
    let (query, params) = match db_query_args(ctx, "db.query", args) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let refs = to_sql_refs(&params);
    let rows_result = unsafe {
        conn_ref(conn).prepare(&query).and_then(|mut stmt| {
            stmt.query_map(refs.as_slice(), row_to_hash)
                .and_then(|mapped| {
                    let mut out: Vec<Object> = Vec::new();
                    for r in mapped {
                        out.push(r?);
                    }
                    Ok(out)
                })
        })
    };
    match rows_result {
        Ok(rows) => array(rows),
        Err(e) => new_error(ctx.pos.clone(), format!("db.query: {}", e)),
    }
}

pub(crate) fn row_to_hash(row: &rusqlite::Row<'_>) -> rusqlite::Result<Object> {
    let col_count = row.as_ref().column_count();
    let mut builder = ObjectBuilder::new();
    for i in 0..col_count {
        let name = row.as_ref().column_name(i)?.to_string();
        let value: rusqlite::types::Value = row.get(i).unwrap_or(rusqlite::types::Value::Null);
        let obj = match value {
            rusqlite::types::Value::Null => Object::Null,
            rusqlite::types::Value::Integer(i) => num_obj(i as f64),
            rusqlite::types::Value::Real(f) => num_obj(f),
            rusqlite::types::Value::Text(s) => str_obj(s),
            rusqlite::types::Value::Blob(b) => str_obj(String::from_utf8_lossy(&b).to_string()),
        };
        builder.insert(name, obj);
    }
    Ok(builder.build())
}

pub(crate) fn db_query_one(ctx: &mut CallContext, conn: &DbConn, args: &[Object]) -> Object {
    let result = db_query(ctx, conn, args);
    if result.is_runtime_error() {
        return result;
    }
    if let Object::Array(arr) = result {
        let elements = &arr.borrow().elements;
        if elements.is_empty() {
            return Object::Null;
        }
        return elements[0].clone();
    }
    Object::Null
}

pub(crate) fn db_prepare(ctx: &mut CallContext, conn: &DbConn, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "db.prepare", args);
    let query = match reader.required_string(0, "query") {
        Ok(v) => v,
        Err(e) => return e,
    };
    // We can't safely stash a rusqlite::Statement across calls without lifetime
    // gymnastics; provide a lightweight prepared-statement facade that re-parses
    // per call. Behaviourally equivalent for the synchronous VM.
    let conn_clone = conn.clone();
    let stmt_obj = ObjectBuilder::new().into_shared();
    let q = query.clone();
    let c = conn_clone.clone();
    stmt_obj.borrow_mut().set(
        "exec",
        native("db.stmt.exec", move |ctx, args| {
            let params: Vec<RusqlParam> = match args.first() {
                Some(Object::Array(arr)) => arr
                    .borrow()
                    .elements
                    .iter()
                    .map(object_to_sql_param)
                    .collect(),
                _ => args.iter().map(object_to_sql_param).collect(),
            };
            let refs = to_sql_refs(&params);
            match unsafe { conn_ref(&c).execute(&q, refs.as_slice()) } {
                Ok(n) => ObjectBuilder::new()
                    .set("rowsAffected", num_obj(n as f64))
                    .build(),
                Err(e) => new_error(ctx.pos.clone(), format!("db.stmt.exec: {}", e)),
            }
        }),
    );
    let q = query.clone();
    let c = conn_clone.clone();
    stmt_obj.borrow_mut().set(
        "query",
        native("db.stmt.query", move |ctx, args| {
            let params: Vec<RusqlParam> = match args.first() {
                Some(Object::Array(arr)) => arr
                    .borrow()
                    .elements
                    .iter()
                    .map(object_to_sql_param)
                    .collect(),
                _ => args.iter().map(object_to_sql_param).collect(),
            };
            let refs = to_sql_refs(&params);
            let res = unsafe {
                conn_ref(&c).prepare(&q).and_then(|mut stmt| {
                    stmt.query_map(refs.as_slice(), row_to_hash)
                        .and_then(|mapped| {
                            let mut out: Vec<Object> = Vec::new();
                            for r in mapped {
                                out.push(r?);
                            }
                            Ok(out)
                        })
                })
            };
            match res {
                Ok(rows) => array(rows),
                Err(e) => new_error(ctx.pos.clone(), format!("db.stmt.query: {}", e)),
            }
        }),
    );
    let q = query.clone();
    let c = conn_clone.clone();
    stmt_obj.borrow_mut().set(
        "queryOne",
        native("db.stmt.queryOne", move |ctx, args| {
            let params: Vec<RusqlParam> = match args.first() {
                Some(Object::Array(arr)) => arr
                    .borrow()
                    .elements
                    .iter()
                    .map(object_to_sql_param)
                    .collect(),
                _ => args.iter().map(object_to_sql_param).collect(),
            };
            let refs = to_sql_refs(&params);
            let res = unsafe {
                conn_ref(&c).prepare(&q).and_then(|mut stmt| {
                    stmt.query_map(refs.as_slice(), row_to_hash)
                        .and_then(|mapped| {
                            let mut out: Vec<Object> = Vec::new();
                            for r in mapped {
                                out.push(r?);
                            }
                            Ok(out)
                        })
                })
            };
            match res {
                Ok(rows) => {
                    if rows.is_empty() {
                        Object::Null
                    } else {
                        rows[0].clone()
                    }
                }
                Err(e) => new_error(ctx.pos.clone(), format!("db.stmt.queryOne: {}", e)),
            }
        }),
    );
    Object::Hash(stmt_obj)
}

pub(crate) fn db_begin(ctx: &mut CallContext, conn: &DbConn) -> Object {
    // sqlite transaction with unchecked borrow. We emulate by executing
    // "BEGIN" and returning a tx facade whose commit/rollback run the
    // corresponding SQL.
    let res = unsafe { conn_ref(conn).execute_batch("BEGIN") };
    if let Err(e) = res {
        return new_error(ctx.pos.clone(), format!("db.begin: {}", e));
    }
    let tx_obj = ObjectBuilder::new().into_shared();
    let c = conn.clone();
    tx_obj.borrow_mut().set(
        "exec",
        native("db.tx.exec", move |ctx, args| db_exec(ctx, &c, args)),
    );
    let c = conn.clone();
    tx_obj.borrow_mut().set(
        "query",
        native("db.tx.query", move |ctx, args| db_query(ctx, &c, args)),
    );
    let c = conn.clone();
    tx_obj.borrow_mut().set(
        "queryOne",
        native("db.tx.queryOne", move |ctx, args| {
            db_query_one(ctx, &c, args)
        }),
    );
    let c = conn.clone();
    tx_obj.borrow_mut().set(
        "commit",
        native("db.tx.commit", move |ctx, _args| {
            match unsafe { conn_ref(&c).execute_batch("COMMIT") } {
                Ok(_) => Object::Undefined,
                Err(e) => new_error(ctx.pos.clone(), format!("db.tx.commit: {}", e)),
            }
        }),
    );
    let c = conn.clone();
    tx_obj.borrow_mut().set(
        "rollback",
        native("db.tx.rollback", move |ctx, _args| {
            match unsafe { conn_ref(&c).execute_batch("ROLLBACK") } {
                Ok(_) => Object::Undefined,
                Err(e) => new_error(ctx.pos.clone(), format!("db.tx.rollback: {}", e)),
            }
        }),
    );
    Object::Hash(tx_obj)
}

pub(crate) fn db_close(conn: &DbConn) -> Object {
    // Drop the inner connection by replacing it with a closed handle.
    // We can't take ownership out of the UnsafeCell without unsafe code; rely on
    // the fact that closing on a sqlite handle is best-effort and the OS will
    // reclaim resources on process exit. For correctness, swap in a fresh
    // in-memory connection to invalidate the previous one.
    unsafe {
        let _ = std::ptr::replace(conn.get(), rusqlite::Connection::open_in_memory().unwrap());
    }
    Object::Undefined
}

// ---------------------------------------------------------------------------
// mail: RFC 5322 address / message parsing and formatting (@std/mail)
// ---------------------------------------------------------------------------
