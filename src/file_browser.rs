use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, Clone, PartialEq)]
pub enum Entry {
    Parent,
    Directory(PathBuf),
    File(PathBuf),
}

/// 文件指纹：用于标题/会话缓存的失效判断
#[derive(Debug, Clone, PartialEq)]
pub struct FileStamp {
    pub mtime: SystemTime,
    pub size: u64,
}

impl FileStamp {
    pub fn from_path(path: &Path) -> Option<Self> {
        let meta = fs::metadata(path).ok()?;
        Some(Self {
            mtime: meta.modified().ok()?,
            size: meta.len(),
        })
    }
}

impl Entry {
    pub fn is_dir(&self) -> bool {
        matches!(self, Entry::Parent | Entry::Directory(_))
    }
}

#[derive(Debug, Clone)]
pub struct FileItem {
    pub path: PathBuf,
    pub title: String,
}

pub struct FileBrowser {
    pub entries: Vec<Entry>,
    pub selected: usize,
    pub current_dir: PathBuf,
    pub file_titles: Vec<FileItem>,
    title_cache: HashMap<PathBuf, (FileStamp, Option<String>)>,
}

const TITLE_CACHE_LIMIT: usize = 4096;

impl FileBrowser {
    pub fn new(dir: &str) -> Self {
        let current_dir = PathBuf::from(dir);
        let mut fb = Self {
            entries: Vec::new(),
            selected: 0,
            current_dir,
            file_titles: Vec::new(),
            title_cache: HashMap::new(),
        };
        fb.refresh();
        fb
    }

