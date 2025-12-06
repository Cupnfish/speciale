use duckdb::{Connection, params};
use rmcp::{
    ErrorData as McpError, ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::*,
    schemars, tool, tool_handler, tool_router,
    transport::stdio,
};
use serde::{Deserialize, Serialize};
use std::{
    borrow::Cow,
    env,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::Mutex;
use uuid::Uuid;

const DEEPSEEK_BASE_URL: &str = "https://api.deepseek.com/v3.2_speciale_expires_on_20251215";

fn read_single_file_to_markdown(path: &Path) -> String {
    let path_str = path.display().to_string();
    match std::fs::read_to_string(path) {
        Ok(file_content) => {
            format!("\n## File: `{}`\n\n```\n{}\n```\n", path_str, file_content.trim_end())
        }
        Err(e) => format!("\n## File: `{}` (Error: {})\n\n", path_str, e),
    }
}

fn read_dir_to_markdown(dir: &Path) -> String {
    let mut content = String::new();

    fn visit_dir(dir: &Path, content: &mut String) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            let mut entries: Vec<_> = entries.filter_map(|e| e.ok()).collect();
            entries.sort_by_key(|e| e.path());

            for entry in entries {
                let path = entry.path();
                // Skip hidden files and common ignore patterns
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with('.')
                        || name == "node_modules"
                        || name == "target"
                        || name == "__pycache__"
                    {
                        continue;
                    }
                }

                if path.is_dir() {
                    visit_dir(&path, content);
                } else if path.is_file() {
                    // Skip binary files by checking extension
                    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                        let skip_exts = [
                            "png", "jpg", "jpeg", "gif", "ico", "svg", "woff", "woff2", "ttf",
                            "eot", "mp3", "mp4", "zip", "tar", "gz", "exe", "dll", "so", "dylib",
                            "o", "a",
                        ];
                        if skip_exts.contains(&ext.to_lowercase().as_str()) {
                            continue;
                        }
                    }
                    content.push_str(&read_single_file_to_markdown(&path));
                }
            }
        }
    }

    visit_dir(dir, &mut content);
    content
}

fn run_grep(pwd: &Path, pattern: &str, glob: Option<&str>) -> String {
    // Try ripgrep first, fallback to grep
    let result = try_ripgrep(pwd, pattern, glob)
        .or_else(|| try_grep(pwd, pattern, glob));
    
    match result {
        Some(output) if output.is_empty() => {
            format!("\n## Grep: `{}`\n\nNo matches found.\n", pattern)
        }
        Some(output) => {
            format!("\n## Grep: `{}`{}\n\n```\n{}\n```\n", 
                pattern,
                glob.map(|g| format!(" (glob: {})", g)).unwrap_or_default(),
                output.trim_end())
        }
        None => format!("\n## Grep: `{}` (Error: neither rg nor grep available)\n\n", pattern),
    }
}

fn try_ripgrep(pwd: &Path, pattern: &str, glob: Option<&str>) -> Option<String> {
    let mut cmd = std::process::Command::new("rg");
    cmd.arg("--no-heading")
        .arg("--line-number")
        .arg("--color=never")
        .arg("-m").arg("100")
        .current_dir(pwd);
    
    if let Some(g) = glob {
        cmd.arg("-g").arg(g);
    }
    cmd.arg(pattern);
    
    cmd.output().ok().map(|o| String::from_utf8_lossy(&o.stdout).to_string())
}

fn try_grep(pwd: &Path, pattern: &str, glob: Option<&str>) -> Option<String> {
    let mut cmd = std::process::Command::new("grep");
    cmd.arg("-rn")
        .arg("--color=never")
        .current_dir(pwd);
    
    if let Some(g) = glob {
        cmd.arg("--include").arg(g);
    }
    cmd.arg(pattern).arg(".");
    
    cmd.output().ok().map(|o| String::from_utf8_lossy(&o.stdout).to_string())
}

