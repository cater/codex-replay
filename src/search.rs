use crate::parser::{is_system_message, parse_session_file};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistorySource {
    Codex,
    Claude,
}

impl HistorySource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::Claude => "Claude",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SearchRoot {
    pub path: PathBuf,
    pub source: HistorySource,
}

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub path: PathBuf,
    pub source: HistorySource,
    pub title: String,
    pub snippet: String,
    pub match_count: usize,
}

#[derive(Debug, Default)]
pub struct SearchReport {
    pub results: Vec<SearchResult>,
    pub scanned_files: usize,
    pub errors: Vec<String>,
}

pub fn default_search_roots() -> Vec<SearchRoot> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };

    vec![
        SearchRoot {
            path: home.join(".codex"),
            source: HistorySource::Codex,
        },
        SearchRoot {
            path: home.join(".claude"),
            source: HistorySource::Claude,
        },
    ]
}

pub fn search_history(roots: &[SearchRoot], query: &str) -> SearchReport {
    let query = query.trim().to_lowercase();
    let mut report = SearchReport::default();
    if query.is_empty() {
        return report;
    }

    for root in roots {
        collect_files(root, &query, &mut report);
    }

    report.results.sort_by(|left, right| {
        left.title
            .to_lowercase()
            .cmp(&right.title.to_lowercase())
            .then_with(|| left.path.cmp(&right.path))
    });
    report
}

fn collect_files(root: &SearchRoot, query: &str, report: &mut SearchReport) {
    if !root.path.is_dir() {
        return;
    }

    let mut pending = vec![root.path.clone()];
    while let Some(directory) = pending.pop() {
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) => {
                report
                    .errors
                    .push(format!("{}: {}", directory.display(), error));
                continue;
            }
        };

        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    report
                        .errors
                        .push(format!("{}: {}", directory.display(), error));
                    continue;
                }
            };
            let path = entry.path();
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(error) => {
                    report.errors.push(format!("{}: {}", path.display(), error));
                    continue;
                }
            };

            if file_type.is_dir() {
                pending.push(path);
            } else if file_type.is_file()
                && path
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("jsonl"))
            {
                report.scanned_files += 1;
                match search_file(&path, root.source, query) {
                    Ok(Some(result)) => report.results.push(result),
                    Ok(None) => {}
                    Err(error) => report.errors.push(format!("{}: {}", path.display(), error)),
                }
            }
        }
    }
}

fn search_file(
    path: &Path,
    source: HistorySource,
    query: &str,
) -> anyhow::Result<Option<SearchResult>> {
    let session = parse_session_file(path)?;
    let title = session
        .title
        .filter(|title| !title.trim().is_empty())
        .unwrap_or_else(|| {
            path.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        });

    let mut match_count = count_matches(&title, query);
    let mut snippet = if match_count > 0 {
        title.clone()
    } else {
        String::new()
    };

    for message in session.messages {
        if (message.role == "user" || message.role == "developer")
            && is_system_message(&message.content)
        {
            continue;
        }
        let matches = count_matches(&message.content, query);
        if matches == 0 {
            continue;
        }
        match_count += matches;
        if snippet.is_empty() {
            snippet = matching_line(&message.content, query);
        }
    }

    if match_count == 0 {
        return Ok(None);
    }

    Ok(Some(SearchResult {
        path: path.to_path_buf(),
        source,
        title,
        snippet: truncate(&snippet, 100),
        match_count,
    }))
}

fn count_matches(text: &str, query: &str) -> usize {
    text.to_lowercase().match_indices(query).count()
}

fn matching_line(text: &str, query: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|line| line.to_lowercase().contains(query))
        .unwrap_or("")
        .to_string()
}

fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        text.to_string()
    } else {
        format!("{}…", text.chars().take(max_chars).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_root() -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "codex_replay_search_{}_{}",
            std::process::id(),
            suffix
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn write_session(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut file = fs::File::create(path).unwrap();
        file.write_all(content.as_bytes()).unwrap();
    }

    #[test]
    fn searches_codex_and_claude_recursively() {
        let root = test_root();
        let codex = root.join(".codex");
        let claude = root.join(".claude");
        write_session(
            &codex.join("sessions/one.jsonl"),
            r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"text","text":"Rust 搜索功能"}]}}"#,
        );
        write_session(
            &claude.join("projects/two.jsonl"),
            r#"{"type":"user","message":{"role":"user","content":"请实现 rust 搜索"},"isMeta":false}"#,
        );
        let roots = vec![
            SearchRoot {
                path: codex,
                source: HistorySource::Codex,
            },
            SearchRoot {
                path: claude,
                source: HistorySource::Claude,
            },
        ];

        let report = search_history(&roots, "RUST");

        assert_eq!(report.scanned_files, 2);
        assert_eq!(report.results.len(), 2);
        assert!(report
            .results
            .iter()
            .any(|result| result.source == HistorySource::Codex));
        assert!(report
            .results
            .iter()
            .any(|result| result.source == HistorySource::Claude));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn excludes_injected_system_messages() {
        let root = test_root();
        let session = root.join("system.jsonl");
        write_session(
            &session,
            r##"{"type":"user","message":{"role":"user","content":"# AGENTS.md instructions\nsecret-keyword"},"isMeta":false}"##,
        );
        let roots = vec![SearchRoot {
            path: root.clone(),
            source: HistorySource::Claude,
        }];

        let report = search_history(&roots, "secret-keyword");

        assert!(report.results.is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn skips_broken_lines_and_keeps_valid_messages() {
        let root = test_root();
        let session = root.join("broken.jsonl");
        write_session(
            &session,
            "not-json\n{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"仍然可以搜索\"},\"isMeta\":false}",
        );
        let roots = vec![SearchRoot {
            path: root.clone(),
            source: HistorySource::Claude,
        }];

        let report = search_history(&roots, "可以搜索");

        assert_eq!(report.results.len(), 1);
        assert!(report.errors.is_empty());
        fs::remove_dir_all(root).unwrap();
    }
}
