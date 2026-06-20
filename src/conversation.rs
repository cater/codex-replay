use crate::parser::Message;
use ratatui::{
    style::{Color, Style},
    text::{Line, Span, Text},
};

/// 判断一条消息是否为系统注入内容（权限指令、协作模式、技能列表、AGENTS.md 等）
fn is_system_message(content: &str) -> bool {
    content.contains("<INSTRUCTIONS>")
        || content.contains("<environment_context>")
        || content.contains("<permissions instructions>")
        || content.contains("<collaboration_mode>")
        || content.contains("<skills_instructions>")
        || content.starts_with("# AGENTS.md instructions for")
}

#[derive(Default)]
pub struct Conversation {
    pub messages: Vec<Message>,
}

impl Conversation {
    pub fn set_messages(&mut self, msgs: Vec<Message>) {
        self.messages = msgs;
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    /// 获取第一条真正的用户提示词（跳过系统注入消息），用于面板标题
    pub fn first_user_prompt(&self) -> Option<&str> {
        self.messages
            .iter()
            .find(|m| m.role == "user" && !is_system_message(&m.content))
            .map(|m| m.content.as_str())
    }

    /// 渲染对话为带颜色的 Text
    pub fn render(&self) -> Text<'static> {
        let mut lines: Vec<Line<'static>> = Vec::new();

        for msg in &self.messages {
            // 跳过系统注入消息（developer 和 user 都可能包含）
            if (msg.role == "user" || msg.role == "developer") && is_system_message(&msg.content) {
                continue;
            }

            let (role_label, role_color, content_color) = match msg.role.as_str() {
                "user" => ("🧑 User", Color::Rgb(0, 255, 255), Color::Rgb(180, 240, 255)),
                "assistant" => ("🤖 Assistant", Color::Rgb(100, 255, 100), Color::Rgb(220, 255, 220)),
                "developer" => ("⚙️ Developer", Color::Rgb(180, 180, 180), Color::Rgb(200, 200, 200)),
                _ => (msg.role.as_str(), Color::White, Color::White),
            };

            let time_str = msg
                .timestamp
                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_default();

            // 角色头行
            let mut header = format!("── {role_label}");
            if !time_str.is_empty() {
                header.push_str(&format!(" [{time_str}]"));
            }
            lines.push(Line::from(Span::styled(header, Style::new().fg(role_color))));
            lines.push(Line::from(""));

            // 消息内容行
            for content_line in msg.content.lines() {
                lines.push(Line::from(Span::styled(
                    content_line.to_string(),
                    Style::new().fg(content_color),
                )));
            }
            lines.push(Line::from(""));
        }

        Text::from(lines)
    }
}