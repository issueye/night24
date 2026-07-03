//! The VirtualMachine: owns the global scope, async coordination, and the
//! evaluator/importer callbacks used to break cycles between the object layer
//! and the evaluator.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU8, Ordering};
use std::time::{Duration, Instant};

use crate::ast::Position;
use crate::async_runtime::{
    AsyncCompletion, AsyncCompletionData, AsyncCompletionId, AsyncCompletionQueue,
    AsyncCompletionResult, AsyncCompletionSender,
};

use super::promise::Promise;
use super::value::{new_error, Object};

/// The evaluator callback type. Given an AST node and an environment, produce a
/// value. Stored on the VM so runtime objects (closures, Promise continuations)
/// can drive evaluation without a direct dependency on the evaluator module.
pub type EvaluatorFn = dyn Fn(NodeRef, &EnvRef, &Rc<VirtualMachine>) -> Object;

/// An owned AST node reference that can be held by the evaluator callback.
#[derive(Clone)]
pub enum NodeRef {
    Stmt(Rc<crate::ast::Stmt>),
    Expr(Rc<crate::ast::Expr>),
    Program(Rc<crate::ast::Program>),
}

/// Importer callback: load a module by specifier.
pub type ImporterFn = dyn Fn(&EnvRef, &str) -> Result<Object, Object>;

pub use super::value::EnvRef;

/// Execution backend selection. 0 = legacy tree-walker, 1 = bytecode VM (default).
/// Stored as `AtomicU8` so the VM can be queried without borrowing.
pub const EXEC_MODE_TREEWALK: u8 = 0;
pub const EXEC_MODE_BYTECODE: u8 = 1;

/// The runtime for one isolated script execution.
pub struct VirtualMachine {
    globals: RefCell<HashMap<String, Object>>,
    output: RefCell<VmOutput>,
    pub argv: RefCell<Vec<String>>,
    pub type_check: AtomicBool,
    /// D4.2: module allowlist for sandbox mode. When non-empty, only listed
    /// `@std/*` modules can be loaded; others are rejected. "Safe" modules
    /// (path, json, text, etc.) are always allowed regardless of the list.
    allowed_modules: RefCell<Option<Vec<String>>>,
    /// Which execution backend to use. See `EXEC_MODE_*` constants.
    pub exec_mode: AtomicU8,
    next_timer: AtomicI64,
    next_async_completion: AtomicI64,
    /// Pending async work counter; the host drains this before returning.
    async_pending: RefCell<usize>,
    async_completions: AsyncCompletionQueue,
    async_promises: RefCell<HashMap<AsyncCompletionId, Rc<Promise>>>,
    pub bootstrap_source: RefCell<String>,
    evaluator: RefCell<Option<Rc<EvaluatorFn>>>,
    importer: RefCell<Option<Rc<ImporterFn>>>,
    deadline: RefCell<Option<Instant>>,
    /// Instruction limit (D4.1). When set, the dispatch loop aborts if
    /// `instruction_count` exceeds this. Acts as a memory/CPU resource guard.
    /// `0` = disabled (no limit).
    instruction_limit: Cell<u64>,
}

impl VirtualMachine {
    pub fn new() -> Rc<VirtualMachine> {
        Rc::new(VirtualMachine {
            globals: RefCell::new(HashMap::new()),
            output: RefCell::new(VmOutput::default()),
            argv: RefCell::new(Vec::new()),
            type_check: AtomicBool::new(false),
            allowed_modules: RefCell::new(None),
            exec_mode: AtomicU8::new(EXEC_MODE_BYTECODE),
            next_timer: AtomicI64::new(0),
            next_async_completion: AtomicI64::new(0),
            async_pending: RefCell::new(0),
            async_completions: AsyncCompletionQueue::new(),
            async_promises: RefCell::new(HashMap::new()),
            bootstrap_source: RefCell::new(String::new()),
            evaluator: RefCell::new(None),
            importer: RefCell::new(None),
            deadline: RefCell::new(None),
            instruction_limit: Cell::new(0),
        })
    }

    pub fn set_global(&self, name: impl Into<String>, value: Object) {
        self.globals.borrow_mut().insert(name.into(), value);
    }

    pub fn push_stdout(&self, text: impl Into<String>) {
        self.output.borrow_mut().stdout.push(text.into());
    }

    pub fn push_stderr(&self, text: impl Into<String>) {
        self.output.borrow_mut().stderr.push(text.into());
    }

