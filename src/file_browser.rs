use std::path::PathBuf;
use std::fs;

#[derive(Debug, Clone)]
pub enum Entry {
    Parent,
    Directory(PathBuf),
    File(PathBuf),
}

impl Entry {
    pub fn is_dir(&self) -> bool {
        matches!(self, Entry::Parent | Entry::Directory(_))
    }
}

pub struct FileBrowser {
    pub entries: Vec<Entry>,
    pub selected: usize,
    pub current_dir: PathBuf,
}

impl FileBrowser {
    pub fn new(dir: &str) -> Self {
        let current_dir = PathBuf::from(dir);
        let mut fb = Self {
            entries: Vec::new(),
            selected: 0,
            current_dir,
        };
        fb.refresh();
        fb
    }

    /// Rescan the current directory and rebuild the entries list.
    fn refresh(&mut self) {
        self.entries.clear();

        // ".." entry — shown only when current_dir has a parent
        if self.current_dir.parent().is_some() {
            self.entries.push(Entry::Parent);
        }

        let mut dirs: Vec<PathBuf> = Vec::new();
        let mut files: Vec<PathBuf> = Vec::new();

        if let Ok(rd) = fs::read_dir(&self.current_dir) {
            for entry in rd.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    dirs.push(path);
                } else if path.extension().map_or(false, |ext| ext == "jsonl") {
                    files.push(path);
                }
            }
        }

        // Sort alphabetically by directory/file name (case-insensitive)
        dirs.sort_by(|a, b| {
            a.file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_lowercase())
                .cmp(&b.file_name().and_then(|n| n.to_str()).map(|s| s.to_lowercase()))
        });
        files.sort_by(|a, b| {
            a.file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_lowercase())
                .cmp(&b.file_name().and_then(|n| n.to_str()).map(|s| s.to_lowercase()))
        });

        for d in dirs {
            self.entries.push(Entry::Directory(d));
        }
        for f in files {
            self.entries.push(Entry::File(f));
        }

        self.selected = 0;
    }

    pub fn selected_entry(&self) -> Option<&Entry> {
        self.entries.get(self.selected)
    }

    /// Returns the path if the selected entry is a File; None otherwise.
    pub fn selected_path(&self) -> Option<&PathBuf> {
        match self.selected_entry() {
            Some(Entry::File(p)) => Some(p),
            _ => None,
        }
    }

    pub fn is_selected_dir(&self) -> bool {
        self.selected_entry().map_or(false, |e| e.is_dir())
    }

    /// Enter the currently selected directory. Returns true on success.
    pub fn enter_dir(&mut self) -> bool {
        match self.selected_entry() {
            Some(Entry::Parent) => self.go_parent(),
            Some(Entry::Directory(p)) => {
                self.current_dir = p.clone();
                self.refresh();
                true
            }
            _ => false,
        }
    }

    /// Navigate to the parent directory. Returns true on success.
    pub fn go_parent(&mut self) -> bool {
        if let Some(parent) = self.current_dir.parent().map(|p| p.to_path_buf()) {
            self.current_dir = parent;
            self.refresh();
            true
        } else {
            false
        }
    }

    pub fn next(&mut self) {
        if !self.entries.is_empty() {
            self.selected = (self.selected + 1) % self.entries.len();
        }
    }

    pub fn previous(&mut self) {
        if !self.entries.is_empty() {
            self.selected = if self.selected == 0 {
                self.entries.len() - 1
            } else {
                self.selected - 1
            };
        }
    }
}
