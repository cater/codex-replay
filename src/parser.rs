use anyhow::Result;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

/// 单条消息
#[derive(Debug, Clone)]
pub struct Message {
    pub role: String,
    pub content: String,
    pub timestamp: Option<DateTime<Utc>>,
}

pub struct ParsedSession {
    pub title: Option<String>,
    pub messages: Vec<Message>,
}

/// 判断一条消息是否为系统注入内容（权限指令、协作模式、技能列表、AGENTS.md 等）
pub fn is_system_message(content: &str) -> bool {
    content.contains("<INSTRUCTIONS>")
        || content.contains("<environment_context>")
        || content.contains("<permissions instructions>")
        || content.contains("<collaboration_mode>")
        || content.contains("<skills_instructions>")
        || content.starts_with("# AGENTS.md instructions")
}

/// 解析 .jsonl 文件，自动识别格式并提取所有对话消息
///
/// 支持两种格式：
/// 1. **Codex 格式** — `type: "response_item"` + `payload.type: "message"`
/// 2. **Claude 格式** — `type: "user"` / `"assistant"` (Claude Code session)
pub fn parse_jsonl_file(path: &Path) -> Result<Vec<Message>> {
    Ok(parse_session_file(path)?.messages)
}

/// 单次读取会话文件，同时提取标题与消息。
pub fn parse_session_file(path: &Path) -> Result<ParsedSession> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut messages = Vec::new();
    let mut ai_title: Option<String> = None;
    let mut first_user_prompt: Option<String> = None;

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let obj: Value = match serde_json::from_str(&line) {
            Ok(obj) => obj,
            Err(_) => continue,
        };

        match obj.get("type").and_then(|v| v.as_str()) {
            Some("ai-title") if ai_title.is_none() => {
                ai_title = obj
                    .get("aiTitle")
                    .and_then(|value| value.as_str())
                    .map(str::trim)
                    .filter(|title| !title.is_empty())
                    .map(str::to_string);
            }
            // ── Codex 格式 ──────────────────────────────
            Some("response_item") => {
                if let Some(msg) = parse_codex_message(&obj) {
                    remember_first_user_prompt(&mut first_user_prompt, &msg);
                    messages.push(msg);
                }
            }

            // ── Claude 格式 ─────────────────────────────
            Some("user") => {
                if let Some(msg) = parse_claude_user_message(&obj) {
                    remember_first_user_prompt(&mut first_user_prompt, &msg);
                    messages.push(msg);
                }
            }
            Some("assistant") => {
                if let Some(msg) = parse_claude_assistant_message(&obj) {
                    messages.push(msg);
                }
            }

            // 跳过: mode, permission-mode, file-history-snapshot,
            //       attachment, ai-title, last-prompt, system 等
            _ => {}
        }
    }
    Ok(ParsedSession {
        title: ai_title.or(first_user_prompt),
        messages,
    })
}

/// 提取会话标题。
///
/// 优先级：
/// 1. Claude/Codex 日志中的 `ai-title`
/// 2. 第一条真实用户消息
pub fn extract_session_title(path: &Path) -> Result<Option<String>> {
    Ok(parse_session_file(path)?.title)
}