    pub fn take_output(&self) -> VmOutput {
        std::mem::take(&mut *self.output.borrow_mut())
    }

    pub fn get_global(&self, name: &str) -> Option<Object> {
        self.globals.borrow().get(name).cloned()
    }

    pub fn has_global(&self, name: &str) -> bool {
        self.globals.borrow().contains_key(name)
    }

    pub fn set_argv(&self, argv: Vec<String>) {
        *self.argv.borrow_mut() = argv;
    }

    pub fn set_evaluator(&self, f: Rc<EvaluatorFn>) {
        *self.evaluator.borrow_mut() = Some(f);
    }

    pub fn evaluator(&self) -> Option<Rc<EvaluatorFn>> {
        self.evaluator.borrow().clone()
    }

    pub fn set_importer(&self, f: Rc<ImporterFn>) {
        *self.importer.borrow_mut() = Some(f);
    }

    pub fn importer(&self) -> Option<Rc<ImporterFn>> {
        self.importer.borrow().clone()
    }

    pub fn next_timer_id(&self) -> i64 {
        self.next_timer.fetch_add(1, Ordering::Relaxed) + 1
    }

    pub fn next_async_completion_id(&self) -> AsyncCompletionId {
        (self.next_async_completion.fetch_add(1, Ordering::Relaxed) + 1) as AsyncCompletionId
    }

    pub fn set_timeout(&self, timeout: Option<Duration>) {
        *self.deadline.borrow_mut() = timeout.map(|duration| Instant::now() + duration);
    }

    pub fn clear_timeout(&self) {
        *self.deadline.borrow_mut() = None;
    }

    pub fn check_timeout(&self, pos: Position) -> Option<Object> {
        let deadline = *self.deadline.borrow();
        if let Some(deadline) = deadline {
            if Instant::now() >= deadline {
                return Some(new_error(pos, "TimeoutError: script execution timed out"));
            }
        }
        None
    }

    /// Cheap deadline probe that returns only whether the timeout has elapsed,
    /// without building an error value or needing a source `Position`.
    ///
    /// The bytecode dispatch loop samples this every `TIMEOUT_CHECK_INTERVAL`
    /// instructions; in the common (not-yet-expired) case it must avoid the
    /// `Rc<str>` clone that `position_at(ip)` + the error path perform. Only
    /// when this returns `true` does the caller pay for the `Position` lookup
    /// and the full `check_timeout(pos)` to construct the error.
    pub fn is_deadline_exceeded(&self) -> bool {
        match *self.deadline.borrow() {
            Some(deadline) => Instant::now() >= deadline,
            None => false,
        }
    }

    // —— D4.1: instruction limit (resource guard) ——

    /// Set the module allowlist for sandbox mode (D4.2). Pass `Some(list)` to
    /// enable; `None` to disable (allow all). When enabled, only modules in the
    /// list (plus always-safe modules) can be loaded.
    pub fn set_allowed_modules(&self, list: Option<Vec<String>>) {
        *self.allowed_modules.borrow_mut() = list;
    }

    /// Check whether a `@std/*` module specifier is allowed under the current
    /// sandbox allowlist. Returns `true` if allowed (or sandbox disabled).
    pub fn is_module_allowed(&self, spec: &str) -> bool {
        let guard = self.allowed_modules.borrow();
        let Some(allowed) = guard.as_ref() else {
            return true; // No sandbox → allow all.
        };
        // Always-safe modules (no I/O, no process control).
        const SAFE: &[&str] = &[
            "@std/path",
            "@std/json",
            "@std/text",
            "@std/semver",
            "@std/collections",
            "@std/regexp",
            "@std/validation",
            "@std/template",
            "@std/schema",
            "@std/diff",
            "@std/encoding/base64",
            "@std/encoding/hex",
            "@std/encoding/csv",
            "@std/hash",
            "@std/color",
            "@std/table",
            "@std/url",
            "@std/mime",
            "@std/highlight",
            "@std/markdown",
            "@std/cli",
            "@std/cache",
            "@std/toml",
            "@std/yaml",
            "@std/xml",
            "@std/async",
        ];
        if SAFE.contains(&spec) {
            return true;
        }
        allowed.iter().any(|m| {
            // Match by short name or full spec: "fs" matches "@std/fs".
            m == spec || format!("@std/{}", m) == spec
        })
    }

    /// Set the instruction limit. `0` disables the limit.
    pub fn set_instruction_limit(&self, limit: u64) {
        self.instruction_limit.set(limit);
    }

