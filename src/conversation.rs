use crate::parser::Message;
use ratatui::{
    style::{Color, Style},
    text::{Line, Span, Text},
};

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
            .find(|m| m.role == "user" && !crate::parser::is_system_message(&m.content))
            .map(|m| m.content.as_str())
    }

    /// 渲染对话为带颜色的 Text
    pub fn render(&self) -> Text<'static> {
        let mut lines: Vec<Line<'static>> = Vec::new();

        for msg in &self.messages {
            // 跳过系统注入消息
            if (msg.role == "user" || msg.role == "developer")
                && crate::parser::is_system_message(&msg.content)
            {
                continue;
            }

            let (role_label, role_color, _content_color) = match msg.role.as_str() {
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

            // 消息内容行 —— 对 assistant 消息中的特殊块做颜色区分
            if msg.role == "assistant" {
                render_assistant_content(&msg.content, &mut lines);
            } else {
                for content_line in msg.content.lines() {
                    lines.push(Line::from(Span::styled(
                        content_line.to_string(),
                        Style::new().fg(Color::Rgb(180, 240, 255)),
                    )));
                }
            }
            lines.push(Line::from(""));
        }

        Text::from(lines)
    }
}

/// 渲染 assistant 消息内容，对 thinking / tool / 普通文本使用不同颜色
fn render_assistant_content(content: &str, lines: &mut Vec<Line<'static>>) {
    // 颜色定义
    let text_color = Color::Rgb(220, 255, 220);
    let thinking_color = Color::Rgb(160, 160, 200); // 淡紫色
    let tool_color = Color::Rgb(255, 200, 100); // 暖橙色
    let dim_color = Color::Rgb(120, 120, 120);

    let mut in_thinking = false;
    let mut in_tool = false;

    for line in content.lines() {
        if line.starts_with("💭 Thinking:") {
            in_thinking = true;
            in_tool = false;
            lines.push(Line::from(Span::styled(line.to_string(), Style::new().fg(thinking_color))));
        } else if line.starts_with("🔧 Tool:") {
            in_thinking = false;
            in_tool = true;
            lines.push(Line::from(Span::styled(line.to_string(), Style::new().fg(tool_color))));
        } else if line.starts_with("[思考内容过长，已截断]") {
            lines.push(Line::from(Span::styled(line.to_string(), Style::new().fg(dim_color))));
        } else if in_thinking {
            lines.push(Line::from(Span::styled(line.to_string(), Style::new().fg(thinking_color))));
        } else if in_tool {
            // 工具参数行用 dim 色
            if line.starts_with("  ") {
                lines.push(Line::from(Span::styled(line.to_string(), Style::new().fg(dim_color))));
            } else {
                lines.push(Line::from(Span::styled(line.to_string(), Style::new().fg(tool_color))));
            }
        } else {
            lines.push(Line::from(Span::styled(line.to_string(), Style::new().fg(text_color))));
        }
    }
}
