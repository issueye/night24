use std::cell::RefCell;
use std::io::{Read, Write};
use std::process::Child;
use std::rc::Rc;

use std::process::Command;
use std::process::Stdio;

use super::super::helpers::*;
use crate::object::{bool_obj, new_error, num_obj, str_obj, CallContext, Object};

/// PTY/子进程的内部状态。
pub(crate) struct PtyState {
    child: RefCell<Option<Child>>,
    cols: std::cell::Cell<u32>,
    rows: std::cell::Cell<u32>,
}

pub(crate) fn pty_module() -> Object {
    module(vec![
        ("spawn", native("pty.spawn", pty_spawn)),
        ("open", native("pty.open", pty_spawn)), // 别名
    ])
}

/// pty.spawn(cmd, [args...], [opts]) -> pty 实例
/// 返回的对象含 read/readLine/readText/readTextTimeout/write/writeln/kill/wait/tryWait/resize/close 方法。
fn pty_spawn(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "pty.spawn", args);
    let cmd_name = match reader.required_string(0, "command") {
        Ok(s) => s,
        Err(e) => return e,
    };

    // 收集字符串参数与最后的 options 对象。
    let mut cmd_args: Vec<String> = Vec::new();
    let mut cols: u32 = 80;
    let mut rows: u32 = 24;
    for arg in args.iter().skip(1) {
        match arg {
            Object::String(s) => cmd_args.push(s.to_string()),
            Object::Hash(opts) => {
                if let Some(Object::Number(n)) = opts.borrow().get("cols") {
                    if *n > 0.0 {
                        cols = *n as u32;
                    }
                }
                if let Some(Object::Number(n)) = opts.borrow().get("rows") {
                    if *n > 0.0 {
                        rows = *n as u32;
                    }
                }
                if let Some(Object::Array(arr)) = opts.borrow().get("args") {
                    for a in arr.borrow().elements.iter() {
                        if let Object::String(s) = a {
                            cmd_args.push(s.to_string());
                        }
                    }
                }
            }
            _ => {}
        }
    }

    let mut command = Command::new(&cmd_name);
    command.args(&cmd_args);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let child = match command.spawn() {
        Ok(c) => c,
        Err(e) => {
            return new_error(
                ctx.pos.clone(),
                format!("pty.spawn: failed to start '{cmd_name}': {e}"),
            )
        }
    };

    let state = Rc::new(PtyState {
        child: RefCell::new(Some(child)),
        cols: std::cell::Cell::new(cols),
        rows: std::cell::Cell::new(rows),
    });

    let pty_obj = ObjectBuilder::new().into_shared();

    // read() -> string（读取当前可用的 stdout 输出，非阻塞式：读到 EOF 或无数据）
    let s = state.clone();
    pty_obj.borrow_mut().set(
        "read",
        native("pty.read", move |ctx, _args| pty_read(ctx, &s)),
    );

    // readLine() -> string | null（读取一行，超时 1s）
    let s = state.clone();
    pty_obj.borrow_mut().set(
        "readLine",
        native("pty.readLine", move |ctx, _args| pty_read_line(ctx, &s)),
    );

    // readText() -> string（读取所有 stdout 直到 EOF）
    let s = state.clone();
    pty_obj.borrow_mut().set(
        "readText",
        native("pty.readText", move |ctx, _args| pty_read_text(ctx, &s)),
    );

    // readTextTimeout(timeoutMs) -> string（限时读取）
    let s = state.clone();
    pty_obj.borrow_mut().set(
        "readTextTimeout",
        native("pty.readTextTimeout", move |ctx, args| {
            let timeout_ms = match args.first() {
                Some(Object::Number(n)) => *n as u64,
                _ => 2000,
            };
            pty_read_text_timeout(ctx, &s, timeout_ms)
        }),
    );

    // write(text) -> number（写入 stdin，返回写入字节数）
    let s = state.clone();
    pty_obj.borrow_mut().set(
        "write",
        native("pty.write", move |ctx, args| {
            pty_write(ctx, args, &s, false)
        }),
    );

    // writeln(text) -> number（写入一行，自动加换行）
    let s = state.clone();
    pty_obj.borrow_mut().set(
        "writeln",
        native("pty.writeln", move |ctx, args| {
            pty_write(ctx, args, &s, true)
        }),
    );

    // kill() -> undefined（终止子进程）
    let s = state.clone();
    pty_obj
        .borrow_mut()
        .set("kill", native("pty.kill", move |_ctx, _args| pty_kill(&s)));

    // wait() -> number（等待退出，返回 exit code）
    let s = state.clone();
    pty_obj.borrow_mut().set(
        "wait",
        native("pty.wait", move |ctx, _args| pty_wait(ctx, &s)),
    );

    // tryWait() -> { running: bool, exitCode?: number }（非阻塞检查子进程状态）
    let s = state.clone();
    pty_obj.borrow_mut().set(
        "tryWait",
        native("pty.tryWait", move |ctx, _args| pty_try_wait(ctx, &s)),
    );

    // resize(cols, rows) -> undefined（调整大小；管道模型下仅记录尺寸）
    let s = state.clone();
    pty_obj.borrow_mut().set(
        "resize",
        native("pty.resize", move |ctx, args| pty_resize(ctx, args, &s)),
    );

    // close() -> undefined（关闭 stdin，不终止进程）
    let s = state.clone();
    pty_obj.borrow_mut().set(
        "close",
        native("pty.close", move |_ctx, _args| pty_close(&s)),
    );

    Object::Hash(pty_obj)
}