fn expand_file_references(question: &str, pwd: &Path) -> String {
    // Patterns:
    // - @path or @path/ for files/directories
    // - @grep:pattern or @grep:pattern:glob for ripgrep search
    let mut result = question.to_string();
    let mut references: Vec<(String, String)> = Vec::new();

    let mut i = 0;
    let chars: Vec<char> = question.chars().collect();

    while i < chars.len() {
        if chars[i] == '@' && (i == 0 || chars[i - 1].is_whitespace()) {
            i += 1;

            // Collect until whitespace or end
            let mut ref_chars = String::new();
            while i < chars.len() && !chars[i].is_whitespace() {
                ref_chars.push(chars[i]);
                i += 1;
            }

            if !ref_chars.is_empty() {
                let reference = format!("@{}", ref_chars);
                
                let content = if ref_chars.starts_with("grep:") {
                    // @grep:pattern or @grep:pattern:glob
                    let grep_part = &ref_chars[5..]; // skip "grep:"
                    let parts: Vec<&str> = grep_part.splitn(2, ':').collect();
                    let pattern = parts[0];
                    let glob = parts.get(1).copied();
                    run_grep(pwd, pattern, glob)
                } else {
                    // File or directory reference
                    let full_path = pwd.join(&ref_chars);
                    if ref_chars.ends_with('/') || full_path.is_dir() {
                        read_dir_to_markdown(&full_path)
                    } else {
                        read_single_file_to_markdown(&full_path)
                    }
                };

                references.push((reference, content));
            }
        } else {
            i += 1;
        }
    }

    // Replace references with content
    for (reference, content) in references {
        result = result.replacen(&reference, &content, 1);
    }

    result
}
const DEFAULT_MAX_TOKENS: u32 = 64 * 1024;
const MAX_CONTEXT_TOKENS: u32 = 128 * 1024;

fn get_db_path() -> PathBuf {
    let home = dirs::home_dir().expect("Failed to get home directory");
    let dir = home.join(".speciale");
    std::fs::create_dir_all(&dir).expect("Failed to create ~/.speciale directory");
    dir.join("conversations.db")
}

fn init_db(conn: &Connection) -> duckdb::Result<()> {
    conn.execute_batch(
        "CREATE SEQUENCE IF NOT EXISTS seq_messages_id START 1;
        CREATE TABLE IF NOT EXISTS conversations (
            id VARCHAR PRIMARY KEY,
            system_prompt TEXT,
            total_tokens INTEGER DEFAULT 0,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
        );
        CREATE TABLE IF NOT EXISTS messages (
            id INTEGER DEFAULT nextval('seq_messages_id'),
            conversation_id VARCHAR NOT NULL,
            role VARCHAR NOT NULL,
            content TEXT NOT NULL,
            reasoning_content TEXT,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
        );
        CREATE INDEX IF NOT EXISTS idx_messages_conv ON messages(conversation_id);",
    )?;
    Ok(())
}

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

#[derive(Debug, Serialize, Clone)]
struct ChatMessage {
    role: String,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ResponseMessage,
}

#[derive(Debug, Deserialize)]
struct ResponseMessage {
    content: String,
    #[serde(default)]
    reasoning_content: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct Usage {
    #[serde(default)]
    total_tokens: u32,
}

#[derive(Clone)]
pub struct Speciale {
    client: Arc<reqwest::Client>,
    api_key: Arc<String>,
    db: Arc<Mutex<Connection>>,
    pwd: Arc<PathBuf>,
    tool_router: ToolRouter<Speciale>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AskRequest {
    #[schemars(
        description = "The question to ask DeepSeek Speciale. Supports complex reasoning tasks."
    )]
    pub question: String,
    #[schemars(
        description = "Optional system prompt. Only used when starting a new conversation."
    )]
    #[serde(default)]
    pub system_prompt: Option<String>,
    #[schemars(description = "Whether to include reasoning process. Default is false.")]
    #[serde(default)]
    pub include_reasoning: bool,
    #[schemars(
        description = "Conversation ID for follow-up questions. Omit to start a new conversation."
    )]
    #[serde(default)]
    pub conversation_id: Option<String>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct AskResponse {
    pub answer: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    pub conversation_id: String,
    pub context_tokens: u32,
    pub max_context_tokens: u32,
    pub tip: String,
}

const MAX_RETRIES: u32 = 3;

