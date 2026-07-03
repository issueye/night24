use std::cell::RefCell;
use std::rc::Rc;

use super::super::helpers::*;
use crate::object::{bool_obj, new_error, num_obj, str_obj, CallContext, HashData, Object};

pub(crate) fn cli_module() -> Object {
    module(vec![
        ("command", native("cli.command", cli_command_new)),
        ("root", native("cli.root", cli_command_new)),
        ("noArgs", native("cli.noArgs", cli_no_args)),
        (
            "arbitraryArgs",
            native("cli.arbitraryArgs", cli_arbitrary_args),
        ),
        ("exactArgs", native("cli.exactArgs", cli_exact_args)),
        ("minArgs", native("cli.minArgs", cli_min_args)),
        ("maxArgs", native("cli.maxArgs", cli_max_args)),
        ("rangeArgs", native("cli.rangeArgs", cli_range_args)),
    ])
}

#[derive(Clone)]
pub(crate) struct CliCommand {
    use_line: String,
    name: String,
    short: String,
    version: String,
    run: Option<Object>,
    args_validator: CliArgValidator,
    parent: Option<Rc<RefCell<CliCommand>>>,
    children: Vec<Rc<RefCell<CliCommand>>>,
    flags: Rc<RefCell<CliFlagSet>>,
    persistent_flags: Rc<RefCell<CliFlagSet>>,
}

#[derive(Clone)]
pub(crate) struct CliFlagSet {
    flags: Vec<CliFlag>,
}

#[derive(Clone)]
pub(crate) struct CliFlag {
    name: String,
    short: String,
    usage: String,
    kind: String,
    default: Object,
    value: Object,
    changed: bool,
}

#[derive(Clone)]
pub(crate) struct CliArgValidator {
    kind: String,
    min: usize,
    max: usize,
}

pub(crate) fn cli_command_new(ctx: &mut CallContext, args: &[Object]) -> Object {
    let cmd = Rc::new(RefCell::new(CliCommand {
        use_line: String::new(),
        name: String::new(),
        short: String::new(),
        version: String::new(),
        run: None,
        args_validator: CliArgValidator {
            kind: "any".into(),
            min: 0,
            max: usize::MAX,
        },
        parent: None,
        children: Vec::new(),
        flags: Rc::new(RefCell::new(CliFlagSet { flags: Vec::new() })),
        persistent_flags: Rc::new(RefCell::new(CliFlagSet { flags: Vec::new() })),
    }));
    if let Some(value) = args.first() {
        if let Err(err) = cli_apply_options(ctx, &cmd, value) {
            return err;
        }
    }
    cli_command_object(cmd)
}

pub(crate) fn cli_apply_options(
    ctx: &mut CallContext,
    cmd: &Rc<RefCell<CliCommand>>,
    value: &Object,
) -> Result<(), Object> {
    if matches!(value, Object::Undefined | Object::Null) {
        return Ok(());
    }
    let Object::Hash(hash) = value else {
        return Err(new_error(
            ctx.pos.clone(),
            "cli.command: options must be an object",
        ));
    };
    let hash = hash.borrow();
    let use_line = hash_string(&hash, "use").or_else(|| hash_string(&hash, "Use"));
    let mut cmd_mut = cmd.borrow_mut();
    if let Some(use_line) = use_line {
        cmd_mut.use_line = use_line;
        cmd_mut.name = cmd_mut
            .use_line
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_string();
    }
    if let Some(short) = hash_string(&hash, "short").or_else(|| hash_string(&hash, "Short")) {
        cmd_mut.short = short;
    }
    if let Some(version) = hash_string(&hash, "version").or_else(|| hash_string(&hash, "Version")) {
        cmd_mut.version = version;
    }
    if let Some(run) = hash.get("run").or_else(|| hash.get("Run")) {
        match run {
            Object::Function(_) | Object::Builtin(_) | Object::Closure(_) => {
                cmd_mut.run = Some(run.clone());
            }
            Object::Undefined | Object::Null => {}
            _ => {
                return Err(new_error(
                    ctx.pos.clone(),
                    "cli.command: run must be a function",
                ));
            }
        }
    }
    if let Some(Object::Hash(vhash)) = hash.get("args").or_else(|| hash.get("Args")) {
        if vhash.borrow().contains("__cliArgValidator") {
            cmd_mut.args_validator = cli_validator_from_hash(&vhash.borrow());
        }
    }
    Ok(())
}