fn remember_first_user_prompt(first_user_prompt: &mut Option<String>, message: &Message) {
    if first_user_prompt.is_none() && message.role == "user" && !is_system_message(&message.content)
    {
        let prompt = first_line(&message.content);
        if !prompt.is_empty() {
            *first_user_prompt = Some(prompt);
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// Codex 格式 (type == "response_item")
// ═══════════════════════════════════════════════════════════════

fn parse_codex_message(obj: &Value) -> Option<Message> {
    let payload = obj.get("payload")?.as_object()?;

    // payload.type == "message"
    if payload.get("type")?.as_str()? != "message" {
        return None;
    }

    let role = payload
        .get("role")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let content = extract_codex_content(payload);
    if content.is_empty() {
        return None;
    }

    let timestamp = extract_timestamp(obj);

    Some(Message {
        role,
        content,
        timestamp,
    })
}

/// 从 Codex payload 的 "content" 数组中提取所有 text 字段
fn extract_codex_content(payload: &serde_json::Map<String, Value>) -> String {
    if let Some(content_arr) = payload.get("content").and_then(|v| v.as_array()) {
        let mut texts = Vec::new();
        for item in content_arr {
            if let Some(obj) = item.as_object() {
                if let Some(text) = obj.get("text").and_then(|v| v.as_str()) {
                    texts.push(text.to_string());
                }
            }
        }
        texts.join("\n")
    } else {
        String::new()
    }
}

// ═══════════════════════════════════════════════════════════════
// Claude 格式 (type == "user" / "assistant")
// ═══════════════════════════════════════════════════════════════

fn parse_claude_user_message(obj: &Value) -> Option<Message> {
    // 跳过元数据消息
    if obj.get("isMeta").and_then(|v| v.as_bool()).unwrap_or(false) {
        return None;
    }

    let message = obj.get("message")?;
    let content_val = message.get("content")?;

    let content = match content_val {
        Value::String(s) => {
            // 跳过命令回显 / 系统注入
            if s.contains("<command-name>") || s.contains("<local-command-caveat>") {
                return None;
            }
            s.clone()
        }
        Value::Array(arr) => extract_claude_user_content_blocks(arr),
        _ => return None,
    };

    if content.is_empty() {
        return None;
    }

    let timestamp = extract_timestamp(obj);

    Some(Message {
        role: "user".to_string(),
        content,
        timestamp,
    })
}

/// 从 Claude user 消息的 content 数组中提取文本（处理 tool_result 等）
fn extract_claude_user_content_blocks(arr: &[Value]) -> String {
    let mut parts: Vec<String> = Vec::new();

    for item in arr {
        let obj = match item.as_object() {
            Some(o) => o,
            None => continue,
        };

        match obj.get("type").and_then(|v| v.as_str()) {
            Some("text") => {
                if let Some(text) = obj.get("text").and_then(|v| v.as_str()) {
                    parts.push(text.to_string());
                }
            }
            Some("tool_result") => {
                if let Some(inner) = obj.get("content").and_then(|v| v.as_array()) {
                    for inner_item in inner {
                        if let Some(text) = inner_item
                            .as_object()
                            .and_then(|o| o.get("text"))
                            .and_then(|v| v.as_str())
                        {
                            parts.push(text.to_string());
                        }
                    }
                }
                if let Some(text) = obj.get("content").and_then(|v| v.as_str()) {
                    parts.push(text.to_string());
                }
            }
            _ => {}
        }
    }

    parts.join("\n")
}

fn parse_claude_assistant_message(obj: &Value) -> Option<Message> {
    let message = obj.get("message")?;
    let content_arr = message.get("content")?.as_array()?;

    let mut parts: Vec<String> = Vec::new();

    for item in content_arr {
        let block = match item.as_object() {
            Some(o) => o,
            None => continue,
        };

        match block.get("type").and_then(|v| v.as_str()) {
            Some("thinking") => {
                if let Some(thinking) = block.get("thinking").and_then(|v| v.as_str()) {
                    let summary = summarize_thinking(thinking);
                    parts.push(format!("💭 Thinking:\n{}", summary));
                }
            }
            Some("text") => {
                if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                    parts.push(text.to_string());
                }
            }
            Some("tool_use") => {
                let name = block
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let tool_id = block.get("id").and_then(|v| v.as_str()).unwrap_or("?");

                let args_str = block
                    .get("input")
                    .map(|input| format_tool_input(name, input))
                    .unwrap_or_default();

                parts.push(format!("🔧 Tool: {} [{}]\n{}", name, tool_id, args_str));
            }
            _ => {}
        }
    }

    if parts.is_empty() {
        return None;
    }

    let timestamp = extract_timestamp(obj);

    Some(Message {
        role: "assistant".to_string(),
        content: parts.join("\n\n"),
        timestamp,
    })
}

// ── helpers ───────────────────────────────────────────────────

/// 截断思考内容，取前 500 字符 + 省略提示
fn summarize_thinking(thinking: &str) -> String {
    let max_len = 500;
    if thinking.len() <= max_len {
        thinking.to_string()
    } else {
        let truncated: String = thinking.chars().take(max_len).collect();
        format!("{}…\n[思考内容过长，已截断]", truncated)
    }
}

/// 格式化工具调用参数，对长字符串值做截断
fn format_tool_input(_name: &str, input: &Value) -> String {
    match input {
        Value::Object(map) => {
            let mut lines: Vec<String> = Vec::new();
            for (k, v) in map {
                let val_str = match v {
                    Value::String(s) => {
                        if s.len() > 200 {
                            let t: String = s.chars().take(200).collect();
                            format!("{}…", t)
                        } else {
                            s.clone()
                        }
                    }
                    other => other.to_string(),
                };
                lines.push(format!("  {}: {}", k, val_str));
            }
            lines.join("\n")
        }
        other => other.to_string(),
    }
}

fn extract_timestamp(obj: &Value) -> Option<DateTime<Utc>> {
    obj.get("timestamp")
        .and_then(|v| v.as_str())
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc))
}

