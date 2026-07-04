use crate::model::Tool;

pub fn builtin_tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "developer__shell".to_string(),
            description: "Run a shell command in the working directory.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The shell command to execute."
                    }
                },
                "required": ["command"]
            }),
        },
        Tool {
            name: "developer__read_file".to_string(),
            description: "Read the content of a file within the working directory.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Relative path to the file to read."
                    }
                },
                "required": ["path"]
            }),
        },
        Tool {
            name: "developer__write_file".to_string(),
            description: "Write content to a file within the working directory.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Relative path to the file to write."
                    },
                    "content": {
                        "type": "string",
                        "description": "Content to write into the file."
                    }
                },
                "required": ["path", "content"]
            }),
        },
        Tool {
            name: "developer__echo".to_string(),
            description: "Echo back the provided text.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "text": {
                        "type": "string",
                        "description": "Text to echo back."
                    }
                },
                "required": ["text"]
            }),
        },
        Tool {
            name: "developer__list_files".to_string(),
            description: "List files and directories under the given relative path.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Relative directory path to list. Defaults to working directory."
                    },
                    "recursive": {
                        "type": "boolean",
                        "description": "Whether to list recursively.",
                        "default": false
                    }
                },
                "required": []
            }),
        },
        Tool {
            name: "developer__datetime".to_string(),
            description: "Get the current UTC and local datetime.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "format": {
                        "type": "string",
                        "description": "Optional output format. Defaults to rfc3339.",
                        "default": "rfc3339"
                    }
                },
                "required": []
            }),
        },
        Tool {
            name: "developer__calculator".to_string(),
            description: "Evaluate a basic math expression safely.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "expression": {
                        "type": "string",
                        "description": "Math expression to evaluate, e.g. 2 + 3 * 4."
                    }
                },
                "required": ["expression"]
            }),
        },
        Tool {
            name: "developer__http_request".to_string(),
            description: "Make an HTTP request and return the response body.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "Target URL."
                    },
                    "method": {
                        "type": "string",
                        "description": "HTTP method.",
                        "default": "GET"
                    },
                    "headers": {
                        "type": "object",
                        "description": "Optional JSON object of headers."
                    },
                    "body": {
                        "type": "string",
                        "description": "Optional request body."
                    },
                    "proxy": {
                        "type": "string",
                        "description": "Optional HTTP/HTTPS proxy URL for this request. Use \"direct\" to ignore environment proxy settings."
                    }
                },
                "required": ["url"]
            }),
        },
        Tool {
            name: "developer__network_request".to_string(),
            description: "Make an HTTP or HTTPS network request and return status and body."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "Target HTTP or HTTPS URL."
                    },
                    "method": {
                        "type": "string",
                        "description": "HTTP method.",
                        "default": "GET"
                    },
                    "headers": {
                        "type": "object",
                        "description": "Optional JSON object of headers."
                    },
                    "body": {
                        "type": "string",
                        "description": "Optional request body."
                    },
                    "proxy": {
                        "type": "string",
                        "description": "Optional HTTP/HTTPS proxy URL for this request. Use \"direct\" to ignore environment proxy settings."
                    }
                },
                "required": ["url"]
            }),
        },
        Tool {
            name: "developer__web_search".to_string(),
            description: "Search the web and return a short result summary.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query."
                    },
                    "proxy": {
                        "type": "string",
                        "description": "Optional HTTP/HTTPS proxy URL for this search. Use \"direct\" to ignore environment proxy settings."
                    }
                },
                "required": ["query"]
            }),
        },
        Tool {
            name: "developer__network_search".to_string(),
            description: "Search the web and return a short result summary.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query."
                    },
                    "proxy": {
                        "type": "string",
                        "description": "Optional HTTP/HTTPS proxy URL for this search. Use \"direct\" to ignore environment proxy settings."
                    }
                },
                "required": ["query"]
            }),
        },
        Tool {
            name: "developer__jq".to_string(),
            description: "Query JSON data with a simple jq-like expression.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "data": {
                        "type": "string",
                        "description": "JSON string to query."
                    },
                    "query": {
                        "type": "string",
                        "description": "jq-like query expression."
                    }
                },
                "required": ["data", "query"]
            }),
        },
        Tool {
            name: "developer__file_search".to_string(),
            description: "Search files in the working directory for a text pattern.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Text pattern to search for."
                    },
                    "path": {
                        "type": "string",
                        "description": "Directory to search in. Defaults to working directory."
                    },
                    "file_pattern": {
                        "type": "string",
                        "description": "Optional glob pattern for filenames, e.g. *.rs."
                    }
                },
                "required": ["query"]
            }),
        },
        Tool {
            name: "developer__web_scraper".to_string(),
            description: "Fetch a web page and return extracted plain text.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "Target URL to scrape."
                    },
                    "proxy": {
                        "type": "string",
                        "description": "Optional HTTP/HTTPS proxy URL for this request. Use \"direct\" to ignore environment proxy settings."
                    }
                },
                "required": ["url"]
            }),
        },
        Tool {
            name: "developer__code_interpreter".to_string(),
            description: "Run a short Python or JavaScript snippet and return stdout.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "code": {
                        "type": "string",
                        "description": "Code snippet to execute."
                    },
                    "language": {
                        "type": "string",
                        "description": "Language: python or javascript.",
                        "default": "python"
                    }
                },
                "required": ["code"]
            }),
        },
        Tool {
            name: "developer__database_query".to_string(),
            description: "Run a read-only SQL query against the local SQLite database.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "SQL query to execute."
                    }
                },
                "required": ["query"]
            }),
        },
        Tool {
            name: "developer__subagent_spawn".to_string(),
            description: "Create a sub-agent to handle a delegated task. Use sync mode to wait for the result immediately, or async mode to run it in the background and manage it through the sub-agent pool.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "task": {
                        "type": "string",
                        "description": "The delegated task for the sub-agent. Include all context needed to complete it."
                    },
                    "mode": {
                        "type": "string",
                        "enum": ["async", "sync"],
                        "description": "Execution mode. async returns immediately with a subagent_id. sync waits for completion.",
                        "default": "async"
                    },
                    "name": {
                        "type": "string",
                        "description": "Optional human-readable sub-agent name."
                    },
                    "max_turns": {
                        "type": "integer",
                        "description": "Optional max turns for the child agent."
                    },
                    "timeout_ms": {
                        "type": "integer",
                        "description": "Optional total timeout for the child agent."
                    },
                    "provider": {
                        "type": "string",
                        "description": "Optional provider override. Defaults to the parent provider."
                    },
                    "model": {
                        "type": "string",
                        "description": "Optional model override. Defaults to the parent model."
                    }
                },
                "required": ["task"]
            }),
        },
        Tool {
            name: "developer__subagent_status".to_string(),
            description: "Inspect the sub-agent pool or a specific sub-agent, including status, messages, and optional result.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "subagent_id": {
                        "type": "string",
                        "description": "Optional sub-agent id. Omit to list the whole pool."
                    },
                    "include_messages": {
                        "type": "boolean",
                        "description": "Whether to include mailbox messages.",
                        "default": false
                    },
                    "include_result": {
                        "type": "boolean",
                        "description": "Whether to include full result text.",
                        "default": false
                    }
                },
                "required": []
            }),
        },
        Tool {
            name: "developer__subagent_message".to_string(),
            description: "Send a mailbox message between the parent agent and a sub-agent. Parent calls should specify subagent_id; a sub-agent may omit it to message its parent.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "subagent_id": {
                        "type": "string",
                        "description": "Target sub-agent id. Required when the parent sends to a child."
                    },
                    "message": {
                        "type": "string",
                        "description": "Message text."
                    }
                },
                "required": ["message"]
            }),
        },
        Tool {
            name: "developer__subagent_wait".to_string(),
            description: "Wait for an async sub-agent to reach a terminal status and return its result/status.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "subagent_id": {
                        "type": "string",
                        "description": "Sub-agent id to wait for."
                    },
                    "timeout_ms": {
                        "type": "integer",
                        "description": "Maximum wait time.",
                        "default": 60000
                    },
                    "include_messages": {
                        "type": "boolean",
                        "description": "Whether to include mailbox messages.",
                        "default": true
                    }
                },
                "required": ["subagent_id"]
            }),
        },
        Tool {
            name: "developer__subagent_cancel".to_string(),
            description: "Cancel a running sub-agent, or cancel all running sub-agents when subagent_id is omitted.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "subagent_id": {
                        "type": "string",
                        "description": "Optional sub-agent id. Omit to cancel all running sub-agents in the pool."
                    }
                },
                "required": []
            }),
        },
        Tool {
            name: "developer__skill_load".to_string(),
            description: "Load full instructions for an available skill, or read a text file inside that skill bundle. Use this before following an implicitly selected skill.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Skill name, such as code-review."
                    },
                    "file": {
                        "type": "string",
                        "description": "Optional relative path inside the skill bundle, such as references/checklist.md."
                    }
                },
                "required": ["name"]
            }),
        },
    ]
}