async fn call_deepseek(
    client: &reqwest::Client,
    api_key: &str,
    messages: Vec<ChatMessage>,
) -> Result<(String, Option<String>, u32), McpError> {
    let request = ChatRequest {
        model: "deepseek-reasoner".to_string(),
        messages,
        max_tokens: Some(DEFAULT_MAX_TOKENS),
    };

    let mut last_error = None;

    for attempt in 1..=MAX_RETRIES {
        let result = do_request(client, api_key, &request).await;

        match result {
            Ok((content, reasoning, tokens)) => {
                // Retry if content is empty
                if content.is_empty() && attempt < MAX_RETRIES {
                    last_error = Some("Empty content from model".to_string());
                    continue;
                }
                return Ok((content, reasoning, tokens));
            }
            Err(e) => {
                last_error = Some(e.clone());
                // Don't retry on client errors (4xx)
                if e.contains("API error 4") {
                    return Err(McpError {
                        code: ErrorCode::INTERNAL_ERROR,
                        message: Cow::from(e),
                        data: None,
                    });
                }
                // Retry on other errors
                if attempt < MAX_RETRIES {
                    continue;
                }
            }
        }
    }

    Err(McpError {
        code: ErrorCode::INTERNAL_ERROR,
        message: Cow::from(format!(
            "Failed after {} retries: {}",
            MAX_RETRIES,
            last_error.unwrap_or_default()
        )),
        data: None,
    })
}

async fn do_request(
    client: &reqwest::Client,
    api_key: &str,
    request: &ChatRequest,
) -> Result<(String, Option<String>, u32), String> {
    let response = client
        .post(format!("{}/chat/completions", DEEPSEEK_BASE_URL))
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(request)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(format!("API error {}: {}", status, text));
    }

    let chat_response: ChatResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    let choice = chat_response
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| "No response from model".to_string())?;

    let total_tokens = chat_response.usage.map(|u| u.total_tokens).unwrap_or(0);
    Ok((
        choice.message.content,
        choice.message.reasoning_content,
        total_tokens,
    ))
}

#[tool_router]
impl Speciale {
    pub fn new(pwd: PathBuf) -> Result<Self, String> {
        let api_key = env::var("DEEPSEEK_API_KEY")
            .map_err(|_| "DEEPSEEK_API_KEY environment variable not set")?;

        let db_path = get_db_path();
        let conn =
            Connection::open(&db_path).map_err(|e| format!("Failed to open database: {}", e))?;
        init_db(&conn).map_err(|e| format!("Failed to init database: {}", e))?;

        Ok(Self {
            client: Arc::new(reqwest::Client::new()),
            api_key: Arc::new(api_key),
            db: Arc::new(Mutex::new(conn)),
            pwd: Arc::new(pwd),
            tool_router: Self::tool_router(),
        })
    }

