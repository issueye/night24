use std::cell::RefCell;
use std::fs;
use std::io::Write;
use std::rc::Rc;

use super::super::helpers::*;
use crate::object::{bool_obj, new_error, num_obj, str_obj, CallContext, HashData, Object};

pub(crate) fn archive_zip_module() -> Object {
    module(vec![
        ("list", native("zip.list", zip_list)),
        ("extract", native("zip.extract", zip_extract)),
        ("create", native("zip.create", zip_create)),
    ])
}

pub(crate) fn zip_list(ctx: &mut CallContext, args: &[Object]) -> Object {
    let path = match required_string(ctx, "zip.list", args, 0, "path") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let file = match fs::File::open(&path) {
        Ok(f) => f,
        Err(e) => return new_error(ctx.pos.clone(), format!("zip.list: {}", e)),
    };
    let mut archive = match zip::ZipArchive::new(file) {
        Ok(a) => a,
        Err(e) => return new_error(ctx.pos.clone(), format!("zip.list: {}", e)),
    };
    let mut entries = Vec::with_capacity(archive.len() as usize);
    for i in 0..archive.len() {
        let entry = match archive.by_index(i) {
            Ok(e) => e,
            Err(_) => continue,
        };
        let name = entry.name().to_string();
        let size = entry.size();
        let compressed = entry.compressed_size();
        let is_dir = entry.is_dir();
        let modified = entry
            .last_modified()
            .map(|d| format!("{}", d))
            .unwrap_or_default();
        let hash = Rc::new(RefCell::new(HashData::default()));
        hash.borrow_mut().set("name", str_obj(name));
        hash.borrow_mut().set("size", num_obj(size as f64));
        hash.borrow_mut()
            .set("compressedSize", num_obj(compressed as f64));
        hash.borrow_mut().set("isDir", bool_obj(is_dir));
        hash.borrow_mut().set("modified", str_obj(modified));
        entries.push(Object::Hash(hash));
    }
    array(entries)
}

pub(crate) fn zip_extract(ctx: &mut CallContext, args: &[Object]) -> Object {
    let archive_path = match required_string(ctx, "zip.extract", args, 0, "archive path") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let dest_path = match required_string(ctx, "zip.extract", args, 1, "destination path") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let file = match fs::File::open(&archive_path) {
        Ok(f) => f,
        Err(e) => return new_error(ctx.pos.clone(), format!("zip.extract: {}", e)),
    };
    let mut archive = match zip::ZipArchive::new(file) {
        Ok(a) => a,
        Err(e) => return new_error(ctx.pos.clone(), format!("zip.extract: {}", e)),
    };
    for i in 0..archive.len() {
        let mut entry = match archive.by_index(i) {
            Ok(e) => e,
            Err(_) => continue,
        };
        let outpath = match safe_zip_target(&dest_path, entry.name()) {
            Ok(p) => p,
            Err(_) => continue,
        };
        if entry.is_dir() {
            let _ = fs::create_dir_all(&outpath);
            continue;
        }
        if let Some(parent) = outpath.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let mut outfile = match fs::File::create(&outpath) {
            Ok(f) => f,
            Err(_) => continue,
        };
        let _ = std::io::copy(&mut entry, &mut outfile);
    }
    Object::Undefined
}

