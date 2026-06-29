# Night24

Night24 is a Rust-based AI Agent backend inspired by Goose. It provides an HTTP API for interacting with AI models through a provider-agnostic architecture.

## Features

- **Provider-agnostic**: Supports OpenAI-compatible APIs, Ollama, and an echo provider for testing
- **Tool execution**: Built-in tools for shell commands, file read/write
- **Session management**: In-memory and SQLite-backed session persistence
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

- `NIGHT24_DATABASE_URL`: SQLite database URL (e.g., `sqlite:file:/data/night24.db?mode=rwc`)
- `OPENAI_API_KEY`: OpenAI API key for the OpenAI provider

## License

Apache-2.0
