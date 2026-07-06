use std::env;
use std::path::{Path, PathBuf, MAIN_SEPARATOR, MAIN_SEPARATOR_STR};

use super::super::helpers::*;
use crate::object::{bool_obj, new_error, str_obj, CallContext, HashData, Object};

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
    let reader = ArgReader::new(ctx, "path.relative", args);
    let from = match reader.required_string(0, "from") {
        Ok(value) => value,
        Err(err) => return err,
    };
    let to = match reader.required_string(1, "to") {
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
    let reader = ArgReader::new(ctx, "path.normalize", args);
    let path = match reader.required_string(0, "path") {
        Ok(value) => value,
        Err(err) => return err,
    };
    str_obj(normalize_path_string(&path))
}

pub(crate) fn path_dirname(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "path.dirname", args);
    let path = match reader.required_string(0, "path") {
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
    let reader = ArgReader::new(ctx, "path.basename", args);
    let path = match reader.required_string(0, "path") {
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
    let reader = ArgReader::new(ctx, "path.extname", args);
    let path = match reader.required_string(0, "path") {
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
    let reader = ArgReader::new(ctx, "path.isAbs", args);
    match reader.required_string(0, "path") {
        Ok(value) => bool_obj(Path::new(&value).is_absolute()),
        Err(err) => err,
    }
}

pub(crate) fn path_to_slash(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "path.toSlash", args);
    match reader.required_string(0, "path") {
        Ok(value) => str_obj(value.replace('\\', "/")),
        Err(err) => err,
    }
}

pub(crate) fn path_from_slash(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "path.fromSlash", args);
    match reader.required_string(0, "path") {
        Ok(value) => str_obj(value.replace('/', MAIN_SEPARATOR_STR)),
        Err(err) => err,
    }
}

pub(crate) fn path_parse(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "path.parse", args);
    let value = match reader.required_string(0, "path") {
        Ok(value) => value,
        Err(err) => return err,
    };
    path_parse_object(&value)
}

pub(crate) fn path_parse_object(value: &str) -> Object {
    let path = Path::new(value);
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
    ObjectBuilder::new()
        .set("root", str_obj(root))
        .set("dir", str_obj(dir))
        .set("base", str_obj(base))
        .set("name", str_obj(name))
        .set("ext", str_obj(ext))
        .build()
}

pub(crate) fn path_format(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "path.format", args);
    let Some(hash) = reader.object_view(0) else {
        return new_error(ctx.pos.clone(), "path.format requires a path object");
    };
    path_format_object(&hash)
}

pub(crate) fn path_format_object(hash: &HashData) -> Object {
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
    let reader = ArgReader::new(ctx, "path.splitList", args);
    let value = match reader.required_string(0, "value") {
        Ok(value) => value,
        Err(err) => return err,
    };
    array(
        env::split_paths(&value)
            .map(|p| str_obj(p.to_string_lossy()))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object_string_field(object: &Object, key: &str) -> String {
        let Object::Hash(hash) = object else {
            panic!("expected object");
        };
        match hash.borrow().get(key) {
            Some(Object::String(value)) => value.to_string(),
            _ => panic!("expected string field {key}"),
        }
    }

    fn object_as_string(object: Object) -> String {
        match object {
            Object::String(value) => value.to_string(),
            _ => panic!("expected string"),
        }
    }

    #[test]
    fn path_parse_object_builds_expected_fields() {
        let parsed = path_parse_object("dir/file.txt");

        assert_eq!(object_string_field(&parsed, "root"), "");
        assert_eq!(object_string_field(&parsed, "dir"), "dir");
        assert_eq!(object_string_field(&parsed, "base"), "file.txt");
        assert_eq!(object_string_field(&parsed, "name"), "file");
        assert_eq!(object_string_field(&parsed, "ext"), ".txt");
    }

    #[test]
    fn path_parse_object_handles_extensionless_file() {
        let parsed = path_parse_object("README");

        assert_eq!(object_string_field(&parsed, "dir"), "");
        assert_eq!(object_string_field(&parsed, "base"), "README");
        assert_eq!(object_string_field(&parsed, "name"), "README");
        assert_eq!(object_string_field(&parsed, "ext"), "");
    }

    #[test]
    fn path_format_object_prefers_base_over_name_ext() {
        let object = ObjectBuilder::new()
            .set("dir", str_obj("dir"))
            .set("base", str_obj("file.txt"))
            .set("name", str_obj("ignored"))
            .set("ext", str_obj(".md"))
            .build();
        let Object::Hash(hash) = object else {
            panic!("expected hash");
        };

        assert_eq!(
            object_as_string(path_format_object(&hash.borrow())),
            "dir\\file.txt".replace('\\', MAIN_SEPARATOR_STR)
        );
    }

    #[test]
    fn path_format_object_ignores_non_string_fields() {
        let object = ObjectBuilder::new()
            .set("dir", str_obj("dir"))
            .set("name", str_obj("file"))
            .set("ext", bool_obj(true))
            .build();
        let Object::Hash(hash) = object else {
            panic!("expected hash");
        };

        assert_eq!(
            object_as_string(path_format_object(&hash.borrow())),
            "dir\\file".replace('\\', MAIN_SEPARATOR_STR)
        );
    }
}
