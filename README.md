# Night24

Night24 is a Rust-based AI Agent backend inspired by Goose. It provides an HTTP API for interacting with AI models through a provider-agnostic architecture.

## Features

- **Provider-agnostic**: Supports OpenAI-compatible APIs, Ollama, and an echo provider for testing
- **Tool execution**: Built-in tools for shell commands, file read/write, HTTP requests, web search, and web scraping
- **Session management**: SQLite-backed session persistence by default, with in-memory fallback if SQLite cannot initialize
- **Context compaction**: Automatic context window management
- **Security**: Input/output inspection and permission-based tool access
- **Streaming**: Server-Sent Events (SSE) for real-time responses
- **OpenAPI**: Auto-generated API docs at `/swagger-ui`

## Quick Start

```bash
# Build
cargo build --release -p night24-server

# Run
./target/release/night24-server.exe

# Or with Docker
docker compose up --build
```

## API

### Health Check
```
GET /healthz
```

### Chat
```
POST /reply
Content-Type: application/json

{
  "text": "hello world",
  "provider": "echo",
  "model": "gpt-4o-mini"
}
```

### OpenAPI Docs
```
GET /swagger-ui
GET /api-docs/openapi.json
```

## Environment Variables

- `NIGHT24_DATABASE_URL`: SQLite database URL (defaults to `night24.db` in the server working directory)
- `NIGHT24_DATA_DIR`: directory used for the default `night24.db` when `NIGHT24_DATABASE_URL` is not set
- `NIGHT24_NETWORK_PROXY`: HTTP/HTTPS proxy used by network tools. Per-tool `proxy` arguments override this;
  use `proxy: "direct"` to bypass proxy environment settings for a single call.
- `OPENAI_API_KEY`: OpenAI API key for the OpenAI provider
- `OPENAI_BASE_URL` / `OPENAI_MODEL`: override the OpenAI-compatible endpoint and default model
- `ANTHROPIC_API_KEY` / `ANTHROPIC_BASE_URL` / `ANTHROPIC_MODEL`: Anthropic provider (registered only if key is set)
- `STEPFUN_API_KEY` / `STEPFUN_BASE_URL` / `STEPFUN_MODEL`: StepFun provider (registered only if key is set)
- `OLLAMA_BASE_URL` / `OLLAMA_MODEL`: Ollama provider (defaults to `http://localhost:11434` / `llama3.2`)
- `NIGHT24_API_KEY`: when set, all routes (except `/healthz`, `/swagger-ui`, `/api-docs`) require
  `Authorization: Bearer <key>` or `X-API-Key: <key>`. When unset, the server is open.
- `NIGHT24_PERMISSION_MODE`: tool permission policy — `strict` (default, confirm all),
  `permissive` (auto-allow read-only tools), `allow_all`, or `deny_all`.

Network tools also respect `HTTPS_PROXY` / `HTTP_PROXY` when `NIGHT24_NETWORK_PROXY` is unset.
Web search results are cleaned, deduplicated, and truncated before being returned to reduce token usage.

> Providers are registered lazily: only those whose API key is present in the
> environment are enabled. No secrets are hard-coded in the source.

## License

Apache-2.0
