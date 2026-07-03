//! Runtime session wiring the VM, module cache, and source loader.

use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use crate::ast::{Position, Program};
use crate::evaluator::builtins::register_globals;
use crate::evaluator::expressions::apply_function;
use crate::lexer::Lexer;
use crate::module::{
    cache_get, cache_insert, new_module_cache, ModuleCache, ModuleKind, ResolveOptions, Resolver,
};
use crate::object::{
    new_error, str_obj, ArrayData, Builtin, BuiltinFn, Environment, HashData, Object,
    VirtualMachine, EXEC_MODE_BYTECODE,
};
use crate::parser::Parser;
use crate::stdlib::load_native_module;

/// Result of running a script.
pub type RuntimeResult<T> = Result<T, Object>;

/// Human-readable runtime mode exposed by the CLI and `@std/runtime`.
#[cfg(feature = "tokio")]
pub fn runtime_mode() -> &'static str {
    "bytecode + tokio-io"
}

/// Human-readable runtime mode exposed by the CLI and `@std/runtime`.
#[cfg(not(feature = "tokio"))]
pub fn runtime_mode() -> &'static str {
    "bytecode + native-io"
}

/// Options for running a script file.
#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    pub argv: Vec<String>,
    pub call_main: bool,
    pub timeout: Option<Duration>,
}

/// One isolated GoScript execution session.
pub struct Session {
    vm: Rc<VirtualMachine>,
    root: crate::object::EnvRef,
    module_cache: Rc<ModuleCache>,
    resolver: Rc<Resolver>,
}

impl Session {
    /// Create a fresh session with standard globals installed.
    pub fn new() -> Session {
        let vm = VirtualMachine::new();
        vm.exec_mode.store(EXEC_MODE_BYTECODE, Ordering::Relaxed);
        register_globals(&vm);

        let root = Environment::new_root(vm.clone());
        let module_cache = Rc::new(new_module_cache());
        let resolver = Rc::new(Resolver::new(None));

        let session = Session {
            vm,
            root,
            module_cache,
            resolver,
        };
        session.install_host_globals();
        session.install_importer();
        session
    }

    /// Access the underlying VM.
    pub fn vm(&self) -> Rc<VirtualMachine> {
        self.vm.clone()
    }

    /// Install a JSON-serializable value as a global script object.
    pub fn set_global_json(&self, name: impl Into<String>, value: &serde_json::Value) {
        self.vm
            .set_global(name, crate::stdlib::helpers::value_to_object(value));
    }

    /// Run a source string as the top-level script.
    pub fn run_source(&self, source: &str, file: impl AsRef<Path>) -> RuntimeResult<Object> {
        self.run_source_with_options(source, file, false)
    }

    /// Run a source string, optionally invoking a top-level `main()` after load.
    pub fn run_source_with_options(
        &self,
        source: &str,
        file: impl AsRef<Path>,
        call_main: bool,
    ) -> RuntimeResult<Object> {
        let file = file.as_ref();
        // Record the entry script path so native modules (e.g. @std/web's
        // concurrent workers) can locate and re-run the script.
        *self.vm.bootstrap_source.borrow_mut() = file.to_string_lossy().into_owned();
        let program = parse_source(source, file)?;
        let module_dir = file.parent().unwrap_or_else(|| Path::new("."));
        self.root.borrow_mut().module_dir = module_dir.to_string_lossy().into_owned();
        let exports = Object::Hash(Rc::new(RefCell::new(HashData::default())));
        install_module_bindings(&self.root, exports);
        let mut result = eval_program_for_session(&program, &self.root);
        if !result.is_runtime_error() && call_main {
            result = self.call_main_if_present();
        }
        self.vm.wait_async();
        if result.is_runtime_error() {
            Err(result)
        } else {
            Ok(result)
        }
    }

    /// Read and run a `.gs` file.
    pub fn run_file(&self, file: impl AsRef<Path>, argv: Vec<String>) -> RuntimeResult<Object> {
        self.run_file_with_options(
            file,
            RunOptions {
                argv,
                call_main: false,
                timeout: None,
            },
        )
    }

