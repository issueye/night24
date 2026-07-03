use std::cell::RefCell;
use std::fs;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::UNIX_EPOCH;

use super::super::helpers::*;
use crate::object::{bool_obj, new_error, num_obj, str_obj, CallContext, Object};

pub(crate) fn fs_module() -> Object {
    module(vec![
        ("readFileSync", native("fs.readFileSync", fs_read_file_sync)),
        ("readTextSync", native("fs.readTextSync", fs_read_file_sync)),
        (
            "writeFileSync",
            native("fs.writeFileSync", fs_write_file_sync),
        ),
        (
            "writeTextSync",
            native("fs.writeTextSync", fs_write_file_sync),
        ),
        (
            "appendFileSync",
            native("fs.appendFileSync", fs_append_file_sync),
        ),
        (
            "appendTextSync",
            native("fs.appendTextSync", fs_append_file_sync),
        ),
        (
            "writeFileAtomicSync",
            native("fs.writeFileAtomicSync", fs_write_file_atomic_sync),
        ),
        (
            "createThrottledWriter",
            native("fs.createThrottledWriter", fs_create_throttled_writer),
        ),
        ("existsSync", native("fs.existsSync", fs_exists_sync)),
        ("readdirSync", native("fs.readdirSync", fs_readdir_sync)),
        ("walkSync", native("fs.walkSync", fs_walk_sync)),
        ("globSync", native("fs.globSync", fs_glob_sync)),
        ("mkdirSync", native("fs.mkdirSync", fs_mkdir_sync)),
        ("statSync", native("fs.statSync", fs_stat_sync)),
        ("lstatSync", native("fs.lstatSync", fs_lstat_sync)),
        ("realpathSync", native("fs.realpathSync", fs_realpath_sync)),
        ("copyFileSync", native("fs.copyFileSync", fs_copy_file_sync)),
        ("renameSync", native("fs.renameSync", fs_rename_sync)),
        ("unlinkSync", native("fs.unlinkSync", fs_unlink_sync)),
        ("rmSync", native("fs.rmSync", fs_rm_sync)),
        ("mkdtempSync", native("fs.mkdtempSync", fs_mkdtemp_sync)),
    ])
}

pub(crate) fn fs_read_file_sync(ctx: &mut CallContext, args: &[Object]) -> Object {
    let path = match required_string(ctx, "fs.readFileSync", args, 0, "path") {
        Ok(path) => path,
        Err(err) => return err,
    };
    match fs::read_to_string(&path) {
        Ok(text) => str_obj(text),
        Err(e) => new_error(ctx.pos.clone(), format!("fs.readFileSync: {}", e)),
    }
}

pub(crate) fn fs_write_file_sync(ctx: &mut CallContext, args: &[Object]) -> Object {
    let path = match required_string(ctx, "fs.writeFileSync", args, 0, "path") {
        Ok(path) => path,
        Err(err) => return err,
    };
    let Some(data) = args.get(1) else {
        return new_error(ctx.pos.clone(), "fs.writeFileSync requires data");
    };
    match fs::write(&path, object_to_text(data)) {
        Ok(_) => Object::Undefined,
        Err(e) => new_error(ctx.pos.clone(), format!("fs.writeFileSync: {}", e)),
    }
}

pub(crate) fn fs_append_file_sync(ctx: &mut CallContext, args: &[Object]) -> Object {
    let path = match required_string(ctx, "fs.appendFileSync", args, 0, "path") {
        Ok(path) => path,
        Err(err) => return err,
    };
    let Some(data) = args.get(1) else {
        return new_error(ctx.pos.clone(), "fs.appendFileSync requires data");
    };
    match OpenOptions::new().create(true).append(true).open(&path) {
        Ok(mut file) => match file.write_all(object_to_text(data).as_bytes()) {
            Ok(_) => Object::Undefined,
            Err(e) => new_error(ctx.pos.clone(), format!("fs.appendFileSync: {}", e)),
        },
        Err(e) => new_error(ctx.pos.clone(), format!("fs.appendFileSync: {}", e)),
    }
}

pub(crate) fn fs_write_file_atomic_sync(ctx: &mut CallContext, args: &[Object]) -> Object {
    let path = match required_string(ctx, "fs.writeFileAtomicSync", args, 0, "path") {
        Ok(path) => path,
        Err(err) => return err,
    };
    let Some(data) = args.get(1) else {
        return new_error(ctx.pos.clone(), "fs.writeFileAtomicSync requires data");
    };
    match atomic_write_file(Path::new(&path), object_to_text(data).as_bytes()) {
        Ok(_) => Object::Undefined,
        Err(e) => new_error(ctx.pos.clone(), format!("fs.writeFileAtomicSync: {}", e)),
    }
}

