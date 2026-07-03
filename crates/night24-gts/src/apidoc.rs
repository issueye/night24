// API Documentation generator
//
// 生成标准库模块的 API 文档

use std::collections::HashMap;

/// 标准库模块文档
#[derive(Debug, Clone)]
pub struct ModuleDoc {
    pub name: String,
    pub description: String,
    pub functions: Vec<FunctionDoc>,
    pub classes: Vec<ClassDoc>,
    pub constants: Vec<ConstantDoc>,
}

#[derive(Debug, Clone)]
pub struct FunctionDoc {
    pub name: String,
    pub signature: String,
    pub description: String,
    pub params: Vec<ParamDoc>,
    pub returns: String,
    pub example: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ClassDoc {
    pub name: String,
    pub description: String,
    pub methods: Vec<FunctionDoc>,
}

#[derive(Debug, Clone)]
pub struct ConstantDoc {
    pub name: String,
    pub value: String,
    pub description: String,
}

#[derive(Debug, Clone)]
pub struct ParamDoc {
    pub name: String,
    pub type_: String,
    pub description: String,
}

/// 获取所有标准库模块的文档
pub fn get_all_stdlib_docs() -> HashMap<String, ModuleDoc> {
    let mut docs = HashMap::new();

    // @std/fs
    docs.insert(
        "@std/fs".to_string(),
        ModuleDoc {
            name: "@std/fs".to_string(),
            description: "File system operations".to_string(),
            functions: vec![
                FunctionDoc {
                    name: "readFile".to_string(),
                    signature: "readFile(path: string): string".to_string(),
                    description: "Read entire file as string".to_string(),
                    params: vec![ParamDoc {
                        name: "path".to_string(),
                        type_: "string".to_string(),
                        description: "File path to read".to_string(),
                    }],
                    returns: "string - File contents".to_string(),
                    example: Some("const content = fs.readFile('data.txt');".to_string()),
                },
                FunctionDoc {
                    name: "writeFile".to_string(),
                    signature: "writeFile(path: string, content: string): void".to_string(),
                    description: "Write content to file".to_string(),
                    params: vec![
                        ParamDoc {
                            name: "path".to_string(),
                            type_: "string".to_string(),
                            description: "File path to write".to_string(),
                        },
                        ParamDoc {
                            name: "content".to_string(),
                            type_: "string".to_string(),
                            description: "Content to write".to_string(),
                        },
                    ],
                    returns: "void".to_string(),
                    example: Some("fs.writeFile('output.txt', 'Hello!');".to_string()),
                },
                FunctionDoc {
                    name: "exists".to_string(),
                    signature: "exists(path: string): boolean".to_string(),
                    description: "Check if file or directory exists".to_string(),
                    params: vec![ParamDoc {
                        name: "path".to_string(),
                        type_: "string".to_string(),
                        description: "Path to check".to_string(),
                    }],
                    returns: "boolean - true if exists".to_string(),
                    example: Some("if (fs.exists('data.txt')) { ... }".to_string()),
                },
            ],
            classes: vec![],
            constants: vec![],
        },
    );

    // @std/json
    docs.insert(
        "@std/json".to_string(),
        ModuleDoc {
            name: "@std/json".to_string(),
            description: "JSON parsing and serialization".to_string(),
            functions: vec![
                FunctionDoc {
                    name: "parse".to_string(),
                    signature: "parse(text: string): any".to_string(),
                    description: "Parse JSON string to object".to_string(),
                    params: vec![ParamDoc {
                        name: "text".to_string(),
                        type_: "string".to_string(),
                        description: "JSON string".to_string(),
                    }],
                    returns: "any - Parsed object".to_string(),
                    example: Some("const obj = json.parse('{\"x\": 10}');".to_string()),
                },
                FunctionDoc {
                    name: "stringify".to_string(),
                    signature: "stringify(value: any, indent?: number): string".to_string(),
                    description: "Convert object to JSON string".to_string(),
                    params: vec![
                        ParamDoc {
                            name: "value".to_string(),
                            type_: "any".to_string(),
                            description: "Value to serialize".to_string(),
                        },
                        ParamDoc {
                            name: "indent".to_string(),
                            type_: "number".to_string(),
                            description: "Indentation spaces (optional)".to_string(),
                        },
                    ],
                    returns: "string - JSON string".to_string(),
                    example: Some("const json = json.stringify({x: 10}, 2);".to_string()),
                },
            ],
            classes: vec![],
            constants: vec![],
        },
    );

    // @std/http
    docs.insert(
        "@std/http".to_string(),
        ModuleDoc {
            name: "@std/http".to_string(),
            description: "HTTP client for making requests".to_string(),
            functions: vec![
                FunctionDoc {
                    name: "get".to_string(),
                    signature: "get(url: string, options?: object): Response".to_string(),
                    description: "Make HTTP GET request".to_string(),
                    params: vec![
                        ParamDoc {
                            name: "url".to_string(),
                            type_: "string".to_string(),
                            description: "URL to request".to_string(),
                        },
                        ParamDoc {
                            name: "options".to_string(),
                            type_: "object".to_string(),
                            description: "Request options (optional)".to_string(),
                        },
                    ],
                    returns: "Response - HTTP response object".to_string(),
                    example: Some(
                        "const resp = http.get('https://api.example.com/data');".to_string(),
                    ),
                },
                FunctionDoc {
                    name: "post".to_string(),
                    signature: "post(url: string, body: any, options?: object): Response"
                        .to_string(),
                    description: "Make HTTP POST request".to_string(),
                    params: vec![
                        ParamDoc {
                            name: "url".to_string(),
                            type_: "string".to_string(),
                            description: "URL to request".to_string(),
                        },
                        ParamDoc {
                            name: "body".to_string(),
                            type_: "any".to_string(),
                            description: "Request body".to_string(),
                        },
                    ],
                    returns: "Response - HTTP response object".to_string(),
                    example: Some(
                        "const resp = http.post('https://api.example.com/data', {x: 1});"
                            .to_string(),
                    ),
                },
            ],
            classes: vec![],
            constants: vec![],
        },
    );

    // @std/signal
    docs.insert("@std/signal".to_string(), ModuleDoc {
        name: "@std/signal".to_string(),
        description: "OS signal handling (SIGINT, SIGTERM, etc.)".to_string(),
        functions: vec![
            FunctionDoc {
                name: "supported".to_string(),
                signature: "supported(): string[]".to_string(),
                description: "List signal names supported on this platform".to_string(),
                params: vec![],
                returns: "string[] - array of signal names (SIGINT, SIGTERM, ...)".to_string(),
                example: Some("const sigs = signal.supported();".to_string()),
            },
            FunctionDoc {
                name: "wait".to_string(),
                signature: "wait(signals?, timeoutMs?): string | null".to_string(),
                description: "Block until a signal arrives or timeout. Returns the signal name, or null on timeout".to_string(),
                params: vec![
                    ParamDoc { name: "signals".to_string(), type_: "string | string[]".to_string(), description: "Signal(s) to wait for (default SIGINT/SIGTERM)".to_string() },
                    ParamDoc { name: "timeoutMs".to_string(), type_: "number".to_string(), description: "Timeout in ms (required in this runtime)".to_string() },
                ],
                returns: "string | null - signal name or null on timeout".to_string(),
                example: Some("const s = signal.wait([\"SIGTERM\"], 1000);".to_string()),
            },
            FunctionDoc {
                name: "notify".to_string(),
                signature: "notify(signals?): watcher".to_string(),
                description: "Create a signal watcher with wait(timeoutMs)/stop() methods".to_string(),
                params: vec![ParamDoc { name: "signals".to_string(), type_: "string | string[]".to_string(), description: "Signals to watch".to_string() }],
                returns: "watcher - object with wait() and stop()".to_string(),
                example: Some("const w = signal.notify([\"SIGINT\"]); w.wait(5000);".to_string()),
            },
            FunctionDoc {
                name: "send".to_string(),
                signature: "send(pid, signal?): void".to_string(),
                description: "Send a signal to a process by PID".to_string(),
                params: vec![
                    ParamDoc { name: "pid".to_string(), type_: "number".to_string(), description: "Process ID".to_string() },
                    ParamDoc { name: "signal".to_string(), type_: "string | number".to_string(), description: "Signal name or number (default SIGINT)".to_string() },
                ],
                returns: "void".to_string(),
                example: Some("signal.send(1234, \"SIGTERM\");".to_string()),
            },
        ],
        classes: vec![],
        constants: vec![],
    });

    // @std/watch
    docs.insert("@std/watch".to_string(), ModuleDoc {
        name: "@std/watch".to_string(),
        description: "File change watcher (polling-based)".to_string(),
        functions: vec![
            FunctionDoc {
                name: "file".to_string(),
                signature: "file(path, callback, options?): boolean".to_string(),
                description: "Poll a file for modification; call callback synchronously when changed. Returns true if changed, false on timeout".to_string(),
                params: vec![
                    ParamDoc { name: "path".to_string(), type_: "string".to_string(), description: "File path to watch".to_string() },
                    ParamDoc { name: "callback".to_string(), type_: "function".to_string(), description: "Called (no args) when file changes".to_string() },
                    ParamDoc { name: "options".to_string(), type_: "object".to_string(), description: "{ interval?: number, timeout?: number }".to_string() },
                ],
                returns: "boolean - true if change detected, false on timeout".to_string(),
                example: Some("watch.file(\"log.txt\", () => println(\"changed\"), {interval: 1000, timeout: 60000});".to_string()),
            },
        ],
        classes: vec![],
        constants: vec![],
    });

    // @std/tui
    docs.insert("@std/tui".to_string(), ModuleDoc {
        name: "@std/tui".to_string(),
        description: "Terminal UI: declarative node tree + flexbox layout engine (Ink-inspired)".to_string(),
        functions: vec![
            FunctionDoc {
                name: "createApp".to_string(),
                signature: "createApp(spec): app".to_string(),
                description: "Create an app with the Elm architecture: spec.init(size)->state, spec.update(state,msg)->{state,quit}, spec.view(state,size)->nodeTree".to_string(),
                params: vec![
                    ParamDoc { name: "spec".to_string(), type_: "object".to_string(), description: "{ init?, update?, view?, state? }".to_string() },
                ],
                returns: "app - with dispatch/render/run/stop/state methods".to_string(),
                example: Some("let app = tui.createApp({ state: 0, view: (s) => tui.text(String(s)) });\napp.run({tickMs: 120});".to_string()),
            },
            FunctionDoc {
                name: "text".to_string(),
                signature: "text(value, opts?): node".to_string(),
                description: "Build a styled text node. opts: {color, bg, bold, dim, underline, inverse, wrap:'wrap'|'truncate'|'end'}".to_string(),
                params: vec![
                    ParamDoc { name: "value".to_string(), type_: "string".to_string(), description: "Text content".to_string() },
                    ParamDoc { name: "opts".to_string(), type_: "object".to_string(), description: "Style + wrap mode".to_string() },
                ],
                returns: "node - a tuiNode marker".to_string(),
                example: Some("tui.text(\"hi\", {color: \"green\", bold: true})".to_string()),
            },
            FunctionDoc {
                name: "box".to_string(),
                signature: "box(opts?): node".to_string(),
                description: "Flexbox container. opts: {children:[node], flexDirection:'row'|'column', width, height, grow, padding, margin, border, alignItems, justifyContent, title}".to_string(),
                params: vec![
                    ParamDoc { name: "opts".to_string(), type_: "object".to_string(), description: "Flexbox + children".to_string() },
                ],
                returns: "node".to_string(),
                example: Some("tui.box({flexDirection:\"row\", border:true, children:[tui.text(\"A\"), tui.text(\"B\")]})".to_string()),
            },
            FunctionDoc {
                name: "row".to_string(),
                signature: "row(opts?): node".to_string(),
                description: "Shorthand for box with flexDirection: 'row'".to_string(),
                params: vec![ ParamDoc { name: "opts".to_string(), type_: "object".to_string(), description: "{children:[node], ...flexProps}".to_string() } ],
                returns: "node".to_string(),
                example: Some("tui.row({children:[tui.text(\"A\"), tui.text(\"B\")]})".to_string()),
            },
            FunctionDoc {
                name: "column".to_string(),
                signature: "column(opts?): node".to_string(),
                description: "Shorthand for box with flexDirection: 'column'".to_string(),
                params: vec![ ParamDoc { name: "opts".to_string(), type_: "object".to_string(), description: "{children:[node], ...flexProps}".to_string() } ],
                returns: "node".to_string(),
                example: Some("tui.column({children:[tui.text(\"A\"), tui.text(\"B\")]})".to_string()),
            },
            FunctionDoc {
                name: "input".to_string(),
                signature: "input(opts): node".to_string(),
                description: "Text input node with cursor. opts: {value, cursor, placeholder, prompt, focused, width}".to_string(),
                params: vec![ ParamDoc { name: "opts".to_string(), type_: "object".to_string(), description: "Input options".to_string() } ],
                returns: "node".to_string(),
                example: Some("tui.input({value:\"hi\", cursor:2, focused:true})".to_string()),
            },
            FunctionDoc {
                name: "list".to_string(),
                signature: "list(opts): node".to_string(),
                description: "Selectable list. opts: {items:[string], selected:number, focused:bool}".to_string(),
                params: vec![ ParamDoc { name: "opts".to_string(), type_: "object".to_string(), description: "List options".to_string() } ],
                returns: "node".to_string(),
                example: Some("tui.list({items:[\"a\",\"b\"], selected:1})".to_string()),
            },
            FunctionDoc {
                name: "table".to_string(),
                signature: "table(opts): node".to_string(),
                description: "Tabular data. opts: {headers:[string], rows:[[string]], columnWidths:[number]}".to_string(),
                params: vec![ ParamDoc { name: "opts".to_string(), type_: "object".to_string(), description: "Table options".to_string() } ],
                returns: "node".to_string(),
                example: Some("tui.table({headers:[\"k\",\"v\"], rows:[[\"a\",\"1\"]]})".to_string()),
            },
            FunctionDoc {
                name: "progress".to_string(),
                signature: "progress(opts): node".to_string(),
                description: "Progress bar. opts: {value:number, total:number, label:string, width:number}".to_string(),
                params: vec![ ParamDoc { name: "opts".to_string(), type_: "object".to_string(), description: "Progress options".to_string() } ],
                returns: "node".to_string(),
                example: Some("tui.progress({value:50, total:100, width:20})".to_string()),
            },
            FunctionDoc {
                name: "checkbox".to_string(),
                signature: "checkbox(opts): node".to_string(),
                description: "Checkbox. opts: {checked:bool, label:string}".to_string(),
                params: vec![ ParamDoc { name: "opts".to_string(), type_: "object".to_string(), description: "Checkbox options".to_string() } ],
                returns: "node".to_string(),
                example: Some("tui.checkbox({checked:true, label:\"done\"})".to_string()),
            },
            FunctionDoc {
                name: "key".to_string(),
                signature: "key(name): msg".to_string(),
                description: "Build a key event message {type:'key', key:name} for manual dispatch/testing".to_string(),
                params: vec![ ParamDoc { name: "name".to_string(), type_: "string".to_string(), description: "Key name e.g. 'enter', 'ctrl+c'".to_string() } ],
                returns: "msg".to_string(),
                example: Some("app.dispatch(tui.key(\"enter\"))".to_string()),
            },
            FunctionDoc {
                name: "tick".to_string(),
                signature: "tick(): msg".to_string(),
                description: "Build a tick message {type:'tick', timeMs} for manual dispatch/testing".to_string(),
                params: vec![],
                returns: "msg".to_string(),
                example: Some("app.dispatch(tui.tick())".to_string()),
            },
            FunctionDoc {
                name: "style".to_string(),
                signature: "style(text, opts): string".to_string(),
                description: "Apply ANSI styling to a string. opts: {fg/color, bg, bold, dim, underline, inverse}".to_string(),
                params: vec![
                    ParamDoc { name: "text".to_string(), type_: "string".to_string(), description: "Text".to_string() },
                    ParamDoc { name: "opts".to_string(), type_: "object".to_string(), description: "Style options".to_string() },
                ],
                returns: "string - styled text".to_string(),
                example: Some("tui.style(\"ok\", {color:\"green\", bold:true})".to_string()),
            },
        ],
        classes: vec![],
        constants: vec![],
    });

    // @std/async
    docs.insert(
        "@std/async".to_string(),
        ModuleDoc {
            name: "@std/async".to_string(),
            description: "Async concurrency primitives (HTTP-as-Promise, worker)".to_string(),
            functions: vec![
                FunctionDoc {
                    name: "fetchAsync".to_string(),
                    signature: "fetchAsync(url, opts?): Promise<Response>".to_string(),
                    description: "Async HTTP request, returns a Promise".to_string(),
                    params: vec![
                        ParamDoc {
                            name: "url".to_string(),
                            type_: "string | object".to_string(),
                            description: "URL or options".to_string(),
                        },
                        ParamDoc {
                            name: "opts".to_string(),
                            type_: "object".to_string(),
                            description: "Request options".to_string(),
                        },
                    ],
                    returns: "Promise<Response>".to_string(),
                    example: Some(
                        "const r = await async.fetchAsync(\"https://api.example.com\");"
                            .to_string(),
                    ),
                },
                FunctionDoc {
                    name: "getAsync".to_string(),
                    signature: "getAsync(url, opts?): Promise<Response>".to_string(),
                    description: "Async HTTP GET".to_string(),
                    params: vec![ParamDoc {
                        name: "url".to_string(),
                        type_: "string".to_string(),
                        description: "URL".to_string(),
                    }],
                    returns: "Promise<Response>".to_string(),
                    example: Some("const r = await async.getAsync(url);".to_string()),
                },
                FunctionDoc {
                    name: "postAsync".to_string(),
                    signature: "postAsync(url, body, opts?): Promise<Response>".to_string(),
                    description: "Async HTTP POST".to_string(),
                    params: vec![
                        ParamDoc {
                            name: "url".to_string(),
                            type_: "string".to_string(),
                            description: "URL".to_string(),
                        },
                        ParamDoc {
                            name: "body".to_string(),
                            type_: "any".to_string(),
                            description: "Request body".to_string(),
                        },
                    ],
                    returns: "Promise<Response>".to_string(),
                    example: Some("const r = await async.postAsync(url, {x:1});".to_string()),
                },
                FunctionDoc {
                    name: "runWorker".to_string(),
                    signature: "runWorker(fn, ...args): Promise<result>".to_string(),
                    description: "Run fn(args) in isolated scope, return Promise of result"
                        .to_string(),
                    params: vec![
                        ParamDoc {
                            name: "fn".to_string(),
                            type_: "function".to_string(),
                            description: "Function to run".to_string(),
                        },
                        ParamDoc {
                            name: "args".to_string(),
                            type_: "...any".to_string(),
                            description: "Arguments to pass".to_string(),
                        },
                    ],
                    returns: "Promise<result>".to_string(),
                    example: Some(
                        "const r = await async.runWorker((a,b) => a+b, 3, 4);".to_string(),
                    ),
                },
            ],
            classes: vec![],
            constants: vec![],
        },
    );

    // @std/rate-limit
    docs.insert("@std/rate-limit".to_string(), ModuleDoc {
        name: "@std/rate-limit".to_string(),
        description: "Token-bucket rate limiter".to_string(),
        functions: vec![
            FunctionDoc {
                name: "create".to_string(),
                signature: "create(opts?): limiter".to_string(),
                description: "Create a rate limiter with {rate, capacity}. Returns object with tryAcquire()/acquire()/remaining()".to_string(),
                params: vec![ParamDoc { name: "opts".to_string(), type_: "object".to_string(), description: "{ rate?: number (default 10), capacity?: number (default 10) }".to_string() }],
                returns: "limiter - { tryAcquire: ()=>bool, acquire: ()=>void, remaining: ()=>number }".to_string(),
                example: Some("const rl = rateLimit.create({rate: 5, capacity: 5}); if (rl.tryAcquire()) { ... }".to_string()),
            },
        ],
        classes: vec![],
        constants: vec![],
    });

    // @std/pty
    docs.insert("@std/pty".to_string(), ModuleDoc {
        name: "@std/pty".to_string(),
        description: "Pseudo-terminal / subprocess management".to_string(),
        functions: vec![
            FunctionDoc {
                name: "spawn".to_string(),
                signature: "spawn(cmd, args?, opts?): pty".to_string(),
                description: "Start a subprocess. Returns object with read/readLine/readText/readTextTimeout/write/writeln/kill/wait/resize/close".to_string(),
                params: vec![
                    ParamDoc { name: "cmd".to_string(), type_: "string".to_string(), description: "Command to run".to_string() },
                    ParamDoc { name: "args".to_string(), type_: "string | string[]".to_string(), description: "Arguments (varargs or array)".to_string() },
                    ParamDoc { name: "opts".to_string(), type_: "object".to_string(), description: "{ cols?: number, rows?: number, args?: string[] }".to_string() },
                ],
                returns: "pty - subprocess handle".to_string(),
                example: Some("const p = pty.spawn(\"git\", [\"status\"]); println(p.readText()); p.wait();".to_string()),
            },
        ],
        classes: vec![],
        constants: vec![],
    });

    docs
}

/// 格式化模块文档为文本
pub fn format_module_doc(doc: &ModuleDoc) -> String {
    let mut output = String::new();

    // 标题
    output.push_str(&format!("# {}\n\n", doc.name));
    output.push_str(&format!("{}\n\n", doc.description));

    // 函数
    if !doc.functions.is_empty() {
        output.push_str("## Functions\n\n");
        for func in &doc.functions {
            output.push_str(&format!("### {}\n\n", func.name));
            output.push_str(&format!("**Signature**: `{}`\n\n", func.signature));
            output.push_str(&format!("{}\n\n", func.description));

            if !func.params.is_empty() {
                output.push_str("**Parameters**:\n");
                for param in &func.params {
                    output.push_str(&format!(
                        "- `{}` ({}): {}\n",
                        param.name, param.type_, param.description
                    ));
                }
                output.push('\n');
            }

            output.push_str(&format!("**Returns**: {}\n\n", func.returns));

            if let Some(example) = &func.example {
                output.push_str("**Example**:\n```javascript\n");
                output.push_str(example);
                output.push_str("\n```\n\n");
            }
        }
    }

    // 常量
    if !doc.constants.is_empty() {
        output.push_str("## Constants\n\n");
        for constant in &doc.constants {
            output.push_str(&format!("### {}\n\n", constant.name));
            output.push_str(&format!("**Value**: `{}`\n\n", constant.value));
            output.push_str(&format!("{}\n\n", constant.description));
        }
    }

    output
}

/// Render every documented module as a single aggregated Markdown document
/// (W3.2). Intended for `gs --api_doc all --markdown` → standard-library
/// reference. Code blocks use the `goscript` language tag.
pub fn format_all_modules_markdown(docs: &HashMap<String, ModuleDoc>) -> String {
    let mut out = String::new();
    out.push_str("# GoScript 标准库参考\n\n");
    out.push_str("> 本文档由 `gs --api_doc all` 自动生成。请勿手动编辑。\n\n");
    // Stable order: sort by module name.
    let mut names: Vec<&String> = docs.keys().collect();
    names.sort();
    out.push_str("## 模块索引\n\n");
    for name in &names {
        // Anchor: strip non-alphanumeric for a stable link target.
        let anchor: String = name
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-')
            .collect();
        out.push_str(&format!("- [{}](#{})\n", name, anchor));
    }
    out.push_str("\n---\n\n");
    for name in names {
        if let Some(doc) = docs.get(name) {
            out.push_str(&format_module_markdown(doc));
            out.push_str("\n---\n\n");
        }
    }
    out
}

/// Format a single module as Markdown (goscript code blocks).
pub fn format_module_markdown(doc: &ModuleDoc) -> String {
    let mut output = String::new();
    output.push_str(&format!("## {}\n\n", doc.name));
    output.push_str(&format!("{}\n\n", doc.description));

    if !doc.functions.is_empty() {
        output.push_str("### Functions\n\n");
        for func in &doc.functions {
            output.push_str(&format!("#### `{}`\n\n", func.name));
            output.push_str(&format!("```goscript\n{}\n```\n\n", func.signature));
            output.push_str(&format!("{}\n\n", func.description));

            if !func.params.is_empty() {
                output.push_str("**Parameters**:\n\n");
                for param in &func.params {
                    output.push_str(&format!(
                        "- `{}` ({}): {}\n",
                        param.name, param.type_, param.description
                    ));
                }
                output.push('\n');
            }

            output.push_str(&format!("**Returns**: {}\n\n", func.returns));

            if let Some(example) = &func.example {
                output.push_str("**Example**:\n\n```goscript\n");
                output.push_str(example);
                output.push_str("\n```\n\n");
            }
        }
    }

    if !doc.constants.is_empty() {
        output.push_str("### Constants\n\n");
        for constant in &doc.constants {
            output.push_str(&format!(
                "- **`{}`** (`{}`): {}\n",
                constant.name, constant.value, constant.description
            ));
        }
        output.push('\n');
    }

    output
}

/// 列出所有可用的标准库模块
pub fn list_all_modules() -> Vec<String> {
    vec![
        "@std/fs".to_string(),
        "@std/path".to_string(),
        "@std/os".to_string(),
        "@std/json".to_string(),
        "@std/yaml".to_string(),
        "@std/toml".to_string(),
        "@std/xml".to_string(),
        "@std/http".to_string(),
        "@std/socket".to_string(),
        "@std/ws".to_string(),
        "@std/web".to_string(),
        "@std/db".to_string(),
        "@std/crypto".to_string(),
        "@std/hash".to_string(),
        "@std/buffer".to_string(),
        "@std/zip".to_string(),
        "@std/time".to_string(),
        "@std/test".to_string(),
        "@std/runtime".to_string(),
        "@std/gtp".to_string(),
        "@std/signal".to_string(),
        "@std/watch".to_string(),
        "@std/async".to_string(),
        "@std/rate-limit".to_string(),
        "@std/pty".to_string(),
        "@std/events".to_string(),
        "@std/cache".to_string(),
        "@std/regexp".to_string(),
        "@std/markdown".to_string(),
        "@std/terminal".to_string(),
        "@std/tui".to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_all_docs() {
        let docs = get_all_stdlib_docs();
        assert!(!docs.is_empty());
        assert!(docs.contains_key("@std/fs"));
        assert!(docs.contains_key("@std/json"));
    }

    #[test]
    fn test_format_doc() {
        let docs = get_all_stdlib_docs();
        let fs_doc = docs.get("@std/fs").unwrap();
        let formatted = format_module_doc(fs_doc);
        assert!(formatted.contains("@std/fs"));
        assert!(formatted.contains("readFile"));
    }

    #[test]
    fn test_format_all_modules_markdown() {
        let docs = get_all_stdlib_docs();
        let md = format_all_modules_markdown(&docs);
        // Has the title, an index, and at least one module section.
        assert!(md.contains("# GoScript 标准库参考"));
        assert!(md.contains("## 模块索引"));
        assert!(md.contains("## @std/fs"));
        // Code blocks use the goscript language tag.
        assert!(md.contains("```goscript"));
        // Index links are present for each module.
        assert!(md.contains("- [@std/fs](#stdfs)"));
    }

    #[test]
    fn test_format_module_markdown_uses_goscript_blocks() {
        let docs = get_all_stdlib_docs();
        let fs_doc = docs.get("@std/fs").unwrap();
        let md = format_module_markdown(fs_doc);
        assert!(md.starts_with("## @std/fs"));
        assert!(md.contains("```goscript"));
        assert!(md.contains("readFile"));
    }
}
