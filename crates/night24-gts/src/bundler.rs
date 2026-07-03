// Module bundler implementation
//
// 功能：将多个模块合并为单个 .gs 文件
// 用途：
// - 简化部署（单文件）
// - 减少文件 I/O
// - 便于分享代码片段

use std::collections::{HashSet, VecDeque};
use std::fs;
use std::io::{self};
use std::path::{Path, PathBuf};

/// 模块依赖图
#[derive(Debug)]
pub struct DependencyGraph {
    /// 模块路径 -> 依赖列表
    dependencies: std::collections::HashMap<PathBuf, Vec<PathBuf>>,
    /// 已访问的模块
    visited: HashSet<PathBuf>,
    /// 模块内容缓存
    contents: std::collections::HashMap<PathBuf, String>,
}

impl Default for DependencyGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl DependencyGraph {
    pub fn new() -> Self {
        Self {
            dependencies: std::collections::HashMap::new(),
            visited: HashSet::new(),
            contents: std::collections::HashMap::new(),
        }
    }

    /// 从入口文件构建依赖图
    pub fn build_from_entry(&mut self, entry: &Path) -> io::Result<()> {
        let entry = fs::canonicalize(entry)?;
        let mut queue = VecDeque::new();
        queue.push_back(entry.clone());

        while let Some(path) = queue.pop_front() {
            if self.visited.contains(&path) {
                continue;
            }
            self.visited.insert(path.clone());

            // 读取文件内容
            let content = fs::read_to_string(&path)?;
            self.contents.insert(path.clone(), content.clone());

            // 解析依赖
            let deps = self.extract_dependencies(&content, &path)?;

            // 添加到队列
            for dep in &deps {
                if !self.visited.contains(dep) {
                    queue.push_back(dep.clone());
                }
            }

            self.dependencies.insert(path, deps);
        }

        Ok(())
    }

    /// 提取文件中的依赖
    fn extract_dependencies(&self, content: &str, current_file: &Path) -> io::Result<Vec<PathBuf>> {
        let mut deps = Vec::new();

        // 简单的正则匹配 require() 和 import
        for line in content.lines() {
            let line = line.trim();

            // require("./module")
            if let Some(module) = extract_require(line) {
                if let Some(path) = self.resolve_module(module, current_file)? {
                    deps.push(path);
                }
            }

            // import ... from "./module"
            if let Some(module) = extract_import(line) {
                if let Some(path) = self.resolve_module(module, current_file)? {
                    deps.push(path);
                }
            }
        }

        Ok(deps)
    }

    /// 解析模块路径
    fn resolve_module(&self, module: &str, current_file: &Path) -> io::Result<Option<PathBuf>> {
        // 跳过标准库模块
        if module.starts_with('@') {
            return Ok(None);
        }

        let current_dir = current_file.parent().unwrap_or_else(|| Path::new("."));

        // 相对路径
        if module.starts_with('.') {
            let path = current_dir.join(module);

            // 尝试直接路径
            if path.exists() {
                return Ok(Some(fs::canonicalize(path)?));
            }

            // 尝试添加 .gs 扩展名
            let with_ext = path.with_extension("gs");
            if with_ext.exists() {
                return Ok(Some(fs::canonicalize(with_ext)?));
            }

            // 尝试 index.gs
            let index = path.join("index.gs");
            if index.exists() {
                return Ok(Some(fs::canonicalize(index)?));
            }
        }

        Ok(None)
    }

    /// 获取拓扑排序后的模块列表
    pub fn topological_sort(&self, entry: &Path) -> io::Result<Vec<PathBuf>> {
        let entry = fs::canonicalize(entry)?;
        let mut result = Vec::new();
        let mut visited = HashSet::new();
        let mut visiting = HashSet::new();

        self.visit_node(&entry, &mut visited, &mut visiting, &mut result)?;

        Ok(result)
    }

    fn visit_node(
        &self,
        node: &Path,
        visited: &mut HashSet<PathBuf>,
        visiting: &mut HashSet<PathBuf>,
        result: &mut Vec<PathBuf>,
    ) -> io::Result<()> {
        if visited.contains(node) {
            return Ok(());
        }

        if visiting.contains(node) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Circular dependency detected: {}", node.display()),
            ));
        }

        visiting.insert(node.to_path_buf());

        if let Some(deps) = self.dependencies.get(node) {
            for dep in deps {
                self.visit_node(dep, visited, visiting, result)?;
            }
        }

        visiting.remove(node);
        visited.insert(node.to_path_buf());
        result.push(node.to_path_buf());

        Ok(())
    }

    /// 获取模块内容
    pub fn get_content(&self, path: &Path) -> Option<&str> {
        self.contents.get(path).map(|s| s.as_str())
    }
}