pub(crate) fn fs_create_throttled_writer(ctx: &mut CallContext, args: &[Object]) -> Object {
    let path = match required_string(ctx, "fs.createThrottledWriter", args, 0, "path") {
        Ok(path) => path,
        Err(err) => return err,
    };
    let state = Rc::new(RefCell::new(ThrottledWriterState { path, latest: None }));
    let write_state = state.clone();
    let flush_state = state.clone();
    let flush_async_state = state.clone();
    let close_state = state.clone();
    module(vec![
        (
            "write",
            native("throttledWriter.write", move |ctx, args| {
                let Some(data) = args.first() else {
                    return new_error(ctx.pos.clone(), "throttledWriter.write: data required");
                };
                write_state.borrow_mut().latest = Some(object_to_text(data));
                Object::Undefined
            }),
        ),
        (
            "flush",
            native("throttledWriter.flush", move |ctx, _args| {
                flush_throttled_writer(ctx, &flush_state)
            }),
        ),
        (
            "flushAsync",
            native("throttledWriter.flushAsync", move |ctx, _args| {
                flush_throttled_writer(ctx, &flush_async_state)
            }),
        ),
        (
            "close",
            native("throttledWriter.close", move |ctx, _args| {
                flush_throttled_writer(ctx, &close_state)
            }),
        ),
        (
            "markDirty",
            native("throttledWriter.markDirty", |_ctx, _args| Object::Undefined),
        ),
        (
            "setProvider",
            native("throttledWriter.setProvider", |_ctx, _args| {
                Object::Undefined
            }),
        ),
    ])
}

pub(crate) fn fs_exists_sync(ctx: &mut CallContext, args: &[Object]) -> Object {
    match required_string(ctx, "fs.existsSync", args, 0, "path") {
        Ok(path) => bool_obj(Path::new(&path).exists()),
        Err(err) => err,
    }
}

pub(crate) fn fs_readdir_sync(ctx: &mut CallContext, args: &[Object]) -> Object {
    let path = match required_string(ctx, "fs.readdirSync", args, 0, "path") {
        Ok(path) => path,
        Err(err) => return err,
    };
    let with_file_types = hash_bool_arg(args.get(1), "withFileTypes").unwrap_or(false);
    match fs::read_dir(&path) {
        Ok(entries) => {
            let mut values = Vec::new();
            for entry in entries {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(e) => return new_error(ctx.pos.clone(), format!("fs.readdirSync: {}", e)),
                };
                if with_file_types {
                    let entry_path = entry.path();
                    let meta = match entry.metadata() {
                        Ok(meta) => meta,
                        Err(e) => {
                            return new_error(ctx.pos.clone(), format!("fs.readdirSync: {}", e))
                        }
                    };
                    let value = stat_object_for_path(entry_path, meta);
                    if let Object::Hash(hash) = &value {
                        hash.borrow_mut()
                            .set("name", str_obj(entry.file_name().to_string_lossy()));
                    }
                    values.push(value);
                } else {
                    values.push(str_obj(entry.file_name().to_string_lossy()));
                }
            }
            array(values)
        }
        Err(e) => new_error(ctx.pos.clone(), format!("fs.readdirSync: {}", e)),
    }
}

pub(crate) fn fs_walk_sync(ctx: &mut CallContext, args: &[Object]) -> Object {
    let root = match required_string(ctx, "fs.walkSync", args, 0, "root") {
        Ok(root) => root,
        Err(err) => return err,
    };
    let include_dirs = hash_bool_arg(args.get(1), "includeDirs").unwrap_or(true);
    let root_path = PathBuf::from(&root);
    let mut entries = Vec::new();
    if let Err(e) = walk_dir_collect(&root_path, &root_path, include_dirs, &mut entries) {
        return new_error(ctx.pos.clone(), format!("fs.walkSync: {}", e));
    }
    entries.sort_by_key(|value| value.inspect());
    array(entries)
}

pub(crate) fn fs_glob_sync(ctx: &mut CallContext, args: &[Object]) -> Object {
    let pattern = match required_string(ctx, "fs.globSync", args, 0, "pattern") {
        Ok(pattern) => pattern,
        Err(err) => return err,
    };
    match glob_paths(&pattern) {
        Ok(matches) => array(
            matches
                .into_iter()
                .map(|path| str_obj(path.to_string_lossy()))
                .collect(),
        ),
        Err(e) => new_error(ctx.pos.clone(), format!("fs.globSync: {}", e)),
    }
}

