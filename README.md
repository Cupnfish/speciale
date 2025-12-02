# Speciale

MCP server for DeepSeek-V3.2-Speciale, a powerful reasoning model.

## Features

- Deep reasoning for complex math, code analysis, logical problems
- Multi-turn conversations with persistence
- Context tracking (token usage)
- Storage in DuckDB (`~/.speciale/conversations.db`)

## Build

```bash
cargo build --release
```

## Configuration

```bash
export DEEPSEEK_API_KEY="your-api-key"
```

## MCP Client Setup

### Claude Code

`~/.claude/claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "speciale": {
      "command": "/path/to/speciale",
      "env": {
        "DEEPSEEK_API_KEY": "your-api-key"
      }
    }
  }
}
```

### Cursor

`~/.cursor/mcp.json`:

```json
{
  "mcpServers": {
    "speciale": {
      "command": "/path/to/speciale",
      "env": {
        "DEEPSEEK_API_KEY": "your-api-key"
      }
    }
  }
}
```

### Claude Desktop

- macOS: `~/Library/Application Support/Claude/claude_desktop_config.json`
- Windows: `%APPDATA%\Claude\claude_desktop_config.json`

```json
{
  "mcpServers": {
    "speciale": {
      "command": "/path/to/speciale",
      "env": {
        "DEEPSEEK_API_KEY": "your-api-key"
      }
    }
  }
}
```

## Tool: ask_speciale

### Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `question` | string | Yes | Include ALL relevant context |
| `system_prompt` | string | No | For new conversations only |
| `include_reasoning` | boolean | No | Show reasoning (default: false) |
| `conversation_id` | string | No | Continue previous conversation |

### Response

```json
{
  "answer": "Response text",
  "reasoning": "If requested",
  "conversation_id": "uuid",
  "context_tokens": 1234,
  "max_context_tokens": 131072,
  "tip": "Usage info"
}
```

## Important

**This model has NO external access and CANNOT call tools.**

Always include complete context:
- Full code snippets
- Relevant data
- Background info

## Best For

- Complex math
- Code review/analysis
- Logical reasoning
- Algorithm design
- Debugging (with full code)

## License

MIT
