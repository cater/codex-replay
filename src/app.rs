use crate::conversation::Conversation;
use crate::file_browser::FileBrowser;
use crate::search::{default_search_roots, search_history, SearchReport, SearchResult};
use ratatui::text::Text;
use std::path::Path;
use std::sync::mpsc::{self, Receiver};

#[derive(PartialEq)]
pub enum Focus {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchMode {
    Browsing,
    Input,
    Searching,
    Results,
}

pub struct App {
    pub files: FileBrowser,
    pub conversation: Conversation,
    pub should_quit: bool,
    pub focus: Focus,
    pub scroll_offset: usize,
    pub rendered_text: Text<'static>,
    pub total_lines: usize,
    pub search_mode: SearchMode,
    pub search_query: String,
    pub search_results: Vec<SearchResult>,
    pub search_selected: usize,
    pub search_scanned_files: usize,
    pub search_errors: Vec<String>,
    search_receiver: Option<Receiver<SearchReport>>,
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
            search_mode: SearchMode::Browsing,
            search_query: String::new(),
            search_results: Vec::new(),
            search_selected: 0,
            search_scanned_files: 0,
            search_errors: Vec::new(),
            search_receiver: None,
        }
    }

    pub fn load_selected(&mut self) -> anyhow::Result<()> {
        if let Some(path) = self.files.selected_path().cloned() {
            self.load_path(&path)?;
        }
        Ok(())
    }

    pub fn load_selected_search_result(&mut self) -> anyhow::Result<()> {
        if let Some(path) = self
            .search_results
            .get(self.search_selected)
            .map(|result| result.path.clone())
        {
            self.load_path(&path)?;
            self.focus = Focus::Right;
        }
        Ok(())
    }

    fn load_path(&mut self, path: &Path) -> anyhow::Result<()> {
        let messages = crate::parser::parse_jsonl_file(path)?;
        self.conversation.set_messages(messages);
        self.rendered_text = self.conversation.render();
        self.total_lines = self.rendered_text.height();
        self.scroll_offset = 0;
        Ok(())
    }

    pub fn begin_search_input(&mut self) {
        self.search_mode = SearchMode::Input;
        self.focus = Focus::Left;
    }

    pub fn push_search_char(&mut self, character: char) {
        self.search_query.push(character);
    }

    pub fn pop_search_char(&mut self) {
        self.search_query.pop();
    }

    pub fn submit_search(&mut self) {
        if self.search_query.trim().is_empty() {
            return;
        }

        let query = self.search_query.clone();
        let roots = default_search_roots();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(search_history(&roots, &query));
        });
        self.search_receiver = Some(receiver);
        self.search_results.clear();
        self.search_selected = 0;
        self.search_scanned_files = 0;
        self.search_errors.clear();
        self.search_mode = SearchMode::Searching;
    }

    pub fn poll_search(&mut self) {
        let Some(receiver) = &self.search_receiver else {
            return;
        };
        let Ok(report) = receiver.try_recv() else {
            return;
        };

        self.search_results = report.results;
        self.search_scanned_files = report.scanned_files;
        self.search_errors = report.errors;
        self.search_selected = 0;
        self.search_mode = SearchMode::Results;
        self.search_receiver = None;
    }

    pub fn close_search(&mut self) {
        self.search_mode = SearchMode::Browsing;
        self.search_query.clear();
        self.search_results.clear();
        self.search_selected = 0;
        self.search_scanned_files = 0;
        self.search_errors.clear();
        self.search_receiver = None;
    }

    pub fn next_search_result(&mut self) {
        if !self.search_results.is_empty() {
            self.search_selected = (self.search_selected + 1) % self.search_results.len();
        }
    }

    pub fn previous_search_result(&mut self) {
        if !self.search_results.is_empty() {
            self.search_selected = if self.search_selected == 0 {
                self.search_results.len() - 1
            } else {
                self.search_selected - 1
            };
        }
    }

    pub fn search_is_visible(&self) -> bool {
        self.search_mode != SearchMode::Browsing
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