pub(crate) fn fs_mkdir_sync(ctx: &mut CallContext, args: &[Object]) -> Object {
    let path = match required_string(ctx, "fs.mkdirSync", args, 0, "path") {
        Ok(path) => path,
        Err(err) => return err,
    };
    let recursive = match args.get(1) {
        Some(Object::Boolean(value)) => *value,
        other => hash_bool_arg(other, "recursive").unwrap_or(false),
    };
    let result = if recursive {
        fs::create_dir_all(&path)
    } else {
        fs::create_dir(&path)
    };
    match result {
        Ok(_) => Object::Undefined,
        Err(e) => new_error(ctx.pos.clone(), format!("fs.mkdirSync: {}", e)),
    }
}

pub(crate) fn fs_stat_sync(ctx: &mut CallContext, args: &[Object]) -> Object {
    let path = match required_string(ctx, "fs.statSync", args, 0, "path") {
        Ok(path) => path,
        Err(err) => return err,
    };
    let path_buf = PathBuf::from(&path);
    match fs::metadata(&path_buf) {
        Ok(meta) => stat_object_for_path(path_buf, meta),
        Err(e) => new_error(ctx.pos.clone(), format!("fs.statSync: {}", e)),
    }
}

pub(crate) fn fs_lstat_sync(ctx: &mut CallContext, args: &[Object]) -> Object {
    let path = match required_string(ctx, "fs.lstatSync", args, 0, "path") {
        Ok(path) => path,
        Err(err) => return err,
    };
    let path_buf = PathBuf::from(&path);
    match fs::symlink_metadata(&path_buf) {
        Ok(meta) => stat_object_for_path(path_buf, meta),
        Err(e) => new_error(ctx.pos.clone(), format!("fs.lstatSync: {}", e)),
    }
}

pub(crate) fn fs_realpath_sync(ctx: &mut CallContext, args: &[Object]) -> Object {
    let path = match required_string(ctx, "fs.realpathSync", args, 0, "path") {
        Ok(path) => path,
        Err(err) => return err,
    };
    match fs::canonicalize(&path) {
        Ok(path) => str_obj(path.to_string_lossy()),
        Err(e) => new_error(ctx.pos.clone(), format!("fs.realpathSync: {}", e)),
    }
}

pub(crate) fn fs_copy_file_sync(ctx: &mut CallContext, args: &[Object]) -> Object {
    let from = match required_string(ctx, "fs.copyFileSync", args, 0, "from") {
        Ok(path) => path,
        Err(err) => return err,
    };
    let to = match required_string(ctx, "fs.copyFileSync", args, 1, "to") {
        Ok(path) => path,
        Err(err) => return err,
    };
    match fs::copy(&from, &to) {
        Ok(_) => Object::Undefined,
        Err(e) => new_error(ctx.pos.clone(), format!("fs.copyFileSync: {}", e)),
    }
}

pub(crate) fn fs_rename_sync(ctx: &mut CallContext, args: &[Object]) -> Object {
    let from = match required_string(ctx, "fs.renameSync", args, 0, "from") {
        Ok(path) => path,
        Err(err) => return err,
    };
    let to = match required_string(ctx, "fs.renameSync", args, 1, "to") {
        Ok(path) => path,
        Err(err) => return err,
    };
    match fs::rename(&from, &to) {
        Ok(_) => Object::Undefined,
        Err(e) => new_error(ctx.pos.clone(), format!("fs.renameSync: {}", e)),
    }
}

pub(crate) fn fs_unlink_sync(ctx: &mut CallContext, args: &[Object]) -> Object {
    let path = match required_string(ctx, "fs.unlinkSync", args, 0, "path") {
        Ok(path) => path,
        Err(err) => return err,
    };
    match fs::remove_file(&path) {
        Ok(_) => Object::Undefined,
        Err(e) => new_error(ctx.pos.clone(), format!("fs.unlinkSync: {}", e)),
    }
}

pub(crate) fn fs_rm_sync(ctx: &mut CallContext, args: &[Object]) -> Object {
    let path = match required_string(ctx, "fs.rmSync", args, 0, "path") {
        Ok(path) => path,
        Err(err) => return err,
    };
    let recursive = hash_bool_arg(args.get(1), "recursive").unwrap_or(false);
    let force = hash_bool_arg(args.get(1), "force").unwrap_or(false);
    let target = Path::new(&path);
    let result = if recursive {
        fs::remove_dir_all(target)
    } else {
        fs::remove_file(target).or_else(|file_err| {
            if target.is_dir() {
                fs::remove_dir(target)
            } else {
                Err(file_err)
            }
        })
    };
    match result {
        Ok(_) => Object::Undefined,
        Err(e) if force && e.kind() == std::io::ErrorKind::NotFound => Object::Undefined,
        Err(e) => new_error(ctx.pos.clone(), format!("fs.rmSync: {}", e)),
    }
}