pub(crate) fn cli_command_object(cmd: Rc<RefCell<CliCommand>>) -> Object {
    let flags_cmd = cmd.clone();
    let pflags_cmd = cmd.clone();
    let child_cmd = cmd.clone();
    let execute_cmd = cmd.clone();
    let usage_cmd = cmd.clone();
    let path_cmd = cmd.clone();
    let flag_cmd = cmd.clone();
    module(vec![
        (
            "flags",
            native("cli.Command.flags", move |_ctx, _args| {
                cli_flag_set_object(flags_cmd.borrow().flags.clone())
            }),
        ),
        (
            "persistentFlags",
            native("cli.Command.persistentFlags", move |_ctx, _args| {
                cli_flag_set_object(pflags_cmd.borrow().persistent_flags.clone())
            }),
        ),
        (
            "addCommand",
            native("cli.Command.addCommand", move |_ctx, _args| {
                Object::Undefined
            }),
        ),
        (
            "command",
            native("cli.Command.command", move |ctx, args| {
                let child_state = Rc::new(RefCell::new(CliCommand {
                    use_line: String::new(),
                    name: String::new(),
                    short: String::new(),
                    version: String::new(),
                    run: None,
                    args_validator: CliArgValidator {
                        kind: "any".into(),
                        min: 0,
                        max: usize::MAX,
                    },
                    parent: Some(child_cmd.clone()),
                    children: Vec::new(),
                    flags: Rc::new(RefCell::new(CliFlagSet { flags: Vec::new() })),
                    persistent_flags: Rc::new(RefCell::new(CliFlagSet { flags: Vec::new() })),
                }));
                if let Some(value) = args.first() {
                    if let Err(err) = cli_apply_options(ctx, &child_state, value) {
                        return err;
                    }
                }
                child_cmd.borrow_mut().children.push(child_state.clone());
                cli_command_object(child_state)
            }),
        ),
        (
            "execute",
            native("cli.Command.execute", move |ctx, args| {
                cli_execute(ctx, &execute_cmd, args)
            }),
        ),
        (
            "usage",
            native("cli.Command.usage", move |_ctx, _args| {
                str_obj(cli_usage(&usage_cmd))
            }),
        ),
        (
            "help",
            native("cli.Command.help", move |_ctx, _args| Object::Undefined),
        ),
        (
            "commandPath",
            native("cli.Command.commandPath", move |_ctx, _args| {
                str_obj(cli_command_path(&path_cmd))
            }),
        ),
        (
            "flag",
            native("cli.Command.flag", move |ctx, args| {
                let name = match required_string(ctx, "cli.Command.flag", args, 0, "name") {
                    Ok(name) => name,
                    Err(err) => return err,
                };
                cli_lookup_flag(&flag_cmd, &name)
                    .map(|flag| flag.value)
                    .unwrap_or(Object::Undefined)
            }),
        ),
    ])
}

pub(crate) fn cli_flag_set_object(set: Rc<RefCell<CliFlagSet>>) -> Object {
    let s1 = set.clone();
    let s2 = set.clone();
    let s3 = set.clone();
    let s4 = set.clone();
    let s5 = set.clone();
    let s6 = set.clone();
    module(vec![
        (
            "string",
            native("cli.FlagSet.string", move |ctx, args| {
                cli_flag_add(ctx, &s1, "string", args)
            }),
        ),
        (
            "bool",
            native("cli.FlagSet.bool", move |ctx, args| {
                cli_flag_add(ctx, &s2, "bool", args)
            }),
        ),
        (
            "int",
            native("cli.FlagSet.int", move |ctx, args| {
                cli_flag_add(ctx, &s3, "int", args)
            }),
        ),
        (
            "number",
            native("cli.FlagSet.number", move |ctx, args| {
                cli_flag_add(ctx, &s4, "number", args)
            }),
        ),
        (
            "get",
            native("cli.FlagSet.get", move |ctx, args| {
                let name = match required_string(ctx, "cli.FlagSet.get", args, 0, "name") {
                    Ok(name) => name,
                    Err(err) => return err,
                };
                s5.borrow()
                    .flags
                    .iter()
                    .find(|flag| flag.name == name)
                    .map(|flag| flag.value.clone())
                    .unwrap_or(Object::Undefined)
            }),
        ),
        (
            "changed",
            native("cli.FlagSet.changed", move |ctx, args| {
                let name = match required_string(ctx, "cli.FlagSet.changed", args, 0, "name") {
                    Ok(name) => name,
                    Err(err) => return err,
                };
                bool_obj(
                    s6.borrow()
                        .flags
                        .iter()
                        .any(|flag| flag.name == name && flag.changed),
                )
            }),
        ),
    ])
}

