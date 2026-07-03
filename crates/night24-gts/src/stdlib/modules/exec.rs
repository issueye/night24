use std::cell::RefCell;
use std::rc::Rc;

use super::super::helpers::*;
use super::process::process_result;
use crate::object::{format_number, new_error, str_obj, CallContext, HashData, Object};

pub(crate) fn exec_module() -> Object {
    module(vec![
        ("run", native("exec.run", exec_run)),
        ("output", native("exec.output", exec_output)),
        (
            "combinedOutput",
            native("exec.combinedOutput", exec_combined_output),
        ),
        ("command", native("exec.command", exec_command)),
    ])
}

pub(crate) fn exec_run(ctx: &mut CallContext, args: &[Object]) -> Object {
    use std::process::Command;

    let (cmd_name, cmd_args) = match parse_exec_args(ctx, args) {
        Ok(v) => v,
        Err(e) => return e,
    };

    let output = match Command::new(&cmd_name).args(&cmd_args).output() {
        Ok(o) => o,
        Err(e) => return new_error(ctx.pos.clone(), format!("exec.run: {}", e)),
    };

    let exit_code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    process_result(exit_code, stdout, stderr)
}

pub(crate) fn exec_output(ctx: &mut CallContext, args: &[Object]) -> Object {
    use std::process::Command;

    let (cmd_name, cmd_args) = match parse_exec_args(ctx, args) {
        Ok(v) => v,
        Err(e) => return e,
    };

    match Command::new(&cmd_name).args(&cmd_args).output() {
        Ok(output) => str_obj(String::from_utf8_lossy(&output.stdout).to_string()),
        Err(e) => new_error(ctx.pos.clone(), format!("exec.output: {}", e)),
    }
}

pub(crate) fn exec_combined_output(ctx: &mut CallContext, args: &[Object]) -> Object {
    use std::process::Command;

    let (cmd_name, cmd_args) = match parse_exec_args(ctx, args) {
        Ok(v) => v,
        Err(e) => return e,
    };

    match Command::new(&cmd_name).args(&cmd_args).output() {
        Ok(output) => {
            let combined = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            str_obj(combined)
        }
        Err(e) => new_error(ctx.pos.clone(), format!("exec.combinedOutput: {}", e)),
    }
}

pub(crate) fn run_process(
    cmd_name: &str,
    cmd_args: &[String],
    dir: Option<&str>,
) -> std::io::Result<std::process::Output> {
    let mut command = std::process::Command::new(cmd_name);
    command.args(cmd_args);
    if let Some(dir) = dir {
        command.current_dir(dir);
    }
    command.output()
}

pub(crate) fn exec_command(ctx: &mut CallContext, args: &[Object]) -> Object {
    let (cmd_name, cmd_args) = match parse_exec_args(ctx, args) {
        Ok(v) => v,
        Err(e) => return e,
    };

    #[derive(Clone)]
    struct ExecCommandState {
        dir: Option<String>,
    }

    let state = Rc::new(RefCell::new(ExecCommandState { dir: None }));

    // Return a command builder object with chainable configuration and run/output methods.
    let hash = Rc::new(RefCell::new(HashData::default()));

    let state_for_set_dir = state.clone();
    let builder_for_set_dir = hash.clone();
    hash.borrow_mut().set(
        "setDir",
        native("command.setDir", move |ctx, args| {
            let dir = match required_string(ctx, "command.setDir", args, 0, "dir") {
                Ok(v) => v,
                Err(err) => return err,
            };
            state_for_set_dir.borrow_mut().dir = Some(dir);
            Object::Hash(builder_for_set_dir.clone())
        }),
    );

    let cmd_name_clone = cmd_name.clone();
    let cmd_args_clone = cmd_args.clone();
    let state_for_run = state.clone();
    hash.borrow_mut().set(
        "run",
        native("command.run", move |ctx, _args| {
            let state = state_for_run.borrow();
            let output = match run_process(&cmd_name_clone, &cmd_args_clone, state.dir.as_deref()) {
                Ok(o) => o,
                Err(e) => return new_error(ctx.pos.clone(), format!("command.run: {}", e)),
            };
            let exit_code = output.status.code().unwrap_or(-1);
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            process_result(exit_code, stdout, stderr)
        }),
    );

    let cmd_name_clone2 = cmd_name.clone();
    let cmd_args_clone2 = cmd_args.clone();
    let state_for_output = state.clone();
    hash.borrow_mut().set(
        "output",
        native("command.output", move |ctx, _args| {
            let state = state_for_output.borrow();
            match run_process(&cmd_name_clone2, &cmd_args_clone2, state.dir.as_deref()) {
                Ok(output) => str_obj(String::from_utf8_lossy(&output.stdout).to_string()),
                Err(e) => new_error(ctx.pos.clone(), format!("command.output: {}", e)),
            }
        }),
    );

    Object::Hash(hash)
}

pub(crate) fn parse_exec_args(
    ctx: &mut CallContext,
    args: &[Object],
) -> Result<(String, Vec<String>), Object> {
    if args.is_empty() {
        return Err(new_error(ctx.pos.clone(), "exec requires a command name"));
    }

    let cmd_name = match &args[0] {
        Object::String(s) => s.to_string(),
        _ => {
            return Err(new_error(
                ctx.pos.clone(),
                "exec: first argument must be a string",
            ))
        }
    };

    let cmd_args = if args.len() > 1 {
        // Check if second arg is an array
        if let Object::Array(arr) = &args[1] {
            let elements = &arr.borrow().elements;
            elements
                .iter()
                .map(|obj| match obj {
                    Object::String(s) => s.to_string(),
                    Object::Number(n) => format_number(*n),
                    Object::Boolean(b) => b.to_string(),
                    _ => format!("{:?}", obj),
                })
                .collect()
        } else {
            // Treat remaining args as individual arguments
            args[1..]
                .iter()
                .map(|obj| match obj {
                    Object::String(s) => s.to_string(),
                    Object::Number(n) => format_number(*n),
                    Object::Boolean(b) => b.to_string(),
                    _ => format!("{:?}", obj),
                })
                .collect()
        }
    } else {
        Vec::new()
    };

    Ok((cmd_name, cmd_args))
}