    fn load_conversation(
        &self,
        conn: &Connection,
        conv_id: &str,
    ) -> Result<(Option<String>, Vec<ChatMessage>, u32), McpError> {
        // Get conversation info
        let mut stmt = conn
            .prepare("SELECT system_prompt, total_tokens FROM conversations WHERE id = ?")
            .map_err(|e| McpError {
                code: ErrorCode::INTERNAL_ERROR,
                message: Cow::from(format!("DB error: {}", e)),
                data: None,
            })?;

        let conv: Result<(Option<String>, u32), _> =
            stmt.query_row([conv_id], |row| Ok((row.get(0)?, row.get(1)?)));

        let (system_prompt, total_tokens) = conv.map_err(|_| McpError {
            code: ErrorCode::INVALID_PARAMS,
            message: Cow::from(format!("Conversation '{}' not found", conv_id)),
            data: None,
        })?;

        // Get messages
        let mut stmt = conn.prepare(
            "SELECT role, content, reasoning_content FROM messages WHERE conversation_id = ? ORDER BY id"
        ).map_err(|e| McpError {
            code: ErrorCode::INTERNAL_ERROR,
            message: Cow::from(format!("DB error: {}", e)),
            data: None,
        })?;

        let messages: Vec<ChatMessage> = stmt
            .query_map([conv_id], |row| {
                Ok(ChatMessage {
                    role: row.get(0)?,
                    content: row.get(1)?,
                    reasoning_content: row.get(2)?,
                })
            })
            .map_err(|e| McpError {
                code: ErrorCode::INTERNAL_ERROR,
                message: Cow::from(format!("DB error: {}", e)),
                data: None,
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok((system_prompt, messages, total_tokens))
    }

    fn save_message(
        &self,
        conn: &Connection,
        conv_id: &str,
        role: &str,
        content: &str,
        reasoning: Option<&str>,
    ) -> Result<(), McpError> {
        conn.execute(
            "INSERT INTO messages (conversation_id, role, content, reasoning_content) VALUES (?, ?, ?, ?)",
            params![conv_id, role, content, reasoning]
        ).map_err(|e| McpError {
            code: ErrorCode::INTERNAL_ERROR,
            message: Cow::from(format!("Failed to save message: {}", e)),
            data: None,
        })?;
        Ok(())
    }

    fn update_tokens(&self, conn: &Connection, conv_id: &str, tokens: u32) -> Result<(), McpError> {
        conn.execute(
            "UPDATE conversations SET total_tokens = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
            params![tokens, conv_id]
        ).map_err(|e| McpError {
            code: ErrorCode::INTERNAL_ERROR,
            message: Cow::from(format!("Failed to update tokens: {}", e)),
            data: None,
        })?;
        Ok(())
    }

    #[tool(
        description = "Ask DeepSeek-V3.2-Speciale (a powerful reasoning model) a question. This model has NO external access. Use @path for files, @dir/ for directories, @grep:pattern or @grep:pattern:glob for search. Only use these when full content is needed; otherwise include snippets directly. Good for: math, code analysis, logical reasoning. Returns conversation_id for follow-ups."
    )]
    async fn ask_speciale(
        &self,
        Parameters(req): Parameters<AskRequest>,
    ) -> Result<CallToolResult, McpError> {
        let db = self.db.lock().await;

        let (conv_id, mut api_messages, is_new) = if let Some(ref cid) = req.conversation_id {
            // Continue existing conversation
            let (system_prompt, messages, _) = self.load_conversation(&db, cid)?;

            let mut api_messages = Vec::new();
            if let Some(sys) = system_prompt {
                api_messages.push(ChatMessage {
                    role: "system".to_string(),
                    content: sys,
                    reasoning_content: None,
                });
            }
            api_messages.extend(messages);

            (cid.clone(), api_messages, false)
        } else {
            // Create new conversation
            let conv_id = Uuid::new_v4().to_string();
            db.execute(
                "INSERT INTO conversations (id, system_prompt) VALUES (?, ?)",
                params![&conv_id, &req.system_prompt],
            )
            .map_err(|e| McpError {
                code: ErrorCode::INTERNAL_ERROR,
                message: Cow::from(format!("Failed to create conversation: {}", e)),
                data: None,
            })?;

            let mut api_messages = Vec::new();
            if let Some(ref sys) = req.system_prompt {
                api_messages.push(ChatMessage {
                    role: "system".to_string(),
                    content: sys.clone(),
                    reasoning_content: None,
                });
            }

            (conv_id, api_messages, true)
        };

        // Build user message: expand @path references
        let user_content = expand_file_references(&req.question, &self.pwd);

        // Add user message
        api_messages.push(ChatMessage {
            role: "user".to_string(),
            content: user_content.clone(),
            reasoning_content: None,
        });

        // Call API
        let (answer, reasoning, total_tokens) =
            call_deepseek(&self.client, &self.api_key, api_messages).await?;

        // Handle empty answer (might have reasoning only)
        let final_answer = if answer.is_empty() {
            if let Some(ref r) = reasoning {
                format!("[No direct answer, see reasoning]\n\n{}", r)
            } else {
                "[Empty response from model]".to_string()
            }
        } else {
            answer.clone()
        };

        // Save to DB (save full content with files for conversation history)
        self.save_message(&db, &conv_id, "user", &user_content, None)?;
        self.save_message(
            &db,
            &conv_id,
            "assistant",
            &final_answer,
            reasoning.as_deref(),
        )?;
        self.update_tokens(&db, &conv_id, total_tokens)?;

        let usage_pct = (total_tokens as f64 / MAX_CONTEXT_TOKENS as f64 * 100.0) as u32;
        let tip = if is_new {
            format!(
                "To continue this conversation, include conversation_id: \"{}\"",
                conv_id
            )
        } else if usage_pct > 80 {
            format!(
                "Warning: {}% context used ({}/{}). Consider starting a new conversation.",
                usage_pct, total_tokens, MAX_CONTEXT_TOKENS
            )
        } else {
            format!(
                "Context: {}% used ({}/{} tokens)",
                usage_pct, total_tokens, MAX_CONTEXT_TOKENS
            )
        };

        let response = AskResponse {
            answer: final_answer.clone(),
            reasoning: if req.include_reasoning {
                reasoning
            } else {
                None
            },
            conversation_id: conv_id,
            context_tokens: total_tokens,
            max_context_tokens: MAX_CONTEXT_TOKENS,
            tip,
        };

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&response).unwrap_or(final_answer),
        )]))
    }
}