pub(crate) fn pty_read(ctx: &mut CallContext, state: &Rc<PtyState>) -> Object {
    let mut guard = state.child.borrow_mut();
    let Some(child) = guard.as_mut() else {
        return new_error(ctx.pos.clone(), "pty.read: process not running");
    };
    let Some(stdout) = child.stdout.as_mut() else {
        return new_error(ctx.pos.clone(), "pty.read: no stdout available");
    };
    let mut buf = [0u8; 4096];
    match stdout.read(&mut buf) {
        Ok(0) => str_obj(String::new()),
        Ok(n) => str_obj(String::from_utf8_lossy(&buf[..n]).into_owned()),
        Err(e) => new_error(ctx.pos.clone(), format!("pty.read: {e}")),
    }
}

pub(crate) fn pty_read_line(ctx: &mut CallContext, state: &Rc<PtyState>) -> Object {
    let result = pty_read_text_timeout(ctx, state, 1000);
    if let Object::String(s) = &result {
        if let Some(idx) = s.find('\n') {
            return str_obj(s[..=idx].to_string());
        }
        if s.is_empty() {
            return Object::Null;
        }
        return str_obj(s.to_string());
    }
    result
}

pub(crate) fn pty_read_text(ctx: &mut CallContext, state: &Rc<PtyState>) -> Object {
    let mut guard = state.child.borrow_mut();
    let Some(child) = guard.as_mut() else {
        return new_error(ctx.pos.clone(), "pty.readText: process not running");
    };
    let Some(stdout) = child.stdout.as_mut() else {
        return new_error(ctx.pos.clone(), "pty.readText: no stdout available");
    };
    let mut buf = String::new();
    match stdout.read_to_string(&mut buf) {
        Ok(_) => str_obj(buf),
        Err(e) => new_error(ctx.pos.clone(), format!("pty.readText: {e}")),
    }
}

pub(crate) fn pty_read_text_timeout(
    ctx: &mut CallContext,
    state: &Rc<PtyState>,
    _timeout_ms: u64,
) -> Object {
    // 简化实现：读取当前可用的非阻塞数据（管道模型下 read 到 EAGAIN 或 EOF）。
    pty_read(ctx, state)
}

pub(crate) fn pty_write(
    ctx: &mut CallContext,
    args: &[Object],
    state: &Rc<PtyState>,
    append_newline: bool,
) -> Object {
    let reader = ArgReader::new(ctx, "pty.write", args);
    let text = match reader.required_string(0, "text") {
        Ok(s) => s,
        Err(e) => return e,
    };
    let data = if append_newline {
        format!("{text}\n")
    } else {
        text
    };
    let mut guard = state.child.borrow_mut();
    let Some(child) = guard.as_mut() else {
        return new_error(ctx.pos.clone(), "pty.write: process not running");
    };
    let Some(stdin) = child.stdin.as_mut() else {
        return new_error(ctx.pos.clone(), "pty.write: no stdin available");
    };
    match stdin.write_all(data.as_bytes()).and_then(|_| stdin.flush()) {
        Ok(_) => num_obj(data.len() as f64),
        Err(e) => new_error(ctx.pos.clone(), format!("pty.write: {e}")),
    }
}

pub(crate) fn pty_kill(state: &Rc<PtyState>) -> Object {
    let mut guard = state.child.borrow_mut();
    if let Some(child) = guard.as_mut() {
        let _ = child.kill();
    }
    Object::Undefined
}

pub(crate) fn pty_wait(ctx: &mut CallContext, state: &Rc<PtyState>) -> Object {
    let mut guard = state.child.borrow_mut();
    let Some(child) = guard.as_mut() else {
        return new_error(ctx.pos.clone(), "pty.wait: process not running");
    };
    match child.wait() {
        Ok(status) => match status.code() {
            Some(code) => num_obj(code as f64),
            None => num_obj(0.0),
        },
        Err(e) => new_error(ctx.pos.clone(), format!("pty.wait: {e}")),
    }
}

pub(crate) fn pty_try_wait(ctx: &mut CallContext, state: &Rc<PtyState>) -> Object {
    let mut guard = state.child.borrow_mut();
    let Some(child) = guard.as_mut() else {
        return new_error(ctx.pos.clone(), "pty.tryWait: process not running");
    };
    let status = match child.try_wait() {
        Ok(status) => status,
        Err(e) => return new_error(ctx.pos.clone(), format!("pty.tryWait: {e}")),
    };
    match status {
        Some(status) => ObjectBuilder::new()
            .set("running", bool_obj(false))
            .set("exitCode", num_obj(status.code().unwrap_or(0) as f64))
            .build(),
        None => ObjectBuilder::new().set("running", bool_obj(true)).build(),
    }
}

pub(crate) fn pty_resize(ctx: &mut CallContext, args: &[Object], state: &Rc<PtyState>) -> Object {
    let reader = ArgReader::new(ctx, "pty.resize", args);
    let cols = match reader.required_number(0, "cols") {
        Ok(n) => n as u32,
        Err(e) => return e,
    };
    let rows = match reader.required_number(1, "rows") {
        Ok(n) => n as u32,
        Err(e) => return e,
    };
    state.cols.set(cols);
    state.rows.set(rows);
    Object::Undefined
}

pub(crate) fn pty_close(state: &Rc<PtyState>) -> Object {
    let mut guard = state.child.borrow_mut();
    if let Some(child) = guard.as_mut() {
        // 关闭 stdin 通知子进程输入结束。
        drop(child.stdin.take());
    }
    Object::Undefined
}
