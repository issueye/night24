// Package file format implementation (.gspkg)
//
// .gspkg 文件格式：
// - 基于 ZIP 压缩格式
// - 包含项目的所有源文件和资源
// - 保留目录结构
// - 可以直接被 gs 命令执行

use std::fs::{self, File};
use std::io::{self, Read, Seek, Write};
use std::path::{Path, PathBuf};
use zip::write::FileOptions;
use zip::{CompressionMethod, ZipWriter};

/// .gspkg 文件扩展名
pub const EXTENSION: &str = ".gspkg";

/// 打包项目目录为 .gspkg 文件
pub fn pack_directory<P: AsRef<Path>>(dir: P, output: Option<P>) -> io::Result<PathBuf> {
    let dir = dir.as_ref();
    let abs_dir = fs::canonicalize(dir)?;

    // 确定输出文件名
    let output_path = match output {
        Some(out) => {
            let out = out.as_ref();
            if out.is_absolute() {
                out.to_path_buf()
            } else {
                std::env::current_dir()?.join(out)
            }
        }
        None => {
            let name = abs_dir
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("package");
            abs_dir
                .parent()
                .unwrap_or(&abs_dir)
                .join(format!("{}{}", name, EXTENSION))
        }
    };

    // 创建输出目录
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }

    // 创建 ZIP 文件
    let file = File::create(&output_path)?;
    let mut zip = ZipWriter::new(file);

    let options = FileOptions::default().compression_method(CompressionMethod::Deflated);

    // 递归添加文件
    add_dir_to_zip(&mut zip, &abs_dir, &abs_dir, options)?;

    zip.finish()?;

    Ok(output_path)
}

/// 递归添加目录中的文件到 ZIP
fn add_dir_to_zip<W: Write + Seek>(
    zip: &mut ZipWriter<W>,
    base_dir: &Path,
    current_dir: &Path,
    options: FileOptions<'_, ()>,
) -> io::Result<()> {
    for entry in fs::read_dir(current_dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = path.strip_prefix(base_dir).map_err(io::Error::other)?;

        // 跳过特定文件和目录
        if should_skip(&path) {
            continue;
        }

        let metadata = entry.metadata()?;

        if metadata.is_file() {
            // 添加文件
            let name_str = name.to_str().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "invalid UTF-8 in filename")
            })?;

            zip.start_file(name_str, options)?;
            let mut file = File::open(&path)?;
            io::copy(&mut file, zip)?;
        } else if metadata.is_dir() {
            // 递归添加子目录
            add_dir_to_zip(zip, base_dir, &path, options)?;
        }
    }

    Ok(())
}

/// 判断是否应该跳过文件/目录
fn should_skip(path: &Path) -> bool {
    let name = match path.file_name().and_then(|n| n.to_str()) {
        Some(n) => n,
        None => return true,
    };

    // 跳过隐藏文件和目录
    if name.starts_with('.') {
        return true;
    }

    // 跳过特定目录
    matches!(
        name,
        "node_modules" | "dist" | "target" | "build" | "tmp" | "temp"
    )
}

/// 从 .gspkg 文件中提取到临时目录
pub fn extract_package<P: AsRef<Path>>(package: P) -> io::Result<PathBuf> {
    let package = package.as_ref();

    // 创建临时目录
    let temp_dir = std::env::temp_dir().join(format!("gts_{}", uuid_simple()));
    fs::create_dir_all(&temp_dir)?;

    // 打开 ZIP 文件
    let file = File::open(package)?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    // 提取所有文件
    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        let outpath = temp_dir.join(file.name());

        if file.is_dir() {
            fs::create_dir_all(&outpath)?;
        } else {
            if let Some(parent) = outpath.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut outfile = File::create(&outpath)?;
            io::copy(&mut file, &mut outfile)?;
        }
    }

    Ok(temp_dir)
}

/// 生成简单的 UUID（用于临时目录）
fn uuid_simple() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    format!("{:x}{:x}", now.as_secs(), now.subsec_nanos())
}

