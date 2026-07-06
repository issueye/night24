use super::super::helpers::*;
use crate::object::{bool_obj, new_error, str_obj, CallContext, Object};

/// Options parsed from an optional GTS object: { cwd, argv, autoMain }.
struct RuntimeOpts {
    #[allow(dead_code)]
    cwd: Option<String>,
    argv: Vec<String>,
    auto_main: bool,
}

pub(crate) fn runtime_module() -> Object {
    module(vec![
        ("mode", str_obj(crate::runtime::runtime_mode())),
        ("state", native("runtime.state", runtime_state)),
        ("runScript", native("runtime.runScript", runtime_run_script)),
        (
            "callScript",
            native("runtime.callScript", runtime_call_script),
        ),
        ("runTool", native("runtime.runTool", runtime_run_tool)),
    ])
}

pub(crate) fn runtime_state(_ctx: &mut CallContext, _args: &[Object]) -> Object {
    module(vec![
        ("mode", str_obj(crate::runtime::runtime_mode())),
        ("execMode", str_obj("bytecode")),
        (
            "io",
            str_obj(if cfg!(feature = "tokio") {
                "tokio-io"
            } else {
                "native-io"
            }),
        ),
        ("tokio", bool_obj(cfg!(feature = "tokio"))),
    ])
}

pub(crate) fn runtime_run_script(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "runtime.runScript", args);
    let path = match reader.required_string(0, "path") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let opts = parse_runtime_opts(ctx, "runtime.runScript", args, 1);
    match run_sub_script(&path, &opts) {
        Ok(exports) => exports,
        Err(e) => e,
    }
}

pub(crate) fn runtime_call_script(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "runtime.callScript", args);
    let path = match reader.required_string(0, "path") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let export_name = match reader.required_string(1, "exportName") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let call_args = match args.get(2) {
        Some(Object::Array(arr)) => arr.borrow().elements.clone(),
        Some(Object::Undefined | Object::Null) | None => Vec::new(),
        Some(_) => return new_error(ctx.pos.clone(), "runtime.callScript: args must be an array"),
    };
    let opts = parse_runtime_opts(ctx, "runtime.callScript", args, 3);
    runtime_call_export(
        ctx,
        &path,
        &export_name,
        &call_args,
        &opts,
        "runtime.callScript",
    )
}

pub(crate) fn runtime_run_tool(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "runtime.runTool", args);
    let path = match reader.required_string(0, "path") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let input = args.get(1).cloned().unwrap_or(Object::Undefined);
    let opts = parse_runtime_opts(ctx, "runtime.runTool", args, 2);
    runtime_call_export(ctx, &path, "run", &[input], &opts, "runtime.runTool")
}

/// Run an isolated sub-script and invoke a named export on it. The export's
/// return value (or resolved Promise) is forwarded to the caller.
fn runtime_call_export(
    ctx: &mut CallContext,
    path: &str,
    export_name: &str,
    call_args: &[Object],
    opts: &RuntimeOpts,
    api_name: &str,
) -> Object {
    let exports = match run_sub_script(path, opts) {
        Ok(e) => e,
        Err(err) => return err,
    };
    let export_obj = match &exports {
        Object::Hash(h) => h.borrow().get(export_name).cloned(),
        _ => None,
    };
    let func = match export_obj {
        Some(f) if !matches!(f, Object::Undefined | Object::Null) => f,
        _ => {
            return new_error(
                ctx.pos.clone(),
                format!("{}: {} must export {}(...)", api_name, path, export_name),
            )
        }
    };
    call_script_function(&func, ctx.env, call_args)
}

/// Parse the optional options object for runtime helpers.
fn parse_runtime_opts(
    ctx: &mut CallContext,
    name: &str,
    args: &[Object],
    index: usize,
) -> RuntimeOpts {
    let mut opts = RuntimeOpts {
        cwd: None,
        argv: Vec::new(),
        auto_main: false,
    };
    let reader = ArgReader::new(ctx, name, args);
    if let Some(view) = reader.object_view(index) {
        let opts_view = ObjectView::new(&view);
        if let Some(Object::String(s)) = opts_view.object("cwd") {
            opts.cwd = Some(s.to_string());
        }
        if let Some(Object::Array(arr)) = opts_view.object("argv") {
            opts.argv = arr
                .borrow()
                .elements
                .iter()
                .map(|o| match o {
                    Object::String(s) => s.to_string(),
                    other => other.inspect(),
                })
                .collect();
        }
        if let Some(Object::Boolean(b)) = opts_view.object("autoMain") {
            opts.auto_main = b;
        }
    }
    opts
}

/// Spawn a fresh `Session`, run the file, and return its `module.exports`.
fn run_sub_script(path: &str, opts: &RuntimeOpts) -> crate::runtime::RuntimeResult<Object> {
    use crate::runtime::Session;
    let session = Session::new();
    let argv = if opts.argv.is_empty() {
        vec![std::env::current_exe()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default()]
    } else {
        opts.argv.clone()
    };
    session.run_file_for_exports(path, argv, opts.auto_main)
}

// ---------------------------------------------------------------------------
// image / pdf: placeholder modules aligned with the Go version (@std/image, @std/pdf)
// ---------------------------------------------------------------------------