/// 从字符串中提取 require() 调用
fn extract_require(line: &str) -> Option<&str> {
    // 简化版：require("module") 或 require('module')
    if let Some(start) = line.find("require(") {
        let after = &line[start + 8..];
        if let Some(quote_start) = after.find(['"', '\'']) {
            let quote = after.as_bytes()[quote_start] as char;
            let module_start = quote_start + 1;
            if let Some(quote_end) = after[module_start..].find(quote) {
                return Some(&after[module_start..module_start + quote_end]);
            }
        }
    }
    None
}

/// 从字符串中提取 import from
fn extract_import(line: &str) -> Option<&str> {
    // 简化版：import ... from "module"
    if let Some(from_pos) = line.find(" from ") {
        let after = &line[from_pos + 6..].trim();
        if let Some(quote_start) = after.find(['"', '\'']) {
            let quote = after.as_bytes()[quote_start] as char;
            let module_start = quote_start + 1;
            if let Some(quote_end) = after[module_start..].find(quote) {
                return Some(&after[module_start..module_start + quote_end]);
            }
        }
    }
    None
}

/// 将模块内容转换为 IIFE（立即执行函数表达式）
fn wrap_module(path: &Path, content: &str) -> String {
    let _name = path
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or("module");

    format!(
        "// Module: {}\n(function() {{\n{}\n}})();\n\n",
        path.display(),
        content
    )
}

/// 移除模块中的 require/import 语句
fn remove_imports(content: &str) -> String {
    let mut result = String::new();

    for line in content.lines() {
        let trimmed = line.trim();

        // 跳过 require 和 import 语句
        if trimmed.starts_with("const") && trimmed.contains("require(") {
            continue;
        }
        if trimmed.starts_with("let") && trimmed.contains("require(") {
            continue;
        }
        if trimmed.starts_with("var") && trimmed.contains("require(") {
            continue;
        }
        if trimmed.starts_with("import ") {
            continue;
        }

        result.push_str(line);
        result.push('\n');
    }

    result
}

/// 打包模块
pub fn bundle_modules(entry: &Path, output: Option<&Path>) -> io::Result<String> {
    let mut graph = DependencyGraph::new();

    // 构建依赖图
    graph.build_from_entry(entry)?;

    // 获取拓扑排序
    let sorted = graph.topological_sort(entry)?;

    // 生成打包后的代码
    let mut bundled = String::new();

    // 添加头部注释
    bundled.push_str(&format!(
        "// Bundled by GoScript\n\
         // Entry: {}\n\
         // Modules: {}\n\
         // Generated: {}\n\n",
        entry.display(),
        sorted.len(),
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
    ));

    // 添加每个模块
    for module_path in &sorted {
        if let Some(content) = graph.get_content(module_path) {
            // 移除 import/require
            let cleaned = remove_imports(content);

            // 包装为 IIFE（除了入口文件）
            if module_path != entry {
                bundled.push_str(&wrap_module(module_path, &cleaned));
            } else {
                // 入口文件直接添加
                bundled.push_str(&format!("// Entry module: {}\n", entry.display()));
                bundled.push_str(&cleaned);
            }
        }
    }

    // 写入输出文件（如果指定）
    if let Some(out) = output {
        fs::write(out, &bundled)?;
    }

    Ok(bundled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_require() {
        assert_eq!(
            extract_require("const x = require('./module');"),
            Some("./module")
        );
        assert_eq!(
            extract_require("const x = require(\"./module\");"),
            Some("./module")
        );
        assert_eq!(extract_require("require('./test')"), Some("./test"));
        assert_eq!(extract_require("no require here"), None);
    }

    #[test]
    fn test_extract_import() {
        assert_eq!(
            extract_import("import { x } from './module';"),
            Some("./module")
        );
        assert_eq!(
            extract_import("import * as x from \"./module\";"),
            Some("./module")
        );
        assert_eq!(extract_import("no import here"), None);
    }

    #[test]
    fn test_remove_imports() {
        let input = r#"
const x = require('./module');
import { y } from './other';
function test() {
    return 42;
}
"#;
        let output = remove_imports(input);
        assert!(!output.contains("require"));
        assert!(!output.contains("import"));
        assert!(output.contains("function test()"));
    }
}
