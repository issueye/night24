use super::super::helpers::*;
use crate::object::{bool_obj, new_error, str_obj, CallContext, Object};

pub(crate) fn glob_module() -> Object {
    module(vec![
        ("glob", native("glob.glob", glob_glob)),
        ("globSync", native("glob.globSync", glob_glob)),
        ("match", native("glob.match", glob_match_native)),
        ("hasMagic", native("glob.hasMagic", glob_has_magic)),
    ])
}

pub(crate) fn glob_glob(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "glob.glob", args);
    let pattern = match reader.required_string(0, "pattern") {
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
        Err(e) => new_error(ctx.pos.clone(), format!("glob.glob: {}", e)),
    }
}

pub(crate) fn glob_match_native(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "glob.match", args);
    let pattern = match reader.required_string(0, "pattern") {
        Ok(pattern) => pattern,
        Err(err) => return err,
    };
    let path = match reader.required_string(1, "path") {
        Ok(path) => path,
        Err(err) => return err,
    };
    bool_obj(glob_match(&pattern, &path))
}

pub(crate) fn glob_has_magic(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "glob.hasMagic", args);
    match reader.required_string(0, "pattern") {
        Ok(pattern) => bool_obj(pattern.contains('*') || pattern.contains('?')),
        Err(err) => err,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_match_normalizes_windows_separators() {
        assert!(glob_match("src\\*.rs", "src/lib.rs"));
        assert!(glob_match("src/*.rs", "src\\main.rs"));
    }

    #[test]
    fn glob_match_supports_star_and_question_mark() {
        assert!(glob_match("src/*.r?", "src/lib.rs"));
        assert!(!glob_match("src/*.r?", "src/lib.ts"));
    }
}

// ---------------------------------------------------------------------------
// color: simple ANSI SGR wrappers and escape stripping.
// ---------------------------------------------------------------------------
