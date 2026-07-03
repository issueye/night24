use std::cell::RefCell;
use std::rc::Rc;

use super::super::helpers::*;
use crate::object::{bool_obj, new_error, num_obj, str_obj, CallContext, HashData, Object};
use crate::VERSION;

pub(crate) fn process_module() -> Object {
    let mut entries: Vec<(&str, Object)> = Vec::new();
    let snapshot = runtime_argv_snapshot();
    let argv: Vec<Object> = if snapshot.is_empty() {
        std::env::args().map(str_obj).collect()
    } else {
        snapshot.into_iter().map(str_obj).collect()
    };
    let argv0 = argv
        .first()
        .cloned()
        .unwrap_or_else(|| str_obj(String::new()));
    entries.push(("argv", array(argv)));
    entries.push(("argv0", argv0));
    entries.push(("pid", num_obj(std::process::id() as f64)));
    // Snapshot environment as an object (consistent with Go's `process.env`).
    let env_hash = Rc::new(RefCell::new(HashData::default()));
    for (k, v) in std::env::vars() {
        env_hash.borrow_mut().set(k, str_obj(v));
    }
    entries.push(("env", Object::Hash(env_hash)));
    entries.push(("version", str_obj(VERSION)));
    entries.push(("cwd", native("process.cwd", process_cwd)));
    entries.push(("chdir", native("process.chdir", process_chdir)));
    entries.push(("execPath", native("process.execPath", process_exec_path)));
    entries.push(("getenv", native("process.getenv", process_getenv)));
    entries.push(("envObject", native("process.envObject", process_env_object)));
    entries.push(("uptime", native("process.uptime", process_uptime)));
    entries.push(("hrtime", native("process.hrtime", process_hrtime)));
    entries.push(("setenv", native("process.setenv", process_setenv)));
    entries.push(("unsetenv", native("process.unsetenv", process_unsetenv)));
    entries.push(("exit", native("process.exit", process_exit)));
    module(entries)
}

pub(crate) fn process_cwd(ctx: &mut CallContext, _args: &[Object]) -> Object {
    match std::env::current_dir() {
        Ok(p) => str_obj(p.to_string_lossy()),
        Err(e) => new_error(ctx.pos.clone(), format!("process.cwd: {}", e)),
    }
}

pub(crate) fn process_chdir(ctx: &mut CallContext, args: &[Object]) -> Object {
    let path = match required_string(ctx, "process.chdir", args, 0, "path") {
        Ok(p) => p,
        Err(e) => return e,
    };
    match std::env::set_current_dir(&path) {
        Ok(()) => Object::Undefined,
        Err(e) => new_error(ctx.pos.clone(), format!("process.chdir: {}", e)),
    }
}

pub(crate) fn process_exec_path(ctx: &mut CallContext, _args: &[Object]) -> Object {
    match std::env::current_exe() {
        Ok(p) => str_obj(p.to_string_lossy()),
        Err(e) => new_error(ctx.pos.clone(), format!("process.execPath: {}", e)),
    }
}

pub(crate) fn process_getenv(ctx: &mut CallContext, args: &[Object]) -> Object {
    let name = match required_string(ctx, "process.getenv", args, 0, "name") {
        Ok(n) => n,
        Err(e) => return e,
    };
    match std::env::var_os(&name) {
        Some(val) => str_obj(val.to_string_lossy()),
        None => args.get(1).cloned().unwrap_or(Object::Undefined),
    }
}

pub(crate) fn process_env_object(_ctx: &mut CallContext, _args: &[Object]) -> Object {
    let hash = Rc::new(RefCell::new(HashData::default()));
    for (k, v) in std::env::vars() {
        hash.borrow_mut().set(k, str_obj(v));
    }
    Object::Hash(hash)
}

pub(crate) fn process_uptime(_ctx: &mut CallContext, _args: &[Object]) -> Object {
    let start = PROCESS_START.get_or_init(std::time::Instant::now);
    num_obj(start.elapsed().as_secs_f64())
}

