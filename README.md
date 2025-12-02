# Speciale

[![Crates.io](https://img.shields.io/crates/v/speciale.svg)](https://crates.io/crates/speciale)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

MCP server for [DeepSeek-V3.2-Speciale](https://api-docs.deepseek.com/zh-cn/guides/thinking_mode), a powerful reasoning model with deep thinking capabilities.

## Features

- Deep reasoning for complex math, code analysis, logical problems
- Multi-turn conversations with persistence
- Context tracking (token usage)
- Local storage in DuckDB (`~/.speciale/conversations.db`)

## Installation

### From crates.io

```bash
cargo install speciale
```

### From source

```bash
git clone https://github.com/Cupnfish/speciale.git
cd speciale
cargo install --path .
```

### Build only

```bash
git clone https://github.com/Cupnfish/speciale.git
cd speciale
cargo build --release
# Binary at: target/release/speciale
```

## Configuration

Set your DeepSeek API key:

```bash
export DEEPSEEK_API_KEY="your-api-key"
```

## MCP Client Setup

### Claude Code

Add to `~/.claude/claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "speciale": {
      "command": "speciale",
      "env": {
        "DEEPSEEK_API_KEY": "your-api-key"
      }
    }
  }
}
```

Or with full path:

```json
{
  "mcpServers": {
    "speciale": {
      "command": "/Users/yourname/.cargo/bin/speciale",
      "env": {
        "DEEPSEEK_API_KEY": "your-api-key"
      }
    }
  }
}
```

### Cursor

Add to `~/.cursor/mcp.json`:

```json
{
  "mcpServers": {
    "speciale": {
      "command": "speciale",
      "env": {
        "DEEPSEEK_API_KEY": "your-api-key"
      }
    }
  }
}
```

### Claude Desktop

Config file location:
- **macOS**: `~/Library/Application Support/Claude/claude_desktop_config.json`
- **Windows**: `%APPDATA%\Claude\claude_desktop_config.json`

```json
{
  "mcpServers": {
    "speciale": {
      "command": "speciale",
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
| `question` | string | Yes | The question. Include ALL relevant context. |
| `system_prompt` | string | No | System prompt (new conversations only) |
| `include_reasoning` | boolean | No | Show reasoning process (default: false) |
| `conversation_id` | string | No | Continue a previous conversation |

### Response

```json
{
  "answer": "The model's response",
  "reasoning": "Reasoning process (if include_reasoning=true)",
  "conversation_id": "uuid-for-follow-ups",
  "context_tokens": 1234,
  "max_context_tokens": 131072,
  "tip": "Context usage or follow-up instructions"
}
```

### Example

**New conversation:**
```json
{
  "question": "Analyze this code:\n```python\ndef calc(x):\n    return x / 0\n```"
}
```

**Follow-up:**
```json
{
  "question": "How to fix it?",
  "conversation_id": "uuid-from-previous-response"
}
```

## Important Notes

> **This model has NO external access and CANNOT call tools.**

Always include complete context in your question:
- Full code snippets (not references)
- Relevant data and examples  
- Background information
- Specific requirements

The model can only reason based on what you provide.

## Best Use Cases

- Complex mathematical problems
- Code review and analysis
- Logical reasoning tasks
- Algorithm design
- Debugging (with full code provided)
- Detailed explanations

## License

MIT - see [LICENSE](LICENSE)

## Links

- [GitHub Repository](https://github.com/Cupnfish/speciale)
- [DeepSeek API Docs](https://api-docs.deepseek.com/zh-cn/guides/thinking_mode)
- [MCP Protocol](https://modelcontextprotocol.io/)