#[tool_handler]
impl ServerHandler for Speciale {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::LATEST,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation::from_build_env(),
            instructions: Some(
                "DeepSeek-V3.2-Speciale: A powerful reasoning model. NO external access. \
                 Syntax: @file, @dir/, @grep:pattern, @grep:pattern:glob. \
                 Only use when full content needed; otherwise include snippets directly. \
                 Use conversation_id for follow-ups. Best for: math, code review, logic."
                    .to_string(),
            ),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse --pwd argument (required)
    let args: Vec<String> = env::args().collect();
    let mut pwd: Option<PathBuf> = None;

    let mut i = 1;
    while i < args.len() {
        if args[i] == "--pwd" && i + 1 < args.len() {
            pwd = Some(PathBuf::from(&args[i + 1]));
            i += 2;
        } else if args[i].starts_with("--pwd=") {
            pwd = Some(PathBuf::from(args[i].strip_prefix("--pwd=").unwrap()));
            i += 1;
        } else {
            i += 1;
        }
    }

    let pwd = pwd.unwrap_or_else(|| {
        panic!("Error: --pwd argument is required\nUsage: speciale --pwd /path/to/project");
    });

    let server = Speciale::new(pwd).map_err(|e| {
        eprintln!("Error: {}", e);
        e
    })?;