    /// Read and run a `.gs` file with explicit runtime options.
    pub fn run_file_with_options(
        &self,
        file: impl AsRef<Path>,
        options: RunOptions,
    ) -> RuntimeResult<Object> {
        let file = normalize_path(file.as_ref());
        self.vm.set_argv(options.argv);
        self.vm.set_timeout(options.timeout);
        self.refresh_process_argv();
        let source = fs::read_to_string(&file).map_err(|e| {
            new_error(
                Default::default(),
                format!("IOError: cannot read {}: {}", file.display(), e),
            )
        })?;
        let result = self.run_source_with_options(&source, &file, options.call_main);
        self.vm.clear_timeout();
        result
    }

    fn install_host_globals(&self) {
        let require_fn: BuiltinFn = Rc::new(|ctx, args| {
            let spec = match args.first() {
                Some(Object::String(s)) => s.to_string(),
                Some(other) => other.inspect(),
                None => {
                    return new_error(ctx.pos.clone(), "TypeError: require expects a module path")
                }
            };
            let importer = ctx.env.borrow().vm.importer();
            match importer {
                Some(importer) => match importer(ctx.env, &spec) {
                    Ok(module) => module,
                    Err(err) => err,
                },
                None => new_error(
                    ctx.pos.clone(),
                    "ImportError: module loading is not configured",
                ),
            }
        });
        self.vm.set_global(
            "require",
            Object::Builtin(Rc::new(Builtin {
                name: "require".into(),
                func: require_fn,
                extra: None,
            })),
        );
        self.refresh_process_argv();
    }

    fn refresh_process_argv(&self) {
        let argv_snapshot: Vec<String> = self.vm.argv.borrow().clone();
        // Publish the same normalized argv to the stdlib thread-local so that
        // `require("@std/process")` agrees with the global `process` object.
        crate::stdlib::set_runtime_argv(argv_snapshot.clone());

        let elements: Vec<Object> = argv_snapshot.iter().map(|s| str_obj(s.clone())).collect();
        let args = Object::Array(Rc::new(RefCell::new(ArrayData { elements })));
        let argv0 = argv_snapshot
            .first()
            .map(|s| str_obj(s.clone()))
            .unwrap_or(Object::Undefined);

        let env_hash = Rc::new(RefCell::new(HashData::default()));
        for (k, v) in std::env::vars() {
            env_hash.borrow_mut().set(k, str_obj(v));
        }

        let process = Object::Hash(Rc::new(RefCell::new(HashData::default())));
        if let Object::Hash(h) = &process {
            h.borrow_mut().set("argv", args);
            h.borrow_mut().set("argv0", argv0);
            h.borrow_mut().set("env", Object::Hash(env_hash));
            h.borrow_mut()
                .set("pid", Object::Number(std::process::id() as f64));
        }
        self.vm.set_global("process", process);
    }

    fn call_main_if_present(&self) -> Object {
        let main = self.root.borrow().get("main");
        match main {
            Some(Object::Undefined) | None => Object::Undefined,
            Some(value) => apply_function(&value, &self.root, &[], None, Position::default()),
        }
    }