    /// Check whether the instruction count has exceeded the limit. Returns
    /// `Some(error)` if exceeded, `None` if within bounds or limit disabled.
    pub fn check_instruction_limit(&self, count: u64, pos: Position) -> Option<Object> {
        let limit = self.instruction_limit.get();
        if limit > 0 && count > limit {
            return Some(new_error(
                pos,
                format!(
                    "MemoryLimitError: instruction count {} exceeded limit {}",
                    count, limit
                ),
            ));
        }
        None
    }

    /// Cheap probe: is the instruction limit set and exceeded?
    pub fn is_instruction_limit_exceeded(&self, count: u64) -> bool {
        let limit = self.instruction_limit.get();
        limit > 0 && count > limit
    }

    /// Register outstanding async work.
    pub fn async_add(&self, n: usize) {
        *self.async_pending.borrow_mut() += n;
    }

    /// Clone a thread-safe sender for Tokio/background workers.
    pub fn async_completion_sender(&self) -> AsyncCompletionSender {
        self.async_completions.sender()
    }

    /// Allocate an async completion id, register a Promise on the VM thread,
    /// and count it as pending async work.
    pub fn create_async_completion_promise(&self) -> (AsyncCompletionId, Rc<Promise>) {
        let id = self.next_async_completion_id();
        let promise = Promise::new();
        self.register_async_completion_promise(id, promise.clone());
        (id, promise)
    }

    /// Register a Promise that will be settled when the matching completion is
    /// drained on the VM thread.
    pub fn register_async_completion_promise(&self, id: AsyncCompletionId, promise: Rc<Promise>) {
        self.async_promises.borrow_mut().insert(id, promise);
        self.async_add(1);
    }

    /// Queue a completion from the VM thread or tests.
    pub fn enqueue_async_completion(&self, completion: AsyncCompletion) {
        self.async_completions.enqueue(completion);
    }

    /// Convenience helper to resolve an async operation with owned data.
    pub fn enqueue_async_resolve(&self, id: AsyncCompletionId, data: AsyncCompletionData) {
        self.enqueue_async_completion(AsyncCompletion::resolve(id, data));
    }

    /// Convenience helper to reject an async operation with an owned error.
    pub fn enqueue_async_reject(&self, id: AsyncCompletionId, error: impl Into<String>) {
        self.enqueue_async_completion(AsyncCompletion::reject(id, error));
    }

    /// Drain queued completions on the VM thread.
    ///
    /// Matching registered Promises are resolved/rejected here so Object work
    /// remains on the VM thread. The returned completions are pure data for
    /// diagnostics and low-level tests.
    pub fn drain_async_completions(&self) -> Vec<AsyncCompletion> {
        let completions = self.async_completions.drain();
        for completion in &completions {
            if let Some(promise) = self.async_promises.borrow_mut().remove(&completion.id) {
                match &completion.result {
                    AsyncCompletionResult::Resolve(data) => {
                        promise.resolve(
                            crate::object::http_stream::async_completion_data_to_object(
                                data.clone(),
                            ),
                        );
                    }
                    AsyncCompletionResult::Reject(error) => {
                        promise.reject(new_error(Position::default(), error.clone()));
                    }
                }
            }
            self.async_done();
        }
        completions
    }

    pub fn async_completion_len(&self) -> usize {
        self.async_completions.len()
    }

    pub fn async_registered_promise_len(&self) -> usize {
        self.async_promises.borrow().len()
    }

    pub fn has_async_pending(&self) -> bool {
        *self.async_pending.borrow() > 0
    }

    /// Mark async work complete.
    pub fn async_done(&self) {
        let mut g = self.async_pending.borrow_mut();
        if *g > 0 {
            *g -= 1;
        }
    }

    /// Block until all outstanding async tasks complete. In the single-threaded
    /// model this simply polls; the event loop on the host thread drives the
    /// resolution.
    pub fn wait_async(&self) {
        while *self.async_pending.borrow() > 0 {
            let drained = self.drain_async_completions();
            if !drained.is_empty() {
                continue;
            }
            if self.check_timeout(Position::default()).is_some() {
                break;
            }
            if !self.async_promises.borrow().is_empty() {
                self.async_completions
                    .wait_for_completion(Duration::from_millis(100));
            } else {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct VmOutput {
    pub stdout: Vec<String>,
    pub stderr: Vec<String>,
}

/// Helper to create an error positioned at the given call site.
pub fn vm_error(pos: Position, msg: impl Into<String>) -> Object {
    new_error(pos, msg)
}