    let service = server.serve(stdio()).await?;
    service.waiting().await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ask_request_new_conversation() {
        let json = r#"{"question": "Hello"}"#;
        let req: AskRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.question, "Hello");
        assert!(req.conversation_id.is_none());
    }

    #[test]
    fn test_ask_request_continue_conversation() {
        let json = r#"{"question": "Follow up", "conversation_id": "abc-123"}"#;
        let req: AskRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.conversation_id, Some("abc-123".to_string()));
    }

    #[test]
    fn test_ask_response_serialization() {
        let response = AskResponse {
            answer: "42".to_string(),
            reasoning: None,
            conversation_id: "test-id".to_string(),
            context_tokens: 100,
            max_context_tokens: MAX_CONTEXT_TOKENS,
            tip: "test tip".to_string(),
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("conversation_id"));
        assert!(json.contains("tip"));
    }

    #[test]
    fn test_db_init() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();

        // Verify tables exist
        let count: i32 = conn.query_row(
            "SELECT COUNT(*) FROM information_schema.tables WHERE table_name IN ('conversations', 'messages')",
            [],
            |row| row.get(0)
        ).unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn test_db_insert_conversation_and_messages() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();

        // Insert conversation
        let conv_id = "test-conv-123";
        conn.execute(
            "INSERT INTO conversations (id, system_prompt) VALUES (?, ?)",
            params![conv_id, "You are helpful"],
        )
        .unwrap();

        // Insert messages - should auto-generate IDs
        conn.execute(
            "INSERT INTO messages (conversation_id, role, content, reasoning_content) VALUES (?, ?, ?, ?)",
            params![conv_id, "user", "Hello", Option::<String>::None],
        ).unwrap();

        conn.execute(
            "INSERT INTO messages (conversation_id, role, content, reasoning_content) VALUES (?, ?, ?, ?)",
            params![conv_id, "assistant", "Hi there!", Some("thinking...")],
        ).unwrap();

        // Verify messages were inserted with auto-generated IDs
        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE conversation_id = ?",
                [conv_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);

        // Verify IDs are different
        let ids: Vec<i32> = conn
            .prepare("SELECT id FROM messages WHERE conversation_id = ? ORDER BY id")
            .unwrap()
            .query_map([conv_id], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert_eq!(ids.len(), 2);
        assert_ne!(ids[0], ids[1]);
    }

    #[test]
    fn test_db_load_messages() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();

        let conv_id = "load-test-conv";
        conn.execute(
            "INSERT INTO conversations (id, system_prompt, total_tokens) VALUES (?, ?, ?)",
            params![conv_id, "Be helpful", 100],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO messages (conversation_id, role, content, reasoning_content) VALUES (?, ?, ?, ?)",
            params![conv_id, "user", "Question", Option::<String>::None],
        ).unwrap();

        conn.execute(
            "INSERT INTO messages (conversation_id, role, content, reasoning_content) VALUES (?, ?, ?, ?)",
            params![conv_id, "assistant", "Answer", Some("reasoning")],
        ).unwrap();

        // Load conversation
        let (system_prompt, total_tokens): (Option<String>, i32) = conn
            .query_row(
                "SELECT system_prompt, total_tokens FROM conversations WHERE id = ?",
                [conv_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(system_prompt, Some("Be helpful".to_string()));
        assert_eq!(total_tokens, 100);

        // Load messages
        let messages: Vec<(String, String, Option<String>)> = conn
            .prepare("SELECT role, content, reasoning_content FROM messages WHERE conversation_id = ? ORDER BY id")
            .unwrap()
            .query_map([conv_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert_eq!(messages.len(), 2);
        assert_eq!(
            messages[0],
            ("user".to_string(), "Question".to_string(), None)
        );
        assert_eq!(
            messages[1],
            (
                "assistant".to_string(),
                "Answer".to_string(),
                Some("reasoning".to_string())
            )
        );
    }

    #[test]
    fn test_chat_message_serialization() {
        let msg = ChatMessage {
            role: "assistant".to_string(),
            content: "answer".to_string(),
            reasoning_content: Some("thinking".to_string()),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("reasoning_content"));

        let msg_no_reasoning = ChatMessage {
            role: "user".to_string(),
            content: "question".to_string(),
            reasoning_content: None,
        };
        let json = serde_json::to_string(&msg_no_reasoning).unwrap();
        assert!(!json.contains("reasoning_content"));
    }

    #[test]
    fn test_multi_turn_messages_include_reasoning() {
        // Simulate loading messages from DB for multi-turn conversation
        let messages: Vec<ChatMessage> = vec![
            ChatMessage {
                role: "user".to_string(),
                content: "What is 2+2?".to_string(),
                reasoning_content: None,
            },
            ChatMessage {
                role: "assistant".to_string(),
                content: "4".to_string(),
                reasoning_content: Some("Let me calculate: 2+2=4".to_string()),
            },
            ChatMessage {
                role: "user".to_string(),
                content: "And 3+3?".to_string(),
                reasoning_content: None,
            },
        ];

        // Build API request
        let request = ChatRequest {
            model: "deepseek-reasoner".to_string(),
            messages,
            max_tokens: Some(1000),
        };

        let json = serde_json::to_string_pretty(&request).unwrap();

        // Verify reasoning_content is included for assistant message
        assert!(json.contains("reasoning_content"));
        assert!(json.contains("Let me calculate: 2+2=4"));

        // Verify user messages don't have reasoning_content in JSON
        // (skip_serializing_if = "Option::is_none" works)
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let messages = parsed["messages"].as_array().unwrap();

        // User message (index 0) should NOT have reasoning_content key
        assert!(messages[0].get("reasoning_content").is_none());

        // Assistant message (index 1) SHOULD have reasoning_content
        assert!(messages[1].get("reasoning_content").is_some());
        assert_eq!(messages[1]["reasoning_content"], "Let me calculate: 2+2=4");
    }

    #[test]
    fn test_read_single_file_to_markdown() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "line 1\nline 2\n").unwrap();
        
        let result = read_single_file_to_markdown(&file_path);
        assert!(result.contains("## File:"));
        assert!(result.contains("test.txt"));
        assert!(result.contains("line 1"));
        assert!(result.contains("line 2"));
        assert!(result.contains("```"));
    }

    #[test]
    fn test_read_single_file_to_markdown_not_found() {
        let path = Path::new("/nonexistent/file.txt");
        let result = read_single_file_to_markdown(path);
        assert!(result.contains("Error:"));
    }

    #[test]
    fn test_read_dir_to_markdown() {
        let dir = tempfile::tempdir().unwrap();
        
        // Create test files
        let file1 = dir.path().join("a.txt");
        std::fs::write(&file1, "content a").unwrap();
        
        let file2 = dir.path().join("b.txt");
        std::fs::write(&file2, "content b").unwrap();
        
        // Create subdirectory with file
        let subdir = dir.path().join("sub");
        std::fs::create_dir(&subdir).unwrap();
        let file3 = subdir.join("c.txt");
        std::fs::write(&file3, "content c").unwrap();
        
        let result = read_dir_to_markdown(dir.path());
        assert!(result.contains("a.txt"));
        assert!(result.contains("content a"));
        assert!(result.contains("b.txt"));
        assert!(result.contains("content b"));
        assert!(result.contains("c.txt"));
        assert!(result.contains("content c"));
    }

    #[test]
    fn test_read_dir_skips_hidden_and_special() {
        let dir = tempfile::tempdir().unwrap();
        
        // Create visible file
        std::fs::write(dir.path().join("visible.txt"), "visible").unwrap();
        
        // Create hidden file (should be skipped)
        std::fs::write(dir.path().join(".hidden"), "hidden").unwrap();
        
        // Create node_modules dir (should be skipped)
        let nm = dir.path().join("node_modules");
        std::fs::create_dir(&nm).unwrap();
        std::fs::write(nm.join("pkg.js"), "package").unwrap();
        
        let result = read_dir_to_markdown(dir.path());
        assert!(result.contains("visible.txt"));
        assert!(!result.contains(".hidden"));
        assert!(!result.contains("node_modules"));
        assert!(!result.contains("pkg.js"));
    }

    #[test]
    fn test_expand_file_references_single_file() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("code.rs");
        std::fs::write(&file_path, "fn main() {}").unwrap();
        
        let question = "Review @code.rs please";
        let result = expand_file_references(question, dir.path());
        
        assert!(!result.contains("@code.rs"));
        assert!(result.contains("fn main() {}"));
        assert!(result.contains("## File:"));
    }

    #[test]
    fn test_expand_file_references_directory() {
        let dir = tempfile::tempdir().unwrap();
        let subdir = dir.path().join("src");
        std::fs::create_dir(&subdir).unwrap();
        std::fs::write(subdir.join("lib.rs"), "pub mod test;").unwrap();
        
        let question = "Check @src/ directory";
        let result = expand_file_references(question, dir.path());
        
        assert!(!result.contains("@src/"));
        assert!(result.contains("pub mod test;"));
    }

    #[test]
    fn test_expand_file_references_grep() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("test.rs"), "fn hello() {}\nfn world() {}").unwrap();
        
        let question = "Find @grep:hello";
        let result = expand_file_references(question, dir.path());
        
        assert!(!result.contains("@grep:hello"));
        assert!(result.contains("## Grep:"));
        // Result depends on whether rg/grep is available
    }

    #[test]
    fn test_expand_file_references_grep_with_glob() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("test.rs"), "fn rust_fn() {}").unwrap();
        std::fs::write(dir.path().join("test.py"), "def python_fn(): pass").unwrap();
        
        let question = "Search @grep:fn:*.rs";
        let result = expand_file_references(question, dir.path());
        
        assert!(!result.contains("@grep:fn:*.rs"));
        assert!(result.contains("## Grep:"));
        assert!(result.contains("(glob: *.rs)"));
    }

    #[test]
    fn test_expand_file_references_multiple() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "content a").unwrap();
        std::fs::write(dir.path().join("b.txt"), "content b").unwrap();
        
        let question = "Compare @a.txt and @b.txt";
        let result = expand_file_references(question, dir.path());
        
        assert!(!result.contains("@a.txt"));
        assert!(!result.contains("@b.txt"));
        assert!(result.contains("content a"));
        assert!(result.contains("content b"));
    }

    #[test]
    fn test_expand_file_references_no_refs() {
        let dir = tempfile::tempdir().unwrap();
        let question = "What is 2+2?";
        let result = expand_file_references(question, dir.path());
        assert_eq!(result, question);
    }

    #[test]
    fn test_expand_file_references_at_in_middle_of_word() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("test.txt"), "test").unwrap();
        
        // @ in middle of word should not be treated as reference
        let question = "email@test.txt is not a file";
        let result = expand_file_references(question, dir.path());
        
        // Should not expand because @ is not preceded by whitespace
        assert_eq!(result, question);
    }

    #[test]
    fn test_try_ripgrep_available() {
        // This test checks if ripgrep works when available
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("test.txt"), "hello world").unwrap();
        
        let result = try_ripgrep(dir.path(), "hello", None);
        // Result is Some if rg is available, None if not
        if let Some(output) = result {
            assert!(output.contains("hello") || output.is_empty());
        }
    }

    #[test]
    fn test_try_grep_available() {
        // This test checks if grep works when available
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("test.txt"), "hello world").unwrap();
        
        let result = try_grep(dir.path(), "hello", None);
        // Result is Some if grep is available, None if not
        if let Some(output) = result {
            assert!(output.contains("hello") || output.is_empty());
        }
    }

    #[test]
    fn test_run_grep_fallback() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("test.txt"), "searchable content").unwrap();
        
        let result = run_grep(dir.path(), "searchable", None);
        assert!(result.contains("## Grep:"));
        // Should either find results or report no matches/error
    }
}
