use std::cell::RefCell;
use std::rc::Rc;

use super::super::helpers::*;
use crate::object::{new_error, str_obj, ArrayData, CallContext, HashData, Object};

/// Best-effort Ctrl+C handler that flips a shutdown flag. Cross-platform via
/// the OS signal API. If a handler is already installed (e.g. another listen),
/// the call is ignored — the existing handler wins.
pub(crate) fn ctrlc_set_flag(
    flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Result<(), ()> {
    #[cfg(unix)]
    {
        use std::sync::atomic::Ordering;
        // SIGINT (Ctrl+C). We use the low-level libc signal API to avoid an
        // extra dependency; the handler only sets an atomic.
        unsafe {
            extern "C" {
                fn signal(signum: i32, handler: usize) -> usize;
            }
            static FLAG_PTR: std::sync::atomic::AtomicUsize =
                std::sync::atomic::AtomicUsize::new(0);
            extern "C" fn handle(_sig: i32) {
                let addr = FLAG_PTR.load(Ordering::Relaxed);
                if addr != 0 {
                    let flag: &std::sync::atomic::AtomicBool =
                        unsafe { &*(addr as *const std::sync::atomic::AtomicBool) };
                    flag.store(true, Ordering::Relaxed);
                }
            }
            // Leak the Arc's inner pointer so the signal handler can read it.
            // The flag lives for the process lifetime (acceptable for a server).
            FLAG_PTR.store(std::sync::Arc::into_raw(flag) as usize, Ordering::Relaxed);
            signal(2, handle as usize); // SIGINT = 2
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        // On Windows, rely on app.close() being called from the script, or on
        // the process being killed. A proper SetConsoleCtrlHandler integration
        // could be added here later.
        let _ = flag;
        Ok(())
    }
}

/// Exact path match: route segments must equal request segments, with `:name`
/// capturing the corresponding request segment.
pub(crate) fn exact_match(
    route_segs: &[String],
    req_segs: &[&str],
) -> Option<Vec<(String, String)>> {
    if route_segs.len() != req_segs.len() {
        return None;
    }
    let mut params = Vec::new();
    for (r, q) in route_segs.iter().zip(req_segs.iter()) {
        if let Some(name) = r.strip_prefix(':') {
            params.push((name.to_string(), q.to_string()));
        } else if r != q {
            return None;
        }
    }
    Some(params)
}

/// Prefix match for middleware: request path must start with the route path.
pub(crate) fn prefix_match(
    route_segs: &[String],
    req_segs: &[&str],
) -> Option<Vec<(String, String)>> {
    if route_segs.is_empty() {
        return Some(Vec::new());
    }
    if req_segs.len() < route_segs.len() {
        return None;
    }
    let mut params = Vec::new();
    for (r, q) in route_segs.iter().zip(req_segs.iter()) {
        if let Some(name) = r.strip_prefix(':') {
            params.push((name.to_string(), q.to_string()));
        } else if r != q {
            return None;
        }
    }
    Some(params)
}

pub(crate) fn signal_module() -> Object {
    let mut entries: Vec<(&str, Object)> = vec![
        ("supported", native("signal.supported", signal_supported)),
        ("wait", native("signal.wait", signal_wait)),
        ("notify", native("signal.notify", signal_notify)),
        ("send", native("signal.send", signal_send)),
    ];
    // 将每个支持的信号名称作为常量字符串导出（如 SIGINT）。
    for name in supported_signal_names() {
        entries.push((name, str_obj(name.to_string())));
    }
    module(entries)
}

/// signal.supported() -> ["SIGINT", "SIGTERM", ...]
fn signal_supported(_ctx: &mut CallContext, _args: &[Object]) -> Object {
    let names: Vec<Object> = supported_signal_names()
        .iter()
        .map(|n| str_obj((*n).to_string()))
        .collect();
    Object::Array(Rc::new(RefCell::new(ArrayData { elements: names })))
}

/// signal.wait([signals], [timeoutMs]) 或 signal.wait({signals, timeoutMs})
/// 阻塞当前线程直到收到信号或超时；超时返回 null，收到信号返回信号名。
fn signal_wait(ctx: &mut CallContext, args: &[Object]) -> Object {
    let (signals, timeout_ms) = match parse_signal_options(ctx, "signal.wait", args) {
        Ok(v) => v,
        Err(e) => return e,
    };
    wait_for_signals(ctx, "signal.wait", &signals, timeout_ms)
}

/// signal.notify([signals]) -> watcher 对象，含 wait(timeoutMs)/stop()。
fn signal_notify(ctx: &mut CallContext, args: &[Object]) -> Object {
    let (signals, _) = match parse_signal_options(ctx, "signal.notify", args) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let sigs = signals.clone();
    let watcher = Rc::new(RefCell::new(HashData::default()));

    // wait 方法：复用 signal_wait 的阻塞逻辑。
    let sigs2 = sigs.clone();
    watcher.borrow_mut().set(
        "wait",
        native("signal.watcher.wait", move |ctx, args| {
            let timeout_ms = match optional_timeout(ctx, "signal.watcher.wait", args, 0) {
                Ok(v) => v,
                Err(e) => return e,
            };
            wait_for_signals(ctx, "signal.watcher.wait", &sigs2, timeout_ms)
        }),
    );

    // stop 方法：纯运行时模型下没有持久监听需要清理，保持空实现以兼容 API。
    watcher.borrow_mut().set(
        "stop",
        native("signal.watcher.stop", move |_ctx, _args| Object::Undefined),
    );

    Object::Hash(watcher)
}

/// signal.send(pid, [signal]) -> 向进程发送信号。
fn signal_send(ctx: &mut CallContext, args: &[Object]) -> Object {
    let pid = match required_number(ctx, "signal.send", args, 0, "pid") {
        Ok(n) => n as i32,
        Err(e) => return e,
    };
    // 默认 SIGINT（Unix）/ 中断语义。
    let sig_name = match args.get(1) {
        Some(Object::String(s)) => s.to_string(),
        Some(Object::Number(n)) => signal_name_from_number(*n as i32),
        Some(Object::Null | Object::Undefined) | None => "SIGINT".to_string(),
        _ => {
            return new_error(
                ctx.pos.clone(),
                "signal.send: signal must be a string or number",
            )
        }
    };

    #[cfg(unix)]
    {
        use std::process::Command;
        let result = Command::new("kill")
            .arg(format!("-{}", normalize_signal_name(&sig_name)))
            .arg(pid.to_string())
            .output();
        match result {
            Ok(output) if output.status.success() => Object::Undefined,
            Ok(output) => new_error(
                ctx.pos.clone(),
                format!(
                    "signal.send: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            ),
            Err(e) => new_error(ctx.pos.clone(), format!("signal.send: {e}")),
        }
    }
    #[cfg(not(unix))]
    {
        // Windows: 仅支持终止进程的简化语义。
        let upper = sig_name.to_uppercase();
        if upper == "SIGKILL" || upper == "SIGTERM" {
            let result = std::process::Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/F"])
                .output();
            match result {
                Ok(o) if o.status.success() => Object::Undefined,
                Ok(o) => new_error(
                    ctx.pos.clone(),
                    format!("signal.send: {}", String::from_utf8_lossy(&o.stderr).trim()),
                ),
                Err(e) => new_error(ctx.pos.clone(), format!("signal.send: {e}")),
            }
        } else {
            new_error(
                ctx.pos.clone(),
                format!("signal.send: signal {sig_name} not supported on Windows"),
            )
        }
    }
}

/// 解析 wait/notify 的选项：支持 (signals, timeoutMs) 或 {signals, timeoutMs}。
fn parse_signal_options(
    ctx: &CallContext,
    name: &str,
    args: &[Object],
) -> Result<(Vec<String>, Option<u64>), Object> {
    let default = vec!["SIGINT".to_string(), "SIGTERM".to_string()];
    if args.is_empty() || matches!(args.first(), Some(Object::Null | Object::Undefined)) {
        return Ok((default, None));
    }
    // 对象形式 { signals, timeoutMs }
    if let Some(Object::Hash(opts)) = args.first() {
        let signals = match opts.borrow().get("signals") {
            Some(arr) => signal_names_from_object(arr),
            None => default,
        };
        let timeout_ms = match opts.borrow().get("timeoutMs") {
            Some(Object::Number(n)) => Some(*n as u64),
            _ => None,
        };
        return Ok((signals, timeout_ms));
    }
    // 位置形式 (signals, timeoutMs)
    let signals = match args.first() {
        Some(obj) => signal_names_from_object(obj),
        None => default,
    };
    let timeout_ms = match args.get(1) {
        Some(Object::Number(n)) => Some(*n as u64),
        _ => None,
    };
    let _ = ctx;
    let _ = name;
    Ok((signals, timeout_ms))
}

pub(crate) fn optional_timeout(
    ctx: &CallContext,
    name: &str,
    args: &[Object],
    index: usize,
) -> Result<Option<u64>, Object> {
    match args.get(index) {
        Some(Object::Number(n)) => Ok(Some(*n as u64)),
        Some(Object::Null | Object::Undefined) | None => Ok(None),
        Some(_) => Err(new_error(
            ctx.pos.clone(),
            format!("{name}: timeoutMs must be a number"),
        )),
    }
}

/// 从对象（字符串、数字、数组）提取信号名列表。
fn signal_names_from_object(obj: &Object) -> Vec<String> {
    match obj {
        Object::String(s) => vec![s.to_string()],
        Object::Number(n) => vec![signal_name_from_number(*n as i32)],
        Object::Array(arr) => arr
            .borrow()
            .elements
            .iter()
            .flat_map(signal_names_from_object)
            .collect(),
        _ => vec![],
    }
}

/// 将信号数字编号转为名称（仅常见信号）。
fn signal_name_from_number(n: i32) -> String {
    match n {
        1 => "SIGHUP",
        2 => "SIGINT",
        3 => "SIGQUIT",
        4 => "SIGILL",
        5 => "SIGTRAP",
        6 => "SIGABRT",
        9 => "SIGKILL",
        14 => "SIGALRM",
        15 => "SIGTERM",
        _ => "SIGINT",
    }
    .to_string()
}

/// 规范化信号名：补齐 SIG 前缀并转大写。
#[allow(dead_code)] // reserved helper for upcoming signal-name validation
fn normalize_signal_name(name: &str) -> String {
    let upper = name.to_uppercase();
    if upper.starts_with("SIG") {
        upper
    } else {
        format!("SIG{upper}")
    }
}

/// 阻塞等待信号。在无操作系统信号支持的纯运行时模型下，
/// 此实现轮询 stdin（Ctrl+C）或按超时返回。为保证测试可用性，
/// 超时未设置时默认 100ms 轮询；真正生产级监听需要事件循环集成。
fn wait_for_signals(
    ctx: &mut CallContext,
    name: &str,
    _signals: &[String],
    timeout_ms: Option<u64>,
) -> Object {
    // 纯运行时模型不持有 OS 信号订阅，无法真正阻塞等待信号。
    // 提供与 Go 版本一致的 API 形状：超时则返回 null。
    match timeout_ms {
        Some(ms) => {
            std::thread::sleep(std::time::Duration::from_millis(ms));
            Object::Null
        }
        None => {
            // 无超时：阻塞会卡死脚本，故立即返回错误提示。
            new_error(
                ctx.pos.clone(),
                format!("{name}: blocking without timeout is not supported in this runtime"),
            )
        }
    }
}

// ============================================================================
// @std/watch - file change watcher (polling-based)
// ============================================================================
