# Speciale

[![Release](https://img.shields.io/github/v/release/Cupnfish/speciale)](https://github.com/Cupnfish/speciale/releases)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

MCP server for [DeepSeek-V3.2-Speciale](https://api-docs.deepseek.com/zh-cn/guides/thinking_mode), a powerful reasoning model.

## Features

- Deep reasoning for complex math, code analysis, logical problems
- **File references**: `@path` syntax for files and directories
- **Grep search**: `@grep:pattern` syntax using ripgrep/grep
- Multi-turn conversations with persistence
- Local storage in DuckDB (`~/.speciale/conversations.db`)

## Installation

### Download from GitHub Releases

Download the latest binary for your platform from [Releases](https://github.com/Cupnfish/speciale/releases):

| Platform | File |
|----------|------|
| Linux x86_64 | `speciale-linux-x86_64` |
| Linux ARM64 | `speciale-linux-aarch64` |
| macOS Intel | `speciale-darwin-x86_64` |
| macOS Apple Silicon | `speciale-darwin-aarch64` |
| Windows | `speciale-windows-x86_64.exe` |

```bash
# Example for macOS Apple Silicon
curl -L https://github.com/Cupnfish/speciale/releases/latest/download/speciale-darwin-aarch64 -o speciale
chmod +x speciale
sudo mv speciale /usr/local/bin/
```

### Build from source

```bash
cargo install --git https://github.com/Cupnfish/speciale.git
```

## Configuration

Set your DeepSeek API key:

```bash
export DEEPSEEK_API_KEY="your-api-key"
```

## MCP Client Setup

### Claude Code (CLI)

The easiest way - automatically uses current directory:

**macOS/Linux:**
```bash
claude mcp add speciale -e DEEPSEEK_API_KEY=your-api-key -- speciale --pwd $(pwd)
```

**Windows (PowerShell):**
```powershell
claude mcp add speciale -e DEEPSEEK_API_KEY=your-api-key -- speciale --pwd $PWD
```

**Windows (CMD):**
```cmd
claude mcp add speciale -e DEEPSEEK_API_KEY=your-api-key -- speciale --pwd %cd%
```

### Claude Code (Config File)

Edit `~/.claude.json`:

**macOS/Linux:**
```json
{
  "mcpServers": {
    "speciale": {
      "command": "bash",
      "args": ["-c", "speciale --pwd \"$PWD\""],
      "env": {
        "DEEPSEEK_API_KEY": "your-api-key"
      }
    }
  }
}
```

**Windows:**
```json
{
  "mcpServers": {
    "speciale": {
      "command": "cmd",
      "args": ["/c", "speciale --pwd %cd%"],
      "env": {
        "DEEPSEEK_API_KEY": "your-api-key"
      }
    }
  }
}
```

### Cursor

Edit `~/.cursor/mcp.json`:

**macOS/Linux:**
```json
{
  "mcpServers": {
    "speciale": {
      "command": "bash",
      "args": ["-c", "speciale --pwd \"$PWD\""],
      "env": {
        "DEEPSEEK_API_KEY": "your-api-key"
      }
    }
  }
}
```

### Claude Desktop

Config locations:
- **macOS**: `~/Library/Application Support/Claude/claude_desktop_config.json`
- **Windows**: `%APPDATA%\Claude\claude_desktop_config.json`

```json
{
  "mcpServers": {
    "speciale": {
      "command": "speciale",
      "args": ["--pwd", "/path/to/your/project"],
      "env": {
        "DEEPSEEK_API_KEY": "your-api-key"
      }
    }
  }
}
```

> **Note**: Claude Desktop doesn't support dynamic PWD injection. Specify the project path directly.

## Reference Syntax

| Syntax | Description |
|--------|-------------|
| `@src/main.rs` | Include a single file |
| `@src/` | Include all files in directory (recursive) |
| `@grep:pattern` | Search with ripgrep (fallback to grep) |
| `@grep:pattern:*.rs` | Search with glob filter |

**Examples:**
```
Review @src/main.rs for bugs
Analyze all code in @src/
Find usages of @grep:async_fn
Search in Rust files @grep:impl:*.rs
```

> **Tip**: Only use `@` syntax when entire file content is needed. For small snippets, include them directly in your question.

## Tool: ask_speciale

### Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `question` | string | Yes | Use `@` syntax to include files/searches |
| `system_prompt` | string | No | System prompt (new conversations only) |
| `include_reasoning` | boolean | No | Show reasoning (default: false) |
| `conversation_id` | string | No | Continue previous conversation |

### Response

```json
{
  "answer": "Response",
  "reasoning": "If requested",
  "conversation_id": "uuid",
  "context_tokens": 1234,
  "max_context_tokens": 131072,
  "tip": "Usage info"
}
```

## Important

**This model has NO external access and CANNOT call tools.** It can only reason based on what you provide.

## License

MIT

## Links

- [GitHub](https://github.com/Cupnfish/speciale)
- [Releases](https://github.com/Cupnfish/speciale/releases)
- [DeepSeek API](https://api-docs.deepseek.com/zh-cn/guides/thinking_mode)
- [MCP Protocol](https://modelcontextprotocol.io/)