pub(crate) fn cli_flag_add(
    ctx: &mut CallContext,
    set: &Rc<RefCell<CliFlagSet>>,
    kind: &str,
    args: &[Object],
) -> Object {
    let name = match required_string(ctx, &format!("cli.FlagSet.{}", kind), args, 0, "name") {
        Ok(name) => name,
        Err(err) => return err,
    };
    let short = match args.get(1) {
        Some(Object::String(s)) => s.to_string(),
        _ => String::new(),
    };
    let default = args
        .get(2)
        .cloned()
        .unwrap_or_else(|| cli_default_for_kind(kind));
    let usage = match args.get(3) {
        Some(Object::String(s)) => s.to_string(),
        _ => String::new(),
    };
    let value = match cli_coerce_flag(ctx, kind, default) {
        Ok(value) => value,
        Err(err) => return err,
    };
    if set.borrow().flags.iter().any(|flag| flag.name == name) {
        return new_error(
            ctx.pos.clone(),
            format!("cli: flag {} is already defined", name),
        );
    }
    set.borrow_mut().flags.push(CliFlag {
        name,
        short,
        usage,
        kind: kind.into(),
        default: value.clone(),
        value,
        changed: false,
    });
    cli_flag_set_object(set.clone())
}

pub(crate) fn cli_default_for_kind(kind: &str) -> Object {
    match kind {
        "string" => str_obj(""),
        "bool" => bool_obj(false),
        "int" | "number" => num_obj(0.0),
        _ => Object::Undefined,
    }
}

pub(crate) fn cli_coerce_flag(
    ctx: &mut CallContext,
    kind: &str,
    value: Object,
) -> Result<Object, Object> {
    match kind {
        "string" if matches!(value, Object::String(_)) => Ok(value),
        "bool" if matches!(value, Object::Boolean(_)) => Ok(value),
        "int" | "number" if matches!(value, Object::Number(_)) => Ok(value),
        "string" => Err(new_error(ctx.pos.clone(), "cli: default must be a string")),
        "bool" => Err(new_error(ctx.pos.clone(), "cli: default must be a bool")),
        "int" | "number" => Err(new_error(ctx.pos.clone(), "cli: default must be a number")),
        _ => Ok(value),
    }
}

pub(crate) fn cli_execute(
    ctx: &mut CallContext,
    cmd: &Rc<RefCell<CliCommand>>,
    args: &[Object],
) -> Object {
    let argv = if let Some(arg) = args.first() {
        match cli_string_array(ctx, "cli.Command.execute", arg, "args") {
            Ok(argv) => argv,
            Err(err) => return err,
        }
    } else {
        Vec::new()
    };
    cli_reset_flags(cmd);
    if let Err(err) = cli_parse_flags(ctx, cmd, &argv) {
        return err;
    }
    let positionals = cli_positionals(cmd, &argv);
    match cmd.borrow().args_validator.validate(ctx, positionals.len()) {
        Ok(()) => {}
        Err(err) => return err,
    }
    let run = cmd.borrow().run.clone();
    let Some(run) = run else {
        return num_obj(0.0);
    };
    let cmd_obj = cli_command_object(cmd.clone());
    let args_obj = array(positionals.into_iter().map(str_obj).collect());
    call_script_function(&run, ctx.env, &[cmd_obj, args_obj])
}

pub(crate) fn cli_reset_flags(cmd: &Rc<RefCell<CliCommand>>) {
    for flag in &mut cmd.borrow_mut().flags.borrow_mut().flags {
        flag.value = flag.default.clone();
        flag.changed = false;
    }
    for flag in &mut cmd.borrow_mut().persistent_flags.borrow_mut().flags {
        flag.value = flag.default.clone();
        flag.changed = false;
    }
}

