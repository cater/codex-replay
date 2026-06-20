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

/// 解析 .jsonl 文件，提取所有对话消息（仅 `type == "response_item"` 且 `payload.type == "message"`）
pub fn parse_jsonl_file(path: &Path) -> Result<Vec<Message>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut messages = Vec::new();

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let obj: Value = serde_json::from_str(&line)?;

        // 只处理 type == "response_item"
        if let Some("response_item") = obj.get("type").and_then(|v| v.as_str()) {
            if let Some(payload) = obj.get("payload").and_then(|v| v.as_object()) {
                // payload.type == "message"
                if let Some("message") = payload.get("type").and_then(|v| v.as_str()) {
                    let role = payload
                        .get("role")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    let content = extract_content(payload);
                    if !content.is_empty() {
                        let timestamp = obj
                            .get("timestamp")
                            .and_then(|v| v.as_str())
                            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                            .map(|dt| dt.with_timezone(&Utc));
                        messages.push(Message {
                            role,
                            content,
                            timestamp,
                        });
                    }
                }
            }
        }
    }
    Ok(messages)
}

/// 从 payload 的 "content" 数组中提取所有 text 字段，合并为一个字符串
fn extract_content(payload: &serde_json::Map<String, Value>) -> String {
    if let Some(content_arr) = payload.get("content").and_then(|v| v.as_array()) {
        let mut texts = Vec::new();
        for item in content_arr {
            if let Some(obj) = item.as_object() {
                if let Some(text) = obj.get("text").and_then(|v| v.as_str()) {
                    texts.push(text);
                }
            }
        }
        texts.join("\n")
    } else {
        String::new()
    }
}