/// Resolve a module path (without extension, or with `.gs`/`.json`) inside a
/// `.gspkg` archive to its archive entry name. Mirrors the source-path
/// resolution rules: exact file, append `.gs`/`.json`, or `<dir>/main.gs`.
/// Returns the matching archive entry name, or `None` if not found.
///
/// `archive_path` is the on-disk path to the `.gspkg` zip; `entry` is the
/// in-archive logical path (forward-slash, may omit extension).
pub fn resolve_archive_entry_name(archive_path: &Path, entry: &str) -> Option<String> {
    let file = File::open(archive_path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;
    // Normalize each entry to forward slashes so cross-platform packed
    // archives (Windows packs with backslashes) resolve consistently.
    // NOTE: use `name()` (raw) rather than `enclosed_name()` — the latter
    // rejects backslash path separators that Windows-packed archives use.
    let normalize = |s: &str| s.replace('\\', "/").trim_start_matches('/').to_string();
    let names: Vec<String> = (0..archive.len())
        .filter_map(|i| archive.by_index(i).ok().map(|f| normalize(f.name())))
        .collect();
    let candidate = normalize(entry);
    // 1. exact match
    if names.iter().any(|n| n == &candidate) {
        return Some(candidate);
    }
    // 2. append .gs / .json
    for ext in ["gs", "json"] {
        let with_ext = format!("{candidate}.{ext}");
        if names.iter().any(|n| n == &with_ext) {
            return Some(with_ext);
        }
    }
    // 3. directory entry → <dir>/main.gs
    let dir_entry = format!("{candidate}/main.gs");
    if names.iter().any(|n| n == &dir_entry) {
        return Some(dir_entry);
    }
    None
}

/// Read an archive entry's bytes (source text) by entry name.
pub fn read_archive_entry(archive_path: &Path, entry_name: &str) -> io::Result<Vec<u8>> {
    let file = File::open(archive_path)?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    for i in 0..archive.len() {
        let mut zf = archive
            .by_index(i)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        // Use the raw name (backslashes on Windows-packed archives).
        let name_norm = zf.name().replace('\\', "/");
        if name_norm == entry_name {
            let mut buf = Vec::new();
            io::copy(&mut zf, &mut buf)?;
            return Ok(buf);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!("archive entry '{entry_name}' not found"),
    ))
}

/// 将 .gspkg 附加到可执行文件末尾
pub fn append_package_to_executable<P: AsRef<Path>>(
    stub: P,
    package: P,
    output: P,
) -> io::Result<()> {
    let stub = stub.as_ref();
    let package = package.as_ref();
    let output = output.as_ref();

    // 读取 stub（当前可执行文件）
    let mut stub_data = Vec::new();
    File::open(stub)?.read_to_end(&mut stub_data)?;

    // 读取 package
    let mut package_data = Vec::new();
    File::open(package)?.read_to_end(&mut package_data)?;

    // 写入输出文件
    let mut out = File::create(output)?;
    out.write_all(&stub_data)?;
    out.write_all(&package_data)?;

    // 写入魔术字节和偏移量
    let package_offset = stub_data.len() as u64;
    out.write_all(b"GTSPKG\x00\x00")?; // 魔术字节
    out.write_all(&package_offset.to_le_bytes())?; // 包偏移量

    // 在 Unix 系统上设置可执行权限
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(output)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(output, perms)?;
    }

    Ok(())
}

/// 检查当前可执行文件是否包含附加的包
pub fn current_executable_has_appended_package() -> bool {
    match std::env::current_exe() {
        Ok(exe) => has_appended_package(&exe).unwrap_or(false),
        Err(_) => false,
    }
}

/// 检查文件是否包含附加的包
pub fn has_appended_package<P: AsRef<Path>>(path: P) -> io::Result<bool> {
    let mut file = File::open(path.as_ref())?;

    // 定位到文件末尾前 16 字节
    let len = file.metadata()?.len();
    if len < 16 {
        return Ok(false);
    }

    file.seek(io::SeekFrom::End(-16))?;

    // 读取魔术字节
    let mut magic = [0u8; 8];
    file.read_exact(&mut magic)?;

    Ok(&magic == b"GTSPKG\x00\x00")
}

/// 从可执行文件中提取附加的包
pub fn extract_appended_package<P: AsRef<Path>>(exe: P) -> io::Result<PathBuf> {
    let exe = exe.as_ref();

    // 打开文件
    let mut file = File::open(exe)?;

    // 读取文件末尾的元数据
    let len = file.metadata()?.len();
    if len < 16 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "file too small"));
    }

    file.seek(io::SeekFrom::End(-16))?;

    // 验证魔术字节
    let mut magic = [0u8; 8];
    file.read_exact(&mut magic)?;
    if &magic != b"GTSPKG\x00\x00" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "no appended package found",
        ));
    }

    // 读取包偏移量
    let mut offset_bytes = [0u8; 8];
    file.read_exact(&mut offset_bytes)?;
    let offset = u64::from_le_bytes(offset_bytes);

    // 提取包数据
    file.seek(io::SeekFrom::Start(offset))?;
    let package_size = len - offset - 16;
    let mut package_data = vec![0u8; package_size as usize];
    file.read_exact(&mut package_data)?;

    // 写入临时文件
    let temp_pkg = std::env::temp_dir().join(format!("gts_pkg_{}.gspkg", uuid_simple()));
    fs::write(&temp_pkg, package_data)?;

    // 提取到目录
    extract_package(&temp_pkg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uuid_simple() {
        let uuid1 = uuid_simple();
        let uuid2 = uuid_simple();
        assert_ne!(uuid1, uuid2);
        assert!(uuid1.len() > 8);
    }

    #[test]
    fn test_should_skip() {
        assert!(should_skip(Path::new(".git")));
        assert!(should_skip(Path::new(".gitignore")));
        assert!(should_skip(Path::new("node_modules")));
        assert!(should_skip(Path::new("dist")));
        assert!(!should_skip(Path::new("src")));
        assert!(!should_skip(Path::new("main.gs")));
    }
}
