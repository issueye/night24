use std::env;

use super::super::helpers::*;
use crate::object::{new_error, num_obj, str_obj, CallContext, Object};

pub(crate) fn os_module() -> Object {
    module(vec![
        ("platform", str_obj(env::consts::OS)),
        ("arch", str_obj(env::consts::ARCH)),
        ("eol", str_obj(if cfg!(windows) { "\r\n" } else { "\n" })),
        ("type", native("os.type", os_type)),
        (
            "release",
            native("os.release", |_ctx, _args| {
                str_obj(format!("{}/{}", env::consts::OS, env::consts::ARCH))
            }),
        ),
        ("homedir", native("os.homedir", os_homedir)),
        (
            "tmpdir",
            native("os.tmpdir", |_ctx, _args| {
                str_obj(env::temp_dir().to_string_lossy())
            }),
        ),
        ("hostname", native("os.hostname", os_hostname)),
        (
            "cpus",
            native("os.cpus", |_ctx, _args| {
                num_obj(
                    std::thread::available_parallelism()
                        .map(|n| n.get())
                        .unwrap_or(1) as f64,
                )
            }),
        ),
        ("userInfo", native("os.userInfo", os_user_info)),
    ])
}

pub(crate) fn os_type(_ctx: &mut CallContext, _args: &[Object]) -> Object {
    str_obj(match env::consts::OS {
        "windows" => "Windows_NT",
        "macos" => "Darwin",
        "linux" => "Linux",
        other => other,
    })
}

pub(crate) fn os_homedir(ctx: &mut CallContext, _args: &[Object]) -> Object {
    match env::var("USERPROFILE").or_else(|_| env::var("HOME")) {
        Ok(home) => str_obj(home),
        Err(e) => new_error(ctx.pos.clone(), format!("os.homedir: {}", e)),
    }
}

pub(crate) fn os_hostname(_ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Ok(name) = env::var("COMPUTERNAME").or_else(|_| env::var("HOSTNAME")) {
        return str_obj(name);
    }
    str_obj("")
}

pub(crate) fn os_user_info(_ctx: &mut CallContext, _args: &[Object]) -> Object {
    let username = env::var("USERNAME")
        .or_else(|_| env::var("USER"))
        .unwrap_or_default();
    let home = env::var("USERPROFILE")
        .or_else(|_| env::var("HOME"))
        .unwrap_or_default();
    module(vec![
        ("uid", str_obj("")),
        ("gid", str_obj("")),
        ("username", str_obj(username.clone())),
        ("name", str_obj(username)),
        ("homedir", str_obj(home)),
    ])
}
