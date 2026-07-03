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
    ]
}