pub(crate) fn fs_mkdtemp_sync(ctx: &mut CallContext, args: &[Object]) -> Object {
    let prefix = match required_string(ctx, "fs.mkdtempSync", args, 0, "prefix") {
        Ok(prefix) => prefix,
        Err(err) => return err,
    };
    let prefix_path = PathBuf::from(&prefix);
    let dir = prefix_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let base = prefix_path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    for attempt in 0..10_000 {
        let candidate = dir.join(format!("{}{}-{}", base, now_ms(), attempt));
        match fs::create_dir(&candidate) {
            Ok(_) => return str_obj(candidate.to_string_lossy()),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return new_error(ctx.pos.clone(), format!("fs.mkdtempSync: {}", e)),
        }
    }
    new_error(
        ctx.pos.clone(),
        "fs.mkdtempSync: could not create unique directory",
    )
}

pub(crate) fn stat_object_for_path(path: PathBuf, meta: fs::Metadata) -> Object {
    let mtime_ms = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as f64)
        .map(num_obj)
        .unwrap_or(Object::Undefined);
    let is_file = meta.is_file();
    let is_dir = meta.is_dir();
    let is_symlink = meta.file_type().is_symlink();
    module(vec![
        ("path", str_obj(path.to_string_lossy())),
        (
            "name",
            str_obj(
                path.file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_default(),
            ),
        ),
        ("size", num_obj(meta.len() as f64)),
        ("mode", str_obj(format!("{:?}", meta.permissions()))),
        ("mtimeMs", mtime_ms.clone()),
        ("modifiedMs", mtime_ms),
        ("isFileValue", bool_obj(is_file)),
        ("isDirectoryValue", bool_obj(is_dir)),
        ("isDir", bool_obj(is_dir)),
        ("isSymlinkValue", bool_obj(is_symlink)),
        (
            "isFile",
            native("fs.stat.isFile", move |_ctx, _args| bool_obj(is_file)),
        ),
        (
            "isDirectory",
            native("fs.stat.isDirectory", move |_ctx, _args| bool_obj(is_dir)),
        ),
        (
            "isSymlink",
            native("fs.stat.isSymlink", move |_ctx, _args| bool_obj(is_symlink)),
        ),
    ])
}

#[derive(Default)]
pub(crate) struct ThrottledWriterState {
    path: String,
    latest: Option<String>,
}

pub(crate) fn flush_throttled_writer(
    ctx: &mut CallContext,
    state: &Rc<RefCell<ThrottledWriterState>>,
) -> Object {
    let (path, latest) = {
        let mut state = state.borrow_mut();
        (state.path.clone(), state.latest.take())
    };
    let Some(latest) = latest else {
        return Object::Undefined;
    };
    match atomic_write_file(Path::new(&path), latest.as_bytes()) {
        Ok(_) => Object::Undefined,
        Err(e) => new_error(ctx.pos.clone(), format!("throttledWriter.flush: {}", e)),
    }
}

pub(crate) fn atomic_write_file(path: &Path, data: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    let dir = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let base = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".into());
    let tmp = dir.join(format!(".{}.{}.tmp", base, now_ms()));
    fs::write(&tmp, data)?;
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

pub(crate) fn walk_dir_collect(
    root: &Path,
    current: &Path,
    include_dirs: bool,
    entries: &mut Vec<Object>,
) -> std::io::Result<()> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let meta = entry.metadata()?;
        if meta.is_dir() {
            if include_dirs {
                let value = stat_object_for_path(path.clone(), meta);
                set_relative_path(&value, root, &path);
                entries.push(value);
            }
            walk_dir_collect(root, &path, include_dirs, entries)?;
        } else {
            let value = stat_object_for_path(path.clone(), meta);
            set_relative_path(&value, root, &path);
            entries.push(value);
        }
    }
    Ok(())
}

pub(crate) fn set_relative_path(value: &Object, root: &Path, path: &Path) {
    if let Object::Hash(hash) = value {
        let relative = path.strip_prefix(root).unwrap_or(path);
        hash.borrow_mut()
            .set("relativePath", str_obj(relative.to_string_lossy()));
    }
}