pub(crate) fn zip_create(ctx: &mut CallContext, args: &[Object]) -> Object {
    let files = match args.first() {
        Some(Object::Array(_)) => args[0].clone(),
        _ => return new_error(ctx.pos.clone(), "zip.create: files must be an array"),
    };
    let output_path = match required_string(ctx, "zip.create", args, 1, "output path") {
        Ok(v) => v,
        Err(e) => return e,
    };
    if let Some(parent) = std::path::Path::new(&output_path).parent() {
        let _ = fs::create_dir_all(parent);
    }
    let file = match fs::File::create(&output_path) {
        Ok(f) => f,
        Err(e) => return new_error(ctx.pos.clone(), format!("zip.create: {}", e)),
    };
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    if let Object::Array(arr) = &files {
        for (i, spec) in arr.borrow().elements.iter().enumerate() {
            let spec_hash = match spec {
                Object::Hash(h) => h.clone(),
                _ => {
                    return new_error(
                        ctx.pos.clone(),
                        format!("zip.create: files[{}] must be an object", i),
                    )
                }
            };
            let path = match spec_hash.borrow().get("path") {
                Some(Object::String(s)) => s.as_str().to_string(),
                _ => {
                    return new_error(
                        ctx.pos.clone(),
                        format!("zip.create: files[{}].path is required", i),
                    )
                }
            };
            let name = match spec_hash.borrow().get("name") {
                Some(Object::String(s)) => s.as_str().to_string(),
                _ => std::path::Path::new(&path)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default(),
            };
            let clean_name = clean_zip_name(&name);
            if clean_name.is_empty() {
                continue;
            }
            if std::path::Path::new(&path).is_dir() {
                if let Ok(walker) = walkdir_collect(&path) {
                    for (rel, abs) in walker {
                        let entry_name = format!("{}/{}", clean_name, rel.replace('\\', "/"));
                        if let Ok(data) = fs::read(&abs) {
                            let _ = zip.start_file(entry_name, options);
                            let _ = zip.write_all(&data);
                        }
                    }
                }
                continue;
            }
            match fs::read(&path) {
                Ok(data) => {
                    let _ = zip.start_file(clean_name.clone(), options);
                    let _ = zip.write_all(&data);
                }
                Err(e) => return new_error(ctx.pos.clone(), format!("zip.create: {}", e)),
            }
        }
    }
    match zip.finish() {
        Ok(_) => Object::Undefined,
        Err(e) => new_error(ctx.pos.clone(), format!("zip.create: {}", e)),
    }
}

/// Reject path-traversal targets so an entry's name cannot escape `dest`.
fn safe_zip_target(dest: &str, name: &str) -> Result<std::path::PathBuf, String> {
    let clean = clean_zip_name(name);
    if clean.is_empty() {
        return Err("empty name".to_string());
    }
    let dest_abs = fs::canonicalize(dest).unwrap_or_else(|_| std::path::PathBuf::from(dest));
    let target = dest_abs.join(&clean);
    let canonical = target.ancestors().last().unwrap_or(&target).to_path_buf();
    let _ = canonical;
    if target
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err("path traversal".to_string());
    }
    Ok(target)
}

pub(crate) fn clean_zip_name(name: &str) -> String {
    let slashed = name.replace('\\', "/");
    let stripped = slashed.strip_prefix('/').unwrap_or(&slashed);
    let cleaned = std::path::Path::new(stripped)
        .components()
        .filter(|c| !matches!(c, std::path::Component::ParentDir))
        .collect::<std::path::PathBuf>();
    let s = cleaned.to_string_lossy().to_string();
    if s == "." || s == ".." {
        String::new()
    } else {
        s
    }
}

/// Collect (relative_path, absolute_path) pairs under `root`, depth-first.
fn walkdir_collect(root: &str) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    walkdir_inner(root, root, &mut out)?;
    Ok(out)
}

pub(crate) fn walkdir_inner(
    root: &str,
    dir: &str,
    out: &mut Vec<(String, String)>,
) -> Result<(), String> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => return Err(e.to_string()),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let abs = path.to_string_lossy().to_string();
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .to_string();
        if path.is_dir() {
            walkdir_inner(root, &abs, out)?;
        } else {
            out.push((rel, abs));
        }
    }
    Ok(())
}

// ===========================================================================
// P7 batch: buffer / events / jwt / mime / net/ip / retry / stream.
// Pure-algorithm modules (no nested VM execution, no real async) — CI friendly.
// ===========================================================================

// ---------------------------------------------------------------------------
// buffer: byte buffers constructed from strings/arrays, with instance methods.
// Reuses the existing make_buffer helper (Hash carrying __buffer_data__).
// ---------------------------------------------------------------------------

/// Tile `src` to exactly `size` bytes (reserved for image/texture helpers).
#[allow(dead_code)] // not yet wired into a @std module export
pub(crate) fn tile_bytes(src: &[u8], size: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(size);
    for i in 0..size {
        out.push(src[i % src.len()]);
    }
    out
}