    /// Rescan the current directory and rebuild the entries list.
    fn refresh(&mut self) {
        let prev_selected = self.selected_entry().cloned();

        self.entries.clear();
        self.file_titles.clear();

        // ".." entry — shown only when current_dir has a parent
        if self.current_dir.parent().is_some() {
            self.entries.push(Entry::Parent);
        }

        let mut dirs: Vec<PathBuf> = Vec::new();
        let mut files: Vec<PathBuf> = Vec::new();

        if let Ok(rd) = fs::read_dir(&self.current_dir) {
            for entry in rd.flatten() {
                let path = entry.path();
                let is_dir = entry
                    .file_type()
                    .map(|t| t.is_dir())
                    .unwrap_or_else(|_| path.is_dir());
                if is_dir {
                    dirs.push(path);
                } else if path.extension().is_some_and(|ext| ext == "jsonl") {
                    files.push(path);
                }
            }
        }

        // Sort alphabetically by directory/file name (case-insensitive)
        let lower_name = |p: &PathBuf| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_lowercase())
        };
        dirs.sort_by_cached_key(lower_name);
        files.sort_by_cached_key(lower_name);

        for d in dirs {
            self.entries.push(Entry::Directory(d));
        }
        for f in files {
            let title_opt = self.cached_title(&f).unwrap_or_else(|| {
                let title = crate::parser::extract_session_title_fast(&f)
                    .ok()
                    .flatten()
                    .filter(|s| !s.trim().is_empty());
                if let Some(stamp) = FileStamp::from_path(&f) {
                    if self.title_cache.len() >= TITLE_CACHE_LIMIT {
                        self.title_cache.clear();
                    }
                    self.title_cache.insert(f.clone(), (stamp, title.clone()));
                }
                title
            });
            let title = title_opt.unwrap_or_else(|| {
                f.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string()
            });
            self.file_titles.push(FileItem {
                path: f.clone(),
                title,
            });
            self.entries.push(Entry::File(f));
        }

        // 尽量保留刷新前的选中项（如返回上级后重新进入目录）
        self.selected = prev_selected
            .and_then(|entry| self.entries.iter().position(|e| *e == entry))
            .unwrap_or(0);
    }

    /// 命中缓存（mtime/size 未变）时返回已解析的标题；否则返回 None
    fn cached_title(&self, path: &Path) -> Option<Option<String>> {
        let stamp = FileStamp::from_path(path)?;
        self.title_cache
            .get(path)
            .filter(|(cached, _)| *cached == stamp)
            .map(|(_, title)| title.clone())
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
        self.selected_entry().is_some_and(|e| e.is_dir())
    }

    pub fn file_title(&self, path: &PathBuf) -> Option<&str> {
        self.file_titles
            .iter()
            .find(|item| &item.path == path)
            .map(|item| item.title.as_str())
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// 创建测试目录结构：
    /// base/
    /// ├── aaa.jsonl  (首条用户消息 "Title AAA")
    /// ├── bbb.jsonl  (ai-title "Title BBB")
    /// └── sub/
    ///     └── ccc.jsonl (首条用户消息 "Title CCC")
    fn setup_dir(tag: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!("fb_test_{}_{}", tag, std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let sub = base.join("sub");
        fs::create_dir_all(&sub).unwrap();
        let mut f1 = fs::File::create(base.join("aaa.jsonl")).unwrap();
        writeln!(
            f1,
            r#"{{"type":"user","message":{{"role":"user","content":"Title AAA"}},"isMeta":false}}"#
        )
        .unwrap();
        let mut f2 = fs::File::create(base.join("bbb.jsonl")).unwrap();
        writeln!(f2, r#"{{"type":"ai-title","aiTitle":"Title BBB"}}"#).unwrap();
        let mut f3 = fs::File::create(sub.join("ccc.jsonl")).unwrap();
        writeln!(
            f3,
            r#"{{"type":"user","message":{{"role":"user","content":"Title CCC"}},"isMeta":false}}"#
        )
        .unwrap();
        base
    }

    fn cleanup(base: &Path) {
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn test_refresh_lists_dirs_and_titles() {
        let base = setup_dir("list");
        let fb = FileBrowser::new(base.to_str().unwrap());

        assert_eq!(fb.entries[0], Entry::Parent);
        assert!(matches!(fb.entries[1], Entry::Directory(_)));
        assert!(matches!(fb.entries[2], Entry::File(_)));
        assert!(matches!(fb.entries[3], Entry::File(_)));

        let aaa = base.join("aaa.jsonl");
        let bbb = base.join("bbb.jsonl");
        assert_eq!(fb.file_title(&aaa), Some("Title AAA"));
        assert_eq!(fb.file_title(&bbb), Some("Title BBB"));
        cleanup(&base);
    }

    #[test]
    fn test_enter_dir_and_go_parent_preserves_parent_selection() {
        let base = setup_dir("nav");
        let mut fb = FileBrowser::new(base.to_str().unwrap());

        // 选中 sub 目录并进入
        let sub_idx = fb
            .entries
            .iter()
            .position(|e| matches!(e, Entry::Directory(_)))
            .unwrap();
        fb.selected = sub_idx;
        assert!(fb.enter_dir());
        assert_eq!(fb.current_dir, base.join("sub"));
        assert_eq!(fb.selected, 0);

        let ccc = base.join("sub").join("ccc.jsonl");
        assert_eq!(fb.file_title(&ccc), Some("Title CCC"));

        // 返回上级，选中项应回到 Parent（索引 0）
        assert!(fb.go_parent());
        assert_eq!(fb.current_dir, base);
        assert_eq!(fb.entries[fb.selected], Entry::Parent);
        cleanup(&base);
    }

    #[test]
    fn test_title_cache_invalidated_on_change() {
        let base = setup_dir("cache");
        let mut fb = FileBrowser::new(base.to_str().unwrap());
        let aaa = base.join("aaa.jsonl");
        assert_eq!(fb.file_title(&aaa), Some("Title AAA"));

        // 进入子目录再返回（触发 refresh，走缓存命中路径）
        let sub_idx = fb
            .entries
            .iter()
            .position(|e| matches!(e, Entry::Directory(_)))
            .unwrap();
        fb.selected = sub_idx;
        fb.enter_dir();
        fb.go_parent();
        assert_eq!(fb.file_title(&aaa), Some("Title AAA"));

        // 修改文件（追加 ai-title，size 变化），缓存应失效并重新提取
        let mut f = fs::OpenOptions::new().append(true).open(&aaa).unwrap();
        writeln!(f, r#"{{"type":"ai-title","aiTitle":"Updated Title"}}"#).unwrap();
        drop(f);
        let sub_idx = fb
            .entries
            .iter()
            .position(|e| matches!(e, Entry::Directory(_)))
            .unwrap();
        fb.selected = sub_idx;
        fb.enter_dir();
        fb.go_parent();
        assert_eq!(fb.file_title(&aaa), Some("Updated Title"));
        cleanup(&base);
    }
}
