use super::super::helpers::*;
use crate::object::{new_error, CallContext, Object};

pub(crate) fn watch_module() -> Object {
    module(vec![("file", native("watch.file", watch_file))])
}

/// watch.file(path, callback, [options]) -> 同步轮询直到文件修改。
///
/// 在纯单线程运行时模型下无法启动后台 goroutine 回调，因此采用同步语义：
/// 阻塞当前脚本，轮询文件的修改时间，一旦变化立即同步调用回调函数。
/// 可通过 options.duration（毫秒，默认 1000）和 options.timeout（毫秒，默认无限）控制。
fn watch_file(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "watch.file", args);
    let path = match reader.required_string(0, "path") {
        Ok(s) => s,
        Err(e) => return e,
    };
    let callback = match args.get(1) {
        Some(value) if is_callable(value) => value.clone(),
        _ => return new_error(ctx.pos.clone(), "watch.file expects function callback"),
    };

    let mut interval_ms: u64 = 1000;
    let mut timeout_ms: Option<u64> = None;
    if let Some(opts) = reader.object_view(2) {
        let opts = ObjectView::new(&opts);
        if let Some(n) = opts.number("interval") {
            interval_ms = n as u64;
        }
        if let Some(n) = opts.number("duration") {
            interval_ms = n as u64;
        }
        if let Some(n) = opts.number("timeout") {
            timeout_ms = Some(n as u64);
        }
    }
    if interval_ms == 0 {
        interval_ms = 1000;
    }

    // 记录初始修改时间（仅与下一次轮询的 mtime 比较，不再回写）。
    let last_mod = std::fs::metadata(&path)
        .ok()
        .and_then(|m| m.modified().ok());

    let start = std::time::Instant::now();
    let interval = std::time::Duration::from_millis(interval_ms);
    loop {
        if let Some(t) = timeout_ms {
            if start.elapsed().as_millis() as u64 >= t {
                // 超时：返回 false 表示未检测到变化。
                return Object::Boolean(false);
            }
        }
        std::thread::sleep(interval);

        let current_mod = match std::fs::metadata(&path) {
            Ok(m) => m.modified().ok(),
            Err(_) => continue,
        };

        let changed = match (last_mod, current_mod) {
            (Some(prev), Some(cur)) => cur > prev,
            (None, Some(_)) => true,
            _ => false,
        };

        if changed {
            // 同步调用回调。该分支随即 return，`current_mod` 无需回写
            // （`last_mod` 不会被再次读取），省略可避免 dead-store 告警。
            let _ = call_script_function(&callback, ctx.env, &[]);
            return Object::Boolean(true);
        }
    }
}

// ============================================================================
// @std/async - async concurrency primitives
// ----------------------------------------------------------------------------
// Rust 版本是单线程 Rc<RefCell> 模型，无法跨线程执行用户函数（借用检查器
// 禁止跨线程共享 Rc）。此模块提供与 Go 版本 API 兼容的语义：
//   - fetchAsync/getAsync/postAsync：同步执行 HTTP 请求，返回已 resolve 的 Promise。
//     与 Go 版本一样返回 Promise，便于 await/then 链式调用。
//   - runWorker：在隔离 scope 同步求值 fn(args)，返回已 resolve 的 Promise。
// 虽然不是真正的并行，但保持了 API 形状一致，迁移代码无需改动。
// ============================================================================
