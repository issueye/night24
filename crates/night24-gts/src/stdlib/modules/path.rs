use std::env;
use std::path::{Path, PathBuf, MAIN_SEPARATOR, MAIN_SEPARATOR_STR};

use super::super::helpers::*;
use crate::object::{bool_obj, new_error, str_obj, CallContext, Object};

pub(crate) fn path_module() -> Object {
    module(vec![
        ("join", native("path.join", path_join)),
        ("resolve", native("path.resolve", path_resolve)),
        ("relative", native("path.relative", path_relative)),
        ("normalize", native("path.normalize", path_normalize)),
        ("dirname", native("path.dirname", path_dirname)),
        ("basename", native("path.basename", path_basename)),
        ("extname", native("path.extname", path_extname)),
        ("isAbs", native("path.isAbs", path_is_abs)),
        ("toSlash", native("path.toSlash", path_to_slash)),
        ("fromSlash", native("path.fromSlash", path_from_slash)),
        ("parse", native("path.parse", path_parse)),
        ("format", native("path.format", path_format)),
        ("splitList", native("path.splitList", path_split_list)),
        ("sep", str_obj(MAIN_SEPARATOR.to_string())),
        ("delimiter", str_obj(if cfg!(windows) { ";" } else { ":" })),
    ])
}

pub(crate) fn path_join(ctx: &mut CallContext, args: &[Object]) -> Object {
    let parts = match string_args(ctx, "path.join", args) {
        Ok(parts) => parts,
        Err(err) => return err,
    };
    let mut path = PathBuf::new();
    for part in parts {
        path.push(part);
    }
    str_obj(path.to_string_lossy())
}

pub(crate) fn path_resolve(ctx: &mut CallContext, args: &[Object]) -> Object {
    let parts = match string_args(ctx, "path.resolve", args) {
        Ok(parts) => parts,
        Err(err) => return err,
    };
    let mut path = if parts.is_empty() {
        PathBuf::from(".")
    } else {
        let mut path = PathBuf::new();
        for part in parts {
            path.push(part);
        }
        path
    };
    if !path.is_absolute() {
        match env::current_dir() {
            Ok(cwd) => path = cwd.join(path),
            Err(e) => return new_error(ctx.pos.clone(), format!("path.resolve: {}", e)),
        }
    }
    str_obj(path.to_string_lossy())
}

pub(crate) fn path_relative(ctx: &mut CallContext, args: &[Object]) -> Object {
    let from = match required_string(ctx, "path.relative", args, 0, "from") {
        Ok(value) => value,
        Err(err) => return err,
    };
    let to = match required_string(ctx, "path.relative", args, 1, "to") {
        Ok(value) => value,
        Err(err) => return err,
    };
    match pathdiff(&PathBuf::from(from), &PathBuf::from(to)) {
        Some(path) => str_obj(path.to_string_lossy()),
        None => new_error(
            ctx.pos.clone(),
            "path.relative: cannot compute relative path",
        ),
    }
}

pub(crate) fn path_normalize(ctx: &mut CallContext, args: &[Object]) -> Object {
    let path = match required_string(ctx, "path.normalize", args, 0, "path") {
        Ok(value) => value,
        Err(err) => return err,
    };
    str_obj(normalize_path_string(&path))
}

pub(crate) fn path_dirname(ctx: &mut CallContext, args: &[Object]) -> Object {
    let path = match required_string(ctx, "path.dirname", args, 0, "path") {
        Ok(value) => value,
        Err(err) => return err,
    };
    str_obj(
        Path::new(&path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| ".".into()),
    )
}

pub(crate) fn path_basename(ctx: &mut CallContext, args: &[Object]) -> Object {
    let path = match required_string(ctx, "path.basename", args, 0, "path") {
        Ok(value) => value,
        Err(err) => return err,
    };
    str_obj(
        Path::new(&path)
            .file_name()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default(),
    )
}

pub(crate) fn path_extname(ctx: &mut CallContext, args: &[Object]) -> Object {
    let path = match required_string(ctx, "path.extname", args, 0, "path") {
        Ok(value) => value,
        Err(err) => return err,
    };
    let ext = Path::new(&path)
        .extension()
        .map(|ext| format!(".{}", ext.to_string_lossy()))
        .unwrap_or_default();
    str_obj(ext)
}

pub(crate) fn path_is_abs(ctx: &mut CallContext, args: &[Object]) -> Object {
    match required_string(ctx, "path.isAbs", args, 0, "path") {
        Ok(value) => bool_obj(Path::new(&value).is_absolute()),
        Err(err) => err,
    }
}

pub(crate) fn path_to_slash(ctx: &mut CallContext, args: &[Object]) -> Object {
    match required_string(ctx, "path.toSlash", args, 0, "path") {
        Ok(value) => str_obj(value.replace('\\', "/")),
        Err(err) => err,
    }
}

pub(crate) fn path_from_slash(ctx: &mut CallContext, args: &[Object]) -> Object {
    match required_string(ctx, "path.fromSlash", args, 0, "path") {
        Ok(value) => str_obj(value.replace('/', MAIN_SEPARATOR_STR)),
        Err(err) => err,
    }
}

pub(crate) fn path_parse(ctx: &mut CallContext, args: &[Object]) -> Object {
    let value = match required_string(ctx, "path.parse", args, 0, "path") {
        Ok(value) => value,
        Err(err) => return err,
    };
    let path = Path::new(&value);
    let base = path
        .file_name()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let ext = path
        .extension()
        .map(|p| format!(".{}", p.to_string_lossy()))
        .unwrap_or_default();
    let name = base.strip_suffix(&ext).unwrap_or(&base).to_string();
    let dir = path
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| ".".into());
    let root = if path.is_absolute() {
        MAIN_SEPARATOR.to_string()
    } else {
        String::new()
    };
    module(vec![
        ("root", str_obj(root)),
        ("dir", str_obj(dir)),
        ("base", str_obj(base)),
        ("name", str_obj(name)),
        ("ext", str_obj(ext)),
    ])
}

pub(crate) fn path_format(ctx: &mut CallContext, args: &[Object]) -> Object {
    let Some(Object::Hash(hash)) = args.first() else {
        return new_error(ctx.pos.clone(), "path.format requires a path object");
    };
    let hash = hash.borrow();
    let dir = hash_string(&hash, "dir").unwrap_or_default();
    let root = hash_string(&hash, "root").unwrap_or_default();
    let base = hash_string(&hash, "base").unwrap_or_default();
    let name = hash_string(&hash, "name").unwrap_or_default();
    let ext = hash_string(&hash, "ext").unwrap_or_default();
    let file = if !base.is_empty() {
        base
    } else {
        format!("{}{}", name, ext)
    };
    if !dir.is_empty() {
        str_obj(PathBuf::from(dir).join(file).to_string_lossy())
    } else {
        str_obj(PathBuf::from(root).join(file).to_string_lossy())
    }
}

pub(crate) fn path_split_list(ctx: &mut CallContext, args: &[Object]) -> Object {
    let value = match required_string(ctx, "path.splitList", args, 0, "value") {
        Ok(value) => value,
        Err(err) => return err,
    };
    array(
        env::split_paths(&value)
            .map(|p| str_obj(p.to_string_lossy()))
            .collect(),
    )
}