pub(crate) fn cli_parse_flags(
    ctx: &mut CallContext,
    cmd: &Rc<RefCell<CliCommand>>,
    argv: &[String],
) -> Result<(), Object> {
    let mut i = 0;
    while i < argv.len() {
        let token = &argv[i];
        if token == "--" {
            break;
        }
        if let Some(raw) = token.strip_prefix("--") {
            let (name, value) = raw
                .split_once('=')
                .map_or((raw, None), |(n, v)| (n, Some(v)));
            let flags_ref = cmd.borrow().flags.clone();
            let mut flags = flags_ref.borrow_mut();
            let Some(flag) = flags.flags.iter_mut().find(|flag| flag.name == name) else {
                return Err(new_error(
                    ctx.pos.clone(),
                    format!("cli: unknown flag --{}", name),
                ));
            };
            let raw_value = if flag.kind == "bool" && value.is_none() {
                "true".to_string()
            } else if let Some(value) = value {
                value.to_string()
            } else {
                i += 1;
                argv.get(i).cloned().ok_or_else(|| {
                    new_error(
                        ctx.pos.clone(),
                        format!("cli: flag --{} requires value", name),
                    )
                })?
            };
            cli_set_flag(ctx, flag, &raw_value)?;
        } else if token.starts_with('-') && token.len() > 1 {
            let key = token.trim_start_matches('-');
            let flags_ref = cmd.borrow().flags.clone();
            let mut flags = flags_ref.borrow_mut();
            let Some(flag) = flags.flags.iter_mut().find(|flag| flag.short == key) else {
                return Err(new_error(
                    ctx.pos.clone(),
                    format!("cli: unknown shorthand -{}", key),
                ));
            };
            let raw_value = if flag.kind == "bool" {
                "true".to_string()
            } else {
                i += 1;
                argv.get(i).cloned().ok_or_else(|| {
                    new_error(
                        ctx.pos.clone(),
                        format!("cli: flag -{} requires value", key),
                    )
                })?
            };
            cli_set_flag(ctx, flag, &raw_value)?;
        }
        i += 1;
    }
    Ok(())
}

pub(crate) fn cli_set_flag(
    ctx: &mut CallContext,
    flag: &mut CliFlag,
    raw: &str,
) -> Result<(), Object> {
    flag.value = match flag.kind.as_str() {
        "string" => str_obj(raw),
        "bool" => match raw {
            "true" | "1" => bool_obj(true),
            "false" | "0" => bool_obj(false),
            _ => {
                return Err(new_error(
                    ctx.pos.clone(),
                    format!("cli: flag --{} expects bool", flag.name),
                ))
            }
        },
        "int" => num_obj(raw.parse::<i64>().map_err(|_| {
            new_error(
                ctx.pos.clone(),
                format!("cli: flag --{} expects int", flag.name),
            )
        })? as f64),
        "number" => num_obj(raw.parse::<f64>().map_err(|_| {
            new_error(
                ctx.pos.clone(),
                format!("cli: flag --{} expects number", flag.name),
            )
        })?),
        _ => Object::Undefined,
    };
    flag.changed = true;
    Ok(())
}

pub(crate) fn cli_lookup_flag(cmd: &Rc<RefCell<CliCommand>>, name: &str) -> Option<CliFlag> {
    cmd.borrow()
        .flags
        .borrow()
        .flags
        .iter()
        .find(|flag| flag.name == name || flag.short == name)
        .cloned()
}

pub(crate) fn cli_positionals(cmd: &Rc<RefCell<CliCommand>>, argv: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < argv.len() {
        let token = &argv[i];
        if token == "--" {
            out.extend(argv.iter().skip(i + 1).cloned());
            break;
        }
        if token.starts_with("--") {
            let name = token
                .trim_start_matches("--")
                .split('=')
                .next()
                .unwrap_or("");
            if token.contains('=') {
                i += 1;
                continue;
            }
            if let Some(flag) = cli_lookup_flag(cmd, name) {
                if flag.kind != "bool" {
                    i += 1;
                }
            }
        } else if token.starts_with('-') && token.len() > 1 {
            let name = token.trim_start_matches('-');
            if let Some(flag) = cli_lookup_flag(cmd, name) {
                if flag.kind != "bool" {
                    i += 1;
                }
            }
        } else {
            out.push(token.clone());
        }
        i += 1;
    }
    out
}

pub(crate) fn cli_usage(cmd: &Rc<RefCell<CliCommand>>) -> String {
    let cmd = cmd.borrow();
    let mut out = String::new();
    if !cmd.name.is_empty() {
        out.push_str(&cmd.name);
        if !cmd.short.is_empty() {
            out.push_str(" - ");
            out.push_str(&cmd.short);
        }
        out.push_str("\n\n");
    }
    if !cmd.use_line.is_empty() {
        out.push_str("Usage:\n  ");
        out.push_str(&cmd.use_line);
        out.push_str("\n\n");
    }
    let flags = cmd.flags.borrow();
    if !flags.flags.is_empty() {
        out.push_str("Flags:\n");
        for flag in &flags.flags {
            if flag.short.is_empty() {
                out.push_str(&format!("    --{} {}\n", flag.name, flag.usage));
            } else {
                out.push_str(&format!(
                    "  -{}, --{} {}\n",
                    flag.short, flag.name, flag.usage
                ));
            }
        }
    }
    out
}

pub(crate) fn cli_command_path(cmd: &Rc<RefCell<CliCommand>>) -> String {
    let mut parts = Vec::new();
    let mut current = Some(cmd.clone());
    while let Some(cmd) = current {
        let borrowed = cmd.borrow();
        if !borrowed.name.is_empty() {
            parts.push(borrowed.name.clone());
        }
        current = borrowed.parent.clone();
    }
    parts.reverse();
    parts.join(" ")
}

