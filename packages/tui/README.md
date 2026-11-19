# @ananke/tui — Ananke Terminal Client

Official TypeScript Terminal User Interface (TUI) for the Ananke Agent Runtime, following **AD-12 (Headless Architecture)** with **Differential Terminal Rendering** to eliminate screen flickering and preserve terminal scrollback history.

---

## 1. Key Features

- **Differential Screen Rendering**: Calculates diff between display frames and outputs updates atomically using Synchronized Output escapes (`CSI ? 2026 h/l`).
- **Scrollback Preservation**: Unlike full-screen alternative buffer TUIs, history logs scroll natively within your terminal scrollback.
- **Visual Width Accuracy**: Proper measurement of full-width CJK characters and Emoji (2 visual columns).
- **Self-Wrapping Layout**: The renderer word-wraps every line and splits multi-line text before diffing, so each frame buffer entry is exactly one physical terminal row — long answers no longer drift or re-print previous lines.
- **Collapsible Thinking Blocks**: Streaming `reasoning` deltas render as a `Thinking…` block with elapsed time; toggle it with `Ctrl+T` or by clicking the block.
- **Unicode / IME Friendly Editor**: Accepts multi-character chunks from Vietnamese IMEs (UniKey, EVKey) and other input methods, preserving case and precomposed Unicode.
- **Interactive Multi-line Editor**: Arrow key navigation, history traversal, `Shift+Enter` for multiline breaks, `Enter` to submit, and bracketed paste.
- **Dynamic Streaming Markdown**: Real-time markdown parsing with code block formatting, headers, lists, and inline styles, applied to both in-flight and completed messages.
- **Modal Permission Dialog**: Prominent prompt on `permission.requested` events (`Allow` / `Deny` / `Always`).
- **Slash Commands**: `/help`, `/clear`, `/interrupt`, `/model <name>`, `/perm <mode>`, `/exit`.

---

## 2. Requirements

- Runtime: [Bun](https://bun.sh/) (recommended) or Node.js >= 18.
- Ananke Server running at `http://127.0.0.1:9876` (`cargo run -p ananke-server`).

---

## 3. Quick Start

### Install Dependencies
```bash
cd packages/tui
bun install
```

### Run Tests
```bash
bun test
```

### Build Distribution
```bash
bun run build
```

### Launch TUI
```bash
bun run start
```
*Or configure custom server endpoint:*
```bash
ANANKE_SERVER_URL=http://127.0.0.1:9876 bun run start
```

---

## 4. Slash Commands Guide

| Command | Action |
|---|---|
| `/help` | Display interactive help menu and keyboard shortcuts |
| `/clear` | Clear local messages log and reset render buffer |
| `/interrupt` | Stop currently running LLM response or tool execution |
| `/model <name>` | Switch session model dynamically (e.g. `/model default`) |
| `/perm <mode>` | Update session permission mode (`ReadOnly`, `WorkspaceWrite`, `DangerFullAccess`) |
| `/exit` (or `Ctrl+D` / `Ctrl+Q`) | Terminate session and restore terminal raw mode safely |

---

## 5. Keyboard Shortcuts

| Key | Action |
|---|---|
| `Enter` | Submit the message |
| `Shift+Enter` / `Alt+Enter` | Insert a newline |
| `Ctrl+T` / left click on the block | Expand or collapse the latest `Thinking…` block |
| `Up` / `Down` | Move the cursor between lines, or browse input history at the edges |
| `Left` / `Right` / `Home` / `End` | Move the cursor |
| `Ctrl+D` / `Ctrl+Q` | Exit the TUI |
| `Ctrl+C` | Leaves the TUI running (kept as terminal copy) |

---

## 6. Architecture Notes

Following **AD-12**, `@ananke/tui` is completely headless and communicates with `ananke-server` strictly via:
- **HTTP REST**: Session lifecycle, turns creation, interrupts, model updates, and permission responses.
- **SSE Stream**: Real-time event consumption (`part.delta`, `turn.started`, `turn.completed`, `permission.requested`) with auto-reconnection via `Last-Event-ID`.
