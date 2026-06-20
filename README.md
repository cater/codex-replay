# codex-replay

Replay and browse Codex session logs (`.jsonl`) in the terminal.  
Built with Rust + [ratatui](https://github.com/ratatui-org/ratatui).

## Features

- **File browser** — browse `.jsonl` files under `.codex`, enter subdirectories, go back to parent
- **Conversation viewer** — color-coded messages with timestamps
- **System message filter** — automatically hides injected system prompts (AGENTS.md, permissions, collaboration mode, etc.)
- **Focus indicator** — focused panel gets a cyan border; unfocused panel dims to dark gray
- **Scrollbar** — vertical scrollbar with arrow indicators for long conversations
- **Dynamic title** — conversation panel title shows the first real user prompt (truncated at 60 chars)
- **Keyboard-driven** — vi-style keys + arrows, no mouse needed

## Installation

Requires [Rust](https://rustup.rs/).

```bash
git clone <repo-url>
cd codex-replay
cargo build --release
```

The binary is at `target/release/codex-replay(.exe)`.

Or install directly:

```bash
cargo install --path .
```

## Usage

```bash
codex-replay [codex-directory]
```

If no directory is given, it checks the current directory for `.codex`, then falls back to `~/.codex`.

### Keyboard Shortcuts

| Key | Action |
|---|---|
| `Tab` / `←` / `→` | Switch focus between left and right panels |
| `↑` / `↓` / `j` / `k` | Navigate file list (left) or scroll conversation (right) |
| `Enter` | Open directory or load selected `.jsonl` file |
| `Backspace` | Go to parent directory |
| `PgUp` / `PgDn` | Page up/down (10 lines in file list, 10 lines in conversation) |
| `q` | Quit |

## UI Layout

```
┌─ Files [dirname] ───┐ ┌─ <first user prompt> ──────────────────────┬──┐
│ 📁 subdir/          │ │ ── 🧑 User [2026-06-10 10:54:17]           │↑ │
│ 📄 session-a.jsonl  │ │                                             │  │
│ 📄 session-b.jsonl  │ │ worker\comfy-zimage.py 参考这个例子...       │  │
│                     │ │                                             │  │
│                     │ │ ── 🤖 Assistant [2026-06-10 10:54:20]      │  │
│                     │ │                                             │  │
│                     │ │ 我来分析这个文件并实现 ComfyUI 调用...        │  │
│                     │ │                                             │↓ │
├─────────────────────┴──────────────────────────────────────────────┴──┤
│  [Tab]切换焦点  [↑↓]移动  [PgUp/PgDn]翻页  [Enter]进入/加载  ...      │
└─ Help ────────────────────────────────────────────────────────────────┘
```

- Focused panel border: **cyan**; unfocused: dark gray
- File selection highlight: **yellow** when focused, dimmed when unfocused
- Messages are color-coded: **User** (cyan), **Assistant** (green), **Developer** (gray)

## File Format

Parses Codex `.jsonl` session logs. Each line is one JSON object. Only `type: "response_item"` entries with `payload.type: "message"` are extracted:

```json
{
  "type": "response_item",
  "timestamp": "2026-06-10T02:54:17.948Z",
  "payload": {
    "type": "message",
    "role": "user",
    "content": [
      { "type": "input_text", "text": "Hello, World!" }
    ]
  }
}
```

## Project Structure

```
src/
├── main.rs          # Entry point, event loop, UI rendering
├── app.rs           # App state, focus management, scroll logic
├── file_browser.rs  # File list navigation and directory traversal
├── conversation.rs  # Message rendering with color, system message filter
└── parser.rs        # JSONL parsing
```

## Dependencies

| Crate | Purpose |
|---|---|
| `ratatui` | TUI rendering |
| `crossterm` | Terminal I/O |
| `serde` / `serde_json` | JSON parsing |
| `chrono` | Timestamp formatting |
| `anyhow` | Error handling |
| `dirs` | Home directory detection |
| `unicode-width` | Text width calculation |

## License

MIT