fn first_line(s: &str) -> String {
    s.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("")
        .to_string()
}

// ── tests ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_jsonl(name: &str, content: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("test_{}_{}.jsonl", std::process::id(), name));
        let mut f = File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f.flush().unwrap();
        path
    }

    // ── Codex 格式测试 ──────────────────────────────────────

    #[test]
    fn test_codex_user_message() {
        let jsonl = r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"text","text":"Hello, can you help me?"}]},"timestamp":"2025-01-01T00:00:00Z"}"#;
        let path = temp_jsonl("codex_user", jsonl);
        let msgs = parse_jsonl_file(&path).unwrap();
        let _ = std::fs::remove_file(&path);

        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[0].content, "Hello, can you help me?");
        assert!(msgs[0].timestamp.is_some());
    }

    #[test]
    fn test_codex_assistant_message() {
        let jsonl = r#"{"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"text","text":"Sure!"},{"type":"text","text":"What do you need?"}]}}"#;
        let path = temp_jsonl("codex_assistant", jsonl);
        let msgs = parse_jsonl_file(&path).unwrap();
        let _ = std::fs::remove_file(&path);

        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].role, "assistant");
        assert_eq!(msgs[0].content, "Sure!\nWhat do you need?");
    }

    #[test]
    fn test_codex_skips_non_message() {
        let jsonl = r#"{"type":"response_item","payload":{"type":"tool_call","name":"bash"}}"#;
        let path = temp_jsonl("codex_skip", jsonl);
        let msgs = parse_jsonl_file(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert!(msgs.is_empty());
    }

    #[test]
    fn test_codex_and_claude_mixed() {
        let jsonl = r#"
{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"text","text":"codex prompt"}]}}
{"type":"user","message":{"role":"user","content":"claude prompt"},"isMeta":false}
"#;
        let path = temp_jsonl("mixed", jsonl);
        let msgs = parse_jsonl_file(&path).unwrap();
        let _ = std::fs::remove_file(&path);

        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[0].content, "codex prompt");
        assert_eq!(msgs[1].role, "user");
        assert_eq!(msgs[1].content, "claude prompt");
    }

    // ── Claude 格式测试 ────────────────────────────────────

    #[test]
    fn test_claude_user_string_content() {
        let jsonl = r#"{"type":"user","message":{"role":"user","content":"hello world"},"timestamp":"2026-06-20T02:28:22.881Z","isMeta":false}"#;
        let path = temp_jsonl("claude_user_string", jsonl);
        let msgs = parse_jsonl_file(&path).unwrap();
        let _ = std::fs::remove_file(&path);

        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[0].content, "hello world");
        assert!(msgs[0].timestamp.is_some());
    }

    #[test]
    fn test_claude_skip_meta_user() {
        let jsonl = r#"{"type":"user","message":{"role":"user","content":"<command-name>/clear</command-name>"},"isMeta":true}"#;
        let path = temp_jsonl("claude_skip_meta", jsonl);
        let msgs = parse_jsonl_file(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert!(msgs.is_empty());
    }

    #[test]
    fn test_claude_skip_system_types() {
        let jsonl = r#"
{"type":"mode","mode":"normal","sessionId":"abc"}
{"type":"file-history-snapshot","messageId":"x"}
{"type":"ai-title","aiTitle":"test"}
{"type":"user","message":{"role":"user","content":"real prompt"},"isMeta":false}
"#;
        let path = temp_jsonl("claude_skip_system", jsonl);
        let msgs = parse_jsonl_file(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].content, "real prompt");
    }

    #[test]
    fn test_claude_assistant_with_text_and_thinking() {
        let jsonl = r#"{"type":"assistant","message":{"role":"assistant","model":"deepseek-v4-pro","content":[{"type":"thinking","thinking":"I need to analyze this"},{"type":"text","text":"Here is my response"}]},"timestamp":"2026-06-20T02:28:25.641Z"}"#;
        let path = temp_jsonl("claude_assistant_text_thinking", jsonl);
        let msgs = parse_jsonl_file(&path).unwrap();
        let _ = std::fs::remove_file(&path);

        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].role, "assistant");
        assert!(msgs[0].content.contains("💭 Thinking:"));
        assert!(msgs[0].content.contains("I need to analyze this"));
        assert!(msgs[0].content.contains("Here is my response"));
    }

    #[test]
    fn test_claude_assistant_tool_use() {
        let jsonl = r#"{"type":"assistant","message":{"role":"assistant","model":"deepseek-v4-pro","content":[{"type":"tool_use","id":"call_00_abc","name":"Read","input":{"file_path":"D:\\test\\main.rs"}}]}}"#;
        let path = temp_jsonl("claude_assistant_tool", jsonl);
        let msgs = parse_jsonl_file(&path).unwrap();
        let _ = std::fs::remove_file(&path);

        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].role, "assistant");
        assert!(msgs[0].content.contains("🔧 Tool:"));
        assert!(msgs[0].content.contains("Read"));
        assert!(msgs[0].content.contains("file_path"));
    }

    #[test]
    fn test_claude_thinking_truncation() {
        let long = "x".repeat(600);
        let result = summarize_thinking(&long);
        assert!(result.len() < 600);
        assert!(result.contains("[思考内容过长，已截断]"));
    }

    #[test]
    fn test_claude_user_with_tool_result_blocks() {
        let jsonl = r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":[{"type":"text","text":"File content here"}]}]}}"#;
        let path = temp_jsonl("claude_user_tool_result", jsonl);
        let msgs = parse_jsonl_file(&path).unwrap();
        let _ = std::fs::remove_file(&path);

        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[0].content, "File content here");
    }

    #[test]
    fn test_extract_session_title_prefers_ai_title() {
        let jsonl = r#"
{"type":"user","message":{"role":"user","content":"real prompt"},"isMeta":false}
{"type":"ai-title","aiTitle":"Add directory navigation to file browser"}
"#;
        let path = temp_jsonl("title_ai_title", jsonl);
        let title = extract_session_title(&path).unwrap();
        let _ = std::fs::remove_file(&path);

        assert_eq!(
            title.as_deref(),
            Some("Add directory navigation to file browser")
        );
    }

    #[test]
    fn test_extract_session_title_falls_back_to_first_user_prompt() {
        let jsonl = r#"
{"type":"user","message":{"role":"user","content":"first prompt\nsecond line"},"isMeta":false}
{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"ok"}]}}
"#;
        let path = temp_jsonl("title_first_prompt", jsonl);
        let title = extract_session_title(&path).unwrap();
        let _ = std::fs::remove_file(&path);

        assert_eq!(title.as_deref(), Some("first prompt"));
    }

    #[test]
    fn test_extract_session_title_skips_agents_instructions() {
        let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"# AGENTS.md instructions\\n\\n<INSTRUCTIONS>\\n系统提示词\\n</INSTRUCTIONS>\"},\"isMeta\":false}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"真正的标题\"},\"isMeta\":false}";
        let path = temp_jsonl("title_skip_agents", jsonl);
        let title = extract_session_title(&path).unwrap();
        let _ = std::fs::remove_file(&path);

        assert_eq!(title.as_deref(), Some("真正的标题"));
    }
}