pub(crate) fn cli_string_array(
    ctx: &mut CallContext,
    name: &str,
    value: &Object,
    label: &str,
) -> Result<Vec<String>, Object> {
    let Object::Array(arr) = value else {
        return Err(new_error(
            ctx.pos.clone(),
            format!("{}: {} must be an array of strings", name, label),
        ));
    };
    let mut out = Vec::new();
    for item in &arr.borrow().elements {
        match item {
            Object::String(s) => out.push(s.to_string()),
            _ => {
                return Err(new_error(
                    ctx.pos.clone(),
                    format!("{}: {} must be an array of strings", name, label),
                ))
            }
        }
    }
    Ok(out)
}

pub(crate) fn cli_validator_object(kind: &str, min: usize, max: usize) -> Object {
    module(vec![
        ("__cliArgValidator", bool_obj(true)),
        ("kind", str_obj(kind)),
        ("min", num_obj(min as f64)),
        ("max", num_obj(max as f64)),
    ])
}

pub(crate) fn cli_validator_from_hash(hash: &HashData) -> CliArgValidator {
    CliArgValidator {
        kind: hash_string(hash, "kind").unwrap_or_else(|| "any".into()),
        min: match hash.get("min") {
            Some(Object::Number(n)) => *n as usize,
            _ => 0,
        },
        max: match hash.get("max") {
            Some(Object::Number(n)) => *n as usize,
            _ => usize::MAX,
        },
    }
}

impl CliArgValidator {
    fn validate(&self, ctx: &mut CallContext, count: usize) -> Result<(), Object> {
        match self.kind.as_str() {
            "none" if count != 0 => Err(new_error(
                ctx.pos.clone(),
                format!("cli: accepts no arguments, got {}", count),
            )),
            "exact" if count != self.min => Err(new_error(
                ctx.pos.clone(),
                format!("cli: accepts {} argument(s), got {}", self.min, count),
            )),
            "min" if count < self.min => Err(new_error(
                ctx.pos.clone(),
                format!(
                    "cli: requires at least {} argument(s), got {}",
                    self.min, count
                ),
            )),
            "max" if count > self.max => Err(new_error(
                ctx.pos.clone(),
                format!(
                    "cli: accepts at most {} argument(s), got {}",
                    self.max, count
                ),
            )),
            "range" if count < self.min || count > self.max => Err(new_error(
                ctx.pos.clone(),
                format!(
                    "cli: accepts between {} and {} argument(s), got {}",
                    self.min, self.max, count
                ),
            )),
            _ => Ok(()),
        }
    }
}

pub(crate) fn cli_no_args(_ctx: &mut CallContext, _args: &[Object]) -> Object {
    cli_validator_object("none", 0, 0)
}

pub(crate) fn cli_arbitrary_args(_ctx: &mut CallContext, _args: &[Object]) -> Object {
    cli_validator_object("any", 0, usize::MAX)
}

pub(crate) fn cli_exact_args(ctx: &mut CallContext, args: &[Object]) -> Object {
    match required_number(ctx, "cli.exactArgs", args, 0, "n") {
        Ok(n) => cli_validator_object("exact", n as usize, n as usize),
        Err(err) => err,
    }
}

pub(crate) fn cli_min_args(ctx: &mut CallContext, args: &[Object]) -> Object {
    match required_number(ctx, "cli.minArgs", args, 0, "n") {
        Ok(n) => cli_validator_object("min", n as usize, usize::MAX),
        Err(err) => err,
    }
}

pub(crate) fn cli_max_args(ctx: &mut CallContext, args: &[Object]) -> Object {
    match required_number(ctx, "cli.maxArgs", args, 0, "n") {
        Ok(n) => cli_validator_object("max", 0, n as usize),
        Err(err) => err,
    }
}

pub(crate) fn cli_range_args(ctx: &mut CallContext, args: &[Object]) -> Object {
    let min = match required_number(ctx, "cli.rangeArgs", args, 0, "min") {
        Ok(min) => min as usize,
        Err(err) => return err,
    };
    let max = match required_number(ctx, "cli.rangeArgs", args, 1, "max") {
        Ok(max) => max as usize,
        Err(err) => return err,
    };
    if max < min {
        return new_error(ctx.pos.clone(), "cli.rangeArgs: max must be >= min");
    }
    cli_validator_object("range", min, max)
}

// ---------------------------------------------------------------------------
// table: ASCII table rendering for arrays of rows or objects.
// ---------------------------------------------------------------------------
