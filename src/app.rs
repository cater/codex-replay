use crate::file_browser::FileBrowser;
use crate::conversation::Conversation;
use ratatui::text::Text;

#[derive(PartialEq)]
pub enum Focus {
    Left,
    Right,
}

pub struct App {
    pub files: FileBrowser,
    pub conversation: Conversation,
    pub should_quit: bool,
    pub focus: Focus,
    pub scroll_offset: usize,
    pub rendered_text: Text<'static>,
    pub total_lines: usize,
}

impl App {
    pub fn new(codex_dir: &str) -> Self {
        Self {
            files: FileBrowser::new(codex_dir),
            conversation: Conversation::default(),
            should_quit: false,
            focus: Focus::Left,
            scroll_offset: 0,
            rendered_text: Text::default(),
            total_lines: 0,
        }
    }

    pub fn load_selected(&mut self) -> anyhow::Result<()> {
        if let Some(path) = self.files.selected_path() {
            let messages = crate::parser::parse_jsonl_file(path)?;
            self.conversation.set_messages(messages);
            self.rendered_text = self.conversation.render();
            self.total_lines = self.rendered_text.height();
            self.scroll_offset = 0;
        }
        Ok(())
    }

    pub fn enter_selected_dir(&mut self) -> bool {
        self.files.enter_dir()
    }

    pub fn go_parent(&mut self) -> bool {
        self.files.go_parent()
    }

    pub fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Left => Focus::Right,
            Focus::Right => Focus::Left,
        };
    }

    pub fn scroll(&mut self, delta: isize) {
        if self.total_lines == 0 {
            return;
        }
        let max_offset = self.total_lines.saturating_sub(1);
        let new_offset = self.scroll_offset as isize + delta;
        self.scroll_offset = new_offset.clamp(0, max_offset as isize) as usize;
    }
}
