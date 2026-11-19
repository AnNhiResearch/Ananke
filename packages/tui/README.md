# @ananke/tui

Official TypeScript TUI client for Ananke agent runtime, implemented according to **AD-12** and **AD-14**.

## Features

- **Decoupled Architecture**: Talks to `ananke-server` purely over HTTP REST + SSE with `X-Ananke-Protocol: 1`.
- **Live Streaming**: Real-time token streaming (`part.delta`).
- **Interactive Permissions**: Directly prompts `(y/N)` on terminal when `permission.requested` events occur for mutating tools.
- **Runtime Discovery**: Displays sandbox capability (`none`, `restricted-token`, `seatbelt`, `landlock`) and hardware device profile from `/api/meta`.
- **Turn Control**: Interrupt running turns via `/interrupt`.

## Quick Start

### 1. Start the Ananke Server

```bash
cargo run -p ananke-server
```

### 2. Run the TUI Client

Using Bun:
```bash
bun run src/index.ts
```

Or using Node:
```bash
npm run build
node dist/index.js
```

### Environment Variables

- `ANANKE_SERVER_URL`: URL of the Ananke server (default: `http://127.0.0.1:9876`).
