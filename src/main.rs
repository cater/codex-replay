mod app;
mod file_browser;
mod conversation;
mod parser;

use app::App;
use app::Focus;
use file_browser::Entry;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    widgets::{Block, Borders, List, ListItem, Paragraph, Scrollbar, ScrollbarState, Wrap},
    Frame, Terminal,
};
use crossterm::{
    event::{self, KeyCode, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use std::io::stdout;

fn main() -> anyhow::Result<()> {
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    // ----- 自动查找 .codex 目录 -----
    let args: Vec<String> = std::env::args().collect();
    let codex_dir = if args.len() > 1 {
        args[1].clone()
    } else if std::path::Path::new(".codex").exists() {
        ".codex".to_string()
    } else if let Some(home) = dirs::home_dir() {
        home.join(".codex").to_string_lossy().to_string()
    } else {
        ".codex".to_string()
    };

    let mut app = App::new(&codex_dir);

    // 如果没有任何文件，在终端打印提示（可选）
    if app.files.entries.is_empty() {
        eprintln!("提示: 未在 '{}' 中找到任何 .jsonl 文件", codex_dir);
    }

    while !app.should_quit {
        terminal.draw(|f| ui(f, &mut app))?;  // 注意：ui 需要 mutable 引用

        if let event::Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Char('q') => app.should_quit = true,

                    // 焦点切换
                    KeyCode::Tab => app.toggle_focus(),

                    // 左右方向键也可切换焦点
                    KeyCode::Left => { app.focus = Focus::Left; }
                    KeyCode::Right => { app.focus = Focus::Right; }

                    // 上下键 / j/k 根据焦点执行不同操作
                    KeyCode::Up | KeyCode::Char('k') => {
                        if app.focus == Focus::Left {
                            app.files.previous();
                        } else {
                            app.scroll(-1);
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if app.focus == Focus::Left {
                            app.files.next();
                        } else {
                            app.scroll(1);
                        }
                    }

                    KeyCode::PageDown => {
                        if app.focus == Focus::Left {
                            // 文件列表翻页（例如一次跳转 10 个）
                            for _ in 0..10 {
                                app.files.next();
                            }
                        } else {
                            // 右侧滚动半屏（例如 10 行，或动态计算）
                            app.scroll(10);  // 向下滚动 10 行
                        }
                    }
                    KeyCode::PageUp => {
                        if app.focus == Focus::Left {
                            for _ in 0..10 {
                                app.files.previous();
                            }
                        } else {
                            app.scroll(-10);
                        }
                    }

                    // Enter: 左侧焦点时，目录则进入，文件则加载
                    KeyCode::Enter => {
                        if app.focus == Focus::Left {
                            if app.files.is_selected_dir() {
                                app.enter_selected_dir();
                            } else {
                                if let Err(e) = app.load_selected() {
                                    eprintln!("加载失败: {}", e);
                                }
                            }
                        }
                    }

                    // Backspace: 左侧焦点时，返回上一级目录
                    KeyCode::Backspace => {
                        if app.focus == Focus::Left {
                            app.go_parent();
                        }
                    }

                    _ => {}
                }
            }
        }
    }

    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;
    Ok(())
}

fn ui(f: &mut Frame, app: &mut App) {
    let main_chunks = Layout::default()
    .direction(Direction::Vertical)
    .constraints([Constraint::Min(0), Constraint::Length(3)])
    .split(f.size());

    // 上部分再分为左右
    let chunks = Layout::default()
    .direction(Direction::Horizontal)
    .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
    .split(main_chunks[0]);

    // 左侧文件列表
    let dir_name = app.files.current_dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| app.files.current_dir.to_string_lossy().to_string());
    let title = format!("Files [{}]", dir_name);

    let items: Vec<ListItem> = if app.files.entries.is_empty() {
        vec![ListItem::new("(empty)")]
    } else {
        app.files.entries
            .iter()
            .map(|entry| {
                match entry {
                    Entry::Parent => ListItem::new("📁 .."),
                    Entry::Directory(p) => {
                        let name = p.file_name().unwrap_or_default().to_string_lossy();
                        ListItem::new(format!("📁 {}/", name))
                    }
                    Entry::File(p) => {
                        let name = p.file_name().unwrap_or_default().to_string_lossy();
                        ListItem::new(format!("📄 {}", name))
                    }
                }
            })
            .collect()
    };

    let (list_border_style, list_highlight) = if app.focus == Focus::Left {
        (Style::new().fg(Color::Cyan), Style::new().fg(Color::Yellow))
    } else {
        (Style::new().fg(Color::DarkGray), Style::new().fg(Color::Rgb(80, 80, 0)))
    };

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(list_border_style),
        )
        .highlight_style(list_highlight);
    let mut state = ratatui::widgets::ListState::default();
    if !app.files.entries.is_empty() {
        state.select(Some(app.files.selected));
    }
    f.render_stateful_widget(list, chunks[0], &mut state);

    // 右侧对话
    let right_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(95), Constraint::Percentage(5)])
        .split(chunks[1]);

    let paragraph = if app.conversation.is_empty() {
        Paragraph::new("Select a file and press Enter")
    } else {
        Paragraph::new(app.rendered_text.clone())
    };

    let conv_border_style = if app.focus == Focus::Right {
        Style::new().fg(Color::Cyan)
    } else {
        Style::new().fg(Color::DarkGray)
    };

    let conv_title = app
        .conversation
        .first_user_prompt()
        .map(|p| truncate_str(p, 60))
        .unwrap_or_else(|| "Conversation".to_string());

    let paragraph = paragraph
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(conv_title)
                .border_style(conv_border_style),
        )
        .wrap(Wrap { trim: false })
        .scroll((app.scroll_offset as u16, 0));
    f.render_widget(paragraph, right_chunks[0]);

    // 滚动条（只有在有内容时才显示）
    if app.total_lines > 0 {
        let mut scrollbar_state = ScrollbarState::default()
            .content_length(app.total_lines)
            .position(app.scroll_offset);
        let scrollbar = Scrollbar::default()
            .orientation(ratatui::widgets::ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("↑"))
            .end_symbol(Some("↓"));
        f.render_stateful_widget(scrollbar, right_chunks[1], &mut scrollbar_state);
    }

    // ---- 底部帮助栏 ----
    let help_text = " [Tab]切换焦点  [↑↓]移动  [PgUp/PgDn]翻页  [Enter]进入/加载  [Backspace]返回上级  [q]退出 ";
    let help_paragraph = Paragraph::new(help_text)
        .block(Block::default().borders(Borders::ALL).title("Help"))
        .alignment(ratatui::layout::Alignment::Center);
    f.render_widget(help_paragraph, main_chunks[1]);
}

/// 截断字符串，超出 max_len 时加 "…"
fn truncate_str(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_len).collect();
        format!("{}…", truncated)
    }
}