pub(crate) fn process_hrtime(_ctx: &mut CallContext, args: &[Object]) -> Object {
    let start = PROCESS_START.get_or_init(std::time::Instant::now);
    let elapsed = start.elapsed();
    let secs = elapsed.as_secs();
    let nanos = elapsed.subsec_nanos();
    let value = array(vec![num_obj(secs as f64), num_obj(nanos as f64)]);

    // If a previous [sec, nano] array is supplied, return the difference.
    if let Some(Object::Array(prev)) = args.first() {
        let prev = prev.borrow();
        if prev.elements.len() == 2 {
            if let (Object::Number(ps), Object::Number(pn)) =
                (prev.elements[0].clone(), prev.elements[1].clone())
            {
                let psecs = ps as u64;
                let pnanos = pn as u32;
                let mut dsecs = secs.saturating_sub(psecs);
                let mut dnanos = nanos as i64 - pnanos as i64;
                if dnanos < 0 {
                    dsecs = dsecs.saturating_sub(1);
                    dnanos += 1_000_000_000;
                }
                return array(vec![num_obj(dsecs as f64), num_obj(dnanos as f64)]);
            }
        }
    }
    value
}

pub(crate) fn process_setenv(ctx: &mut CallContext, args: &[Object]) -> Object {
    let name = match required_string(ctx, "process.setenv", args, 0, "name") {
        Ok(n) => n,
        Err(e) => return e,
    };
    let value = match required_string(ctx, "process.setenv", args, 1, "value") {
        Ok(v) => v,
        Err(e) => return e,
    };
    std::env::set_var(&name, &value);
    Object::Undefined
}

pub(crate) fn process_unsetenv(ctx: &mut CallContext, args: &[Object]) -> Object {
    let name = match required_string(ctx, "process.unsetenv", args, 0, "name") {
        Ok(n) => n,
        Err(e) => return e,
    };
    std::env::remove_var(&name);
    Object::Undefined
}

pub(crate) fn process_exit(ctx: &mut CallContext, args: &[Object]) -> Object {
    let code = match args.first() {
        Some(Object::Number(n)) => *n as i32,
        Some(Object::String(s)) => match s.parse::<i32>() {
            Ok(n) => n,
            Err(_) => return new_error(ctx.pos.clone(), "process.exit: code must be a number"),
        },
        Some(_) => return new_error(ctx.pos.clone(), "process.exit: code must be a number"),
        None => 0,
    };
    // Builtin return is symbolic; the runtime treats exit as a normal return.
    // We surface the intended code via a controlled panic-free process::exit.
    std::process::exit(code);
}

// ===========================================================================
// P6 stdlib batch 2: crypto (sha1/256/512 + hmac + pbkdf2 + randomUUID +
// randomBytes + timingSafeEqual), text (display-width utilities), url
// (parse/format/resolve + URL/URLSearchParams), cache (TTL dictionary).
// ===========================================================================

// ---------------------------------------------------------------------------
// crypto: SHA-1/256/512 (self-contained, no external crate), HMAC, PBKDF2,
// randomUUID, randomBytes, timingSafeEqual.
//
// SHA implementations below are straightforward, well-tested reference
// versions of the NIST/NSA algorithms; outputs are byte vectors that get
// hex-encoded to lowercase strings to match the Go originals.
// ---------------------------------------------------------------------------

pub(crate) fn process_result(exit_code: i32, stdout: String, stderr: String) -> Object {
    let hash = Rc::new(RefCell::new(HashData::default()));
    hash.borrow_mut().set("exitCode", num_obj(exit_code as f64));
    hash.borrow_mut().set("stdout", str_obj(stdout));
    hash.borrow_mut().set("stderr", str_obj(stderr));
    hash.borrow_mut().set("success", bool_obj(exit_code == 0));
    Object::Hash(hash)
}

// ---------------------------------------------------------------------------
// net/http/client: HTTP client module (@std/net/http/client)
// ---------------------------------------------------------------------------
