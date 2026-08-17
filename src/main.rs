mod app;
mod conversation;
mod file_browser;
mod parser;
mod search;

use app::App;
use app::Focus;
use app::SearchMode;
use crossterm::{
    event::{self, KeyCode, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use file_browser::Entry;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    widgets::{Block, Borders, List, ListItem, Paragraph, Scrollbar, ScrollbarState, Wrap},
    Frame, Terminal,
};
use std::io::stdout;
use std::time::Duration;

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
        app.poll_search();
        terminal.draw(|f| ui(f, &mut app))?; // 注意：ui 需要 mutable 引用

        if event::poll(Duration::from_millis(100))? {
            if let event::Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    if app.search_mode == SearchMode::Input {
                        match key.code {
                            KeyCode::Esc => app.close_search(),
                            KeyCode::Enter => app.submit_search(),
                            KeyCode::Backspace => app.pop_search_char(),
                            KeyCode::Char(character) => app.push_search_char(character),
                            _ => {}
                        }
                        continue;
                    }

                    match key.code {
                        KeyCode::Char('q') => app.should_quit = true,
                        KeyCode::Char('/') => app.begin_search_input(),
                        KeyCode::Esc if app.search_is_visible() => app.close_search(),

                        // 焦点切换
                        KeyCode::Tab => app.toggle_focus(),

                        // 左右方向键也可切换焦点
                        KeyCode::Left => {
                            app.focus = Focus::Left;
                        }
                        KeyCode::Right => {
                            app.focus = Focus::Right;
                        }

                        // 上下键 / j/k 根据焦点执行不同操作
                        KeyCode::Up | KeyCode::Char('k') => {
                            if app.search_mode == SearchMode::Results && app.focus == Focus::Left {
                                app.previous_search_result();
                            } else if app.focus == Focus::Left {
                                app.files.previous();
                            } else {
                                app.scroll(-1);
                            }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if app.search_mode == SearchMode::Results && app.focus == Focus::Left {
                                app.next_search_result();
                            } else if app.focus == Focus::Left {
                                app.files.next();
                            } else {
                                app.scroll(1);
                            }
                        }

                        KeyCode::PageDown => {
                            if app.focus == Focus::Left {
                                for _ in 0..10 {
                                    if app.search_mode == SearchMode::Results {
                                        app.next_search_result();
                                    } else {
                                        app.files.next();
                                    }
                                }
                            } else {
                                // 右侧滚动半屏（例如 10 行，或动态计算）
                                app.scroll(10); // 向下滚动 10 行
                            }
                        }
                        KeyCode::PageUp => {
                            if app.focus == Focus::Left {
                                for _ in 0..10 {
                                    if app.search_mode == SearchMode::Results {
                                        app.previous_search_result();
                                    } else {
                                        app.files.previous();
                                    }
                                }
                            } else {
                                app.scroll(-10);
                            }
                        }

                        // Enter: 左侧焦点时，目录则进入，文件则加载
                        KeyCode::Enter if app.focus == Focus::Left => {
                            if app.search_mode == SearchMode::Results {
                                if let Err(e) = app.load_selected_search_result() {
                                    eprintln!("加载失败: {}", e);
                                }
                            } else if app.files.is_selected_dir() {
                                app.enter_selected_dir();
                            } else {
                                if let Err(e) = app.load_selected() {
                                    eprintln!("加载失败: {}", e);
                                }
                            }
                        }

                        // Backspace: 左侧焦点时，返回上一级目录
                        KeyCode::Backspace
                            if app.focus == Focus::Left
                                && app.search_mode == SearchMode::Browsing =>
                        {
                            app.go_parent();
                        }

                        _ => {}
                    }
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
    let dir_name = app
        .files
        .current_dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| app.files.current_dir.to_string_lossy().to_string());
    let title = match app.search_mode {
        SearchMode::Input => format!("Search: {}█", app.search_query),
        SearchMode::Searching => format!("Searching: {}", app.search_query),
        SearchMode::Results => format!(
            "Search: {} ({} results)",
            app.search_query,
            app.search_results.len()
        ),
        SearchMode::Browsing => format!("Files [{}]", dir_name),
    };

    let items: Vec<ListItem> = if app.search_mode == SearchMode::Searching {
        vec![ListItem::new("正在搜索 Codex 与 Claude 历史记录...")]
    } else if app.search_mode == SearchMode::Input {
        vec![ListItem::new("输入关键词后按 Enter 搜索")]
    } else if app.search_mode == SearchMode::Results {
        if app.search_results.is_empty() {
            vec![ListItem::new(format!(
                "未找到匹配项（扫描 {} 个文件）",
                app.search_scanned_files
            ))]
        } else {
            app.search_results
                .iter()
                .map(|result| {
                    ListItem::new(format!(
                        "[{}] {}  · {}处命中\n  {}",
                        result.source.label(),
                        truncate_str(&result.title, 44),
                        result.match_count,
                        truncate_str(&result.snippet, 52)
                    ))
                })
                .collect()
        }
    } else if app.files.entries.is_empty() {
        vec![ListItem::new("(empty)")]
    } else {
        app.files
            .entries
            .iter()
            .map(|entry| match entry {
                Entry::Parent => ListItem::new("📁 .."),
                Entry::Directory(p) => {
                    let name = p.file_name().unwrap_or_default().to_string_lossy();
                    ListItem::new(format!("📁 {}/", name))
                }
                Entry::File(p) => {
                    let title = app
                        .files
                        .file_title(p)
                        .map(|s| truncate_str(s, 50))
                        .unwrap_or_else(|| {
                            p.file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string()
                        });
                    ListItem::new(format!("📄 {}", title))
                }
            })
            .collect()
    };

    let (list_border_style, list_highlight) = if app.focus == Focus::Left {
        (Style::new().fg(Color::Cyan), Style::new().fg(Color::Yellow))
    } else {
        (
            Style::new().fg(Color::DarkGray),
            Style::new().fg(Color::Rgb(80, 80, 0)),
        )
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
    if app.search_mode == SearchMode::Results && !app.search_results.is_empty() {
        state.select(Some(app.search_selected));
    } else if app.search_mode == SearchMode::Browsing && !app.files.entries.is_empty() {
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
    let help_text = match app.search_mode {
        SearchMode::Input => " 输入关键词  [Enter]搜索  [Backspace]删除  [Esc]取消 ",
        SearchMode::Searching => " 正在搜索 ~/.codex 与 ~/.claude  [Esc]返回浏览 ",
        SearchMode::Results => " [↑↓]选择  [Enter]打开  [Tab]切换焦点  [/]重新搜索  [Esc]返回浏览 ",
        SearchMode::Browsing => {
            " [/]搜索  [Tab]切换焦点  [↑↓]移动  [Enter]进入/加载  [Backspace]返回  [q]退出 "
        }
    };
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