    fn install_importer(&self) {
        let cache = self.module_cache.clone();
        let resolver = self.resolver.clone();
        self.vm.set_importer(Rc::new(move |env, spec| {
            let base_dir = PathBuf::from(env.borrow().module_dir.clone());
            let resolved = resolver
                .resolve(
                    spec,
                    ResolveOptions {
                        base_dir: Some(base_dir.clone()),
                        ..ResolveOptions::default()
                    },
                )
                .map_err(|e| new_error(Default::default(), format!("ImportError: {e}")))?;

            if resolved.kind == ModuleKind::Native {
                // D4.2: sandbox module allowlist check.
                if !env.borrow().vm.is_module_allowed(spec) {
                    return Err(new_error(
                        Default::default(),
                        format!(
                            "PermissionError: module '{}' is not allowed (use --allow to grant access)",
                            spec
                        ),
                    ));
                }
                return load_native_module(spec).ok_or_else(|| {
                    new_error(
                        Default::default(),
                        format!("ImportError: unknown native module '{}'", spec),
                    )
                });
            }

            if let Some(module) = cache_get(&cache, &resolved.id) {
                return Ok(module);
            }

            let path = resolved.path.clone().ok_or_else(|| {
                new_error(
                    Default::default(),
                    format!("ImportError: module '{}' has no source path", spec),
                )
            })?;
            if resolved.kind == ModuleKind::Json {
                let module = load_json_module(&path)?;
                cache_insert(&cache, resolved.id, module.clone());
                return Ok(module);
            }
            // `.gspkg` archive module (E1.1): read the source from the zip
            // entry instead of the filesystem.
            let (source, module_dir) = if let Some(archive) = &resolved.archive {
                let bytes = crate::packagefile::read_archive_entry(
                    &archive.archive_path,
                    &archive.entry_name,
                )
                .map_err(|e| {
                    new_error(
                        Default::default(),
                        format!(
                            "ImportError: cannot read archive entry '{}' in {}: {}",
                            archive.entry_name,
                            archive.archive_path.display(),
                            e
                        ),
                    )
                })?;
                let source = String::from_utf8_lossy(&bytes).into_owned();
                let module_dir = PathBuf::from(format!(
                    "{}!{}",
                    archive.archive_path.display(),
                    parent_dir_of(&archive.entry_name)
                ));
                (source, module_dir)
            } else {
                let source = fs::read_to_string(&path).map_err(|e| {
                    new_error(
                        Default::default(),
                        format!("ImportError: cannot read {}: {}", path.display(), e),
                    )
                })?;
                let module_dir = path
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .to_path_buf();
                (source, module_dir)
            };
            let program = parse_source(&source, &path)?;
            let module = Object::Hash(Rc::new(RefCell::new(HashData::default())));
            cache_insert(&cache, resolved.id.clone(), module.clone());

            let scope = Environment::child(env);
            scope.borrow_mut().module_dir = module_dir.to_string_lossy().into_owned();
            install_module_bindings(&scope, module.clone());

            let result = eval_program_for_session(&program, &scope);
            if result.is_runtime_error() {
                Err(result)
            } else {
                let final_exports = module_exports(&scope).unwrap_or(module);
                cache_insert(&cache, resolved.id, final_exports.clone());
                Ok(final_exports)
            }
        }));
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

impl Session {
    /// Read and run a `.gs` file, then return its final `module.exports`.
    ///
    /// Used by `@std/runtime` to support `runScript`/`callScript`/`runTool`
    /// style helpers that spawn an isolated sub-script and inspect what it
    /// exported. The sub-script runs in a fresh VM with its own argv.
    pub fn run_file_for_exports(
        &self,
        file: impl AsRef<Path>,
        argv: Vec<String>,
        call_main: bool,
    ) -> RuntimeResult<Object> {
        let file = normalize_path(file.as_ref());
        self.vm.set_argv(argv);
        self.refresh_process_argv();
        let source = fs::read_to_string(&file).map_err(|e| {
            new_error(
                Default::default(),
                format!("IOError: cannot read {}: {}", file.display(), e),
            )
        })?;
        let program = parse_source(&source, &file)?;
        let module_dir = file
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_string_lossy()
            .into_owned();
        self.root.borrow_mut().module_dir = module_dir;
        let exports = Object::Hash(Rc::new(RefCell::new(HashData::default())));
        install_module_bindings(&self.root, exports);
        let mut result = eval_program_for_session(&program, &self.root);
        if !result.is_runtime_error() && call_main {
            result = self.call_main_if_present();
        }
        self.vm.wait_async();
        if result.is_runtime_error() {
            return Err(result);
        }
        Ok(module_exports(&self.root).unwrap_or(Object::Undefined))
    }

    /// Look up a named export on the root environment of the last-run script.
    pub fn root_export(&self, name: &str) -> Option<Object> {
        module_exports(&self.root).and_then(|exports| match exports {
            Object::Hash(h) => h.borrow().get(name).cloned(),
            _ => None,
        })
    }

    /// Invoke an exported or top-level `execute(args)` function from the last-run script.
    pub fn call_execute_json(
        &self,
        args: &serde_json::Value,
    ) -> RuntimeResult<Option<serde_json::Value>> {
        let execute = self.root_export("execute").or_else(|| {
            let root = self.root.borrow();
            root.get("execute")
        });
        let Some(execute) =
            execute.filter(|value| !matches!(value, Object::Undefined | Object::Null))
        else {
            return Ok(None);
        };

        let arg = crate::stdlib::helpers::value_to_object(args);
        let result = apply_function(&execute, &self.root, &[arg], None, Position::default());
        self.vm.wait_async();
        if result.is_runtime_error() {
            Err(result)
        } else {
            Ok(Some(crate::stdlib::helpers::object_to_value(&result)))
        }
    }
}

/// Return the parent directory of a forward-slash archive entry path
/// (e.g. `"lib/mod.gs"` → `"lib"`; `"mod.gs"` → `""`).
fn parent_dir_of(entry: &str) -> String {
    match entry.rfind('/') {
        Some(idx) => entry[..idx].to_string(),
        None => String::new(),
    }
}

fn parse_source(source: &str, file: &Path) -> RuntimeResult<Program> {
    let lex = Lexer::new(source);
    let mut parser = Parser::new(lex, file.to_string_lossy());
    let program = parser.parse_program();
    if !program.errors.is_empty() {
        let message = program
            .errors
            .iter()
            .take(8)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        Err(new_error(
            Default::default(),
            format!("SyntaxError: {}", message),
        ))
    } else {
        Ok(program)
    }
}

fn eval_program_for_session(program: &Program, env: &crate::object::EnvRef) -> Object {
    // The bytecode VM is the sole execution backend as of Phase 3 (B3.4).
    // The tree-walker is retired; `eval_program` is no longer a dispatch arm.
    let vm = env.borrow().vm.clone();
    let _ = vm;
    match crate::bytecode::compile(program) {
        Ok(chunk) => crate::bytecode::interpret(&chunk, env),
        Err(error) => error,
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn install_module_bindings(env: &crate::object::EnvRef, exports: Object) {
    let module = Object::Hash(Rc::new(RefCell::new(HashData::default())));
    if let Object::Hash(h) = &module {
        h.borrow_mut().set("exports", exports.clone());
    }
    let mut env = env.borrow_mut();
    env.set_here("exports", exports);
    env.set_here("module", module);
}

fn module_exports(env: &crate::object::EnvRef) -> Option<Object> {
    let module = env.borrow().get("module")?;
    match module {
        Object::Hash(h) => h.borrow().get("exports").cloned(),
        _ => None,
    }
}

fn load_json_module(path: &Path) -> RuntimeResult<Object> {
    let source = fs::read_to_string(path).map_err(|e| {
        new_error(
            Default::default(),
            format!("ImportError: cannot read {}: {}", path.display(), e),
        )
    })?;
    json_to_object(
        serde_json::from_str::<serde_json::Value>(&source).map_err(|e| {
            new_error(
                Default::default(),
                format!(
                    "ImportError: cannot parse JSON module {}: {}",
                    path.display(),
                    e
                ),
            )
        })?,
    )
    .map_err(|e| new_error(Default::default(), format!("ImportError: {e}")))
}

fn json_to_object(value: serde_json::Value) -> Result<Object, String> {
    Ok(match value {
        serde_json::Value::Null => Object::Null,
        serde_json::Value::Bool(value) => Object::Boolean(value),
        serde_json::Value::Number(value) => Object::Number(
            value
                .as_f64()
                .ok_or_else(|| format!("JSON number {} is not representable", value))?,
        ),
        serde_json::Value::String(value) => str_obj(value),
        serde_json::Value::Array(values) => Object::Array(Rc::new(RefCell::new(ArrayData {
            elements: values
                .into_iter()
                .map(json_to_object)
                .collect::<Result<Vec<_>, _>>()?,
        }))),
        serde_json::Value::Object(values) => {
            let hash = Rc::new(RefCell::new(HashData::default()));
            for (key, value) in values {
                hash.borrow_mut().set(key, json_to_object(value)?);
            }
            Object::Hash(hash)
        }
    })
}
